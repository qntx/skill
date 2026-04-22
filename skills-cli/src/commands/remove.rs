//! `skills remove` command implementation.
//!
//! Matches the TS `remove.ts` UX: uses cliclack prompts for interactive
//! selection and confirmation, but plain output for results. Shows
//! per-skill error details on failure.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use clap::Args;
use miette::{IntoDiagnostic, Result, miette};
use skill::SkillManager;
use skill::installer::canonical_skills_dir;
use skill::types::{AgentId, InstallScope, RemoveOptions};

use crate::ui::emit;
use crate::ui::{self, DIM, GREEN, RED, RESET, YELLOW};

/// Arguments for the `remove` command.
#[derive(Args)]
pub(crate) struct RemoveArgs {
    /// Skill names to remove (interactive selection if omitted).
    pub skills: Vec<String>,

    /// Remove from global scope.
    #[arg(short, long)]
    pub global: bool,

    /// Remove from specific agents (use '*' for all).
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Remove specific skills by name (use '*' for all).
    #[arg(short, long, num_args = 1..)]
    pub skill: Option<Vec<String>>,

    /// Skip confirmation prompts.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Shorthand for --skill '*' --agent '*' -y.
    #[arg(long)]
    pub all: bool,
}

#[allow(clippy::excessive_nesting, reason = "scope × agent × entry iteration")]
async fn scan_installed_skills(
    manager: &SkillManager,
    scope: InstallScope,
    global: bool,
    cwd: &Path,
) -> Vec<String> {
    let canonical = canonical_skills_dir(scope, cwd);
    let mut installed_names: HashSet<String> = HashSet::new();

    if let Ok(mut entries) = tokio::fs::read_dir(&canonical).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(ft) = entry.file_type().await
                && (ft.is_dir() || ft.is_symlink())
            {
                installed_names.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }

    for config in manager.agents().all_configs() {
        let dir = if global {
            config.global_skills_dir.clone()
        } else {
            Some(cwd.join(&config.skills_dir))
        };
        if let Some(dir) = dir
            && let Ok(mut entries) = tokio::fs::read_dir(&dir).await
        {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(ft) = entry.file_type().await
                    && (ft.is_dir() || ft.is_symlink())
                {
                    installed_names.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }

    let mut installed: Vec<String> = installed_names.into_iter().collect();
    installed.sort();
    installed
}

/// Validate agent names against registry before proceeding.
fn validate_agents(manager: &SkillManager, agent_names: &[String]) -> Result<Vec<AgentId>> {
    let all_ids = manager.agents().all_ids();
    let mut result = Vec::new();
    for name in agent_names {
        if name == "*" {
            return Ok(all_ids);
        }
        let id = AgentId::new(name);
        if all_ids.contains(&id) {
            result.push(id);
        } else {
            return Err(miette!(
                "Unknown agent: \"{name}\". Available agents: {}",
                all_ids
                    .iter()
                    .map(AgentId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(result)
}

/// Run the remove command.
pub(crate) async fn run(mut args: RemoveArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let cwd = std::env::current_dir().into_diagnostic()?;

    merge_skill_flags(&mut args);

    let target_agents: Vec<AgentId> = if let Some(ref agent_names) = args.agent {
        validate_agents(&manager, agent_names)?
    } else {
        manager.agents().all_ids()
    };

    let spinner = cliclack::spinner();
    spinner.start("Scanning for installed skills...");
    let installed = scan_installed_skills(&manager, scope, args.global, &cwd).await;
    spinner.stop(format!(
        "Found {} unique installed skill(s)",
        installed.len()
    ));

    if installed.is_empty() {
        emit::outro_cancel(format!("{YELLOW}No skills found to remove.{RESET}"));
        return Ok(());
    }

    let Some(selected) = resolve_selection(&args, &installed) else {
        emit::outro_cancel("Removal cancelled");
        return Ok(());
    };

    if selected.is_empty() {
        println!("{DIM}No matching skills found.{RESET}");
        return Ok(());
    }

    if !confirm_removal(&selected, &args)? {
        emit::outro_cancel("Removal cancelled");
        return Ok(());
    }

    let remove_spinner = cliclack::spinner();
    remove_spinner.start("Removing skills...");

    let agents_for_telemetry = target_agents.clone();
    manager
        .remove_skills(
            &selected,
            &RemoveOptions {
                scope,
                agents: target_agents,
                cwd: Some(cwd.clone()),
            },
        )
        .await
        .map_err(|e| miette!("{e}"))?;

    remove_spinner.stop("Removal process complete");

    emit::success(format!(
        "{GREEN}Successfully removed {} skill(s){RESET}",
        selected.len()
    ));

    cleanup_lock_entries(&selected, args.global, &cwd).await;
    send_remove_telemetry(&selected, &agents_for_telemetry, args.global).await;

    println!();
    emit::outro(format!("{GREEN}Done!{RESET}"));
    Ok(())
}

/// Fold `-s/--skill` flag values into the positional `skills` list.
fn merge_skill_flags(args: &mut RemoveArgs) {
    if let Some(ref skill_names) = args.skill {
        for name in skill_names {
            if !args.skills.contains(name) {
                args.skills.push(name.clone());
            }
        }
    }
}

/// Resolve which skills to remove from flags + optional interactive prompt.
///
/// Returns `None` when the user cancels the multiselect prompt.
fn resolve_selection(args: &RemoveArgs, installed: &[String]) -> Option<Vec<String>> {
    if args.all {
        return Some(installed.to_vec());
    }

    if !args.skills.is_empty() {
        if args.skills.contains(&"*".to_owned()) {
            return Some(installed.to_vec());
        }
        let names_lower: Vec<String> = args.skills.iter().map(|s| s.to_lowercase()).collect();
        return Some(
            installed
                .iter()
                .filter(|s| names_lower.contains(&s.to_lowercase()))
                .cloned()
                .collect(),
        );
    }

    let mut prompt = cliclack::multiselect(format!(
        "Select skills to remove {DIM}(space to toggle){RESET}"
    ));
    for s in installed {
        prompt = prompt.item(s.clone(), s, "");
    }
    prompt = prompt.required(true);
    ui::drain_input_events();
    prompt.interact().ok()
}

/// Confirm removal with the user unless `--yes` or `--all` was supplied.
fn confirm_removal(selected: &[String], args: &RemoveArgs) -> Result<bool> {
    if args.yes || args.all {
        return Ok(true);
    }

    println!();
    emit::info("Skills to remove:");
    for s in selected {
        emit::remark(format!(" {RED}•{RESET} {s}"));
    }
    println!();

    ui::drain_input_events();
    cliclack::confirm(format!(
        "Are you sure you want to uninstall {} skill(s)?",
        selected.len()
    ))
    .initial_value(false)
    .interact()
    .into_diagnostic()
}

/// Remove entries from the appropriate lock file after successful uninstall.
async fn cleanup_lock_entries(selected: &[String], global: bool, cwd: &Path) {
    if global {
        for name in selected {
            let _ = skill::lock::remove_skill_from_lock(name).await;
        }
    } else {
        for name in selected {
            let _ = skill::local_lock::remove_skill_from_local_lock(name, cwd).await;
        }
    }
}

async fn send_remove_telemetry(skills: &[String], agents: &[AgentId], global: bool) {
    let lock = skill::lock::read_skill_lock().await.ok();

    // Group removed skills by source for telemetry.
    let mut by_source: HashMap<String, Vec<String>> = HashMap::new();
    for name in skills {
        let source = lock
            .as_ref()
            .and_then(|l| l.skills.get(name))
            .map_or_else(|| "unknown".to_owned(), |e| e.source.clone());
        by_source.entry(source).or_default().push(name.clone());
    }

    for (source, names) in &by_source {
        let mut props = HashMap::new();
        props.insert("source".to_owned(), source.clone());
        props.insert("skills".to_owned(), names.join(","));
        props.insert(
            "agents".to_owned(),
            agents
                .iter()
                .map(|a| a.as_str().to_owned())
                .collect::<Vec<_>>()
                .join(","),
        );
        if global {
            props.insert("global".to_owned(), "1".to_owned());
        }
        skill::telemetry::track("remove", props);
    }
}

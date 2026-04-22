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
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "interactive removal flow with multiple branches"
)]
pub(crate) async fn run(mut args: RemoveArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Merge --skill flag values into positional skills list (matches TS -s flag).
    if let Some(ref skill_names) = args.skill {
        for name in skill_names {
            if !args.skills.contains(name) {
                args.skills.push(name.clone());
            }
        }
    }

    // Validate agent names early
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

    #[allow(
        clippy::option_if_let_else,
        clippy::single_match_else,
        reason = "sequential conditions read clearer than match"
    )]
    let selected: Vec<String> = if args.all {
        installed.clone()
    } else if !args.skills.is_empty() {
        // Handle wildcard: '*' selects all (matches TS).
        if args.skills.contains(&"*".to_owned()) {
            installed.clone()
        } else {
            let names_lower: Vec<String> = args.skills.iter().map(|s| s.to_lowercase()).collect();
            installed
                .iter()
                .filter(|s| names_lower.contains(&s.to_lowercase()))
                .cloned()
                .collect()
        }
    } else {
        let mut prompt = cliclack::multiselect(format!(
            "Select skills to remove {DIM}(space to toggle){RESET}"
        ));
        for s in &installed {
            prompt = prompt.item(s.clone(), s, "");
        }
        prompt = prompt.required(true);
        ui::drain_input_events();
        match prompt.interact() {
            Ok(sel) => sel,
            Err(_) => {
                emit::outro_cancel("Removal cancelled");
                return Ok(());
            }
        }
    };

    if selected.is_empty() {
        println!("{DIM}No matching skills found.{RESET}");
        return Ok(());
    }

    if !args.yes && !args.all {
        println!();
        emit::info("Skills to remove:");
        for s in &selected {
            emit::remark(format!(" {RED}•{RESET} {s}"));
        }
        println!();

        ui::drain_input_events();
        let confirmed: bool = cliclack::confirm(format!(
            "Are you sure you want to uninstall {} skill(s)?",
            selected.len()
        ))
        .initial_value(false)
        .interact()
        .into_diagnostic()?;

        if !confirmed {
            emit::outro_cancel("Removal cancelled");
            return Ok(());
        }
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

    // Clean up lock files: global lock for --global, local lock for project scope.
    if args.global {
        for name in &selected {
            let _ = skill::lock::remove_skill_from_lock(name).await;
        }
    } else {
        for name in &selected {
            let _ = skill::local_lock::remove_skill_from_local_lock(name, &cwd).await;
        }
    }

    // Telemetry (matches TS remove.ts: group by source).
    send_remove_telemetry(&selected, &agents_for_telemetry, args.global).await;

    println!();
    emit::outro(format!("{GREEN}Done!{RESET}"));
    Ok(())
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

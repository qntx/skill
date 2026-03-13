//! `skills remove` command implementation.
//!
//! Matches the TS `remove.ts` UX: uses cliclack prompts for interactive
//! selection and confirmation, but plain output for results. Shows
//! per-skill error details on failure.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result, miette};

use skill::SkillManager;
use skill::installer::canonical_skills_dir;
use skill::types::{AgentId, InstallScope, RemoveOptions};

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

/// Arguments for the `remove` command.
#[derive(Args)]
pub struct RemoveArgs {
    /// Skill names to remove (interactive selection if omitted).
    pub skills: Vec<String>,

    /// Remove from global scope.
    #[arg(short, long)]
    pub global: bool,

    /// Remove from specific agents (use '*' for all).
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Skip confirmation prompts.
    #[arg(short, long)]
    pub yes: bool,

    /// Shorthand for --skill '*' --agent '*' -y.
    #[arg(long)]
    pub all: bool,
}

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
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(result)
}

/// Run the remove command.
pub async fn run(args: RemoveArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Validate agent names early
    let target_agents: Vec<AgentId> = if let Some(ref agent_names) = args.agent {
        validate_agents(&manager, agent_names)?
    } else {
        manager.agents().all_ids()
    };

    let installed = scan_installed_skills(&manager, scope, args.global, &cwd).await;

    if installed.is_empty() {
        println!("{DIM}No skills found to remove.{RESET}");
        return Ok(());
    }

    let selected: Vec<String> = if args.all {
        installed.clone()
    } else if !args.skills.is_empty() {
        let names_lower: Vec<String> = args.skills.iter().map(|s| s.to_lowercase()).collect();
        installed
            .iter()
            .filter(|s| names_lower.contains(&s.to_lowercase()))
            .cloned()
            .collect()
    } else {
        let mut prompt = cliclack::multiselect("Select skills to remove");
        for s in &installed {
            prompt = prompt.item(s.clone(), s, "");
        }
        prompt = prompt.required(true);
        match prompt.interact() {
            Ok(sel) => sel,
            Err(_) => {
                println!("{DIM}Removal cancelled{RESET}");
                std::process::exit(0);
            }
        }
    };

    if selected.is_empty() {
        println!("{DIM}No matching skills found.{RESET}");
        return Ok(());
    }

    if !args.yes && !args.all {
        println!("{TEXT}Skills to remove:{RESET}");
        for s in &selected {
            println!("  {} {s}", style("•").red());
        }
        println!();

        let confirmed: bool = cliclack::confirm(format!(
            "Are you sure you want to uninstall {} skill(s)?",
            selected.len()
        ))
        .initial_value(false)
        .interact()
        .into_diagnostic()?;

        if !confirmed {
            println!("{DIM}Removal cancelled{RESET}");
            std::process::exit(0);
        }
    }

    println!("{TEXT}Removing skills...{RESET}");

    let results = manager
        .remove_skills(
            &selected,
            &RemoveOptions {
                scope,
                agents: target_agents,
                cwd: Some(cwd),
            },
        )
        .await
        .map_err(|e| miette!("{e}"))?;

    let success_count = results.iter().filter(|r| r.success).count();
    let fail_count = results.iter().filter(|r| !r.success).count();

    println!();
    if success_count > 0 {
        println!("{TEXT}✓ Removed {success_count} skill(s){RESET}");
    }
    if fail_count > 0 {
        println!("{DIM}✗ Failed to remove {fail_count} skill(s){RESET}");
        for r in &results {
            if !r.success {
                if let Some(ref err) = r.error {
                    println!("  {DIM}{}: {err}{RESET}", r.skill);
                }
            }
        }
    }

    if args.global {
        for name in &selected {
            let _ = skill::lock::remove_skill_from_lock(name).await;
        }
    }

    // Telemetry (matches TS remove.ts: group by source).
    send_remove_telemetry(&selected, args.global).await;

    println!();
    Ok(())
}

async fn send_remove_telemetry(skills: &[String], global: bool) {
    let lock = skill::lock::read_skill_lock().await.ok();

    // Group removed skills by source for telemetry.
    let mut by_source: HashMap<String, Vec<String>> = HashMap::new();
    for name in skills {
        let source = lock
            .as_ref()
            .and_then(|l| l.skills.get(name))
            .map(|e| e.source.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        by_source.entry(source).or_default().push(name.clone());
    }

    for (source, names) in &by_source {
        let mut props = HashMap::new();
        props.insert("source".to_owned(), source.clone());
        props.insert("skills".to_owned(), names.join(","));
        if global {
            props.insert("global".to_owned(), "1".to_owned());
        }
        skill::telemetry::track("remove", props);
    }
}

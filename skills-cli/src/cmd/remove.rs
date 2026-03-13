//! `skills remove` command implementation.

use std::collections::HashSet;
use std::path::Path;

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result, miette};

use skill::SkillManager;
use skill::installer::canonical_skills_dir;
use skill::types::{AgentId, InstallScope, RemoveOptions};

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

/// Run the remove command.
pub async fn run(args: RemoveArgs) -> Result<()> {
    cliclack::intro(style(" skills remove ").on_cyan().black()).into_diagnostic()?;

    let manager = SkillManager::builder().build();
    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let cwd = std::env::current_dir().into_diagnostic()?;
    let installed = scan_installed_skills(&manager, scope, args.global, &cwd).await;

    if installed.is_empty() {
        cliclack::outro("No skills found to remove.").into_diagnostic()?;
        return Ok(());
    }

    cliclack::log::info(format!("Found {} installed skill(s)", installed.len()))
        .into_diagnostic()?;

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
        prompt.interact().into_diagnostic()?
    };

    if selected.is_empty() {
        cliclack::outro(style("No matching skills found").dim()).into_diagnostic()?;
        return Ok(());
    }

    if !args.yes && !args.all {
        cliclack::note("Skills to remove", selected.join("\n")).into_diagnostic()?;

        let confirmed: bool = cliclack::confirm(format!(
            "Are you sure you want to uninstall {} skill(s)?",
            selected.len()
        ))
        .initial_value(false)
        .interact()
        .into_diagnostic()?;

        if !confirmed {
            cliclack::outro(style("Removal cancelled").dim()).into_diagnostic()?;
            return Ok(());
        }
    }

    let target_agents: Vec<AgentId> = args.agent.as_ref().map_or_else(
        || manager.agents().all_ids(),
        |agent_names| {
            if agent_names.contains(&"*".to_owned()) {
                manager.agents().all_ids()
            } else {
                agent_names.iter().map(AgentId::new).collect()
            }
        },
    );

    let spinner = cliclack::spinner();
    spinner.start("Removing skills...");

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
    spinner.stop("Removal complete");

    if success_count > 0 {
        cliclack::log::success(format!("Removed {success_count} skill(s)")).into_diagnostic()?;
    }
    if fail_count > 0 {
        cliclack::log::error(format!("Failed to remove {fail_count} skill(s)"))
            .into_diagnostic()?;
    }

    if args.global {
        for name in &selected {
            let _ = skill::lock::remove_skill_from_lock(name).await;
        }
    }

    cliclack::outro(style("Done!").green()).into_diagnostic()?;
    Ok(())
}

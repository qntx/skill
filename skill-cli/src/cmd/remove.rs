//! `skills remove` command implementation.

use std::collections::HashSet;

use clap::Args;
use console::style;
use dialoguer::{Confirm, MultiSelect};
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

/// Run the remove command.
pub async fn run(args: RemoveArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Scan for installed skill directories
    let canonical = canonical_skills_dir(scope, &cwd);
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

    // Also scan agent-specific directories
    for config in manager.agents().all_configs() {
        let dir = if args.global {
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

    if installed.is_empty() {
        println!("  {}", style("No skills found to remove.").yellow());
        return Ok(());
    }

    println!(
        "  Found {} installed skill(s)",
        style(installed.len()).green()
    );

    // Select skills to remove
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
        let selections = MultiSelect::new()
            .with_prompt("Select skills to remove (space to toggle)")
            .items(&installed)
            .interact()
            .into_diagnostic()?;

        if selections.is_empty() {
            println!("  {}", style("No skills selected").dim());
            return Ok(());
        }

        selections.iter().map(|&i| installed[i].clone()).collect()
    };

    if selected.is_empty() {
        return Err(miette!(
            "No matching skills found for: {}",
            args.skills.join(", ")
        ));
    }

    // Confirmation
    if !args.yes && !args.all {
        println!();
        println!("  Skills to remove:");
        for s in &selected {
            println!("    {} {s}", style("•").red());
        }
        println!();

        let confirmed = Confirm::new()
            .with_prompt(format!(
                "Are you sure you want to uninstall {} skill(s)?",
                selected.len()
            ))
            .default(false)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            println!("  {}", style("Removal cancelled").dim());
            return Ok(());
        }
    }

    let target_agents: Vec<AgentId> = if let Some(ref agent_names) = args.agent {
        if agent_names.contains(&"*".to_owned()) {
            manager.agents().all_ids()
        } else {
            agent_names.iter().map(AgentId::new).collect()
        }
    } else {
        manager.agents().all_ids()
    };

    let remove_opts = RemoveOptions {
        scope,
        agents: target_agents,
        cwd: Some(cwd),
    };

    let results = manager
        .remove_skills(&selected, &remove_opts)
        .await
        .map_err(|e| miette!("{e}"))?;

    let success_count = results.iter().filter(|r| r.success).count();
    let fail_count = results.iter().filter(|r| !r.success).count();

    if success_count > 0 {
        println!(
            "  {} Successfully removed {} skill(s)",
            style("✓").green(),
            success_count
        );
    }
    if fail_count > 0 {
        eprintln!(
            "  {} Failed to remove {} skill(s)",
            style("✗").red(),
            fail_count
        );
    }

    // Remove from global lock
    if args.global {
        for name in &selected {
            let _ = skill::lock::remove_skill_from_lock(name).await;
        }
    }

    println!();
    println!("  {}", style("Done!").green());

    Ok(())
}

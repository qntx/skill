//! `skills experimental_sync` command implementation.
//!
//! Syncs skills from `node_modules` into agent directories.

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result};

use skill::SkillManager;
use skill::skills::discover_skills;
use skill::types::{AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallScope, Skill};

/// Arguments for the `experimental_sync` command.
#[derive(Args)]
pub struct SyncArgs {
    /// Target agents (use '*' for all).
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Skip confirmation prompts.
    #[arg(short, long)]
    pub yes: bool,
}

async fn scan_node_modules(
    node_modules: &std::path::Path,
    discover_opts: &DiscoverOptions,
) -> Vec<Skill> {
    let mut skills = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(node_modules).await else {
        return skills;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if name_str.starts_with('@') {
            let scope_dir = entry.path();
            if let Ok(mut scoped) = tokio::fs::read_dir(&scope_dir).await {
                while let Ok(Some(pkg)) = scoped.next_entry().await {
                    let pkg_path = pkg.path();
                    if let Ok(found) = discover_skills(&pkg_path, None, discover_opts).await {
                        skills.extend(found);
                    }
                }
            }
        } else {
            let pkg_path = entry.path();
            if let Ok(found) = discover_skills(&pkg_path, None, discover_opts).await {
                skills.extend(found);
            }
        }
    }

    skills
}

/// Run the `experimental_sync` command.
pub async fn run(args: SyncArgs) -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let node_modules = cwd.join("node_modules");

    if !node_modules.exists() {
        println!("  {}", style("No node_modules directory found.").yellow());
        return Ok(());
    }

    let manager = SkillManager::builder().build();

    println!("  Scanning node_modules for skills...");
    let discover_opts = DiscoverOptions::default();
    let all_skills = scan_node_modules(&node_modules, &discover_opts).await;

    if all_skills.is_empty() {
        println!("  {}", style("No skills found in node_modules.").dim());
        return Ok(());
    }

    println!(
        "  Found {} skill(s) in node_modules",
        style(all_skills.len()).green()
    );

    for s in &all_skills {
        println!("    {} - {}", style(&s.name).cyan(), s.description);
    }

    if !args.yes {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Install these skills?")
            .default(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            println!("  {}", style("Sync cancelled").dim());
            return Ok(());
        }
    }

    let target_agents: Vec<AgentId> = if let Some(ref names) = args.agent {
        if names.contains(&"*".to_owned()) {
            manager.agents().all_ids()
        } else {
            names.iter().map(AgentId::new).collect()
        }
    } else {
        let detected = manager.detect_installed_agents().await;
        if detected.is_empty() {
            manager.agents().all_ids()
        } else {
            let mut agents = detected;
            for ua in manager.agents().universal_agents() {
                if !agents.contains(&ua) {
                    agents.push(ua);
                }
            }
            agents
        }
    };

    let opts = InstallOptions {
        scope: InstallScope::Project,
        mode: InstallMode::Symlink,
        cwd: Some(cwd),
    };

    for skill_item in &all_skills {
        for agent_id in &target_agents {
            let _ = manager.install_skill(skill_item, agent_id, &opts).await;
        }
    }

    println!();
    println!(
        "  {} Synced {} skill(s)",
        style("✓").green(),
        all_skills.len()
    );
    println!();

    Ok(())
}

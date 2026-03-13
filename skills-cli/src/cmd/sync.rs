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
    cliclack::intro(style(" skills sync ").on_cyan().black()).into_diagnostic()?;

    let cwd = std::env::current_dir().into_diagnostic()?;
    let node_modules = cwd.join("node_modules");

    if !node_modules.exists() {
        cliclack::outro("No node_modules directory found.").into_diagnostic()?;
        return Ok(());
    }

    let manager = SkillManager::builder().build();

    let spinner = cliclack::spinner();
    spinner.start("Scanning node_modules for skills...");
    let discover_opts = DiscoverOptions::default();
    let all_skills = scan_node_modules(&node_modules, &discover_opts).await;

    if all_skills.is_empty() {
        spinner.stop("No skills found in node_modules.");
        cliclack::outro("Done").into_diagnostic()?;
        return Ok(());
    }

    spinner.stop(format!("Found {} skill(s)", all_skills.len()));

    cliclack::note("Skills found", {
        use std::fmt::Write;
        let mut body = String::new();
        for s in &all_skills {
            let _ = writeln!(body, "{} - {}", s.name, s.description);
        }
        body
    })
    .into_diagnostic()?;

    if !args.yes {
        let confirmed: bool = cliclack::confirm("Install these skills?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            cliclack::outro(style("Sync cancelled").dim()).into_diagnostic()?;
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

    let spinner = cliclack::spinner();
    spinner.start("Syncing skills...");

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

    spinner.stop(format!("Synced {} skill(s)", all_skills.len()));
    cliclack::outro(style("Done!").green()).into_diagnostic()?;
    Ok(())
}

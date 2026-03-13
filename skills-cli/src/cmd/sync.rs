//! `skills experimental_sync` command implementation.
//!
//! Syncs skills from `node_modules` into agent directories.
//! Uses plain console output to match TS style.

use clap::Args;
use miette::{IntoDiagnostic, Result};

use skill::SkillManager;
use skill::skills::discover_skills;
use skill::types::{AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallScope, Skill};

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

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
        println!("{DIM}No node_modules directory found.{RESET}");
        return Ok(());
    }

    let manager = SkillManager::builder().build();

    println!("{TEXT}Scanning node_modules for skills...{RESET}");
    let discover_opts = DiscoverOptions::default();
    let all_skills = scan_node_modules(&node_modules, &discover_opts).await;

    if all_skills.is_empty() {
        println!("{DIM}No skills found in node_modules.{RESET}");
        return Ok(());
    }

    println!("{TEXT}Found {} skill(s):{RESET}", all_skills.len());
    println!();
    for s in &all_skills {
        println!("  {TEXT}{}{RESET} {DIM}- {}{RESET}", s.name, s.description);
    }
    println!();

    if !args.yes {
        let confirmed: bool = cliclack::confirm("Install these skills?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            println!("{DIM}Sync cancelled{RESET}");
            std::process::exit(0);
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

    println!("{TEXT}Syncing skills...{RESET}");

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

    println!("{TEXT}✓ Synced {} skill(s){RESET}", all_skills.len());
    println!();
    Ok(())
}

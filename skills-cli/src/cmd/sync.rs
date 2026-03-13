//! `skills experimental_sync` command implementation.
//!
//! Syncs skills from `node_modules` into agent directories.  Matches TS
//! `sync.ts`: computes skill folder hashes, checks local lock for
//! up-to-date skills, uses search-multiselect for agent selection, and
//! updates the local lock file after installation.

use std::collections::HashMap;

use clap::Args;
use miette::{IntoDiagnostic, Result};

use skill::SkillManager;
use skill::local_lock::{self, LocalSkillLockEntry};
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

/// Filter out skills that are already up-to-date in the local lock file.
async fn filter_outdated(skills: Vec<Skill>, cwd: &std::path::Path) -> (Vec<Skill>, usize) {
    let lock =
        local_lock::read_local_lock(cwd)
            .await
            .unwrap_or_else(|_| local_lock::LocalSkillLockFile {
                version: 1,
                skills: Default::default(),
            });

    let mut outdated = Vec::new();
    let mut up_to_date = 0usize;

    for skill_item in skills {
        let current_hash = local_lock::compute_skill_folder_hash(&skill_item.path)
            .await
            .unwrap_or_default();

        if let Some(entry) = lock.skills.get(&skill_item.name) {
            if entry.computed_hash == current_hash && !current_hash.is_empty() {
                up_to_date += 1;
                continue;
            }
        }

        outdated.push(skill_item);
    }

    (outdated, up_to_date)
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

    // Check local lock for up-to-date skills (matches TS sync.ts).
    let (skills_to_sync, up_to_date) = filter_outdated(all_skills, &cwd).await;

    if up_to_date > 0 {
        println!("{DIM}{up_to_date} skill(s) already up to date{RESET}");
    }

    if skills_to_sync.is_empty() {
        println!("{TEXT}✓ All skills are up to date{RESET}");
        println!();
        return Ok(());
    }

    println!(
        "{TEXT}Found {} skill(s) to sync:{RESET}",
        skills_to_sync.len()
    );
    println!();
    for s in &skills_to_sync {
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

    // Agent selection: use search-multiselect for interactive, matching TS.
    let target_agents: Vec<AgentId> =
        super::add::select_agents(&manager, args.agent.as_ref(), args.yes).await?;

    println!("{TEXT}Syncing skills...{RESET}");

    let opts = InstallOptions {
        scope: InstallScope::Project,
        mode: InstallMode::Symlink,
        cwd: Some(cwd.clone()),
    };

    let mut success = 0u32;
    let mut failed = 0u32;

    for skill_item in &skills_to_sync {
        let mut any_ok = false;
        for agent_id in &target_agents {
            match manager.install_skill(skill_item, agent_id, &opts).await {
                Ok(r) if r.success => any_ok = true,
                _ => {}
            }
        }

        if any_ok {
            success += 1;

            // Update local lock with new hash.
            let hash = local_lock::compute_skill_folder_hash(&skill_item.path)
                .await
                .unwrap_or_default();

            // Derive npm package source from skill path relative to node_modules.
            let source = skill_item
                .path
                .strip_prefix(&cwd.join("node_modules"))
                .ok()
                .and_then(|rel| rel.components().next())
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();

            let _ = local_lock::add_skill_to_local_lock(
                &skill_item.name,
                LocalSkillLockEntry {
                    source,
                    source_type: "node_modules".to_owned(),
                    computed_hash: hash,
                },
                &cwd,
            )
            .await;
        } else {
            failed += 1;
        }
    }

    println!();
    if success > 0 {
        println!("{TEXT}✓ Synced {success} skill(s){RESET}");
    }
    if failed > 0 {
        println!("{DIM}✗ Failed to sync {failed} skill(s){RESET}");
    }

    // Telemetry.
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), skills_to_sync.len().to_string());
    props.insert("successCount".to_owned(), success.to_string());
    props.insert("source".to_owned(), "node_modules".to_owned());
    skill::telemetry::track("sync", props);

    println!();
    Ok(())
}

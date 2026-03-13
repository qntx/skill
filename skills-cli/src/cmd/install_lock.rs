//! `skills experimental_install` command implementation.
//!
//! Restores skills from a project `skills-lock.json`.
//! Groups skills by source and calls internal `run_add` — matches TS
//! `install.ts` which calls `runAdd` and `runSync` internally rather
//! than spawning subprocesses.

use std::collections::BTreeMap;

use miette::{IntoDiagnostic, Result};

use skill::SkillManager;

use super::add::RunAddOptions;

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

/// Run the `experimental_install` command.
pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock_path = cwd.join("skills-lock.json");

    if !lock_path.exists() {
        println!("{DIM}No skills-lock.json found.{RESET}");
        println!(
            "{DIM}Install skills with{RESET} {TEXT}skills add <package>{RESET} {DIM}to create one.{RESET}"
        );
        return Ok(());
    }

    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        println!("{DIM}No skills in lock file.{RESET}");
        return Ok(());
    }

    // Only install to universal agents (matches TS install.ts: getUniversalAgents()).
    let manager = SkillManager::builder().build();
    let universal_agent_names: Vec<String> = manager
        .agents()
        .universal_agents()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();

    println!(
        "{TEXT}Restoring {} skill(s) from skills-lock.json{RESET}",
        lock.skills.len()
    );
    println!();

    // Group skills by source (matches TS: groups by source before calling runAdd).
    let mut by_source: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut node_modules_skills: Vec<String> = Vec::new();

    for (name, entry) in &lock.skills {
        if entry.source_type == "node_modules" {
            node_modules_skills.push(name.clone());
        } else {
            by_source
                .entry(entry.source.clone())
                .or_default()
                .push(name.clone());
        }
    }

    let mut success = 0usize;
    let mut failed = 0usize;

    // Install remote skills grouped by source via internal run_add.
    // TS installs only to universal agents for project-level lock installs.
    for (source, skill_names) in &by_source {
        println!("{DIM}Installing from {source}...{RESET}");
        let result = super::add::run_add(RunAddOptions {
            source: source.clone(),
            global: false,
            yes: true,
            skill_filter: Some(skill_names.clone()),
            agent: Some(universal_agent_names.clone()),
        })
        .await;

        match result {
            Ok(()) => {
                success += skill_names.len();
                for name in skill_names {
                    println!("  {TEXT}✓{RESET} {name}");
                }
            }
            Err(e) => {
                failed += skill_names.len();
                for name in skill_names {
                    println!("  {DIM}✗ {name}{RESET}");
                }
                tracing::warn!(source = %source, error = %e, "install from lock failed");
            }
        }
    }

    // node_modules skills: run experimental_sync instead.
    if !node_modules_skills.is_empty() {
        println!(
            "{DIM}Syncing {} node_modules skill(s)...{RESET}",
            node_modules_skills.len()
        );
        let sync_args = super::sync::SyncArgs {
            agent: None,
            yes: true,
            force: false,
        };
        if let Err(e) = super::sync::run(sync_args).await {
            tracing::warn!(error = %e, "node_modules sync during install failed");
            failed += node_modules_skills.len();
        } else {
            success += node_modules_skills.len();
        }
    }

    println!();
    if success > 0 {
        println!("{TEXT}✓ Restored {success} skill(s){RESET}");
    }
    if failed > 0 {
        println!("{DIM}✗ Failed to restore {failed} skill(s){RESET}");
    }
    println!();

    Ok(())
}

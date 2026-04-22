//! Project skills update pass.
//!
//! Reads `skills-lock.json` from the current working directory and re-runs
//! `skills add` (at project scope) for each entry that matches the optional
//! name filter.

use miette::{IntoDiagnostic, Result};
use skill::local_lock::LocalSkillLockEntry;
use skill::sanitize::sanitize_metadata;

use super::source_builder::build_project_source;
use super::stats::{ScopeStats, matches_skill_filter};
use crate::commands::add::{RunAddOptions, run_add};
use crate::ui::{DIM, RESET, TEXT};

/// Refresh project-level skills recorded in `skills-lock.json`.
pub(super) async fn update(filter: &[String]) -> Result<ScopeStats> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    let targets: Vec<(String, LocalSkillLockEntry)> = lock
        .skills
        .iter()
        .filter(|(name, entry)| {
            matches_skill_filter(name, filter)
                // node_modules and local path skills are managed out-of-band
                // (by experimental_sync / direct edits).
                && entry.source_type != "node_modules"
                && entry.source_type != "local"
        })
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect();

    if targets.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No project skills to update.{RESET}");
            println!("{DIM}Install project skills with{RESET} {TEXT}skills add <package>{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    println!(
        "{TEXT}Refreshing {} project skill(s)...{RESET}",
        targets.len()
    );
    println!();

    let mut stats = ScopeStats {
        found: targets.len().try_into().unwrap_or(u32::MAX),
        ..ScopeStats::default()
    };

    for (name, entry) in &targets {
        let safe_name = sanitize_metadata(name);
        println!("{TEXT}Updating {safe_name}...{RESET}");
        let install_url = build_project_source(entry);
        match run_add(RunAddOptions {
            source: install_url,
            global: Some(false),
            yes: true,
            skill_filter: Some(vec![name.clone()]),
            agent: None,
            dry_run: false,
        })
        .await
        {
            Ok(()) => {
                stats.success += 1;
                println!("  {TEXT}\u{2713}{RESET} Updated {safe_name}");
            }
            Err(e) => {
                stats.fail += 1;
                println!("  {DIM}\u{2717} Failed to update {safe_name}{RESET}");
                tracing::warn!(skill = %name, error = %e, "project update failed");
            }
        }
    }

    Ok(stats)
}

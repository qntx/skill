//! Global skills update pass.
//!
//! Reads the global lock, probes each entry's tree SHA, and reinstalls
//! anything whose remote hash differs from the recorded one via `run_add`.

use miette::Result;
use skill::lock::SkillLockEntry;
use skill::sanitize::sanitize_metadata;

use super::source_builder::build_global_source;
use super::stats::{ScopeStats, matches_skill_filter};
use crate::commands::add::{RunAddOptions, run_add};
use crate::commands::{SkippedSkill, get_skip_reason, print_skipped_skills, should_skip};
use crate::ui::{DIM, RESET, TEXT};

/// Refresh global skills that differ from their latest upstream hash.
pub(super) async fn update(filter: &[String]) -> Result<ScopeStats> {
    let lock = skill::lock::read_skill_lock()
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No global skills tracked in lock file.{RESET}");
            println!("{DIM}Install skills with{RESET} {TEXT}skills add <package> -g{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    let token = skill::github::get_token();
    let mut skipped: Vec<SkippedSkill> = Vec::new();
    let mut checkable: Vec<(&String, &SkillLockEntry)> = Vec::new();

    for (name, entry) in &lock.skills {
        if !matches_skill_filter(name, filter) {
            continue;
        }
        if should_skip(entry) {
            skipped.push(SkippedSkill {
                name: name.clone(),
                reason: get_skip_reason(entry),
                source_url: entry.source_url.clone(),
                source_type: entry.source_type.clone(),
                git_ref: entry.git_ref.clone(),
            });
            continue;
        }
        checkable.push((name, entry));
    }

    // Probe latest tree SHA for each checkable skill.
    let mut updates: Vec<(String, SkillLockEntry)> = Vec::new();
    for (idx, (name, entry)) in checkable.iter().enumerate() {
        print!(
            "\r{DIM}Checking global skill {}/{}: {}{RESET}\x1b[K",
            idx + 1,
            checkable.len(),
            sanitize_metadata(name),
        );
        let skill_path = entry.skill_path.as_deref().unwrap_or_default();
        let latest = skill::github::fetch_skill_folder_hash(
            &entry.source,
            skill_path,
            token.as_deref(),
            entry.git_ref.as_deref(),
        )
        .await
        .unwrap_or(None);

        if let Some(hash) = latest
            && hash != entry.skill_folder_hash
        {
            updates.push(((*name).clone(), (*entry).clone()));
        }
    }
    if !checkable.is_empty() {
        print!("\r\x1b[K");
    }

    let checked_count = checkable.len() + skipped.len();

    if checkable.is_empty() && skipped.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No global skills to check.{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    if checkable.is_empty() && !skipped.is_empty() {
        print_skipped_skills(&skipped);
        return Ok(ScopeStats {
            success: 0,
            fail: 0,
            found: checked_count.try_into().unwrap_or(u32::MAX),
        });
    }

    if updates.is_empty() {
        println!("{TEXT}\u{2713} All global skills are up to date{RESET}");
        print_skipped_skills(&skipped);
        return Ok(ScopeStats {
            success: 0,
            fail: 0,
            found: checked_count.try_into().unwrap_or(u32::MAX),
        });
    }

    println!("{TEXT}Found {} global update(s){RESET}", updates.len());
    println!();

    let mut stats = ScopeStats {
        found: checked_count.try_into().unwrap_or(u32::MAX),
        ..ScopeStats::default()
    };

    for (name, entry) in &updates {
        let safe_name = sanitize_metadata(name);
        println!("{TEXT}Updating {safe_name}...{RESET}");
        let install_url = build_global_source(entry);
        match run_add(RunAddOptions {
            source: install_url,
            global: Some(true),
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
                tracing::warn!(skill = %name, error = %e, "update failed");
            }
        }
    }

    print_skipped_skills(&skipped);
    Ok(stats)
}

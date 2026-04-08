//! `skills update` command implementation.
//!
//! Matches the TS `cli.ts` `runUpdate` UX: plain console output with ANSI
//! colors, skipped skills handling.  Uses internal `run_add` instead of
//! spawning a subprocess for cross-platform reliability.

use std::collections::HashMap;

use miette::Result;

use super::add::RunAddOptions;
use super::{SkippedSkill, get_skip_reason, print_skipped_skills, should_skip};
use crate::ui::{DIM, RESET, TEXT};

struct UpdateEntry {
    name: String,
    source_url: String,
    skill_path: Option<String>,
}

/// Build the install URL from `source_url` + `skill_path` (matches TS logic).
fn build_install_url(source_url: &str, skill_path: Option<&str>) -> String {
    let Some(sp) = skill_path else {
        return source_url.to_owned();
    };

    let mut folder = sp.to_owned();
    if folder.ends_with("/SKILL.md") {
        folder.truncate(folder.len() - 9);
    } else if folder.ends_with("SKILL.md") {
        folder.truncate(folder.len() - 8);
    }
    let folder = folder.trim_end_matches('/');

    if folder.is_empty() {
        return source_url.to_owned();
    }

    let base = source_url.trim_end_matches(".git").trim_end_matches('/');
    format!("{base}/tree/main/{folder}")
}

/// Run the update command.
pub(crate) async fn run() -> Result<()> {
    println!("{TEXT}Checking for skill updates...{RESET}");
    println!();

    let lock = skill::lock::read_skill_lock()
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        println!("{DIM}No skills tracked in lock file.{RESET}");
        println!("{DIM}Install skills with{RESET} {TEXT}skills add <package>{RESET}");
        return Ok(());
    }

    let token = skill::lock::get_github_token();
    let mut updates: Vec<UpdateEntry> = Vec::new();
    let mut skipped: Vec<SkippedSkill> = Vec::new();

    for (name, entry) in &lock.skills {
        if should_skip(entry) {
            skipped.push(SkippedSkill {
                name: name.clone(),
                reason: get_skip_reason(entry),
                source_url: entry.source_url.clone(),
            });
            continue;
        }

        let skill_path = entry.skill_path.as_deref().unwrap_or_default();
        if let Ok(Some(latest)) =
            skill::lock::fetch_skill_folder_hash(&entry.source, skill_path, token.as_deref()).await
            && latest != entry.skill_folder_hash
        {
            updates.push(UpdateEntry {
                name: name.clone(),
                source_url: entry.source_url.clone(),
                skill_path: entry.skill_path.clone(),
            });
        }
    }

    let checked_count = lock.skills.len() - skipped.len();

    if checked_count == 0 {
        println!("{DIM}No skills to check.{RESET}");
        print_skipped_skills(&skipped);
        return Ok(());
    }

    if updates.is_empty() {
        println!("{TEXT}✓ All skills are up to date{RESET}");
        println!();
        return Ok(());
    }

    println!("{TEXT}Found {} update(s){RESET}", updates.len());
    println!();

    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    for update in &updates {
        println!("{TEXT}Updating {}...{RESET}", update.name);

        let install_url = build_install_url(&update.source_url, update.skill_path.as_deref());

        let result = super::add::run_add(RunAddOptions {
            source: install_url,
            global: Some(true),
            yes: true,
            skill_filter: Some(vec![update.name.clone()]),
            agent: None,
            dry_run: false,
        })
        .await;

        match result {
            Ok(()) => {
                success_count += 1;
                println!("  {TEXT}✓{RESET} Updated {}", update.name);
            }
            Err(e) => {
                fail_count += 1;
                println!("  {DIM}✗ Failed to update {}{RESET}", update.name);
                tracing::warn!(skill = %update.name, error = %e, "update failed");
            }
        }
    }

    println!();
    if success_count > 0 {
        println!("{TEXT}✓ Updated {success_count} skill(s){RESET}");
    }
    if fail_count > 0 {
        println!("{DIM}Failed to update {fail_count} skill(s){RESET}");
    }

    print_skipped_skills(&skipped);

    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), updates.len().to_string());
    props.insert("successCount".to_owned(), success_count.to_string());
    props.insert("failCount".to_owned(), fail_count.to_string());
    skill::telemetry::track("update", props);

    println!();
    Ok(())
}

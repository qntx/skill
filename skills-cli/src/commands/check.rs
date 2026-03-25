//! `skills check` command implementation.
//!
//! Matches the TS `cli.ts` `runCheck` UX: plain console output with ANSI
//! colors, skipped skills with reasons and manual update commands.

use std::collections::HashMap;

use miette::Result;

use crate::ui::{DIM, RESET, TEXT};

use super::{SkippedSkill, get_skip_reason, print_skipped_skills, should_skip};

/// Run the check command.
pub async fn run() -> Result<()> {
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
    let mut updates: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<SkippedSkill> = Vec::new();
    let mut errors: Vec<(String, String, String)> = Vec::new();

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
        match skill::lock::fetch_skill_folder_hash(&entry.source, skill_path, token.as_deref())
            .await
        {
            Ok(Some(latest)) if latest != entry.skill_folder_hash => {
                updates.push((name.clone(), entry.source.clone()));
            }
            Err(e) => {
                errors.push((name.clone(), entry.source.clone(), format!("{e}")));
            }
            _ => {}
        }
    }

    let total_checked = lock.skills.len() - skipped.len();
    if total_checked == 0 {
        println!("{DIM}No GitHub skills to check.{RESET}");
        print_skipped_skills(&skipped);
        return Ok(());
    }

    println!("{DIM}Checking {total_checked} skill(s) for updates...{RESET}");
    println!();

    if updates.is_empty() {
        println!("{TEXT}✓ All skills are up to date{RESET}");
    } else {
        println!("{TEXT}{} update(s) available:{RESET}", updates.len());
        println!();
        for (name, source) in &updates {
            println!("  {TEXT}↑{RESET} {name}");
            println!("    {DIM}source: {source}{RESET}");
        }
        println!();
        println!("{DIM}Run{RESET} {TEXT}skills update{RESET} {DIM}to update all skills{RESET}");
    }

    if !errors.is_empty() {
        println!();
        println!(
            "{DIM}Could not check {} skill(s) (may need reinstall){RESET}",
            errors.len()
        );
    }

    print_skipped_skills(&skipped);

    // Telemetry
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), total_checked.to_string());
    props.insert("updatesAvailable".to_owned(), updates.len().to_string());
    skill::telemetry::track("check", props);

    println!();
    Ok(())
}

//! `skills update` command implementation.
//!
//! Matches the TS `cli.ts` `runUpdate` UX: plain console output with ANSI
//! colors, skipped skills handling.

use std::collections::HashMap;

use miette::Result;

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

struct UpdateEntry {
    name: String,
    source_url: String,
    skill_path: Option<String>,
}

/// Run the update command.
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
    let mut updates: Vec<UpdateEntry> = Vec::new();
    let mut skipped = 0usize;

    for (name, entry) in &lock.skills {
        if entry.skill_folder_hash.is_empty() || entry.skill_path.is_none() {
            skipped += 1;
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

    let checked_count = lock.skills.len() - skipped;

    if checked_count == 0 {
        println!("{DIM}No skills to check.{RESET}");
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

        let mut install_url = update.source_url.clone();
        if let Some(ref sp) = update.skill_path {
            let mut folder = sp.clone();
            if folder.ends_with("/SKILL.md") {
                folder = folder[..folder.len() - 9].to_owned();
            } else if folder.ends_with("SKILL.md") {
                folder = folder[..folder.len() - 8].to_owned();
            }
            folder = folder.trim_end_matches('/').to_owned();

            install_url = install_url
                .trim_end_matches(".git")
                .trim_end_matches('/')
                .to_owned();
            install_url = format!("{install_url}/tree/main/{folder}");
        }

        let output = tokio::process::Command::new("skills")
            .args(["add", &install_url, "-g", "-y"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                success_count += 1;
                println!("  {TEXT}✓{RESET} Updated {}", update.name);
            }
            _ => {
                fail_count += 1;
                println!("  {DIM}✗ Failed to update {}{RESET}", update.name);
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

    // Telemetry
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), updates.len().to_string());
    props.insert("successCount".to_owned(), success_count.to_string());
    props.insert("failCount".to_owned(), fail_count.to_string());
    skill::telemetry::track("update", props);

    println!();
    Ok(())
}

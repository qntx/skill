//! `skills check` command implementation.

use std::collections::HashMap;

use console::style;
use miette::Result;

/// Run the check command.
pub async fn run() -> Result<()> {
    println!("  Checking for skill updates...");
    println!();

    let lock = skill::lock::read_skill_lock()
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        println!("  {}", style("No skills tracked in lock file.").dim());
        println!(
            "  {} {} {}",
            style("Install skills with").dim(),
            style("skills add <package>"),
            style("").dim()
        );
        return Ok(());
    }

    let token = skill::lock::get_github_token();
    let mut updates: Vec<(String, String)> = Vec::new();
    let mut skipped = 0u32;
    let mut errors = 0u32;

    for (name, entry) in &lock.skills {
        if entry.skill_folder_hash.is_empty() || entry.skill_path.is_none() {
            skipped += 1;
            continue;
        }

        let skill_path = entry.skill_path.as_deref().unwrap_or_default();
        match skill::lock::fetch_skill_folder_hash(&entry.source, skill_path, token.as_deref())
            .await
        {
            Ok(Some(latest)) if latest != entry.skill_folder_hash => {
                updates.push((name.clone(), entry.source.clone()));
            }
            Err(_) => {
                errors += 1;
            }
            _ => {}
        }
    }

    if updates.is_empty() {
        println!("  {} All skills are up to date", style("✓").green());
    } else {
        println!("  {} update(s) available:", style(updates.len()).yellow());
        println!();
        for (name, source) in &updates {
            println!("    {} {name}", style("↑").cyan());
            println!("      {}", style(format!("source: {source}")).dim());
        }
        println!();
        println!(
            "  {} {} {}",
            style("Run").dim(),
            style("skills update"),
            style("to update all skills").dim()
        );
    }

    if skipped > 0 {
        println!();
        println!(
            "  {}",
            style(format!(
                "{skipped} skill(s) cannot be checked automatically"
            ))
            .dim()
        );
    }

    if errors > 0 {
        println!(
            "  {}",
            style(format!(
                "Could not check {errors} skill(s) (may need reinstall)"
            ))
            .dim()
        );
    }

    // Telemetry
    let total = lock.skills.len() as u32 - skipped;
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), total.to_string());
    props.insert("updatesAvailable".to_owned(), updates.len().to_string());
    skill::telemetry::track("check", props);

    println!();
    Ok(())
}

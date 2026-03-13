//! `skills check` command implementation.

use std::collections::HashMap;

use console::style;
use miette::{IntoDiagnostic, Result};

/// Run the check command.
pub async fn run() -> Result<()> {
    cliclack::intro(style(" skills check ").on_cyan().black()).into_diagnostic()?;

    let spinner = cliclack::spinner();
    spinner.start("Reading lock file...");

    let lock = skill::lock::read_skill_lock()
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        spinner.stop("No skills tracked in lock file.");
        cliclack::outro(format!(
            "Install skills with {}",
            style("skills add <package>").cyan()
        ))
        .into_diagnostic()?;
        return Ok(());
    }

    spinner.stop(format!("Found {} tracked skill(s)", lock.skills.len()));

    let spinner = cliclack::spinner();
    spinner.start("Checking for updates...");

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
        spinner.stop(format!("{} All skills are up to date", style("✓").green()));
    } else {
        spinner.stop(format!(
            "{} update(s) available",
            style(updates.len()).yellow()
        ));
        println!();
        for (name, source) in &updates {
            println!("    {} {name}", style("↑").cyan());
            println!("      {}", style(format!("source: {source}")).dim());
        }
    }

    if skipped > 0 {
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
    let total = u32::try_from(lock.skills.len()).unwrap_or(u32::MAX) - skipped;
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), total.to_string());
    props.insert("updatesAvailable".to_owned(), updates.len().to_string());
    skill::telemetry::track("check", props);

    if updates.is_empty() {
        cliclack::outro("Done").into_diagnostic()?;
    } else {
        cliclack::outro(format!(
            "Run {} to update all skills",
            style("skills update").cyan()
        ))
        .into_diagnostic()?;
    }

    Ok(())
}

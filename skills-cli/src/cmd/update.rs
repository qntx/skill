//! `skills update` command implementation.

use std::collections::HashMap;

use console::style;
use miette::{IntoDiagnostic, Result};

use crate::ui;

struct UpdateEntry {
    name: String,
    source_url: String,
    skill_path: Option<String>,
}

/// Run the update command.
pub async fn run() -> Result<()> {
    cliclack::intro(style(" skills update ").on_cyan().black()).into_diagnostic()?;

    let spinner = cliclack::spinner();
    spinner.start("Checking for updates...");

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

    let token = skill::lock::get_github_token();
    let mut updates: Vec<UpdateEntry> = Vec::new();

    for (name, entry) in &lock.skills {
        if entry.skill_folder_hash.is_empty() || entry.skill_path.is_none() {
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

    if updates.is_empty() {
        spinner.stop(format!("{} All skills are up to date", style("✓").green()));
        cliclack::outro("Done").into_diagnostic()?;
        return Ok(());
    }

    spinner.stop(format!("Found {} update(s)", style(updates.len()).yellow()));

    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    for update in &updates {
        let spinner = cliclack::spinner();
        spinner.start(format!("Updating {}...", update.name));

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
                spinner.stop(format!("{} Updated {}", style("✓").green(), update.name));
            }
            _ => {
                fail_count += 1;
                spinner.stop(format!(
                    "{} Failed to update {}",
                    style("✗").red(),
                    update.name
                ));
            }
        }
    }

    if success_count > 0 {
        ui::print_success(&format!("Updated {success_count} skill(s)"));
    }
    if fail_count > 0 {
        ui::print_error(&format!("Failed to update {fail_count} skill(s)"));
    }

    // Telemetry
    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), updates.len().to_string());
    props.insert("successCount".to_owned(), success_count.to_string());
    props.insert("failCount".to_owned(), fail_count.to_string());
    skill::telemetry::track("update", props);

    cliclack::outro("Done").into_diagnostic()?;
    Ok(())
}

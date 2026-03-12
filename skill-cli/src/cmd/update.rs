//! `skills update` command implementation.

use std::collections::HashMap;

use console::style;
use miette::Result;

/// Run the update command.
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

    struct UpdateEntry {
        name: String,
        source_url: String,
        skill_path: Option<String>,
    }

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
        println!("  {} All skills are up to date", style("✓").green());
        println!();
        return Ok(());
    }

    println!("  Found {} update(s)", style(updates.len()).yellow());
    println!();

    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    for update in &updates {
        println!("  Updating {}...", update.name);

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
                println!("    {} Updated {}", style("✓").green(), update.name);
            }
            _ => {
                fail_count += 1;
                println!("    {} Failed to update {}", style("✗").red(), update.name);
            }
        }
    }

    println!();
    if success_count > 0 {
        println!(
            "  {} Updated {} skill(s)",
            style("✓").green(),
            success_count
        );
    }
    if fail_count > 0 {
        println!(
            "  {} Failed to update {} skill(s)",
            style("✗").red(),
            fail_count
        );
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

//! `skills experimental_install` command implementation.
//!
//! Restores skills from a project `skills-lock.json`.

use console::style;
use miette::{IntoDiagnostic, Result};

/// Run the `experimental_install` command.
pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock_path = cwd.join("skills-lock.json");

    if !lock_path.exists() {
        println!();
        println!(
            "  {}",
            style("No skills-lock.json found in current directory.").yellow()
        );
        println!(
            "  {} {} {}",
            style("Install skills with").dim(),
            style("skills add <package>"),
            style("to create one.").dim()
        );
        println!();
        return Ok(());
    }

    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        println!("  {}", style("No skills in lock file.").dim());
        return Ok(());
    }

    println!(
        "  Restoring {} skill(s) from skills-lock.json...",
        lock.skills.len()
    );
    println!();

    let mut success = 0u32;
    let mut failed = 0u32;

    for (name, entry) in &lock.skills {
        println!("  Installing {name} from {}...", entry.source);

        let output = tokio::process::Command::new("skills")
            .args(["add", &entry.source, "-y", "--skill", name])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                success += 1;
                println!("    {} {name}", style("✓").green());
            }
            _ => {
                failed += 1;
                println!("    {} {name}", style("✗").red());
            }
        }
    }

    println!();
    if success > 0 {
        println!("  {} Restored {} skill(s)", style("✓").green(), success);
    }
    if failed > 0 {
        println!(
            "  {} Failed to restore {} skill(s)",
            style("✗").red(),
            failed
        );
    }
    println!();

    Ok(())
}

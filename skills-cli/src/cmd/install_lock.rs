//! `skills experimental_install` command implementation.
//!
//! Restores skills from a project `skills-lock.json`.

use console::style;
use miette::{IntoDiagnostic, Result};

use crate::ui;

/// Run the `experimental_install` command.
pub async fn run() -> Result<()> {
    cliclack::intro(style(" skills install ").on_cyan().black()).into_diagnostic()?;

    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock_path = cwd.join("skills-lock.json");

    if !lock_path.exists() {
        cliclack::outro(format!(
            "No skills-lock.json found. Install skills with {} to create one.",
            style("skills add <package>").cyan()
        ))
        .into_diagnostic()?;
        return Ok(());
    }

    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        cliclack::outro("No skills in lock file.").into_diagnostic()?;
        return Ok(());
    }

    cliclack::log::info(format!(
        "Restoring {} skill(s) from skills-lock.json",
        lock.skills.len()
    ))
    .into_diagnostic()?;

    let mut success = 0u32;
    let mut failed = 0u32;

    for (name, entry) in &lock.skills {
        let spinner = cliclack::spinner();
        spinner.start(format!("Installing {name} from {}...", entry.source));

        let output = tokio::process::Command::new("skills")
            .args(["add", &entry.source, "-y", "--skill", name])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                success += 1;
                spinner.stop(format!("{} {name}", style("✓").green()));
            }
            _ => {
                failed += 1;
                spinner.stop(format!("{} {name}", style("✗").red()));
            }
        }
    }

    if success > 0 {
        ui::print_success(&format!("Restored {success} skill(s)"));
    }
    if failed > 0 {
        ui::print_error(&format!("Failed to restore {failed} skill(s)"));
    }

    cliclack::outro("Done").into_diagnostic()?;
    Ok(())
}

//! `skills experimental_install` command implementation.
//!
//! Restores skills from a project `skills-lock.json`.
//! Uses plain console output to match TS style.

use miette::{IntoDiagnostic, Result};

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

/// Run the `experimental_install` command.
pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock_path = cwd.join("skills-lock.json");

    if !lock_path.exists() {
        println!("{DIM}No skills-lock.json found.{RESET}");
        println!(
            "{DIM}Install skills with{RESET} {TEXT}skills add <package>{RESET} {DIM}to create one.{RESET}"
        );
        return Ok(());
    }

    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        println!("{DIM}No skills in lock file.{RESET}");
        return Ok(());
    }

    println!(
        "{TEXT}Restoring {} skill(s) from skills-lock.json{RESET}",
        lock.skills.len()
    );
    println!();

    let mut success = 0u32;
    let mut failed = 0u32;

    for (name, entry) in &lock.skills {
        println!("{DIM}Installing {name} from {}...{RESET}", entry.source);

        let output = tokio::process::Command::new("skills")
            .args(["add", &entry.source, "-y", "--skill", name])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                success += 1;
                println!("  {TEXT}✓{RESET} {name}");
            }
            _ => {
                failed += 1;
                println!("  {DIM}✗ {name}{RESET}");
            }
        }
    }

    println!();
    if success > 0 {
        println!("{TEXT}✓ Restored {success} skill(s){RESET}");
    }
    if failed > 0 {
        println!("{DIM}✗ Failed to restore {failed} skill(s){RESET}");
    }
    println!();

    Ok(())
}

//! `skills init [name]` command implementation.
//!
//! Matches the TS `cli.ts` `runInit` UX: plain console output with ANSI
//! colors, no cliclack framing.

use clap::Args;
use miette::{IntoDiagnostic, Result};

use crate::ui::{DIM, RESET, TEXT};

/// Arguments for the `init` command.
#[derive(Args)]
pub struct InitArgs {
    /// Skill name (defaults to current directory name).
    pub name: Option<String>,
}

/// Run the init command.
pub fn run(args: &InitArgs) -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let cwd_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-skill")
        .to_owned();

    let skill_name = args.name.as_deref().unwrap_or(&cwd_name);
    let has_name = args.name.is_some();

    #[allow(clippy::redundant_clone)]
    let skill_dir = if has_name {
        cwd.join(skill_name)
    } else {
        cwd.clone()
    };

    let skill_file = skill_dir.join("SKILL.md");
    let display_path = if has_name {
        format!("{skill_name}/SKILL.md")
    } else {
        "SKILL.md".to_owned()
    };

    if skill_file.exists() {
        println!("{TEXT}Skill already exists at {DIM}{display_path}{RESET}");
        return Ok(());
    }

    if has_name {
        std::fs::create_dir_all(&skill_dir).into_diagnostic()?;
    }

    let content = format!(
        r"---
name: {skill_name}
description: A brief description of what this skill does
---

# {skill_name}

Instructions for the agent to follow when this skill is activated.

## When to use

Describe when this skill should be used.

## Instructions

1. First step
2. Second step
3. Additional steps as needed
"
    );

    std::fs::write(&skill_file, content).into_diagnostic()?;

    println!("{TEXT}Initialized skill: {DIM}{skill_name}{RESET}");
    println!();
    println!("{DIM}Created:{RESET}");
    println!("  {display_path}");
    println!();
    println!("{DIM}Next steps:{RESET}");
    println!("  1. Edit {TEXT}{display_path}{RESET} to define your skill instructions");
    println!("  2. Update the {TEXT}name{RESET} and {TEXT}description{RESET} in the frontmatter");
    println!();
    println!("{DIM}Publishing:{RESET}");
    println!("  {DIM}GitHub:{RESET}  Push to a repo, then {TEXT}skills add <owner>/<repo>{RESET}");
    println!(
        "  {DIM}URL:{RESET}     Host the file, then {TEXT}skills add https://example.com/{display_path}{RESET}"
    );
    println!();
    println!("Browse existing skills for inspiration at {TEXT}https://skills.sh/{RESET}");
    println!();

    Ok(())
}

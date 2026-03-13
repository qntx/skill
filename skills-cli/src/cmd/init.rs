//! `skills init [name]` command implementation.

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result};

/// Arguments for the `init` command.
#[derive(Args)]
pub struct InitArgs {
    /// Skill name (defaults to current directory name).
    pub name: Option<String>,
}

/// Run the init command.
pub fn run(args: &InitArgs) -> Result<()> {
    cliclack::intro(style(" skills init ").on_cyan().black()).into_diagnostic()?;

    let cwd = std::env::current_dir().into_diagnostic()?;
    let cwd_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-skill")
        .to_owned();

    let skill_name = args.name.as_deref().unwrap_or(&cwd_name);
    let has_name = args.name.is_some();

    let skill_dir = if has_name { cwd.join(skill_name) } else { cwd };

    let skill_file = skill_dir.join("SKILL.md");
    let display_path = if has_name {
        format!("{skill_name}/SKILL.md")
    } else {
        "SKILL.md".to_owned()
    };

    if skill_file.exists() {
        cliclack::outro(format!(
            "Skill already exists at {}",
            style(&display_path).dim()
        ))
        .into_diagnostic()?;
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

    cliclack::log::success(format!("Created {display_path}")).into_diagnostic()?;

    cliclack::note(
        "Next steps",
        format!(
            "1. Edit {display_path} to define your skill instructions\n\
             2. Update the name and description in the frontmatter\n\
             3. Push to a repo, then run: skills add <owner>/<repo>"
        ),
    )
    .into_diagnostic()?;

    cliclack::outro(format!(
        "Browse existing skills for inspiration at {}",
        style("https://skills.sh/").cyan()
    ))
    .into_diagnostic()?;

    Ok(())
}

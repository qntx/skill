//! `skills completions <shell>` command implementation.
//!
//! Generates shell completion scripts for bash, zsh, fish, and PowerShell.
//! This is a Rust-only capability that the TS CLI cannot provide.

use std::io;

use clap::Args;
use clap_complete::Shell;
use miette::Result;

/// Arguments for the `completions` command.
#[derive(Args)]
pub struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, powershell).
    pub shell: Shell,
}

/// Generate shell completions and write to stdout.
pub fn run(args: &CompletionsArgs, cmd: &mut clap::Command) -> Result<()> {
    clap_complete::generate(args.shell, cmd, "skills", &mut io::stdout());
    Ok(())
}

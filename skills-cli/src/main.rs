//! `skills` CLI — The open agent skills ecosystem.
//!
//! Feature-equivalent Rust port of the Vercel `TypeScript` `skills` CLI.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary uses stdout/stderr for user output"
)]
#![allow(
    clippy::let_underscore_must_use,
    reason = "cliclack log/note/outro calls return Result for IO which we intentionally ignore in CLI output"
)]
#![allow(
    clippy::missing_docs_in_private_items,
    reason = "CLI commands are self-documenting via clap attributes"
)]

mod commands;
mod ui;

use clap::{CommandFactory, Parser, Subcommand};

/// The open agent skills ecosystem.
#[derive(Parser)]
#[command(
    name = "skills",
    version,
    about = "The open agent skills ecosystem",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a skill package.
    #[command(visible_aliases = &["a", "install", "i"])]
    Add(commands::add::AddArgs),

    /// Remove installed skills.
    #[command(visible_aliases = &["rm", "r"])]
    Remove(commands::remove::RemoveArgs),

    /// List installed skills.
    #[command(visible_alias = "ls")]
    List(commands::list::ListArgs),

    /// Search for skills interactively.
    #[command(visible_aliases = &["f", "s", "search"])]
    Find(commands::find::FindArgs),

    /// Check for available skill updates.
    Check,

    /// Update all skills to latest versions.
    Update,

    /// Initialize a skill (creates SKILL.md).
    Init(commands::init::InitArgs),

    /// Restore skills from skills-lock.json.
    #[command(name = "experimental_install")]
    ExperimentalInstall,

    /// Sync skills from `node_modules` into agent directories.
    #[command(name = "experimental_sync")]
    ExperimentalSync(commands::sync::SyncArgs),

    /// Generate shell completions.
    Completions(commands::completions::CompletionsArgs),

    /// Check installation health (broken symlinks, lock consistency).
    Doctor,

    /// Upgrade the skills CLI binary to the latest release.
    Upgrade,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    skill::telemetry::set_version(env!("CARGO_PKG_VERSION"));

    let cli = Cli::parse();

    match cli.command {
        None => {
            ui::show_banner(env!("CARGO_PKG_VERSION"));
        }
        Some(cmd) => match cmd {
            Commands::Add(args) => {
                ui::show_logo();
                commands::add::run(args).await?;
            }
            Commands::Remove(args) => commands::remove::run(args).await?,
            Commands::List(args) => commands::list::run(args).await?,
            Commands::Find(args) => {
                ui::show_logo();
                println!();
                commands::find::run(args).await?;
            }
            Commands::Check => commands::check::run().await?,
            Commands::Update => commands::update::run().await?,
            Commands::Init(args) => {
                ui::show_logo();
                println!();
                commands::init::run(&args)?;
            }
            Commands::ExperimentalInstall => {
                ui::show_logo();
                commands::install_lock::run().await?;
            }
            Commands::ExperimentalSync(args) => {
                ui::show_logo();
                commands::sync::run(args).await?;
            }
            Commands::Completions(args) => {
                let mut command = Cli::command();
                commands::completions::run(&args, &mut command);
            }
            Commands::Doctor => {
                ui::show_logo();
                commands::doctor::run().await?;
            }
            Commands::Upgrade => {
                ui::show_logo();
                commands::upgrade::run().await?;
            }
        },
    }

    Ok(())
}

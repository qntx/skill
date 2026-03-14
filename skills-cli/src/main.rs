//! `skills` CLI — The open agent skills ecosystem.
//!
//! Feature-equivalent Rust port of the Vercel `TypeScript` `skills` CLI.

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod cmd;
mod ui;

use clap::{Parser, Subcommand};

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
    Add(cmd::add::AddArgs),

    /// Remove installed skills.
    #[command(visible_aliases = &["rm", "r"])]
    Remove(cmd::remove::RemoveArgs),

    /// List installed skills.
    #[command(visible_alias = "ls")]
    List(cmd::list::ListArgs),

    /// Search for skills interactively.
    #[command(visible_aliases = &["f", "s", "search"])]
    Find(cmd::find::FindArgs),

    /// Check for available skill updates.
    Check,

    /// Update all skills to latest versions.
    #[command(visible_alias = "upgrade")]
    Update,

    /// Initialize a skill (creates SKILL.md).
    Init(cmd::init::InitArgs),

    /// Restore skills from skills-lock.json.
    #[command(name = "experimental_install")]
    ExperimentalInstall,

    /// Sync skills from `node_modules` into agent directories.
    #[command(name = "experimental_sync")]
    ExperimentalSync(cmd::sync::SyncArgs),
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
                cmd::add::run(args).await?;
            }
            Commands::Remove(args) => cmd::remove::run(args).await?,
            Commands::List(args) => cmd::list::run(args).await?,
            Commands::Find(args) => {
                ui::show_logo();
                println!();
                cmd::find::run(args).await?;
            }
            Commands::Check => cmd::check::run().await?,
            Commands::Update => cmd::update::run().await?,
            Commands::Init(args) => {
                ui::show_logo();
                println!();
                cmd::init::run(&args)?;
            }
            Commands::ExperimentalInstall => {
                ui::show_logo();
                cmd::install_lock::run().await?;
            }
            Commands::ExperimentalSync(args) => {
                ui::show_logo();
                cmd::sync::run(args).await?;
            }
        },
    }

    Ok(())
}

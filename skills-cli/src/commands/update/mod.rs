//! `skills update` command.
//!
//! Split by concern:
//!
//! - [`scope`]          — arg parsing + `Project` / `Global` / `Both` resolver
//! - [`stats`]          — per-scope success/fail tallies + filter predicate
//! - [`source_builder`] — reconstruct `skills add` args from a lock entry
//! - [`global`]         — global lock update pass
//! - [`project`]        — project lock update pass
//!
//! Entry point: [`run`].

mod global;
mod project;
mod scope;
mod source_builder;
mod stats;

use std::collections::HashMap;

use miette::Result;

pub(crate) use self::scope::UpdateArgs;
use self::scope::UpdateScope;
use self::stats::ScopeStats;
use crate::ui::{BOLD, DIM, RESET, TEXT};

/// Run the `skills update` command.
///
/// Matches TS `cli.ts::runUpdate` behaviour:
///
/// - `--global` / `-g`, `--project` / `-p`, `--yes` / `-y` flags
/// - Positional skill name filter (case-insensitive)
/// - Interactive scope prompt (Project / Global / Both) when no flag + TTY
/// - Non-interactive auto-detection via `has_project_skills`
/// - Ref-aware: uses each lock entry's stored `ref` when re-installing
pub(crate) async fn run(args: UpdateArgs) -> Result<()> {
    let scope = scope::resolve(&args).await?;
    let filter = &args.skills;

    if filter.is_empty() {
        println!("{TEXT}Checking for skill updates...{RESET}");
    } else {
        println!("{TEXT}Updating {}...{RESET}", filter.join(", "));
    }
    println!();

    let mut totals = ScopeStats::default();

    if scope.includes_global() {
        if scope == UpdateScope::Both && filter.is_empty() {
            println!("{BOLD}Global Skills{RESET}");
        }
        totals.merge(global::update(filter).await?);
        if scope == UpdateScope::Both && filter.is_empty() {
            println!();
        }
    }

    if scope.includes_project() {
        if scope == UpdateScope::Both && filter.is_empty() {
            println!("{BOLD}Project Skills{RESET}");
        }
        totals.merge(project::update(filter).await?);
    }

    if !filter.is_empty() && totals.found == 0 {
        println!(
            "{DIM}No skills matching {} found in {} scope.{RESET}",
            filter.join(", "),
            match scope {
                UpdateScope::Global => "global",
                UpdateScope::Project => "project",
                UpdateScope::Both => "any",
            }
        );
    }

    let mut props = HashMap::new();
    props.insert("successCount".to_owned(), totals.success.to_string());
    props.insert("failCount".to_owned(), totals.fail.to_string());
    props.insert("scope".to_owned(), scope.telemetry_label().to_owned());
    skill::telemetry::track("update", props);

    println!();
    Ok(())
}

//! `skills update` command implementation.
//!
//! Matches the TS `cli.ts::runUpdate` UX:
//!
//! - `--global` / `-g`, `--project` / `-p`, `--yes` / `-y` flags
//! - Positional skill name filter (case-insensitive)
//! - Interactive scope prompt (Project / Global / Both) when no flag + TTY
//! - Non-interactive auto-detection via [`has_project_skills`]
//! - Ref-aware: uses each lock entry's stored `ref` when re-installing
//! - Uses in-process `run_add` instead of spawning a subprocess for
//!   cross-platform reliability (matches existing Rust convention).

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::Path;

use clap::Args;
use miette::{IntoDiagnostic, Result};
use skill::local_lock::LocalSkillLockEntry;
use skill::lock::SkillLockEntry;
use skill::sanitize::sanitize_metadata;

use super::add::{RunAddOptions, run_add};
use super::{SkippedSkill, get_skip_reason, print_skipped_skills, should_skip};
use crate::ui::{BOLD, DIM, RESET, TEXT};

/// Arguments for the `skills update` command.
#[derive(Args, Debug, Default)]
pub(crate) struct UpdateArgs {
    /// Update global skills only.
    #[arg(short, long)]
    pub global: bool,
    /// Update project skills only.
    #[arg(short, long)]
    pub project: bool,
    /// Skip interactive scope prompt (auto-detects project vs global).
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Optional skill names to restrict the update to (case-insensitive).
    pub skills: Vec<String>,
}

/// Requested update scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UpdateScope {
    Project,
    Global,
    Both,
}

/// Aggregated stats for a single scope.
#[derive(Clone, Copy, Debug, Default)]
struct ScopeStats {
    success: u32,
    fail: u32,
    found: u32,
}

impl ScopeStats {
    const fn merge(&mut self, other: Self) {
        self.success += other.success;
        self.fail += other.fail;
        self.found += other.found;
    }
}

/// Run the `skills update` command.
pub(crate) async fn run(args: UpdateArgs) -> Result<()> {
    let scope = resolve_scope(&args).await?;
    let filter = &args.skills;

    if filter.is_empty() {
        println!("{TEXT}Checking for skill updates...{RESET}");
    } else {
        println!("{TEXT}Updating {}...{RESET}", filter.join(", "));
    }
    println!();

    let mut totals = ScopeStats::default();

    if scope == UpdateScope::Global || scope == UpdateScope::Both {
        if scope == UpdateScope::Both && filter.is_empty() {
            println!("{BOLD}Global Skills{RESET}");
        }
        totals.merge(update_global_skills(filter).await?);
        if scope == UpdateScope::Both && filter.is_empty() {
            println!();
        }
    }

    if scope == UpdateScope::Project || scope == UpdateScope::Both {
        if scope == UpdateScope::Both && filter.is_empty() {
            println!("{BOLD}Project Skills{RESET}");
        }
        totals.merge(update_project_skills(filter).await?);
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
    props.insert(
        "scope".to_owned(),
        match scope {
            UpdateScope::Global => "global",
            UpdateScope::Project => "project",
            UpdateScope::Both => "both",
        }
        .to_owned(),
    );
    skill::telemetry::track("update", props);

    println!();
    Ok(())
}

// -----------------------------------------------------------------------------
// Scope resolution
// -----------------------------------------------------------------------------

/// Resolve the effective update scope.
///
/// Matches TS `resolveUpdateScope`:
///
/// 1. When `skills` filter is set, `-g` / `-p` select single scope, else `Both`.
/// 2. Explicit `-g` and `-p` are combined into `Both`.
/// 3. Non-interactive (`-y` or no TTY): auto-detect via [`has_project_skills`].
/// 4. Otherwise prompt the user interactively.
async fn resolve_scope(args: &UpdateArgs) -> Result<UpdateScope> {
    if !args.skills.is_empty() {
        return Ok(match (args.global, args.project) {
            (true, false) => UpdateScope::Global,
            (false, true) => UpdateScope::Project,
            _ => UpdateScope::Both,
        });
    }

    match (args.global, args.project) {
        (true, true) => return Ok(UpdateScope::Both),
        (true, false) => return Ok(UpdateScope::Global),
        (false, true) => return Ok(UpdateScope::Project),
        (false, false) => {}
    }

    let cwd = std::env::current_dir().into_diagnostic()?;

    if args.yes || !std::io::stdin().is_terminal() {
        return Ok(if has_project_skills(&cwd).await {
            UpdateScope::Project
        } else {
            UpdateScope::Global
        });
    }

    crate::ui::drain_input_events();
    let scope = cliclack::select("Update scope")
        .item(
            UpdateScope::Project,
            "Project",
            "Update skills in current directory",
        )
        .item(
            UpdateScope::Global,
            "Global",
            "Update skills in home directory",
        )
        .item(UpdateScope::Both, "Both", "Update all skills")
        .interact()
        .into_diagnostic()?;
    Ok(scope)
}

/// Whether the current directory has any project-level skills.
///
/// Mirrors TS `hasProjectSkills`: returns `true` if either `skills-lock.json`
/// exists, or `.agents/skills/` contains at least one sub-directory with a
/// `SKILL.md` file.
async fn has_project_skills(cwd: &Path) -> bool {
    if tokio::fs::try_exists(cwd.join("skills-lock.json"))
        .await
        .unwrap_or(false)
    {
        return true;
    }

    let skills_dir = cwd.join(".agents").join("skills");
    let Ok(mut entries) = tokio::fs::read_dir(&skills_dir).await else {
        return false;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let is_dir = entry
            .file_type()
            .await
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        let has_skill_md = tokio::fs::try_exists(entry.path().join("SKILL.md"))
            .await
            .unwrap_or(false);
        if has_skill_md {
            return true;
        }
    }

    false
}

// -----------------------------------------------------------------------------
// Global skills
// -----------------------------------------------------------------------------

/// Refresh global skills that differ from their latest upstream hash.
async fn update_global_skills(filter: &[String]) -> Result<ScopeStats> {
    let lock = skill::lock::read_skill_lock()
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    if lock.skills.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No global skills tracked in lock file.{RESET}");
            println!("{DIM}Install skills with{RESET} {TEXT}skills add <package> -g{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    let token = skill::github::get_token();
    let mut skipped: Vec<SkippedSkill> = Vec::new();
    let mut checkable: Vec<(&String, &SkillLockEntry)> = Vec::new();

    for (name, entry) in &lock.skills {
        if !matches_skill_filter(name, filter) {
            continue;
        }
        if should_skip(entry) {
            skipped.push(SkippedSkill {
                name: name.clone(),
                reason: get_skip_reason(entry),
                source_url: entry.source_url.clone(),
                source_type: entry.source_type.clone(),
                git_ref: entry.git_ref.clone(),
            });
            continue;
        }
        checkable.push((name, entry));
    }

    // Probe latest tree SHA for each checkable skill.
    let mut updates: Vec<(String, SkillLockEntry)> = Vec::new();
    for (idx, (name, entry)) in checkable.iter().enumerate() {
        print!(
            "\r{DIM}Checking global skill {}/{}: {}{RESET}\x1b[K",
            idx + 1,
            checkable.len(),
            sanitize_metadata(name),
        );
        let skill_path = entry.skill_path.as_deref().unwrap_or_default();
        let latest = skill::github::fetch_skill_folder_hash(
            &entry.source,
            skill_path,
            token.as_deref(),
            entry.git_ref.as_deref(),
        )
        .await
        .unwrap_or(None);

        if let Some(hash) = latest
            && hash != entry.skill_folder_hash
        {
            updates.push(((*name).clone(), (*entry).clone()));
        }
    }
    if !checkable.is_empty() {
        print!("\r\x1b[K");
    }

    let checked_count = checkable.len() + skipped.len();

    if checkable.is_empty() && skipped.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No global skills to check.{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    if checkable.is_empty() && !skipped.is_empty() {
        print_skipped_skills(&skipped);
        return Ok(ScopeStats {
            success: 0,
            fail: 0,
            found: checked_count.try_into().unwrap_or(u32::MAX),
        });
    }

    if updates.is_empty() {
        println!("{TEXT}\u{2713} All global skills are up to date{RESET}");
        print_skipped_skills(&skipped);
        return Ok(ScopeStats {
            success: 0,
            fail: 0,
            found: checked_count.try_into().unwrap_or(u32::MAX),
        });
    }

    println!("{TEXT}Found {} global update(s){RESET}", updates.len());
    println!();

    let mut stats = ScopeStats {
        found: checked_count.try_into().unwrap_or(u32::MAX),
        ..ScopeStats::default()
    };

    for (name, entry) in &updates {
        let safe_name = sanitize_metadata(name);
        println!("{TEXT}Updating {safe_name}...{RESET}");
        let install_url = build_update_install_source(entry);
        match run_add(RunAddOptions {
            source: install_url,
            global: Some(true),
            yes: true,
            skill_filter: Some(vec![name.clone()]),
            agent: None,
            dry_run: false,
        })
        .await
        {
            Ok(()) => {
                stats.success += 1;
                println!("  {TEXT}\u{2713}{RESET} Updated {safe_name}");
            }
            Err(e) => {
                stats.fail += 1;
                println!("  {DIM}\u{2717} Failed to update {safe_name}{RESET}");
                tracing::warn!(skill = %name, error = %e, "update failed");
            }
        }
    }

    print_skipped_skills(&skipped);
    Ok(stats)
}

// -----------------------------------------------------------------------------
// Project skills
// -----------------------------------------------------------------------------

/// Refresh project-level skills recorded in `skills-lock.json`.
async fn update_project_skills(filter: &[String]) -> Result<ScopeStats> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let lock = skill::local_lock::read_local_lock(&cwd)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    let targets: Vec<(String, LocalSkillLockEntry)> = lock
        .skills
        .iter()
        .filter(|(name, entry)| {
            matches_skill_filter(name, filter)
                // node_modules and local path skills are managed out-of-band
                // (by experimental_sync / direct edits).
                && entry.source_type != "node_modules"
                && entry.source_type != "local"
        })
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect();

    if targets.is_empty() {
        if filter.is_empty() {
            println!("{DIM}No project skills to update.{RESET}");
            println!("{DIM}Install project skills with{RESET} {TEXT}skills add <package>{RESET}");
        }
        return Ok(ScopeStats::default());
    }

    println!(
        "{TEXT}Refreshing {} project skill(s)...{RESET}",
        targets.len()
    );
    println!();

    let mut stats = ScopeStats {
        found: targets.len().try_into().unwrap_or(u32::MAX),
        ..ScopeStats::default()
    };

    for (name, entry) in &targets {
        let safe_name = sanitize_metadata(name);
        println!("{TEXT}Updating {safe_name}...{RESET}");
        let install_url = build_local_update_source(entry);
        match run_add(RunAddOptions {
            source: install_url,
            global: Some(false),
            yes: true,
            skill_filter: Some(vec![name.clone()]),
            agent: None,
            dry_run: false,
        })
        .await
        {
            Ok(()) => {
                stats.success += 1;
                println!("  {TEXT}\u{2713}{RESET} Updated {safe_name}");
            }
            Err(e) => {
                stats.fail += 1;
                println!("  {DIM}\u{2717} Failed to update {safe_name}{RESET}");
                tracing::warn!(skill = %name, error = %e, "project update failed");
            }
        }
    }

    Ok(stats)
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Case-insensitive skill-name filter. Empty filter matches all.
fn matches_skill_filter(name: &str, filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    let lower = name.to_lowercase();
    filter.iter().any(|f| f.to_lowercase() == lower)
}

/// Append `#ref` to `source_url` when `git_ref` is set.
fn format_source_input(source_url: &str, git_ref: Option<&str>) -> String {
    git_ref.map_or_else(|| source_url.to_owned(), |r| format!("{source_url}#{r}"))
}

/// Build the install source for a global lock entry (matches TS
/// `buildUpdateInstallSource`). Uses shorthand `owner/repo/path` + optional
/// `#ref` so the resolver can apply subpath filtering without guessing a
/// branch name.
fn build_update_install_source(entry: &SkillLockEntry) -> String {
    let Some(skill_path) = entry.skill_path.as_deref() else {
        return format_source_input(&entry.source_url, entry.git_ref.as_deref());
    };

    // Strip SKILL.md suffix and trailing slash.
    let mut folder = skill_path.to_owned();
    if let Some(stripped) = folder.strip_suffix("/SKILL.md") {
        folder.truncate(stripped.len());
    } else if let Some(stripped) = folder.strip_suffix("SKILL.md") {
        folder.truncate(stripped.len());
    }
    let folder = folder.trim_end_matches('/');

    let mut install = if folder.is_empty() {
        entry.source.clone()
    } else {
        format!("{}/{folder}", entry.source)
    };
    if let Some(r) = entry.git_ref.as_deref() {
        install.push('#');
        install.push_str(r);
    }
    install
}

/// Build the install source for a project lock entry (matches TS
/// `buildLocalUpdateSource`). Project entries only carry `source` + `ref`.
fn build_local_update_source(entry: &LocalSkillLockEntry) -> String {
    format_source_input(&entry.source, entry.git_ref.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_global(skill_path: Option<&str>, git_ref: Option<&str>) -> SkillLockEntry {
        SkillLockEntry {
            source: "owner/repo".to_owned(),
            source_type: "github".to_owned(),
            source_url: "https://github.com/owner/repo.git".to_owned(),
            git_ref: git_ref.map(String::from),
            skill_path: skill_path.map(String::from),
            skill_folder_hash: "abc".to_owned(),
            installed_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            plugin_name: None,
        }
    }

    #[test]
    fn matches_skill_filter_empty_matches_all() {
        assert!(matches_skill_filter("any", &[]));
    }

    #[test]
    fn matches_skill_filter_case_insensitive() {
        let filter = vec!["Find-Skills".to_owned()];
        assert!(matches_skill_filter("find-skills", &filter));
        assert!(!matches_skill_filter("other", &filter));
    }

    #[test]
    fn format_source_input_no_ref() {
        assert_eq!(
            format_source_input("https://github.com/a/b.git", None),
            "https://github.com/a/b.git"
        );
    }

    #[test]
    fn format_source_input_with_ref() {
        assert_eq!(
            format_source_input("https://github.com/a/b.git", Some("v2")),
            "https://github.com/a/b.git#v2"
        );
    }

    #[test]
    fn build_update_install_source_no_path_no_ref() {
        let e = sample_global(None, None);
        assert_eq!(build_update_install_source(&e), e.source_url);
    }

    #[test]
    fn build_update_install_source_with_path_trims_skill_md() {
        let e = sample_global(Some("skills/foo/SKILL.md"), None);
        assert_eq!(build_update_install_source(&e), "owner/repo/skills/foo");
    }

    #[test]
    fn build_update_install_source_with_path_and_ref() {
        let e = sample_global(Some("skills/bar/"), Some("main"));
        assert_eq!(
            build_update_install_source(&e),
            "owner/repo/skills/bar#main"
        );
    }

    #[test]
    fn build_local_update_source_with_ref() {
        let e = LocalSkillLockEntry {
            source: "owner/repo".to_owned(),
            git_ref: Some("release".to_owned()),
            source_type: "github".to_owned(),
            computed_hash: String::new(),
        };
        assert_eq!(build_local_update_source(&e), "owner/repo#release");
    }
}

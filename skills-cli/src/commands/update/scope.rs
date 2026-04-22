//! Scope resolution for `skills update`.
//!
//! The TS reference picks among Project / Global / Both via a mix of
//! explicit flags, interactive prompting, and TTY auto-detection. This
//! module encapsulates that decision tree behind a single [`resolve`]
//! entry point.

use std::io::IsTerminal;
use std::path::Path;

use clap::Args;
use miette::{IntoDiagnostic, Result};

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

/// Which scopes to visit during an update run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpdateScope {
    Project,
    Global,
    Both,
}

impl UpdateScope {
    /// Whether this scope includes the global lock.
    pub(crate) const fn includes_global(self) -> bool {
        matches!(self, Self::Global | Self::Both)
    }

    /// Whether this scope includes the project lock.
    pub(crate) const fn includes_project(self) -> bool {
        matches!(self, Self::Project | Self::Both)
    }

    /// Short label used in telemetry payloads.
    pub(crate) const fn telemetry_label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
            Self::Both => "both",
        }
    }
}

/// Resolve the effective update scope.
///
/// Matches TS `resolveUpdateScope`:
///
/// 1. When `skills` filter is set, `-g` / `-p` select single scope, else `Both`.
/// 2. Explicit `-g` and `-p` are combined into `Both`.
/// 3. Non-interactive (`-y` or no TTY): auto-detect via [`has_project_skills`].
/// 4. Otherwise prompt the user interactively.
pub(crate) async fn resolve(args: &UpdateArgs) -> Result<UpdateScope> {
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
/// Mirrors TS `hasProjectSkills`: returns `true` if either
/// `skills-lock.json` exists, or `.agents/skills/` contains at least one
/// sub-directory with a `SKILL.md` file.
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
        let is_dir = entry.file_type().await.is_ok_and(|ft| ft.is_dir());
        if !is_dir {
            continue;
        }
        if tokio::fs::try_exists(entry.path().join("SKILL.md"))
            .await
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_scope_includes_flags() {
        assert!(UpdateScope::Global.includes_global());
        assert!(!UpdateScope::Global.includes_project());
        assert!(UpdateScope::Project.includes_project());
        assert!(!UpdateScope::Project.includes_global());
        assert!(UpdateScope::Both.includes_global());
        assert!(UpdateScope::Both.includes_project());
    }

    #[test]
    fn test_update_scope_telemetry_labels() {
        assert_eq!(UpdateScope::Project.telemetry_label(), "project");
        assert_eq!(UpdateScope::Global.telemetry_label(), "global");
        assert_eq!(UpdateScope::Both.telemetry_label(), "both");
    }

    #[tokio::test]
    async fn test_has_project_skills_detects_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("skills-lock.json"), "{}")
            .await
            .unwrap();
        assert!(has_project_skills(dir.path()).await);
    }

    #[tokio::test]
    async fn test_has_project_skills_detects_agents_skills_dir() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join(".agents").join("skills").join("my-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(skill_dir.join("SKILL.md"), "# test")
            .await
            .unwrap();
        assert!(has_project_skills(dir.path()).await);
    }

    #[tokio::test]
    async fn test_has_project_skills_returns_false_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_project_skills(dir.path()).await);
    }

    #[tokio::test]
    async fn test_has_project_skills_ignores_dir_without_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join(".agents").join("skills").join("incomplete");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        assert!(!has_project_skills(dir.path()).await);
    }
}

//! Path utilities for skill installation.
//!
//! Sync, allocation-explicit computations that turn a skill name plus an
//! [`AgentConfig`] / [`InstallScope`] / cwd into the filesystem locations
//! the skill may occupy. All I/O lives in the sibling `scan` module; the
//! two concerns are deliberately kept apart so tests can reason about path
//! resolution without touching the filesystem and callers can pre-compute
//! paths on the sync side before spawning async probes.

use std::path::{Path, PathBuf};

use crate::agents::AgentRegistry;
use crate::sanitize::{candidate_slugs, sanitize_name};
use crate::types::{AGENTS_DIR, AgentConfig, InstallScope, SKILLS_SUBDIR};

/// Validate that `target_path` is within `base_path`.
///
/// Uses purely lexical normalization so the result is consistent regardless
/// of whether the paths exist on disk (important during fresh installs).
pub(super) fn is_path_safe(base_path: &Path, target_path: &Path) -> bool {
    crate::path_util::normalize_absolute(target_path)
        .starts_with(crate::path_util::normalize_absolute(base_path))
}

/// Get the canonical `.agents/skills` directory.
#[must_use]
pub fn canonical_skills_dir(scope: InstallScope, cwd: &Path) -> PathBuf {
    let base = match scope {
        InstallScope::Global => dirs::home_dir().unwrap_or_else(|| PathBuf::from("~")),
        InstallScope::Project => cwd.to_path_buf(),
    };
    base.join(AGENTS_DIR).join(SKILLS_SUBDIR)
}

/// Get the base directory for a specific agent's skills.
#[must_use]
pub fn agent_base_dir(
    agent: &AgentConfig,
    registry: &AgentRegistry,
    scope: InstallScope,
    cwd: &Path,
) -> PathBuf {
    if registry.is_universal(&agent.name) {
        return canonical_skills_dir(scope, cwd);
    }

    match scope {
        InstallScope::Global => agent
            .global_skills_dir
            .clone()
            .unwrap_or_else(|| cwd.join(&agent.skills_dir)),
        InstallScope::Project => cwd.join(&agent.skills_dir),
    }
}

/// The canonical install path for a skill (`<base>/.agents/skills/<sanitized-name>`).
#[must_use]
pub fn canonical_install_path(skill_name: &str, scope: InstallScope, cwd: &Path) -> PathBuf {
    canonical_skills_dir(scope, cwd).join(sanitize_name(skill_name))
}

/// Resolve the install directory that would hold `slug` under an agent,
/// returning `None` when the target is out of bounds or the agent has no
/// global directory in global scope.
///
/// Does not re-sanitize: callers pass in the final directory name already.
fn resolve_variant_path(
    slug: &str,
    project_skills_dir: &str,
    global_skills_dir: Option<&Path>,
    scope: InstallScope,
    cwd: &Path,
) -> Option<PathBuf> {
    let target_base = match scope {
        InstallScope::Global => global_skills_dir?.to_path_buf(),
        InstallScope::Project => cwd.join(project_skills_dir),
    };
    let target_dir = target_base.join(slug);
    is_path_safe(&target_base, &target_dir).then_some(target_dir)
}

/// Compute every on-disk location a skill may occupy under a specific agent.
///
/// Sync, pure, and allocation-explicit. The returned `Vec` holds one or two
/// [`PathBuf`]s in priority order (canonical sanitize first, then the
/// legacy TS slug if it differs). Out-of-bounds or scope-unsupported
/// combinations (e.g. global scope for an agent without a `global_skills_dir`)
/// yield an empty vec.
///
/// This is the building block callers use when they need to probe multiple
/// `(skill × agent)` pairs in parallel: compute all paths synchronously,
/// then move them into spawned tasks that call the async
/// [`crate::installer::any_path_exists`] primitive.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use skill::installer::candidate_install_paths;
/// use skill::types::InstallScope;
///
/// let cwd = Path::new("/project");
/// let paths = candidate_install_paths(
///     "My Skill!",
///     ".claude/skills",
///     None,
///     InstallScope::Project,
///     cwd,
/// );
/// assert!(!paths.is_empty());
/// assert!(paths[0].ends_with("my-skill"));
/// ```
#[must_use]
pub fn candidate_install_paths(
    skill_name: &str,
    project_skills_dir: &str,
    global_skills_dir: Option<&Path>,
    scope: InstallScope,
    cwd: &Path,
) -> Vec<PathBuf> {
    candidate_slugs(skill_name)
        .into_iter()
        .filter_map(|slug| {
            resolve_variant_path(&slug, project_skills_dir, global_skills_dir, scope, cwd)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_safe_rejects_traversal() {
        let base = Path::new("/home/user/.agents/skills");
        let evil = base.join("../../etc/passwd");
        assert!(!is_path_safe(base, &evil));
    }

    #[test]
    fn path_safe_accepts_child() {
        let base = Path::new("/home/user/.agents/skills");
        let child = base.join("my-skill");
        assert!(is_path_safe(base, &child));
    }

    #[test]
    fn path_safe_accepts_nested_child() {
        let base = Path::new("/home/user/.agents/skills");
        let nested = base.join("group/sub-skill");
        assert!(is_path_safe(base, &nested));
    }

    #[test]
    fn path_safe_rejects_sibling() {
        let base = Path::new("/home/user/.agents/skills");
        let sibling = Path::new("/home/user/.agents/other");
        assert!(!is_path_safe(base, sibling));
    }

    #[test]
    fn canonical_skills_dir_project() {
        let cwd = Path::new("/project");
        let dir = canonical_skills_dir(InstallScope::Project, cwd);
        assert_eq!(dir, PathBuf::from("/project/.agents/skills"));
    }

    #[test]
    fn canonical_install_path_sanitizes() {
        let cwd = Path::new("/project");
        let path = canonical_install_path("My Skill!", InstallScope::Project, cwd);
        assert_eq!(path, PathBuf::from("/project/.agents/skills/my-skill"));
    }

    #[test]
    fn candidate_install_paths_single_variant_for_plain_name() {
        let cwd = Path::new("/project");
        let paths = candidate_install_paths(
            "my-skill",
            ".claude/skills",
            None,
            InstallScope::Project,
            cwd,
        );
        assert_eq!(
            paths,
            vec![PathBuf::from("/project/.claude/skills/my-skill")]
        );
    }

    #[test]
    fn candidate_install_paths_emits_both_variants_for_punctuation() {
        let cwd = Path::new("/project");
        let paths = candidate_install_paths(
            "hello!world",
            ".claude/skills",
            None,
            InstallScope::Project,
            cwd,
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/project/.claude/skills/hello-world"),
                PathBuf::from("/project/.claude/skills/hello!world"),
            ]
        );
    }

    #[test]
    fn candidate_install_paths_returns_empty_without_global_dir() {
        let cwd = Path::new("/project");
        let paths = candidate_install_paths(
            "my-skill",
            ".claude/skills",
            None,
            InstallScope::Global,
            cwd,
        );
        assert!(paths.is_empty());
    }
}

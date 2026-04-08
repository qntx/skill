//! Path and name utilities for skill installation.

use std::path::{Path, PathBuf};

use crate::agents::AgentRegistry;
use crate::types::{AGENTS_DIR, AgentConfig, InstallScope, SKILLS_SUBDIR};

/// Sanitize a skill name for safe use as a directory name.
///
/// Converts to lowercase, replaces unsafe characters with hyphens, strips
/// leading/trailing dots and hyphens, and limits to 255 characters.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive hyphens
    let mut collapsed = String::with_capacity(sanitized.len());
    let mut prev_hyphen = false;
    for ch in sanitized.chars() {
        if ch == '-' {
            if !prev_hyphen {
                collapsed.push(ch);
            }
            prev_hyphen = true;
        } else {
            collapsed.push(ch);
            prev_hyphen = false;
        }
    }

    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == '-');
    if trimmed.is_empty() {
        return "unnamed-skill".to_owned();
    }
    if trimmed.len() <= 255 {
        return trimmed.to_owned();
    }
    // Truncate at a char boundary to avoid UTF-8 panic
    let mut end = 255;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_owned()
}

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

/// Get the canonical install path for a skill.
#[must_use]
pub fn get_canonical_path(skill_name: &str, scope: InstallScope, cwd: &Path) -> PathBuf {
    canonical_skills_dir(scope, cwd).join(sanitize_name(skill_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize_name("My Skill Name"), "my-skill-name");
        assert_eq!(sanitize_name("../../evil"), "evil");
        assert_eq!(sanitize_name("hello_world.v2"), "hello_world.v2");
    }

    #[test]
    fn sanitize_empty_and_dots() {
        assert_eq!(sanitize_name("..."), "unnamed-skill");
        assert_eq!(sanitize_name(""), "unnamed-skill");
        assert_eq!(sanitize_name("---"), "unnamed-skill");
    }

    #[test]
    fn sanitize_consecutive_hyphens_collapsed() {
        assert_eq!(sanitize_name("a   b   c"), "a-b-c");
        assert_eq!(sanitize_name("a---b"), "a-b");
    }

    #[test]
    fn sanitize_unicode() {
        assert_eq!(sanitize_name("日本語スキル"), "unnamed-skill");
        assert_eq!(sanitize_name("café-skill"), "caf-skill");
    }

    #[test]
    fn sanitize_truncates_at_255() {
        let long = "a".repeat(300);
        let result = sanitize_name(&long);
        assert!(result.len() <= 255);
        assert_eq!(result.len(), 255);
    }

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
    fn get_canonical_path_sanitizes() {
        let cwd = Path::new("/project");
        let path = get_canonical_path("My Skill!", InstallScope::Project, cwd);
        assert_eq!(path, PathBuf::from("/project/.agents/skills/my-skill"));
    }
}

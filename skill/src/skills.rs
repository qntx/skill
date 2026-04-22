//! Skill discovery and `SKILL.md` parsing.
//!
//! This module handles finding and parsing skills from the filesystem,
//! including YAML frontmatter extraction and multi-strategy directory
//! scanning.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{Result, SkillError};
use crate::sanitize::sanitize_metadata;
use crate::types::{DiscoverOptions, Skill};

/// Directories to skip during recursive skill search.
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// Maximum recursion depth for skill directory scanning.
const MAX_DEPTH: usize = 5;

/// Check whether `INSTALL_INTERNAL_SKILLS` is enabled.
#[must_use]
pub fn should_install_internal_skills() -> bool {
    std::env::var("INSTALL_INTERNAL_SKILLS")
        .ok()
        .is_some_and(|v| v == "1" || v == "true")
}

/// Parse a `SKILL.md` file and return a [`Skill`] if valid.
///
/// Returns `Ok(None)` when the file exists but lacks required frontmatter
/// fields (`name` and `description`), or when the skill is internal and
/// internal skill installation is not enabled.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub async fn parse_skill_md(skill_md_path: &Path, include_internal: bool) -> Result<Option<Skill>> {
    let content = match tokio::fs::read_to_string(skill_md_path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(SkillError::io(skill_md_path, e)),
    };

    let Some((frontmatter, _body)) = extract_frontmatter(&content) else {
        return Ok(None);
    };

    let Ok(data) = serde_yml::from_str::<serde_yml::Value>(frontmatter) else {
        return Ok(None);
    };

    let name = data
        .get("name")
        .and_then(serde_yml::Value::as_str)
        .map(sanitize_metadata);
    let description = data
        .get("description")
        .and_then(serde_yml::Value::as_str)
        .map(sanitize_metadata);

    let (Some(name), Some(description)) = (name, description) else {
        return Ok(None);
    };

    let is_internal = data
        .get("metadata")
        .and_then(|m| m.get("internal"))
        .and_then(serde_yml::Value::as_bool)
        .unwrap_or(false);

    if is_internal && !should_install_internal_skills() && !include_internal {
        return Ok(None);
    }

    let metadata = data.get("metadata").and_then(|m| {
        serde_yml::from_value::<std::collections::HashMap<String, serde_yml::Value>>(m.clone()).ok()
    });

    let dir = skill_md_path
        .parent()
        .unwrap_or(skill_md_path)
        .to_path_buf();

    Ok(Some(Skill {
        name,
        description,
        path: dir,
        raw_content: Some(content),
        plugin_name: None,
        metadata,
    }))
}

/// Extract YAML frontmatter delimited by `---`.
///
/// Returns `(frontmatter, body)` or `None` if no valid delimiters are found.
/// This is the public entry point used by providers.
#[must_use]
pub fn extract_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end = after_first.find("\n---")?;
    let frontmatter = &after_first[..end];
    let body_start = end + 4;
    let body = after_first.get(body_start..).unwrap_or("");
    Some((frontmatter.trim(), body))
}

/// Check whether a directory contains a `SKILL.md` file.
async fn has_skill_md(dir: &Path) -> bool {
    tokio::fs::try_exists(dir.join("SKILL.md"))
        .await
        .unwrap_or(false)
}

/// Recursively find directories containing `SKILL.md`.
async fn find_skill_dirs(dir: &Path, depth: usize) -> Vec<PathBuf> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }

    let mut results = Vec::new();

    if has_skill_md(dir).await {
        results.push(dir.to_path_buf());
    }

    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return results;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(file_type) = entry.file_type().await else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }
        let child_path = entry.path();
        let sub = Box::pin(find_skill_dirs(&child_path, depth + 1));
        results.extend(sub.await);
    }

    results
}

/// Validate that a resolved subpath stays within the base directory.
///
/// Uses lexical normalization (no filesystem access) to ensure consistent
/// behavior across environments where paths may not exist.
#[must_use]
pub fn is_subpath_safe(base_path: &Path, subpath: &str) -> bool {
    // Check for obvious path traversal patterns first
    let normalized_subpath = subpath.replace('\\', "/");
    for segment in normalized_subpath.split('/') {
        if segment == ".." {
            return false;
        }
    }

    // Use lexical normalization for the full path check
    let target = base_path.join(subpath);
    let normalized_base = crate::path_util::normalize_absolute(base_path);
    let normalized_target = crate::path_util::normalize_absolute(&target);
    normalized_target.starts_with(&normalized_base)
}

/// Discover skills under `base_path` (optionally scoped by `subpath`).
///
/// The discovery strategy mirrors the `TypeScript` reference:
/// 1. Check if the search path itself has a `SKILL.md` (single root skill).
/// 2. Scan priority directories (common skill locations for each agent),
///    including paths declared in plugin manifests.
/// 3. Fall back to recursive search if nothing was found, or if `full_depth`
///    is enabled.
///
/// # Errors
///
/// Returns an error if the `subpath` escapes the `base_path`.
#[allow(
    clippy::cognitive_complexity,
    reason = "multi-strategy discovery with fallback logic"
)]
pub async fn discover_skills(
    base_path: &Path,
    subpath: Option<&str>,
    options: &DiscoverOptions,
) -> Result<Vec<Skill>> {
    if let Some(sp) = subpath
        && !is_subpath_safe(base_path, sp)
    {
        return Err(SkillError::PathTraversal {
            context: "subpath",
            path: sp.to_owned(),
        });
    }

    let search_path = subpath.map_or_else(|| base_path.to_path_buf(), |sp| base_path.join(sp));

    let mut skills = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    let include_internal = options.include_internal;

    // Map skills to their parent plugin via manifest groupings.
    let plugin_groupings = crate::plugin_manifest::groupings(&search_path).await;

    let enhance_skill = |mut skill: Skill| -> Skill {
        let resolved = std::path::absolute(&skill.path).unwrap_or_else(|_| skill.path.clone());
        if let Some(name) = plugin_groupings.get(&resolved) {
            skill.plugin_name = Some(name.clone());
        }
        skill
    };

    // 1. Root SKILL.md
    if has_skill_md(&search_path).await
        && let Some(skill) = parse_skill_md(&search_path.join("SKILL.md"), include_internal).await?
    {
        let skill = enhance_skill(skill);
        seen_names.insert(skill.name.clone());
        skills.push(skill);
        if !options.full_depth {
            return Ok(skills);
        }
    }

    // 2. Priority search directories + plugin manifest paths
    let mut priority_dirs = build_priority_dirs(&search_path);
    priority_dirs.extend(crate::plugin_manifest::search_dirs(&search_path).await);

    for dir in &priority_dirs {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let skill_dir = entry.path();
            if !has_skill_md(&skill_dir).await {
                continue;
            }
            if let Some(skill) =
                parse_skill_md(&skill_dir.join("SKILL.md"), include_internal).await?
                && seen_names.insert(skill.name.clone())
            {
                skills.push(enhance_skill(skill));
            }
        }
    }

    // 3. Recursive fallback
    if skills.is_empty() || options.full_depth {
        for skill_dir in find_skill_dirs(&search_path, 0).await {
            if let Some(skill) =
                parse_skill_md(&skill_dir.join("SKILL.md"), include_internal).await?
                && seen_names.insert(skill.name.clone())
            {
                skills.push(enhance_skill(skill));
            }
        }
    }

    Ok(skills)
}

/// Build the list of priority search directories (matching the TS reference).
fn build_priority_dirs(search_path: &Path) -> Vec<PathBuf> {
    let sp = search_path;
    vec![
        sp.to_path_buf(),
        sp.join("skills"),
        sp.join("skills/.curated"),
        sp.join("skills/.experimental"),
        sp.join("skills/.system"),
        sp.join(".agents/skills"),
        sp.join(".claude/skills"),
        sp.join(".cline/skills"),
        sp.join(".codebuddy/skills"),
        sp.join(".codex/skills"),
        sp.join(".commandcode/skills"),
        sp.join(".continue/skills"),
        sp.join(".github/skills"),
        sp.join(".goose/skills"),
        sp.join(".iflow/skills"),
        sp.join(".junie/skills"),
        sp.join(".kilocode/skills"),
        sp.join(".kiro/skills"),
        sp.join(".mux/skills"),
        sp.join(".neovate/skills"),
        sp.join(".opencode/skills"),
        sp.join(".openhands/skills"),
        sp.join(".pi/skills"),
        sp.join(".qoder/skills"),
        sp.join(".roo/skills"),
        sp.join(".trae/skills"),
        sp.join(".windsurf/skills"),
        sp.join(".zencoder/skills"),
    ]
}

/// Filter skills by a list of names (case-insensitive match against both
/// [`Skill::name`] and [`Skill::display_name`]).
#[must_use]
pub fn filter_skills(skills: &[Skill], input_names: &[String]) -> Vec<Skill> {
    let normalized: Vec<String> = input_names.iter().map(|n| n.to_lowercase()).collect();
    skills
        .iter()
        .filter(|skill| {
            let name = skill.name.to_lowercase();
            let display = skill.display_name().to_lowercase();
            normalized
                .iter()
                .any(|input| *input == name || *input == display)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_frontmatter_basic() {
        let content = "---\nname: test\ndescription: hello\n---\n# Body";
        let (fm, body) = extract_frontmatter(content).expect("should parse");
        assert_eq!(fm, "name: test\ndescription: hello");
        assert!(body.contains("# Body"));
    }

    #[test]
    fn extract_frontmatter_missing() {
        assert!(extract_frontmatter("no frontmatter here").is_none());
    }

    #[test]
    fn extract_frontmatter_leading_whitespace() {
        let content = "  \n---\nname: x\n---\nbody";
        let (fm, _body) = extract_frontmatter(content).expect("should handle leading whitespace");
        assert_eq!(fm, "name: x");
    }

    #[test]
    fn extract_frontmatter_no_body() {
        let content = "---\nname: x\n---";
        let (fm, body) = extract_frontmatter(content).expect("should parse without body");
        assert_eq!(fm, "name: x");
        assert!(body.is_empty());
    }

    #[test]
    fn extract_frontmatter_empty_frontmatter() {
        let content = "---\n---\nbody";
        let (fm, _body) = extract_frontmatter(content).expect("should parse empty frontmatter");
        assert!(fm.is_empty());
    }

    #[test]
    fn extract_frontmatter_no_closing() {
        assert!(extract_frontmatter("---\nname: x\nno closing").is_none());
    }

    #[test]
    fn subpath_safe_valid() {
        let base = Path::new("/tmp/repo");
        assert!(is_subpath_safe(base, "skills/my-skill"));
    }

    #[test]
    fn subpath_safe_traversal_blocked() {
        let base = Path::new("/tmp/repo");
        assert!(!is_subpath_safe(base, "../../etc/passwd"));
    }

    #[tokio::test]
    async fn parse_skill_md_valid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_md = dir.path().join("SKILL.md");
        tokio::fs::write(
            &skill_md,
            "---\nname: test-skill\ndescription: A test skill\n---\n# Instructions\nDo things.",
        )
        .await
        .unwrap();

        let result = parse_skill_md(&skill_md, false).await.unwrap();
        let skill = result.expect("should parse valid SKILL.md");
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert!(skill.raw_content.is_some());
    }

    #[tokio::test]
    async fn parse_skill_md_missing_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_md = dir.path().join("SKILL.md");
        tokio::fs::write(&skill_md, "---\ndescription: no name\n---\nbody")
            .await
            .unwrap();

        let result = parse_skill_md(&skill_md, false).await.unwrap();
        assert!(result.is_none(), "should return None when name is missing");
    }

    #[tokio::test]
    async fn parse_skill_md_not_found() {
        let result = parse_skill_md(Path::new("/nonexistent/SKILL.md"), false).await;
        assert!(
            result.unwrap().is_none(),
            "should return Ok(None) for missing file"
        );
    }
}

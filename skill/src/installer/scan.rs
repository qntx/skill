//! Scanning and querying installed skills.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::paths::{canonical_skills_dir, is_path_safe, sanitize_name};
use crate::agents::AgentRegistry;
use crate::error::Result;
use crate::skills::parse_skill_md;
use crate::types::{AgentConfig, AgentId, InstallScope, InstalledSkill, ListOptions};

/// Build a deduplicated list of directories to scan for a given scope.
fn build_scan_dirs(
    registry: &AgentRegistry,
    agents_to_check: &[&AgentId],
    scope: InstallScope,
    canonical: &Path,
    cwd: &Path,
) -> Vec<(PathBuf, Option<AgentId>)> {
    let mut scan_dirs = vec![(canonical.to_path_buf(), None)];
    for agent_id in agents_to_check {
        let Some(config) = registry.get(agent_id) else {
            continue;
        };
        if scope == InstallScope::Global && config.global_skills_dir.is_none() {
            continue;
        }
        let agent_dir = match scope {
            InstallScope::Global => config
                .global_skills_dir
                .clone()
                .unwrap_or_else(|| cwd.join(&config.skills_dir)),
            InstallScope::Project => cwd.join(&config.skills_dir),
        };
        if !scan_dirs.iter().any(|(d, _)| *d == agent_dir) {
            scan_dirs.push((agent_dir, Some((*agent_id).clone())));
        }
    }
    scan_dirs
}

/// Check which agents have a skill installed and add them to its agent list.
async fn detect_agents_for_skill(
    installed: &mut InstalledSkill,
    agents_to_check: &[&AgentId],
    registry: &AgentRegistry,
    scope: InstallScope,
    cwd: &Path,
) {
    for aid in agents_to_check {
        if installed.agents.contains(aid) {
            continue;
        }
        if let Some(config) = registry.get(aid)
            && is_skill_installed(&installed.name, config, scope, cwd).await
        {
            installed.agents.push((*aid).clone());
        }
    }
}

/// List all installed skills from canonical and agent-specific directories.
///
/// Matches the Vercel TS `listInstalledSkills`: detects which agents are
/// actually installed first, then only scans canonical + those agent
/// directories to avoid unnecessary I/O.
///
/// # Errors
///
/// Returns an error on I/O or parse failure.
pub async fn list_installed_skills(
    registry: &AgentRegistry,
    options: &ListOptions,
) -> Result<Vec<InstalledSkill>> {
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let mut skills_map: HashMap<String, InstalledSkill> = HashMap::new();

    let scopes: Vec<InstallScope> = options.scope.map_or_else(
        || vec![InstallScope::Project, InstallScope::Global],
        |s| vec![s],
    );

    let detected = registry.detect_installed().await;
    let agents_to_check: Vec<&AgentId> = if options.agent_filter.is_empty() {
        detected.iter().collect()
    } else {
        detected
            .iter()
            .filter(|id| options.agent_filter.contains(id))
            .collect()
    };

    for scope in &scopes {
        let canonical = canonical_skills_dir(*scope, &cwd);
        let scan_dirs = build_scan_dirs(registry, &agents_to_check, *scope, &canonical, &cwd);

        for (dir, agent_id) in &scan_dirs {
            if let Some(aid) = agent_id {
                scan_skills_dir_for_agent(dir, *scope, aid, &mut skills_map).await;
            } else {
                scan_skills_dir(&canonical, *scope, &mut skills_map).await;
            }
        }

        // After scanning all dirs for this scope, detect which agents
        // have each canonical skill installed.
        let scope_prefix = format!("{scope:?}:");
        let keys: Vec<String> = skills_map
            .keys()
            .filter(|k| k.starts_with(&scope_prefix))
            .cloned()
            .collect();
        for key in keys {
            if let Some(installed) = skills_map.get_mut(&key) {
                detect_agents_for_skill(installed, &agents_to_check, registry, *scope, &cwd).await;
            }
        }
    }

    let mut result: Vec<InstalledSkill> = skills_map.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

/// Scan a directory for installed skills.
async fn scan_skills_dir(
    dir: &Path,
    scope: InstallScope,
    map: &mut HashMap<String, InstalledSkill>,
) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() && !ft.is_symlink() {
            continue;
        }
        let skill_dir = entry.path();
        let skill_md = skill_dir.join("SKILL.md");
        if !tokio::fs::try_exists(&skill_md).await.unwrap_or(false) {
            continue;
        }
        if let Ok(Some(skill)) = parse_skill_md(&skill_md, false).await {
            let key = format!("{scope:?}:{}", skill.name);
            map.entry(key).or_insert_with(|| InstalledSkill {
                name: skill.name,
                description: skill.description,
                path: skill_dir.clone(),
                canonical_path: Some(skill_dir),
                scope,
                agents: Vec::new(),
            });
        }
    }
}

/// Scan a single agent's skills directory and merge results.
async fn scan_skills_dir_for_agent(
    dir: &Path,
    scope: InstallScope,
    agent_id: &AgentId,
    map: &mut HashMap<String, InstalledSkill>,
) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() && !ft.is_symlink() {
            continue;
        }
        let skill_dir = entry.path();
        let skill_md = skill_dir.join("SKILL.md");
        if !tokio::fs::try_exists(&skill_md).await.unwrap_or(false) {
            continue;
        }
        if let Ok(Some(skill)) = parse_skill_md(&skill_md, false).await {
            let key = format!("{scope:?}:{}", skill.name);
            let installed = map.entry(key).or_insert_with(|| InstalledSkill {
                name: skill.name,
                description: skill.description,
                path: skill_dir,
                canonical_path: None,
                scope,
                agents: Vec::new(),
            });
            if !installed.agents.contains(agent_id) {
                installed.agents.push(agent_id.clone());
            }
        }
    }
}

/// Resolve the install directory that would hold `slug` under an agent's
/// skills dir, returning `None` when the target is out of bounds or the
/// agent has no global directory in global scope.
///
/// Unlike the public `sanitize_name` helper, this does **not** re-sanitize
/// the slug — callers are expected to have materialised the final directory
/// name already (via `sanitize_name`, `legacy_skill_slug`, or similar).
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

/// On-disk directory-name variants a skill may occupy.
///
/// The primary candidate is always `sanitize_name(skill_name)` (the
/// convention this crate installs under). A second **legacy** candidate is
/// appended when it differs — matching the TS reference
/// `installer.ts:972-981 possibleNames`, which accommodates:
///
/// - skills installed by older tooling with looser slug rules,
/// - handcrafted directories (e.g. `"Git Review"` under `.cursor/skills/`),
/// - forks that ship with their own slug variants.
///
/// Returns at least one element and never duplicates.
fn candidate_slugs(skill_name: &str) -> Vec<String> {
    let sanitized = sanitize_name(skill_name);
    let legacy = legacy_skill_slug(skill_name);
    if legacy.is_empty() || legacy == sanitized {
        vec![sanitized]
    } else {
        vec![sanitized, legacy]
    }
}

/// Legacy slug algorithm preserved from the TS reference.
///
/// Mirrors the TS snippet embedded in `installer.ts:972-981`:
///
/// ```text
/// name.toLowerCase()
///     .replace(/\s+/g, '-')
///     .replace(/[\/\\:\0]/g, '')
/// ```
///
/// Runs of ASCII whitespace collapse into a single `-`, path separators
/// (`/`, `\`), colon, and NUL are dropped, and every other character —
/// including punctuation like `!` and `.` — is kept as-is after Unicode
/// lowercasing. This is intentionally more permissive than the stricter
/// `sanitize_name` used for fresh installs.
fn legacy_skill_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_space = false;
    for ch in name.chars() {
        if ch.is_ascii_whitespace() {
            if !in_space {
                out.push('-');
                in_space = true;
            }
            continue;
        }
        in_space = false;
        if matches!(ch, '/' | '\\' | ':' | '\0') {
            continue;
        }
        // `char::to_lowercase` returns an iterator because some code points
        // lowercase to multiple chars (e.g. `İ` → `i\u{307}`), matching JS
        // `String#toLowerCase` output byte-for-byte.
        out.extend(ch.to_lowercase());
    }
    out
}

/// Check if a skill is installed for an agent.
///
/// Probes both the canonical sanitize of `skill_name` and the legacy TS
/// slug variant (see the private `candidate_slugs` helper) in order,
/// returning true on the first existing path. This lets skills installed
/// under legacy / handcrafted directory names still be detected.
pub async fn is_skill_installed(
    skill_name: &str,
    agent: &AgentConfig,
    scope: InstallScope,
    cwd: &Path,
) -> bool {
    for slug in candidate_slugs(skill_name) {
        if let Some(target) = resolve_variant_path(
            &slug,
            &agent.skills_dir,
            agent.global_skills_dir.as_deref(),
            scope,
            cwd,
        ) && tokio::fs::try_exists(&target).await.unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// Check if a skill is installed — owned-value variant for `tokio::spawn`.
///
/// Accepts fully owned values instead of `&AgentConfig` so the returned
/// future is `Send + 'static`, safe for parallel spawning. Honours the same
/// candidate-slug probing as [`is_skill_installed`] (canonical sanitize
/// first, then the legacy TS slug).
pub async fn is_skill_installed_owned(
    skill_name: String,
    project_skills_dir: String,
    global_skills_dir: Option<PathBuf>,
    scope: InstallScope,
    cwd: PathBuf,
) -> bool {
    for slug in candidate_slugs(&skill_name) {
        if let Some(target) = resolve_variant_path(
            &slug,
            &project_skills_dir,
            global_skills_dir.as_deref(),
            scope,
            &cwd,
        ) && tokio::fs::try_exists(&target).await.unwrap_or(false)
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
    fn candidate_slugs_returns_single_entry_when_variants_align() {
        // Plain names produce identical sanitize + legacy output.
        assert_eq!(candidate_slugs("my-skill"), vec!["my-skill"]);
        assert_eq!(candidate_slugs("deploy"), vec!["deploy"]);
    }

    #[test]
    fn candidate_slugs_adds_legacy_variant_when_punctuation_differs() {
        // `!` is mapped to `-` by sanitize_name but kept by TS legacy slug.
        let variants = candidate_slugs("hello!world");
        assert_eq!(variants, vec!["hello-world", "hello!world"]);
    }

    #[test]
    fn candidate_slugs_adds_legacy_variant_when_path_separator_differs() {
        // `/` is mapped to `-` by sanitize_name but dropped by TS legacy slug.
        let variants = candidate_slugs("scope/name");
        assert_eq!(variants, vec!["scope-name", "scopename"]);
    }

    #[test]
    fn legacy_slug_collapses_whitespace_runs_to_single_hyphen() {
        assert_eq!(legacy_skill_slug("Git  Review"), "git-review");
        assert_eq!(legacy_skill_slug("a\t  b"), "a-b");
    }

    #[test]
    fn legacy_slug_keeps_generic_punctuation() {
        // TS algorithm only strips `\s+` (→ `-`) and `[\/\\:\0]` (→ ``).
        // Everything else — including `!`, `.`, `_` — survives.
        assert_eq!(legacy_skill_slug("hello!world"), "hello!world");
        assert_eq!(legacy_skill_slug("a.b"), "a.b");
        assert_eq!(legacy_skill_slug("x_y"), "x_y");
    }

    #[test]
    fn legacy_slug_drops_path_separators_and_nul() {
        assert_eq!(legacy_skill_slug("scope/name"), "scopename");
        assert_eq!(legacy_skill_slug("win\\path"), "winpath");
        assert_eq!(legacy_skill_slug("k:v"), "kv");
        assert_eq!(legacy_skill_slug("a\0b"), "ab");
    }

    #[test]
    fn legacy_slug_lowercases_full_unicode() {
        // `.to_lowercase()` applies Unicode rules like TS `toLowerCase`.
        assert_eq!(legacy_skill_slug("CAFÉ"), "café");
        assert_eq!(legacy_skill_slug("Δέλτα"), "δέλτα");
    }
}

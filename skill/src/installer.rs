//! Skill installation, removal, and listing.
//!
//! Handles copying or symlinking skills into agent-specific directories,
//! with a canonical `.agents/skills/` location as the single source of truth.

use std::path::{Path, PathBuf};

use crate::agents::AgentRegistry;
use crate::error::{Error, Result};
use crate::skills::parse_skill_md;
use crate::types::{
    AGENTS_DIR, AgentConfig, AgentId, InstallMode, InstallResult, InstallScope, InstalledSkill,
    ListOptions, SKILLS_SUBDIR, Skill,
};

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
    let result = if trimmed.is_empty() {
        "unnamed-skill"
    } else if trimmed.len() > 255 {
        &trimmed[..255]
    } else {
        trimmed
    };

    result.to_owned()
}

/// Validate that `target_path` is within `base_path`.
fn is_path_safe(base_path: &Path, target_path: &Path) -> bool {
    crate::path_util::normalize(target_path).starts_with(crate::path_util::normalize(base_path))
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

/// Install a local skill for a single agent.
///
/// # Errors
///
/// Returns an error on I/O failure or path-safety violation.
pub async fn install_skill_for_agent(
    skill: &Skill,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &crate::types::InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(Error::AgentUnsupported {
            agent: agent.display_name.clone(),
            operation: "global skill installation",
        });
    }

    let skill_name = sanitize_name(&skill.name);
    let canonical_base = canonical_skills_dir(scope, &cwd);
    let canonical_dir = canonical_base.join(&skill_name);
    let agent_base = agent_base_dir(agent, registry, scope, &cwd);
    let agent_dir = agent_base.join(&skill_name);

    if !is_path_safe(&canonical_base, &canonical_dir) || !is_path_safe(&agent_base, &agent_dir) {
        return Err(Error::PathTraversal {
            context: "skill name",
            path: skill_name,
        });
    }

    let mode = options.mode;

    // Copy mode: copy directly to agent location
    if mode == InstallMode::Copy {
        clean_and_create(&agent_dir).await?;
        copy_directory(&skill.path, &agent_dir).await?;
        return Ok(InstallResult {
            path: agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    // Symlink mode: copy to canonical, symlink from agent dir
    clean_and_create(&canonical_dir).await?;
    copy_directory(&skill.path, &canonical_dir).await?;

    if scope == InstallScope::Global && registry.is_universal(&agent.name) {
        return Ok(InstallResult {
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: InstallMode::Symlink,
            symlink_failed: false,
        });
    }

    let symlink_ok = create_symlink(&canonical_dir, &agent_dir).await;
    if !symlink_ok {
        clean_and_create(&agent_dir).await?;
        copy_directory(&skill.path, &agent_dir).await?;
    }

    Ok(InstallResult {
        path: agent_dir,
        canonical_path: Some(canonical_dir),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    })
}

/// Install a remote skill (single `SKILL.md` content) for an agent.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn install_remote_skill_content(
    install_name: &str,
    content: &str,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &crate::types::InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(Error::AgentUnsupported {
            agent: agent.display_name.clone(),
            operation: "global skill installation",
        });
    }

    let skill_name = sanitize_name(install_name);
    let canonical_base = canonical_skills_dir(scope, &cwd);
    let canonical_dir = canonical_base.join(&skill_name);
    let agent_base = agent_base_dir(agent, registry, scope, &cwd);
    let agent_dir = agent_base.join(&skill_name);

    if !is_path_safe(&canonical_base, &canonical_dir) || !is_path_safe(&agent_base, &agent_dir) {
        return Err(Error::PathTraversal {
            context: "skill name",
            path: skill_name,
        });
    }

    let mode = options.mode;

    if mode == InstallMode::Copy {
        clean_and_create(&agent_dir).await?;
        tokio::fs::write(agent_dir.join("SKILL.md"), content)
            .await
            .map_err(|e| Error::io(&agent_dir, e))?;
        return Ok(InstallResult {
            path: agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&canonical_dir).await?;
    tokio::fs::write(canonical_dir.join("SKILL.md"), content)
        .await
        .map_err(|e| Error::io(&canonical_dir, e))?;

    if scope == InstallScope::Global && registry.is_universal(&agent.name) {
        return Ok(InstallResult {
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: InstallMode::Symlink,
            symlink_failed: false,
        });
    }

    let symlink_ok = create_symlink(&canonical_dir, &agent_dir).await;
    if !symlink_ok {
        clean_and_create(&agent_dir).await?;
        tokio::fs::write(agent_dir.join("SKILL.md"), content)
            .await
            .map_err(|e| Error::io(&agent_dir, e))?;
    }

    Ok(InstallResult {
        path: agent_dir,
        canonical_path: Some(canonical_dir),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    })
}

/// Install a well-known skill with multiple files for an agent.
///
/// # Errors
///
/// Returns an error on I/O failure.
#[allow(clippy::implicit_hasher)]
pub async fn install_wellknown_skill_files(
    install_name: &str,
    files: &std::collections::HashMap<String, String>,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &crate::types::InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(Error::AgentUnsupported {
            agent: agent.display_name.clone(),
            operation: "global skill installation",
        });
    }

    let skill_name = sanitize_name(install_name);
    let canonical_base = canonical_skills_dir(scope, &cwd);
    let canonical_dir = canonical_base.join(&skill_name);
    let agent_base = agent_base_dir(agent, registry, scope, &cwd);
    let agent_dir = agent_base.join(&skill_name);

    if !is_path_safe(&canonical_base, &canonical_dir) || !is_path_safe(&agent_base, &agent_dir) {
        return Err(Error::PathTraversal {
            context: "skill name",
            path: skill_name,
        });
    }

    let write_files = |dir: &Path| {
        let dir = dir.to_path_buf();
        let files = files.clone();
        async move {
            for (file_path, content) in &files {
                let full = dir.join(file_path);
                if !is_path_safe(&dir, &full) {
                    continue;
                }
                if let Some(parent) = full.parent()
                    && parent != dir
                {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| Error::io(parent, e))?;
                }
                tokio::fs::write(&full, content)
                    .await
                    .map_err(|e| Error::io(&full, e))?;
            }
            Ok::<(), Error>(())
        }
    };

    let mode = options.mode;

    if mode == InstallMode::Copy {
        clean_and_create(&agent_dir).await?;
        write_files(&agent_dir).await?;
        return Ok(InstallResult {
            path: agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&canonical_dir).await?;
    write_files(&canonical_dir).await?;

    if scope == InstallScope::Global && registry.is_universal(&agent.name) {
        return Ok(InstallResult {
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: InstallMode::Symlink,
            symlink_failed: false,
        });
    }

    let symlink_ok = create_symlink(&canonical_dir, &agent_dir).await;
    if !symlink_ok {
        clean_and_create(&agent_dir).await?;
        write_files(&agent_dir).await?;
    }

    Ok(InstallResult {
        path: agent_dir,
        canonical_path: Some(canonical_dir),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    })
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

    let mut skills_map: std::collections::HashMap<String, InstalledSkill> =
        std::collections::HashMap::new();

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

        // Build deduplicated list of directories to scan, mirroring the TS
        // approach: canonical first, then each detected agent's directory.
        let mut scan_dirs: Vec<(PathBuf, Option<AgentId>)> = Vec::new();
        scan_dirs.push((canonical.clone(), None));

        for agent_id in &agents_to_check {
            if let Some(config) = registry.get(agent_id) {
                if *scope == InstallScope::Global && config.global_skills_dir.is_none() {
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
        }

        for (dir, agent_id) in &scan_dirs {
            if let Some(aid) = agent_id {
                scan_skills_dir_for_agent(dir, *scope, aid, &mut skills_map).await;
            } else {
                scan_skills_dir(&canonical, *scope, &mut skills_map).await;

                // For skills found in the canonical dir, check which of the
                // detected agents also have them installed.
                for (key, installed) in &mut skills_map {
                    if !key.starts_with(&format!("{scope:?}:")) {
                        continue;
                    }
                    for aid in &agents_to_check {
                        if installed.agents.contains(aid) {
                            continue;
                        }
                        if let Some(config) = registry.get(aid)
                            && is_skill_installed(&installed.name, config, *scope, &cwd).await
                        {
                            installed.agents.push((*aid).clone());
                        }
                    }
                }
            }
        }
    }

    let mut result: Vec<InstalledSkill> = skills_map.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

async fn scan_skills_dir(
    dir: &Path,
    scope: InstallScope,
    map: &mut std::collections::HashMap<String, InstalledSkill>,
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
                canonical_path: skill_dir,
                scope,
                agents: Vec::new(),
            });
        }
    }
}

async fn scan_skills_dir_for_agent(
    dir: &Path,
    scope: InstallScope,
    agent_id: &AgentId,
    map: &mut std::collections::HashMap<String, InstalledSkill>,
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
                path: skill_dir.clone(),
                canonical_path: skill_dir,
                scope,
                agents: Vec::new(),
            });
            if !installed.agents.contains(agent_id) {
                installed.agents.push(agent_id.clone());
            }
        }
    }
}

/// Check if a skill is installed for an agent.
pub async fn is_skill_installed(
    skill_name: &str,
    agent: &AgentConfig,
    scope: InstallScope,
    cwd: &Path,
) -> bool {
    let sanitized = sanitize_name(skill_name);
    let target_base = match scope {
        InstallScope::Global => match &agent.global_skills_dir {
            Some(d) => d.clone(),
            None => return false,
        },
        InstallScope::Project => cwd.join(&agent.skills_dir),
    };
    let skill_dir = target_base.join(sanitized);
    if !is_path_safe(&target_base, &skill_dir) {
        return false;
    }
    tokio::fs::try_exists(&skill_dir).await.unwrap_or(false)
}

/// Get the canonical install path for a skill.
#[must_use]
pub fn get_canonical_path(skill_name: &str, scope: InstallScope, cwd: &Path) -> PathBuf {
    canonical_skills_dir(scope, cwd).join(sanitize_name(skill_name))
}

async fn clean_and_create(path: &Path) -> Result<()> {
    let _ = tokio::fs::remove_dir_all(path).await;
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| Error::io(path, e))
}

const EXCLUDE_FILES: &[&str] = &["metadata.json"];
const EXCLUDE_DIRS: &[&str] = &[".git"];

async fn copy_directory(src: &Path, dest: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| Error::io(dest, e))?;

    let mut entries = tokio::fs::read_dir(src)
        .await
        .map_err(|e| Error::io(src, e))?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| Error::io(src, e))? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        let ft = entry.file_type().await.map_err(|e| Error::io(src, e))?;

        if ft.is_dir() {
            if EXCLUDE_DIRS.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let sub_dest = dest.join(&name);
            Box::pin(copy_directory(&entry.path(), &sub_dest)).await?;
        } else if ft.is_symlink() {
            // Dereference symlinks: copy the target file content, matching
            // the TS `cp(src, dest, { dereference: true })` behavior.
            // Skip broken symlinks that can't be resolved.
            if EXCLUDE_FILES.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let src_path = entry.path();
            let dest_file = dest.join(&name);
            // Follow the symlink chain via metadata (not symlink_metadata)
            match tokio::fs::metadata(&src_path).await {
                Ok(meta) if meta.is_dir() => {
                    Box::pin(copy_directory(&src_path, &dest_file)).await?;
                }
                Ok(_) => {
                    let _ = tokio::fs::copy(&src_path, &dest_file).await;
                }
                Err(_) => {
                    tracing::warn!("Skipping broken symlink: {}", src_path.display());
                }
            }
        } else {
            if EXCLUDE_FILES.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let dest_file = dest.join(&name);
            tokio::fs::copy(entry.path(), &dest_file)
                .await
                .map_err(|e| Error::io(&dest_file, e))?;
        }
    }

    Ok(())
}

/// Resolve a path's parent directory through symlinks, keeping the final
/// component.  This handles the case where a parent directory (e.g.
/// `~/.claude/skills`) is itself a symlink to another location (e.g.
/// `~/.agents/skills`).  Computing relative paths from the un-resolved
/// symlink path would produce broken symlinks.
async fn resolve_parent_symlinks(path: &Path) -> PathBuf {
    let resolved = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(dir) = resolved.parent() else {
        return resolved;
    };
    let base = resolved.file_name().unwrap_or_default().to_os_string();
    tokio::fs::canonicalize(dir)
        .await
        .map_or(resolved, |real_dir| real_dir.join(base))
}

/// Create a symlink (or junction on Windows). Returns `true` on success.
///
/// Mirrors the Vercel TS `createSymlink` logic:
///   1. Resolve both paths through real path to detect same-location.
///   2. Resolve parent symlinks to avoid broken relative links.
///   3. Remove stale symlinks / directories before creating.
///   4. Use relative paths on unix, junctions on Windows.
async fn create_symlink(target: &Path, link_path: &Path) -> bool {
    let resolved_target = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    let resolved_link = std::path::absolute(link_path).unwrap_or_else(|_| link_path.to_path_buf());

    // Check if both resolve to the same real path (skip creating symlink).
    let real_target = tokio::fs::canonicalize(&resolved_target)
        .await
        .unwrap_or_else(|_| resolved_target.clone());
    let real_link = tokio::fs::canonicalize(&resolved_link)
        .await
        .unwrap_or_else(|_| resolved_link.clone());
    if real_target == real_link {
        return true;
    }

    // Also check with parent symlinks resolved.
    let real_target_parent = resolve_parent_symlinks(target).await;
    let real_link_parent = resolve_parent_symlinks(link_path).await;
    if real_target_parent == real_link_parent {
        return true;
    }

    // Handle existing entry at link_path.
    if let Ok(meta) = tokio::fs::symlink_metadata(link_path).await {
        if meta.is_symlink() {
            #[cfg(unix)]
            if let Ok(existing) = tokio::fs::read_link(link_path).await {
                let existing_abs = if existing.is_relative() {
                    link_path.parent().unwrap_or(Path::new(".")).join(&existing)
                } else {
                    existing
                };
                let existing_resolved = std::path::absolute(&existing_abs).unwrap_or(existing_abs);
                if existing_resolved == resolved_target {
                    return true;
                }
            }
            let _ = tokio::fs::remove_file(link_path).await;
        } else {
            let _ = tokio::fs::remove_dir_all(link_path).await;
        }
    } else {
        // ELOOP (circular symlink) or ENOENT — try force-remove just in case.
        let _ = tokio::fs::remove_file(link_path).await;
    }

    // Ensure parent directory exists.
    if let Some(parent) = link_path.parent()
        && tokio::fs::create_dir_all(parent).await.is_err()
    {
        return false;
    }

    #[cfg(unix)]
    {
        // Use a relative symlink target, computed from the real (resolved)
        // parent of the link path, matching the TS implementation.
        let real_link_dir =
            resolve_parent_symlinks(link_path.parent().unwrap_or(Path::new("."))).await;
        let rel =
            pathdiff::diff_paths(target, &real_link_dir).unwrap_or_else(|| target.to_path_buf());
        tokio::fs::symlink(&rel, link_path).await.is_ok()
    }

    #[cfg(windows)]
    {
        let target = target.to_path_buf();
        let link = link_path.to_path_buf();
        tokio::task::spawn_blocking(move || junction::create(&target, &link).is_ok())
            .await
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name_basic() {
        assert_eq!(sanitize_name("My Skill Name"), "my-skill-name");
        assert_eq!(sanitize_name("../../evil"), "evil");
        assert_eq!(sanitize_name("hello_world.v2"), "hello_world.v2");
    }

    #[test]
    fn test_sanitize_name_empty() {
        assert_eq!(sanitize_name("..."), "unnamed-skill");
    }
}

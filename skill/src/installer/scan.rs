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
                canonical_path: skill_dir,
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

/// Check if a skill is installed — owned-value variant for `tokio::spawn`.
///
/// Accepts fully owned values instead of `&AgentConfig` so the returned
/// future is `Send + 'static`, safe for parallel spawning.
pub async fn is_skill_installed_owned(
    skill_name: String,
    skills_dir: String,
    global_skills_dir: Option<PathBuf>,
    scope: InstallScope,
    cwd: PathBuf,
) -> bool {
    let sanitized = sanitize_name(&skill_name);
    let target_base = match scope {
        InstallScope::Global => match global_skills_dir {
            Some(d) => d,
            None => return false,
        },
        InstallScope::Project => cwd.join(&skills_dir),
    };
    let skill_dir = target_base.join(sanitized);
    if !is_path_safe(&target_base, &skill_dir) {
        return false;
    }
    tokio::fs::try_exists(&skill_dir).await.unwrap_or(false)
}

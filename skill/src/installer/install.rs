//! Core install operations for local, remote, and well-known skills.

use std::path::Path;

use super::fs::{clean_and_create, copy_directory, create_symlink};
use super::paths::{agent_base_dir, canonical_skills_dir, is_path_safe, sanitize_name};
use crate::agents::AgentRegistry;
use crate::error::{Result, SkillError};
use crate::types::{AgentConfig, InstallMode, InstallOptions, InstallResult, InstallScope, Skill};

/// Write multiple files into a directory, skipping paths that escape it.
async fn write_skill_files<S: ::std::hash::BuildHasher + Sync>(
    dir: &Path,
    files: &std::collections::HashMap<String, String, S>,
) -> Result<()> {
    for (file_path, content) in files {
        let full = dir.join(file_path);
        if !is_path_safe(dir, &full) {
            continue;
        }
        if let Some(parent) = full.parent()
            && parent != dir
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SkillError::io(parent, e))?;
        }
        tokio::fs::write(&full, content)
            .await
            .map_err(|e| SkillError::io(&full, e))?;
    }
    Ok(())
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
    options: &InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(SkillError::AgentUnsupported {
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
        return Err(SkillError::PathTraversal {
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
    options: &InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(SkillError::AgentUnsupported {
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
        return Err(SkillError::PathTraversal {
            context: "skill name",
            path: skill_name,
        });
    }

    let mode = options.mode;

    if mode == InstallMode::Copy {
        clean_and_create(&agent_dir).await?;
        tokio::fs::write(agent_dir.join("SKILL.md"), content)
            .await
            .map_err(|e| SkillError::io(&agent_dir, e))?;
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
        .map_err(|e| SkillError::io(&canonical_dir, e))?;

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
            .map_err(|e| SkillError::io(&agent_dir, e))?;
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
pub async fn install_wellknown_skill_files<S: ::std::hash::BuildHasher + Clone + Send + Sync>(
    install_name: &str,
    files: &std::collections::HashMap<String, String, S>,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &InstallOptions,
) -> Result<InstallResult> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if scope == InstallScope::Global && agent.global_skills_dir.is_none() {
        return Err(SkillError::AgentUnsupported {
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
        return Err(SkillError::PathTraversal {
            context: "skill name",
            path: skill_name,
        });
    }

    let mode = options.mode;

    if mode == InstallMode::Copy {
        clean_and_create(&agent_dir).await?;
        write_skill_files(&agent_dir, files).await?;
        return Ok(InstallResult {
            path: agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&canonical_dir).await?;
    write_skill_files(&canonical_dir, files).await?;

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
        write_skill_files(&agent_dir, files).await?;
    }

    Ok(InstallResult {
        path: agent_dir,
        canonical_path: Some(canonical_dir),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    })
}

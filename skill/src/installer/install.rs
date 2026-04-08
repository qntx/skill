//! Core install operations for local, remote, and well-known skills.

use std::path::{Path, PathBuf};

use super::fs::{clean_and_create, copy_directory, create_symlink};
use super::paths::{agent_base_dir, canonical_skills_dir, is_path_safe, sanitize_name};
use crate::agents::AgentRegistry;
use crate::error::{Result, SkillError};
use crate::types::{AgentConfig, InstallMode, InstallOptions, InstallResult, InstallScope, Skill};

/// Computed paths and metadata for a skill installation.
struct InstallContext {
    /// Canonical `.agents/skills/<name>` directory.
    canonical_dir: PathBuf,
    /// Agent-specific skills directory.
    agent_dir: PathBuf,
    /// Copy or symlink.
    mode: InstallMode,
    /// Project or global.
    scope: InstallScope,
    /// Whether the agent uses the universal skills directory.
    is_universal: bool,
}

/// Validate options and compute install paths. Shared across all install functions.
fn prepare_install(
    install_name: &str,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &InstallOptions,
) -> Result<InstallContext> {
    let scope = options.scope;
    let cwd = options
        .cwd
        .as_deref()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    let cwd = if cwd.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_default()
    } else {
        cwd
    };

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

    Ok(InstallContext {
        canonical_dir,
        agent_dir,
        mode: options.mode,
        scope,
        is_universal: registry.is_universal(&agent.name),
    })
}

/// Finalize a symlink-mode install: create symlink from canonical to agent dir,
/// falling back to copy if the symlink fails.
///
/// Returns `(symlink_ok)` — caller must copy content to `agent_dir` on `false`.
fn try_symlink_or_return(ctx: &InstallContext) -> Option<InstallResult> {
    if ctx.scope == InstallScope::Global && ctx.is_universal {
        return Some(InstallResult {
            path: ctx.canonical_dir.clone(),
            canonical_path: Some(ctx.canonical_dir.clone()),
            mode: InstallMode::Symlink,
            symlink_failed: false,
        });
    }
    None
}

/// Build the final [`InstallResult`] after symlink attempt.
fn build_symlink_result(ctx: &InstallContext, symlink_ok: bool) -> InstallResult {
    InstallResult {
        path: ctx.agent_dir.clone(),
        canonical_path: Some(ctx.canonical_dir.clone()),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    }
}

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

/// Write a single `SKILL.md` file to a directory.
async fn write_single_skill_md(dir: &Path, content: &str) -> Result<()> {
    tokio::fs::write(dir.join("SKILL.md"), content)
        .await
        .map_err(|e| SkillError::io(dir, e))
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
    let ctx = prepare_install(&skill.name, agent, registry, options)?;

    if ctx.mode == InstallMode::Copy {
        clean_and_create(&ctx.agent_dir).await?;
        copy_directory(&skill.path, &ctx.agent_dir).await?;
        return Ok(InstallResult {
            path: ctx.agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&ctx.canonical_dir).await?;
    copy_directory(&skill.path, &ctx.canonical_dir).await?;

    if let Some(result) = try_symlink_or_return(&ctx) {
        return Ok(result);
    }

    let symlink_ok = create_symlink(&ctx.canonical_dir, &ctx.agent_dir).await;
    if !symlink_ok {
        clean_and_create(&ctx.agent_dir).await?;
        copy_directory(&skill.path, &ctx.agent_dir).await?;
    }

    Ok(build_symlink_result(&ctx, symlink_ok))
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
    let ctx = prepare_install(install_name, agent, registry, options)?;

    if ctx.mode == InstallMode::Copy {
        clean_and_create(&ctx.agent_dir).await?;
        write_single_skill_md(&ctx.agent_dir, content).await?;
        return Ok(InstallResult {
            path: ctx.agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&ctx.canonical_dir).await?;
    write_single_skill_md(&ctx.canonical_dir, content).await?;

    if let Some(result) = try_symlink_or_return(&ctx) {
        return Ok(result);
    }

    let symlink_ok = create_symlink(&ctx.canonical_dir, &ctx.agent_dir).await;
    if !symlink_ok {
        clean_and_create(&ctx.agent_dir).await?;
        write_single_skill_md(&ctx.agent_dir, content).await?;
    }

    Ok(build_symlink_result(&ctx, symlink_ok))
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
    let ctx = prepare_install(install_name, agent, registry, options)?;

    if ctx.mode == InstallMode::Copy {
        clean_and_create(&ctx.agent_dir).await?;
        write_skill_files(&ctx.agent_dir, files).await?;
        return Ok(InstallResult {
            path: ctx.agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&ctx.canonical_dir).await?;
    write_skill_files(&ctx.canonical_dir, files).await?;

    if let Some(result) = try_symlink_or_return(&ctx) {
        return Ok(result);
    }

    let symlink_ok = create_symlink(&ctx.canonical_dir, &ctx.agent_dir).await;
    if !symlink_ok {
        clean_and_create(&ctx.agent_dir).await?;
        write_skill_files(&ctx.agent_dir, files).await?;
    }

    Ok(build_symlink_result(&ctx, symlink_ok))
}

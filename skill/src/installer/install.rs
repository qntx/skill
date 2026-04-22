//! Core install operations for local, remote, and well-known skills.
//!
//! All three public entry points share a common choreography:
//!
//! 1. Validate the skill name, compute the canonical + agent-specific dirs.
//! 2. In copy mode — clean the agent dir, write, return.
//! 3. In symlink mode — clean the canonical dir, write, short-circuit for
//!    global universals, otherwise symlink agent dir → canonical (or copy on
//!    fallback).
//!
//! The only per-source variation is *how* the content is materialised on
//! disk.  That difference is abstracted behind the [`SkillWriter`] trait;
//! [`install`] owns the shared choreography.

use std::collections::HashMap;
use std::future::Future;
use std::hash::BuildHasher;
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

/// Abstraction over the three ways skill content can be written to disk.
///
/// Implementors define how to populate an already-created directory; [`install`]
/// handles all the cleanup / symlink / short-circuit logic around them.
trait SkillWriter {
    /// Write this skill's content into `dir`, which is guaranteed to exist
    /// and be empty.
    fn write(&self, dir: &Path) -> impl Future<Output = Result<()>> + Send;
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

/// Shared install choreography: copy / canonical write / symlink / fallback.
///
/// `writer.write(dir)` is called up to twice — once into the canonical dir
/// (symlink mode) and, on symlink failure, once more into the agent dir.
async fn install<W: SkillWriter>(ctx: InstallContext, writer: W) -> Result<InstallResult> {
    if ctx.mode == InstallMode::Copy {
        clean_and_create(&ctx.agent_dir).await?;
        writer.write(&ctx.agent_dir).await?;
        return Ok(InstallResult {
            path: ctx.agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
        });
    }

    clean_and_create(&ctx.canonical_dir).await?;
    writer.write(&ctx.canonical_dir).await?;

    // For global + universal installs, the canonical directory *is* the
    // agent directory — no symlink step is needed.
    if ctx.scope == InstallScope::Global && ctx.is_universal {
        return Ok(InstallResult {
            path: ctx.canonical_dir.clone(),
            canonical_path: Some(ctx.canonical_dir),
            mode: InstallMode::Symlink,
            symlink_failed: false,
        });
    }

    let symlink_ok = create_symlink(&ctx.canonical_dir, &ctx.agent_dir).await;
    if !symlink_ok {
        clean_and_create(&ctx.agent_dir).await?;
        writer.write(&ctx.agent_dir).await?;
    }

    Ok(InstallResult {
        path: ctx.agent_dir,
        canonical_path: Some(ctx.canonical_dir),
        mode: InstallMode::Symlink,
        symlink_failed: !symlink_ok,
    })
}

/// Writer that copies a local directory tree.
struct CopyTree<'a> {
    /// Source directory that will be cloned into the target location.
    source: &'a Path,
}

impl SkillWriter for CopyTree<'_> {
    async fn write(&self, dir: &Path) -> Result<()> {
        copy_directory(self.source, dir).await
    }
}

/// Writer that writes a single inline `SKILL.md` file.
struct InlineSkillMd<'a> {
    /// Raw markdown (with frontmatter) to write as `SKILL.md`.
    content: &'a str,
}

impl SkillWriter for InlineSkillMd<'_> {
    async fn write(&self, dir: &Path) -> Result<()> {
        tokio::fs::write(dir.join("SKILL.md"), self.content)
            .await
            .map_err(|e| SkillError::io(dir, e))
    }
}

/// Writer that materialises a map of `relative path → content` pairs.
struct FileMap<'a, S: BuildHasher + Sync> {
    /// Map of skill-relative path to file contents.
    files: &'a HashMap<String, String, S>,
}

impl<S: BuildHasher + Sync> SkillWriter for FileMap<'_, S> {
    async fn write(&self, dir: &Path) -> Result<()> {
        for (file_path, content) in self.files {
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
    install(
        ctx,
        CopyTree {
            source: &skill.path,
        },
    )
    .await
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
    install(ctx, InlineSkillMd { content }).await
}

/// Install a well-known skill with multiple files for an agent.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn install_wellknown_skill_files<S: BuildHasher + Clone + Send + Sync>(
    install_name: &str,
    files: &HashMap<String, String, S>,
    agent: &AgentConfig,
    registry: &AgentRegistry,
    options: &InstallOptions,
) -> Result<InstallResult> {
    let ctx = prepare_install(install_name, agent, registry, options)?;
    install(ctx, FileMap { files }).await
}

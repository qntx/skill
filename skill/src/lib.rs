//! # skill
//!
//! A library for managing AI agent skills across the open skills ecosystem.
//!
//! This crate provides the core functionality for discovering, installing,
//! listing, and removing agent skills. It is designed to be embedded in agent
//! frameworks so they gain full skills ecosystem support out of the box.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use skill::manager::SkillManager;
//!
//! # async fn example() -> skill::error::Result<()> {
//! let manager = SkillManager::builder().build();
//!
//! // Discover skills in a repository
//! let skills = manager
//!     .discover_skills(std::path::Path::new("./my-repo"), &Default::default())
//!     .await?;
//!
//! // List installed skills
//! let installed = manager.list_installed(&Default::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Module Map
//!
//! - [`manager`] — [`SkillManager`] façade; the main entry point for
//!   agent frameworks.
//! - [`types`]   — plain data types (`Skill`, `AgentId`, `InstallScope`, …).
//! - [`error`]   — library-wide [`SkillError`] / [`Result`].
//! - [`agents`]  — built-in agent registry + custom registration hooks.
//! - [`skills`]  — on-disk skill discovery and `SKILL.md` parsing.
//! - [`installer`] — install / remove / scan choreography.
//! - [`source`]  — source-string parsing (`owner/repo`, URLs, local paths).
//! - [`providers`] — remote skill hosts (well-known, GitHub, GitLab).
//! - [`lock`] / [`local_lock`] — global and project lock-file I/O.
//! - [`blob`], [`git`], [`github`] — network transports (feature-gated).
//! - [`sanitize`] — input sanitization helpers.
//! - [`telemetry`] — anonymous usage reporting (feature-gated).
//!
//! ## Feature Flags
//!
//! - **`network`** (default) — Enables HTTP-based operations (fetching remote
//!   skills, well-known providers, GitHub API).
//! - **`telemetry`** — Enables anonymous usage telemetry. Disabled by default
//!   for library consumers; enabled by the CLI.

use pathdiff as _;

pub mod agents;
#[cfg(feature = "network")]
pub mod blob;
pub mod error;
pub mod git;
pub mod github;
pub mod installer;
pub mod local_lock;
pub mod lock;
pub mod manager;
pub(crate) mod path_util;
pub(crate) mod plugin_manifest;
pub mod providers;
pub mod sanitize;
pub mod skills;
pub mod source;
pub mod telemetry;
pub mod types;
pub(crate) mod util;

pub use error::{Result, SkillError};
pub use manager::SkillManager;

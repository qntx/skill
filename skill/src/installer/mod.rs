//! Skill installation, removal, and listing.
//!
//! Handles copying or symlinking skills into agent-specific directories,
//! with a canonical `.agents/skills/` location as the single source of truth.
//!
//! # Public API layering
//!
//! - [`candidate_install_paths`] — **sync**, pure: turn `(skill_name, agent,
//!   scope, cwd)` into every on-disk path the skill may occupy.
//! - [`any_path_exists`] — **async**: probe a pre-computed path list.
//! - [`is_skill_installed`] — convenience wrapper over the two; callers
//!   that need to fan out across many `(skill × agent)` pairs should
//!   compose the two primitives instead for clean `Send + 'static` tasks.

mod fs;
mod install;
mod paths;
mod scan;

pub use install::{
    install_remote_skill_content, install_skill_for_agent, install_wellknown_skill_files,
};
pub use paths::{
    agent_base_dir, candidate_install_paths, canonical_install_path, canonical_skills_dir,
};
pub use scan::{any_path_exists, is_skill_installed, list_installed_skills};

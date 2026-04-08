//! Skill installation, removal, and listing.
//!
//! Handles copying or symlinking skills into agent-specific directories,
//! with a canonical `.agents/skills/` location as the single source of truth.

mod fs;
mod install;
mod paths;
mod scan;

pub use install::{
    install_remote_skill_content, install_skill_for_agent, install_wellknown_skill_files,
};
pub use paths::{agent_base_dir, canonical_skills_dir, get_canonical_path, sanitize_name};
pub use scan::{is_skill_installed, is_skill_installed_owned, list_installed_skills};

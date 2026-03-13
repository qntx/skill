//! Core data types for the skill ecosystem.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Unique identifier for an agent.
///
/// This is a newtype over `String` rather than a closed enum, allowing agent
/// frameworks to register custom agents without forking the library.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    /// Create a new agent identifier.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Return the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<S: Into<String>> From<S> for AgentId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// Configuration for a single agent.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Machine-readable identifier (e.g. `"cursor"`, `"claude-code"`).
    pub name: AgentId,
    /// Human-readable display name (e.g. `"Cursor"`, `"Claude Code"`).
    pub display_name: String,
    /// Project-relative skills directory (e.g. `".agents/skills"`).
    pub skills_dir: String,
    /// Global skills directory. `None` if global install is unsupported.
    pub global_skills_dir: Option<PathBuf>,
    /// Paths to check for agent detection (existence = installed).
    pub detect_paths: Vec<PathBuf>,
    /// Whether this agent appears in the universal agent list.
    pub show_in_universal_list: bool,
}

/// A discovered local skill parsed from a `SKILL.md` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name from frontmatter.
    pub name: String,
    /// Skill description from frontmatter.
    pub description: String,
    /// Absolute path to the skill directory.
    pub path: PathBuf,
    /// Raw content of the `SKILL.md` file (for hashing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    /// Name of the plugin this skill belongs to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Additional metadata from frontmatter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_yaml::Value>>,
}

/// A skill fetched from a remote host provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSkill {
    /// Display name from frontmatter.
    pub name: String,
    /// Description from frontmatter.
    pub description: String,
    /// Full markdown content including frontmatter.
    pub content: String,
    /// Identifier used for the installation directory name.
    pub install_name: String,
    /// Original source URL.
    pub source_url: String,
    /// Provider identifier that fetched this skill.
    pub provider_id: String,
    /// Source identifier for telemetry (e.g. `"mintlify.com"`).
    pub source_identifier: String,
    /// Additional metadata from frontmatter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_yaml::Value>>,
}

/// A well-known skill with multiple files.
#[derive(Debug, Clone)]
pub struct WellKnownSkill {
    /// The remote skill metadata.
    pub remote: RemoteSkill,
    /// All files keyed by relative path (e.g. `"SKILL.md"` -> content).
    pub files: HashMap<String, String>,
    /// The index entry from `index.json`.
    pub index_entry: WellKnownSkillEntry,
}

/// A single entry in a well-known `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownSkillEntry {
    /// Skill identifier (directory name).
    pub name: String,
    /// Brief description.
    pub description: String,
    /// List of files in the skill directory.
    pub files: Vec<String>,
}

/// The index structure for well-known skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownIndex {
    /// All skills listed in the index.
    pub skills: Vec<WellKnownSkillEntry>,
}

/// The type of a parsed skill source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceType {
    /// GitHub repository (URL or shorthand).
    Github,
    /// `GitLab` repository.
    Gitlab,
    /// Generic git repository URL.
    Git,
    /// Local filesystem path.
    Local,
    /// Well-known skills endpoint (RFC 8615).
    WellKnown,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Github => write!(f, "github"),
            Self::Gitlab => write!(f, "gitlab"),
            Self::Git => write!(f, "git"),
            Self::Local => write!(f, "local"),
            Self::WellKnown => write!(f, "well-known"),
        }
    }
}

/// A parsed source reference.
#[derive(Debug, Clone)]
pub struct ParsedSource {
    /// Source type.
    pub source_type: SourceType,
    /// Canonical URL or resolved path.
    pub url: String,
    /// Subpath within the repository.
    pub subpath: Option<String>,
    /// Resolved local filesystem path (only for `Local` type).
    pub local_path: Option<PathBuf>,
    /// Git ref (branch/tag/commit).
    pub git_ref: Option<String>,
    /// Skill name filter from `@skill` syntax.
    pub skill_filter: Option<String>,
}

/// Installation mode: symlink (default) or copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMode {
    /// Create a canonical copy and symlink from agent directories.
    #[default]
    Symlink,
    /// Copy directly to each agent directory.
    Copy,
}

/// Installation scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallScope {
    /// Project-level installation (current directory).
    #[default]
    Project,
    /// Global / user-level installation (home directory).
    Global,
}

/// Result of a single skill installation.
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Whether the installation succeeded.
    pub success: bool,
    /// Path where the skill was installed.
    pub path: PathBuf,
    /// Canonical path (for symlink mode).
    pub canonical_path: Option<PathBuf>,
    /// Mode that was used.
    pub mode: InstallMode,
    /// Whether a symlink attempt failed and fell back to copy.
    pub symlink_failed: bool,
    /// Error message if `success` is false.
    pub error: Option<String>,
}

/// A skill that is currently installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    /// Skill name.
    pub name: String,
    /// Skill description.
    pub description: String,
    /// Path to the installed skill directory.
    pub path: PathBuf,
    /// Canonical path (may differ from `path` for symlinked installs).
    pub canonical_path: PathBuf,
    /// Installation scope.
    pub scope: InstallScope,
    /// Agents this skill is installed for.
    pub agents: Vec<AgentId>,
}

/// Options for skill discovery.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoverOptions {
    /// Include skills marked as `internal` in metadata.
    pub include_internal: bool,
    /// Search all subdirectories even when a root `SKILL.md` exists.
    pub full_depth: bool,
}

/// Options for installation operations.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Installation scope.
    pub scope: InstallScope,
    /// Installation mode.
    pub mode: InstallMode,
    /// Override the working directory.
    pub cwd: Option<PathBuf>,
}

/// Options for listing installed skills.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Scope to list. `None` lists both project and global.
    pub scope: Option<InstallScope>,
    /// Filter by specific agents.
    pub agent_filter: Vec<AgentId>,
    /// Override the working directory.
    pub cwd: Option<PathBuf>,
}

/// Options for removal operations.
#[derive(Debug, Clone, Default)]
pub struct RemoveOptions {
    /// Installation scope.
    pub scope: InstallScope,
    /// Target specific agents. Empty = all agents.
    pub agents: Vec<AgentId>,
    /// Override the working directory.
    pub cwd: Option<PathBuf>,
}

/// The canonical agents directory name.
pub const AGENTS_DIR: &str = ".agents";

/// The skills subdirectory name.
pub const SKILLS_SUBDIR: &str = "skills";

/// The universal skills directory (project-relative).
pub const UNIVERSAL_SKILLS_DIR: &str = ".agents/skills";

//! Error types for the skill library.

use std::path::PathBuf;

/// The primary error type for all skill operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A required skill was not found.
    #[error("skill not found: {0}")]
    SkillNotFound(String),

    /// The provided source string could not be parsed.
    #[error("invalid source: {0}")]
    InvalidSource(String),

    /// A path traversal attempt was detected in a subpath or skill name.
    #[error("path traversal detected in {context}: {path}")]
    PathTraversal {
        /// What was being validated (e.g. "subpath", "skill name").
        context: &'static str,
        /// The offending path.
        path: String,
    },

    /// Git clone operation failed.
    #[error("git clone failed for {url}: {message}")]
    GitClone {
        /// The repository URL that failed.
        url: String,
        /// Error description.
        message: String,
        /// Whether the clone timed out.
        is_timeout: bool,
        /// Whether the error is an authentication failure.
        is_auth_error: bool,
    },

    /// An HTTP request failed.
    #[cfg(feature = "network")]
    #[error("network error: {source}")]
    Network {
        /// The underlying reqwest error.
        #[from]
        source: reqwest::Error,
    },

    /// Filesystem I/O error.
    #[error("I/O error at {}: {source}", path.display())]
    Io {
        /// The path involved in the operation.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// JSON serialization / deserialization error.
    #[error("JSON error: {source}")]
    Json {
        /// The underlying `serde_json` error.
        #[from]
        source: serde_json::Error,
    },

    /// YAML parsing error (frontmatter).
    #[error("YAML error: {source}")]
    Yaml {
        /// The underlying `serde_yml` error.
        #[from]
        source: serde_yml::Error,
    },

    /// The agent does not support the requested operation.
    #[error("agent `{agent}` does not support {operation}")]
    AgentUnsupported {
        /// Agent display name.
        agent: String,
        /// Operation that is not supported.
        operation: &'static str,
    },

    /// The specified agent was not found in the registry.
    #[error("unknown agent: {0}")]
    UnknownAgent(String),

    /// An installation operation failed.
    #[error("installation failed for `{skill}`: {message}")]
    InstallFailed {
        /// Skill name.
        skill: String,
        /// Error description.
        message: String,
    },
}

/// Convenience type alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create an I/O error with path context.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

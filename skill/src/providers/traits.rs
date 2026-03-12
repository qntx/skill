//! Provider trait definitions.

use crate::error::Result;
use crate::types::RemoteSkill;

/// Result of matching a URL against a provider.
#[derive(Debug, Clone)]
pub struct ProviderMatch {
    /// Whether the URL matches this provider.
    pub matches: bool,
    /// Source identifier for telemetry / storage.
    pub source_identifier: Option<String>,
}

/// Interface for remote skill host providers.
///
/// Each provider knows how to detect matching URLs, fetch skills, and
/// provide source identifiers.
#[async_trait::async_trait]
pub trait HostProvider: Send + Sync + std::fmt::Debug {
    /// Unique identifier for this provider.
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Check if a URL matches this provider.
    fn matches(&self, url: &str) -> ProviderMatch;

    /// Fetch a skill from the given URL.
    async fn fetch_skill(&self, url: &str) -> Result<Option<RemoteSkill>>;

    /// Convert a user-facing URL to a raw content URL.
    fn to_raw_url(&self, url: &str) -> String;

    /// Get the source identifier for telemetry / storage.
    fn source_identifier(&self, url: &str) -> String;
}

//! Provider trait definitions.

use std::future::Future;
use std::pin::Pin;

use crate::error::Result;
use crate::types::RemoteSkill;

/// A boxed, `Send`-safe future — the dyn-compatible replacement for
/// `async fn` in traits (native `async fn` is not object-safe).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Interface for remote skill host providers.
///
/// Each provider knows how to detect matching URLs, fetch skills, and
/// provide source identifiers.
pub trait HostProvider: Send + Sync + std::fmt::Debug {
    /// Unique identifier for this provider.
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Check if a URL matches this provider.
    ///
    /// Returns `Some(source_identifier)` on match, `None` otherwise.
    fn matches_url(&self, url: &str) -> Option<String>;

    /// Fetch a skill from the given URL.
    fn fetch_skill<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Option<RemoteSkill>>>;

    /// Convert a user-facing URL to a raw content URL.
    fn to_raw_url(&self, url: &str) -> String;

    /// Get the source identifier for telemetry / storage.
    fn source_identifier(&self, url: &str) -> String;
}

//! Provider registry.

use super::traits::HostProvider;
use super::wellknown::WellKnownProvider;

/// Registry managing host providers.
///
/// Pre-populated with the built-in well-known provider.
#[derive(Debug)]
pub struct ProviderRegistry {
    providers: Vec<Box<dyn HostProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ProviderRegistry {
    /// Create a registry with the built-in well-known provider.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut reg = Self {
            providers: Vec::new(),
        };
        reg.register(WellKnownProvider);
        reg
    }

    /// Create an empty registry.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a new provider.
    pub fn register(&mut self, provider: impl HostProvider + 'static) {
        self.providers.push(Box::new(provider));
    }

    /// Find the first provider matching `url`.
    ///
    /// Returns the provider reference and its source identifier.
    #[must_use]
    pub fn find_match(&self, url: &str) -> Option<(&dyn HostProvider, String)> {
        self.providers
            .iter()
            .find_map(|p| p.matches_url(url).map(|source_id| (p.as_ref(), source_id)))
    }

    /// Get all registered providers.
    #[must_use]
    pub fn providers(&self) -> &[Box<dyn HostProvider>] {
        &self.providers
    }
}

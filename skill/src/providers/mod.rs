//! Skill source providers.
//!
//! Defines the [`HostProvider`] trait for fetching skills from remote hosts
//! and provides a [`ProviderRegistry`] for managing multiple providers.

mod registry;
mod traits;
mod wellknown;

pub use registry::ProviderRegistry;
pub use traits::{HostProvider, ProviderMatch};
pub use wellknown::WellKnownProvider;

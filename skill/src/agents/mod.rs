//! Agent registry and detection.
//!
//! The **registry** API ([`AgentRegistry`]) is in this file; the actual
//! table of built-in agents lives in the sibling `builtin` submodule so the
//! data (what each agent's skill dir looks like on disk) stays separated
//! from the behaviour (how to look agents up, detect them, classify them).

mod builtin;

use std::collections::BTreeMap;

use crate::types::{AgentConfig, AgentId, UNIVERSAL_SKILLS_DIR};

/// Registry holding all known agent configurations.
///
/// Pre-populated with the built-in agents via [`AgentRegistry::with_defaults`].
/// Agent frameworks can register additional agents with
/// [`AgentRegistry::register`].
///
/// Backed by a [`BTreeMap`] so iteration order is always ascending-by-id,
/// which is exactly what every caller needs (`all_ids`, `universal_agents`,
/// `non_universal_agents`, `detect_installed`). This removes four separate
/// `.sort()` trailers at effectively zero runtime cost (the 45-element
/// lookup table makes `O(log n)` vs `O(1)` indistinguishable).
///
/// # Examples
///
/// ```
/// use skill::agents::AgentRegistry;
///
/// let registry = AgentRegistry::with_defaults();
/// assert!(!registry.is_empty());
/// assert!(registry.universal_agents().len() > 0);
/// ```
#[derive(Debug)]
pub struct AgentRegistry {
    /// Map of agent IDs to their configurations.
    agents: BTreeMap<AgentId, AgentConfig>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl AgentRegistry {
    /// Create a registry pre-populated with all built-in agent definitions.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut registry = Self::empty();
        for config in builtin::builtin_agents() {
            registry.register(config);
        }
        registry
    }

    /// Create an empty registry with no agents.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            agents: BTreeMap::new(),
        }
    }

    /// Register a custom agent configuration.
    pub fn register(&mut self, config: AgentConfig) {
        self.agents.insert(config.name.clone(), config);
    }

    /// Look up an agent by its identifier.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<&AgentConfig> {
        self.agents.get(id)
    }

    /// Return all registered agent identifiers, sorted alphabetically.
    #[must_use]
    pub fn all_ids(&self) -> Vec<AgentId> {
        self.agents.keys().cloned().collect()
    }

    /// Return all registered agent configurations.
    #[must_use]
    pub fn all_configs(&self) -> Vec<&AgentConfig> {
        self.agents.values().collect()
    }

    /// Detect which agents are installed by probing their known paths.
    ///
    /// Matches the TS reference `agents.ts::detectInstalledAgents`: uses
    /// [`tokio::task::JoinSet`] to fan-out probes across all registered
    /// agents. Each task short-circuits on the first existing path, so
    /// best-case latency is a single `try_exists` call regardless of how
    /// many `detect_paths` an agent declares.
    ///
    /// Returns the sorted list of installed agent IDs.
    pub async fn detect_installed(&self) -> Vec<AgentId> {
        let mut set: tokio::task::JoinSet<Option<AgentId>> = tokio::task::JoinSet::new();
        for (id, config) in &self.agents {
            let id = id.clone();
            let paths = config.detect_paths.clone();
            set.spawn(async move {
                crate::installer::any_path_exists(&paths)
                    .await
                    .then_some(id)
            });
        }

        let mut installed = Vec::with_capacity(set.len());
        while let Some(result) = set.join_next().await {
            // JoinSet task panics are swallowed: detection is best-effort
            // and must never crash the caller. Missing an agent on a panic
            // just reports it as "not installed", mirroring TS `catch`.
            if let Ok(Some(id)) = result {
                installed.push(id);
            }
        }
        installed.sort();
        installed
    }

    /// Return agent IDs that use the universal `.agents/skills` directory
    /// and appear in the universal list.
    ///
    /// Result is sorted ascending thanks to the backing `BTreeMap`.
    #[must_use]
    pub fn universal_agents(&self) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|(_, c)| c.skills_dir == UNIVERSAL_SKILLS_DIR && c.show_in_universal_list)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Return agent IDs that use agent-specific (non-universal) directories.
    ///
    /// Result is sorted ascending thanks to the backing `BTreeMap`.
    #[must_use]
    pub fn non_universal_agents(&self) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|(_, c)| c.skills_dir != UNIVERSAL_SKILLS_DIR)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Check whether an agent uses the universal `.agents/skills` directory.
    #[must_use]
    pub fn is_universal(&self, id: &AgentId) -> bool {
        self.agents
            .get(id)
            .is_some_and(|c| c.skills_dir == UNIVERSAL_SKILLS_DIR)
    }

    /// Return the number of registered agents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry_has_no_agents() {
        let r = AgentRegistry::empty();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn test_with_defaults_registers_all_builtin_agents() {
        let r = AgentRegistry::with_defaults();
        // Sanity: at least the TS-known set (45 incl. universal).
        assert!(
            r.len() >= 45,
            "expected >= 45 built-in agents, got {}",
            r.len()
        );
    }

    #[test]
    fn test_with_defaults_includes_recently_added_agents() {
        let r = AgentRegistry::with_defaults();
        for id in ["bob", "deepagents", "firebender", "warp"] {
            assert!(
                r.get(&AgentId::new(id)).is_some(),
                "missing builtin agent: {id}"
            );
        }
    }

    #[test]
    fn test_register_overwrites_existing_agent() {
        let mut r = AgentRegistry::with_defaults();
        let original = r.get(&AgentId::new("cursor")).cloned().unwrap();
        let mut modified = original;
        modified.display_name = "CustomCursor".to_owned();
        r.register(modified);
        assert_eq!(
            r.get(&AgentId::new("cursor")).unwrap().display_name,
            "CustomCursor"
        );
    }

    #[test]
    fn test_universal_agents_excludes_agent_specific_dirs() {
        let r = AgentRegistry::with_defaults();
        let universals = r.universal_agents();
        assert!(universals.contains(&AgentId::new("cursor")));
        assert!(!universals.contains(&AgentId::new("claude-code")));
    }

    #[test]
    fn test_universal_agents_excludes_hidden_list_entries() {
        let r = AgentRegistry::with_defaults();
        let universals = r.universal_agents();
        // `replit` and `universal` both have `show_in_universal_list = false`.
        assert!(!universals.contains(&AgentId::new("replit")));
        assert!(!universals.contains(&AgentId::new("universal")));
    }

    #[test]
    fn test_is_universal_matches_skills_dir() {
        let r = AgentRegistry::with_defaults();
        assert!(r.is_universal(&AgentId::new("cursor")));
        assert!(!r.is_universal(&AgentId::new("claude-code")));
    }

    #[test]
    fn test_all_ids_sorted() {
        let r = AgentRegistry::with_defaults();
        let ids = r.all_ids();
        for pair in ids.windows(2) {
            let [a, b] = pair else { unreachable!() };
            assert!(a <= b, "{pair:?} not sorted");
        }
    }

    #[test]
    fn test_antigravity_uses_plural_agents_skills_dir() {
        let r = AgentRegistry::with_defaults();
        let config = r.get(&AgentId::new("antigravity")).unwrap();
        assert_eq!(config.skills_dir, ".agents/skills");
    }
}

//! `SkillManager` — the primary API surface for agent frameworks.
//!
//! Provides a unified interface for discovering, installing, listing, and
//! removing skills. Agent frameworks embed this struct to gain full skills
//! ecosystem support.

use std::path::{Path, PathBuf};

use crate::agents::AgentRegistry;
use crate::error::Result;
use crate::installer;
use crate::providers::ProviderRegistry;
use crate::types::{
    AgentConfig, AgentId, DiscoverOptions, InstallOptions, InstallResult, InstallScope,
    InstalledSkill, ListOptions, ParsedSource, RemoveOptions, Skill,
};

/// Configuration for building a [`SkillManager`].
#[derive(Debug, Clone, Default)]
pub struct ManagerConfig {
    /// Override the working directory (defaults to `std::env::current_dir()`).
    pub cwd: Option<PathBuf>,
}

/// Builder for constructing a [`SkillManager`].
#[derive(Debug, Default)]
pub struct SkillManagerBuilder {
    /// Custom agent registry override.
    agents: Option<AgentRegistry>,
    /// Custom provider registry override.
    providers: Option<ProviderRegistry>,
    /// Manager configuration.
    config: ManagerConfig,
}

impl SkillManagerBuilder {
    /// Use a custom agent registry instead of the defaults.
    #[must_use]
    pub fn agents(mut self, agents: AgentRegistry) -> Self {
        self.agents = Some(agents);
        self
    }

    /// Use a custom provider registry instead of the defaults.
    #[must_use]
    pub fn providers(mut self, providers: ProviderRegistry) -> Self {
        self.providers = Some(providers);
        self
    }

    /// Override the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.config.cwd = Some(cwd.into());
        self
    }

    /// Build the manager.
    #[must_use]
    pub fn build(self) -> SkillManager {
        SkillManager {
            agents: self.agents.unwrap_or_default(),
            providers: self.providers.unwrap_or_default(),
            config: self.config,
        }
    }
}

/// The primary API for managing agent skills.
///
/// # Example
///
/// ```rust,no_run
/// use skill::manager::SkillManager;
///
/// # async fn example() -> skill::error::Result<()> {
/// let manager = SkillManager::builder().build();
///
/// // Discover skills in a directory
/// let skills = manager
///     .discover_skills(std::path::Path::new("./my-repo"), &Default::default())
///     .await?;
///
/// // List installed skills
/// let installed = manager.list_installed(&Default::default()).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SkillManager {
    /// Agent registry.
    agents: AgentRegistry,
    /// Provider registry.
    providers: ProviderRegistry,
    /// Manager configuration.
    config: ManagerConfig,
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl SkillManager {
    /// Create a new builder.
    #[must_use]
    pub fn builder() -> SkillManagerBuilder {
        SkillManagerBuilder::default()
    }

    /// Get the effective working directory.
    #[must_use]
    pub fn cwd(&self) -> std::borrow::Cow<'_, Path> {
        self.config.cwd.as_deref().map_or_else(
            || std::borrow::Cow::Owned(std::env::current_dir().unwrap_or_default()),
            std::borrow::Cow::Borrowed,
        )
    }

    /// Access the agent registry (immutable).
    #[must_use]
    pub const fn agents(&self) -> &AgentRegistry {
        &self.agents
    }

    /// Access the agent registry (mutable).
    pub const fn agents_mut(&mut self) -> &mut AgentRegistry {
        &mut self.agents
    }

    /// Detect which agents are installed on the system.
    pub async fn detect_installed_agents(&self) -> Vec<AgentId> {
        self.agents.detect_installed().await
    }

    /// Access the provider registry.
    #[must_use]
    pub const fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    /// Register a custom host provider.
    pub fn register_provider(&mut self, provider: impl crate::providers::HostProvider + 'static) {
        self.providers.register(provider);
    }

    /// Parse a source string into a [`ParsedSource`].
    #[must_use]
    #[allow(clippy::unused_self, reason = "method form for API consistency")]
    pub fn parse_source(&self, input: &str) -> ParsedSource {
        crate::source::parse_source(input)
    }

    /// Discover skills in a directory.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O or path-safety failure.
    pub async fn discover_skills(
        &self,
        path: &Path,
        options: &DiscoverOptions,
    ) -> Result<Vec<Skill>> {
        crate::skills::discover_skills(path, None, options).await
    }

    /// Discover skills with a subpath.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O or path-safety failure.
    pub async fn discover_skills_with_subpath(
        &self,
        path: &Path,
        subpath: &str,
        options: &DiscoverOptions,
    ) -> Result<Vec<Skill>> {
        crate::skills::discover_skills(path, Some(subpath), options).await
    }

    /// Install a discovered skill for a specific agent.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O or installation failure.
    pub async fn install_skill(
        &self,
        skill: &Skill,
        agent_id: &AgentId,
        options: &InstallOptions,
    ) -> Result<InstallResult> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| crate::error::SkillError::UnknownAgent(agent_id.to_string()))?;

        installer::install_skill_for_agent(skill, agent, &self.agents, options).await
    }

    /// List all installed skills.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub async fn list_installed(&self, options: &ListOptions) -> Result<Vec<InstalledSkill>> {
        installer::list_installed_skills(&self.agents, options).await
    }

    /// Remove installed skills by name.
    ///
    /// Matches the Vercel TS `removeCommand` behavior:
    ///  - Cleans up all agent-specific directories (including the "native"
    ///    directory for universal agents to remove legacy symlinks).
    ///  - Only removes the canonical path if no remaining (non-targeted)
    ///    agents still reference it.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub async fn remove_skills(
        &self,
        skill_names: &[String],
        options: &RemoveOptions,
    ) -> Result<()> {
        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| self.cwd().into_owned());
        let scope = options.scope;
        let target_agents: Vec<AgentId> = if options.agents.is_empty() {
            self.agents.all_ids()
        } else {
            options.agents.clone()
        };

        for name in skill_names {
            let canonical = installer::canonical_install_path(name, scope, &cwd);
            self.cleanup_agent_paths(name, &target_agents, scope, &canonical, &cwd)
                .await;

            if !self
                .canonical_still_referenced(name, &target_agents, scope, &cwd)
                .await
            {
                force_remove(&canonical).await;
            }
        }

        Ok(())
    }

    /// Delete every agent-specific path that might hold the skill, skipping
    /// the canonical directory (that deletion is handled separately).
    async fn cleanup_agent_paths(
        &self,
        name: &str,
        target_agents: &[AgentId],
        scope: InstallScope,
        canonical: &Path,
        cwd: &Path,
    ) {
        let sanitized = crate::sanitize::sanitize_name(name);
        for agent_id in target_agents {
            let Some(agent) = self.agents.get(agent_id) else {
                continue;
            };
            let paths = candidate_paths(agent, &self.agents, scope, &sanitized, cwd);
            for path in paths.into_iter().filter(|p| p != canonical) {
                force_remove(&path).await;
            }
        }
    }

    /// Check whether any non-targeted **detected** agent still references the
    /// canonical skill directory.
    ///
    /// Matches TS `remove.ts:194-205`: only agents that are currently
    /// installed on the system participate in the "still in use" check.
    /// A stale directory belonging to an uninstalled agent does not block
    /// canonical cleanup, since removing it is exactly what the user wants
    /// when they uninstall.
    async fn canonical_still_referenced(
        &self,
        name: &str,
        target_agents: &[AgentId],
        scope: InstallScope,
        cwd: &Path,
    ) -> bool {
        for aid in self.agents.detect_installed().await {
            if target_agents.contains(&aid) {
                continue;
            }
            if let Some(agent) = self.agents.get(&aid)
                && installer::is_skill_installed(name, agent, scope, cwd).await
            {
                return true;
            }
        }
        false
    }
}

/// Best-effort removal of whatever exists at `path` — directory, symlink,
/// or regular file.  We try `remove_dir_all` first (handles directories and
/// dir-symlinks on Unix, junctions on Windows) and fall through to
/// `remove_file` for file-symlinks.  Every error is swallowed because this
/// is called during cleanup where partial progress is acceptable.
async fn force_remove(path: &Path) {
    drop(tokio::fs::remove_dir_all(path).await);
    drop(tokio::fs::remove_file(path).await);
}

/// Every directory an agent may have used for `sanitized` skill name under
/// `scope`.  Includes both the canonical `agent_base_dir` (which for universal
/// agents *is* the canonical skills dir) and the "native" agent-specific
/// directory so legacy symlinks get cleaned up too.
fn candidate_paths(
    agent: &AgentConfig,
    registry: &AgentRegistry,
    scope: InstallScope,
    sanitized: &str,
    cwd: &Path,
) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(2);
    paths.push(installer::agent_base_dir(agent, registry, scope, cwd).join(sanitized));

    let native_dir = match scope {
        InstallScope::Global => agent.global_skills_dir.as_ref().map(|d| d.join(sanitized)),
        InstallScope::Project => Some(cwd.join(&agent.skills_dir).join(sanitized)),
    };
    if let Some(nd) = native_dir
        && !paths.contains(&nd)
    {
        paths.push(nd);
    }
    paths
}

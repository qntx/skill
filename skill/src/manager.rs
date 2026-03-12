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
    AgentId, DiscoverOptions, InstallOptions, InstallResult, InstalledSkill, ListOptions,
    ParsedSource, RemoveOptions, Skill,
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
    agents: Option<AgentRegistry>,
    providers: Option<ProviderRegistry>,
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
    agents: AgentRegistry,
    providers: ProviderRegistry,
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
    pub fn cwd(&self) -> PathBuf {
        self.config
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    // -----------------------------------------------------------------------
    // Agent management
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Provider management
    // -----------------------------------------------------------------------

    /// Access the provider registry.
    #[must_use]
    pub const fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    /// Register a custom host provider.
    pub fn register_provider(&mut self, provider: impl crate::providers::HostProvider + 'static) {
        self.providers.register(provider);
    }

    // -----------------------------------------------------------------------
    // Discovery
    // -----------------------------------------------------------------------

    /// Parse a source string into a [`ParsedSource`].
    #[must_use]
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

    // -----------------------------------------------------------------------
    // Installation
    // -----------------------------------------------------------------------

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
            .ok_or_else(|| crate::error::Error::UnknownAgent(agent_id.to_string()))?;

        installer::install_skill_for_agent(skill, agent, &self.agents, options).await
    }

    // -----------------------------------------------------------------------
    // Listing
    // -----------------------------------------------------------------------

    /// List all installed skills.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub async fn list_installed(&self, options: &ListOptions) -> Result<Vec<InstalledSkill>> {
        installer::list_installed_skills(&self.agents, options).await
    }

    // -----------------------------------------------------------------------
    // Removal
    // -----------------------------------------------------------------------

    /// Remove installed skills by name.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure.
    pub async fn remove_skills(
        &self,
        skill_names: &[String],
        options: &RemoveOptions,
    ) -> Result<Vec<RemoveResult>> {
        let cwd = options.cwd.clone().unwrap_or_else(|| self.cwd());
        let scope = options.scope;
        let mut results = Vec::new();

        for name in skill_names {
            let canonical = installer::get_canonical_path(name, scope, &cwd);

            let target_agents: Vec<AgentId> = if options.agents.is_empty() {
                self.agents.all_ids()
            } else {
                options.agents.clone()
            };

            for agent_id in &target_agents {
                if let Some(agent) = self.agents.get(agent_id) {
                    let agent_base = installer::agent_base_dir(agent, &self.agents, scope, &cwd);
                    let skill_dir = agent_base.join(installer::sanitize_name(name));

                    if skill_dir != canonical {
                        let _ = tokio::fs::remove_dir_all(&skill_dir).await;
                    }
                }
            }

            let removed = tokio::fs::remove_dir_all(&canonical).await.is_ok();
            results.push(RemoveResult {
                skill: name.clone(),
                success: removed,
                error: None,
            });
        }

        Ok(results)
    }
}

/// Result of a removal operation.
#[derive(Debug, Clone)]
pub struct RemoveResult {
    /// Skill name.
    pub skill: String,
    /// Whether the removal succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

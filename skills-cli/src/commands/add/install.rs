//! Installation execution logic for the `add` command.

use std::path::PathBuf;

use miette::{Result, miette};
use skill::SkillManager;
use skill::types::{
    AgentId, InstallMode, InstallOptions, InstallResult, Skill, SourceType, WellKnownSkill,
};

/// Per-agent classification of how a skill ended up on disk.
#[derive(Debug, Clone)]
pub(super) enum AgentInstallStatus {
    /// Agent uses the universal `.agents/skills` directory, so canonical
    /// placement *is* the agent placement.
    Universal,
    /// Agent-specific directory is a symlink into the canonical tree.
    Symlinked,
    /// Agent-specific directory was requested as a copy (`--copy`), or
    /// the agent receives a physical copy for other reasons.  Carries the
    /// absolute path that was written.
    Copied { path: PathBuf },
    /// A symlink was attempted but failed (e.g. missing admin rights on
    /// Windows) and the installer fell back to copying.  Carries the path
    /// of the fallback copy.
    SymlinkFellBackToCopy { path: PathBuf },
    /// Installation for this agent failed outright.
    Failed,
}

/// One (display-name, status) pair — `display_name` is the user-facing agent
/// label, `status` is the classification of the per-agent outcome.
#[derive(Debug, Clone)]
pub(super) struct AgentOutcome {
    pub display_name: String,
    pub status: AgentInstallStatus,
}

/// Aggregated outcome for a single skill across every target agent.
#[derive(Debug, Clone)]
pub(super) struct SkillInstallOutcome {
    pub skill_name: String,
    pub plugin_name: Option<String>,
    /// Canonical `.agents/skills/<name>` path, populated the first time any
    /// agent reports one.  `None` when every agent used pure copy mode.
    pub canonical_path: Option<PathBuf>,
    /// One entry per target agent, in the order the installer visited them.
    pub agents: Vec<AgentOutcome>,
}

impl SkillInstallOutcome {
    fn new(skill: &Skill) -> Self {
        Self {
            skill_name: skill.name.clone(),
            plugin_name: skill.plugin_name.clone(),
            canonical_path: None,
            agents: Vec::new(),
        }
    }

    fn for_wellknown(wk: &WellKnownSkill) -> Self {
        Self {
            skill_name: wk.remote.name.clone(),
            plugin_name: None,
            canonical_path: None,
            agents: Vec::new(),
        }
    }

    pub(super) fn has_success(&self) -> bool {
        self.agents
            .iter()
            .any(|a| !matches!(a.status, AgentInstallStatus::Failed))
    }

    pub(super) fn failed_agents(&self) -> impl Iterator<Item = &str> {
        self.agents.iter().filter_map(|a| match a.status {
            AgentInstallStatus::Failed => Some(a.display_name.as_str()),
            _ => None,
        })
    }

    pub(super) fn symlink_fallback_agents(&self) -> impl Iterator<Item = &str> {
        self.agents.iter().filter_map(|a| match a.status {
            AgentInstallStatus::SymlinkFellBackToCopy { .. } => Some(a.display_name.as_str()),
            _ => None,
        })
    }

    /// Whether every successful agent went through pure copy mode
    /// (i.e. the user passed `--copy`, no canonical symlink exists).
    pub(super) fn is_pure_copy_mode(&self) -> bool {
        self.agents.iter().all(|a| {
            matches!(
                a.status,
                AgentInstallStatus::Copied { .. } | AgentInstallStatus::Failed
            )
        }) && self.has_success()
    }

    /// Paths written in `Copy` + `SymlinkFellBackToCopy` modes, for rendering.
    pub(super) fn copy_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.agents.iter().filter_map(|a| match &a.status {
            AgentInstallStatus::Copied { path }
            | AgentInstallStatus::SymlinkFellBackToCopy { path } => Some(path),
            _ => None,
        })
    }

    /// Display names grouped by category for the summary renderer.
    pub(super) fn by_status(&self) -> AgentStatusGroups<'_> {
        let mut groups = AgentStatusGroups::default();
        for a in &self.agents {
            match a.status {
                AgentInstallStatus::Universal => groups.universal.push(a.display_name.as_str()),
                AgentInstallStatus::Symlinked => groups.symlinked.push(a.display_name.as_str()),
                AgentInstallStatus::Copied { .. } => groups.copied.push(a.display_name.as_str()),
                AgentInstallStatus::SymlinkFellBackToCopy { .. } => {
                    groups.symlink_failed.push(a.display_name.as_str());
                }
                AgentInstallStatus::Failed => {}
            }
        }
        groups
    }
}

/// Per-status buckets of agent display names for summary rendering.
#[derive(Debug, Default)]
pub(super) struct AgentStatusGroups<'a> {
    /// Agents installed via the universal `.agents/skills` directory.
    pub(super) universal: Vec<&'a str>,
    /// Agents symlinked into the canonical tree.
    pub(super) symlinked: Vec<&'a str>,
    /// Agents explicitly copied (`--copy`).
    pub(super) copied: Vec<&'a str>,
    /// Agents that attempted a symlink and fell back to a copy.
    pub(super) symlink_failed: Vec<&'a str>,
}

pub(super) async fn resolve_source(
    parsed: &skill::types::ParsedSource,
) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if parsed.source_type == SourceType::Local {
        let local_path = parsed
            .local_path
            .as_ref()
            .ok_or_else(|| miette!("Local path not resolved"))?;
        if !local_path.exists() {
            return Err(miette!(
                "Local path does not exist: {}",
                local_path.display()
            ));
        }
        return Ok((local_path.clone(), None));
    }

    // Try the blob install fast-path first for GitHub sources.
    //
    // Downloads only the target skill folder via the GitHub Trees API +
    // raw.githubusercontent.com, avoiding full `git clone` for large repos
    // (the mitigation upstream introduced for issues like heygen-com's
    // hyperframes, vercel-labs/skills#300).
    //
    // `try_blob_install` returns `Ok(None)` on any non-fatal error so we
    // silently fall back to `git clone` in that case.
    if parsed.source_type == SourceType::Github
        && let Some(owner_repo) = skill::source::get_owner_repo(parsed)
    {
        // For shorthand `owner/repo/subpath`, `subpath` already holds the
        // right prefix; for tree URLs the subpath was extracted from the
        // path, so it also works without extra massaging.
        let token = skill::github::get_token();
        match skill::blob::try_blob_install(
            &owner_repo,
            parsed.subpath.as_deref(),
            parsed.git_ref.as_deref(),
            token.as_deref(),
        )
        .await
        {
            Ok(Some(td)) => {
                let path = td.path().to_path_buf();
                return Ok((path, Some(td)));
            }
            Ok(None) => {
                tracing::debug!(
                    owner_repo,
                    "blob install skipped, falling back to git clone"
                );
            }
            Err(e) => {
                tracing::debug!(owner_repo, error = %e, "blob install failed, falling back to git clone");
            }
        }
    }

    let td = skill::git::clone_repo(&parsed.url, parsed.git_ref.as_deref())
        .await
        .map_err(|e| miette!("{e}"))?;
    let path = td.path().to_path_buf();
    Ok((path, Some(td)))
}

/// Install skills for all target agents and collect per-skill outcomes.
pub(super) async fn do_install(
    manager: &SkillManager,
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    install_opts: &InstallOptions,
) -> Vec<SkillInstallOutcome> {
    let mut outcomes = Vec::new();

    for skill_item in selected_skills {
        let mut outcome = SkillInstallOutcome::new(skill_item);

        for agent_id in target_agents {
            let display_name = manager
                .agents()
                .get(agent_id)
                .map_or_else(|| agent_id.to_string(), |c| c.display_name.clone());

            let status = match manager
                .install_skill(skill_item, agent_id, install_opts)
                .await
            {
                Ok(result) => {
                    record_canonical(&mut outcome, &result);
                    classify(manager, agent_id, &result)
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %skill_item.name,
                        agent = %agent_id,
                        error = %e,
                        "install failed"
                    );
                    AgentInstallStatus::Failed
                }
            };

            outcome.agents.push(AgentOutcome {
                display_name,
                status,
            });
        }

        outcomes.push(outcome);
    }

    outcomes
}

fn record_canonical(outcome: &mut SkillInstallOutcome, result: &InstallResult) {
    if outcome.canonical_path.is_some() {
        return;
    }
    outcome.canonical_path = result
        .canonical_path
        .clone()
        .or_else(|| Some(result.path.clone()));
}

fn classify(
    manager: &SkillManager,
    agent_id: &AgentId,
    result: &InstallResult,
) -> AgentInstallStatus {
    if manager.agents().is_universal(agent_id) {
        AgentInstallStatus::Universal
    } else if result.symlink_failed {
        AgentInstallStatus::SymlinkFellBackToCopy {
            path: result.path.clone(),
        }
    } else if result.mode == InstallMode::Copy {
        AgentInstallStatus::Copied {
            path: result.path.clone(),
        }
    } else {
        AgentInstallStatus::Symlinked
    }
}

/// Install well-known skills (from HTTP-based providers).
pub(super) async fn install_wellknown_skills(
    wk_skills: &[WellKnownSkill],
    target_agents: &[AgentId],
    manager: &SkillManager,
    install_opts: &InstallOptions,
) -> Vec<SkillInstallOutcome> {
    let mut outcomes = Vec::new();

    for wk in wk_skills {
        let mut outcome = SkillInstallOutcome::for_wellknown(wk);

        for agent_id in target_agents {
            let display_name = manager
                .agents()
                .get(agent_id)
                .map_or_else(|| agent_id.to_string(), |c| c.display_name.clone());

            let Some(agent) = manager.agents().get(agent_id) else {
                outcome.agents.push(AgentOutcome {
                    display_name,
                    status: AgentInstallStatus::Failed,
                });
                continue;
            };

            let status = skill::installer::install_wellknown_skill_files(
                &wk.remote.install_name,
                &wk.files,
                agent,
                manager.agents(),
                install_opts,
            )
            .await
            .map_or(AgentInstallStatus::Failed, |result| {
                record_canonical(&mut outcome, &result);
                classify(manager, agent_id, &result)
            });

            outcome.agents.push(AgentOutcome {
                display_name,
                status,
            });
        }

        outcomes.push(outcome);
    }

    outcomes
}

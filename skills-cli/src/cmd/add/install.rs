//! Installation execution logic for the `add` command.

use std::path::PathBuf;

use miette::{Result, miette};

use skill::SkillManager;
use skill::types::{
    AgentId, InstallMode, InstallOptions, InstallResult, Skill, SourceType, WellKnownSkill,
};

pub(super) struct SkillInstallOutcome {
    pub skill_name: String,
    pub plugin_name: Option<String>,
    pub canonical_path: Option<PathBuf>,
    pub universal_agents: Vec<String>,
    pub symlinked_agents: Vec<String>,
    pub copied_agents: Vec<String>,
    pub symlink_failed_agents: Vec<String>,
    pub failed_agents: Vec<String>,
    pub copy_paths: Vec<PathBuf>,
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
        let mut outcome = SkillInstallOutcome {
            skill_name: skill_item.name.clone(),
            plugin_name: skill_item.plugin_name.clone(),
            canonical_path: None,
            universal_agents: Vec::new(),
            symlinked_agents: Vec::new(),
            copied_agents: Vec::new(),
            symlink_failed_agents: Vec::new(),
            failed_agents: Vec::new(),
            copy_paths: Vec::new(),
        };

        for agent_id in target_agents {
            let display_name = manager
                .agents()
                .get(agent_id)
                .map_or_else(|| agent_id.to_string(), |c| c.display_name.clone());

            match manager
                .install_skill(skill_item, agent_id, install_opts)
                .await
            {
                Ok(result) => {
                    classify_result(manager, agent_id, &result, &display_name, &mut outcome);
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %skill_item.name,
                        agent = %agent_id,
                        error = %e,
                        "install failed"
                    );
                    outcome.failed_agents.push(display_name);
                }
            }
        }

        outcomes.push(outcome);
    }

    outcomes
}

fn classify_result(
    manager: &SkillManager,
    agent_id: &AgentId,
    result: &InstallResult,
    display_name: &str,
    outcome: &mut SkillInstallOutcome,
) {
    if outcome.canonical_path.is_none() {
        outcome.canonical_path = result
            .canonical_path
            .clone()
            .or_else(|| Some(result.path.clone()));
    }

    if manager.agents().is_universal(agent_id) {
        outcome.universal_agents.push(display_name.to_owned());
    } else if result.symlink_failed {
        outcome.symlink_failed_agents.push(display_name.to_owned());
        outcome.copy_paths.push(result.path.clone());
    } else if result.mode == InstallMode::Copy {
        outcome.copied_agents.push(display_name.to_owned());
        outcome.copy_paths.push(result.path.clone());
    } else {
        outcome.symlinked_agents.push(display_name.to_owned());
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
        let mut outcome = SkillInstallOutcome {
            skill_name: wk.remote.name.clone(),
            plugin_name: None,
            canonical_path: None,
            universal_agents: Vec::new(),
            symlinked_agents: Vec::new(),
            copied_agents: Vec::new(),
            symlink_failed_agents: Vec::new(),
            failed_agents: Vec::new(),
            copy_paths: Vec::new(),
        };

        for agent_id in target_agents {
            let display_name = manager
                .agents()
                .get(agent_id)
                .map_or_else(|| agent_id.to_string(), |c| c.display_name.clone());

            let Some(agent) = manager.agents().get(agent_id) else {
                outcome.failed_agents.push(display_name);
                continue;
            };

            match skill::installer::install_wellknown_skill_files(
                &wk.remote.install_name,
                &wk.files,
                agent,
                manager.agents(),
                install_opts,
            )
            .await
            {
                Ok(result) => {
                    classify_result(manager, agent_id, &result, &display_name, &mut outcome);
                }
                Err(_) => {
                    outcome.failed_agents.push(display_name);
                }
            }
        }

        outcomes.push(outcome);
    }

    outcomes
}

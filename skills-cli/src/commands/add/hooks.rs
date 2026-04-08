//! Post-install hooks: telemetry, lock file updates, security prompts.

use std::collections::HashMap;
use std::path::Path;

use miette::{IntoDiagnostic, Result, miette};
use skill::types::{AgentId, InstallScope, Skill, SourceType, WellKnownSkill};

use crate::ui::{self, DIM, RESET, TEXT};

/// Send install telemetry for git-based sources.
pub(super) fn send_telemetry(
    parsed: &skill::types::ParsedSource,
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    scope: InstallScope,
    is_private: Option<bool>,
) {
    if is_private.unwrap_or(false) {
        return;
    }

    let Some(source_str) = skill::source::get_owner_repo(parsed) else {
        return;
    };

    let skills_csv = selected_skills
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let agents_csv = target_agents
        .iter()
        .map(|a| a.as_str().to_owned())
        .collect::<Vec<_>>()
        .join(",");

    let mut props = HashMap::new();
    props.insert("source".to_owned(), source_str);
    props.insert("skills".to_owned(), skills_csv);
    props.insert("agents".to_owned(), agents_csv);
    props.insert("sourceType".to_owned(), parsed.source_type.to_string());
    if scope == InstallScope::Global {
        props.insert("global".to_owned(), "1".to_owned());
    }

    // Include skillFiles mapping (skill name → relative path from source)
    let skill_files: HashMap<&str, String> = selected_skills
        .iter()
        .map(|s| {
            let rel = parsed
                .subpath
                .as_deref()
                .map_or_else(|| s.name.clone(), |sp| format!("{sp}/{}", s.name));
            (s.name.as_str(), rel)
        })
        .collect();
    if let Ok(json) = serde_json::to_string(&skill_files) {
        props.insert("skillFiles".to_owned(), json);
    }

    skill::telemetry::track("install", props);
}

/// Send install telemetry for well-known sources.
pub(super) fn send_wellknown_telemetry(
    wk_skills: &[WellKnownSkill],
    target_agents: &[AgentId],
    scope: InstallScope,
) {
    for wk in wk_skills {
        let mut props = HashMap::new();
        props.insert("source".to_owned(), wk.remote.source_identifier.clone());
        props.insert("skills".to_owned(), wk.remote.name.clone());
        props.insert(
            "agents".to_owned(),
            target_agents
                .iter()
                .map(|a| a.as_str().to_owned())
                .collect::<Vec<_>>()
                .join(","),
        );
        props.insert("sourceType".to_owned(), "well-known".to_owned());
        if scope == InstallScope::Global {
            props.insert("global".to_owned(), "1".to_owned());
        }
        skill::telemetry::track("install", props);
    }
}

/// Check if the source is a private GitHub repository.
///
/// Returns `Some(true)` for private repos, `Some(false)` for public,
/// `None` if unknown. Prompts the user for confirmation when private.
pub(super) async fn prompt_security_advisory(
    parsed: &skill::types::ParsedSource,
    yes: bool,
) -> Result<Option<bool>> {
    if parsed.source_type != SourceType::Github {
        return Ok(None);
    }

    let Some(owner_repo) = skill::source::get_owner_repo(parsed) else {
        return Ok(None);
    };
    let Some((owner, repo)) = skill::source::parse_owner_repo(&owner_repo) else {
        return Ok(None);
    };

    let is_private = skill::lock::is_repo_private(&owner, &repo)
        .await
        .ok()
        .flatten();

    if is_private == Some(true) && !yes {
        println!();
        println!(
            "\x1b[33m\u{26a0}  Security notice:\x1b[0m {TEXT}{owner}/{repo}{RESET} is a \x1b[33m\x1b[1mprivate\x1b[0m repository."
        );
        println!(
            "{DIM}   Skills run with full agent permissions. Private repos cannot be audited by others.{RESET}"
        );
        println!();

        ui::drain_input_events();
        let confirmed: bool = cliclack::confirm("Continue with installation?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            println!("{DIM}Installation cancelled{RESET}");
            return Err(miette!("Installation cancelled by user"));
        }
    }

    Ok(is_private)
}

/// Prompt user to install the find-skills skill on first use.
pub(super) async fn prompt_for_find_skills(
    manager: &skill::SkillManager,
    target_agents: &[AgentId],
) {
    if skill::lock::is_prompt_dismissed("findSkillsPrompt")
        .await
        .unwrap_or(true)
    {
        return;
    }

    if let Some(agent) = manager.agents().get(&AgentId::new("claude-code"))
        && skill::installer::is_skill_installed(
            "find-skills",
            agent,
            InstallScope::Global,
            &std::env::current_dir().unwrap_or_default(),
        )
        .await
    {
        drop(skill::lock::dismiss_prompt("findSkillsPrompt").await);
        return;
    }

    println!();
    drop(cliclack::log::remark(format!(
        "{DIM}One-time prompt - you won't be asked again if you dismiss.{RESET}"
    )));
    ui::drain_input_events();
    let Ok(yes) = cliclack::confirm(
        "Install the \x1b[36mfind-skills\x1b[0m skill? It helps your agent discover and suggest skills."
    )
        .initial_value(true)
        .interact()
    else {
        drop(skill::lock::dismiss_prompt("findSkillsPrompt").await);
        return;
    };

    drop(skill::lock::dismiss_prompt("findSkillsPrompt").await);
    if yes {
        let agents: Vec<String> = target_agents
            .iter()
            .filter(|id| id.as_str() != "replit")
            .map(|id| id.as_str().to_owned())
            .collect();
        if agents.is_empty() {
            return;
        }
        println!();
        drop(cliclack::log::step("Installing find-skills skill..."));
        let add_args = super::AddArgs {
            source: vec!["vercel-labs/skills".to_owned()],
            global: Some(true),
            agent: Some(agents),
            skill: Some(vec!["find-skills".to_owned()]),
            list: false,
            yes: true,
            copy: false,
            all: false,
            full_depth: false,
            dry_run: false,
        };
        drop(Box::pin(super::run(add_args)).await);
    } else {
        drop(cliclack::log::remark(format!(
            "{DIM}You can install it later with: skills add vercel-labs/skills@find-skills{RESET}"
        )));
    }
}

/// Update the global lock file after a successful git-based install.
pub(super) async fn update_lock_file(parsed: &skill::types::ParsedSource, skills: &[Skill]) {
    let Some(owner_repo) = skill::source::get_owner_repo(parsed) else {
        return;
    };

    for s in skills {
        let skill_path = parsed
            .subpath
            .as_deref()
            .map(|sp| format!("{}/SKILL.md", sp.trim_end_matches('/')));
        let hash = skill::lock::fetch_skill_folder_hash(
            &owner_repo,
            skill_path.as_deref().unwrap_or(""),
            skill::lock::get_github_token().as_deref(),
        )
        .await
        .unwrap_or(None)
        .unwrap_or_default();

        drop(
            skill::lock::add_skill_to_lock(&skill::lock::AddLockInput {
                name: &s.name,
                source: &owner_repo,
                source_type: &parsed.source_type.to_string(),
                source_url: &parsed.url,
                skill_path: skill_path.as_deref(),
                skill_folder_hash: &hash,
                plugin_name: s.plugin_name.as_deref(),
            })
            .await,
        );
    }
}

/// Update the project-scoped `skills-lock.json` after a successful install.
pub(super) async fn update_local_lock_file(
    parsed: &skill::types::ParsedSource,
    skills: &[Skill],
    cwd: &Path,
) {
    let source = skill::source::get_owner_repo(parsed).unwrap_or_else(|| parsed.url.clone());

    for s in skills {
        let hash = skill::local_lock::compute_skill_folder_hash(&s.path)
            .await
            .unwrap_or_default();

        drop(
            skill::local_lock::add_skill_to_local_lock(
                &s.name,
                skill::local_lock::LocalSkillLockEntry {
                    source: source.clone(),
                    source_type: parsed.source_type.to_string(),
                    computed_hash: hash,
                },
                cwd,
            )
            .await,
        );
    }
}

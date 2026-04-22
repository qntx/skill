//! Skill and agent selection prompts for the `add` command.

use std::collections::BTreeMap;

use miette::{IntoDiagnostic, Result, miette};
use skill::SkillManager;
use skill::types::{AgentId, InstallMode, InstallScope, Skill};

use crate::ui::emit;
use crate::ui::{self, BOLD, CYAN, DIM, RESET, kebab_to_title};

pub(super) fn truncate_hint(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

pub(super) fn select_skills(
    skills: &[Skill],
    skill_filter: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<Skill>> {
    if skill_filter.is_some_and(|s| s.contains(&"*".to_owned())) {
        emit::info(format!("Installing all {} skills", skills.len()));
        return Ok(skills.to_vec());
    }

    if let Some(names) = skill_filter {
        let filtered = skill::skills::filter_skills(skills, names);
        if filtered.is_empty() {
            emit::error(format!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
            emit::info("Available skills:");
            for s in skills {
                emit::remark(format!("  - {}", s.name));
            }
            return Err(miette!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
        }
        let display: Vec<String> = filtered
            .iter()
            .map(|s| format!("{CYAN}{}{RESET}", s.name))
            .collect();
        emit::info(format!(
            "Selected {} skill{}: {}",
            filtered.len(),
            if filtered.len() == 1 { "" } else { "s" },
            display.join(", ")
        ));
        return Ok(filtered);
    }

    if let [s] = skills {
        emit::info(format!("Skill: {CYAN}{}{RESET}", s.name));
        emit::remark(format!("{DIM}{}{RESET}", s.description));
        return Ok(skills.to_vec());
    }

    if yes {
        emit::info(format!("Installing all {} skills", skills.len()));
        return Ok(skills.to_vec());
    }

    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| match (&a.plugin_name, &b.plugin_name) {
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(pa), Some(pb)) if pa != pb => pa.cmp(pb),
        _ => a.name.cmp(&b.name),
    });

    let has_groups = sorted.iter().any(|s| s.plugin_name.is_some());

    if has_groups {
        let mut groups: BTreeMap<String, Vec<(&Skill, String)>> = BTreeMap::new();
        for s in &sorted {
            let group = s
                .plugin_name
                .as_deref()
                .map_or_else(|| "Other".to_owned(), kebab_to_title);
            groups
                .entry(group)
                .or_default()
                .push((s, truncate_hint(&s.description, 60)));
        }

        let mut prompt = cliclack::multiselect(format!(
            "Select skills to install {DIM}(space to toggle){RESET}"
        ));
        for (group, items) in &groups {
            prompt = prompt.item(
                format!("__group__{group}"),
                format!("{BOLD}{group}{RESET}"),
                "",
            );
            for (skill, hint) in items {
                prompt = prompt.item(skill.name.clone(), &skill.name, hint);
            }
        }
        prompt = prompt.required(true);

        ui::drain_input_events();
        let selected_names: Vec<String> = prompt.interact().into_diagnostic()?;
        let selected_names: Vec<String> = selected_names
            .into_iter()
            .filter(|n| !n.starts_with("__group__"))
            .collect();

        if selected_names.is_empty() {
            return Err(miette!("No skills selected"));
        }

        Ok(sorted
            .iter()
            .filter(|s| selected_names.contains(&s.name))
            .cloned()
            .collect())
    } else {
        let mut prompt = cliclack::multiselect(format!(
            "Select skills to install {DIM}(space to toggle){RESET}"
        ));
        for s in &sorted {
            prompt = prompt.item(s.name.clone(), &s.name, truncate_hint(&s.description, 60));
        }
        prompt = prompt.required(true);

        ui::drain_input_events();
        let selected_names: Vec<String> = prompt.interact().into_diagnostic()?;
        if selected_names.is_empty() {
            return Err(miette!("No skills selected"));
        }

        Ok(sorted
            .iter()
            .filter(|s| selected_names.contains(&s.name))
            .cloned()
            .collect())
    }
}

/// Select target agents using the custom search-multiselect component.
///
/// Matches the TS: universal agents in a locked section, detected agents
/// pre-selected, search filtering, last-selection memory.
pub(crate) async fn select_agents(
    manager: &SkillManager,
    agent_arg: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<AgentId>> {
    let all_ids = manager.agents().all_ids();

    if let Some(agents) = select_from_arg(manager, agent_arg, &all_ids)? {
        return Ok(agents);
    }

    let spinner = cliclack::spinner();
    spinner.start("Loading agents...");
    let detected = manager.detect_installed_agents().await;
    spinner.stop(format!("{} agents", all_ids.len()));

    if yes {
        return Ok(select_non_interactive(manager, detected, all_ids));
    }

    if detected.is_empty() {
        return prompt_all_agents(manager, &all_ids).await;
    }

    if detected.len() == 1 {
        return Ok(install_with_single_detected(manager, &detected));
    }

    prompt_with_detected(manager, &detected).await
}

/// Map explicit `--agent` flags to concrete agent IDs, validating each entry.
///
/// Returns `Ok(None)` when no explicit flag was provided so the caller
/// continues with detection-based flows.
fn select_from_arg(
    manager: &SkillManager,
    agent_arg: Option<&Vec<String>>,
    all_ids: &[AgentId],
) -> Result<Option<Vec<AgentId>>> {
    let Some(names) = agent_arg else {
        return Ok(None);
    };

    if names.contains(&"*".to_owned()) {
        emit::info(format!("Installing to all {} agents", all_ids.len()));
        return Ok(Some(all_ids.to_vec()));
    }

    let valid_ids = manager.agents().all_ids();
    let invalid: Vec<_> = names
        .iter()
        .filter(|n| !valid_ids.iter().any(|id| id.as_str() == n.as_str()))
        .collect();
    if !invalid.is_empty() {
        return Err(miette!(
            "Invalid agents: {}\nValid agents: {}",
            invalid
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            valid_ids
                .iter()
                .map(AgentId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(Some(names.iter().map(AgentId::new).collect()))
}

/// `--yes` path: auto-install to detected agents (+ universals) or all.
fn select_non_interactive(
    manager: &SkillManager,
    detected: Vec<AgentId>,
    all_ids: Vec<AgentId>,
) -> Vec<AgentId> {
    if detected.is_empty() {
        emit::info("Installing to all agents");
        return all_ids;
    }
    let agents = ensure_universal_agents(manager, detected);
    let display: Vec<String> = agents
        .iter()
        .filter_map(|id| {
            manager
                .agents()
                .get(id)
                .map(|c| format!("{CYAN}{}{RESET}", c.display_name))
        })
        .collect();
    emit::info(format!("Installing to: {}", display.join(", ")));
    agents
}

/// No agents were auto-detected — let the user pick from the full list.
async fn prompt_all_agents(manager: &SkillManager, all_ids: &[AgentId]) -> Result<Vec<AgentId>> {
    emit::info("Select agents to install skills to");
    let items: Vec<ui::SearchItem> = all_ids
        .iter()
        .filter_map(|id| {
            manager.agents().get(id).map(|c| ui::SearchItem {
                value: id.as_str().to_owned(),
                label: c.display_name.clone(),
                hint: None,
            })
        })
        .collect();

    let result = ui::search_multiselect(&ui::SearchMultiselectOptions {
        message: "Which agents do you want to install to?".to_owned(),
        items,
        max_visible: 8,
        initial_selected: Vec::new(),
        required: true,
        locked_section: None,
    })
    .into_diagnostic()?;

    match result {
        ui::SearchMultiselectResult::Selected(values) => {
            let _ = skill::lock::save_selected_agents(&values).await;
            Ok(values.into_iter().map(AgentId::new).collect())
        }
        ui::SearchMultiselectResult::Cancelled => {
            emit::outro_cancel("Installation cancelled");
            Err(miette!("Installation cancelled"))
        }
    }
}

/// Only one agent detected — install to it (+ universals) without prompting.
fn install_with_single_detected(manager: &SkillManager, detected: &[AgentId]) -> Vec<AgentId> {
    let agents = ensure_universal_agents(manager, detected.to_vec());
    let display_name = detected
        .first()
        .and_then(|id| manager.agents().get(id))
        .map_or_else(String::new, |c| c.display_name.clone());
    emit::info(format!("Installing to: {CYAN}{display_name}{RESET}"));
    agents
}

/// Multiple agents detected — show a prompt with universals locked in and
/// detected ones pre-selected.
async fn prompt_with_detected(
    manager: &SkillManager,
    detected: &[AgentId],
) -> Result<Vec<AgentId>> {
    let universal = manager.agents().universal_agents();
    let non_universal = manager.agents().non_universal_agents();

    let locked = (!universal.is_empty()).then(|| ui::LockedSection {
        title: "Universal agents".to_owned(),
        items: universal
            .iter()
            .filter_map(|id| {
                manager.agents().get(id).map(|c| ui::SearchItem {
                    value: id.as_str().to_owned(),
                    label: c.display_name.clone(),
                    hint: None,
                })
            })
            .collect(),
    });

    let items: Vec<ui::SearchItem> = non_universal
        .iter()
        .filter_map(|id| {
            manager.agents().get(id).map(|c| ui::SearchItem {
                value: id.as_str().to_owned(),
                label: c.display_name.clone(),
                hint: if detected.contains(id) {
                    Some("detected".to_owned())
                } else {
                    None
                },
            })
        })
        .collect();

    let initial: Vec<String> = detected
        .iter()
        .filter(|id| !universal.contains(id))
        .map(|id| id.as_str().to_owned())
        .collect();

    let last_selected = skill::lock::read_last_selected_agents()
        .await
        .unwrap_or(None);
    let initial = last_selected.as_ref().map_or(initial, Clone::clone);

    let result = ui::search_multiselect(&ui::SearchMultiselectOptions {
        message: "Which agents do you want to install to?".to_owned(),
        items,
        max_visible: 8,
        initial_selected: initial,
        required: true,
        locked_section: locked,
    })
    .into_diagnostic()?;

    match result {
        ui::SearchMultiselectResult::Selected(values) => {
            let _ = skill::lock::save_selected_agents(&values).await;
            Ok(values.into_iter().map(AgentId::new).collect())
        }
        ui::SearchMultiselectResult::Cancelled => {
            emit::outro_cancel("Installation cancelled");
            Err(miette!("Installation cancelled"))
        }
    }
}

fn ensure_universal_agents(manager: &SkillManager, mut agents: Vec<AgentId>) -> Vec<AgentId> {
    for ua in manager.agents().universal_agents() {
        if !agents.contains(&ua) {
            agents.push(ua);
        }
    }
    agents
}

/// Resolve installation scope interactively when not specified via flags.
pub(super) fn resolve_scope(
    global_flag: Option<bool>,
    yes: bool,
    target_agents: &[AgentId],
    manager: &SkillManager,
) -> Result<InstallScope> {
    if let Some(g) = global_flag {
        return Ok(if g {
            InstallScope::Global
        } else {
            InstallScope::Project
        });
    }

    if yes {
        return Ok(InstallScope::Project);
    }

    let supports_global = target_agents.iter().any(|a| {
        manager
            .agents()
            .get(a)
            .and_then(|c| c.global_skills_dir.as_ref())
            .is_some()
    });

    if !supports_global {
        return Ok(InstallScope::Project);
    }

    ui::drain_input_events();
    let scope: bool = cliclack::select("Installation scope")
        .item(
            false,
            "Project",
            "Install in current directory (committed with your project)",
        )
        .item(
            true,
            "Global",
            "Install in home directory (available across all projects)",
        )
        .interact()
        .into_diagnostic()?;

    Ok(if scope {
        InstallScope::Global
    } else {
        InstallScope::Project
    })
}

/// Resolve installation mode interactively when not specified via flags.
pub(super) fn resolve_mode(copy_flag: bool, yes: bool) -> Result<InstallMode> {
    if copy_flag {
        return Ok(InstallMode::Copy);
    }
    if yes {
        return Ok(InstallMode::Symlink);
    }

    ui::drain_input_events();
    let mode: InstallMode = cliclack::select("Installation method")
        .item(
            InstallMode::Symlink,
            "Symlink (Recommended)",
            "Single source of truth, easy updates",
        )
        .item(
            InstallMode::Copy,
            "Copy to all agents",
            "Independent copies for each agent",
        )
        .interact()
        .into_diagnostic()?;

    Ok(mode)
}

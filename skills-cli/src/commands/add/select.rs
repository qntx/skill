//! Skill and agent selection prompts for the `add` command.

use std::collections::BTreeMap;

use miette::{IntoDiagnostic, Result, miette};
use skill::SkillManager;
use skill::types::{AgentId, InstallMode, InstallScope, Skill};

use crate::ui::{self, DIM, RESET};

pub(super) fn kebab_to_title(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |first| {
                let upper: String = first.to_uppercase().collect();
                upper + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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
        let _ = cliclack::log::info(format!("Installing all {} skills", skills.len()));
        return Ok(skills.to_vec());
    }

    if let Some(names) = skill_filter {
        let filtered = skill::skills::filter_skills(skills, names);
        if filtered.is_empty() {
            let _ = cliclack::log::error(format!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
            let _ = cliclack::log::info("Available skills:");
            for s in skills {
                let _ = cliclack::log::remark(format!("  - {}", s.name));
            }
            return Err(miette!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
        }
        let display: Vec<String> = filtered
            .iter()
            .map(|s| format!("\x1b[36m{}\x1b[0m", s.name))
            .collect();
        let _ = cliclack::log::info(format!(
            "Selected {} skill{}: {}",
            filtered.len(),
            if filtered.len() == 1 { "" } else { "s" },
            display.join(", ")
        ));
        return Ok(filtered);
    }

    if skills.len() == 1 {
        let s = &skills[0];
        let _ = cliclack::log::info(format!("Skill: \x1b[36m{}\x1b[0m", s.name));
        let _ = cliclack::log::remark(format!("{DIM}{}{RESET}", s.description));
        return Ok(skills.to_vec());
    }

    if yes {
        let _ = cliclack::log::info(format!("Installing all {} skills", skills.len()));
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
                format!("\x1b[1m{group}\x1b[0m"),
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

    if agent_arg.is_some_and(|a| a.contains(&"*".to_owned())) {
        let _ = cliclack::log::info(format!("Installing to all {} agents", all_ids.len()));
        return Ok(all_ids);
    }

    if let Some(names) = agent_arg {
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
        return Ok(names.iter().map(AgentId::new).collect());
    }

    let spinner = cliclack::spinner();
    spinner.start("Loading agents...");
    let detected = manager.detect_installed_agents().await;
    let total_agents = all_ids.len();
    spinner.stop(format!("{total_agents} agents"));

    if yes {
        return Ok(if detected.is_empty() {
            let _ = cliclack::log::info("Installing to all agents");
            all_ids
        } else {
            let agents = ensure_universal_agents(manager, detected.clone());
            let display: Vec<String> = agents
                .iter()
                .filter_map(|id| {
                    manager
                        .agents()
                        .get(id)
                        .map(|c| format!("\x1b[36m{}\x1b[0m", c.display_name))
                })
                .collect();
            let _ = cliclack::log::info(format!("Installing to: {}", display.join(", ")));
            agents
        });
    }

    if detected.is_empty() {
        let _ = cliclack::log::info("Select agents to install skills to");
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

        return match result {
            ui::SearchMultiselectResult::Selected(values) => {
                let _ = skill::lock::save_selected_agents(&values).await;
                Ok(values.into_iter().map(AgentId::new).collect())
            }
            ui::SearchMultiselectResult::Cancelled => {
                let _ = cliclack::outro_cancel("Installation cancelled");
                std::process::exit(0);
            }
        };
    }

    if detected.len() == 1 {
        let agents = ensure_universal_agents(manager, detected.clone());
        let display_name = detected
            .first()
            .and_then(|id| manager.agents().get(id))
            .map_or_else(String::new, |c| c.display_name.clone());
        let _ = cliclack::log::info(format!("Installing to: \x1b[36m{display_name}\x1b[0m"));
        return Ok(agents);
    }

    let universal = manager.agents().universal_agents();
    let non_universal = manager.agents().non_universal_agents();

    let locked = if universal.is_empty() {
        None
    } else {
        Some(ui::LockedSection {
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
        })
    };

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

    let last_selected = skill::lock::get_last_selected_agents()
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
            let _ = cliclack::outro_cancel("Installation cancelled");
            std::process::exit(0);
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

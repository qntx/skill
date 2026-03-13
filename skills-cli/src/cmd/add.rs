//! `skills add <source>` command implementation.
//!
//! Matches the `TypeScript` `add.ts` UX: cliclack prompts for skill and
//! agent selection, scope and mode prompts when not specified via flags,
//! installation summary display, and plain ANSI output for results.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use clap::Args;
use miette::{IntoDiagnostic, Result, miette};

use skill::SkillManager;
use skill::types::{
    AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallResult, InstallScope, Skill,
    SourceType, WellKnownSkill,
};

use crate::ui;

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

/// Arguments for the `add` command.
#[derive(Args)]
pub struct AddArgs {
    /// Source(s) to install from (e.g. `owner/repo`, URL, local path).
    pub source: Vec<String>,

    /// Install globally (user-level) instead of project-level.
    #[arg(short, long, default_missing_value = "true", num_args = 0)]
    pub global: Option<bool>,

    /// Target agents (use `*` for all).
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Install specific skills (use `*` for all).
    #[arg(short, long, num_args = 1..)]
    pub skill: Option<Vec<String>>,

    /// List available skills without installing.
    #[arg(short, long)]
    pub list: bool,

    /// Skip confirmation prompts.
    #[arg(short, long)]
    pub yes: bool,

    /// Copy files instead of symlinking.
    #[arg(long)]
    pub copy: bool,

    /// Shorthand for `--skill '*' --agent '*' -y`.
    #[arg(long)]
    pub all: bool,

    /// Search all subdirectories even when a root `SKILL.md` exists.
    #[arg(long)]
    pub full_depth: bool,
}

fn missing_source_error() -> miette::Report {
    miette!(
        help = "Usage: skills add <source> [options]\nExample: skills add qntx/skills",
        "Missing required argument: source"
    )
}

// ── Skill selection ─────────────────────────────────────────────────

fn kebab_to_title(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + c.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_hint(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn select_skills(
    skills: &[Skill],
    skill_filter: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<Skill>> {
    if skill_filter.is_some_and(|s| s.contains(&"*".to_owned())) {
        println!("{TEXT}Installing all {} skills{RESET}", skills.len());
        return Ok(skills.to_vec());
    }

    if let Some(names) = skill_filter {
        let filtered = skill::skills::filter_skills(skills, names);
        if filtered.is_empty() {
            println!("{DIM}Available skills:{RESET}");
            for s in skills {
                println!("  {DIM}- {}{RESET}", s.name);
            }
            return Err(miette!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
        }
        let display: Vec<String> = filtered.iter().map(|s| s.name.clone()).collect();
        println!(
            "{TEXT}Selected {} skill{}: {}{RESET}",
            filtered.len(),
            if filtered.len() != 1 { "s" } else { "" },
            display.join(", ")
        );
        return Ok(filtered);
    }

    if skills.len() == 1 {
        let s = &skills[0];
        println!("{TEXT}Skill: {}{RESET}", s.name);
        println!("{DIM}{}{RESET}", s.description);
        return Ok(skills.to_vec());
    }

    if yes {
        println!("{TEXT}Installing all {} skills{RESET}", skills.len());
        return Ok(skills.to_vec());
    }

    // Sort by plugin name then skill name (matches TS).
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| match (&a.plugin_name, &b.plugin_name) {
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(pa), Some(pb)) if pa != pb => pa.cmp(pb),
        _ => a.name.cmp(&b.name),
    });

    let has_groups = sorted.iter().any(|s| s.plugin_name.is_some());

    if has_groups {
        // Build grouped options: group header label → items
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
            // Add group header as a non-selectable separator via label styling
            prompt = prompt.item(
                format!("__group__{group}"),
                &format!("\x1b[1m{group}\x1b[0m"),
                "",
            );
            for (skill, hint) in items {
                prompt = prompt.item(skill.name.clone(), &skill.name, hint);
            }
        }
        prompt = prompt.required(true);

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
        let mut prompt = cliclack::multiselect("Select skills to install");
        for s in &sorted {
            prompt = prompt.item(s.name.clone(), &s.name, &truncate_hint(&s.description, 60));
        }
        prompt = prompt.required(true);

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

// ── Agent selection ─────────────────────────────────────────────────

/// Select target agents using the custom search-multiselect component.
///
/// Matches the TS: universal agents in a locked section, detected agents
/// pre-selected, search filtering, last-selection memory.
pub async fn select_agents(
    manager: &SkillManager,
    agent_arg: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<AgentId>> {
    let all_ids = manager.agents().all_ids();

    if agent_arg.is_some_and(|a| a.contains(&"*".to_owned())) {
        println!("{TEXT}Installing to all {} agents{RESET}", all_ids.len());
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
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return Ok(names.iter().map(AgentId::new).collect());
    }

    let detected = manager.detect_installed_agents().await;

    // Matches TS: --yes with detected → auto-select; --yes without → all agents.
    if yes {
        return Ok(if detected.is_empty() {
            println!("{TEXT}Installing to all agents{RESET}");
            all_ids
        } else {
            let agents = ensure_universal_agents(manager, detected.clone());
            let display: Vec<String> = agents
                .iter()
                .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
                .collect();
            println!("{TEXT}Installing to: {}{RESET}", display.join(", "));
            agents
        });
    }

    // Matches TS: 0 detected → prompt with ALL agents (no locked section).
    if detected.is_empty() {
        println!("{TEXT}Select agents to install skills to{RESET}");
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
                println!("{DIM}Installation cancelled{RESET}");
                std::process::exit(0);
            }
        };
    }

    // Matches TS: exactly 1 detected → auto-select (+ universal), no prompt.
    if detected.len() == 1 {
        let agents = ensure_universal_agents(manager, detected.clone());
        let display_name = detected
            .first()
            .and_then(|id| manager.agents().get(id))
            .map_or_else(String::new, |c| c.display_name.clone());
        println!("{TEXT}Installing to: {display_name}{RESET}");
        return Ok(agents);
    }

    // >1 detected → interactive search-multiselect with universal locked section.
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
            println!("{DIM}Installation cancelled{RESET}");
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
/// Matches TS: prompt for Project vs Global when `options.global === undefined`.
fn resolve_scope(
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
/// Matches TS: prompt for Symlink vs Copy when `!options.copy && !options.yes`.
fn resolve_mode(copy_flag: bool, yes: bool) -> Result<InstallMode> {
    if copy_flag {
        return Ok(InstallMode::Copy);
    }
    if yes {
        return Ok(InstallMode::Symlink);
    }

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

/// Print a pre-confirmation installation summary box (matches TS `p.note(..., 'Installation Summary')`).
fn print_installation_summary(
    skills: &[Skill],
    agents: &[AgentId],
    manager: &SkillManager,
    scope: InstallScope,
    mode: InstallMode,
    cwd: &Path,
) {
    let mut lines: Vec<String> = Vec::new();

    // Group skills by plugin name for summary.
    let mut grouped: BTreeMap<String, Vec<&Skill>> = BTreeMap::new();
    let mut ungrouped: Vec<&Skill> = Vec::new();
    for s in skills {
        if let Some(ref plugin) = s.plugin_name {
            grouped.entry(plugin.clone()).or_default().push(s);
        } else {
            ungrouped.push(s);
        }
    }

    let print_skill_summary = |lines: &mut Vec<String>, skill_list: &[&Skill]| {
        for s in skill_list {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            let canonical = skill::installer::get_canonical_path(&s.name, scope, cwd);
            let short = ui::shorten_path_with_cwd(&canonical, cwd);
            lines.push(format!("\x1b[36m{short}\x1b[0m"));
            lines.extend(build_agent_summary_lines(agents, manager, mode));
        }
    };

    for (group, skill_list) in &grouped {
        let title = kebab_to_title(group);
        lines.push(String::new());
        lines.push(format!("\x1b[1m{title}\x1b[0m"));
        print_skill_summary(&mut lines, skill_list);
    }

    if !ungrouped.is_empty() {
        if !grouped.is_empty() {
            lines.push(String::new());
            lines.push("\x1b[1mGeneral\x1b[0m".to_owned());
        }
        print_skill_summary(&mut lines, &ungrouped);
    }

    // Remove leading empty line if present.
    if lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }

    println!();
    println!("{DIM}╭ {TEXT}Installation Summary{RESET} {DIM}─{RESET}");
    for line in &lines {
        println!("{DIM}│{RESET}  {line}");
    }
    println!("{DIM}╰─{RESET}");
}

fn build_agent_summary_lines(
    agents: &[AgentId],
    manager: &SkillManager,
    mode: InstallMode,
) -> Vec<String> {
    let mut lines = Vec::new();

    if mode == InstallMode::Copy {
        let names: Vec<String> = agents
            .iter()
            .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
            .collect();
        lines.push(format!("  {DIM}copied:{RESET} {}", ui::format_list(&names)));
        return lines;
    }

    let universal_names: Vec<String> = agents
        .iter()
        .filter(|a| manager.agents().is_universal(a))
        .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
        .collect();
    let symlinked_names: Vec<String> = agents
        .iter()
        .filter(|a| !manager.agents().is_universal(a))
        .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
        .collect();

    if !universal_names.is_empty() {
        lines.push(format!(
            "  {GREEN}universal:{RESET} {}",
            ui::format_list(&universal_names)
        ));
    }
    if !symlinked_names.is_empty() {
        lines.push(format!(
            "  {DIM}symlinked:{RESET} {}",
            ui::format_list(&symlinked_names)
        ));
    }
    lines
}

// ── Per-skill install result, for grouped output ────────────────────

struct SkillInstallOutcome {
    #[allow(dead_code)]
    skill_name: String,
    canonical_path: Option<PathBuf>,
    universal_agents: Vec<String>,
    symlinked_agents: Vec<String>,
    copied_agents: Vec<String>,
    failed_agents: Vec<String>,
}

/// Install skills for all target agents and collect per-skill outcomes.
async fn do_install(
    manager: &SkillManager,
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    install_opts: &InstallOptions,
) -> Vec<SkillInstallOutcome> {
    let mut outcomes = Vec::new();

    for skill_item in selected_skills {
        let mut outcome = SkillInstallOutcome {
            skill_name: skill_item.name.clone(),
            canonical_path: None,
            universal_agents: Vec::new(),
            symlinked_agents: Vec::new(),
            copied_agents: Vec::new(),
            failed_agents: Vec::new(),
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
                Ok(result) if result.success => {
                    classify_result(manager, agent_id, &result, &display_name, &mut outcome);
                }
                Ok(result) => {
                    tracing::warn!(
                        skill = %skill_item.name,
                        agent = %agent_id,
                        error = result.error.as_deref().unwrap_or("unknown"),
                        "install failed"
                    );
                    outcome.failed_agents.push(display_name);
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
    } else if result.symlink_failed || result.mode == InstallMode::Copy {
        outcome.copied_agents.push(display_name.to_owned());
    } else {
        outcome.symlinked_agents.push(display_name.to_owned());
    }
}

/// Install well-known skills (from HTTP-based providers).
async fn install_wellknown_skills(
    wk_skills: &[WellKnownSkill],
    target_agents: &[AgentId],
    manager: &SkillManager,
    install_opts: &InstallOptions,
) -> Vec<SkillInstallOutcome> {
    let mut outcomes = Vec::new();

    for wk in wk_skills {
        let mut outcome = SkillInstallOutcome {
            skill_name: wk.remote.name.clone(),
            canonical_path: None,
            universal_agents: Vec::new(),
            symlinked_agents: Vec::new(),
            copied_agents: Vec::new(),
            failed_agents: Vec::new(),
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
                Ok(result) if result.success => {
                    classify_result(manager, agent_id, &result, &display_name, &mut outcome);
                }
                Ok(_) | Err(_) => {
                    outcome.failed_agents.push(display_name);
                }
            }
        }

        outcomes.push(outcome);
    }

    outcomes
}

// ── Output formatting ───────────────────────────────────────────────

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";

fn print_install_results(
    outcomes: &[SkillInstallOutcome],
    cwd: &Path,
    target_agents: &[AgentId],
    manager: &SkillManager,
) {
    let successful: Vec<&SkillInstallOutcome> = outcomes
        .iter()
        .filter(|o| {
            !o.universal_agents.is_empty()
                || !o.symlinked_agents.is_empty()
                || !o.copied_agents.is_empty()
        })
        .collect();
    let failed_outcomes: Vec<&SkillInstallOutcome> = outcomes
        .iter()
        .filter(|o| !o.failed_agents.is_empty())
        .collect();

    if !successful.is_empty() {
        let mut result_lines: Vec<String> = Vec::new();

        for outcome in &successful {
            if let Some(ref canonical) = outcome.canonical_path {
                let short = ui::shorten_path_with_cwd(canonical, cwd);
                result_lines.push(format!("{GREEN}✓{RESET} {short}"));
            } else {
                result_lines.push(format!("{GREEN}✓{RESET} {}", outcome.skill_name));
            }

            if !outcome.universal_agents.is_empty() {
                result_lines.push(format!(
                    "  {GREEN}universal:{RESET} {}",
                    ui::format_list(&outcome.universal_agents)
                ));
            }
            if !outcome.symlinked_agents.is_empty() {
                result_lines.push(format!(
                    "  {DIM}symlinked:{RESET} {}",
                    ui::format_list(&outcome.symlinked_agents)
                ));
            }
            if !outcome.copied_agents.is_empty() {
                result_lines.push(format!(
                    "  {YELLOW}copied:{RESET} {}",
                    ui::format_list(&outcome.copied_agents)
                ));
            }
        }

        let skill_count = successful.len();
        let title = format!(
            "{GREEN}Installed {} skill{}{RESET}",
            skill_count,
            if skill_count == 1 { "" } else { "s" }
        );

        // Print clack-style note box (matches TS p.note).
        println!();
        println!("{DIM}╭ {title} {DIM}─{RESET}");
        for line in &result_lines {
            println!("{DIM}│{RESET}  {line}");
        }
        println!("{DIM}╰─{RESET}");

        // Symlink failure warning (matches TS).
        let all_copied: Vec<&str> = outcomes
            .iter()
            .flat_map(|o| o.copied_agents.iter())
            .map(String::as_str)
            .collect();
        if !all_copied.is_empty()
            && target_agents
                .iter()
                .any(|a| !manager.agents().is_universal(a))
        {
            println!(
                "{YELLOW}⚠{RESET} {YELLOW}Symlinks failed for: {}{RESET}",
                ui::format_list(
                    &all_copied
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                )
            );
            println!(
                "{DIM}  Files were copied instead. On Windows, enable Developer Mode for symlink support.{RESET}"
            );
        }
    }

    if !failed_outcomes.is_empty() {
        let total_fail: usize = failed_outcomes.iter().map(|o| o.failed_agents.len()).sum();
        println!();
        println!("\x1b[31m✗ Failed to install {total_fail}{RESET}");
        for outcome in &failed_outcomes {
            for agent in &outcome.failed_agents {
                println!(
                    "  \x1b[31m✗{RESET} {} → {agent}: {DIM}installation error{RESET}",
                    outcome.skill_name
                );
            }
        }
    }
}

// ── Source resolution ───────────────────────────────────────────────

async fn resolve_source(
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

    println!("{TEXT}Cloning repository...{RESET}");
    let td = skill::git::clone_repo(&parsed.url, parsed.git_ref.as_deref())
        .await
        .map_err(|e| miette!("{e}"))?;
    let path = td.path().to_path_buf();
    Ok((path, Some(td)))
}

// ── Telemetry ───────────────────────────────────────────────────────

fn send_telemetry(
    parsed: &skill::types::ParsedSource,
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    scope: InstallScope,
) {
    let Some(source_str) = skill::source::get_owner_repo(parsed) else {
        return;
    };
    let mut props = HashMap::new();
    props.insert("source".to_owned(), source_str);
    props.insert(
        "skills".to_owned(),
        selected_skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(","),
    );
    props.insert(
        "agents".to_owned(),
        target_agents
            .iter()
            .map(|a| a.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(","),
    );
    if scope == InstallScope::Global {
        props.insert("global".to_owned(), "1".to_owned());
    }
    skill::telemetry::track("install", props);
}

fn send_wellknown_telemetry(
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

/// Warn when installing from a private GitHub repository.
///
/// Matches TS `promptSecurityAdvisory`: skills run with full agent
/// permissions; a private repo makes third-party auditing impossible.
async fn prompt_security_advisory(parsed: &skill::types::ParsedSource, yes: bool) -> Result<()> {
    if yes || parsed.source_type != SourceType::Github {
        return Ok(());
    }

    let Some(owner_repo) = skill::source::get_owner_repo(parsed) else {
        return Ok(());
    };
    let Some((owner, repo)) = skill::source::parse_owner_repo(&owner_repo) else {
        return Ok(());
    };

    let is_private = skill::lock::is_repo_private(&owner, &repo)
        .await
        .ok()
        .flatten();

    if is_private == Some(true) {
        println!();
        println!(
            "\x1b[33m⚠  Security notice:\x1b[0m {TEXT}{owner}/{repo}{RESET} is a \x1b[33m\x1b[1mprivate\x1b[0m repository."
        );
        println!(
            "{DIM}   Skills run with full agent permissions. Private repos cannot be audited by others.{RESET}"
        );
        println!();

        let confirmed: bool = cliclack::confirm("Continue with installation?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            println!("{DIM}Installation cancelled{RESET}");
            return Err(miette!("Installation cancelled by user"));
        }
    }

    Ok(())
}

async fn prompt_for_find_skills(manager: &SkillManager) {
    if skill::lock::is_prompt_dismissed("findSkillsPrompt")
        .await
        .unwrap_or(true)
    {
        return;
    }

    // Check if find-skills is already installed (auto-dismiss prompt if so).
    // Matches TS: checks universal agents for existing find-skills installation.
    let list_opts = skill::types::ListOptions {
        scope: Some(InstallScope::Global),
        agent_filter: manager.agents().universal_agents(),
        cwd: None,
    };
    let installed = manager.list_installed(&list_opts).await.unwrap_or_default();
    let already_installed = installed.iter().any(|s| s.name == "find-skills");
    if already_installed {
        let _ = skill::lock::dismiss_prompt("findSkillsPrompt").await;
        return;
    }

    println!();
    println!("{DIM}One-time prompt:{RESET}");
    let Ok(yes) =
        cliclack::confirm("Want to install find-skills? It helps agents discover new skills.")
            .initial_value(true)
            .interact()
    else {
        return;
    };

    if yes {
        println!("{TEXT}Installing find-skills...{RESET}");
        // Filter out replit agent (matches TS behavior).
        let agents: Vec<String> = manager
            .agents()
            .universal_agents()
            .iter()
            .filter(|id| id.as_str() != "replit")
            .map(|id| id.as_str().to_owned())
            .collect();
        let add_args = AddArgs {
            source: vec!["vercel-labs/skills@find-skills".to_owned()],
            global: Some(true),
            agent: Some(agents),
            skill: Some(vec!["find-skills".to_owned()]),
            list: false,
            yes: true,
            copy: false,
            all: false,
            full_depth: false,
        };
        let _ = Box::pin(run(add_args)).await;
    } else {
        let _ = skill::lock::dismiss_prompt("findSkillsPrompt").await;
    }
}

// ── Public API for internal callers (install_lock, sync) ────────────

/// Options for `run_add` when called programmatically.
pub struct RunAddOptions {
    pub source: String,
    pub global: Option<bool>,
    pub yes: bool,
    pub skill_filter: Option<Vec<String>>,
    pub agent: Option<Vec<String>>,
}

/// Programmatic entry point used by `install_lock` and `sync`.
pub async fn run_add(opts: RunAddOptions) -> Result<()> {
    let args = AddArgs {
        source: vec![opts.source],
        global: opts.global,
        agent: opts.agent,
        skill: opts.skill_filter,
        list: false,
        yes: opts.yes,
        copy: false,
        all: false,
        full_depth: false,
    };
    run(args).await
}

// ── Main entry point ────────────────────────────────────────────────

/// Run the add command.
pub async fn run(mut args: AddArgs) -> Result<()> {
    if args.source.is_empty() {
        return Err(missing_source_error());
    }

    if args.all {
        args.skill = Some(vec!["*".to_owned()]);
        args.agent = Some(vec!["*".to_owned()]);
        args.yes = true;
    }

    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Process each source (TS supports multiple sources).
    let sources = args.source.clone();
    for source in &sources {
        run_single_source(source, &mut args, &manager, &cwd).await?;
    }

    // Prompt for find-skills on first install (matches TS).
    if !args.yes {
        prompt_for_find_skills(&manager).await;
    }

    Ok(())
}

/// Process a single source string through the full add pipeline.
async fn run_single_source(
    source: &str,
    args: &mut AddArgs,
    manager: &SkillManager,
    cwd: &Path,
) -> Result<()> {
    let parsed = manager.parse_source(source);

    let source_display = if parsed.source_type == SourceType::Local {
        parsed
            .local_path
            .as_ref()
            .map_or(String::new(), |p| p.to_string_lossy().into_owned())
    } else {
        parsed.url.clone()
    };
    println!("{TEXT}Source: {source_display}{RESET}");

    // Merge @skill filter from source syntax.
    if let Some(filter) = &parsed.skill_filter {
        args.skill.get_or_insert_with(Vec::new).push(filter.clone());
    }

    // Security check for private GitHub repos (matches TS promptSecurityAdvisory).
    prompt_security_advisory(&parsed, args.yes).await?;

    // Well-known source: handled via provider API.
    if parsed.source_type == SourceType::WellKnown {
        return handle_wellknown_source(&parsed, args, manager, cwd).await;
    }

    // Git/local source: clone → discover → select → install.
    let (skills_dir, _temp_dir) = resolve_source(&parsed).await?;

    let include_internal = args.skill.as_ref().is_some_and(|s| !s.is_empty());
    let discover_opts = DiscoverOptions {
        include_internal,
        full_depth: args.full_depth,
    };
    let skills =
        skill::skills::discover_skills(&skills_dir, parsed.subpath.as_deref(), &discover_opts)
            .await
            .map_err(|e| miette!("{e}"))?;

    if skills.is_empty() {
        println!(
            "{DIM}No valid skills found. Skills require a SKILL.md with name and description.{RESET}"
        );
        return Ok(());
    }
    println!(
        "{TEXT}Found {} skill{}{RESET}",
        skills.len(),
        if skills.len() > 1 { "s" } else { "" }
    );

    // List mode: group by plugin, print and exit early (matches TS).
    if args.list {
        println!();
        println!("\x1b[1mAvailable Skills\x1b[0m");

        let mut grouped: BTreeMap<String, Vec<&Skill>> = BTreeMap::new();
        let mut ungrouped: Vec<&Skill> = Vec::new();
        for s in &skills {
            if let Some(ref plugin) = s.plugin_name {
                grouped.entry(plugin.clone()).or_default().push(s);
            } else {
                ungrouped.push(s);
            }
        }

        for (group, items) in &grouped {
            let title = kebab_to_title(group);
            println!("\x1b[1m{title}\x1b[0m");
            for s in items {
                println!("  \x1b[36m{}\x1b[0m", s.name);
                println!("    {DIM}{}{RESET}", s.description);
            }
            println!();
        }

        if !ungrouped.is_empty() {
            if !grouped.is_empty() {
                println!("\x1b[1mGeneral\x1b[0m");
            }
            for s in &ungrouped {
                println!("  \x1b[36m{}\x1b[0m", s.name);
                println!("    {DIM}{}{RESET}", s.description);
            }
        }

        println!();
        println!("{DIM}Use --skill <name> to install specific skills{RESET}");
        println!();
        return Ok(());
    }

    let selected_skills = select_skills(&skills, args.skill.as_ref(), args.yes)?;
    let target_agents = select_agents(manager, args.agent.as_ref(), args.yes).await?;

    let scope = resolve_scope(args.global, args.yes, &target_agents, manager)?;
    let mode = resolve_mode(args.copy, args.yes)?;

    print_installation_summary(&selected_skills, &target_agents, manager, scope, mode, cwd);

    if !args.yes {
        let confirmed: bool = cliclack::confirm("Proceed with installation?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;
        if !confirmed {
            println!("{DIM}Installation cancelled{RESET}");
            return Ok(());
        }
    }

    let install_opts = InstallOptions {
        scope,
        mode,
        cwd: Some(cwd.to_path_buf()),
    };

    let outcomes = do_install(manager, &selected_skills, &target_agents, &install_opts).await;
    print_install_results(&outcomes, cwd, &target_agents, manager);

    // Lock file integration.
    if scope == InstallScope::Global {
        update_lock_file(&parsed, &selected_skills).await;
    } else {
        update_local_lock_file(&parsed, &selected_skills, cwd).await;
    }

    send_telemetry(&parsed, &selected_skills, &target_agents, scope);

    println!();
    println!(
        "{GREEN}{BOLD}Done!{RESET} {DIM}Review skills before use; they run with full agent permissions.{RESET}"
    );
    println!();

    Ok(())
}

/// Handle a well-known source (e.g. `https://mintlify.com/docs`).
async fn handle_wellknown_source(
    parsed: &skill::types::ParsedSource,
    args: &AddArgs,
    manager: &SkillManager,
    cwd: &Path,
) -> Result<()> {
    use skill::providers::WellKnownProvider;

    println!("{TEXT}Fetching skills from well-known endpoint...{RESET}");

    let provider = WellKnownProvider;
    let wk_skills = provider
        .fetch_all_skills(&parsed.url)
        .await
        .map_err(|e| miette!("{e}"))?;

    if wk_skills.is_empty() {
        println!("{DIM}No skills found at this endpoint.{RESET}");
        return Ok(());
    }

    println!(
        "{TEXT}Found {} skill{}{RESET}",
        wk_skills.len(),
        if wk_skills.len() > 1 { "s" } else { "" }
    );

    if args.list {
        println!();
        for wk in &wk_skills {
            println!(
                "  {TEXT}{}{RESET} {DIM}- {}{RESET}",
                wk.remote.name, wk.remote.description
            );
        }
        println!();
        return Ok(());
    }

    let target_agents = select_agents(manager, args.agent.as_ref(), args.yes).await?;

    let scope = resolve_scope(args.global, args.yes, &target_agents, manager)?;
    let mode = resolve_mode(args.copy, args.yes)?;

    let install_opts = InstallOptions {
        scope,
        mode,
        cwd: Some(cwd.to_path_buf()),
    };

    let outcomes =
        install_wellknown_skills(&wk_skills, &target_agents, manager, &install_opts).await;
    print_install_results(&outcomes, cwd, &target_agents, manager);

    // Lock file: well-known skills use source_identifier as source.
    for wk in &wk_skills {
        let _ = skill::lock::add_skill_to_lock(
            &wk.remote.install_name,
            &wk.remote.source_identifier,
            "well-known",
            &wk.remote.source_url,
            None,
            "",
            None,
        )
        .await;
    }

    send_wellknown_telemetry(&wk_skills, &target_agents, scope);

    println!();
    println!(
        "{GREEN}{BOLD}Done!{RESET} {DIM}Review skills before use; they run with full agent permissions.{RESET}"
    );
    println!();

    Ok(())
}

/// Update the project-scoped `skills-lock.json` after a successful install.
///
/// Matches TS `addSkillToLocalLock()`: computes a local SHA-256 hash of the
/// skill folder contents and records the source information.
async fn update_local_lock_file(parsed: &skill::types::ParsedSource, skills: &[Skill], cwd: &Path) {
    let source = skill::source::get_owner_repo(parsed).unwrap_or_else(|| parsed.url.clone());

    for s in skills {
        let hash = skill::local_lock::compute_skill_folder_hash(&s.path)
            .await
            .unwrap_or_default();

        let _ = skill::local_lock::add_skill_to_local_lock(
            &s.name,
            skill::local_lock::LocalSkillLockEntry {
                source: source.clone(),
                source_type: parsed.source_type.to_string(),
                computed_hash: hash,
            },
            cwd,
        )
        .await;
    }
}

/// Update the global lock file after a successful git-based install.
async fn update_lock_file(parsed: &skill::types::ParsedSource, skills: &[Skill]) {
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

        let _ = skill::lock::add_skill_to_lock(
            &s.name,
            &owner_repo,
            &parsed.source_type.to_string(),
            &parsed.url,
            skill_path.as_deref(),
            &hash,
            s.plugin_name.as_deref(),
        )
        .await;
    }
}

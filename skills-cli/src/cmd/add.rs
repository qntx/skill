//! `skills add <source>` command implementation.
//!
//! Matches the `TypeScript` `add.ts` UX: clack-style prompts, interactive
//! agent selection with search-multiselect, scope/method prompts, and
//! lock-file integration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result, miette};

use skill::SkillManager;
use skill::types::{
    AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallScope, Skill, SourceType,
};

use crate::ui;

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const RESET: &str = "\x1b[0m";

/// Arguments for the `add` command.
#[derive(Args)]
pub struct AddArgs {
    /// Source to install from (e.g. `owner/repo`, URL, local path).
    pub source: Option<String>,

    /// Install globally (user-level) instead of project-level.
    #[arg(short, long)]
    pub global: bool,

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

fn show_missing_source_error() -> ! {
    eprintln!();
    eprintln!(
        "{} {}",
        style(" ERROR ").white().on_red().bold(),
        style("Missing required argument: source").red()
    );
    eprintln!();
    eprintln!("  {}", style("Usage:").dim());
    eprintln!(
        "    {} {} {}",
        style("skills add").cyan(),
        style("<source>").yellow(),
        style("[options]").dim()
    );
    eprintln!();
    eprintln!("  {}", style("Example:").dim());
    eprintln!(
        "    {} {}",
        style("skills add").cyan(),
        style("qntx/skills").yellow()
    );
    eprintln!();
    std::process::exit(1);
}

/// Select skills to install from the discovered set using cliclack multiselect.
fn select_skills(
    skills: &[Skill],
    skill_filter: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<Skill>> {
    if skill_filter.is_some_and(|s| s.contains(&"*".to_owned())) {
        return Ok(skills.to_vec());
    }

    if let Some(names) = skill_filter {
        let filtered = skill::skills::filter_skills(skills, names);
        if filtered.is_empty() {
            return Err(miette!(
                "No matching skills found for: {}",
                names.join(", ")
            ));
        }
        return Ok(filtered);
    }

    if skills.len() == 1 || yes {
        return Ok(skills.to_vec());
    }

    // Interactive multiselect via cliclack
    let mut prompt = cliclack::multiselect("Select skills to install");
    for s in skills {
        prompt = prompt.item(s.name.clone(), &s.name, &s.description);
    }
    prompt = prompt.required(true);

    let selected_names: Vec<String> = prompt.interact().into_diagnostic()?;
    if selected_names.is_empty() {
        return Err(miette!("No skills selected"));
    }

    Ok(skills
        .iter()
        .filter(|s| selected_names.contains(&s.name))
        .cloned()
        .collect())
}

/// Select target agents using the custom search-multiselect component.
///
/// Matches the TS version: universal agents in a locked section,
/// detected agents pre-selected, search filtering, last-selection memory.
async fn select_agents(
    manager: &SkillManager,
    agent_arg: Option<&Vec<String>>,
    yes: bool,
) -> Result<Vec<AgentId>> {
    let all_ids = manager.agents().all_ids();

    if agent_arg.is_some_and(|a| a.contains(&"*".to_owned())) {
        return Ok(all_ids);
    }

    if let Some(names) = agent_arg {
        return Ok(names.iter().map(AgentId::new).collect());
    }

    let detected = manager.detect_installed_agents().await;

    if yes {
        return Ok(if detected.is_empty() {
            all_ids
        } else {
            ensure_universal_agents(manager, detected)
        });
    }

    // Build search-multiselect matching TS search-multiselect.ts
    let universal = manager.agents().universal_agents();
    let non_universal = manager.agents().non_universal_agents();

    // Locked section: universal agents
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

    // Selectable items: non-universal agents
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

    // Pre-select detected agents
    let initial: Vec<String> = detected
        .iter()
        .filter(|id| !universal.contains(id))
        .map(|id| id.as_str().to_owned())
        .collect();

    // Check for last selected agents
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
            // Save selection for next time
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

/// Perform the actual installation and print results.
async fn do_install(
    manager: &SkillManager,
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    install_opts: &InstallOptions,
) -> (u32, u32) {
    println!("{TEXT}Installing skills...{RESET}");
    let mut successes = 0u32;
    let mut failures = 0u32;

    for skill_item in selected_skills {
        for agent_id in target_agents {
            match manager
                .install_skill(skill_item, agent_id, install_opts)
                .await
            {
                Ok(result) if result.success => successes += 1,
                Ok(result) => {
                    tracing::warn!(
                        skill = %skill_item.name,
                        agent = %agent_id,
                        error = result.error.as_deref().unwrap_or("unknown"),
                        "install failed"
                    );
                    failures += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %skill_item.name,
                        agent = %agent_id,
                        error = %e,
                        "install failed"
                    );
                    failures += 1;
                }
            }
        }
    }

    (successes, failures)
}

/// Run the add command.
pub async fn run(mut args: AddArgs) -> Result<()> {
    let Some(source) = args.source.as_ref() else {
        show_missing_source_error();
    };

    if args.all {
        args.skill = Some(vec!["*".to_owned()]);
        args.agent = Some(vec!["*".to_owned()]);
        args.yes = true;
    }

    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Parse source
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

    // Merge @skill filter
    if let Some(filter) = &parsed.skill_filter {
        args.skill.get_or_insert_with(Vec::new).push(filter.clone());
    }

    // Resolve source to local directory
    let (skills_dir, _temp_dir): (PathBuf, Option<tempfile::TempDir>) =
        resolve_source(&parsed).await?;

    // Discover skills
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

    // List mode
    if args.list {
        println!();
        println!("{TEXT}Available Skills:{RESET}");
        for s in &skills {
            println!("  {TEXT}{}{RESET} {DIM}- {}{RESET}", s.name, s.description);
        }
        println!();
        println!("{DIM}Use --skill <name> to install specific skills{RESET}");
        println!();
        return Ok(());
    }

    let selected_skills = select_skills(&skills, args.skill.as_ref(), args.yes)?;

    // Interactive scope selection (if not specified via flags)
    let scope = if args.global {
        InstallScope::Global
    } else if args.yes {
        InstallScope::Project
    } else {
        let scope_choice: String = cliclack::select("Where should the skills be installed?")
            .item(
                String::from("project"),
                "Project",
                "Install in the current directory",
            )
            .item(
                String::from("global"),
                "Global",
                "Install for all projects (user-level)",
            )
            .interact()
            .into_diagnostic()?;
        if scope_choice == "global" {
            InstallScope::Global
        } else {
            InstallScope::Project
        }
    };

    // Interactive install method (if not specified via flags)
    let mode = if args.copy {
        InstallMode::Copy
    } else if args.yes {
        InstallMode::Symlink
    } else {
        let mode_choice: String = cliclack::select("Installation method")
            .item(
                String::from("symlink"),
                "Symlink (recommended)",
                "Single copy, symlinked to each agent",
            )
            .item(
                String::from("copy"),
                "Copy",
                "Independent copy for each agent",
            )
            .interact()
            .into_diagnostic()?;
        if mode_choice == "copy" {
            InstallMode::Copy
        } else {
            InstallMode::Symlink
        }
    };

    let target_agents = select_agents(&manager, args.agent.as_ref(), args.yes).await?;

    // Summary
    show_install_summary(&selected_skills, &target_agents, &manager, scope, &cwd);

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
        cwd: Some(cwd.clone()),
    };
    let (successes, failures) =
        do_install(&manager, &selected_skills, &target_agents, &install_opts).await;

    // Lock file integration (matching TS add.ts)
    if let Some(owner_repo) = skill::source::get_owner_repo(&parsed) {
        for s in &selected_skills {
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

    println!();
    if successes > 0 {
        println!(
            "{TEXT}✓ Installed {} skill{}{RESET}",
            selected_skills.len(),
            if selected_skills.len() == 1 { "" } else { "s" }
        );
    }
    if failures > 0 {
        println!("{DIM}✗ Failed to install {failures} target(s){RESET}");
    }

    send_telemetry(&parsed, &selected_skills, &target_agents, scope);

    println!("{DIM}Review skills before use; they run with full agent permissions.{RESET}");
    println!();

    Ok(())
}

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

fn show_install_summary(
    selected_skills: &[Skill],
    target_agents: &[AgentId],
    manager: &SkillManager,
    scope: InstallScope,
    cwd: &Path,
) {
    let agent_names: Vec<String> = target_agents
        .iter()
        .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
        .collect();

    println!();
    println!("{TEXT}Installation Summary:{RESET}");
    for s in selected_skills {
        let canonical = skill::installer::get_canonical_path(&s.name, scope, cwd);
        println!("  {}", ui::shorten_path(&canonical));
    }
    println!();
    println!("  {DIM}agents:{RESET} {}", ui::format_list(&agent_names));
    println!(
        "  {DIM}scope:{RESET}  {}",
        if scope == InstallScope::Global {
            "global"
        } else {
            "project"
        }
    );
    println!();
}

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

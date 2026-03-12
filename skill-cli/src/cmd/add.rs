//! `skills add <source>` command implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::Args;
use console::style;
use dialoguer::{Confirm, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use miette::{IntoDiagnostic, Result, miette};

use skill::SkillManager;
use skill::types::{
    AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallScope, Skill, SourceType,
};

use crate::ui;

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

fn make_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("valid template"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message(msg.to_owned());
    pb
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
        style("vercel-labs/agent-skills").yellow()
    );
    eprintln!();
    std::process::exit(1);
}

/// Select skills to install from the discovered set.
fn select_skills(
    skills: &[Skill],
    skill_filter: &Option<Vec<String>>,
    yes: bool,
) -> Result<Vec<Skill>> {
    if skill_filter
        .as_ref()
        .is_some_and(|s| s.contains(&"*".to_owned()))
    {
        println!(
            "  {} Installing all {} skills",
            style("ℹ").blue(),
            skills.len()
        );
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

    if skills.len() == 1 {
        println!(
            "  {} Skill: {}",
            style("ℹ").blue(),
            style(&skills[0].name).cyan()
        );
        return Ok(skills.to_vec());
    }

    if yes {
        return Ok(skills.to_vec());
    }

    let items: Vec<String> = skills
        .iter()
        .map(|s| format!("{} - {}", s.name, s.description))
        .collect();
    let selections = MultiSelect::new()
        .with_prompt("Select skills to install (space to toggle)")
        .items(&items)
        .interact()
        .into_diagnostic()?;

    if selections.is_empty() {
        return Err(miette!("No skills selected"));
    }

    Ok(selections.iter().map(|&i| skills[i].clone()).collect())
}

/// Select target agents for installation.
async fn select_agents(
    manager: &SkillManager,
    agent_arg: &Option<Vec<String>>,
    yes: bool,
) -> Result<Vec<AgentId>> {
    let all_ids = manager.agents().all_ids();

    if agent_arg
        .as_ref()
        .is_some_and(|a| a.contains(&"*".to_owned()))
    {
        return Ok(all_ids);
    }

    if let Some(names) = agent_arg {
        return Ok(names.iter().map(AgentId::new).collect());
    }

    if yes {
        let detected = manager.detect_installed_agents().await;
        return Ok(if detected.is_empty() {
            all_ids
        } else {
            ensure_universal_agents(manager, detected)
        });
    }

    let detected = manager.detect_installed_agents().await;
    if detected.is_empty() || detected.len() > 1 {
        let items: Vec<String> = all_ids
            .iter()
            .map(|id| {
                manager
                    .agents()
                    .get(id)
                    .map_or_else(|| id.as_str().to_owned(), |c| c.display_name.clone())
            })
            .collect();

        let selections = MultiSelect::new()
            .with_prompt("Which agents do you want to install to?")
            .items(&items)
            .interact()
            .into_diagnostic()?;

        if selections.is_empty() {
            return Err(miette!("No agents selected"));
        }

        return Ok(selections.iter().map(|&i| all_ids[i].clone()).collect());
    }

    Ok(ensure_universal_agents(manager, detected))
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
    let spinner = make_spinner("Installing skills...");
    let mut successes = 0u32;
    let mut failures = 0u32;

    for skill_item in selected_skills {
        for agent_id in target_agents {
            match manager
                .install_skill(skill_item, agent_id, install_opts)
                .await
            {
                Ok(result) if result.success => successes += 1,
                Ok(_) | Err(_) => failures += 1,
            }
        }
    }

    spinner.finish_with_message("Installation complete");
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

    ui::show_logo();

    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Parse source
    let spinner = make_spinner("Parsing source...");
    let parsed = manager.parse_source(source);
    spinner.finish_with_message(format!(
        "Source: {}{}{}{}",
        if parsed.source_type == SourceType::Local {
            parsed
                .local_path
                .as_ref()
                .map_or(String::new(), |p| p.to_string_lossy().into_owned())
        } else {
            parsed.url.clone()
        },
        parsed
            .git_ref
            .as_ref()
            .map_or(String::new(), |r| format!(" @ {}", style(r).yellow())),
        parsed
            .subpath
            .as_ref()
            .map_or(String::new(), |s| format!(" ({s})")),
        parsed
            .skill_filter
            .as_ref()
            .map_or(String::new(), |f| format!(" @{}", style(f).cyan())),
    ));

    // Merge @skill filter
    if let Some(filter) = &parsed.skill_filter {
        args.skill.get_or_insert_with(Vec::new).push(filter.clone());
    }

    // Resolve source to local directory
    let (skills_dir, _temp_dir): (PathBuf, Option<tempfile::TempDir>) =
        resolve_source(&parsed, &cwd).await?;

    // Discover skills
    let spinner = make_spinner("Discovering skills...");
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
        spinner.finish_with_message(format!("{}", style("No skills found").red()));
        return Err(miette!(
            "No valid skills found. Skills require a SKILL.md with name and description."
        ));
    }
    spinner.finish_with_message(format!(
        "Found {} skill{}",
        style(skills.len()).green(),
        if skills.len() > 1 { "s" } else { "" }
    ));

    // List mode
    if args.list {
        println!();
        println!("  {}", style("Available Skills").bold());
        for s in &skills {
            println!("    {}", style(&s.name).cyan());
            println!("      {}", style(&s.description).dim());
        }
        println!();
        println!("Use --skill <name> to install specific skills");
        return Ok(());
    }

    let selected_skills = select_skills(&skills, &args.skill, args.yes)?;
    let target_agents = select_agents(&manager, &args.agent, args.yes).await?;

    let scope = if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    };
    let mode = if args.copy {
        InstallMode::Copy
    } else {
        InstallMode::Symlink
    };

    // Summary
    show_install_summary(&selected_skills, &target_agents, &manager, scope, &cwd);

    if !args.yes {
        let confirmed = Confirm::new()
            .with_prompt("Proceed with installation?")
            .default(true)
            .interact()
            .into_diagnostic()?;
        if !confirmed {
            println!("  {}", style("Installation cancelled").dim());
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

    if successes > 0 {
        println!();
        ui::print_success(&format!(
            "Installed {} skill{}",
            selected_skills.len(),
            if selected_skills.len() == 1 { "" } else { "s" }
        ));
    }
    if failures > 0 {
        println!();
        ui::print_error(&format!("Failed to install {failures} target(s)"));
    }

    send_telemetry(&parsed, &selected_skills, &target_agents, scope);

    println!();
    println!(
        "{}{}",
        style("Done!").green(),
        style("  Review skills before use; they run with full agent permissions.").dim()
    );
    println!();

    Ok(())
}

async fn resolve_source(
    parsed: &skill::types::ParsedSource,
    _cwd: &Path,
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

    let spinner = make_spinner("Cloning repository...");
    let td = skill::git::clone_repo(&parsed.url, parsed.git_ref.as_deref())
        .await
        .map_err(|e| miette!("{e}"))?;
    let path = td.path().to_path_buf();
    spinner.finish_with_message("Repository cloned");
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

    let mut summary = String::new();
    for s in selected_skills {
        let canonical = skill::installer::get_canonical_path(&s.name, scope, cwd);
        let short = ui::shorten_path(&canonical, cwd);
        summary.push_str(&format!("{}\n", style(short).cyan()));
        summary.push_str(&format!(
            "  {} {}\n",
            style("agents:").dim(),
            ui::format_list(&agent_names, 5)
        ));
    }
    ui::print_note(&summary, "Installation Summary");
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

//! `skills add <source>` command implementation.
//!
//! Matches the `TypeScript` `add.ts` UX: cliclack prompts for skill and
//! agent selection, plain ANSI output for results.  Scope and install mode
//! come exclusively from CLI flags (no interactive prompts), matching TS.

use std::collections::HashMap;
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

fn missing_source_error() -> miette::Report {
    miette!(
        help = "Usage: skills add <source> [options]\nExample: skills add qntx/skills",
        "Missing required argument: source"
    )
}

// ── Skill selection ─────────────────────────────────────────────────

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

fn print_install_results(outcomes: &[SkillInstallOutcome], cwd: &Path) {
    let total_ok: usize = outcomes
        .iter()
        .map(|o| o.universal_agents.len() + o.symlinked_agents.len() + o.copied_agents.len())
        .sum();
    let total_fail: usize = outcomes.iter().map(|o| o.failed_agents.len()).sum();

    println!();
    for outcome in outcomes {
        if let Some(ref canonical) = outcome.canonical_path {
            let short = ui::shorten_path_with_cwd(canonical, cwd);
            println!("{TEXT}📁 {short}{RESET}");
        }

        if !outcome.universal_agents.is_empty() {
            println!(
                "   {DIM}Universal — {}{RESET}",
                ui::format_list(&outcome.universal_agents)
            );
        }
        if !outcome.symlinked_agents.is_empty() {
            println!(
                "   {DIM}Symlinked — {}{RESET}",
                ui::format_list(&outcome.symlinked_agents)
            );
        }
        if !outcome.copied_agents.is_empty() {
            println!(
                "   {DIM}Copied    — {}{RESET}",
                ui::format_list(&outcome.copied_agents)
            );
        }
        if !outcome.failed_agents.is_empty() {
            println!(
                "   {DIM}Failed    — {}{RESET}",
                ui::format_list(&outcome.failed_agents)
            );
        }
    }

    println!();
    if total_ok > 0 {
        println!(
            "{TEXT}✓ Installed {} skill{}{RESET}",
            outcomes.len(),
            if outcomes.len() == 1 { "" } else { "s" }
        );
    }
    if total_fail > 0 {
        println!("{DIM}✗ Failed {total_fail} target(s){RESET}");
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

async fn prompt_for_find_skills() {
    if skill::lock::is_prompt_dismissed("findSkillsPrompt")
        .await
        .unwrap_or(true)
    {
        return;
    }

    println!();
    let Ok(yes) =
        cliclack::confirm("Want to install find-skills? It helps agents discover new skills.")
            .initial_value(true)
            .interact()
    else {
        return;
    };

    if yes {
        println!("{TEXT}Installing find-skills...{RESET}");
        let add_args = AddArgs {
            source: vec!["vercel-labs/skills@find-skills".to_owned()],
            global: true,
            agent: None,
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
    pub global: bool,
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

    // Scope and mode come from flags only — no interactive prompts (matches TS).
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

    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    // Process each source (TS supports multiple sources).
    let sources = args.source.clone();
    for source in &sources {
        run_single_source(source, &mut args, &manager, scope, mode, &cwd).await?;
    }

    // Prompt for find-skills on first install (matches TS).
    if !args.yes {
        prompt_for_find_skills().await;
    }

    Ok(())
}

/// Process a single source string through the full add pipeline.
async fn run_single_source(
    source: &str,
    args: &mut AddArgs,
    manager: &SkillManager,
    scope: InstallScope,
    mode: InstallMode,
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
        return handle_wellknown_source(&parsed, args, manager, scope, mode, cwd).await;
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

    // List mode: print and exit early.
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
    let target_agents = select_agents(manager, args.agent.as_ref(), args.yes).await?;

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
    print_install_results(&outcomes, cwd);

    // Lock file integration.
    update_lock_file(&parsed, &selected_skills).await;

    send_telemetry(&parsed, &selected_skills, &target_agents, scope);

    println!("{DIM}Review skills before use; they run with full agent permissions.{RESET}");
    println!();

    Ok(())
}

/// Handle a well-known source (e.g. `https://mintlify.com/docs`).
async fn handle_wellknown_source(
    parsed: &skill::types::ParsedSource,
    args: &AddArgs,
    manager: &SkillManager,
    scope: InstallScope,
    mode: InstallMode,
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

    let install_opts = InstallOptions {
        scope,
        mode,
        cwd: Some(cwd.to_path_buf()),
    };

    let outcomes =
        install_wellknown_skills(&wk_skills, &target_agents, manager, &install_opts).await;
    print_install_results(&outcomes, cwd);

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

    println!("{DIM}Review skills before use; they run with full agent permissions.{RESET}");
    println!();

    Ok(())
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

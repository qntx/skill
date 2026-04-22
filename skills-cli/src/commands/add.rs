//! `skills add <source>` command implementation.

mod hooks;
mod install;
mod output;
mod select;

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use clap::Args;
use miette::{IntoDiagnostic, Result, miette};
pub(crate) use select::select_agents;
use skill::SkillManager;
use skill::types::{AgentId, DiscoverOptions, InstallOptions, InstallScope, Skill, SourceType};

use crate::ui::emit;
use crate::ui::{
    self, BOLD, CYAN, DIM, GREEN, INTRO_TAG, RED, RESET, TEXT, YELLOW, kebab_to_title,
};

/// Arguments for the `add` command.
#[derive(Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI flags are naturally boolean"
)]
pub(crate) struct AddArgs {
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

    /// Preview what would be installed without making changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Opt in to installing from the `openclaw/*` organization.
    ///
    /// `OpenClaw` is a user-submitted skill registry without vetting; installs
    /// are blocked by default. Passing this flag acknowledges that the caller
    /// has reviewed the source themselves.
    #[arg(long = "dangerously-accept-openclaw-risks")]
    pub dangerously_accept_openclaw_risks: bool,
}

/// Options for `run_add` when called programmatically.
pub(crate) struct RunAddOptions {
    /// Source to install from.
    pub source: String,
    /// Install globally.
    pub global: Option<bool>,
    /// Skip confirmation.
    pub yes: bool,
    /// Filter to specific skills.
    pub skill_filter: Option<Vec<String>>,
    /// Target agents.
    pub agent: Option<Vec<String>>,
    /// Dry run mode.
    pub dry_run: bool,
}

/// Programmatic entry point used by `install_lock` and `update`.
pub(crate) async fn run_add(opts: RunAddOptions) -> Result<()> {
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
        dry_run: opts.dry_run,
        dangerously_accept_openclaw_risks: false,
    };
    run(args).await
}

/// Whether the parsed source resolves to the `openclaw/*` org.
fn is_openclaw_source(parsed: &skill::types::ParsedSource) -> bool {
    skill::source::get_owner_repo(parsed)
        .and_then(|s| s.split('/').next().map(str::to_ascii_lowercase))
        .is_some_and(|owner| owner == "openclaw")
}

/// Run the add command.
pub(crate) async fn run(mut args: AddArgs) -> Result<()> {
    if args.source.is_empty() {
        return Err(miette!(
            help = "Usage: skills add <source> [options]\nExample: skills add qntx/skills",
            "Missing required argument: source"
        ));
    }

    if args.all {
        args.skill = Some(vec!["*".to_owned()]);
        args.agent = Some(vec!["*".to_owned()]);
        args.yes = true;
    }

    println!();
    emit::intro(format!("{INTRO_TAG} skills {RESET}"));

    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    let sources = args.source.clone();
    let mut last_agents: Vec<AgentId> = Vec::new();
    for source in &sources {
        if let Some(agents) = run_single_source(source, &mut args, &manager, &cwd).await? {
            last_agents = agents;
        }
    }

    if !args.yes {
        hooks::prompt_for_find_skills(&manager, &last_agents).await;
    }

    Ok(())
}

#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "sequential install pipeline with multiple stages"
)]
async fn run_single_source(
    source: &str,
    args: &mut AddArgs,
    manager: &SkillManager,
    cwd: &Path,
) -> Result<Option<Vec<AgentId>>> {
    let spinner = cliclack::spinner();

    // Parse and display source
    spinner.start("Parsing source...");
    let parsed = manager.parse_source(source);
    let source_display = if parsed.source_type == SourceType::Local {
        parsed
            .local_path
            .as_ref()
            .map_or(String::new(), |p| p.to_string_lossy().into_owned())
    } else {
        parsed.url.clone()
    };
    let mut source_suffix = String::new();
    if let Some(ref r) = parsed.git_ref {
        let _ = write!(source_suffix, " @ {YELLOW}{r}{RESET}");
    }
    if let Some(ref s) = parsed.subpath {
        let _ = write!(source_suffix, " ({s})");
    }
    if let Some(ref f) = parsed.skill_filter {
        let _ = write!(source_suffix, " {DIM}@{RESET}{CYAN}{f}{RESET}");
    }
    spinner.stop(format!("Source: {source_display}{source_suffix}"));

    // Block openclaw/* sources unless the caller explicitly opts in. Mirrors
    // the TS `add.ts` guard at `3rdparty/skills/src/add.ts:946`.
    if is_openclaw_source(&parsed) && !args.dangerously_accept_openclaw_risks {
        emit::warning("OpenClaw skills are unverified community submissions.");
        emit::remark(
            "This source contains user-submitted skills that have not been reviewed for safety or quality.",
        );
        emit::remark("Skills run with full agent permissions and could be malicious.");
        emit::remark(format!(
            "If you understand the risks, re-run with:\n  skills add {source} --dangerously-accept-openclaw-risks"
        ));
        emit::outro_cancel(format!("{RED}Installation blocked{RESET}"));
        return Ok(None);
    }

    if let Some(filter) = &parsed.skill_filter {
        args.skill.get_or_insert_with(Vec::new).push(filter.clone());
    }

    let is_private = hooks::prompt_security_advisory(&parsed, args.yes).await?;

    if parsed.source_type == SourceType::WellKnown {
        return handle_wellknown_source(&parsed, args, manager, cwd).await;
    }

    // Clone/resolve source
    let clone_spinner = cliclack::spinner();
    if parsed.source_type == SourceType::Local {
        clone_spinner.start("Validating local path...");
    } else {
        clone_spinner.start("Cloning repository...");
    }
    let (skills_dir, _temp_dir) = install::resolve_source(&parsed).await?;
    if parsed.source_type == SourceType::Local {
        clone_spinner.stop("Local path validated");
    } else {
        clone_spinner.stop("Repository cloned");
    }

    // Discover skills
    let discover_spinner = cliclack::spinner();
    discover_spinner.start("Discovering skills...");
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
        discover_spinner.stop(format!("{RED}No skills found{RESET}"));
        emit::outro(format!(
            "{RED}No valid skills found. Skills require a SKILL.md with name and description.{RESET}",
        ));
        return Ok(None);
    }
    discover_spinner.stop(format!(
        "Found {GREEN}{}{RESET} skill{}",
        skills.len(),
        if skills.len() > 1 { "s" } else { "" }
    ));

    if args.list {
        print_skill_list(&skills);
        return Ok(None);
    }

    // Start audit fetch in parallel before user selection (matching TS pattern)
    let owner_repo_for_audit = skill::source::get_owner_repo(&parsed);
    let skill_slugs: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
    let audit_handle = if is_private.unwrap_or(false) {
        None
    } else {
        let source_id = owner_repo_for_audit.clone().unwrap_or_default();
        Some(tokio::spawn(async move {
            skill::telemetry::fetch_audit_data(&source_id, &skill_slugs).await
        }))
    };

    let selected_skills = select::select_skills(&skills, args.skill.as_ref(), args.yes)?;
    let target_agents = select_agents(manager, args.agent.as_ref(), args.yes).await?;

    let scope = select::resolve_scope(args.global, args.yes, &target_agents, manager)?;
    let mode = select::resolve_mode(args.copy, args.yes)?;

    output::print_installation_summary(&selected_skills, &target_agents, manager, scope, mode, cwd)
        .await;

    // Await and display security audit results (started earlier in parallel)
    if let Some(handle) = audit_handle
        && let Ok(Some(audit_data)) = handle.await
        && let Some(ref audit_source) = owner_repo_for_audit
    {
        output::print_security_audit(&audit_data, &selected_skills, audit_source);
    }

    if args.dry_run {
        println!();
        emit::outro(format!(
            "{DIM}Dry run complete — no changes were made.{RESET}"
        ));
        return Ok(Some(target_agents));
    }

    if !args.yes {
        ui::drain_input_events();
        let confirmed: bool = cliclack::confirm("Proceed with installation?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;
        if !confirmed {
            emit::outro_cancel("Installation cancelled");
            return Ok(None);
        }
    }

    let install_opts = InstallOptions {
        scope,
        mode,
        cwd: Some(cwd.to_path_buf()),
    };

    let install_spinner = cliclack::spinner();
    install_spinner.start("Installing skills...");
    let outcomes =
        install::do_install(manager, &selected_skills, &target_agents, &install_opts).await;
    install_spinner.stop("Installation complete");

    println!();
    output::print_install_results(&outcomes, cwd);

    if scope == InstallScope::Global {
        hooks::update_lock_file(&parsed, &selected_skills).await;
    } else {
        hooks::update_local_lock_file(&parsed, &selected_skills, cwd).await;
    }

    hooks::send_telemetry(&parsed, &selected_skills, &target_agents, scope, is_private);

    println!();
    emit::outro(format!(
        "{GREEN}Done!{RESET}  {DIM}Review skills before use; they run with full agent permissions.{RESET}"
    ));

    Ok(Some(target_agents))
}

async fn handle_wellknown_source(
    parsed: &skill::types::ParsedSource,
    args: &AddArgs,
    manager: &SkillManager,
    cwd: &Path,
) -> Result<Option<Vec<AgentId>>> {
    use skill::providers::WellKnownProvider;

    let discover_spinner = cliclack::spinner();
    discover_spinner.start("Discovering skills from well-known endpoint...");

    let provider = WellKnownProvider;
    let wk_skills = provider
        .fetch_all_skills(&parsed.url)
        .await
        .map_err(|e| miette!("{e}"))?;

    if wk_skills.is_empty() {
        discover_spinner.stop(format!("{RED}No skills found{RESET}"));
        emit::outro(format!(
            "{RED}No skills found at this URL. Make sure the server has a /.well-known/skills/index.json file.{RESET}",
        ));
        return Ok(None);
    }

    discover_spinner.stop(format!(
        "Found {GREEN}{}{RESET} skill{}",
        wk_skills.len(),
        if wk_skills.len() > 1 { "s" } else { "" }
    ));

    if args.list {
        println!();
        for wk in &wk_skills {
            println!(
                "  {TEXT}{}{RESET} {DIM}- {}{RESET}",
                wk.remote.name, wk.remote.description
            );
        }
        println!();
        return Ok(None);
    }

    let target_agents = select_agents(manager, args.agent.as_ref(), args.yes).await?;
    let scope = select::resolve_scope(args.global, args.yes, &target_agents, manager)?;
    let mode = select::resolve_mode(args.copy, args.yes)?;

    let install_opts = InstallOptions {
        scope,
        mode,
        cwd: Some(cwd.to_path_buf()),
    };

    let install_spinner = cliclack::spinner();
    install_spinner.start("Installing skills...");
    let outcomes =
        install::install_wellknown_skills(&wk_skills, &target_agents, manager, &install_opts).await;
    install_spinner.stop("Installation complete");

    println!();
    output::print_install_results(&outcomes, cwd);

    for wk in &wk_skills {
        let _ = skill::lock::add_skill_to_lock(&skill::lock::AddLockInput {
            name: &wk.remote.install_name,
            source: &wk.remote.source_identifier,
            source_type: "well-known",
            source_url: &wk.remote.source_url,
            git_ref: None,
            skill_path: None,
            skill_folder_hash: "",
            plugin_name: None,
        })
        .await;
    }

    hooks::send_wellknown_telemetry(&wk_skills, &target_agents, scope);

    println!();
    emit::outro(format!(
        "{GREEN}Done!{RESET}  {DIM}Review skills before use; they run with full agent permissions.{RESET}"
    ));

    Ok(Some(target_agents))
}

fn print_skill_list(skills: &[Skill]) {
    println!();
    emit::step(format!("{BOLD}Available Skills{RESET}"));

    let mut grouped: BTreeMap<String, Vec<&Skill>> = BTreeMap::new();
    let mut ungrouped: Vec<&Skill> = Vec::new();
    for s in skills {
        if let Some(ref plugin) = s.plugin_name {
            grouped.entry(plugin.clone()).or_default().push(s);
        } else {
            ungrouped.push(s);
        }
    }

    for (group, items) in &grouped {
        let title = kebab_to_title(group);
        println!("{BOLD}{title}{RESET}");
        for s in items {
            emit::remark(format!("  {CYAN}{}{RESET}", s.name));
            emit::remark(format!("    {DIM}{}{RESET}", s.description));
        }
        println!();
    }

    if !ungrouped.is_empty() {
        if !grouped.is_empty() {
            println!("{BOLD}General{RESET}");
        }
        for s in &ungrouped {
            emit::remark(format!("  {CYAN}{}{RESET}", s.name));
            emit::remark(format!("    {DIM}{}{RESET}", s.description));
        }
    }

    println!();
    emit::outro("Use --skill <name> to install specific skills");
}

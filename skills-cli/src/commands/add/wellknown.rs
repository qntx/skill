//! Well-known skill install flow for the `add` command.
//!
//! Separated from the main `add.rs` so the top-level entry point stays
//! focused on git-backed sources.  Each function mirrors a stage of the TS
//! reference's `handleWellKnownSource`.

use std::path::Path;

use miette::{Result, miette};
use skill::SkillManager;
use skill::providers::WellKnownProvider;
use skill::types::{AgentId, InstallOptions, ParsedSource};

use super::select::{resolve_mode, resolve_scope};
use super::{AddArgs, hooks, install, output, select_agents};
use crate::ui::emit;
use crate::ui::{DIM, GREEN, RED, RESET, TEXT};

/// Install skills advertised under `/.well-known/skills/index.json`.
pub(super) async fn run(
    parsed: &ParsedSource,
    args: &AddArgs,
    manager: &SkillManager,
    cwd: &Path,
) -> Result<Option<Vec<AgentId>>> {
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
    let scope = resolve_scope(args.global, args.yes, &target_agents, manager)?;
    let mode = resolve_mode(args.copy, args.yes)?;

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

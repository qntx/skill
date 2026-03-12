//! `skills list` command implementation.

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result};

use skill::SkillManager;
use skill::types::{AgentId, InstallScope, ListOptions};

use crate::ui;

/// Arguments for the `list` command.
#[derive(Args)]
pub struct ListArgs {
    /// List global skills (default: project).
    #[arg(short, long)]
    pub global: bool,

    /// Filter by specific agents.
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Output as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Run the list command.
pub async fn run(args: ListArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    let scope = if args.global {
        Some(InstallScope::Global)
    } else {
        Some(InstallScope::Project)
    };

    let agent_filter: Vec<AgentId> = args
        .agent
        .unwrap_or_default()
        .into_iter()
        .map(AgentId::new)
        .collect();

    let options = ListOptions {
        scope,
        agent_filter,
        cwd: Some(cwd.clone()),
    };

    let installed = manager
        .list_installed(&options)
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    // JSON mode
    if args.json {
        let json_output: Vec<serde_json::Value> = installed
            .iter()
            .map(|s| {
                let agents: Vec<String> = s
                    .agents
                    .iter()
                    .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
                    .collect();
                serde_json::json!({
                    "name": s.name,
                    "path": s.canonical_path.to_string_lossy(),
                    "scope": format!("{:?}", s.scope).to_lowercase(),
                    "agents": agents,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_output).unwrap_or_default()
        );
        return Ok(());
    }

    let scope_label = if args.global { "Global" } else { "Project" };

    if installed.is_empty() {
        println!(
            "  {}",
            style(format!("No {scope_label} skills found.")).dim()
        );
        if args.global {
            println!("  {}", style("Try listing project skills without -g").dim());
        } else {
            println!("  {}", style("Try listing global skills with -g").dim());
        }
        return Ok(());
    }

    println!("  {}", style(format!("{scope_label} Skills")).bold());
    println!();

    for skill in &installed {
        let short_path = ui::shorten_path(&skill.canonical_path, &cwd);
        let agent_names: Vec<String> = skill
            .agents
            .iter()
            .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
            .collect();

        let agent_info = if agent_names.is_empty() {
            format!("{}", style("not linked").yellow())
        } else {
            ui::format_list(&agent_names, 5)
        };

        println!(
            "  {} {}",
            style(&skill.name).cyan(),
            style(&short_path).dim()
        );
        println!("    {} {agent_info}", style("Agents:").dim());
    }

    println!();
    Ok(())
}

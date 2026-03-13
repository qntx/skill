//! `skills list` command implementation.
//!
//! Matches the TS `list.ts` UX: groups skills by plugin name (from lock
//! file), displays agent info per skill, and supports JSON output.

use std::collections::BTreeMap;

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

    let installed = manager
        .list_installed(&ListOptions {
            scope,
            agent_filter,
            cwd: Some(cwd.clone()),
        })
        .await
        .map_err(|e| miette::miette!("{e}"))?;

    // JSON mode — raw output, no cliclack framing
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

    cliclack::intro(style(format!(" {scope_label} Skills ")).on_cyan().black())
        .into_diagnostic()?;

    if installed.is_empty() {
        let hint = if args.global {
            "Try listing project skills without -g"
        } else {
            "Try listing global skills with -g"
        };
        cliclack::outro(format!(
            "{}  {}",
            style(format!("No {scope_label} skills found.")).dim(),
            style(hint).dim()
        ))
        .into_diagnostic()?;
        return Ok(());
    }

    let lock =
        skill::lock::read_skill_lock()
            .await
            .unwrap_or_else(|_| skill::lock::SkillLockFile {
                version: 3,
                skills: std::collections::HashMap::new(),
                dismissed: None,
                last_selected_agents: None,
            });

    let mut grouped: BTreeMap<String, Vec<&skill::types::InstalledSkill>> = BTreeMap::new();
    for s in &installed {
        let plugin = lock
            .skills
            .get(&s.name)
            .and_then(|e| e.plugin_name.clone())
            .unwrap_or_default();
        grouped.entry(plugin).or_default().push(s);
    }

    for (plugin, skills) in &grouped {
        if !plugin.is_empty() {
            cliclack::log::info(format!("▸ {plugin}")).into_diagnostic()?;
        }

        for skill_item in skills {
            let short_path = ui::shorten_path(&skill_item.canonical_path);
            let agent_names: Vec<String> = skill_item
                .agents
                .iter()
                .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
                .collect();

            let agent_info = if agent_names.is_empty() {
                format!("{}", style("not linked").yellow())
            } else {
                ui::format_list(&agent_names)
            };

            cliclack::log::success(format!(
                "{} {}  agents: {agent_info}",
                style(&skill_item.name).cyan(),
                style(&short_path).dim()
            ))
            .into_diagnostic()?;
        }
    }

    cliclack::outro(format!(
        "{} skill(s) installed",
        style(installed.len()).green()
    ))
    .into_diagnostic()?;
    Ok(())
}

//! `skills list` command implementation.
//!
//! Matches the TS `list.ts` UX: groups skills by plugin name (from lock
//! file), displays agent info per skill, and supports JSON output.
//! Uses plain console output (no cliclack framing) to match TS style.

use std::collections::BTreeMap;

use clap::Args;
use miette::{IntoDiagnostic, Result};

use skill::SkillManager;
use skill::types::{AgentId, InstallScope, ListOptions};

use crate::ui;

const DIM: &str = "\x1b[38;5;102m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

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

    // JSON mode — raw output
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
            "{DIM}No {} skills found.{RESET}",
            scope_label.to_lowercase()
        );
        if args.global {
            println!("{DIM}Try listing project skills without -g{RESET}");
        } else {
            println!("{DIM}Try listing global skills with -g{RESET}");
        }
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

    // Group skills by plugin
    let mut grouped: BTreeMap<String, Vec<&skill::types::InstalledSkill>> = BTreeMap::new();
    let mut ungrouped: Vec<&skill::types::InstalledSkill> = Vec::new();

    for s in &installed {
        let plugin = lock
            .skills
            .get(&s.name)
            .and_then(|e| e.plugin_name.clone())
            .unwrap_or_default();
        if plugin.is_empty() {
            ungrouped.push(s);
        } else {
            grouped.entry(plugin).or_default().push(s);
        }
    }

    let has_groups = !grouped.is_empty();

    println!("{BOLD}{scope_label} Skills{RESET}");
    println!();

    let print_skill = |skill_item: &skill::types::InstalledSkill, indent: bool| {
        let prefix = if indent { "  " } else { "" };
        let short_path = ui::shorten_path_with_cwd(&skill_item.canonical_path, &cwd);
        let agent_names: Vec<String> = skill_item
            .agents
            .iter()
            .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
            .collect();

        let agent_info = if agent_names.is_empty() {
            format!("{YELLOW}not linked{RESET}")
        } else {
            ui::format_list(&agent_names)
        };

        println!(
            "{prefix}{CYAN}{}{RESET} {DIM}{short_path}{RESET}",
            skill_item.name
        );
        println!("{prefix}  {DIM}Agents:{RESET} {agent_info}");
    };

    if has_groups {
        for (plugin, skills) in &grouped {
            // Convert kebab-case to Title Case
            let title: String = plugin
                .split('-')
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            println!("{BOLD}{title}{RESET}");
            for skill_item in skills {
                print_skill(skill_item, true);
            }
            println!();
        }

        if !ungrouped.is_empty() {
            println!("{BOLD}General{RESET}");
            for skill_item in &ungrouped {
                print_skill(skill_item, true);
            }
            println!();
        }
    } else {
        for skill_item in &installed {
            print_skill(skill_item, false);
        }
        println!();
    }

    Ok(())
}

//! `skills list` command implementation.
//!
//! Matches the TS `list.ts` UX: groups skills by plugin name (from lock
//! file), displays agent info per skill, and supports JSON output.
//! Uses plain console output (no cliclack framing) to match TS style.

use std::collections::BTreeMap;
use std::path::Path;

use clap::Args;
use miette::{IntoDiagnostic, Result};
use skill::SkillManager;
use skill::types::{AgentId, InstallScope, ListOptions};

use crate::ui::{self, BOLD, CYAN, DIM, RESET, YELLOW, kebab_to_title};

/// Arguments for the `list` command.
#[derive(Args)]
pub(crate) struct ListArgs {
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
pub(crate) async fn run(args: ListArgs) -> Result<()> {
    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;

    let scope = Some(if args.global {
        InstallScope::Global
    } else {
        InstallScope::Project
    });

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

    if args.json {
        print_json(&installed, &manager);
        return Ok(());
    }

    let scope_label = if args.global { "Global" } else { "Project" };

    if installed.is_empty() {
        print_empty_hint(scope_label, args.global);
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

    let (grouped, ungrouped) = partition_by_plugin(&installed, &lock);

    println!("{BOLD}{scope_label} Skills{RESET}");
    println!();

    if grouped.is_empty() {
        for skill_item in &installed {
            print_skill(skill_item, false, &manager, &cwd);
        }
        println!();
    } else {
        for (plugin, skills) in &grouped {
            println!("{BOLD}{}{RESET}", kebab_to_title(plugin));
            for skill_item in skills {
                print_skill(skill_item, true, &manager, &cwd);
            }
            println!();
        }
        if !ungrouped.is_empty() {
            println!("{BOLD}General{RESET}");
            for skill_item in &ungrouped {
                print_skill(skill_item, true, &manager, &cwd);
            }
            println!();
        }
    }

    Ok(())
}

/// Print the JSON-serialized view of installed skills.
fn print_json(installed: &[skill::types::InstalledSkill], manager: &SkillManager) {
    let json_output: Vec<serde_json::Value> = installed
        .iter()
        .map(|s| {
            let agents: Vec<String> = s
                .agents
                .iter()
                .filter_map(|id| manager.agents().get(id).map(|c| c.display_name.clone()))
                .collect();
            let resolved = s.canonical_path.as_deref().unwrap_or(&s.path);
            serde_json::json!({
                "name": s.name,
                "path": resolved.to_string_lossy(),
                "scope": format!("{:?}", s.scope).to_lowercase(),
                "agents": agents,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json_output).unwrap_or_default()
    );
}

/// Print a helpful hint when no skills were found for the selected scope.
fn print_empty_hint(scope_label: &str, global: bool) {
    println!(
        "{DIM}No {} skills found.{RESET}",
        scope_label.to_lowercase()
    );
    if global {
        println!("{DIM}Try listing project skills without -g{RESET}");
    } else {
        println!("{DIM}Try listing global skills with -g{RESET}");
    }
}

/// Split installed skills into plugin-grouped and ungrouped buckets.
fn partition_by_plugin<'a>(
    installed: &'a [skill::types::InstalledSkill],
    lock: &skill::lock::SkillLockFile,
) -> (
    BTreeMap<String, Vec<&'a skill::types::InstalledSkill>>,
    Vec<&'a skill::types::InstalledSkill>,
) {
    let mut grouped: BTreeMap<String, Vec<&skill::types::InstalledSkill>> = BTreeMap::new();
    let mut ungrouped: Vec<&skill::types::InstalledSkill> = Vec::new();

    for s in installed {
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

    (grouped, ungrouped)
}

/// Print a single installed skill with its attached agents.
fn print_skill(
    skill_item: &skill::types::InstalledSkill,
    indent: bool,
    manager: &SkillManager,
    cwd: &Path,
) {
    let prefix = if indent { "  " } else { "" };
    let resolved = skill_item
        .canonical_path
        .as_deref()
        .unwrap_or(&skill_item.path);
    let short_path = ui::shorten_path_with_cwd(resolved, cwd);
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
}

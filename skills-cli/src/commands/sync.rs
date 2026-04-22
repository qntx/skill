//! `skills experimental_sync` command implementation.
//!
//! Syncs skills from `node_modules` into agent directories.  Matches TS
//! `sync.ts`: computes skill folder hashes, checks local lock for
//! up-to-date skills, uses search-multiselect for agent selection, and
//! updates the local lock file after installation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use clap::Args;
use miette::{IntoDiagnostic, Result};
use skill::SkillManager;
use skill::local_lock::{self, LocalSkillLockEntry};
use skill::skills::discover_skills;
use skill::types::{AgentId, DiscoverOptions, InstallMode, InstallOptions, InstallScope, Skill};

use crate::ui::emit;
use crate::ui::{self, CYAN, DIM, GREEN, INTRO_TAG, RED, RESET, YELLOW};

/// Arguments for the `experimental_sync` command.
#[derive(Args)]
pub(crate) struct SyncArgs {
    /// Target agents (use '*' for all).
    #[arg(short, long, num_args = 1..)]
    pub agent: Option<Vec<String>>,

    /// Skip confirmation prompts.
    #[arg(short, long)]
    pub yes: bool,

    /// Force reinstall all skills (skip up-to-date check).
    #[arg(short, long)]
    pub force: bool,
}

#[allow(
    clippy::excessive_nesting,
    reason = "scoped npm packages require extra nesting"
)]
async fn scan_node_modules(node_modules: &Path, discover_opts: &DiscoverOptions) -> Vec<Skill> {
    let mut skills = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(node_modules).await else {
        return skills;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if name_str.starts_with('@') {
            let scope_dir = entry.path();
            if let Ok(mut scoped) = tokio::fs::read_dir(&scope_dir).await {
                while let Ok(Some(pkg)) = scoped.next_entry().await {
                    let pkg_path = pkg.path();
                    if let Ok(found) = discover_skills(&pkg_path, None, discover_opts).await {
                        skills.extend(found);
                    }
                }
            }
        } else {
            let pkg_path = entry.path();
            if let Ok(found) = discover_skills(&pkg_path, None, discover_opts).await {
                skills.extend(found);
            }
        }
    }

    skills
}

/// Filter out skills that are already up-to-date in the local lock file.
async fn filter_outdated(skills: Vec<Skill>, cwd: &Path) -> (Vec<Skill>, usize) {
    let lock =
        local_lock::read_local_lock(cwd)
            .await
            .unwrap_or_else(|_| local_lock::LocalSkillLockFile {
                version: 1,
                skills: BTreeMap::default(),
            });

    let mut outdated = Vec::new();
    let mut up_to_date = 0usize;

    for skill_item in skills {
        let current_hash = local_lock::compute_skill_folder_hash(&skill_item.path)
            .await
            .unwrap_or_default();

        if let Some(entry) = lock.skills.get(&skill_item.name)
            && entry.computed_hash == current_hash
            && !current_hash.is_empty()
        {
            up_to_date += 1;
            continue;
        }

        outdated.push(skill_item);
    }

    (outdated, up_to_date)
}

fn derive_package_name(skill_path: &Path, node_modules: &Path) -> String {
    let Ok(rel) = skill_path.strip_prefix(node_modules) else {
        return String::new();
    };
    let mut components = rel.components();
    let first = components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    if first.starts_with('@') {
        let second = components
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        if second.is_empty() {
            first
        } else {
            format!("{first}/{second}")
        }
    } else {
        first
    }
}

struct SyncInstallOk {
    skill: String,
    package_name: String,
    #[allow(dead_code, reason = "kept for future display/logging")]
    agent: String,
    canonical_path: Option<PathBuf>,
}

struct SyncInstallErr {
    skill: String,
    agent: String,
    error: String,
}

/// Run the `experimental_sync` command.
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    reason = "multi-phase sync logic with install/uninstall"
)]
pub(crate) async fn run(args: SyncArgs) -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let node_modules = cwd.join("node_modules");

    if !node_modules.exists() {
        println!("{DIM}No node_modules directory found.{RESET}");
        return Ok(());
    }

    let manager = SkillManager::builder().build();

    println!();
    emit::intro(format!("{INTRO_TAG} skills experimental_sync {RESET}"));

    let spinner = cliclack::spinner();

    spinner.start("Scanning node_modules for skills...");
    let discover_opts = DiscoverOptions::default();
    let all_skills = scan_node_modules(&node_modules, &discover_opts).await;

    if all_skills.is_empty() {
        spinner.stop(format!("{YELLOW}No skills found{RESET}"));
        emit::outro(format!(
            "{DIM}No SKILL.md files found in node_modules.{RESET}"
        ));
        return Ok(());
    }

    spinner.stop(format!(
        "Found {GREEN}{}{RESET} skill{} in node_modules",
        all_skills.len(),
        if all_skills.len() > 1 { "s" } else { "" }
    ));

    for s in &all_skills {
        let pkg = derive_package_name(&s.path, &node_modules);
        emit::info(format!("{CYAN}{}{RESET} {DIM}from {pkg}{RESET}", s.name));
        if !s.description.is_empty() {
            emit::remark(format!("  {DIM}{}{RESET}", s.description));
        }
    }

    let (skills_to_sync, up_to_date) = if args.force {
        emit::info(format!("{DIM}Force mode: reinstalling all skills{RESET}"));
        (all_skills, 0)
    } else {
        filter_outdated(all_skills, &cwd).await
    };

    if up_to_date > 0 {
        emit::info(format!(
            "{DIM}{up_to_date} skill{} already up to date{RESET}",
            if up_to_date == 1 { "" } else { "s" }
        ));
    }

    if skills_to_sync.is_empty() {
        println!();
        emit::outro(format!("{GREEN}All skills are up to date.{RESET}"));
        return Ok(());
    }

    emit::info(format!(
        "{} skill{} to install/update",
        skills_to_sync.len(),
        if skills_to_sync.len() == 1 { "" } else { "s" }
    ));

    let target_agents: Vec<AgentId> =
        super::add::select_agents(&manager, args.agent.as_ref(), args.yes).await?;

    let mut summary_lines: Vec<String> = Vec::new();
    for s in &skills_to_sync {
        let canonical = skill::installer::get_canonical_path(&s.name, InstallScope::Project, &cwd);
        let short = ui::shorten_path_with_cwd(&canonical, &cwd);
        let pkg = derive_package_name(&s.path, &node_modules);
        summary_lines.push(format!("{CYAN}{}{RESET} {DIM}← {pkg}{RESET}", s.name));
        summary_lines.push(format!("  {DIM}{short}{RESET}"));
    }

    println!();
    emit::note("Sync Summary", summary_lines.join("\n"));

    if !args.yes {
        ui::drain_input_events();
        let confirmed: bool = cliclack::confirm("Proceed with sync?")
            .initial_value(true)
            .interact()
            .into_diagnostic()?;

        if !confirmed {
            emit::outro_cancel("Sync cancelled");
            return Ok(());
        }
    }

    let sync_spinner = cliclack::spinner();
    sync_spinner.start("Syncing skills...");

    let opts = InstallOptions {
        scope: InstallScope::Project,
        mode: InstallMode::Symlink,
        cwd: Some(cwd.clone()),
    };

    let mut successful: Vec<SyncInstallOk> = Vec::new();
    let mut failed: Vec<SyncInstallErr> = Vec::new();

    for skill_item in &skills_to_sync {
        let pkg = derive_package_name(&skill_item.path, &node_modules);
        for agent_id in &target_agents {
            let display_name = manager
                .agents()
                .get(agent_id)
                .map_or_else(|| agent_id.to_string(), |c| c.display_name.clone());

            match manager.install_skill(skill_item, agent_id, &opts).await {
                Ok(r) => {
                    successful.push(SyncInstallOk {
                        skill: skill_item.name.clone(),
                        package_name: pkg.clone(),
                        agent: display_name,
                        canonical_path: r.canonical_path,
                    });
                }
                Err(e) => {
                    failed.push(SyncInstallErr {
                        skill: skill_item.name.clone(),
                        agent: display_name,
                        error: format!("{e}"),
                    });
                }
            }
        }
    }

    sync_spinner.stop("Sync complete");

    let successful_skill_names: HashSet<&str> =
        successful.iter().map(|r| r.skill.as_str()).collect();

    for skill_item in &skills_to_sync {
        if successful_skill_names.contains(skill_item.name.as_str()) {
            let hash = local_lock::compute_skill_folder_hash(&skill_item.path)
                .await
                .unwrap_or_default();
            let source = derive_package_name(&skill_item.path, &node_modules);
            let _ = local_lock::add_skill_to_local_lock(
                &skill_item.name,
                LocalSkillLockEntry {
                    source,
                    git_ref: None,
                    source_type: "node_modules".to_owned(),
                    computed_hash: hash,
                },
                &cwd,
            )
            .await;
        }
    }

    println!();

    if !successful.is_empty() {
        let mut by_skill: BTreeMap<&str, Vec<&SyncInstallOk>> = BTreeMap::new();
        for r in &successful {
            by_skill.entry(r.skill.as_str()).or_default().push(r);
        }

        let mut result_lines: Vec<String> = Vec::new();
        for (skill_name, skill_results) in &by_skill {
            let Some(first) = skill_results.first() else {
                continue;
            };
            let pkg = &first.package_name;
            result_lines.push(format!("{GREEN}✓{RESET} {skill_name} {DIM}← {pkg}{RESET}"));
            if let Some(ref cp) = first.canonical_path {
                let short = ui::shorten_path_with_cwd(cp, &cwd);
                result_lines.push(format!("  {DIM}{short}{RESET}"));
            }
        }

        let skill_count = by_skill.len();
        let title = format!(
            "{GREEN}Synced {} skill{}{RESET}",
            skill_count,
            if skill_count == 1 { "" } else { "s" }
        );
        emit::note(title, result_lines.join("\n"));
    }

    if !failed.is_empty() {
        println!();
        emit::error(format!("{RED}Failed to install {}{RESET}", failed.len()));
        for r in &failed {
            let err = &r.error;
            emit::remark(format!(
                "  {RED}✗{RESET} {} → {}: {DIM}{err}{RESET}",
                r.skill, r.agent
            ));
        }
    }

    let mut props = HashMap::new();
    props.insert("skillCount".to_owned(), skills_to_sync.len().to_string());
    props.insert(
        "successCount".to_owned(),
        successful_skill_names.len().to_string(),
    );
    props.insert(
        "agents".to_owned(),
        target_agents
            .iter()
            .map(|a| a.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(","),
    );
    skill::telemetry::track("experimental_sync", props);

    println!();
    emit::outro(format!(
        "{GREEN}Done!{RESET}  {DIM}Review skills before use; they run with full agent permissions.{RESET}"
    ));

    Ok(())
}

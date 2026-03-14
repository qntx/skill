//! Display formatting for the `add` command.

use std::collections::BTreeMap;
use std::path::Path;

use skill::SkillManager;
use skill::types::{AgentId, InstallMode, InstallScope, Skill};

use skill::telemetry::AuditResponse;

use crate::ui::{self, DIM, GREEN, RESET, YELLOW};

use super::install::SkillInstallOutcome;
use super::select::kebab_to_title;

/// Print a pre-confirmation installation summary box.
///
/// Checks each skill×agent for existing installs and shows overwrite warnings.
pub(super) async fn print_installation_summary(
    skills: &[Skill],
    agents: &[AgentId],
    manager: &SkillManager,
    scope: InstallScope,
    mode: InstallMode,
    cwd: &Path,
) {
    let mut lines: Vec<String> = Vec::new();

    // Check overwrite status for each skill×agent pair (parallel)
    let mut overwrites: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut join_set = tokio::task::JoinSet::new();

    for s in skills {
        for aid in agents {
            if let Some(config) = manager.agents().get(aid) {
                let skill_name = s.name.clone();
                let display_name = config.display_name.clone();
                let skills_dir = config.skills_dir.clone();
                let global_dir = config.global_skills_dir.clone();
                let cwd = cwd.to_path_buf();
                join_set.spawn(async move {
                    let installed = skill::installer::is_skill_installed_owned(
                        skill_name.clone(),
                        skills_dir,
                        global_dir,
                        scope,
                        cwd,
                    )
                    .await;
                    (skill_name, display_name, installed)
                });
            }
        }
    }

    while let Some(Ok((skill_name, display_name, installed))) = join_set.join_next().await {
        if installed {
            overwrites.entry(skill_name).or_default().push(display_name);
        }
    }

    let mut grouped: BTreeMap<String, Vec<&Skill>> = BTreeMap::new();
    let mut ungrouped: Vec<&Skill> = Vec::new();
    for s in skills {
        if let Some(ref plugin) = s.plugin_name {
            grouped.entry(plugin.clone()).or_default().push(s);
        } else {
            ungrouped.push(s);
        }
    }

    let print_skill_summary =
        |lines: &mut Vec<String>,
         skill_list: &[&Skill],
         overwrites: &std::collections::HashMap<String, Vec<String>>| {
            for s in skill_list {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                let canonical = skill::installer::get_canonical_path(&s.name, scope, cwd);
                let short = ui::shorten_path_with_cwd(&canonical, cwd);
                lines.push(format!("\x1b[36m{short}\x1b[0m"));
                lines.extend(build_agent_summary_lines(agents, manager, mode));

                if let Some(ow_agents) = overwrites.get(&s.name) {
                    lines.push(format!(
                        "  {YELLOW}overwrites:{RESET} {}",
                        ui::format_list(ow_agents)
                    ));
                }
            }
        };

    for (group, skill_list) in &grouped {
        let title = kebab_to_title(group);
        lines.push(String::new());
        lines.push(format!("\x1b[1m{title}\x1b[0m"));
        print_skill_summary(&mut lines, skill_list, &overwrites);
    }

    if !ungrouped.is_empty() {
        if !grouped.is_empty() {
            lines.push(String::new());
            lines.push("\x1b[1m\x1b[0m".to_owned());
        }
        print_skill_summary(&mut lines, &ungrouped, &overwrites);
    }

    if lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }

    let body = lines.join("\n");
    let _ = cliclack::note("Installation Summary", body);
}

fn build_agent_summary_lines(
    agents: &[AgentId],
    manager: &SkillManager,
    mode: InstallMode,
) -> Vec<String> {
    let mut lines = Vec::new();

    if mode == InstallMode::Copy {
        let names: Vec<String> = agents
            .iter()
            .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
            .collect();
        lines.push(format!(
            "  {DIM}copy \u{2192}{RESET} {}",
            ui::format_list(&names)
        ));
        return lines;
    }

    let universal_names: Vec<String> = agents
        .iter()
        .filter(|a| manager.agents().is_universal(a))
        .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
        .collect();
    let symlinked_names: Vec<String> = agents
        .iter()
        .filter(|a| !manager.agents().is_universal(a))
        .filter_map(|a| manager.agents().get(a).map(|c| c.display_name.clone()))
        .collect();

    if !universal_names.is_empty() {
        lines.push(format!(
            "  {GREEN}universal:{RESET} {}",
            ui::format_list(&universal_names)
        ));
    }
    if !symlinked_names.is_empty() {
        lines.push(format!(
            "  {DIM}symlink \u{2192}{RESET} {}",
            ui::format_list(&symlinked_names)
        ));
    }
    lines
}

/// Print the post-install results.
pub(super) fn print_install_results(outcomes: &[SkillInstallOutcome], cwd: &Path) {
    let successful: Vec<&SkillInstallOutcome> = outcomes
        .iter()
        .filter(|o| {
            !o.universal_agents.is_empty()
                || !o.symlinked_agents.is_empty()
                || !o.copied_agents.is_empty()
                || !o.symlink_failed_agents.is_empty()
        })
        .collect();
    let failed_outcomes: Vec<&SkillInstallOutcome> = outcomes
        .iter()
        .filter(|o| !o.failed_agents.is_empty())
        .collect();

    if !successful.is_empty() {
        let mut result_lines: Vec<String> = Vec::new();

        for outcome in &successful {
            let is_copy_mode = !outcome.copied_agents.is_empty()
                && outcome.symlinked_agents.is_empty()
                && outcome.symlink_failed_agents.is_empty();

            if is_copy_mode {
                result_lines.push(format!(
                    "{GREEN}\u{2713}{RESET} {} {DIM}(copied){RESET}",
                    outcome.skill_name
                ));
                for p in &outcome.copy_paths {
                    let short = ui::shorten_path_with_cwd(p, cwd);
                    result_lines.push(format!("  {DIM}\u{2192}{RESET} {short}"));
                }
            } else {
                if let Some(ref canonical) = outcome.canonical_path {
                    let short = ui::shorten_path_with_cwd(canonical, cwd);
                    result_lines.push(format!("{GREEN}\u{2713}{RESET} {short}"));
                } else {
                    result_lines.push(format!("{GREEN}\u{2713}{RESET} {}", outcome.skill_name));
                }

                if !outcome.universal_agents.is_empty() {
                    result_lines.push(format!(
                        "  {GREEN}universal:{RESET} {}",
                        ui::format_list(&outcome.universal_agents)
                    ));
                }
                if !outcome.symlinked_agents.is_empty() {
                    result_lines.push(format!(
                        "  {DIM}symlinked:{RESET} {}",
                        ui::format_list(&outcome.symlinked_agents)
                    ));
                }
                let all_copied: Vec<&String> = outcome.symlink_failed_agents.iter().collect();
                if !all_copied.is_empty() {
                    let names: Vec<String> = all_copied.into_iter().cloned().collect();
                    result_lines.push(format!(
                        "  {YELLOW}copied:{RESET} {}",
                        ui::format_list(&names)
                    ));
                }
            }
        }

        let skill_count = successful.len();
        let title = format!(
            "{GREEN}Installed {} skill{}{RESET}",
            skill_count,
            if skill_count == 1 { "" } else { "s" }
        );

        let body = result_lines.join("\n");
        let _ = cliclack::note(title, body);

        let symlink_failures: Vec<&str> = outcomes
            .iter()
            .flat_map(|o| o.symlink_failed_agents.iter())
            .map(String::as_str)
            .collect();
        if !symlink_failures.is_empty() {
            let _ = cliclack::log::warning(format!(
                "{YELLOW}Symlinks failed for: {}{RESET}",
                ui::format_list(
                    &symlink_failures
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                )
            ));
            let _ = cliclack::log::remark(format!(
                "{DIM}Files were copied instead. On Windows, enable Developer Mode for symlink support.{RESET}"
            ));
        }
    }

    if !failed_outcomes.is_empty() {
        let total_fail: usize = failed_outcomes.iter().map(|o| o.failed_agents.len()).sum();
        println!();
        let _ = cliclack::log::error(format!("\x1b[31mFailed to install {total_fail}\x1b[0m"));
        for outcome in &failed_outcomes {
            for agent in &outcome.failed_agents {
                let _ = cliclack::log::remark(format!(
                    " \x1b[31m\u{2717}\x1b[0m {} \u{2192} {agent}: {DIM}installation error{RESET}",
                    outcome.skill_name
                ));
            }
        }
    }
}

/// Display security audit results from partner scanners.
///
/// Shows a compact table with risk assessments from Gen, Socket, and Snyk.
/// Silently returns if no audit data is available.
pub(super) fn print_security_audit(audit_data: &AuditResponse, skills: &[Skill], source: &str) {
    // Check if we have any meaningful audit data
    let has_any = skills
        .iter()
        .any(|s| audit_data.get(&s.name).is_some_and(|d| !d.is_empty()));
    if !has_any {
        return;
    }

    let name_width = skills
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(10)
        .min(36);

    let mut lines: Vec<String> = Vec::new();

    // Header
    lines.push(format!(
        "{:width$}  {DIM}{}  {}  {}{RESET}",
        "",
        "Gen",
        "Socket",
        "Snyk",
        width = name_width,
    ));

    // Rows
    for skill in skills {
        let display_name = if skill.name.len() > name_width {
            let mut end = name_width - 1;
            while !skill.name.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\u{2026}", &skill.name[..end])
        } else {
            skill.name.clone()
        };

        let data = audit_data.get(&skill.name);

        let ath_col = data
            .and_then(|d| d.get("ath"))
            .map_or_else(|| format!("{DIM}--{RESET}"), |a| risk_label(&a.risk));
        let socket_col = data.and_then(|d| d.get("socket")).map_or_else(
            || format!("{DIM}--{RESET}"),
            |a| {
                let count = a.alerts.unwrap_or(0);
                if count > 0 {
                    format!(
                        "\x1b[31m{} alert{}\x1b[0m",
                        count,
                        if count == 1 { "" } else { "s" }
                    )
                } else {
                    format!("{GREEN}0 alerts{RESET}")
                }
            },
        );
        let snyk_col = data
            .and_then(|d| d.get("snyk"))
            .map_or_else(|| format!("{DIM}--{RESET}"), |a| risk_label(&a.risk));

        lines.push(format!(
            "\x1b[36m{display_name:<name_width$}\x1b[0m  {ath_col}  {socket_col}  {snyk_col}",
        ));
    }

    lines.push(String::new());
    lines.push(format!("{DIM}Details: https://skills.sh/{source}{RESET}"));

    let body = lines.join("\n");
    let _ = cliclack::note("Security Risk Assessments", body);
}

fn risk_label(risk: &str) -> String {
    match risk {
        "critical" => "\x1b[31m\x1b[1mCritical Risk\x1b[0m".to_owned(),
        "high" => "\x1b[31mHigh Risk\x1b[0m".to_owned(),
        "medium" => format!("{YELLOW}Med Risk{RESET}"),
        "low" => format!("{GREEN}Low Risk{RESET}"),
        "safe" => format!("{GREEN}Safe{RESET}"),
        _ => format!("{DIM}--{RESET}"),
    }
}

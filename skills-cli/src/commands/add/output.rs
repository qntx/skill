//! Display formatting for the `add` command.

use std::collections::BTreeMap;
use std::path::Path;

use skill::SkillManager;
use skill::telemetry::AuditResponse;
use skill::types::{AgentId, InstallMode, InstallScope, Skill};

use super::install::SkillInstallOutcome;
use crate::ui::emit;
use crate::ui::{self, BOLD, BOLD_RED, CYAN, DIM, GREEN, RED, RESET, YELLOW, kebab_to_title};

/// Print a pre-confirmation installation summary box.
///
/// Checks each skill×agent for existing installs and shows overwrite warnings.
#[allow(clippy::excessive_nesting, reason = "skill × agent overwrite checking")]
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
            let Some(config) = manager.agents().get(aid) else {
                continue;
            };
            // Pre-compute the candidate on-disk locations synchronously so
            // the spawned task only has to do the (async) `try_exists`
            // probes — no cloning of `AgentConfig` internals, no
            // `'static` lifetime gymnastics.
            let paths = skill::installer::candidate_install_paths(
                &s.name,
                &config.skills_dir,
                config.global_skills_dir.as_deref(),
                scope,
                cwd,
            );
            let skill_name = s.name.clone();
            let display_name = config.display_name.clone();
            join_set.spawn(async move {
                let installed = skill::installer::any_path_exists(&paths).await;
                (skill_name, display_name, installed)
            });
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
        |out_lines: &mut Vec<String>,
         skill_list: &[&Skill],
         ow_map: &std::collections::HashMap<String, Vec<String>>| {
            for s in skill_list {
                if !out_lines.is_empty() {
                    out_lines.push(String::new());
                }
                let canonical = skill::installer::canonical_install_path(&s.name, scope, cwd);
                let short = ui::shorten_path_with_cwd(&canonical, cwd);
                out_lines.push(format!("{CYAN}{short}{RESET}"));
                out_lines.extend(build_agent_summary_lines(agents, manager, mode));

                if let Some(ow_agents) = ow_map.get(&s.name) {
                    out_lines.push(format!(
                        "  {YELLOW}overwrites:{RESET} {}",
                        ui::format_list(ow_agents)
                    ));
                }
            }
        };

    for (group, skill_list) in &grouped {
        let title = kebab_to_title(group);
        lines.push(String::new());
        lines.push(format!("{BOLD}{title}{RESET}"));
        print_skill_summary(&mut lines, skill_list, &overwrites);
    }

    if !ungrouped.is_empty() {
        if !grouped.is_empty() {
            lines.push(String::new());
            lines.push(format!("{BOLD}General{RESET}"));
        }
        print_skill_summary(&mut lines, &ungrouped, &overwrites);
    }

    if lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }

    let body = lines.join("\n");
    emit::note("Installation Summary", body);
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

fn append_outcome_lines(lines: &mut Vec<String>, outcomes: &[&SkillInstallOutcome], cwd: &Path) {
    for outcome in outcomes {
        if outcome.is_pure_copy_mode() {
            lines.push(format!(
                "{GREEN}✓{RESET} {} {DIM}(copied){RESET}",
                outcome.skill_name
            ));
            for p in outcome.copy_paths() {
                let short = ui::shorten_path_with_cwd(p, cwd);
                lines.push(format!("  {DIM}→{RESET} {short}"));
            }
        } else if let Some(ref canonical) = outcome.canonical_path {
            let short = ui::shorten_path_with_cwd(canonical, cwd);
            lines.push(format!("{GREEN}✓{RESET} {short}"));
            append_agent_lines(lines, outcome);
        } else {
            lines.push(format!("{GREEN}✓{RESET} {}", outcome.skill_name));
            append_agent_lines(lines, outcome);
        }
    }
}

fn append_agent_lines(lines: &mut Vec<String>, outcome: &SkillInstallOutcome) {
    let groups = outcome.by_status();
    if !groups.universal.is_empty() {
        lines.push(format!(
            "  {GREEN}universal:{RESET} {}",
            ui::format_list(&to_owned_vec(&groups.universal))
        ));
    }
    if !groups.symlinked.is_empty() {
        lines.push(format!(
            "  {DIM}symlinked:{RESET} {}",
            ui::format_list(&to_owned_vec(&groups.symlinked))
        ));
    }
    if !groups.copied.is_empty() {
        lines.push(format!(
            "  {DIM}copied:{RESET} {}",
            ui::format_list(&to_owned_vec(&groups.copied))
        ));
    }
    if !groups.symlink_failed.is_empty() {
        lines.push(format!(
            "  {YELLOW}copied:{RESET} {}",
            ui::format_list(&to_owned_vec(&groups.symlink_failed))
        ));
    }
}

fn to_owned_vec(slice: &[&str]) -> Vec<String> {
    slice.iter().map(|s| (*s).to_owned()).collect()
}

/// Print the post-install results, grouped by plugin name (matching TS).
pub(super) fn print_install_results(outcomes: &[SkillInstallOutcome], cwd: &Path) {
    let successful: Vec<&SkillInstallOutcome> =
        outcomes.iter().filter(|o| o.has_success()).collect();
    let failed_outcomes: Vec<&SkillInstallOutcome> = outcomes
        .iter()
        .filter(|o| o.failed_agents().next().is_some())
        .collect();

    if !successful.is_empty() {
        let mut result_lines: Vec<String> = Vec::new();

        let mut grouped: BTreeMap<String, Vec<&SkillInstallOutcome>> = BTreeMap::new();
        let mut ungrouped: Vec<&SkillInstallOutcome> = Vec::new();
        for o in &successful {
            if let Some(ref plugin) = o.plugin_name {
                grouped.entry(plugin.clone()).or_default().push(o);
            } else {
                ungrouped.push(o);
            }
        }

        for (group, entries) in &grouped {
            let title = kebab_to_title(group);
            result_lines.push(String::new());
            result_lines.push(format!("{BOLD}{title}{RESET}"));
            append_outcome_lines(&mut result_lines, entries, cwd);
        }

        if !ungrouped.is_empty() {
            if !grouped.is_empty() {
                result_lines.push(String::new());
                result_lines.push(format!("{BOLD}General{RESET}"));
            }
            append_outcome_lines(&mut result_lines, &ungrouped, cwd);
        }

        if result_lines.first().is_some_and(String::is_empty) {
            result_lines.remove(0);
        }

        let skill_count = successful.len();
        let title = format!(
            "{GREEN}Installed {} skill{}{RESET}",
            skill_count,
            if skill_count == 1 { "" } else { "s" }
        );

        let body = result_lines.join("\n");
        emit::note(title, body);

        let symlink_failures: Vec<String> = outcomes
            .iter()
            .flat_map(SkillInstallOutcome::symlink_fallback_agents)
            .map(str::to_owned)
            .collect();
        if !symlink_failures.is_empty() {
            emit::warning(format!(
                "{YELLOW}Symlinks failed for: {}{RESET}",
                ui::format_list(&symlink_failures)
            ));
            emit::remark(format!(
                "{DIM}Files were copied instead. On Windows, enable Developer Mode for symlink support.{RESET}"
            ));
        }
    }

    if !failed_outcomes.is_empty() {
        let total_fail: usize = failed_outcomes
            .iter()
            .map(|o| o.failed_agents().count())
            .sum();
        println!();
        emit::error(format!("{RED}Failed to install {total_fail}{RESET}"));
        for outcome in &failed_outcomes {
            for agent in outcome.failed_agents() {
                emit::remark(format!(
                    " {RED}✗{RESET} {} → {agent}: {DIM}installation error{RESET}",
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

    // Header — padEnd matching TS: empty name col + Gen(18) + Socket(18) + Snyk
    let header = format!(
        "{}{}{}",
        ansi_pad_end("", name_width + 2),
        ansi_pad_end(&format!("{DIM}Gen{RESET}"), 18),
        ansi_pad_end(&format!("{DIM}Socket{RESET}"), 18),
    );
    lines.push(format!("{header}{DIM}Snyk{RESET}"));

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
                        "{RED}{} alert{}{RESET}",
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

        let name_col = ansi_pad_end(&format!("{CYAN}{display_name}{RESET}"), name_width + 2);
        let row = format!(
            "{name_col}{}{}{}",
            ansi_pad_end(&ath_col, 18),
            ansi_pad_end(&socket_col, 18),
            snyk_col,
        );
        lines.push(row);
    }

    lines.push(String::new());
    lines.push(format!("{DIM}Details: https://skills.sh/{source}{RESET}"));

    let body = lines.join("\n");
    emit::note("Security Risk Assessments", body);
}

/// Pad a string to a given visible width, ignoring ANSI escape codes.
/// Matches the TS `padEnd` function exactly.
fn ansi_pad_end(s: &str, width: usize) -> String {
    let mut visible = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            visible += 1;
        }
    }
    let pad = width.saturating_sub(visible);
    format!("{s}{}", " ".repeat(pad))
}

fn risk_label(risk: &str) -> String {
    match risk {
        "critical" => format!("{BOLD_RED}Critical Risk{RESET}"),
        "high" => format!("{RED}High Risk{RESET}"),
        "medium" => format!("{YELLOW}Med Risk{RESET}"),
        "low" => format!("{GREEN}Low Risk{RESET}"),
        "safe" => format!("{GREEN}Safe{RESET}"),
        _ => format!("{DIM}--{RESET}"),
    }
}

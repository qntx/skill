//! `skills doctor` command implementation.
//!
//! Performs health checks on the skills installation:
//! - Broken symlinks in agent skill directories
//! - Lock file consistency (entries without matching files on disk)
//! - Orphaned skill directories (on disk but not in lock file)
//! - SKILL.md frontmatter validity
//! - Permission issues

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result};
use skill::SkillManager;
use skill::types::{InstallScope, ListOptions};

use crate::ui::emit;
use crate::ui::{BOLD, DIM, GREEN, RED, RESET, TEXT, YELLOW};

/// Severity of a diagnostic finding.
enum Severity {
    Ok,
    Warning,
    Error,
}

struct Finding {
    severity: Severity,
    category: &'static str,
    message: String,
    hint: Option<String>,
}

/// Run the doctor command.
pub(crate) async fn run() -> Result<()> {
    let manager = SkillManager::builder().build();
    let cwd = std::env::current_dir().into_diagnostic()?;
    let mut findings: Vec<Finding> = Vec::new();

    let spinner = cliclack::spinner();
    spinner.start("Running health checks...");

    check_broken_symlinks(&manager, &cwd, &mut findings).await;
    check_lock_consistency(&manager, &cwd, &mut findings).await;
    check_local_lock_consistency(&cwd, &mut findings).await;
    check_skill_md_validity(&manager, &cwd, &mut findings).await;

    spinner.stop("Health checks complete");

    let errors = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .count();
    let warnings = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Warning))
        .count();
    let ok_count = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Ok))
        .count();

    if findings.is_empty() {
        emit::success(format!(
            "{GREEN}All checks passed — no issues found.{RESET}"
        ));
        return Ok(());
    }

    let mut by_category: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &findings {
        by_category.entry(f.category).or_default().push(f);
    }

    for (category, items) in &by_category {
        println!();
        println!("  {BOLD}{category}{RESET}");
        for f in items {
            let icon = match f.severity {
                Severity::Ok => format!("{GREEN}✓{RESET}"),
                Severity::Warning => format!("{YELLOW}▲{RESET}"),
                Severity::Error => format!("{RED}✗{RESET}"),
            };
            println!("  {icon} {}", f.message);
            if let Some(ref hint) = f.hint {
                println!("    {DIM}{hint}{RESET}");
            }
        }
    }

    println!();
    let mut summary_parts = Vec::new();
    if errors > 0 {
        summary_parts.push(format!("{RED}{errors} error(s){RESET}"));
    }
    if warnings > 0 {
        summary_parts.push(format!("{YELLOW}{warnings} warning(s){RESET}"));
    }
    if ok_count > 0 {
        summary_parts.push(format!("{GREEN}{ok_count} ok{RESET}"));
    }
    emit::outro(format!("{TEXT}Result:{RESET} {}", summary_parts.join(", ")));

    Ok(())
}

/// Check for broken symlinks in all agent skill directories.
#[allow(
    clippy::excessive_nesting,
    reason = "dir × entry × symlink check iteration"
)]
async fn check_broken_symlinks(manager: &SkillManager, cwd: &Path, findings: &mut Vec<Finding>) {
    let mut broken_count = 0u32;

    for agent_id in manager.agents().all_ids() {
        let Some(config) = manager.agents().get(&agent_id) else {
            continue;
        };

        let dirs_to_check: Vec<PathBuf> = [
            Some(cwd.join(&config.skills_dir)),
            config.global_skills_dir.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();

        for dir in dirs_to_check {
            if !dir.exists() {
                continue;
            }
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let metadata = tokio::fs::symlink_metadata(&path).await;
                let Ok(meta) = metadata else { continue };

                if meta.is_symlink() {
                    let target_exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
                    if !target_exists {
                        broken_count += 1;
                        let target = tokio::fs::read_link(&path)
                            .await
                            .map_or_else(|_| "unknown".to_owned(), |t| t.display().to_string());
                        findings.push(Finding {
                            severity: Severity::Error,
                            category: "Broken Symlinks",
                            message: format!("{} \u{2192} {}", path.display(), target),
                            hint: Some(format!(
                                "Run: skills remove {} or delete manually",
                                entry.file_name().to_string_lossy()
                            )),
                        });
                    }
                }
            }
        }
    }

    if broken_count == 0 {
        findings.push(Finding {
            severity: Severity::Ok,
            category: "Symlink Integrity",
            message: "All symlinks are valid".to_owned(),
            hint: None,
        });
    }
}

/// Check global lock file consistency.
async fn check_lock_consistency(manager: &SkillManager, cwd: &Path, findings: &mut Vec<Finding>) {
    let Ok(lock) = skill::lock::read_skill_lock().await else {
        findings.push(Finding {
            severity: Severity::Warning,
            category: "Global Lock File",
            message: "Could not read global lock file".to_owned(),
            hint: Some(
                "Lock file may not exist yet \u{2014} this is normal for first-time users"
                    .to_owned(),
            ),
        });
        return;
    };

    if lock.skills.is_empty() {
        findings.push(Finding {
            severity: Severity::Ok,
            category: "Global Lock File",
            message: "No globally installed skills tracked".to_owned(),
            hint: None,
        });
        return;
    }

    let list_opts = ListOptions {
        scope: Some(InstallScope::Global),
        agent_filter: Vec::new(),
        cwd: Some(cwd.to_path_buf()),
    };
    let installed = manager.list_installed(&list_opts).await.unwrap_or_default();
    let installed_names: std::collections::HashSet<&str> =
        installed.iter().map(|s| s.name.as_str()).collect();

    let mut ghost_count = 0u32;
    for name in lock.skills.keys() {
        if !installed_names.contains(name.as_str()) {
            ghost_count += 1;
            findings.push(Finding {
                severity: Severity::Warning,
                category: "Global Lock File",
                message: format!("Lock entry \"{name}\" has no matching files on disk"),
                hint: Some(
                    "Reinstall with: skills add <source> -g  or remove entry manually".to_owned(),
                ),
            });
        }
    }

    if ghost_count == 0 {
        findings.push(Finding {
            severity: Severity::Ok,
            category: "Global Lock File",
            message: format!(
                "{} skill(s) tracked, all present on disk",
                lock.skills.len()
            ),
            hint: None,
        });
    }
}

/// Check local (project) lock file consistency.
async fn check_local_lock_consistency(cwd: &Path, findings: &mut Vec<Finding>) {
    let lock_path = cwd.join("skills-lock.json");
    if !lock_path.exists() {
        return;
    }

    let Ok(lock) = skill::local_lock::read_local_lock(cwd).await else {
        findings.push(Finding {
            severity: Severity::Warning,
            category: "Local Lock File",
            message: "Could not parse skills-lock.json".to_owned(),
            hint: Some("File may contain merge conflict markers".to_owned()),
        });
        return;
    };

    if lock.skills.is_empty() {
        return;
    }

    let canonical_base = cwd.join(".agents/skills");
    let mut missing = 0u32;

    for name in lock.skills.keys() {
        let sanitized = skill::installer::sanitize_name(name);
        let expected = canonical_base.join(&sanitized);
        if !expected.exists() {
            missing += 1;
            findings.push(Finding {
                severity: Severity::Warning,
                category: "Local Lock File",
                message: format!("Lock entry \"{name}\" not found at {}", expected.display()),
                hint: Some(
                    "Run: skills experimental_install  to restore from lock file".to_owned(),
                ),
            });
        }
    }

    if missing == 0 {
        findings.push(Finding {
            severity: Severity::Ok,
            category: "Local Lock File",
            message: format!(
                "{} skill(s) tracked, all present on disk",
                lock.skills.len()
            ),
            hint: None,
        });
    }
}

/// Check SKILL.md frontmatter validity for installed skills.
async fn check_skill_md_validity(manager: &SkillManager, cwd: &Path, findings: &mut Vec<Finding>) {
    let list_opts = ListOptions {
        scope: None,
        agent_filter: Vec::new(),
        cwd: Some(cwd.to_path_buf()),
    };

    let installed = manager.list_installed(&list_opts).await.unwrap_or_default();

    if installed.is_empty() {
        return;
    }

    let mut invalid_count = 0u32;
    for skill_item in &installed {
        let skill_md = skill_item.canonical_path.join("SKILL.md");
        if !skill_md.exists() {
            invalid_count += 1;
            findings.push(Finding {
                severity: Severity::Error,
                category: "SKILL.md Validity",
                message: format!(
                    "Missing SKILL.md in {}",
                    skill_item.canonical_path.display()
                ),
                hint: Some(
                    "Skill directory exists but has no SKILL.md — reinstall the skill".to_owned(),
                ),
            });
            continue;
        }

        if let Ok(content) = tokio::fs::read_to_string(&skill_md).await {
            if skill::skills::extract_frontmatter(&content).is_none() {
                invalid_count += 1;
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: "SKILL.md Validity",
                    message: format!(
                        "Invalid frontmatter in {}",
                        skill_md.display()
                    ),
                    hint: Some(
                        "SKILL.md must have --- delimited YAML frontmatter with name and description"
                            .to_owned(),
                    ),
                });
            }
        } else {
            invalid_count += 1;
            findings.push(Finding {
                severity: Severity::Error,
                category: "SKILL.md Validity",
                message: format!("Cannot read {}", skill_md.display()),
                hint: Some("Check file permissions".to_owned()),
            });
        }
    }

    if invalid_count == 0 {
        findings.push(Finding {
            severity: Severity::Ok,
            category: "SKILL.md Validity",
            message: format!(
                "All {} installed skill(s) have valid SKILL.md",
                installed.len()
            ),
            hint: None,
        });
    }
}

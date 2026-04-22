//! CLI command implementations.

use std::collections::BTreeMap;

use skill::sanitize::sanitize_metadata;

use crate::ui::{DIM, RESET, TEXT};

pub(crate) mod add;
pub(crate) mod check;
pub(crate) mod completions;
pub(crate) mod doctor;
pub(crate) mod find;
pub(crate) mod init;
pub(crate) mod install_lock;
pub(crate) mod list;
pub(crate) mod remove;
pub(crate) mod sync;
pub(crate) mod update;
pub(crate) mod upgrade;

/// A skill that was skipped during check/update (no trackable version info).
///
/// Mirrors the TS `SkippedSkill` struct in `cli.ts`. All fields are required
/// to reconstruct a manual install command for the user.
pub(crate) struct SkippedSkill {
    pub name: String,
    pub reason: String,
    pub source_url: String,
    pub source_type: String,
    pub git_ref: Option<String>,
}

/// Whether a lock entry should be skipped (not enough info to check).
///
/// Matches TS: `!entry.skillFolderHash || !entry.skillPath`.
pub(crate) const fn should_skip(entry: &skill::lock::SkillLockEntry) -> bool {
    entry.skill_folder_hash.is_empty() || entry.skill_path.is_none()
}

/// Human-readable skip reason for a lock entry.
///
/// Matches TS `getSkipReason` exactly:
/// `local` / `git` / `well-known` short-circuit first, then
/// `!skillFolderHash` \u2192 "Private or deleted repo",
/// `!skillPath`       \u2192 "No skill path recorded",
/// else               \u2192 "No version tracking".
pub(crate) fn skip_reason(entry: &skill::lock::SkillLockEntry) -> String {
    match entry.source_type.as_str() {
        "local" => "Local path".to_owned(),
        "git" => "Git URL".to_owned(),
        "well-known" => "Well-known skill".to_owned(),
        _ if entry.skill_folder_hash.is_empty() => "Private or deleted repo".to_owned(),
        _ if entry.skill_path.is_none() => "No skill path recorded".to_owned(),
        _ => "No version tracking".to_owned(),
    }
}

/// Append `#ref` to a URL if `git_ref` is set (matches TS `formatSourceInput`).
fn format_source_input(source_url: &str, git_ref: Option<&str>) -> String {
    git_ref.map_or_else(|| source_url.to_owned(), |r| format!("{source_url}#{r}"))
}

/// Reconstruct the install source for a skipped skill.
///
/// For well-known skills the stored `source_url` points at `SKILL.md` inside
/// `.well-known/...`; we strip that suffix so the printed command matches the
/// URL the user originally typed. Mirrors TS `getInstallSource`.
fn install_source(skill: &SkippedSkill) -> String {
    let mut url = skill.source_url.as_str();
    if skill.source_type == "well-known"
        && let Some(idx) = url.find("/.well-known/")
    {
        url = &url[..idx];
    }
    format_source_input(url, skill.git_ref.as_deref())
}

/// Print skipped skills with manual update instructions.
///
/// Skills from the same install source are grouped together (matches TS
/// `printSkippedSkills`). Names are sanitized against terminal injection.
pub(crate) fn print_skipped_skills(skipped: &[SkippedSkill]) {
    if skipped.is_empty() {
        return;
    }
    println!();
    println!(
        "{DIM}{} skill(s) cannot be checked automatically:{RESET}",
        skipped.len()
    );

    // Group by reconstructed install source while preserving first-seen order.
    let mut grouped: BTreeMap<String, Vec<&SkippedSkill>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for s in skipped {
        let key = install_source(s);
        if !grouped.contains_key(&key) {
            order.push(key.clone());
        }
        grouped.entry(key).or_default().push(s);
    }

    for source in &order {
        let Some(group) = grouped.get(source) else {
            continue;
        };
        match group.as_slice() {
            [only] => {
                println!(
                    "  {TEXT}\u{2022}{RESET} {} {DIM}({}){RESET}",
                    sanitize_metadata(&only.name),
                    only.reason
                );
            }
            [first, ..] => {
                let reason = &first.reason;
                let names = group
                    .iter()
                    .map(|s| sanitize_metadata(&s.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("  {TEXT}\u{2022}{RESET} {names} {DIM}({reason}){RESET}");
            }
            [] => continue,
        }
        println!("    {DIM}To update: {TEXT}skills add {source} -g -y{RESET}");
    }
}

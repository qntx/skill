//! CLI command implementations.

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
pub(crate) struct SkippedSkill {
    pub name: String,
    pub reason: String,
    pub source_url: String,
}

/// Whether a lock entry should be skipped (not enough info to check).
pub(crate) const fn should_skip(entry: &skill::lock::SkillLockEntry) -> bool {
    entry.skill_folder_hash.is_empty() || entry.skill_path.is_none()
}

/// Human-readable skip reason for a lock entry.
pub(crate) fn get_skip_reason(entry: &skill::lock::SkillLockEntry) -> String {
    match entry.source_type.as_str() {
        "local" => "Local path".to_owned(),
        "git" => "Git URL (hash tracking not supported)".to_owned(),
        _ if entry.skill_folder_hash.is_empty() => "No version hash available".to_owned(),
        _ if entry.skill_path.is_none() => "No skill path recorded".to_owned(),
        _ => "No version tracking".to_owned(),
    }
}

/// Print skipped skills with manual update instructions.
pub(crate) fn print_skipped_skills(skipped: &[SkippedSkill]) {
    if skipped.is_empty() {
        return;
    }
    println!();
    println!(
        "{DIM}{} skill(s) cannot be checked automatically:{RESET}",
        skipped.len()
    );
    for s in skipped {
        println!(
            "  {TEXT}\u{2022}{RESET} {} {DIM}({}){RESET}",
            s.name, s.reason
        );
        println!(
            "    {DIM}To update: {TEXT}skills add {} -g -y{RESET}",
            s.source_url
        );
    }
}

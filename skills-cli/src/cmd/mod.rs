//! CLI command implementations.

use crate::ui::{DIM, RESET, TEXT};

pub mod add;
pub mod check;
pub mod find;
pub mod init;
pub mod install_lock;
pub mod list;
pub mod remove;
pub mod sync;
pub mod update;

/// A skill that was skipped during check/update (no trackable version info).
pub struct SkippedSkill {
    pub name: String,
    pub reason: String,
    pub source_url: String,
}

/// Whether a lock entry should be skipped (not enough info to check).
pub fn should_skip(entry: &skill::lock::SkillLockEntry) -> bool {
    entry.skill_folder_hash.is_empty() || entry.skill_path.is_none()
}

/// Human-readable skip reason for a lock entry.
pub fn get_skip_reason(entry: &skill::lock::SkillLockEntry) -> String {
    if entry.skill_folder_hash.is_empty() {
        return "No version hash available".to_owned();
    }
    if entry.skill_path.is_none() {
        return "No skill path recorded".to_owned();
    }
    "No version tracking".to_owned()
}

/// Print skipped skills with manual update instructions.
pub fn print_skipped_skills(skipped: &[SkippedSkill]) {
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

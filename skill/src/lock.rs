//! Global skill lock file management.
//!
//! The global lock file lives at `$XDG_STATE_HOME/skills/.skill-lock.json` if
//! `XDG_STATE_HOME` is set, otherwise at `~/.agents/.skill-lock.json`. It
//! tracks installed skills for update checking and telemetry.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SkillError};
use crate::types::AGENTS_DIR;

/// Name of the global lock file.
const LOCK_FILE: &str = ".skill-lock.json";
/// Current lock file format version.
const CURRENT_VERSION: u32 = 3;

/// A single installed skill entry in the global lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockEntry {
    /// Normalized source identifier (e.g. `"owner/repo"`).
    pub source: String,
    /// Provider / source type (e.g. `"github"`, `"well-known"`).
    pub source_type: String,
    /// Original URL used for installation.
    pub source_url: String,
    /// Branch or tag ref used at install time (for ref-aware updates).
    ///
    /// Serialized as `ref` to match the TS reference. `None` means the
    /// installer used the repository default branch.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// Subpath within the source repo, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// GitHub tree SHA for the skill folder.
    pub skill_folder_hash: String,
    /// ISO timestamp of first installation.
    pub installed_at: String,
    /// ISO timestamp of last update.
    pub updated_at: String,
    /// Plugin grouping name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

/// Dismissed prompt tracking.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissedPrompts {
    /// Whether the find-skills prompt was dismissed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find_skills_prompt: Option<bool>,
}

/// The global skill lock file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockFile {
    /// Schema version.
    pub version: u32,
    /// Map of skill name to lock entry.
    pub skills: std::collections::HashMap<String, SkillLockEntry>,
    /// Dismissed prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<DismissedPrompts>,
    /// Last selected agents for installation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_selected_agents: Option<Vec<String>>,
}

impl SkillLockFile {
    /// Create an empty lock file with the current version.
    fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            skills: std::collections::HashMap::new(),
            dismissed: Some(DismissedPrompts::default()),
            last_selected_agents: None,
        }
    }
}

/// Get the path to the global lock file.
///
/// Uses `$XDG_STATE_HOME/skills/.skill-lock.json` if `XDG_STATE_HOME` is set
/// and non-empty (matching the TS reference), otherwise falls back to
/// `~/.agents/.skill-lock.json`.
///
/// # Examples
///
/// ```no_run
/// let path = skill::lock::lock_file_path();
/// assert!(path.ends_with(".skill-lock.json"));
/// ```
#[must_use]
pub fn lock_file_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("skills").join(LOCK_FILE);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(AGENTS_DIR).join(LOCK_FILE)
}

/// Read the global skill lock file.
///
/// Returns an empty structure if the file doesn't exist or has an
/// incompatible version.
///
/// # Errors
///
/// Returns an error on JSON parse failure for valid files.
pub async fn read_skill_lock() -> Result<SkillLockFile> {
    let path = lock_file_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let parsed: SkillLockFile = serde_json::from_str(&content)?;
            if parsed.version < CURRENT_VERSION {
                return Ok(SkillLockFile::empty());
            }
            Ok(parsed)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SkillLockFile::empty()),
        Err(e) => Err(SkillError::io(path, e)),
    }
}

/// Write the global skill lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
///
pub async fn write_skill_lock(lock: &SkillLockFile) -> Result<()> {
    let path = lock_file_path();
    let Some(parent) = path.parent() else {
        return Err(SkillError::io(
            &path,
            std::io::Error::other("lock file path has no parent"),
        ));
    };
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| SkillError::io(parent, e))?;

    let content = serde_json::to_string_pretty(lock)?;

    // Atomic write: write to a temp file in the same directory, then rename.
    // This prevents corruption if the process is interrupted mid-write.
    let tmp_path = parent.join(".skill-lock.tmp");
    tokio::fs::write(&tmp_path, &content)
        .await
        .map_err(|e| SkillError::io(&tmp_path, e))?;
    tokio::fs::rename(&tmp_path, &path)
        .await
        .map_err(|e| SkillError::io(&path, e))
}

/// Input for adding or updating a skill in the global lock file.
///
/// Timestamps are managed automatically: `installed_at` is preserved on
/// update, `updated_at` is always set to now.
#[derive(Debug)]
pub struct AddLockInput<'a> {
    /// Skill name (used as the map key).
    pub name: &'a str,
    /// Normalized source identifier (e.g. `"owner/repo"`).
    pub source: &'a str,
    /// Provider / source type (e.g. `"github"`, `"well-known"`).
    pub source_type: &'a str,
    /// Original URL used for installation.
    pub source_url: &'a str,
    /// Branch or tag ref used at install time, if any.
    pub git_ref: Option<&'a str>,
    /// Subpath within the source repo.
    pub skill_path: Option<&'a str>,
    /// GitHub tree SHA for the skill folder.
    pub skill_folder_hash: &'a str,
    /// Plugin grouping name.
    pub plugin_name: Option<&'a str>,
}

/// Add or update a skill entry in the global lock file.
///
/// # Errors
///
/// Returns an error on I/O or serialization failure.
pub async fn add_skill_to_lock(input: &AddLockInput<'_>) -> Result<()> {
    let mut lock = read_skill_lock().await?;
    let now = crate::util::time::iso8601_now();

    let installed_at = lock
        .skills
        .get(input.name)
        .map_or_else(|| now.clone(), |e| e.installed_at.clone());

    lock.skills.insert(
        input.name.to_owned(),
        SkillLockEntry {
            source: input.source.to_owned(),
            source_type: input.source_type.to_owned(),
            source_url: input.source_url.to_owned(),
            git_ref: input.git_ref.map(String::from),
            skill_path: input.skill_path.map(String::from),
            skill_folder_hash: input.skill_folder_hash.to_owned(),
            installed_at,
            updated_at: now,
            plugin_name: input.plugin_name.map(String::from),
        },
    );

    write_skill_lock(&lock).await
}

/// Remove a skill from the global lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn remove_skill_from_lock(skill_name: &str) -> Result<bool> {
    let mut lock = read_skill_lock().await?;
    let removed = lock.skills.remove(skill_name).is_some();
    if removed {
        write_skill_lock(&lock).await?;
    }
    Ok(removed)
}

/// Get a single skill entry from the lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn get_skill_from_lock(skill_name: &str) -> Result<Option<SkillLockEntry>> {
    let lock = read_skill_lock().await?;
    Ok(lock.skills.get(skill_name).cloned())
}

/// Get all skills from the lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn get_all_locked_skills() -> Result<std::collections::HashMap<String, SkillLockEntry>> {
    let lock = read_skill_lock().await?;
    Ok(lock.skills)
}

/// Check if a prompt has been dismissed.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn is_prompt_dismissed(prompt: &str) -> Result<bool> {
    let lock = read_skill_lock().await?;
    Ok(match prompt {
        "findSkillsPrompt" => lock
            .dismissed
            .as_ref()
            .and_then(|d| d.find_skills_prompt)
            .unwrap_or(false),
        _ => false,
    })
}

/// Dismiss a prompt.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn dismiss_prompt(prompt: &str) -> Result<()> {
    let mut lock = read_skill_lock().await?;
    let dismissed = lock.dismissed.get_or_insert_with(DismissedPrompts::default);
    if prompt == "findSkillsPrompt" {
        dismissed.find_skills_prompt = Some(true);
    }
    write_skill_lock(&lock).await
}

/// Save the last selected agents.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn save_selected_agents(agents: &[String]) -> Result<()> {
    let mut lock = read_skill_lock().await?;
    lock.last_selected_agents = Some(agents.to_vec());
    write_skill_lock(&lock).await
}

/// Get the last selected agents.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn get_last_selected_agents() -> Result<Option<Vec<String>>> {
    let lock = read_skill_lock().await?;
    Ok(lock.last_selected_agents)
}

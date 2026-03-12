//! Global skill lock file management.
//!
//! The global lock file lives at `~/.agents/.skill-lock.json` and tracks
//! installed skills for update checking and telemetry.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::types::AGENTS_DIR;

const LOCK_FILE: &str = ".skill-lock.json";
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
    fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            skills: std::collections::HashMap::new(),
            dismissed: Some(DismissedPrompts::default()),
            last_selected_agents: None,
        }
    }
}

/// Get the path to the global lock file (`~/.agents/.skill-lock.json`).
#[must_use]
pub fn lock_file_path() -> PathBuf {
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
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Write the global skill lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn write_skill_lock(lock: &SkillLockFile) -> Result<()> {
    let path = lock_file_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::io(parent, e))?;
    }
    let content = serde_json::to_string_pretty(lock)?;
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| Error::io(path, e))
}

/// Add or update a skill entry in the global lock file.
///
/// # Errors
///
/// Returns an error on I/O or serialization failure.
pub async fn add_skill_to_lock(
    skill_name: &str,
    source: &str,
    source_type: &str,
    source_url: &str,
    skill_path: Option<&str>,
    skill_folder_hash: &str,
    plugin_name: Option<&str>,
) -> Result<()> {
    let mut lock = read_skill_lock().await?;
    let now = chrono_now();

    let installed_at = lock
        .skills
        .get(skill_name)
        .map_or_else(|| now.clone(), |e| e.installed_at.clone());

    lock.skills.insert(
        skill_name.to_owned(),
        SkillLockEntry {
            source: source.to_owned(),
            source_type: source_type.to_owned(),
            source_url: source_url.to_owned(),
            skill_path: skill_path.map(String::from),
            skill_folder_hash: skill_folder_hash.to_owned(),
            installed_at,
            updated_at: now,
            plugin_name: plugin_name.map(String::from),
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

/// Get a GitHub token from the environment or `gh` CLI.
#[must_use]
pub fn get_github_token() -> Option<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && !token.is_empty()
    {
        return Some(token);
    }
    if let Ok(token) = std::env::var("GH_TOKEN")
        && !token.is_empty()
    {
        return Some(token);
    }
    // Try gh CLI
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_owned())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
}

/// Fetch the tree SHA for a skill folder via the GitHub Trees API.
///
/// # Errors
///
/// Returns an error on network failure.
#[cfg(feature = "network")]
pub async fn fetch_skill_folder_hash(
    owner_repo: &str,
    skill_path: &str,
    token: Option<&str>,
) -> Result<Option<String>> {
    let mut folder_path = skill_path.replace('\\', "/");
    folder_path = folder_path
        .trim_end_matches("/SKILL.md")
        .trim_end_matches("SKILL.md")
        .trim_end_matches('/')
        .to_owned();

    let client = reqwest::Client::new();

    for branch in &["main", "master"] {
        let url =
            format!("https://api.github.com/repos/{owner_repo}/git/trees/{branch}?recursive=1");

        let mut req = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skills-cli-rs");

        if let Some(tok) = token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }

        let resp = match req.send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let data: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };

        if folder_path.is_empty() {
            return Ok(data.get("sha").and_then(|v| v.as_str()).map(String::from));
        }

        if let Some(tree) = data.get("tree").and_then(|v| v.as_array()) {
            for entry in tree {
                let is_tree = entry
                    .get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t == "tree");
                let path_match = entry
                    .get("path")
                    .and_then(|v| v.as_str())
                    .is_some_and(|p| p == folder_path);
                if is_tree && path_match {
                    return Ok(entry.get("sha").and_then(|v| v.as_str()).map(String::from));
                }
            }
        }
    }

    Ok(None)
}

/// Check if a GitHub repository is private.
///
/// # Errors
///
/// Returns an error on network failure.
#[cfg(feature = "network")]
pub async fn is_repo_private(owner: &str, repo: &str) -> Result<Option<bool>> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "skills-cli-rs")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let data: serde_json::Value = resp.json().await?;
    Ok(data.get("private").and_then(serde_json::Value::as_bool))
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Format as pseudo-ISO: good enough for lock file tracking
    format!("{secs}")
}

//! Project-scoped skill lock file management.
//!
//! The local lock file lives at `./skills-lock.json` in the project root
//! and is designed to be committed to version control.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const LOCAL_LOCK_FILE: &str = "skills-lock.json";
const CURRENT_VERSION: u32 = 1;

/// A single skill entry in the project lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkillLockEntry {
    /// Source identifier (e.g. `"owner/repo"`, npm package name).
    pub source: String,
    /// Source type (e.g. `"github"`, `"node_modules"`, `"local"`).
    pub source_type: String,
    /// SHA-256 hash of the skill folder contents.
    pub computed_hash: String,
}

/// The project-scoped lock file structure.
///
/// Uses `BTreeMap` for deterministic JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkillLockFile {
    /// Schema version.
    pub version: u32,
    /// Map of skill name to lock entry (sorted by key).
    pub skills: BTreeMap<String, LocalSkillLockEntry>,
}

impl LocalSkillLockFile {
    const fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            skills: BTreeMap::new(),
        }
    }
}

/// Get the path to the local lock file.
#[must_use]
pub fn local_lock_path(cwd: &Path) -> PathBuf {
    cwd.join(LOCAL_LOCK_FILE)
}

/// Read the project-scoped lock file.
///
/// Returns an empty structure if the file doesn't exist.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn read_local_lock(cwd: &Path) -> Result<LocalSkillLockFile> {
    let path = local_lock_path(cwd);
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => match serde_json::from_str::<LocalSkillLockFile>(&content) {
            Ok(parsed) if parsed.version >= CURRENT_VERSION => Ok(parsed),
            _ => Ok(LocalSkillLockFile::empty()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LocalSkillLockFile::empty()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Write the project-scoped lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn write_local_lock(lock: &LocalSkillLockFile, cwd: &Path) -> Result<()> {
    let path = local_lock_path(cwd);
    let mut content = serde_json::to_string_pretty(lock)?;
    content.push('\n');
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| Error::io(path, e))
}

/// Add or update a skill in the project lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn add_skill_to_local_lock(
    skill_name: &str,
    entry: LocalSkillLockEntry,
    cwd: &Path,
) -> Result<()> {
    let mut lock = read_local_lock(cwd).await?;
    lock.skills.insert(skill_name.to_owned(), entry);
    write_local_lock(&lock, cwd).await
}

/// Remove a skill from the project lock file.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn remove_skill_from_local_lock(skill_name: &str, cwd: &Path) -> Result<bool> {
    let mut lock = read_local_lock(cwd).await?;
    let removed = lock.skills.remove(skill_name).is_some();
    if removed {
        write_local_lock(&lock, cwd).await?;
    }
    Ok(removed)
}

/// Compute a SHA-256 hash from all files in a skill directory.
///
/// Files are sorted by relative path for deterministic output.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn compute_skill_folder_hash(skill_dir: &Path) -> Result<String> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_files(skill_dir, skill_dir, &mut files).await?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_path, content) in &files {
        hasher.update(rel_path.as_bytes());
        hasher.update(content);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

async fn collect_files(
    base: &Path,
    current: &Path,
    results: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    let mut entries = tokio::fs::read_dir(current)
        .await
        .map_err(|e| Error::io(current, e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| Error::io(current, e))?
    {
        let ft = entry.file_type().await.map_err(|e| Error::io(current, e))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if ft.is_dir() {
            if name_str == ".git" || name_str == "node_modules" {
                continue;
            }
            Box::pin(collect_files(base, &entry.path(), results)).await?;
        } else if ft.is_file() {
            let content = tokio::fs::read(entry.path())
                .await
                .map_err(|e| Error::io(entry.path(), e))?;
            let rel = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            results.push((rel, content));
        }
    }

    Ok(())
}

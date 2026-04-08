//! Project-scoped skill lock file management.
//!
//! The local lock file lives at `./skills-lock.json` in the project root
//! and is designed to be committed to version control.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, SkillError};

/// Name of the project-scoped lock file.
const LOCAL_LOCK_FILE: &str = "skills-lock.json";
/// Current lock file format version.
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
    /// Create an empty lock file with the current version.
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
        Err(e) => Err(SkillError::io(path, e)),
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
        .map_err(|e| SkillError::io(path, e))
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

    let hash = hasher.finalize();
    let mut hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        #[allow(
            clippy::let_underscore_must_use,
            reason = "write! to String is infallible"
        )]
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Recursively collect file paths and contents from a directory.
async fn collect_files(
    base: &Path,
    current: &Path,
    results: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    let mut entries = tokio::fs::read_dir(current)
        .await
        .map_err(|e| SkillError::io(current, e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| SkillError::io(current, e))?
    {
        let ft = entry
            .file_type()
            .await
            .map_err(|e| SkillError::io(current, e))?;
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
                .map_err(|e| SkillError::io(entry.path(), e))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_file_serialization_roundtrip() {
        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(
            "my-skill".to_owned(),
            LocalSkillLockEntry {
                source: "owner/repo".to_owned(),
                source_type: "github".to_owned(),
                computed_hash: "abc123".to_owned(),
            },
        );

        let json = serde_json::to_string_pretty(&lock).unwrap();
        let parsed: LocalSkillLockFile = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.skills.len(), 1);
        let entry = parsed.skills.get("my-skill").expect("key exists");
        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.source_type, "github");
        assert_eq!(entry.computed_hash, "abc123");
    }

    #[test]
    fn lock_file_btreemap_deterministic_order() {
        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(
            "z-skill".to_owned(),
            LocalSkillLockEntry {
                source: "z".to_owned(),
                source_type: "github".to_owned(),
                computed_hash: String::new(),
            },
        );
        lock.skills.insert(
            "a-skill".to_owned(),
            LocalSkillLockEntry {
                source: "a".to_owned(),
                source_type: "github".to_owned(),
                computed_hash: String::new(),
            },
        );

        let json = serde_json::to_string(&lock).unwrap();
        let a_pos = json.find("a-skill").unwrap();
        let z_pos = json.find("z-skill").unwrap();
        assert!(a_pos < z_pos, "BTreeMap should produce sorted output");
    }

    #[test]
    fn lock_file_camel_case_keys() {
        let lock = LocalSkillLockFile::empty();
        let json = serde_json::to_string(&lock).unwrap();
        assert!(json.contains("\"version\""));
        assert!(!json.contains("\"Version\""));
    }

    #[tokio::test]
    async fn read_write_lock_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(
            "test".to_owned(),
            LocalSkillLockEntry {
                source: "src".to_owned(),
                source_type: "local".to_owned(),
                computed_hash: "hash".to_owned(),
            },
        );

        write_local_lock(&lock, dir.path()).await.unwrap();
        let read_back = read_local_lock(dir.path()).await.unwrap();

        assert_eq!(read_back.skills.len(), 1);
        assert_eq!(
            read_back.skills.get("test").expect("key exists").source,
            "src"
        );
    }

    #[tokio::test]
    async fn read_missing_lock_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lock = read_local_lock(dir.path()).await.unwrap();
        assert!(lock.skills.is_empty());
        assert_eq!(lock.version, 1);
    }

    #[tokio::test]
    async fn compute_hash_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::write(dir.path().join("SKILL.md"), "# test")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("extra.txt"), "data")
            .await
            .unwrap();

        let hash1 = compute_skill_folder_hash(dir.path()).await.unwrap();
        let hash2 = compute_skill_folder_hash(dir.path()).await.unwrap();

        assert_eq!(hash1, hash2, "same content should produce same hash");
        assert_eq!(hash1.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[tokio::test]
    async fn compute_hash_changes_with_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::write(dir.path().join("SKILL.md"), "# v1")
            .await
            .unwrap();
        let hash1 = compute_skill_folder_hash(dir.path()).await.unwrap();

        tokio::fs::write(dir.path().join("SKILL.md"), "# v2")
            .await
            .unwrap();
        let hash2 = compute_skill_folder_hash(dir.path()).await.unwrap();

        assert_ne!(
            hash1, hash2,
            "different content should produce different hash"
        );
    }

    #[tokio::test]
    async fn add_and_remove_lock_entry() {
        let dir = tempfile::tempdir().expect("tempdir");

        add_skill_to_local_lock(
            "foo",
            LocalSkillLockEntry {
                source: "s".to_owned(),
                source_type: "t".to_owned(),
                computed_hash: "h".to_owned(),
            },
            dir.path(),
        )
        .await
        .unwrap();

        let lock = read_local_lock(dir.path()).await.unwrap();
        assert!(lock.skills.contains_key("foo"));

        let removed = remove_skill_from_local_lock("foo", dir.path())
            .await
            .unwrap();
        assert!(removed);

        let after_remove = read_local_lock(dir.path()).await.unwrap();
        assert!(after_remove.skills.is_empty());
    }

    #[tokio::test]
    async fn remove_nonexistent_returns_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let removed = remove_skill_from_local_lock("nope", dir.path())
            .await
            .unwrap();
        assert!(!removed);
    }
}

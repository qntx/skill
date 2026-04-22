//! Blob-based fast install for GitHub sources.
//!
//! Avoids a full `git clone` by pulling only the files inside the target
//! skill folder through the GitHub Trees API + `raw.githubusercontent.com`.
//! Mirrors the TS `blob.ts` module.
//!
//! The public entry point is [`try_blob_install`]: it either materializes
//! the skill tree into a [`TempDir`] (so the caller can treat it identically
//! to a cloned repo), or returns `Ok(None)` to signal the caller should fall
//! back to `git.rs::clone_repo`.
//!
//! **All errors are soft-failures.** Any network, auth, or parse error
//! yields `Ok(None)` rather than propagating, keeping the blob path a pure
//! optimization that never breaks a working `git clone` path.

#![cfg(feature = "network")]

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use tempfile::TempDir;

use crate::error::{Result, SkillError};

/// Network timeout for the Trees API call (per ref candidate).
const TREE_TIMEOUT: Duration = Duration::from_secs(15);
/// Network timeout for each raw blob download.
const BLOB_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound on number of files we'll download before giving up.
///
/// Guards against accidental whole-repo pulls when a user passes an
/// empty/top-level subpath on a huge registry. `git clone --depth=1` is
/// cheaper at that point.
const MAX_BLOB_COUNT: usize = 2048;
/// Max single-blob size. GitHub raw serves arbitrary sizes but LFS pointers
/// and text skills are <1 MiB; cap to prevent runaway downloads.
const MAX_BLOB_BYTES: u64 = 10 * 1024 * 1024;

/// Attempt a blob-based install for a GitHub repository.
///
/// # Parameters
///
/// - `owner_repo` — canonical `owner/repo` identifier (no `.git`).
/// - `subpath` — optional directory within the repo to install from. Empty
///   or `None` means the repository root.
/// - `git_ref` — optional branch or tag; defaults to `HEAD` / `main` / `master`.
/// - `token` — optional GitHub token for authenticated requests.
///
/// # Returns
///
/// - `Ok(Some(tempdir))` — download succeeded; the tempdir contains the
///   skill folder rooted at its top level.
/// - `Ok(None)`          — the blob path is not usable for this source
///   (truncated tree, empty match set, network error, HTTP 4xx/5xx).
///   The caller should fall back to `git clone`.
///
/// # Errors
///
/// Only surfaces errors that the caller cannot meaningfully recover from
/// (local `std::io::Error` when writing files to the tempdir).
pub async fn try_blob_install(
    owner_repo: &str,
    subpath: Option<&str>,
    git_ref: Option<&str>,
    token: Option<&str>,
) -> Result<Option<TempDir>> {
    let prefix = normalize_prefix(subpath);

    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "blob install: failed to build client");
            return Ok(None);
        }
    };

    let Some((tree, resolved_ref)) = fetch_tree(&client, owner_repo, git_ref, token).await? else {
        return Ok(None);
    };

    if tree.truncated {
        tracing::debug!(owner_repo, "blob install: tree truncated, falling back");
        return Ok(None);
    }

    let mut files: Vec<&TreeEntry> = tree
        .tree
        .iter()
        .filter(|e| e.entry_type == "blob" && entry_in_prefix(&e.path, &prefix))
        .collect();

    if files.is_empty() {
        tracing::debug!(owner_repo, prefix, "blob install: no matching files");
        return Ok(None);
    }
    if files.len() > MAX_BLOB_COUNT {
        tracing::debug!(
            owner_repo,
            count = files.len(),
            "blob install: too many files, falling back"
        );
        return Ok(None);
    }

    // Deterministic order simplifies debugging and ensures directories
    // are created before their children on any filesystem.
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let temp_dir = TempDir::new().map_err(|e| SkillError::io(PathBuf::from("/tmp"), e))?;
    let root = temp_dir.path();

    for entry in files {
        let dest = root.join(&entry.path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SkillError::io(parent, e))?;
        }
        match download_raw(&client, owner_repo, &resolved_ref, &entry.path, token).await {
            Ok(bytes) => {
                tokio::fs::write(&dest, &bytes)
                    .await
                    .map_err(|e| SkillError::io(&dest, e))?;
            }
            Err(e) => {
                tracing::debug!(
                    owner_repo,
                    path = entry.path,
                    error = %e,
                    "blob install: download failed, falling back"
                );
                return Ok(None);
            }
        }
    }

    Ok(Some(temp_dir))
}

/// Normalize a user-provided subpath into a directory prefix ending in `/`
/// (or the empty string for repo-root installs).
fn normalize_prefix(subpath: Option<&str>) -> String {
    let Some(s) = subpath else {
        return String::new();
    };
    let s = s.replace('\\', "/");
    let trimmed = s.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

/// Whether a tree entry path falls under the requested prefix.
fn entry_in_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty() || path.starts_with(prefix)
}

/// Shape of a GitHub Git Trees API response.
#[derive(Deserialize)]
struct TreeResponse {
    /// Full tree entries (with `recursive=1`).
    tree: Vec<TreeEntry>,
    /// GitHub sets this when the tree exceeds API size limits.
    #[serde(default)]
    truncated: bool,
}

/// One node in a GitHub Git Trees response.
#[derive(Deserialize)]
struct TreeEntry {
    /// Path relative to the repository root.
    path: String,
    /// `"blob"` for files, `"tree"` for directories.
    #[serde(rename = "type")]
    entry_type: String,
}

/// Fetch the recursive tree for a repository, trying `git_ref` first then
/// the standard fallbacks.
async fn fetch_tree(
    client: &reqwest::Client,
    owner_repo: &str,
    git_ref: Option<&str>,
    token: Option<&str>,
) -> Result<Option<(TreeResponse, String)>> {
    let mut candidates: Vec<String> = Vec::with_capacity(4);
    if let Some(r) = git_ref
        && !r.trim().is_empty()
    {
        candidates.push(r.to_owned());
    }
    for default in ["HEAD", "main", "master"] {
        if !candidates.iter().any(|c| c == default) {
            candidates.push(default.to_owned());
        }
    }

    for branch in candidates {
        let url =
            format!("https://api.github.com/repos/{owner_repo}/git/trees/{branch}?recursive=1");
        let mut req = client
            .get(&url)
            .timeout(TREE_TIMEOUT)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skills-cli-rs");
        if let Some(tok) = token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }

        let resp = match req.send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let Ok(data) = resp.json::<TreeResponse>().await else {
            continue;
        };
        return Ok(Some((data, branch)));
    }

    Ok(None)
}

/// Download one file via `raw.githubusercontent.com`.
async fn download_raw(
    client: &reqwest::Client,
    owner_repo: &str,
    reference: &str,
    path: &str,
    token: Option<&str>,
) -> Result<Vec<u8>> {
    let url = format!("https://raw.githubusercontent.com/{owner_repo}/{reference}/{path}");
    let mut req = client
        .get(&url)
        .timeout(BLOB_TIMEOUT)
        .header("User-Agent", "skills-cli-rs");
    if let Some(tok) = token {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(SkillError::io(
            PathBuf::from(path),
            std::io::Error::other(format!("HTTP {}", resp.status())),
        ));
    }

    if let Some(len) = resp.content_length()
        && len > MAX_BLOB_BYTES
    {
        return Err(SkillError::io(
            PathBuf::from(path),
            std::io::Error::other(format!("blob too large: {len} bytes")),
        ));
    }

    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > MAX_BLOB_BYTES {
        return Err(SkillError::io(
            PathBuf::from(path),
            std::io::Error::other(format!("blob too large: {} bytes", bytes.len())),
        ));
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_prefix_empty_cases() {
        assert_eq!(normalize_prefix(None), "");
        assert_eq!(normalize_prefix(Some("")), "");
        assert_eq!(normalize_prefix(Some("/")), "");
        assert_eq!(normalize_prefix(Some("//")), "");
    }

    #[test]
    fn normalize_prefix_appends_trailing_slash() {
        assert_eq!(normalize_prefix(Some("skills/foo")), "skills/foo/");
        assert_eq!(normalize_prefix(Some("/skills/foo/")), "skills/foo/");
        assert_eq!(normalize_prefix(Some("skills\\foo")), "skills/foo/");
    }

    #[test]
    fn entry_in_prefix_root_matches_all() {
        assert!(entry_in_prefix("anything.md", ""));
        assert!(entry_in_prefix("a/b/c.md", ""));
    }

    #[test]
    fn entry_in_prefix_respects_boundary() {
        assert!(entry_in_prefix("skills/foo/SKILL.md", "skills/foo/"));
        assert!(!entry_in_prefix("skills/foobar/SKILL.md", "skills/foo/"));
        assert!(!entry_in_prefix("other/file.md", "skills/foo/"));
    }
}

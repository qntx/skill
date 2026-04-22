//! GitHub API utilities.
//!
//! Provides authentication token discovery and REST API access for
//! repository metadata and git tree hashing.

use std::path::PathBuf;

use crate::error::{Result, SkillError};

/// Discover a GitHub token from the environment (`GITHUB_TOKEN` / `GH_TOKEN`)
/// or by shelling out to the `gh` CLI.
#[must_use]
pub fn discover_token() -> Option<String> {
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

/// Typed response from the GitHub Git Trees API (`GET /repos/:owner/:repo/git/trees/:sha`).
#[cfg(feature = "network")]
#[derive(serde::Deserialize)]
struct GitTreeResponse {
    /// Root tree SHA (used when `folder_path` is empty).
    sha: Option<String>,
    /// Flat list of tree entries (recursive mode).
    #[serde(default)]
    tree: Vec<GitTreeEntry>,
}

/// A single entry within a GitHub git tree listing.
#[cfg(feature = "network")]
#[derive(serde::Deserialize)]
struct GitTreeEntry {
    /// Relative path of this entry within the repository.
    path: String,
    /// Object SHA-1 hash.
    sha: Option<String>,
    /// Git object type (`"blob"`, `"tree"`, or `"commit"`).
    #[serde(rename = "type")]
    entry_type: String,
}

/// Fetch the tree SHA for a skill folder via the GitHub Trees API.
///
/// Tries `git_ref` first (if provided), then falls back to `HEAD` → `main` →
/// `master`. This matches the TS reference which accepts an optional `ref`
/// for ref-aware updates.
///
/// # Errors
///
/// Returns an error on network failure.
#[cfg(feature = "network")]
pub async fn fetch_skill_folder_hash(
    owner_repo: &str,
    skill_path: &str,
    token: Option<&str>,
    git_ref: Option<&str>,
) -> Result<Option<String>> {
    let folder_path = skill_path
        .replace('\\', "/")
        .trim_end_matches("/SKILL.md")
        .trim_end_matches("SKILL.md")
        .trim_end_matches('/')
        .to_owned();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            SkillError::io(
                PathBuf::from("<network>"),
                std::io::Error::other(e.to_string()),
            )
        })?;

    // Build candidate ref list without duplicates, preserving order.
    let mut candidates: Vec<&str> = Vec::with_capacity(4);
    if let Some(r) = git_ref
        && !r.trim().is_empty()
    {
        candidates.push(r);
    }
    for default in ["HEAD", "main", "master"] {
        if !candidates.contains(&default) {
            candidates.push(default);
        }
    }

    for branch in candidates {
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

        let data: GitTreeResponse = match resp.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };

        if folder_path.is_empty() {
            return Ok(data.sha);
        }

        let found = data
            .tree
            .iter()
            .find(|e| e.entry_type == "tree" && e.path == folder_path)
            .and_then(|e| e.sha.clone());

        if found.is_some() {
            return Ok(found);
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
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

//! Git operations for cloning skill repositories.
//!
//! Uses the system `git` command with a 60-second timeout, matching the
//! the TS `simple-git` reference implementation.

use std::path::PathBuf;

use tempfile::TempDir;

use crate::error::{Result, SkillError};

/// Clone timeout matching the TS `CLONE_TIMEOUT_MS`.
const CLONE_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(1);

/// Clone a git repository to a temporary directory.
///
/// Returns the [`TempDir`] which will be cleaned up on drop.
///
/// # Errors
///
/// Returns [`SkillError::GitClone`] with a human-readable message on timeout,
/// authentication failure, or any other `git` process error.
pub async fn clone_repo(url: &str, git_ref: Option<&str>) -> Result<TempDir> {
    let temp_dir = TempDir::new().map_err(|e| SkillError::io(PathBuf::from("/tmp"), e))?;

    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--single-branch");

    if let Some(r) = git_ref {
        cmd.arg("--branch").arg(r);
    }

    cmd.arg(url).arg(temp_dir.path());

    // Suppress interactive credential prompts (matching TS GIT_TERMINAL_PROMPT=0).
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    // Skills are text files (HTML/MD/JSON) and never LFS-tracked. Registry
    // repos frequently track unrelated large media (test fixtures, demos,
    // docs videos) via LFS. Downloading those during clone adds tens or
    // hundreds of MB of bandwidth for files the installer never reads, and
    // is the main reason `skills add` times out against larger registries
    // (e.g. heygen-com/hyperframes; upstream report #300).
    cmd.env("GIT_LFS_SKIP_SMUDGE", "1");

    let output = match tokio::time::timeout(CLONE_TIMEOUT, cmd.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(SkillError::GitClone {
                url: url.to_owned(),
                message: format!("failed to run git: {e}"),
            });
        }
        Err(_elapsed) => {
            return Err(SkillError::GitClone {
                url: url.to_owned(),
                message: concat!(
                    "Clone timed out after 60s. This often happens with private repos ",
                    "that require authentication.\n",
                    "  Ensure you have access and your SSH keys or credentials are configured:\n",
                    "  - For SSH: ssh-add -l (to check loaded keys)\n",
                    "  - For HTTPS: gh auth status (if using GitHub CLI)",
                )
                .to_owned(),
            });
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let is_auth = stderr.contains("Authentication failed")
            || stderr.contains("could not read Username")
            || stderr.contains("Permission denied")
            || stderr.contains("Repository not found");

        let message = if is_auth {
            format!(
                "Authentication failed for {url}.\n\
                 \x20 - For private repos, ensure you have access\n\
                 \x20 - For SSH: Check your keys with 'ssh -T git@github.com'\n\
                 \x20 - For HTTPS: Run 'gh auth login' or configure git credentials"
            )
        } else {
            stderr
        };

        return Err(SkillError::GitClone {
            url: url.to_owned(),
            message,
        });
    }

    Ok(temp_dir)
}

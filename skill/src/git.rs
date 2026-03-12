//! Git operations for cloning skill repositories.
//!
//! Uses the system `git` command (like the `TypeScript` `simple-git` reference)
//! for maximum compatibility.

use std::path::PathBuf;

use tempfile::TempDir;

use crate::error::{Error, Result};

/// Clone a git repository to a temporary directory.
///
/// Returns the [`TempDir`] which will be cleaned up on drop.
///
/// # Errors
///
/// Returns an error if `git clone` fails.
pub async fn clone_repo(url: &str, git_ref: Option<&str>) -> Result<TempDir> {
    let temp_dir = TempDir::new().map_err(|e| Error::io(PathBuf::from("/tmp"), e))?;

    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--single-branch");

    if let Some(r) = git_ref {
        cmd.arg("--branch").arg(r);
    }

    cmd.arg(url).arg(temp_dir.path());

    let output = cmd.output().await.map_err(|e| Error::GitClone {
        url: url.to_owned(),
        message: format!("failed to run git: {e}"),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitClone {
            url: url.to_owned(),
            message: stderr.to_string(),
        });
    }

    Ok(temp_dir)
}

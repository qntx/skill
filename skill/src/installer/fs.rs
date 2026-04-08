//! Filesystem operations for skill installation (copy, symlink, cleanup).

use std::path::{Path, PathBuf};

use crate::error::{Result, SkillError};

/// Files to exclude when copying skill directories.
const EXCLUDE_FILES: &[&str] = &["metadata.json"];
/// Directories to exclude when copying skill directories.
const EXCLUDE_DIRS: &[&str] = &[".git"];

/// Remove and recreate a directory.
pub(super) async fn clean_and_create(path: &Path) -> Result<()> {
    drop(tokio::fs::remove_dir_all(path).await);
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| SkillError::io(path, e))
}

/// Recursively copy a directory, excluding metadata and hidden files.
pub(super) async fn copy_directory(src: &Path, dest: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| SkillError::io(dest, e))?;

    let mut entries = tokio::fs::read_dir(src)
        .await
        .map_err(|e| SkillError::io(src, e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| SkillError::io(src, e))?
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        let ft = entry
            .file_type()
            .await
            .map_err(|e| SkillError::io(src, e))?;

        if ft.is_dir() {
            if EXCLUDE_DIRS.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let sub_dest = dest.join(&name);
            Box::pin(copy_directory(&entry.path(), &sub_dest)).await?;
        } else if ft.is_symlink() {
            // Dereference symlinks: copy the target file content, matching
            // the TS `cp(src, dest, { dereference: true })` behavior.
            // Skip broken symlinks that can't be resolved.
            if EXCLUDE_FILES.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let src_path = entry.path();
            let dest_file = dest.join(&name);
            // Follow the symlink chain via metadata (not symlink_metadata)
            match tokio::fs::metadata(&src_path).await {
                Ok(meta) if meta.is_dir() => {
                    Box::pin(copy_directory(&src_path, &dest_file)).await?;
                }
                Ok(_) => {
                    drop(tokio::fs::copy(&src_path, &dest_file).await);
                }
                Err(_) => {
                    tracing::warn!("Skipping broken symlink: {}", src_path.display());
                }
            }
        } else {
            if EXCLUDE_FILES.contains(&name_str.as_ref()) || name_str.starts_with('_') {
                continue;
            }
            let dest_file = dest.join(&name);
            tokio::fs::copy(entry.path(), &dest_file)
                .await
                .map_err(|e| SkillError::io(&dest_file, e))?;
        }
    }

    Ok(())
}

/// Resolve a path's parent directory through symlinks, keeping the final
/// component.  This handles the case where a parent directory (e.g.
/// `~/.claude/skills`) is itself a symlink to another location (e.g.
/// `~/.agents/skills`).  Computing relative paths from the un-resolved
/// symlink path would produce broken symlinks.
async fn resolve_parent_symlinks(path: &Path) -> PathBuf {
    let resolved = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(dir) = resolved.parent() else {
        return resolved;
    };
    let base = resolved.file_name().unwrap_or_default().to_os_string();
    tokio::fs::canonicalize(dir)
        .await
        .map_or(resolved, |real_dir| real_dir.join(base))
}

/// Create a symlink (or junction on Windows). Returns `true` on success.
///
/// Mirrors the Vercel TS `createSymlink` logic:
///   1. Resolve both paths through real path to detect same-location.
///   2. Resolve parent symlinks to avoid broken relative links.
///   3. Remove stale symlinks / directories before creating.
///   4. Use relative paths on unix, junctions on Windows.
pub(super) async fn create_symlink(target: &Path, link_path: &Path) -> bool {
    let resolved_target = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    let resolved_link = std::path::absolute(link_path).unwrap_or_else(|_| link_path.to_path_buf());

    // Check if both resolve to the same real path (skip creating symlink).
    let real_target = tokio::fs::canonicalize(&resolved_target)
        .await
        .unwrap_or_else(|_| resolved_target.clone());
    let real_link = tokio::fs::canonicalize(&resolved_link)
        .await
        .unwrap_or_else(|_| resolved_link.clone());
    if real_target == real_link {
        return true;
    }

    // Also check with parent symlinks resolved.
    let real_target_parent = resolve_parent_symlinks(target).await;
    let real_link_parent = resolve_parent_symlinks(link_path).await;
    if real_target_parent == real_link_parent {
        return true;
    }

    // Handle existing entry at link_path.
    if let Ok(meta) = tokio::fs::symlink_metadata(link_path).await {
        if meta.is_symlink() {
            #[cfg(unix)]
            if let Ok(existing) = tokio::fs::read_link(link_path).await {
                let existing_abs = if existing.is_relative() {
                    link_path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(&existing)
                } else {
                    existing
                };
                let existing_resolved = std::path::absolute(&existing_abs).unwrap_or(existing_abs);
                if existing_resolved == resolved_target {
                    return true;
                }
            }
            drop(tokio::fs::remove_file(link_path).await);
        } else {
            drop(tokio::fs::remove_dir_all(link_path).await);
        }
    } else {
        // ELOOP (circular symlink) or ENOENT — try force-remove just in case.
        drop(tokio::fs::remove_file(link_path).await);
    }

    // Ensure parent directory exists.
    if let Some(parent) = link_path.parent()
        && tokio::fs::create_dir_all(parent).await.is_err()
    {
        return false;
    }

    #[cfg(unix)]
    {
        // Use a relative symlink target, computed from the real (resolved)
        // parent of the link path, matching the TS implementation.
        let real_link_dir =
            resolve_parent_symlinks(link_path.parent().unwrap_or_else(|| Path::new("."))).await;
        let rel =
            pathdiff::diff_paths(target, &real_link_dir).unwrap_or_else(|| target.to_path_buf());
        tokio::fs::symlink(&rel, link_path).await.is_ok()
    }

    #[cfg(windows)]
    {
        let target = target.to_path_buf();
        let link = link_path.to_path_buf();
        tokio::task::spawn_blocking(move || junction::create(&target, &link).is_ok())
            .await
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

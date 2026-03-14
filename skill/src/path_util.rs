//! Shared path utilities for normalization and safety checks.
//!
//! Consolidates `lexical_normalize`, `strip_unc_prefix`, and
//! `normalize_path` which were previously duplicated across
//! `skills.rs`, `installer.rs`, and `plugin_manifest.rs`.

use std::path::{Component, Path, PathBuf};

/// Lexical path normalization: resolve `.` and `..` without filesystem access.
///
/// Preserves root / prefix components — `..` past the root is clamped rather
/// than silently dropped, so absolute paths always stay absolute.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                match components.last() {
                    // Never pop root / prefix — clamp at filesystem root
                    Some(Component::RootDir | Component::Prefix(_)) | None => {}
                    Some(Component::ParentDir) => {
                        // Already unresolvable relative `..`, keep stacking
                        components.push(comp);
                    }
                    _ => {
                        components.pop();
                    }
                }
            }
            other => components.push(other),
        }
    }
    if components.is_empty() {
        return PathBuf::from(".");
    }
    components.iter().collect()
}

/// Strip the `\\?\` UNC prefix that Windows `canonicalize` produces.
///
/// This prefix breaks `starts_with` comparisons between canonicalized
/// and non-canonicalized paths.
#[cfg(windows)]
#[must_use]
pub fn strip_unc(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    s.strip_prefix("\\\\?\\").map_or(path, PathBuf::from)
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
#[must_use]
pub fn strip_unc(path: PathBuf) -> PathBuf {
    path
}

/// Best-effort path normalization with filesystem access.
///
/// Tries `canonicalize` first (resolves symlinks), falls back to
/// `std::path::absolute`, then lexical normalization. Always strips
/// UNC prefix on Windows.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| lexical_normalize(path));
    strip_unc(resolved)
}

/// Lightweight normalization without filesystem access.
///
/// Uses `std::path::absolute` (no symlink resolution) + lexical
/// normalization. Suitable for paths that may not exist yet.
#[must_use]
pub fn normalize_absolute(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    lexical_normalize(&absolute)
}

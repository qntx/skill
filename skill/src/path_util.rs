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
                    Some(Component::RootDir | Component::Prefix(_)) => {}
                    // Stack is empty (relative path) or previous is also `..`
                    // — keep the `..` so relative traversals are preserved
                    None | Some(Component::ParentDir) => {
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
pub const fn strip_unc(path: PathBuf) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_resolves_dot_and_dotdot() {
        let p = lexical_normalize(Path::new("a/b/../c/./d"));
        assert_eq!(p, PathBuf::from("a/c/d"));
    }

    #[test]
    fn lexical_normalize_empty_path() {
        let p = lexical_normalize(Path::new(""));
        assert_eq!(p, PathBuf::from("."));
    }

    #[test]
    fn lexical_normalize_only_dots() {
        let p = lexical_normalize(Path::new("."));
        assert_eq!(p, PathBuf::from("."));
    }

    #[test]
    fn lexical_normalize_relative_leading_dotdot_preserved() {
        let p = lexical_normalize(Path::new("../../a/b"));
        // Leading .. on a relative path can't be resolved — must be kept
        assert_eq!(p, PathBuf::from("../../a/b"));
    }

    #[test]
    fn lexical_normalize_relative_dotdot_beyond_components() {
        // a/../../b → a/.. resolves to empty, ../b remains
        let p = lexical_normalize(Path::new("a/../../b"));
        assert_eq!(p, PathBuf::from("../b"));
    }

    #[cfg(unix)]
    #[test]
    fn lexical_normalize_absolute_stays_absolute() {
        let p = lexical_normalize(Path::new("/tmp/repo/../../etc/passwd"));
        assert_eq!(p, PathBuf::from("/etc/passwd"));
    }

    #[cfg(unix)]
    #[test]
    fn lexical_normalize_absolute_dotdot_past_root_clamps() {
        let p = lexical_normalize(Path::new("/../../../etc"));
        // `..` past root is clamped — absolute path stays absolute
        assert_eq!(p, PathBuf::from("/etc"));
    }

    #[cfg(unix)]
    #[test]
    fn lexical_normalize_root_only() {
        let p = lexical_normalize(Path::new("/"));
        assert_eq!(p, PathBuf::from("/"));
    }

    #[test]
    fn sanitize_subpath_dotdot_blocked() {
        // Verify normalize_absolute doesn't help bypass is_subpath_safe
        let base = Path::new("/tmp/repo");
        let target = base.join("../../etc/passwd");
        let norm_base = normalize_absolute(base);
        let norm_target = normalize_absolute(&target);
        assert!(
            !norm_target.starts_with(&norm_base),
            "traversal must not pass: base={norm_base:?} target={norm_target:?}"
        );
    }
}

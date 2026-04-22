//! Pure string-formatting helpers for paths, lists, and casing.
//!
//! Every function here is deterministic and allocation-conscious; none of
//! them touch the terminal or the filesystem other than reading the user's
//! home directory for path shortening.

use std::path::Path;

/// Shorten a path relative to a given `cwd`.
///
/// Priority: project-relative (`./…`) first, home-relative (`~/…`) second,
/// absolute display as a last resort.
#[must_use]
pub(crate) fn shorten_path_with_cwd(path: &Path, cwd: &Path) -> String {
    if let Ok(suffix) = path.strip_prefix(cwd) {
        return if suffix.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            format!(".{}{}", std::path::MAIN_SEPARATOR, suffix.display())
        };
    }
    if let Some(home) = dirs::home_dir()
        && let Ok(suffix) = path.strip_prefix(&home)
    {
        return if suffix.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, suffix.display())
        };
    }
    path.display().to_string()
}

/// Convert `kebab-case` to `Title Case`.
#[must_use]
pub(crate) fn kebab_to_title(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |first| {
                let upper: String = first.to_uppercase().collect();
                upper + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format a list with a default truncation threshold of 5.
///
/// Mirrors the TypeScript `formatList(items, maxShow = 5)` behaviour:
/// items are joined with `, ` and anything beyond 5 items is replaced
/// with `+N more`.
#[must_use]
pub(crate) fn format_list(items: &[String]) -> String {
    format_list_max(items, 5)
}

/// Format with a custom truncation threshold.
#[must_use]
fn format_list_max(items: &[String], max_show: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if items.len() <= max_show {
        return items.join(", ");
    }
    let shown = items.get(..max_show).unwrap_or(items);
    let remaining = items.len().saturating_sub(max_show);
    format!("{} +{remaining} more", shown.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kebab_to_title_single_word() {
        assert_eq!(kebab_to_title("cursor"), "Cursor");
    }

    #[test]
    fn test_kebab_to_title_multi_word() {
        assert_eq!(kebab_to_title("claude-code"), "Claude Code");
    }

    #[test]
    fn test_kebab_to_title_empty() {
        assert_eq!(kebab_to_title(""), "");
    }

    #[test]
    fn test_format_list_empty() {
        assert_eq!(format_list(&[]), "");
    }

    #[test]
    fn test_format_list_under_threshold() {
        let items = vec!["a".into(), "b".into()];
        assert_eq!(format_list(&items), "a, b");
    }

    #[test]
    fn test_format_list_over_threshold_truncates() {
        let items: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        assert_eq!(format_list(&items), "0, 1, 2, 3, 4 +3 more");
    }

    #[test]
    fn test_format_list_max_custom_threshold() {
        let items = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(format_list_max(&items, 1), "a +2 more");
    }

    #[test]
    fn test_shorten_path_matches_cwd_root() {
        let cwd = Path::new("/project");
        assert_eq!(shorten_path_with_cwd(cwd, cwd), ".");
    }

    #[test]
    #[cfg(unix)]
    fn test_shorten_path_matches_cwd_child() {
        let cwd = Path::new("/project");
        let child = Path::new("/project/skills/foo");
        assert_eq!(shorten_path_with_cwd(child, cwd), "./skills/foo");
    }
}

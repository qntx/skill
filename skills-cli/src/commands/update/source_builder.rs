//! Reconstruct `skills add` arguments from a lock entry.
//!
//! Pure string-manipulation helpers. Extracted so they can be unit-tested
//! without touching the network or the filesystem.

use skill::local_lock::LocalSkillLockEntry;
use skill::lock::SkillLockEntry;

/// Append `#ref` to `source_url` when `git_ref` is set.
///
/// Matches the TypeScript `formatSourceInput` helper.
pub(super) fn format_source_input(source_url: &str, git_ref: Option<&str>) -> String {
    git_ref.map_or_else(|| source_url.to_owned(), |r| format!("{source_url}#{r}"))
}

/// Build the install source for a global lock entry.
///
/// Uses shorthand `owner/repo/path` + optional `#ref` so the resolver can
/// apply subpath filtering without guessing a branch name. Mirrors TS
/// `buildUpdateInstallSource`.
pub(super) fn build_global_source(entry: &SkillLockEntry) -> String {
    let Some(skill_path) = entry.skill_path.as_deref() else {
        return format_source_input(&entry.source_url, entry.git_ref.as_deref());
    };

    let folder = strip_skill_md_suffix(skill_path);
    let folder = folder.trim_end_matches('/');

    let mut install = if folder.is_empty() {
        entry.source.clone()
    } else {
        format!("{}/{folder}", entry.source)
    };
    if let Some(r) = entry.git_ref.as_deref() {
        install.push('#');
        install.push_str(r);
    }
    install
}

/// Build the install source for a project lock entry.
///
/// Project entries only carry `source` + `ref` — there is no subpath to
/// recover, so we just compose them.
pub(super) fn build_project_source(entry: &LocalSkillLockEntry) -> String {
    format_source_input(&entry.source, entry.git_ref.as_deref())
}

/// Strip a trailing `SKILL.md` (with optional leading slash) from a path.
fn strip_skill_md_suffix(skill_path: &str) -> String {
    if let Some(rest) = skill_path.strip_suffix("/SKILL.md") {
        return rest.to_owned();
    }
    if let Some(rest) = skill_path.strip_suffix("SKILL.md") {
        return rest.to_owned();
    }
    skill_path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_global(skill_path: Option<&str>, git_ref: Option<&str>) -> SkillLockEntry {
        SkillLockEntry {
            source: "owner/repo".to_owned(),
            source_type: "github".to_owned(),
            source_url: "https://github.com/owner/repo.git".to_owned(),
            git_ref: git_ref.map(String::from),
            skill_path: skill_path.map(String::from),
            skill_folder_hash: "abc".to_owned(),
            installed_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            plugin_name: None,
        }
    }

    #[test]
    fn test_format_source_input_no_ref_returns_url_verbatim() {
        assert_eq!(
            format_source_input("https://github.com/a/b.git", None),
            "https://github.com/a/b.git"
        );
    }

    #[test]
    fn test_format_source_input_appends_hash_ref() {
        assert_eq!(
            format_source_input("https://github.com/a/b.git", Some("v2")),
            "https://github.com/a/b.git#v2"
        );
    }

    #[test]
    fn test_build_global_source_no_path_returns_source_url() {
        let e = sample_global(None, None);
        assert_eq!(build_global_source(&e), e.source_url);
    }

    #[test]
    fn test_build_global_source_trims_skill_md_suffix() {
        let e = sample_global(Some("skills/foo/SKILL.md"), None);
        assert_eq!(build_global_source(&e), "owner/repo/skills/foo");
    }

    #[test]
    fn test_build_global_source_combines_path_and_ref() {
        let e = sample_global(Some("skills/bar/"), Some("main"));
        assert_eq!(build_global_source(&e), "owner/repo/skills/bar#main");
    }

    #[test]
    fn test_build_global_source_trailing_skill_md_without_slash() {
        let e = sample_global(Some("SKILL.md"), None);
        assert_eq!(build_global_source(&e), "owner/repo");
    }

    #[test]
    fn test_build_project_source_with_ref() {
        let e = LocalSkillLockEntry {
            source: "owner/repo".to_owned(),
            git_ref: Some("release".to_owned()),
            source_type: "github".to_owned(),
            computed_hash: String::new(),
        };
        assert_eq!(build_project_source(&e), "owner/repo#release");
    }

    #[test]
    fn test_build_project_source_no_ref() {
        let e = LocalSkillLockEntry {
            source: "pkg-a".to_owned(),
            git_ref: None,
            source_type: "github".to_owned(),
            computed_hash: String::new(),
        };
        assert_eq!(build_project_source(&e), "pkg-a");
    }
}

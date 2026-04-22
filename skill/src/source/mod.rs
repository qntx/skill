//! Source string parsing.
//!
//! Converts user-provided source strings (GitHub shorthand, URLs, local
//! paths, etc.) into a structured [`ParsedSource`].
//!
//! Internal layout:
//! - [`regex`]    — compiled regex patterns and classification helpers
//! - [`fragment`] — `#ref[@filter]` parsing for ref-aware installs
//!
//! The public API is this module's `parse_source`, `get_owner_repo`,
//! `parse_owner_repo`, and `sanitize_subpath`.

mod fragment;
mod regex;

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use ::regex::Regex;

use self::fragment::FragmentRef;
use self::regex::{
    at_skill_re, github_repo_re, github_tree_re, github_tree_with_path_re, gitlab_repo_re,
    gitlab_tree_re, gitlab_tree_with_path_re, is_local_path, is_well_known_url, shorthand_re,
};
use crate::error::{Result, SkillError};
use crate::types::{ParsedSource, SourceType};

/// Source aliases mapping common shorthands to canonical sources.
static SOURCE_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("coinbase/agentWallet", "coinbase/agentic-wallet-skills");
    m
});

/// Parse a source string into a structured [`ParsedSource`].
///
/// Supports:
/// - Local paths (`./`, `../`, absolute paths, Windows drives)
/// - GitHub URLs (`https://github.com/owner/repo/tree/branch/path`)
/// - GitHub shorthand (`owner/repo`, `owner/repo/subpath`, `owner/repo@skill`)
/// - `GitLab` URLs (with `/-/tree/` pattern)
/// - Well-known HTTP(S) URLs (non-GitHub/GitLab)
/// - Prefix shorthands (`github:owner/repo`, `gitlab:owner/repo`)
/// - Fragment refs (`…#branch` or `…#branch@skill-filter`) for git sources
/// - Direct git URLs (fallback)
///
/// # Examples
///
/// ```
/// use skill::source::parse_source;
/// use skill::types::SourceType;
///
/// let parsed = parse_source("vercel-labs/skills#main@find-skills");
/// assert_eq!(parsed.source_type, SourceType::Github);
/// assert_eq!(parsed.url, "https://github.com/vercel-labs/skills.git");
/// assert_eq!(parsed.git_ref.as_deref(), Some("main"));
/// assert_eq!(parsed.skill_filter.as_deref(), Some("find-skills"));
/// ```
#[must_use]
pub fn parse_source(input: &str) -> ParsedSource {
    let FragmentRef {
        base,
        ref_: fragment_ref,
        filter: fragment_filter,
    } = fragment::parse(input);

    let mut parsed = parse_fragment_free(&base);

    // Merge fragment data: an explicit `tree/<ref>` in the URL wins over the
    // fragment ref (matches TS `ref || fragmentRef`). Shorthand `@skill` wins
    // over the fragment filter by the same rule.
    if parsed.git_ref.is_none()
        && let Some(r) = fragment_ref
    {
        parsed.git_ref = Some(r);
    }
    if parsed.skill_filter.is_none()
        && let Some(f) = fragment_filter
    {
        parsed.skill_filter = Some(f);
    }

    parsed
}

/// Parse a source that has already had any `#fragment` stripped.
///
/// Ordered match branches: aliases → prefix shorthands → local paths →
/// GitHub/GitLab (tree > tree-without-path > bare repo) → shorthands →
/// well-known URL → generic git URL fallback.
#[allow(
    clippy::too_many_lines,
    reason = "sequential match arms for different source formats"
)]
fn parse_fragment_free(input: &str) -> ParsedSource {
    let mut input = input.to_owned();

    if let Some(alias) = SOURCE_ALIASES.get(input.as_str()) {
        (*alias).clone_into(&mut input);
    }

    if let Some(rest) = input.strip_prefix("github:") {
        return parse_fragment_free(rest);
    }

    if let Some(rest) = input.strip_prefix("gitlab:") {
        return parse_fragment_free(&format!("https://gitlab.com/{rest}"));
    }

    if is_local_path(&input) {
        let resolved = std::path::absolute(Path::new(&input))
            .unwrap_or_else(|_| Path::new(&input).to_path_buf());
        return ParsedSource {
            source_type: SourceType::Local,
            url: resolved.to_string_lossy().into_owned(),
            subpath: None,
            local_path: Some(resolved),
            git_ref: None,
            skill_filter: None,
        };
    }

    if let Some(caps) = github_tree_with_path_re().captures(&input) {
        let owner = &caps[1];
        let repo = &caps[2];
        let git_ref = &caps[3];
        let subpath = &caps[4];
        return ParsedSource {
            source_type: SourceType::Github,
            url: format!("https://github.com/{owner}/{repo}.git"),
            subpath: Some(sanitize_subpath(subpath).unwrap_or_else(|_| subpath.to_owned())),
            local_path: None,
            git_ref: Some(git_ref.to_owned()),
            skill_filter: None,
        };
    }

    if let Some(caps) = github_tree_re().captures(&input) {
        let owner = &caps[1];
        let repo = &caps[2];
        let git_ref = &caps[3];
        return ParsedSource {
            source_type: SourceType::Github,
            url: format!("https://github.com/{owner}/{repo}.git"),
            subpath: None,
            local_path: None,
            git_ref: Some(git_ref.to_owned()),
            skill_filter: None,
        };
    }

    if let Some(caps) = github_repo_re().captures(&input) {
        let owner = &caps[1];
        let repo = caps[2].trim_end_matches(".git");
        return ParsedSource {
            source_type: SourceType::Github,
            url: format!("https://github.com/{owner}/{repo}.git"),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
    }

    if let Some(caps) = gitlab_tree_with_path_re().captures(&input) {
        let protocol = &caps[1];
        let hostname = &caps[2];
        let repo_path = caps[3].trim_end_matches(".git");
        let git_ref = &caps[4];
        let subpath = &caps[5];
        if hostname != "github.com" {
            return ParsedSource {
                source_type: SourceType::Gitlab,
                url: format!("{protocol}://{hostname}/{repo_path}.git"),
                subpath: Some(sanitize_subpath(subpath).unwrap_or_else(|_| subpath.to_owned())),
                local_path: None,
                git_ref: Some(git_ref.to_owned()),
                skill_filter: None,
            };
        }
    }

    if let Some(caps) = gitlab_tree_re().captures(&input) {
        let protocol = &caps[1];
        let hostname = &caps[2];
        let repo_path = caps[3].trim_end_matches(".git");
        let git_ref = &caps[4];
        if hostname != "github.com" {
            return ParsedSource {
                source_type: SourceType::Gitlab,
                url: format!("{protocol}://{hostname}/{repo_path}.git"),
                subpath: None,
                local_path: None,
                git_ref: Some(git_ref.to_owned()),
                skill_filter: None,
            };
        }
    }

    if let Some(caps) = gitlab_repo_re().captures(&input) {
        let repo_path = &caps[1];
        if repo_path.contains('/') {
            return ParsedSource {
                source_type: SourceType::Gitlab,
                url: format!("https://gitlab.com/{repo_path}.git"),
                subpath: None,
                local_path: None,
                git_ref: None,
                skill_filter: None,
            };
        }
    }

    if let Some(caps) = at_skill_re().captures(&input)
        && !input.contains(':')
        && !input.starts_with('.')
        && !input.starts_with('/')
    {
        let owner = &caps[1];
        let repo = &caps[2];
        let skill_filter = &caps[3];
        return ParsedSource {
            source_type: SourceType::Github,
            url: format!("https://github.com/{owner}/{repo}.git"),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: Some(skill_filter.to_owned()),
        };
    }

    if let Some(caps) = shorthand_re().captures(&input)
        && !input.contains(':')
        && !input.starts_with('.')
        && !input.starts_with('/')
    {
        let owner = &caps[1];
        let repo = &caps[2];
        let subpath = caps.get(3).map(|m| m.as_str().to_owned());
        return ParsedSource {
            source_type: SourceType::Github,
            url: format!("https://github.com/{owner}/{repo}.git"),
            subpath: subpath.map(|sp| sanitize_subpath(&sp).unwrap_or(sp)),
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
    }

    if is_well_known_url(&input) {
        return ParsedSource {
            source_type: SourceType::WellKnown,
            url: input,
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
    }

    ParsedSource {
        source_type: SourceType::Git,
        url: input,
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    }
}

/// Extract `owner/repo` (or `group/subgroup/repo` for `GitLab`) from a
/// parsed source for telemetry and lock-file tracking.
///
/// # Examples
///
/// ```
/// use skill::source::{get_owner_repo, parse_source};
///
/// let parsed = parse_source("https://github.com/vercel-labs/skills.git");
/// assert_eq!(get_owner_repo(&parsed).as_deref(), Some("vercel-labs/skills"));
/// ```
#[must_use]
pub fn get_owner_repo(parsed: &ParsedSource) -> Option<String> {
    if parsed.source_type == SourceType::Local {
        return None;
    }

    // SSH URLs: git@host:path
    if let Some(caps) = Regex::new(r"^git@[^:]+:(.+)$").ok()?.captures(&parsed.url) {
        let path = caps[1].trim_end_matches(".git");
        if path.contains('/') {
            return Some(path.to_owned());
        }
        return None;
    }

    if let Ok(url) = url::Url::parse(&parsed.url) {
        let path = url.path().trim_start_matches('/').trim_end_matches(".git");
        if path.contains('/') {
            return Some(path.to_owned());
        }
    }

    None
}

/// Parse `owner/repo` into separate components.
///
/// Returns `None` for any input that does not have exactly two
/// slash-separated segments.
#[must_use]
pub fn parse_owner_repo(owner_repo: &str) -> Option<(String, String)> {
    let mut parts = owner_repo.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(repo), None) => Some((owner.to_owned(), repo.to_owned())),
        _ => None,
    }
}

/// Sanitize a subpath to prevent path traversal.
///
/// # Errors
///
/// Returns an error if the subpath contains `..` segments.
pub fn sanitize_subpath(subpath: &str) -> Result<String> {
    let normalized = subpath.replace('\\', "/");
    for segment in normalized.split('/') {
        if segment == ".." {
            return Err(SkillError::PathTraversal {
                context: "subpath",
                path: subpath.to_owned(),
            });
        }
    }
    Ok(subpath.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_source: core source types ──

    #[test]
    fn test_parse_source_github_shorthand_yields_canonical_url() {
        let p = parse_source("qntx/skills");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.url, "https://github.com/qntx/skills.git");
    }

    #[test]
    fn test_parse_source_github_at_skill_extracts_filter() {
        let p = parse_source("vercel-labs/skills@find-skills");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.skill_filter.as_deref(), Some("find-skills"));
    }

    #[test]
    fn test_parse_source_local_path_resolves_absolute() {
        let p = parse_source("./my-skills");
        assert_eq!(p.source_type, SourceType::Local);
        assert!(p.local_path.is_some());
    }

    #[test]
    fn test_parse_source_well_known_preserves_url() {
        let p = parse_source("https://mintlify.com/docs");
        assert_eq!(p.source_type, SourceType::WellKnown);
        assert_eq!(p.url, "https://mintlify.com/docs");
    }

    #[test]
    fn test_parse_source_unknown_falls_back_to_git() {
        let p = parse_source("git@internal-host:team/repo.git");
        assert_eq!(p.source_type, SourceType::Git);
    }

    // ── parse_source: fragment refs ──

    #[test]
    fn test_parse_source_fragment_ref_on_shorthand() {
        let p = parse_source("owner/repo#v2");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.url, "https://github.com/owner/repo.git");
        assert_eq!(p.git_ref.as_deref(), Some("v2"));
        assert!(p.skill_filter.is_none());
    }

    #[test]
    fn test_parse_source_fragment_ref_with_filter() {
        let p = parse_source("owner/repo#main@find-skills");
        assert_eq!(p.git_ref.as_deref(), Some("main"));
        assert_eq!(p.skill_filter.as_deref(), Some("find-skills"));
    }

    #[test]
    fn test_parse_source_fragment_ref_on_github_url() {
        let p = parse_source("https://github.com/owner/repo#release-1.0");
        assert_eq!(p.git_ref.as_deref(), Some("release-1.0"));
    }

    #[test]
    fn test_parse_source_tree_path_ref_wins_over_fragment() {
        let p = parse_source("https://github.com/owner/repo/tree/branch-a#branch-b");
        assert_eq!(p.git_ref.as_deref(), Some("branch-a"));
    }

    #[test]
    fn test_parse_source_fragment_untouched_on_well_known() {
        let p = parse_source("https://mintlify.com/docs#section");
        assert_eq!(p.source_type, SourceType::WellKnown);
        assert!(p.git_ref.is_none());
        assert!(p.url.contains("#section"));
    }

    #[test]
    fn test_parse_source_fragment_ref_url_decoded() {
        let p = parse_source("owner/repo#feature%2Fauth");
        assert_eq!(p.git_ref.as_deref(), Some("feature/auth"));
    }

    // ── get_owner_repo ──

    #[test]
    fn test_get_owner_repo_extracts_from_https_url() {
        let p = parse_source("https://github.com/foo/bar.git");
        assert_eq!(get_owner_repo(&p).as_deref(), Some("foo/bar"));
    }

    #[test]
    fn test_get_owner_repo_extracts_from_ssh_url() {
        let p = ParsedSource {
            source_type: SourceType::Git,
            url: "git@github.com:foo/bar.git".to_owned(),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
        assert_eq!(get_owner_repo(&p).as_deref(), Some("foo/bar"));
    }

    #[test]
    fn test_get_owner_repo_none_for_local() {
        let p = parse_source("./somewhere");
        assert_eq!(get_owner_repo(&p), None);
    }

    // ── parse_owner_repo ──

    #[test]
    fn test_parse_owner_repo_valid_pair() {
        assert_eq!(
            parse_owner_repo("foo/bar"),
            Some(("foo".to_owned(), "bar".to_owned()))
        );
    }

    #[test]
    fn test_parse_owner_repo_rejects_three_segments() {
        assert_eq!(parse_owner_repo("a/b/c"), None);
    }

    #[test]
    fn test_parse_owner_repo_rejects_empty() {
        assert_eq!(parse_owner_repo(""), None);
    }

    // ── sanitize_subpath ──

    #[test]
    fn test_sanitize_subpath_safe_path_roundtrips() {
        assert_eq!(
            sanitize_subpath("skills/my-skill").unwrap(),
            "skills/my-skill"
        );
    }

    #[test]
    fn test_sanitize_subpath_rejects_traversal() {
        assert!(sanitize_subpath("../../etc/passwd").is_err());
    }

    #[test]
    fn test_sanitize_subpath_normalizes_backslashes() {
        // Windows paths with `..` must still be rejected after normalization.
        assert!(sanitize_subpath("..\\secret").is_err());
    }
}

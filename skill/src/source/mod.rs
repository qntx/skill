//! Source string parsing.
//!
//! Converts user-provided source strings (GitHub shorthand, URLs, local
//! paths, etc.) into a structured [`ParsedSource`].
//!
//! Internal layout (private submodules):
//!
//! - `regex`    — compiled regex patterns and classification helpers
//! - `fragment` — `#ref[@filter]` parsing for ref-aware installs
//! - `matchers` — individual URL-shape matchers composed by [`parse_source`]
//!
//! The public API is this module's [`parse_source`], [`owner_repo`],
//! [`parse_owner_repo`], and [`sanitize_subpath`].

mod fragment;
mod matchers;
mod regex;

use std::collections::HashMap;
use std::sync::LazyLock;

use self::fragment::FragmentRef;
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

/// Ordered pipeline of matchers.  The first matcher to return `Some` wins.
type Matcher = fn(&str) -> Option<ParsedSource>;

/// Matchers are probed in this order; every URL shape is mutually exclusive
/// in practice, so the first successful match is the right answer.
const MATCHERS: &[Matcher] = &[
    matchers::try_local_path,
    matchers::try_github_tree_with_path,
    matchers::try_github_tree,
    matchers::try_github_repo,
    matchers::try_gitlab_tree_with_path,
    matchers::try_gitlab_tree,
    matchers::try_gitlab_repo,
    matchers::try_at_skill,
    matchers::try_shorthand,
    matchers::try_well_known,
];

/// Parse a source that has already had any `#fragment` stripped.
///
/// Resolution order:
///
/// 1. Alias lookup and `github:` / `gitlab:` prefix stripping.
/// 2. Delegate to [`MATCHERS`] in order — the first matcher to succeed wins.
/// 3. Fall back to a generic `Git` source if nothing matched.
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

    for matcher in MATCHERS {
        if let Some(parsed) = matcher(&input) {
            return parsed;
        }
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
/// use skill::source::{owner_repo, parse_source};
///
/// let parsed = parse_source("https://github.com/vercel-labs/skills.git");
/// assert_eq!(owner_repo(&parsed).as_deref(), Some("vercel-labs/skills"));
/// ```
#[must_use]
pub fn owner_repo(parsed: &ParsedSource) -> Option<String> {
    if parsed.source_type == SourceType::Local {
        return None;
    }

    // SSH URLs: git@host:path
    if let Some(caps) = regex::ssh_url_re().captures(&parsed.url) {
        let path = caps[1].trim_end_matches(".git");
        return path.contains('/').then(|| path.to_owned());
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

    #[test]
    fn test_owner_repo_extracts_from_https_url() {
        let p = parse_source("https://github.com/foo/bar.git");
        assert_eq!(owner_repo(&p).as_deref(), Some("foo/bar"));
    }

    #[test]
    fn test_owner_repo_extracts_from_ssh_url() {
        let p = ParsedSource {
            source_type: SourceType::Git,
            url: "git@github.com:foo/bar.git".to_owned(),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
        assert_eq!(owner_repo(&p).as_deref(), Some("foo/bar"));
    }

    #[test]
    fn test_owner_repo_none_for_local() {
        let p = parse_source("./somewhere");
        assert_eq!(owner_repo(&p), None);
    }

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

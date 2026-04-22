//! Source string parsing.
//!
//! Converts user-provided source strings (GitHub shorthand, URLs, local paths,
//! etc.) into a structured [`ParsedSource`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

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
#[must_use]
pub fn parse_source(input: &str) -> ParsedSource {
    // Strip a trailing `#ref[@filter]` fragment before any other processing so
    // downstream regex matches see clean git URLs / shorthands. Matches the TS
    // reference `parseFragmentRef` behavior exactly.
    let FragmentRef {
        base,
        ref_: fragment_ref,
        filter: fragment_filter,
    } = parse_fragment_ref(input);

    let mut parsed = parse_source_inner(&base);

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

/// Result of stripping a `#ref[@filter]` fragment from a source string.
struct FragmentRef {
    /// Input with the `#…` fragment removed (or the original input if the
    /// fragment was not a valid git-source ref).
    base: String,
    /// The decoded `ref` portion of the fragment, if any.
    ref_: Option<String>,
    /// The decoded `@filter` portion of the fragment, if any.
    filter: Option<String>,
}

/// Split `input` into its base portion and optional `ref` + `skillFilter`.
///
/// Fragments are only honored for git-like sources (GitHub / GitLab repo or
/// tree URLs and shorthands); for other inputs the `#` is preserved verbatim
/// so well-known URLs with fragment identifiers are untouched. Matches TS
/// `parseFragmentRef`.
fn parse_fragment_ref(input: &str) -> FragmentRef {
    let Some(hash_idx) = input.find('#') else {
        return FragmentRef {
            base: input.to_owned(),
            ref_: None,
            filter: None,
        };
    };

    let base = &input[..hash_idx];
    let fragment = &input[hash_idx + 1..];

    if fragment.is_empty() || !looks_like_git_source(base) {
        return FragmentRef {
            base: input.to_owned(),
            ref_: None,
            filter: None,
        };
    }

    let (ref_part, filter_part) = fragment.split_once('@').map_or_else(
        || (fragment, None),
        |(r, f)| (r, Some(decode_fragment_value(f))),
    );

    let ref_value = if ref_part.is_empty() {
        None
    } else {
        Some(decode_fragment_value(ref_part))
    };

    FragmentRef {
        base: base.to_owned(),
        ref_: ref_value,
        filter: filter_part.filter(|s| !s.is_empty()),
    }
}

/// URL-decode a fragment value, falling back to the raw string on error.
fn decode_fragment_value(value: &str) -> String {
    urlencoding::decode(value).map_or_else(|_| value.to_owned(), std::borrow::Cow::into_owned)
}

/// Whether `input` is a git-like source for which `#ref` should be parsed.
fn looks_like_git_source(input: &str) -> bool {
    if input.starts_with("github:") || input.starts_with("gitlab:") {
        return true;
    }

    if is_local_path(input) {
        return false;
    }

    if input.starts_with("http://") || input.starts_with("https://") {
        if let Ok(url) = url::Url::parse(input) {
            let host = url.host_str().unwrap_or("");
            let path = url.path();
            // Only GitHub/GitLab repo-like paths should accept fragment refs;
            // opaque HTTP URLs (well-known) keep their original fragments.
            if host == "github.com" {
                return github_path_re().is_match(path);
            }
            if host == "gitlab.com" {
                return gitlab_path_re().is_match(path);
            }
        }
        return false;
    }

    // Bare `owner/repo[/subpath]` shorthand.
    if !input.contains(':') && !input.starts_with('.') && !input.starts_with('/') {
        return shorthand_re().is_match(input) || at_skill_re().is_match(input);
    }

    false
}

/// Internal parser that operates on a fragment-free source string.
#[allow(
    clippy::too_many_lines,
    reason = "sequential match arms for different source formats"
)]
fn parse_source_inner(input: &str) -> ParsedSource {
    let mut input = input.to_owned();

    // Resolve aliases
    if let Some(alias) = SOURCE_ALIASES.get(input.as_str()) {
        (*alias).clone_into(&mut input);
    }

    // github: prefix
    if let Some(rest) = input.strip_prefix("github:") {
        return parse_source_inner(rest);
    }

    // gitlab: prefix
    if let Some(rest) = input.strip_prefix("gitlab:") {
        return parse_source_inner(&format!("https://gitlab.com/{rest}"));
    }

    // Local path
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

    // GitHub URL with tree path: github.com/owner/repo/tree/branch/path
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

    // GitHub URL with branch only: github.com/owner/repo/tree/branch
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

    // GitHub URL: github.com/owner/repo
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

    // GitLab URL with tree path: /-/tree/branch/path
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

    // GitLab URL with branch only: /-/tree/branch
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

    // GitLab.com URL: gitlab.com/owner/repo or gitlab.com/group/subgroup/repo
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

    // GitHub @skill syntax: owner/repo@skill-name
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

    // GitHub shorthand: owner/repo or owner/repo/subpath
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

    // Well-known: HTTP(S) URL that isn't GitHub/GitLab
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

    // Fallback: direct git URL
    ParsedSource {
        source_type: SourceType::Git,
        url: input,
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    }
}

/// Extract `owner/repo` (or `group/subgroup/repo` for `GitLab`) from a parsed
/// source for telemetry and lock-file tracking.
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

    // HTTP(S) URLs
    if let Ok(url) = url::Url::parse(&parsed.url) {
        let path = url.path().trim_start_matches('/').trim_end_matches(".git");
        if path.contains('/') {
            return Some(path.to_owned());
        }
    }

    None
}

/// Parse `owner/repo` into separate components.
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

/// Check whether the input looks like a local filesystem path.
fn is_local_path(input: &str) -> bool {
    if Path::new(input).is_absolute() {
        return true;
    }
    if input.starts_with("./") || input.starts_with("../") || input == "." || input == ".." {
        return true;
    }
    // Windows absolute paths like C:\ or D:\
    if let [drive, b':', sep, ..] = input.as_bytes()
        && drive.is_ascii_alphabetic()
        && (*sep == b'/' || *sep == b'\\')
    {
        return true;
    }
    false
}

/// Check whether the input is a well-known HTTP(S) URL.
fn is_well_known_url(input: &str) -> bool {
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return false;
    }
    let Ok(parsed) = url::Url::parse(input) else {
        return false;
    };
    let excluded = ["github.com", "gitlab.com", "raw.githubusercontent.com"];
    if let Some(host) = parsed.host_str()
        && excluded.contains(&host)
    {
        return false;
    }
    !Path::new(input)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("git"))
}

/// Declare a lazily-compiled static regex with a doc comment.
macro_rules! static_regex {
    ($($(#[doc = $doc:literal])* $name:ident => $pattern:literal;)*) => {
        $(
            $(#[doc = $doc])*
            #[allow(clippy::expect_used, reason = "static regex pattern is known valid at compile time")]
            fn $name() -> &'static Regex {
                static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
                &RE
            }
        )*
    };
}

static_regex! {
    /// GitHub tree URL with subpath: `github.com/owner/repo/tree/branch/path`.
    github_tree_with_path_re => r"github\.com/([^/]+)/([^/]+)/tree/([^/]+)/(.+)";

    /// GitHub tree URL without subpath: `github.com/owner/repo/tree/branch`.
    github_tree_re => r"github\.com/([^/]+)/([^/]+)/tree/([^/]+)$";

    /// GitHub repository URL: `github.com/owner/repo`.
    github_repo_re => r"github\.com/([^/]+)/([^/]+)";

    /// GitLab tree URL with subpath: `host/path/-/tree/branch/path`.
    gitlab_tree_with_path_re => r"^(https?):?//([^/]+)/(.+?)/-/tree/([^/]+)/(.+)";

    /// GitLab tree URL without subpath: `host/path/-/tree/branch`.
    gitlab_tree_re => r"^(https?):?//([^/]+)/(.+?)/-/tree/([^/]+)$";

    /// GitLab repository URL: `gitlab.com/path`.
    gitlab_repo_re => r"gitlab\.com/(.+?)(?:\.git)?/?$";

    /// `owner/repo@skill` shorthand.
    at_skill_re => r"^([^/]+)/([^/@]+)@(.+)$";

    /// `owner/repo[/subpath]` shorthand.
    shorthand_re => r"^([^/]+)/([^/]+)(?:/(.+))?$";

    /// Path component of a GitHub repo or tree URL (used by fragment logic).
    github_path_re => r"^/[^/]+/[^/]+(?:\.git)?(?:/tree/[^/]+(?:/.*)?)?/?$";

    /// Path component of a GitLab repo or tree URL (used by fragment logic).
    gitlab_path_re => r"^/.+?/[^/]+(?:\.git)?(?:/-/tree/[^/]+(?:/.*)?)?/?$";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_shorthand() {
        let p = parse_source("qntx/skills");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.url, "https://github.com/qntx/skills.git");
    }

    #[test]
    fn test_github_at_skill() {
        let p = parse_source("vercel-labs/skills@find-skills");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.skill_filter.as_deref(), Some("find-skills"));
    }

    #[test]
    fn test_local_path() {
        let p = parse_source("./my-skills");
        assert_eq!(p.source_type, SourceType::Local);
        assert!(p.local_path.is_some());
    }

    #[test]
    fn test_well_known() {
        let p = parse_source("https://mintlify.com/docs");
        assert_eq!(p.source_type, SourceType::WellKnown);
    }

    #[test]
    fn test_sanitize_subpath_safe() {
        assert!(sanitize_subpath("skills/my-skill").is_ok());
    }

    #[test]
    fn test_sanitize_subpath_unsafe() {
        assert!(sanitize_subpath("../../etc/passwd").is_err());
    }

    #[test]
    fn test_fragment_ref_shorthand() {
        let p = parse_source("owner/repo#v2");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.url, "https://github.com/owner/repo.git");
        assert_eq!(p.git_ref.as_deref(), Some("v2"));
        assert!(p.skill_filter.is_none());
    }

    #[test]
    fn test_fragment_ref_with_filter() {
        let p = parse_source("owner/repo#main@find-skills");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.git_ref.as_deref(), Some("main"));
        assert_eq!(p.skill_filter.as_deref(), Some("find-skills"));
    }

    #[test]
    fn test_fragment_ref_github_url() {
        let p = parse_source("https://github.com/owner/repo#release-1.0");
        assert_eq!(p.source_type, SourceType::Github);
        assert_eq!(p.git_ref.as_deref(), Some("release-1.0"));
    }

    #[test]
    fn test_fragment_ref_tree_url_wins() {
        // Explicit tree/<ref> in path should win over fragment ref.
        let p = parse_source("https://github.com/owner/repo/tree/branch-a#branch-b");
        assert_eq!(p.git_ref.as_deref(), Some("branch-a"));
    }

    #[test]
    fn test_fragment_ref_well_known_untouched() {
        // Fragment on a non-git URL must be preserved verbatim.
        let p = parse_source("https://mintlify.com/docs#section");
        assert_eq!(p.source_type, SourceType::WellKnown);
        assert!(p.git_ref.is_none());
        assert!(p.url.contains("#section"));
    }

    #[test]
    fn test_fragment_ref_url_encoded() {
        let p = parse_source("owner/repo#feature%2Fauth");
        assert_eq!(p.git_ref.as_deref(), Some("feature/auth"));
    }

    #[test]
    fn test_fragment_ref_shorthand_filter_wins_over_fragment() {
        // `@skill` shorthand wins over `#ref@filter` filter part.
        let p = parse_source("owner/repo@shorthand-filter#v1@fragment-filter");
        // The `@` comes before `#`, so `owner/repo@shorthand-filter` gets
        // matched as the at-skill form once the fragment is stripped; the
        // fragment's ref still applies.
        assert_eq!(p.skill_filter.as_deref(), Some("shorthand-filter"));
    }
}

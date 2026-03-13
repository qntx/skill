//! Source string parsing.
//!
//! Converts user-provided source strings (GitHub shorthand, URLs, local paths,
//! etc.) into a structured [`ParsedSource`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::error::{Error, Result};
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
/// - Direct git URLs (fallback)
#[must_use]
pub fn parse_source(input: &str) -> ParsedSource {
    let mut input = input.to_owned();

    // Resolve aliases
    if let Some(alias) = SOURCE_ALIASES.get(input.as_str()) {
        (*alias).clone_into(&mut input);
    }

    // github: prefix
    if let Some(rest) = input.strip_prefix("github:") {
        return parse_source(rest);
    }

    // gitlab: prefix
    if let Some(rest) = input.strip_prefix("gitlab:") {
        return parse_source(&format!("https://gitlab.com/{rest}"));
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
    let parts: Vec<&str> = owner_repo.splitn(3, '/').collect();
    if parts.len() == 2 {
        Some((parts[0].to_owned(), parts[1].to_owned()))
    } else {
        None
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
            return Err(Error::PathTraversal {
                context: "subpath",
                path: subpath.to_owned(),
            });
        }
    }
    Ok(subpath.to_owned())
}

fn is_local_path(input: &str) -> bool {
    if Path::new(input).is_absolute() {
        return true;
    }
    if input.starts_with("./") || input.starts_with("../") || input == "." || input == ".." {
        return true;
    }
    // Windows absolute paths like C:\ or D:\
    if input.len() >= 3 {
        let bytes = input.as_bytes();
        if bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\')
        {
            return true;
        }
    }
    false
}

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

// Compiled regex patterns (cached via LazyLock)

fn github_tree_with_path_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"github\.com/([^/]+)/([^/]+)/tree/([^/]+)/(.+)").expect("valid regex")
    });
    &RE
}

fn github_tree_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"github\.com/([^/]+)/([^/]+)/tree/([^/]+)$").expect("valid regex")
    });
    &RE
}

fn github_repo_re() -> &'static Regex {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"github\.com/([^/]+)/([^/]+)").expect("valid regex"));
    &RE
}

fn gitlab_tree_with_path_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(https?):?//([^/]+)/(.+?)/-/tree/([^/]+)/(.+)").expect("valid regex")
    });
    &RE
}

fn gitlab_tree_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(https?):?//([^/]+)/(.+?)/-/tree/([^/]+)$").expect("valid regex")
    });
    &RE
}

fn gitlab_repo_re() -> &'static Regex {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"gitlab\.com/(.+?)(?:\.git)?/?$").expect("valid regex"));
    &RE
}

fn at_skill_re() -> &'static Regex {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([^/]+)/([^/@]+)@(.+)$").expect("valid regex"));
    &RE
}

fn shorthand_re() -> &'static Regex {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([^/]+)/([^/]+)(?:/(.+))?$").expect("valid regex"));
    &RE
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
}

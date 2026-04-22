//! Lazily-compiled regular expressions for source-string parsing.
//!
//! All regexes live here so the rest of the parser reads as a flat match
//! statement rather than a mix of patterns and business logic.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Declare a lazily-compiled static regex with a doc comment.
///
/// Each emitted function returns a `&'static Regex`. Compilation is done once
/// on first access and cached for process lifetime.
macro_rules! static_regex {
    ($($(#[doc = $doc:literal])* $name:ident => $pattern:literal;)*) => {
        $(
            $(#[doc = $doc])*
            #[allow(
                clippy::expect_used,
                reason = "static regex pattern is known valid at compile time"
            )]
            pub(super) fn $name() -> &'static Regex {
                static RE: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
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

/// Check whether `input` is a local filesystem path.
///
/// Recognises:
/// - POSIX-absolute (`/…`) or Windows drive-letter (`C:\…`) paths.
/// - Relative paths introduced by `./` or `../`.
/// - The special tokens `"."` and `".."`.
pub(super) fn is_local_path(input: &str) -> bool {
    if Path::new(input).is_absolute() {
        return true;
    }
    if input.starts_with("./") || input.starts_with("../") || input == "." || input == ".." {
        return true;
    }
    if let [drive, b':', sep, ..] = input.as_bytes()
        && drive.is_ascii_alphabetic()
        && (*sep == b'/' || *sep == b'\\')
    {
        return true;
    }
    false
}

/// Check whether `input` is a well-known HTTP(S) URL (non-GitHub/GitLab).
///
/// Used as the fallback branch so generic `https://` URLs can be probed for
/// `.well-known/agent-skills/index.json`.
pub(super) fn is_well_known_url(input: &str) -> bool {
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

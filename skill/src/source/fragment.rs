//! URL fragment parsing for ref-aware source strings.
//!
//! Rust Skills supports the `#ref[@skill-filter]` suffix on git-like sources:
//!
//! ```text
//! owner/repo#v2                        → { ref: "v2" }
//! owner/repo#main@find-skills          → { ref: "main", filter: "find-skills" }
//! https://github.com/x/y#feature%2Fauth → { ref: "feature/auth" }
//! ```
//!
//! Fragments are only honored for git-like sources so opaque HTTP URLs
//! (well-known skill endpoints) keep their original `#fragment` intact.
//!
//! Matches the TypeScript reference `parseFragmentRef` byte-for-byte.

use std::borrow::Cow;

use super::regex::{at_skill_re, github_path_re, gitlab_path_re, is_local_path, shorthand_re};

/// Result of stripping a `#ref[@filter]` fragment from a source string.
pub(super) struct FragmentRef {
    /// Input with the `#…` fragment removed (or the original input if the
    /// fragment was not a valid git-source ref).
    pub(super) base: String,
    /// The decoded `ref` portion of the fragment, if any.
    pub(super) ref_: Option<String>,
    /// The decoded `@filter` portion of the fragment, if any.
    pub(super) filter: Option<String>,
}

/// Split `input` into its base portion and optional `ref` + `skillFilter`.
///
/// Fragments are only honored for git-like sources (GitHub / GitLab repo or
/// tree URLs and shorthands); for other inputs the `#` is preserved verbatim
/// so well-known URLs with fragment identifiers are untouched.
pub(super) fn parse(input: &str) -> FragmentRef {
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

    let (ref_part, filter_part) = fragment
        .split_once('@')
        .map_or_else(|| (fragment, None), |(r, f)| (r, Some(decode(f))));

    let ref_value = if ref_part.is_empty() {
        None
    } else {
        Some(decode(ref_part))
    };

    FragmentRef {
        base: base.to_owned(),
        ref_: ref_value,
        filter: filter_part.filter(|s| !s.is_empty()),
    }
}

/// URL-decode a fragment value, falling back to the raw string on error.
fn decode(value: &str) -> String {
    urlencoding::decode(value).map_or_else(|_| value.to_owned(), Cow::into_owned)
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
            if host == "github.com" {
                return github_path_re().is_match(path);
            }
            if host == "gitlab.com" {
                return gitlab_path_re().is_match(path);
            }
        }
        return false;
    }

    if !input.contains(':') && !input.starts_with('.') && !input.starts_with('/') {
        return shorthand_re().is_match(input) || at_skill_re().is_match(input);
    }

    false
}

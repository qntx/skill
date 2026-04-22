//! Ordered URL-shape matchers for [`parse_fragment_free`].
//!
//! Each matcher inspects the input string and either produces a fully
//! resolved [`ParsedSource`] or yields control to the next matcher.
//!
//! [`parse_fragment_free`]: super::parse_fragment_free

use std::path::Path;

use super::regex::{
    at_skill_re, github_repo_re, github_tree_re, github_tree_with_path_re, gitlab_repo_re,
    gitlab_tree_re, gitlab_tree_with_path_re, is_local_path, is_well_known_url, shorthand_re,
};
use super::sanitize_subpath;
use crate::types::{ParsedSource, SourceType};

/// Try to match `input` as a local filesystem path.
pub(super) fn try_local_path(input: &str) -> Option<ParsedSource> {
    if !is_local_path(input) {
        return None;
    }
    let resolved =
        std::path::absolute(Path::new(input)).unwrap_or_else(|_| Path::new(input).to_path_buf());
    Some(ParsedSource {
        source_type: SourceType::Local,
        url: resolved.to_string_lossy().into_owned(),
        subpath: None,
        local_path: Some(resolved),
        git_ref: None,
        skill_filter: None,
    })
}

/// `https://github.com/<owner>/<repo>/tree/<ref>/<subpath>`
pub(super) fn try_github_tree_with_path(input: &str) -> Option<ParsedSource> {
    let caps = github_tree_with_path_re().captures(input)?;
    let owner = &caps[1];
    let repo = &caps[2];
    let git_ref = &caps[3];
    let subpath = &caps[4];
    Some(ParsedSource {
        source_type: SourceType::Github,
        url: format!("https://github.com/{owner}/{repo}.git"),
        subpath: Some(sanitize_subpath(subpath).unwrap_or_else(|_| subpath.to_owned())),
        local_path: None,
        git_ref: Some(git_ref.to_owned()),
        skill_filter: None,
    })
}

/// `https://github.com/<owner>/<repo>/tree/<ref>`
pub(super) fn try_github_tree(input: &str) -> Option<ParsedSource> {
    let caps = github_tree_re().captures(input)?;
    let owner = &caps[1];
    let repo = &caps[2];
    let git_ref = &caps[3];
    Some(ParsedSource {
        source_type: SourceType::Github,
        url: format!("https://github.com/{owner}/{repo}.git"),
        subpath: None,
        local_path: None,
        git_ref: Some(git_ref.to_owned()),
        skill_filter: None,
    })
}

/// `https://github.com/<owner>/<repo>[.git]`
pub(super) fn try_github_repo(input: &str) -> Option<ParsedSource> {
    let caps = github_repo_re().captures(input)?;
    let owner = &caps[1];
    let repo = caps[2].trim_end_matches(".git");
    Some(ParsedSource {
        source_type: SourceType::Github,
        url: format!("https://github.com/{owner}/{repo}.git"),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// `<protocol>://<gitlab-host>/<path>/-/tree/<ref>/<subpath>`
pub(super) fn try_gitlab_tree_with_path(input: &str) -> Option<ParsedSource> {
    let caps = gitlab_tree_with_path_re().captures(input)?;
    let protocol = &caps[1];
    let hostname = &caps[2];
    let repo_path = caps[3].trim_end_matches(".git");
    let git_ref = &caps[4];
    let subpath = &caps[5];
    if hostname == "github.com" {
        return None;
    }
    Some(ParsedSource {
        source_type: SourceType::Gitlab,
        url: format!("{protocol}://{hostname}/{repo_path}.git"),
        subpath: Some(sanitize_subpath(subpath).unwrap_or_else(|_| subpath.to_owned())),
        local_path: None,
        git_ref: Some(git_ref.to_owned()),
        skill_filter: None,
    })
}

/// `<protocol>://<gitlab-host>/<path>/-/tree/<ref>`
pub(super) fn try_gitlab_tree(input: &str) -> Option<ParsedSource> {
    let caps = gitlab_tree_re().captures(input)?;
    let protocol = &caps[1];
    let hostname = &caps[2];
    let repo_path = caps[3].trim_end_matches(".git");
    let git_ref = &caps[4];
    if hostname == "github.com" {
        return None;
    }
    Some(ParsedSource {
        source_type: SourceType::Gitlab,
        url: format!("{protocol}://{hostname}/{repo_path}.git"),
        subpath: None,
        local_path: None,
        git_ref: Some(git_ref.to_owned()),
        skill_filter: None,
    })
}

/// `https://gitlab.com/<group>/<subgroup>/<repo>`
pub(super) fn try_gitlab_repo(input: &str) -> Option<ParsedSource> {
    let caps = gitlab_repo_re().captures(input)?;
    let repo_path = &caps[1];
    if !repo_path.contains('/') {
        return None;
    }
    Some(ParsedSource {
        source_type: SourceType::Gitlab,
        url: format!("https://gitlab.com/{repo_path}.git"),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// `<owner>/<repo>@<skill-filter>` (GitHub shorthand with skill filter).
pub(super) fn try_at_skill(input: &str) -> Option<ParsedSource> {
    if input.contains(':') || input.starts_with('.') || input.starts_with('/') {
        return None;
    }
    let caps = at_skill_re().captures(input)?;
    let owner = &caps[1];
    let repo = &caps[2];
    let skill_filter = &caps[3];
    Some(ParsedSource {
        source_type: SourceType::Github,
        url: format!("https://github.com/{owner}/{repo}.git"),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: Some(skill_filter.to_owned()),
    })
}

/// `<owner>/<repo>[/<subpath>]` (bare GitHub shorthand).
pub(super) fn try_shorthand(input: &str) -> Option<ParsedSource> {
    if input.contains(':') || input.starts_with('.') || input.starts_with('/') {
        return None;
    }
    let caps = shorthand_re().captures(input)?;
    let owner = &caps[1];
    let repo = &caps[2];
    let subpath = caps.get(3).map(|m| m.as_str().to_owned());
    Some(ParsedSource {
        source_type: SourceType::Github,
        url: format!("https://github.com/{owner}/{repo}.git"),
        subpath: subpath.map(|sp| sanitize_subpath(&sp).unwrap_or(sp)),
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// Any `http(s)://<non-github-non-gitlab-host>/...` URL.
pub(super) fn try_well_known(input: &str) -> Option<ParsedSource> {
    if !is_well_known_url(input) {
        return None;
    }
    Some(ParsedSource {
        source_type: SourceType::WellKnown,
        url: input.to_owned(),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

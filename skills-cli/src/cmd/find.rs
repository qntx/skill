//! `skills find [query]` command implementation.
//!
//! When no query is provided, launches an interactive fzf-style search
//! prompt matching the TS `find.ts` UX. After selection, automatically
//! runs `skills add` to install the skill. One-shot mode (with query)
//! prints results and exits.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use clap::Args;
use miette::{IntoDiagnostic, Result};

use crate::ui;

const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

/// Arguments for the `find` command.
#[derive(Args)]
pub struct FindArgs {
    /// Search query (interactive if omitted).
    pub query: Vec<String>,
}

/// A skill result from the search API.
#[derive(Debug, Clone, serde::Deserialize)]
struct SearchSkill {
    name: String,
    #[serde(rename = "id")]
    slug: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    installs: u64,
}

#[derive(Debug, serde::Deserialize)]
struct SearchResponse {
    skills: Vec<SearchSkill>,
}

fn format_installs(count: u64) -> String {
    if count == 0 {
        return String::new();
    }
    if count >= 1_000_000 {
        let v = count as f64 / 1_000_000.0;
        let s = format!("{v:.1}");
        let s = s.trim_end_matches(".0");
        format!("{s}M installs")
    } else if count >= 1_000 {
        let v = count as f64 / 1_000.0;
        let s = format!("{v:.1}");
        let s = s.trim_end_matches(".0");
        format!("{s}K installs")
    } else {
        format!("{count} install{}", if count == 1 { "" } else { "s" })
    }
}

fn api_base() -> String {
    std::env::var("SKILLS_API_URL").unwrap_or_else(|_| "https://skills.sh".to_owned())
}

/// Search call using the current tokio Handle to block on async reqwest.
/// Enforces 2-character minimum to match TS behavior.
fn search_api_sync(query: &str) -> Vec<SearchSkill> {
    if query.len() < 2 {
        return Vec::new();
    }
    let url = format!(
        "{}/api/search?q={}&limit=10",
        api_base(),
        urlencoding::encode(query)
    );

    let handle = tokio::runtime::Handle::current();
    handle
        .block_on(async {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .ok()?;
            let resp = client.get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json::<SearchResponse>().await.ok().map(|r| r.skills)
        })
        .unwrap_or_default()
}

/// Cached search result with the actual `SearchSkill` data preserved.
struct InteractiveResult {
    pkg: String,
    skill_name: String,
    slug: String,
}

fn to_fzf_items(skills: &[SearchSkill]) -> Vec<ui::FzfItem> {
    skills
        .iter()
        .map(|s| {
            let pkg = if s.source.is_empty() {
                &s.slug
            } else {
                &s.source
            };
            let label = format!("{pkg}@{}", s.name);
            ui::FzfItem {
                description: format_installs(s.installs),
                value: label.clone(),
                label,
            }
        })
        .collect()
}

/// Run interactive fzf search, returning selected skill info.
#[allow(clippy::expect_used, clippy::unwrap_in_result)]
fn run_interactive(
    cache: &Arc<Mutex<HashMap<String, Vec<SearchSkill>>>>,
) -> Result<Option<InteractiveResult>> {
    let cache_ref = Arc::clone(cache);
    let result = ui::fzf_search("Search skills:", move |query| {
        if query.len() < 2 {
            return Vec::new();
        }

        #[allow(clippy::expect_used)]
        let mut lock = cache_ref.lock().expect("cache lock");
        let skills = lock
            .entry(query.to_owned())
            .or_insert_with(|| search_api_sync(query));
        to_fzf_items(skills)
    })
    .into_diagnostic()?;

    match result {
        ui::FzfResult::Selected(value) => {
            // Parse "owner/repo@skillname" back into components
            #[allow(clippy::option_if_let_else)]
            if let Some(at_pos) = value.rfind('@') {
                let pkg = &value[..at_pos];
                let skill_name = &value[at_pos + 1..];

                // Look up slug from cache (Mutex::lock only fails if poisoned, which is unrecoverable)
                #[allow(clippy::expect_used)]
                let lock = cache.lock().expect("cache lock poisoned");
                let slug = lock
                    .values()
                    .flat_map(|v| v.iter())
                    .find(|s| {
                        let p = if s.source.is_empty() {
                            &s.slug
                        } else {
                            &s.source
                        };
                        p == pkg && s.name == skill_name
                    })
                    .map(|s| s.slug.clone())
                    .unwrap_or_default();

                Ok(Some(InteractiveResult {
                    pkg: pkg.to_owned(),
                    skill_name: skill_name.to_owned(),
                    slug,
                }))
            } else {
                Ok(Some(InteractiveResult {
                    pkg: value,
                    skill_name: String::new(),
                    slug: String::new(),
                }))
            }
        }
        ui::FzfResult::Cancelled => Ok(None),
    }
}

/// Check if a repo is public via GitHub API (for the final URL display).
async fn is_repo_public(pkg: &str) -> bool {
    let parts: Vec<&str> = pkg.split('/').collect();
    if parts.len() != 2 {
        return false;
    }
    let url = format!("https://api.github.com/repos/{}/{}", parts[0], parts[1]);
    let client = reqwest::Client::new();
    client
        .get(&url)
        .header("User-Agent", "skills-cli")
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

/// Run the find command.
pub async fn run(args: FindArgs) -> Result<()> {
    let query = args.query.join(" ");

    // One-shot search (with query argument)
    if !query.is_empty() {
        let url = format!(
            "{}/api/search?q={}&limit=10",
            api_base(),
            urlencoding::encode(&query)
        );

        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| miette::miette!("Search API error: {e}"))?;

        if !resp.status().is_success() {
            return Err(miette::miette!("Search API returned {}", resp.status()));
        }

        let data: SearchResponse = resp
            .json()
            .await
            .map_err(|e| miette::miette!("Failed to parse search results: {e}"))?;

        // Telemetry
        let mut props = HashMap::new();
        props.insert("event".to_owned(), "find".to_owned());
        props.insert("query".to_owned(), query.clone());
        props.insert("resultCount".to_owned(), data.skills.len().to_string());
        skill::telemetry::track("find", props);

        if data.skills.is_empty() {
            println!("{DIM}No skills found for \"{query}\"{RESET}");
            return Ok(());
        }

        println!("{DIM}Install with{RESET} skills add <owner/repo@skill>");
        println!();

        for skill in data.skills.iter().take(6) {
            let pkg = if skill.source.is_empty() {
                &skill.slug
            } else {
                &skill.source
            };
            let installs = format_installs(skill.installs);
            let installs_str = if installs.is_empty() {
                String::new()
            } else {
                format!(" {CYAN}{installs}{RESET}")
            };

            println!("{TEXT}{pkg}@{}{RESET}{installs_str}", skill.name);
            println!("{DIM}└ https://skills.sh/{}{RESET}", skill.slug);
            println!();
        }

        return Ok(());
    }

    // Interactive mode when no query
    let cache: Arc<Mutex<HashMap<String, Vec<SearchSkill>>>> = Arc::new(Mutex::new(HashMap::new()));

    let selected = tokio::task::block_in_place(|| run_interactive(&cache))?;

    // Telemetry
    let mut props = HashMap::new();
    props.insert("event".to_owned(), "find".to_owned());
    props.insert("query".to_owned(), String::new());
    props.insert(
        "resultCount".to_owned(),
        if selected.is_some() { "1" } else { "0" }.to_owned(),
    );
    props.insert("interactive".to_owned(), "1".to_owned());
    skill::telemetry::track("find", props);

    let Some(result) = selected else {
        println!("{DIM}Search cancelled{RESET}");
        println!();
        return Ok(());
    };

    // Auto-install the selected skill (matching TS behavior)
    let pkg = &result.pkg;
    let skill_name = &result.skill_name;

    println!();
    println!("{TEXT}Installing {BOLD}{skill_name}{RESET} from {DIM}{pkg}{RESET}...");
    println!();

    // Build add args: skills add <pkg> --skill <name>
    let add_args = super::add::AddArgs {
        source: vec![pkg.clone()],
        global: false,
        agent: None,
        skill: Some(vec![skill_name.clone()]),
        yes: false,
        list: false,
        all: false,
        full_depth: false,
        copy: false,
    };

    super::add::run(add_args).await?;

    println!();

    // Show view link
    if !result.slug.is_empty() && is_repo_public(pkg).await {
        println!(
            "{DIM}View the skill at{RESET} {TEXT}https://skills.sh/{}{RESET}",
            result.slug
        );
    } else {
        println!("{DIM}Discover more skills at{RESET} {TEXT}https://skills.sh{RESET}");
    }
    println!();

    Ok(())
}

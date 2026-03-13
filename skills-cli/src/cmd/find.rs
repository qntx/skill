//! `skills find [query]` command implementation.
//!
//! When no query is provided, launches an interactive fzf-style search
//! prompt matching the TS `find.ts` UX. Otherwise, performs a one-shot
//! API search and prints results.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use clap::Args;
use console::style;
use miette::{IntoDiagnostic, Result};

use crate::ui;

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
        format!("{:.1}M installs", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K installs", count as f64 / 1_000.0)
    } else {
        format!("{count} install{}", if count == 1 { "" } else { "s" })
    }
}

fn api_base() -> String {
    std::env::var("SKILLS_API_URL").unwrap_or_else(|_| "https://skills.sh".to_owned())
}

/// Search call using the current tokio Handle to block on async reqwest.
fn search_api_sync(query: &str) -> Vec<SearchSkill> {
    if query.is_empty() {
        return Vec::new();
    }
    let url = format!(
        "{}/api/search?q={}&limit=20",
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

/// Run interactive fzf search.
fn run_interactive() -> Result<Option<String>> {
    let cache: Arc<Mutex<HashMap<String, Vec<SearchSkill>>>> = Arc::new(Mutex::new(HashMap::new()));

    let cache_ref = Arc::clone(&cache);
    let result = ui::fzf_search("Search for skills", move |query| {
        if query.is_empty() {
            return Vec::new();
        }

        let mut lock = cache_ref.lock().expect("cache lock");
        let skills = lock
            .entry(query.to_owned())
            .or_insert_with(|| search_api_sync(query));
        to_fzf_items(skills)
    })
    .into_diagnostic()?;

    match result {
        ui::FzfResult::Selected(value) => Ok(Some(value)),
        ui::FzfResult::Cancelled => Ok(None),
    }
}

/// Run the find command.
pub async fn run(args: FindArgs) -> Result<()> {
    let query = args.query.join(" ");

    // Interactive mode when no query
    if query.is_empty() {
        let selected = tokio::task::block_in_place(run_interactive)?;
        if let Some(skill_ref) = selected {
            println!();
            println!(
                "  {} skills add {}",
                style("Install with:").dim(),
                style(&skill_ref).cyan()
            );
            println!();

            // Telemetry
            let mut props = HashMap::new();
            props.insert("action".to_owned(), "interactive_select".to_owned());
            props.insert("selected".to_owned(), skill_ref);
            skill::telemetry::track("find", props);
        }
        return Ok(());
    }

    // One-shot search
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
    props.insert("query".to_owned(), query.clone());
    props.insert("resultCount".to_owned(), data.skills.len().to_string());
    skill::telemetry::track("find", props);

    if data.skills.is_empty() {
        println!();
        println!(
            "  {}",
            style(format!("No skills found for \"{query}\"")).dim()
        );
        println!();
        return Ok(());
    }

    println!();
    println!(
        "  {} skills add <owner/repo@skill>",
        style("Install with").dim()
    );
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
            format!(" {}", style(&installs).cyan())
        };

        println!(
            "  {}{}",
            style(format!("{pkg}@{}", skill.name)),
            installs_str
        );
        println!("  {} https://skills.sh/{}", style("└").dim(), skill.slug);
        println!();
    }

    Ok(())
}

//! `skills find [query]` command implementation.

use std::collections::HashMap;

use clap::Args;
use console::style;
use miette::Result;
use reqwest;

/// Arguments for the `find` command.
#[derive(Args)]
pub struct FindArgs {
    /// Search query (interactive if omitted).
    pub query: Vec<String>,
}

/// A skill result from the search API.
#[derive(Debug, serde::Deserialize)]
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

/// Run the find command.
pub async fn run(args: FindArgs) -> Result<()> {
    let query = args.query.join(" ");

    if query.is_empty() {
        println!();
        println!("  {}", style("Usage: skills find <query>").dim());
        println!("  {}", style("Then:  skills add <owner/repo@skill>").dim());
        println!();
        return Ok(());
    }

    // Search via API
    let api_base =
        std::env::var("SKILLS_API_URL").unwrap_or_else(|_| "https://skills.sh".to_owned());
    let url = format!(
        "{api_base}/api/search?q={}&limit=10",
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
        "  {} npx skills add <owner/repo@skill>",
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

//! Optional anonymous telemetry.
//!
//! Gated behind the `telemetry` feature. Fire-and-forget HTTP GET with URL
//! query parameters to the skills telemetry endpoint (matching the TS CLI).
//! Respects `DISABLE_TELEMETRY` and `DO_NOT_TRACK` environment variables.

use std::collections::HashMap;
use std::sync::OnceLock;

const TELEMETRY_URL: &str = "https://add-skill.vercel.sh/t";
const AUDIT_URL: &str = "https://add-skill.vercel.sh/audit";

static CLI_VERSION: OnceLock<String> = OnceLock::new();

/// Set the CLI version string for telemetry payloads.
pub fn set_version(version: &str) {
    let _ = CLI_VERSION.set(version.to_owned());
}

/// Check if telemetry is disabled via environment variables.
#[must_use]
pub fn is_disabled() -> bool {
    std::env::var("DISABLE_TELEMETRY").is_ok() || std::env::var("DO_NOT_TRACK").is_ok()
}

/// Check if running in a CI environment.
#[must_use]
fn is_ci() -> bool {
    const CI_VARS: &[&str] = &[
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "TRAVIS",
        "BUILDKITE",
        "JENKINS_URL",
        "TEAMCITY_VERSION",
    ];
    CI_VARS.iter().any(|v| std::env::var(v).is_ok())
}

/// Fire-and-forget telemetry event via HTTP GET with query parameters.
///
/// Does nothing if telemetry is disabled or the `telemetry` feature is not
/// enabled.
#[cfg(feature = "telemetry")]
#[allow(clippy::implicit_hasher)]
pub fn track(event: &str, properties: HashMap<String, String>) {
    if is_disabled() {
        return;
    }

    let mut params = properties;
    params.insert("event".to_owned(), event.to_owned());

    if let Some(v) = CLI_VERSION.get() {
        params.insert("v".to_owned(), v.clone());
    }
    if is_ci() {
        params.insert("ci".to_owned(), "1".to_owned());
    }

    let query = params
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{TELEMETRY_URL}?{query}");

    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();
        if let Ok(client) = client {
            let _ = client.get(&url).send().await;
        }
    });
}

/// No-op when the telemetry feature is disabled.
#[cfg(not(feature = "telemetry"))]
pub fn track(_event: &str, _properties: HashMap<String, String>) {}

/// Security audit data from partner scanners.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PartnerAudit {
    /// Risk level.
    pub risk: String,
    /// Number of alerts.
    pub alerts: Option<u32>,
    /// Security score.
    pub score: Option<f64>,
    /// When the analysis was performed.
    #[serde(rename = "analyzedAt")]
    pub analyzed_at: String,
}

/// Audit response: `{ "partner_name": { "skill_slug": PartnerAudit } }`.
pub type AuditResponse = HashMap<String, HashMap<String, PartnerAudit>>;

/// Fetch security audit results for skills from the audit API.
///
/// Returns `None` on any error or timeout — never blocks installation.
#[cfg(feature = "network")]
pub async fn fetch_audit_data(source: &str, skill_slugs: &[String]) -> Option<AuditResponse> {
    if skill_slugs.is_empty() {
        return None;
    }

    let skills_param = skill_slugs.join(",");
    let url = format!(
        "{AUDIT_URL}?source={}&skills={}",
        urlencoding::encode(source),
        urlencoding::encode(&skills_param),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<AuditResponse>().await.ok()
}

/// No-op when the network feature is disabled.
#[cfg(not(feature = "network"))]
pub async fn fetch_audit_data(_source: &str, _skill_slugs: &[String]) -> Option<AuditResponse> {
    None
}

//! Optional anonymous telemetry.
//!
//! Gated behind the `telemetry` feature. Fire-and-forget HTTP POST
//! to the skills telemetry endpoint. Respects `DISABLE_TELEMETRY` and
//! `DO_NOT_TRACK` environment variables.

use std::collections::HashMap;

/// Check if telemetry is disabled via environment variables.
#[must_use]
pub fn is_telemetry_disabled() -> bool {
    for var in &["DISABLE_TELEMETRY", "DO_NOT_TRACK"] {
        if let Ok(val) = std::env::var(var)
            && (val == "1" || val.eq_ignore_ascii_case("true"))
        {
            return true;
        }
    }
    false
}

/// Fire-and-forget telemetry event.
///
/// Does nothing if telemetry is disabled or the `telemetry` feature is not
/// enabled.
#[cfg(feature = "telemetry")]
#[allow(clippy::implicit_hasher)]
pub fn track(event: &str, properties: HashMap<String, String>) {
    if is_telemetry_disabled() {
        return;
    }

    let mut payload = properties;
    payload.insert("event".to_owned(), event.to_owned());

    // Spawn a background task; ignore failures.
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build();

        if let Ok(client) = client {
            let _ = client
                .post("https://add-skill.vercel.sh/t")
                .json(&payload)
                .send()
                .await;
        }
    });
}

/// No-op when the telemetry feature is disabled.
#[cfg(not(feature = "telemetry"))]
pub fn track(_event: &str, _properties: HashMap<String, String>) {}

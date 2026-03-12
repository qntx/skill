//! Well-known skills provider (RFC 8615).
//!
//! Fetches skills from `/.well-known/skills/index.json` endpoints.

use crate::error::Result;
use crate::types::{RemoteSkill, WellKnownIndex, WellKnownSkill, WellKnownSkillEntry};

use super::traits::{HostProvider, ProviderMatch};

const WELL_KNOWN_PATH: &str = ".well-known/skills";
const INDEX_FILE: &str = "index.json";

/// Provider for well-known skills endpoints.
#[derive(Debug, Clone, Copy)]
pub struct WellKnownProvider;

#[async_trait::async_trait]
impl HostProvider for WellKnownProvider {
    fn id(&self) -> &'static str {
        "well-known"
    }

    fn display_name(&self) -> &'static str {
        "Well-Known Skills"
    }

    fn matches(&self, url: &str) -> ProviderMatch {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ProviderMatch {
                matches: false,
                source_identifier: None,
            };
        }

        let Ok(parsed) = url::Url::parse(url) else {
            return ProviderMatch {
                matches: false,
                source_identifier: None,
            };
        };

        let excluded = ["github.com", "gitlab.com", "huggingface.co"];
        if let Some(host) = parsed.host_str() {
            if excluded.contains(&host) {
                return ProviderMatch {
                    matches: false,
                    source_identifier: None,
                };
            }
            ProviderMatch {
                matches: true,
                source_identifier: Some(format!("wellknown/{host}")),
            }
        } else {
            ProviderMatch {
                matches: false,
                source_identifier: None,
            }
        }
    }

    async fn fetch_skill(&self, url: &str) -> Result<Option<RemoteSkill>> {
        let Some(wk) = self.fetch_single_skill(url).await? else {
            return Ok(None);
        };
        Ok(Some(wk.remote))
    }

    fn to_raw_url(&self, url: &str) -> String {
        url.to_owned()
    }

    fn source_identifier(&self, url: &str) -> String {
        url::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

impl WellKnownProvider {
    /// Fetch the skills index from a well-known endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure.
    #[cfg(feature = "network")]
    pub async fn fetch_index(&self, base_url: &str) -> Result<Option<(WellKnownIndex, String)>> {
        let Ok(parsed) = url::Url::parse(base_url) else {
            return Ok(None);
        };
        let base_path = parsed.path().trim_end_matches('/');
        let host = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));

        let urls_to_try = vec![
            (
                format!("{host}{base_path}/{WELL_KNOWN_PATH}/{INDEX_FILE}"),
                format!("{host}{base_path}"),
            ),
            (
                format!("{host}/{WELL_KNOWN_PATH}/{INDEX_FILE}"),
                host.clone(),
            ),
        ];

        let client = reqwest::Client::new();

        for (index_url, resolved_base) in urls_to_try {
            let resp = match client.get(&index_url).send().await {
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };

            let index: WellKnownIndex = match resp.json().await {
                Ok(i) => i,
                Err(_) => continue,
            };

            if index.skills.is_empty() {
                continue;
            }

            let all_valid = index.skills.iter().all(is_valid_skill_entry);
            if all_valid {
                return Ok(Some((index, resolved_base)));
            }
        }

        Ok(None)
    }

    /// Fetch all skills from a well-known endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure.
    #[cfg(feature = "network")]
    pub async fn fetch_all_skills(&self, url: &str) -> Result<Vec<WellKnownSkill>> {
        let Some((index, resolved_base)) = self.fetch_index(url).await? else {
            return Ok(Vec::new());
        };

        let mut skills = Vec::new();
        for entry in &index.skills {
            if let Some(skill) = self.fetch_skill_by_entry(&resolved_base, entry).await? {
                skills.push(skill);
            }
        }

        Ok(skills)
    }

    /// Fetch a single skill by its index entry.
    #[cfg(feature = "network")]
    async fn fetch_skill_by_entry(
        &self,
        base_url: &str,
        entry: &WellKnownSkillEntry,
    ) -> Result<Option<WellKnownSkill>> {
        let skill_base = format!(
            "{}/{WELL_KNOWN_PATH}/{}",
            base_url.trim_end_matches('/'),
            entry.name
        );

        let client = reqwest::Client::new();

        let skill_md_url = format!("{skill_base}/SKILL.md");
        let resp = match client.get(&skill_md_url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => return Ok(None),
        };

        let content = resp.text().await.map_err(crate::error::Error::from)?;

        let fm = crate::skills::extract_frontmatter(&content);
        let (name, description) = match fm {
            Some((fm_str, _)) => {
                let data: serde_yaml::Value = serde_yaml::from_str(fm_str).unwrap_or_default();
                let n = data
                    .get("name")
                    .and_then(serde_yaml::Value::as_str)
                    .map(String::from);
                let d = data
                    .get("description")
                    .and_then(serde_yaml::Value::as_str)
                    .map(String::from);
                match (n, d) {
                    (Some(n), Some(d)) => (n, d),
                    _ => return Ok(None),
                }
            }
            None => return Ok(None),
        };

        let mut files = std::collections::HashMap::new();
        files.insert("SKILL.md".to_owned(), content.clone());

        for file_path in &entry.files {
            if file_path.eq_ignore_ascii_case("SKILL.md") {
                continue;
            }
            let file_url = format!("{skill_base}/{file_path}");
            if let Ok(resp) = client.get(&file_url).send().await
                && resp.status().is_success()
                && let Ok(file_content) = resp.text().await
            {
                files.insert(file_path.clone(), file_content);
            }
        }

        let metadata = fm.and_then(|(fm_str, _)| {
            let data: serde_yaml::Value = serde_yaml::from_str(fm_str).ok()?;
            data.get("metadata").and_then(|m| {
                serde_yaml::from_value::<std::collections::HashMap<String, serde_yaml::Value>>(
                    m.clone(),
                )
                .ok()
            })
        });

        Ok(Some(WellKnownSkill {
            remote: RemoteSkill {
                name,
                description,
                content,
                install_name: entry.name.clone(),
                source_url: skill_md_url,
                provider_id: "well-known".to_owned(),
                source_identifier: self.source_identifier(base_url),
                metadata,
            },
            files,
            index_entry: entry.clone(),
        }))
    }

    /// Fetch a single well-known skill.
    #[cfg(feature = "network")]
    async fn fetch_single_skill(&self, url: &str) -> Result<Option<WellKnownSkill>> {
        let Some((index, resolved_base)) = self.fetch_index(url).await? else {
            return Ok(None);
        };

        if index.skills.len() == 1 {
            return self
                .fetch_skill_by_entry(&resolved_base, &index.skills[0])
                .await;
        }

        Ok(None)
    }
}

fn is_valid_skill_entry(entry: &WellKnownSkillEntry) -> bool {
    if entry.name.is_empty() || entry.description.is_empty() || entry.files.is_empty() {
        return false;
    }

    for file in &entry.files {
        if file.starts_with('/') || file.starts_with('\\') || file.contains("..") {
            return false;
        }
    }

    entry
        .files
        .iter()
        .any(|f| f.eq_ignore_ascii_case("SKILL.md"))
}

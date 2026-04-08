//! Plugin manifest discovery for Claude Code compatibility.
//!
//! Discovers skills declared in `.claude-plugin/marketplace.json` and
//! `.claude-plugin/plugin.json` files, matching the Vercel TS
//! `plugin-manifest.ts` implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
/// Deserialized `marketplace.json` content.
struct MarketplaceManifest {
    /// Top-level metadata.
    metadata: Option<MarketplaceMetadata>,
    /// Plugin entries.
    plugins: Option<Vec<PluginManifestEntry>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Marketplace metadata block.
struct MarketplaceMetadata {
    /// Root directory for plugin assets.
    plugin_root: Option<String>,
}

#[derive(Debug, Deserialize)]
/// A single plugin entry in a marketplace or plugin manifest.
struct PluginManifestEntry {
    /// Source reference (string or object).
    source: Option<serde_json::Value>,
    /// Paths to skill directories.
    skills: Option<Vec<String>>,
    /// Plugin display name.
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
/// Deserialized `plugin.json` content.
struct PluginManifest {
    /// Paths to skill directories.
    skills: Option<Vec<String>>,
    /// Plugin display name.
    name: Option<String>,
}

/// Check if a path is contained within a base directory.
fn is_contained_in(target_path: &Path, base_path: &Path) -> bool {
    let normalized_base = crate::path_util::normalize_absolute(base_path);
    let normalized_target = crate::path_util::normalize_absolute(target_path);
    normalized_target.starts_with(&normalized_base)
}

/// Validate that a relative path starts with `./` (per Claude Code convention).
fn is_valid_relative_path(path: &str) -> bool {
    path.starts_with("./")
}

/// Combined result from plugin manifest parsing.
struct PluginManifestData {
    /// Directories to search for skill subdirectories.
    search_dirs: Vec<PathBuf>,
    /// Mapping from normalized skill directory path to plugin name.
    groupings: HashMap<PathBuf, String>,
}

/// Parse plugin manifests once and extract both search directories and groupings.
#[allow(
    clippy::excessive_nesting,
    reason = "manifest × plugin × skill path iteration"
)]
async fn parse_plugin_manifests(base_path: &Path) -> PluginManifestData {
    let mut data = PluginManifestData {
        search_dirs: Vec::new(),
        groupings: HashMap::new(),
    };

    let process_plugin = |plugin_base: &Path,
                          skills: Option<&Vec<String>>,
                          plugin_name: Option<&str>,
                          out: &mut PluginManifestData| {
        if !is_contained_in(plugin_base, base_path) {
            return;
        }

        if let Some(skill_list) = skills {
            for skill_path in skill_list {
                if !is_valid_relative_path(skill_path) {
                    continue;
                }
                let skill_dir = plugin_base.join(skill_path);
                let skill_parent = skill_dir.parent().unwrap_or(&skill_dir).to_path_buf();
                if is_contained_in(&skill_parent, base_path) {
                    out.search_dirs.push(skill_parent);
                }
                if let Some(name) = plugin_name
                    && is_contained_in(&skill_dir, base_path)
                {
                    let resolved = crate::path_util::normalize_absolute(&skill_dir);
                    out.groupings.insert(resolved, name.to_owned());
                }
            }
        }
        out.search_dirs.push(plugin_base.join("skills"));
    };

    // Try marketplace.json (multi-plugin catalog).
    if let Ok(content) =
        tokio::fs::read_to_string(base_path.join(".claude-plugin/marketplace.json")).await
        && let Ok(manifest) = serde_json::from_str::<MarketplaceManifest>(&content)
    {
        let plugin_root = manifest
            .metadata
            .as_ref()
            .and_then(|m| m.plugin_root.as_deref());

        let valid_plugin_root = plugin_root.is_none_or(is_valid_relative_path);

        if valid_plugin_root {
            for plugin in manifest.plugins.iter().flatten() {
                let source_str = match &plugin.source {
                    Some(serde_json::Value::String(s)) => {
                        if !is_valid_relative_path(s) {
                            continue;
                        }
                        Some(s.as_str())
                    }
                    None => None,
                    _ => continue,
                };

                let plugin_base = base_path
                    .join(plugin_root.unwrap_or(""))
                    .join(source_str.unwrap_or(""));
                process_plugin(
                    &plugin_base,
                    plugin.skills.as_ref(),
                    plugin.name.as_deref(),
                    &mut data,
                );
            }
        }
    }

    // Try plugin.json (single plugin at root).
    if let Ok(content) =
        tokio::fs::read_to_string(base_path.join(".claude-plugin/plugin.json")).await
        && let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content)
    {
        process_plugin(
            base_path,
            manifest.skills.as_ref(),
            manifest.name.as_deref(),
            &mut data,
        );
    }

    data
}

/// Extract skill search directories from plugin manifests.
pub(crate) async fn get_plugin_skill_paths(base_path: &Path) -> Vec<PathBuf> {
    parse_plugin_manifests(base_path).await.search_dirs
}

/// Get a map of skill directory paths to plugin names from plugin manifests.
pub(crate) async fn get_plugin_groupings(base_path: &Path) -> HashMap<PathBuf, String> {
    parse_plugin_manifests(base_path).await.groupings
}

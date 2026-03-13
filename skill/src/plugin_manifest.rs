//! Plugin manifest discovery for Claude Code compatibility.
//!
//! Discovers skills declared in `.claude-plugin/marketplace.json` and
//! `.claude-plugin/plugin.json` files, matching the Vercel TS
//! `plugin-manifest.ts` implementation.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MarketplaceManifest {
    metadata: Option<MarketplaceMetadata>,
    plugins: Option<Vec<PluginManifestEntry>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceMetadata {
    plugin_root: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PluginManifestEntry {
    source: Option<serde_json::Value>,
    skills: Option<Vec<String>>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    skills: Option<Vec<String>>,
    name: Option<String>,
}

/// Check if a path is contained within a base directory.
fn is_contained_in(target_path: &Path, base_path: &Path) -> bool {
    let normalized_base = normalize_resolve(base_path);
    let normalized_target = normalize_resolve(target_path);
    normalized_target.starts_with(&normalized_base)
}

/// Best-effort normalize + resolve.
fn normalize_resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    lexical_normalize(&absolute)
}

/// Lexical path normalization (resolve `.` and `..` without filesystem access).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Validate that a relative path starts with `./` (per Claude Code convention).
fn is_valid_relative_path(path: &str) -> bool {
    path.starts_with("./")
}

/// Extract skill search directories from plugin manifests.
///
/// Handles both `marketplace.json` (multi-plugin) and `plugin.json` (single
/// plugin). Only resolves local paths — remote sources are skipped.
///
/// Returns directories that CONTAIN skills (to be searched for child
/// `SKILL.md` files).
pub async fn get_plugin_skill_paths(base_path: &Path) -> Vec<PathBuf> {
    let mut search_dirs = Vec::new();

    let add_plugin_skill_paths =
        |plugin_base: &Path, skills: Option<&Vec<String>>, dirs: &mut Vec<PathBuf>| {
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
                        dirs.push(skill_parent);
                    }
                }
            }
            // Always add conventional skills/ directory for discovery.
            dirs.push(plugin_base.join("skills"));
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
                // Skip remote sources (object with source/repo) — only handle local
                // string paths.
                let source_str = match &plugin.source {
                    Some(serde_json::Value::String(s)) => {
                        if !is_valid_relative_path(s) {
                            continue;
                        }
                        Some(s.as_str())
                    }
                    None => None,
                    _ => continue, // object or other non-string → remote, skip
                };

                let plugin_base = base_path
                    .join(plugin_root.unwrap_or(""))
                    .join(source_str.unwrap_or(""));
                add_plugin_skill_paths(&plugin_base, plugin.skills.as_ref(), &mut search_dirs);
            }
        }
    }

    // Try plugin.json (single plugin at root).
    if let Ok(content) =
        tokio::fs::read_to_string(base_path.join(".claude-plugin/plugin.json")).await
        && let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content)
    {
        add_plugin_skill_paths(base_path, manifest.skills.as_ref(), &mut search_dirs);
    }

    search_dirs
}

/// Get a map of skill directory paths to plugin names from plugin manifests.
///
/// This allows grouping skills by their parent plugin.
///
/// Returns `HashMap<AbsolutePath, PluginName>`.
pub async fn get_plugin_groupings(base_path: &Path) -> HashMap<PathBuf, String> {
    let mut groupings = HashMap::new();

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
                let Some(ref plugin_name) = plugin.name else {
                    continue;
                };

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

                if !is_contained_in(&plugin_base, base_path) {
                    continue;
                }

                if let Some(skill_list) = &plugin.skills {
                    for skill_path in skill_list {
                        if !is_valid_relative_path(skill_path) {
                            continue;
                        }
                        let skill_dir = plugin_base.join(skill_path);
                        if is_contained_in(&skill_dir, base_path) {
                            let resolved = normalize_resolve(&skill_dir);
                            groupings.insert(resolved, plugin_name.clone());
                        }
                    }
                }
            }
        }
    }

    // Try plugin.json (single plugin at root).
    if let Ok(content) =
        tokio::fs::read_to_string(base_path.join(".claude-plugin/plugin.json")).await
        && let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content)
        && let Some(ref plugin_name) = manifest.name
        && let Some(ref skill_list) = manifest.skills
    {
        for skill_path in skill_list {
            if !is_valid_relative_path(skill_path) {
                continue;
            }
            let skill_dir = base_path.join(skill_path);
            if is_contained_in(&skill_dir, base_path) {
                let resolved = normalize_resolve(&skill_dir);
                groupings.insert(resolved, plugin_name.clone());
            }
        }
    }

    groupings
}

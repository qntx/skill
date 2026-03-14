//! `skills upgrade` command implementation.
//!
//! Downloads the latest release archive from GitHub, extracts the binary,
//! and replaces the current executable. This is a Rust-only capability —
//! the TS CLI relies on npm for updates.

use std::io::{Cursor, Read};
use std::path::Path;

use miette::{IntoDiagnostic, Result, miette};

use crate::ui::{DIM, GREEN, RESET, TEXT};

const GITHUB_API: &str = "https://api.github.com/repos/qntx/skill/releases/latest";

/// Run the upgrade command.
pub async fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("{TEXT}Current version: {current}{RESET}");

    let spinner = cliclack::spinner();
    spinner.start("Checking for updates...");

    let release = fetch_latest_release().await?;
    spinner.stop("Check complete");

    let latest = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);

    if !is_newer(current, latest) {
        let _ = cliclack::log::success(format!("{GREEN}Already up to date ({current}){RESET}"));
        return Ok(());
    }

    println!("{TEXT}New version available: {current} \u{2192} {latest}{RESET}");

    let confirmed: bool = cliclack::confirm("Upgrade now?")
        .initial_value(true)
        .interact()
        .into_diagnostic()?;

    if !confirmed {
        println!("{DIM}Upgrade cancelled{RESET}");
        return Ok(());
    }

    let asset_name =
        platform_asset_name().ok_or_else(|| miette!("Unsupported platform for self-update"))?;

    let asset_url = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| {
            miette!("No binary found for this platform ({asset_name}) in release {latest}")
        })?;

    let spinner = cliclack::spinner();
    spinner.start(format!("Downloading {asset_name}..."));

    let archive_data = download_asset(&asset_url).await?;
    spinner.stop("Download complete");

    let spinner = cliclack::spinner();
    spinner.start("Extracting and installing...");

    let binary_data = extract_binary_from_archive(&archive_data, &asset_name)?;
    replace_current_binary(&binary_data)?;
    spinner.stop("Installation complete");

    let _ = cliclack::outro(format!(
        "{GREEN}Upgraded to {latest}!{RESET} {DIM}Restart your shell for changes to take effect.{RESET}"
    ));

    Ok(())
}

#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

async fn fetch_latest_release() -> Result<GitHubRelease> {
    let client = reqwest::Client::builder()
        .user_agent(format!("skills-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .into_diagnostic()?;

    let resp = client.get(GITHUB_API).send().await.into_diagnostic()?;

    if !resp.status().is_success() {
        return Err(miette!("GitHub API returned status {}", resp.status()));
    }

    resp.json::<GitHubRelease>().await.into_diagnostic()
}

async fn download_asset(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("skills-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .into_diagnostic()?;

    let resp = client.get(url).send().await.into_diagnostic()?;
    if !resp.status().is_success() {
        return Err(miette!("Download failed with status {}", resp.status()));
    }

    resp.bytes().await.into_diagnostic().map(|b| b.to_vec())
}

/// Extract the `skills` binary from a `.tar.gz` or `.zip` archive.
fn extract_binary_from_archive(archive: &[u8], asset_name: &str) -> Result<Vec<u8>> {
    let exe_name = format!("skills{}", std::env::consts::EXE_SUFFIX);

    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        let decoder = flate2::read::GzDecoder::new(Cursor::new(archive));
        let mut tar = tar::Archive::new(decoder);

        for entry in tar.entries().into_diagnostic()? {
            let mut entry = entry.into_diagnostic()?;
            let path = entry.path().into_diagnostic()?.into_owned();

            if path_matches_exe(&path, &exe_name) {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).into_diagnostic()?;
                return Ok(buf);
            }
        }

        Err(miette!("Binary '{exe_name}' not found in tar.gz archive"))
    } else if asset_name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).into_diagnostic()?;

        for i in 0..zip.len() {
            let mut file = zip.by_index(i).into_diagnostic()?;
            let name = file.name().to_owned();

            if name.ends_with(&exe_name) {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).into_diagnostic()?;
                return Ok(buf);
            }
        }

        Err(miette!("Binary '{exe_name}' not found in zip archive"))
    } else {
        // Assume raw binary (no archive wrapper)
        Ok(archive.to_vec())
    }
}

/// Check if a tar entry path ends with the expected executable name.
fn path_matches_exe(path: &Path, exe_name: &str) -> bool {
    path.file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| f == exe_name)
}

fn replace_current_binary(new_binary: &[u8]) -> Result<()> {
    let current_exe = std::env::current_exe().into_diagnostic()?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| miette!("Cannot determine binary directory"))?;

    let backup = parent.join(format!("skills.bak{}", std::env::consts::EXE_SUFFIX));

    // On Windows, we can't overwrite a running binary directly.
    // Rename current → backup, write new. If write fails, restore backup.
    if current_exe.exists() {
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(&current_exe, &backup).into_diagnostic()?;
    }

    if let Err(e) = std::fs::write(&current_exe, new_binary) {
        // Restore backup on failure so the user is never left without a binary
        let _ = std::fs::rename(&backup, &current_exe);
        return Err(e).into_diagnostic();
    }

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&current_exe, perms).into_diagnostic()?;
    }

    // Clean up backup (best-effort — may fail on Windows while running)
    let _ = std::fs::remove_file(&backup);

    Ok(())
}

/// Determine the expected asset name for the current platform.
fn platform_asset_name() -> Option<String> {
    let (os, ext) = match std::env::consts::OS {
        "linux" => ("linux", "tar.gz"),
        "macos" => ("darwin", "tar.gz"),
        "windows" => ("windows", "zip"),
        _ => return None,
    };

    let arch = match std::env::consts::ARCH {
        "x86_64" | "x86" => "x86_64",
        "aarch64" => "aarch64",
        _ => return None,
    };

    Some(format!("skills-{arch}-{os}.{ext}"))
}

/// Semver comparison: returns true if `latest` is newer than `current`.
fn is_newer(current: &str, latest: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    let c = parse(current);
    let l = parse(latest);
    l > c
}

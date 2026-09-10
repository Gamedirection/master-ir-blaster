//! Self-update via GitHub Releases. Only meaningful when running as the
//! distributed AppImage (a dev `cargo run` build has nothing sensible to
//! self-replace) - `appimage_path()` returns `None` in that case and callers
//! should treat updating as unavailable.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

const REPO_API: &str =
    "https://api.github.com/repos/Gamedirection/master-ir-blaster/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

pub struct AvailableUpdate {
    pub version: String,
    pub download_url: String,
}

/// The running AppImage's own path, if we're actually running as one (the
/// AppImage runtime sets this env var to the mounted image's real location).
pub fn appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

/// Path to relaunch the app with: the running AppImage if we are one (so a
/// restart picks up a just-installed self-update, and autostart survives the
/// AppImage being replaced), falling back to the current executable for dev
/// builds / non-AppImage installs.
pub fn exec_path() -> PathBuf {
    appimage_path().unwrap_or_else(|| {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ir-blaster"))
    })
}

/// Checks GitHub's latest release against our own compiled-in version.
/// Returns `Ok(None)` if already up to date.
pub fn check_for_update() -> Result<Option<AvailableUpdate>> {
    let release: GhRelease = ureq::get(REPO_API)
        .set("User-Agent", "ir-blaster-updater")
        .call()
        .context("failed to reach GitHub")?
        .into_json()
        .context("failed to parse GitHub release response")?;

    let latest = release.tag_name.trim_start_matches('v');
    if latest == CURRENT_VERSION {
        return Ok(None);
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".AppImage"))
        .context("latest release has no AppImage asset")?;

    Ok(Some(AvailableUpdate {
        version: latest.to_string(),
        download_url: asset.browser_download_url.clone(),
    }))
}

/// Downloads the new AppImage and overwrites the currently-running one.
/// Safe to do while running: Linux lets you replace a file that's currently
/// open/mapped - this process keeps using the old inode until it exits, and
/// the new file becomes what launches next time (hence "restart to complete
/// install" rather than anything happening immediately).
pub fn install_update(download_url: &str) -> Result<()> {
    let Some(target) = appimage_path() else {
        bail!("not running as an AppImage - nothing to self-update");
    };

    let resp = ureq::get(download_url)
        .set("User-Agent", "ir-blaster-updater")
        .call()
        .context("failed to download update")?;

    let tmp_path = target.with_extension("AppImage.new");
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        std::io::copy(&mut resp.into_reader(), &mut file)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    std::fs::rename(&tmp_path, &target)?;
    Ok(())
}

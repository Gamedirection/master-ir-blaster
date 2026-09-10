use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// XDG autostart entries live here; any `.desktop` file dropped in gets
/// launched by the session manager at login (GNOME, KDE, etc. all honor it).
fn autostart_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/autostart")
}

fn desktop_path() -> PathBuf {
    autostart_dir().join("ir-blaster.desktop")
}

/// Prefer the running AppImage's own path (so autostart survives it being
/// replaced by a self-update) and fall back to the current executable for
/// dev builds / non-AppImage installs.
fn exec_path() -> String {
    crate::updater::appimage_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "ir-blaster".to_string())
        })
}

/// Creates or removes `~/.config/autostart/ir-blaster.desktop`. Safe to call
/// repeatedly with the same value (idempotent), so callers can use it to
/// keep the file in sync with the saved setting on every launch.
pub fn set_enabled(enabled: bool) -> Result<()> {
    if enabled {
        fs::create_dir_all(autostart_dir())?;
        let exec = exec_path();
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=IR Blaster\n\
             Exec=\"{exec}\"\n\
             Icon=ir-blaster\n\
             Comment=IR Blaster (starts in the background; use \"Run hidden\"/tray in Settings)\n\
             X-GNOME-Autostart-enabled=true\n"
        );
        fs::write(desktop_path(), contents)?;
    } else {
        let path = desktop_path();
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

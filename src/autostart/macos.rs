use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.gamedirection.irblaster";

fn plist_path() -> Result<PathBuf> {
    let home = directories::BaseDirs::new()
        .context("couldn't determine home directory")?
        .home_dir()
        .to_path_buf();
    Ok(home.join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

fn current_uid() -> Result<String> {
    let output = Command::new("id").arg("-u").output()?;
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Creates or removes `~/Library/LaunchAgents/com.gamedirection.irblaster.plist`
/// and loads/unloads it via `launchctl` so a Settings toggle takes effect
/// immediately, not just at the next login. Safe to call repeatedly with
/// the same value.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = plist_path()?;
    let uid = current_uid()?;

    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exec = crate::updater::exec_path().display().to_string();
        let contents = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{exec}</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             </dict>\n\
             </plist>\n"
        );
        std::fs::write(&path, contents)?;
        // Best-effort: succeeds even if it was already loaded, and the plist
        // file itself is the source of truth for the next real login either way.
        let _ = Command::new("launchctl")
            .args(["bootstrap", &format!("gui/{uid}")])
            .arg(&path)
            .output();
    } else {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{LABEL}")])
            .output();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

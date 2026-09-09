use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Button {
    pub name: String,
    /// Signed microsecond durations: positive = mark/pulse, negative = space.
    pub codes: Vec<i32>,
    /// Optional user-picked background color (RGB) for the row, e.g. to
    /// color-code buttons by what they do. `#[serde(default)]` so older
    /// saved files without this field still load fine.
    #[serde(default)]
    pub color: Option<[u8; 3]>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Remote {
    pub name: String,
    pub buttons: Vec<Button>,
}

/// A proper user-writable directory, since the compiled binary (especially
/// the distributed AppImage) can't rely on the source tree it was built from
/// existing on whatever machine it's run on.
fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".local/share/ir-blaster");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn store_path() -> PathBuf {
    data_dir().join("remotes.json")
}

/// Older builds stored remotes.json next to the source tree at
/// `CARGO_MANIFEST_DIR` (only correct on the original dev machine); migrate
/// it into the real data dir on first run so existing captures aren't lost.
fn migrate_legacy_store(new_path: &PathBuf) {
    if new_path.exists() {
        return;
    }
    let legacy = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("remotes.json");
    if legacy.exists() {
        let _ = fs::copy(&legacy, new_path);
    }
}

pub fn load() -> Vec<Remote> {
    let path = store_path();
    migrate_legacy_store(&path);
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Parse a previously exported (or hand-written) JSON file of remotes.
/// Returns an error if the file is missing or not valid JSON in this shape.
pub fn import(path: &str) -> Result<Vec<Remote>> {
    let contents = fs::read_to_string(path)?;
    let remotes: Vec<Remote> = serde_json::from_str(&contents)?;
    Ok(remotes)
}

pub fn save(remotes: &[Remote]) -> Result<()> {
    let path = store_path();
    let contents = serde_json::to_string_pretty(remotes)?;
    fs::write(path, contents)?;
    Ok(())
}

fn exports_dir() -> Result<PathBuf> {
    let dir = data_dir().join("exports");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub auto_update_enabled: bool,
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load_settings() -> Settings {
    match fs::read_to_string(settings_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(settings: &Settings) -> Result<()> {
    fs::write(settings_path(), serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

/// Which saved button to auto-fire for each Teams status string (e.g.
/// "busy" -> ("RGB Controller", "Red")). Stored by name rather than index so
/// it survives reordering/renaming of remotes and buttons - resolved to an
/// actual button at trigger time, and simply skipped if the name no longer
/// matches anything.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ReactiveSettings {
    #[serde(default)]
    pub teams_enabled: bool,
    #[serde(default)]
    pub teams_mapping: std::collections::HashMap<String, (String, String)>,
}

fn reactive_settings_path() -> PathBuf {
    data_dir().join("reactive_settings.json")
}

pub fn load_reactive_settings() -> ReactiveSettings {
    match fs::read_to_string(reactive_settings_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => ReactiveSettings::default(),
    }
}

pub fn save_reactive_settings(settings: &ReactiveSettings) -> Result<()> {
    fs::write(
        reactive_settings_path(),
        serde_json::to_string_pretty(settings)?,
    )?;
    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "remote".to_string()
    } else {
        cleaned
    }
}

fn epoch_secs() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

/// Export every remote (same format as the main store, so it's directly
/// re-importable) to its own timestamped file under `exports/`.
pub fn export_all(remotes: &[Remote]) -> Result<PathBuf> {
    let path = exports_dir()?.join(format!("export-all-{}.json", epoch_secs()?));
    fs::write(&path, serde_json::to_string_pretty(remotes)?)?;
    Ok(path)
}

/// Export a single remote (wrapped in a one-element array, so the file has
/// the same shape as a full export and can be re-imported the same way).
pub fn export_remote(remote: &Remote) -> Result<PathBuf> {
    let path = exports_dir()?.join(format!(
        "export-{}-{}.json",
        sanitize_filename(&remote.name),
        epoch_secs()?
    ));
    fs::write(
        &path,
        serde_json::to_string_pretty(std::slice::from_ref(remote))?,
    )?;
    Ok(path)
}

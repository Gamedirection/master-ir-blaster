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

fn store_path() -> PathBuf {
    // Keep the store next to the project sources regardless of the CWD the
    // GUI was launched from.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("remotes.json")
}

pub fn load() -> Vec<Remote> {
    let path = store_path();
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
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("exports");
    fs::create_dir_all(&dir)?;
    Ok(dir)
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

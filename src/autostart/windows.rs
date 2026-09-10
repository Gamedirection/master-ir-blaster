use anyhow::Result;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const VALUE_NAME: &str = "IRBlaster";
const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Creates or removes a value under
/// `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` - the
/// standard no-admin-required per-user autostart mechanism on Windows. Safe
/// to call repeatedly with the same value.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _disposition) = hkcu.create_subkey(RUN_KEY_PATH)?;

    if enabled {
        let exec = crate::updater::exec_path().display().to_string();
        key.set_value(VALUE_NAME, &format!("\"{exec}\""))?;
    } else {
        // Not an error if it was never set - the value just isn't there.
        let _ = key.delete_value(VALUE_NAME);
    }
    Ok(())
}

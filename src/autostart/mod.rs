//! Per-OS "launch at login" mechanism: an XDG autostart `.desktop` entry on
//! Linux, a LaunchAgent plist on macOS, a `HKEY_CURRENT_USER\...\Run`
//! registry value on Windows. All three expose the same
//! `set_enabled(bool) -> Result<()>`, safe to call repeatedly with the same
//! value, so callers can use it to keep the mechanism in sync with the
//! saved setting on every launch.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::set_enabled;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::set_enabled;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::set_enabled;

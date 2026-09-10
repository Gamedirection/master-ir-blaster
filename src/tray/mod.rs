//! System tray icon, with a per-OS backend: `ksni` (D-Bus StatusNotifierItem)
//! on Linux, `tray-icon` (native Win32/AppKit) on Windows and macOS. Both
//! expose the same two functions so callers don't need to care which
//! backend is active.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{show_window, spawn};

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod desktop;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use desktop::{show_window, spawn};

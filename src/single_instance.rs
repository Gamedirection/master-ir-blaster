//! Ensures only one instance runs at a time. A Unix domain socket doubles as
//! both the lock (binding it fails if another instance is already listening)
//! and the IPC channel a second launch uses to tell the first instance to
//! show its window, instead of opening a second one - important now that the
//! app can be launched via autostart, the desktop entry, and manually all in
//! the same session.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

fn socket_path() -> PathBuf {
    crate::store::data_dir().join("instance.sock")
}

/// If another instance is already running, tells it to show its window and
/// exits this process immediately (before any window is created - the whole
/// point is that a second launch never gets as far as opening one).
/// Otherwise, claims the socket for this instance. Returns `None` (rather
/// than crashing) if the lock can't be acquired for some other reason, so a
/// broken lock never blocks the app from starting at all.
pub fn acquire_or_exit() -> Option<UnixListener> {
    let path = socket_path();

    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"show\n");
        std::process::exit(0);
    }

    // Nobody's listening, so any file left at this path is stale (e.g. from
    // a crash that skipped cleanup) - safe to remove before claiming it.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => Some(listener),
        Err(e) => {
            eprintln!("single-instance lock unavailable, continuing without it: {e:#}");
            None
        }
    }
}

/// Background thread that answers other launch attempts by showing this
/// instance's window instead of letting them open their own.
pub fn spawn_listener(listener: UnixListener, ctx: egui::Context) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16];
            let _ = stream.read(&mut buf);
            crate::tray::show_window(&ctx);
        }
    });
}

//! Ensures only one instance runs at a time. A loopback TCP listener doubles
//! as both the lock (binding it fails if another instance is already
//! listening) and the IPC channel a second launch uses to tell the first
//! instance to show its window, instead of opening a second one - important
//! now that the app can be launched via autostart, the desktop entry, and
//! manually all in the same session. Loopback TCP (rather than a Unix domain
//! socket) keeps this module the same on every OS, including Windows, which
//! has no `std::os::unix::net`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

/// An unusual, unregistered high port, chosen to make an accidental
/// collision with something else on loopback unlikely. If a collision (or
/// any other bind failure) ever does happen, `acquire_or_exit` just runs
/// without the lock instead of crashing - see its doc comment.
const PORT: u16 = 57_631;

/// If another instance is already running, tells it to show its window and
/// exits this process immediately (before any window is created - the whole
/// point is that a second launch never gets as far as opening one).
/// Otherwise, claims the port for this instance. Returns `None` (rather than
/// crashing) if the lock can't be acquired for some other reason, so a
/// broken lock never blocks the app from starting at all.
pub fn acquire_or_exit() -> Option<TcpListener> {
    if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", PORT)) {
        let _ = stream.write_all(b"show\n");
        std::process::exit(0);
    }

    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => Some(listener),
        Err(e) => {
            eprintln!("single-instance lock unavailable, continuing without it: {e:#}");
            None
        }
    }
}

/// Background thread that answers other launch attempts by showing this
/// instance's window instead of letting them open their own.
pub fn spawn_listener(listener: TcpListener, ctx: egui::Context) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16];
            let _ = stream.read(&mut buf);
            crate::tray::show_window(&ctx);
        }
    });
}

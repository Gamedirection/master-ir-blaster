//! Windows/macOS tray icon backend, using the `tray-icon` crate. Unlike
//! Linux's `ksni` (which owns a dedicated background thread that reacts to
//! D-Bus calls directly), `tray-icon` on these platforms rides on whatever
//! native event loop is already pumping messages on the thread that created
//! it - which is winit's own loop, already running here - so no manual
//! Win32 message pump is needed; see the crate's own `examples/egui.rs`.
//! Its click/menu events arrive on a plain global channel, which any thread
//! can safely receive from, so a small dedicated thread per event source
//! keeps the same "background thread reacts, calls into the egui Context"
//! shape as the Linux backend.

use std::thread;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};

pub fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

fn to_tray_icon(rgba: &egui::IconData) -> anyhow::Result<Icon> {
    Icon::from_rgba(rgba.rgba.clone(), rgba.width, rgba.height)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Builds the tray icon and menu, then spawns two small background threads
/// that block on the tray-icon crate's global event receivers and react by
/// sending viewport commands - non-fatal if it fails (mirrors the Linux
/// backend's contract), so callers should log and continue rather than
/// treat this as fatal.
pub fn spawn(ctx: egui::Context, icon: &egui::IconData) -> anyhow::Result<()> {
    let show_item = MenuItem::new("Show IR Blaster", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let show_id = show_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    menu.append(&show_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let tray_icon = TrayIconBuilder::new()
        .with_icon(to_tray_icon(icon)?)
        .with_tooltip("IR Blaster")
        .with_menu(Box::new(menu))
        .build()?;
    // Must stay alive for the process's lifetime or the icon disappears;
    // built on the main thread (required on macOS) with nothing else around
    // to own it, so an intentional leak is the simplest correct choice.
    Box::leak(Box::new(tray_icon));

    let click_ctx = ctx.clone();
    thread::spawn(move || {
        while let Ok(event) = TrayIconEvent::receiver().recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_window(&click_ctx);
            }
        }
    });

    thread::spawn(move || {
        while let Ok(event) = MenuEvent::receiver().recv() {
            if event.id == show_id {
                show_window(&ctx);
            } else if event.id == quit_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    });

    Ok(())
}

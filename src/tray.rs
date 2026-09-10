use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Tray};

struct AppTray {
    ctx: egui::Context,
    icon: Icon,
}

pub fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    // Also un-minimize, in case the user minimized it normally rather than
    // via the tray (harmless either way).
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "ir-blaster".into()
    }

    fn title(&self) -> String {
        "IR Blaster".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.icon.clone()]
    }

    /// Left-click on the tray icon: just bring the window back.
    fn activate(&mut self, _x: i32, _y: i32) {
        show_window(&self.ctx);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show IR Blaster".into(),
                activate: Box::new(|this: &mut Self| show_window(&this.ctx)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|this: &mut Self| {
                    this.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Converts the app's window icon (RGBA, straight from `image`) into the
/// ARGB32-network-byte-order format ksni/StatusNotifierItem expects.
fn to_tray_icon(rgba: &egui::IconData) -> Icon {
    let mut data = rgba.rgba.clone();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1); // RGBA -> ARGB
    }
    Icon {
        width: rgba.width as i32,
        height: rgba.height as i32,
        data,
    }
}

/// Spawns the system tray icon in the background. Non-fatal if it fails
/// (e.g. no StatusNotifierWatcher on this desktop, or running sandboxed) -
/// the app works fine without it, just without a way to un-hide from the
/// tray, so callers should log and continue rather than treat this as fatal.
pub fn spawn(ctx: egui::Context, icon: &egui::IconData) -> Result<(), ksni::Error> {
    let tray = AppTray {
        ctx,
        icon: to_tray_icon(icon),
    };
    // The returned Handle is only needed for live icon updates or an
    // explicit shutdown, neither of which this app does - the tray service
    // thread keeps running on its own once spawned, so it's fine to drop.
    tray.spawn()?;
    Ok(())
}

mod store;
mod tiqiaa;
mod updater;

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use store::{Button, Remote};
use tiqiaa::TiqiaaDevice;

const RECORD_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LOG_LINES: usize = 400;

enum DeviceRequest {
    Record {
        remote_index: usize,
        name: String,
        /// Some(i) = overwrite the button already at this index instead of appending.
        overwrite_index: Option<usize>,
    },
    Send {
        remote_index: usize,
        button_index: usize,
        codes: Vec<i32>,
        freq_index: u8,
    },
    /// Force-close and reopen the USB connection (with a hardware reset if a
    /// handle is currently held) - recovers a device wedged by a prior
    /// ungraceful shutdown without needing a physical unplug.
    RefreshDevice,
}

enum DeviceResponse {
    Recorded {
        remote_index: usize,
        button: Button,
        overwrite_index: Option<usize>,
    },
    Sent {
        remote_index: usize,
        button_index: usize,
    },
    /// Live waveform of one in-progress capture attempt (accepted or not
    /// yet) - lets the UI show what the receiver is actually seeing.
    RecordProgress {
        codes: Vec<i32>,
    },
    /// The device has started actually sending reply bytes for the current
    /// Output wait - flips the progress bar green ("receiving").
    Receiving,
    Refreshed,
    Error(String),
}

/// Owns the USB handle and does all blocking I/O off the UI thread.
fn device_worker(
    req_rx: Receiver<DeviceRequest>,
    resp_tx: Sender<DeviceResponse>,
    log_tx: Sender<String>,
) {
    let mut device: Option<TiqiaaDevice> = None;

    for req in req_rx {
        if let DeviceRequest::RefreshDevice = req {
            if let Some(mut d) = device.take() {
                d.hardware_reset().ok();
                drop(d); // graceful cleanup: idle, release interface, reattach kernel driver
            }
            match TiqiaaDevice::open(log_tx.clone()) {
                Ok(d) => {
                    device = Some(d);
                    let _ = resp_tx.send(DeviceResponse::Refreshed);
                }
                Err(e) => {
                    let _ = resp_tx.send(DeviceResponse::Error(format!("{e:#}")));
                }
            }
            continue;
        }

        if device.is_none() {
            match TiqiaaDevice::open(log_tx.clone()) {
                Ok(d) => device = Some(d),
                Err(e) => {
                    let _ = resp_tx.send(DeviceResponse::Error(format!("{e:#}")));
                    continue;
                }
            }
        }
        let dev = device.as_mut().unwrap();

        let result = match req {
            DeviceRequest::Record {
                remote_index,
                name,
                overwrite_index,
            } => dev
                .record_signal(
                    RECORD_TIMEOUT,
                    |codes| {
                        let _ = resp_tx.send(DeviceResponse::RecordProgress {
                            codes: codes.to_vec(),
                        });
                    },
                    || {
                        let _ = resp_tx.send(DeviceResponse::Receiving);
                    },
                )
                .map(|codes| DeviceResponse::Recorded {
                    remote_index,
                    button: Button {
                        name,
                        codes,
                        color: None,
                    },
                    overwrite_index,
                }),
            DeviceRequest::Send {
                remote_index,
                button_index,
                codes,
                freq_index,
            } => dev
                .send_signal(&codes, freq_index)
                .map(|_| DeviceResponse::Sent {
                    remote_index,
                    button_index,
                }),
            DeviceRequest::RefreshDevice => unreachable!("handled above before device is opened"),
        };

        match result {
            Ok(resp) => {
                let _ = resp_tx.send(resp);
            }
            Err(e) => {
                // Drop the handle so the next request re-opens/re-claims the device.
                device = None;
                let _ = resp_tx.send(DeviceResponse::Error(format!("{e:#}")));
            }
        }
    }
}

enum UpdateEvent {
    UpToDate,
    Installed { version: String },
    Error(String),
}

/// Runs one check-and-install pass in a throwaway thread (checks are
/// infrequent enough that a persistent worker isn't worth it). If a newer
/// release is found it's downloaded and installed immediately - there's no
/// separate "confirm" step, matching the auto-update behavior; the manual
/// "Check for Updates" button in Settings does the exact same thing on
/// demand.
fn run_update_check(tx: Sender<UpdateEvent>) {
    let result = match updater::check_for_update() {
        Ok(None) => Ok(UpdateEvent::UpToDate),
        Ok(Some(update)) => match updater::install_update(&update.download_url) {
            Ok(()) => Ok(UpdateEvent::Installed {
                version: update.version,
            }),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    let event = result.unwrap_or_else(|e| UpdateEvent::Error(format!("{e:#}")));
    let _ = tx.send(event);
}

fn draw_waveform(ui: &mut egui::Ui, codes: &[i32], highlight: Option<(usize, usize)>) {
    let height = 50.0;
    let desired_size = egui::vec2(ui.available_width(), height);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

    if !codes.is_empty() {
        // Square-root scale compresses the huge dynamic range between the
        // leading burst and individual bit pulses so both stay visible.
        let weights: Vec<f32> = codes
            .iter()
            .map(|c| (c.unsigned_abs() as f32).sqrt().max(1.0))
            .collect();
        let total: f32 = weights.iter().sum::<f32>().max(1.0);

        let mut edges = Vec::with_capacity(codes.len() + 1);
        let mut x = rect.left();
        edges.push(x);
        for w in &weights {
            x += (w / total) * rect.width();
            edges.push(x);
        }

        let mark_color = ui.visuals().selection.bg_fill;
        for (i, c) in codes.iter().enumerate() {
            if *c > 0 {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(edges[i], rect.top()),
                    egui::pos2(edges[i + 1], rect.bottom()),
                );
                painter.rect_filled(bar, 0.0, mark_color);
            }
        }

        if let Some((start, len)) = highlight {
            let end = (start + len).min(codes.len());
            if start < end && end < edges.len() {
                let hl_rect = egui::Rect::from_min_max(
                    egui::pos2(edges[start], rect.top()),
                    egui::pos2(edges[end], rect.bottom()),
                );
                painter.rect_stroke(
                    hl_rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 180, 0)),
                );
            }
        }
    }
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
    );
}

#[derive(Clone, Copy)]
struct CropSuggestion {
    start: usize,
    len: usize,
    repeat_start: usize,
}

fn fuzzy_eq(a: i32, b: i32) -> bool {
    if (a > 0) != (b > 0) {
        return false;
    }
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
    let tol = ((a.max(b) as f32 * 0.12) as u32).max(60);
    a.abs_diff(b) <= tol
}

/// Look for the longest contiguous run of pulses that repeats elsewhere in
/// the signal (within a fuzz tolerance), e.g. because the remote re-sent the
/// code, or the receiver kept capturing ambient noise after the real signal.
/// Suggests cropping to just the first clean occurrence.
fn analyze(codes: &[i32]) -> Option<CropSuggestion> {
    const MIN_LEN: usize = 8;
    let n = codes.len();
    if n < MIN_LEN * 2 {
        return None;
    }
    let mut best: Option<CropSuggestion> = None;
    for i in 0..n {
        for j in (i + 1)..n {
            let max_len = n - j;
            if max_len < MIN_LEN {
                continue;
            }
            let mut k = 0;
            while k < max_len && fuzzy_eq(codes[i + k], codes[j + k]) {
                k += 1;
            }
            if k >= MIN_LEN && k < n && best.is_none_or(|b| k > b.len) {
                best = Some(CropSuggestion {
                    start: i,
                    len: k,
                    repeat_start: j,
                });
            }
        }
    }
    best
}

/// Black or white, whichever reads better against `bg` (perceptual luminance).
fn contrasting_text_color(bg: [u8; 3]) -> egui::Color32 {
    let luminance = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
    if luminance > 140.0 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}

/// Rough 0-100 heuristic for "does this look like a real captured remote
/// code, or noise/an incomplete capture" - based on everything observed this
/// session: real frames have a long leader burst near the start, a decent
/// pulse count, and a wide spread between short and long pulse widths (real
/// bit encoding); the ambient-noise floor we kept seeing instead is a short
/// run of near-uniform ~32µs blips.
fn confidence_score(codes: &[i32]) -> u8 {
    if codes.len() < 10 {
        return 5;
    }
    let max_abs = codes.iter().map(|c| c.unsigned_abs()).max().unwrap_or(0);
    let min_abs = codes.iter().map(|c| c.unsigned_abs()).min().unwrap_or(0);
    let span = max_abs.saturating_sub(min_abs);

    let mut score: i32 = 0;
    score += (codes.len().min(70) as i32) * 40 / 70; // up to 40 pts for pulse count

    let has_leader = codes.iter().take(4).any(|&c| c.unsigned_abs() > 2000);
    if has_leader {
        score += 25;
    }

    if span > 500 {
        score += 25;
    } else if span > 150 {
        score += 10;
    }

    let tiny_fraction =
        codes.iter().filter(|&&c| c.unsigned_abs() <= 40).count() as f32 / codes.len() as f32;
    if tiny_fraction > 0.8 {
        score -= 30;
    }

    score.clamp(0, 100) as u8
}

const SWEEP_STEP_GAP: Duration = Duration::from_millis(400);

/// One (button, carrier frequency) combination to try during a full sweep.
struct SweepPair {
    button_name: String,
    codes: Vec<i32>,
    freq_index: u8,
}

/// Drives an automated walk through every recorded button crossed with every
/// known carrier frequency, one Send at a time, so it can be paused and
/// rewound - useful for brute-forcing an unknown remote's exact protocol.
struct SweepState {
    pairs: Vec<SweepPair>,
    pos: usize,
    paused: bool,
    next_fire_at: Instant,
}

impl SweepState {
    fn status_line(&self) -> String {
        let pair = &self.pairs[self.pos];
        let hz = tiqiaa::CARRIER_FREQUENCIES[pair.freq_index as usize];
        format!(
            "Sweep {}/{}: \"{}\" @ {hz}Hz{}",
            self.pos + 1,
            self.pairs.len(),
            pair.button_name,
            if self.paused { " (paused)" } else { "" }
        )
    }
}

struct MultiPassCandidate {
    codes: Vec<i32>,
}

/// Captures the same button several times in a row, then lets the user test
/// each capture individually and keep only the one that actually works -
/// useful when a single capture is unreliable (bad timing, noise, etc).
struct MultiPassSession {
    remote_index: usize,
    /// Some(i) = this session will overwrite the existing button at i when
    /// the user picks a winner; None = it'll be appended as a new button.
    overwrite_index: Option<usize>,
    base_name: String,
    target_passes: u8,
    candidates: Vec<MultiPassCandidate>,
    /// Index of the candidate currently being test-sent, if any.
    testing: Option<usize>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Main,
    Settings,
    About,
}

struct App {
    remotes: Vec<Remote>,
    new_remote_name: String,
    new_button_names: Vec<String>,
    status: String,
    busy: bool,
    log: VecDeque<String>,
    show_log: bool,
    preview_label: String,
    preview_codes: Vec<i32>,
    /// (remote_index, button_index) the current preview came from, if any -
    /// lets "Apply Crop" write straight back into the saved button.
    preview_location: Option<(usize, usize)>,
    crop_suggestion: Option<CropSuggestion>,
    /// User-adjustable crop range over `preview_codes` (end exclusive),
    /// initialized from `crop_suggestion` but freely draggable.
    crop_start: usize,
    crop_end: usize,
    /// When a Record/Re-record request is in flight: when it was sent, and
    /// whether the device has started actually sending reply bytes back yet.
    recording_started: Option<Instant>,
    receiving: bool,
    /// Index into `tiqiaa::CARRIER_FREQUENCIES` used for every Send - many
    /// cheap remotes don't use the common 38kHz default.
    send_freq_index: u8,
    sweep: Option<SweepState>,
    /// (remote_index, button_index) currently being renamed inline, if any.
    renaming_button: Option<(usize, usize)>,
    /// remote_index currently being renamed inline, if any.
    renaming_remote: Option<usize>,
    rename_buffer: String,
    multi_pass: Option<MultiPassSession>,
    /// User-adjustable pass count (1-5) for the next multi-pass capture.
    multi_pass_count: u8,
    active_tab: Tab,
    /// Path typed into the Settings tab's "Import from file" field.
    import_path: String,
    /// Persisted (settings.json) - if on, checked once on startup.
    auto_update_enabled: bool,
    update_checking: bool,
    update_tx: Sender<UpdateEvent>,
    update_rx: Receiver<UpdateEvent>,
    req_tx: Sender<DeviceRequest>,
    resp_rx: Receiver<DeviceResponse>,
    log_rx: Receiver<String>,
}

impl App {
    fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel();
        let (resp_tx, resp_rx) = mpsc::channel();
        let (log_tx, log_rx) = mpsc::channel();
        thread::spawn(move || device_worker(req_rx, resp_tx, log_tx));

        let remotes = store::load();
        let new_button_names = vec![String::new(); remotes.len()];

        let settings = store::load_settings();
        let (update_tx, update_rx) = mpsc::channel();
        if settings.auto_update_enabled && updater::appimage_path().is_some() {
            let tx = update_tx.clone();
            thread::spawn(move || run_update_check(tx));
        }

        Self {
            remotes,
            new_remote_name: String::new(),
            new_button_names,
            status: "Ready.".to_string(),
            busy: false,
            log: VecDeque::new(),
            show_log: true,
            preview_label: String::new(),
            preview_codes: Vec::new(),
            preview_location: None,
            crop_suggestion: None,
            crop_start: 0,
            crop_end: 0,
            recording_started: None,
            receiving: false,
            send_freq_index: 0,
            sweep: None,
            renaming_button: None,
            renaming_remote: None,
            rename_buffer: String::new(),
            multi_pass: None,
            multi_pass_count: 3,
            active_tab: Tab::Main,
            import_path: String::new(),
            auto_update_enabled: settings.auto_update_enabled,
            update_checking: false,
            update_tx,
            update_rx,
            req_tx,
            resp_rx,
            log_rx,
        }
    }

    /// Point the waveform preview at `codes`, and (re)initialize the
    /// adjustable crop range from the auto-detected suggestion, or the full
    /// signal if nothing was detected.
    fn set_preview(&mut self, label: String, codes: Vec<i32>, location: Option<(usize, usize)>) {
        self.crop_suggestion = analyze(&codes);
        match self.crop_suggestion {
            Some(cs) => {
                self.crop_start = cs.start;
                self.crop_end = cs.start + cs.len;
            }
            None => {
                self.crop_start = 0;
                self.crop_end = codes.len();
            }
        }
        self.preview_label = label;
        self.preview_codes = codes;
        self.preview_location = location;
    }

    fn push_log(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }

    fn drain_channels(&mut self) {
        while let Ok(line) = self.log_rx.try_recv() {
            self.push_log(line);
        }
        while let Ok(event) = self.update_rx.try_recv() {
            self.update_checking = false;
            match event {
                UpdateEvent::UpToDate => {
                    self.status = "You're running the latest version.".to_string()
                }
                UpdateEvent::Installed { version } => {
                    self.status =
                        format!("Updated to v{version} - restart the app to complete install.")
                }
                UpdateEvent::Error(e) => self.status = format!("Update check failed: {e}"),
            }
        }
        while let Ok(resp) = self.resp_rx.try_recv() {
            match resp {
                DeviceResponse::Recorded {
                    remote_index,
                    button,
                    overwrite_index,
                } => {
                    self.busy = false;
                    self.recording_started = None;
                    self.receiving = false;
                    if let Some(session) = &mut self.multi_pass {
                        let confidence = confidence_score(&button.codes);
                        session.candidates.push(MultiPassCandidate {
                            codes: button.codes.clone(),
                        });
                        self.status = format!(
                            "Pass {}/{} captured: {} pulses (confidence {confidence}%).",
                            session.candidates.len(),
                            session.target_passes,
                            button.codes.len(),
                        );
                        let label =
                            format!("{} - pass {}", session.base_name, session.candidates.len());
                        self.set_preview(label, button.codes, None);
                    } else {
                        self.status = format!(
                            "Recorded \"{}\" ({} pulses).",
                            button.name,
                            button.codes.len()
                        );
                        let label = button.name.clone();
                        let codes = button.codes.clone();
                        let mut location = None;
                        if let Some(remote) = self.remotes.get_mut(remote_index) {
                            let button_index = match overwrite_index {
                                Some(i) if i < remote.buttons.len() => {
                                    let color = remote.buttons[i].color;
                                    remote.buttons[i] = button;
                                    remote.buttons[i].color = color;
                                    i
                                }
                                _ => {
                                    remote.buttons.push(button);
                                    remote.buttons.len() - 1
                                }
                            };
                            location = Some((remote_index, button_index));
                        }
                        self.set_preview(label, codes, location);
                        let _ = store::save(&self.remotes);
                    }
                }
                DeviceResponse::Sent {
                    remote_index,
                    button_index,
                } => {
                    self.busy = false;
                    if let Some(sweep) = &mut self.sweep {
                        // This Sent came from an automated sweep step, not a
                        // manual click - advance the sweep instead of the
                        // usual one-off "Sent" status.
                        sweep.pos += 1;
                        sweep.next_fire_at = Instant::now() + SWEEP_STEP_GAP;
                        if sweep.pos >= sweep.pairs.len() {
                            self.status = "Sweep complete. Did any step react?".to_string();
                            self.sweep = None;
                        } else {
                            self.status = sweep.status_line();
                        }
                    } else if let Some(session) = &mut self.multi_pass {
                        if let Some(i) = session.testing.take() {
                            self.status = format!(
                                "Tested pass {}/{} - did it react?",
                                i + 1,
                                session.candidates.len()
                            );
                        }
                    } else {
                        let name = self
                            .remotes
                            .get(remote_index)
                            .and_then(|r| r.buttons.get(button_index))
                            .map(|b| b.name.clone())
                            .unwrap_or_default();
                        self.status = format!("Sent \"{name}\".");
                    }
                }
                DeviceResponse::RecordProgress { codes } => {
                    self.status = format!(
                        "Listening... last attempt picked up {} pulses.",
                        codes.len()
                    );
                    self.preview_label = "Listening (live)...".to_string();
                    self.crop_suggestion = None;
                    self.preview_codes = codes;
                    self.preview_location = None;
                }
                DeviceResponse::Receiving => {
                    self.receiving = true;
                }
                DeviceResponse::Refreshed => {
                    self.busy = false;
                    self.recording_started = None;
                    self.receiving = false;
                    self.status = "Device connection refreshed.".to_string();
                }
                DeviceResponse::Error(e) => {
                    self.busy = false;
                    self.recording_started = None;
                    self.receiving = false;
                    if self.sweep.is_some() {
                        self.sweep = None;
                        self.status = format!("Sweep stopped by error: {e}");
                    } else if let Some(session) = &mut self.multi_pass {
                        // Keep whatever candidates were already captured - just
                        // clear the in-flight marker so the user can retry.
                        session.testing = None;
                        self.status = format!("Error: {e}");
                    } else {
                        self.status = format!("Error: {e}");
                    }
                }
            }
        }
        // keep per-remote text-input state in sync if remotes were added/removed
        self.new_button_names
            .resize(self.remotes.len(), String::new());
    }

    /// Fire the current sweep step once it's due, if the sweep is running
    /// and nothing else is in flight.
    fn drive_sweep(&mut self) {
        let Some(sweep) = &self.sweep else { return };
        if self.busy || sweep.paused || Instant::now() < sweep.next_fire_at {
            return;
        }
        let pair = &sweep.pairs[sweep.pos];
        self.busy = true;
        self.status = sweep.status_line();
        let _ = self.req_tx.send(DeviceRequest::Send {
            remote_index: usize::MAX, // unused for sweep steps - status comes from SweepState, not the button lookup
            button_index: usize::MAX,
            codes: pair.codes.clone(),
            freq_index: pair.freq_index,
        });
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        let is_appimage = updater::appimage_path().is_some();
        ui.group(|ui| {
            ui.label("Automatic updates");
            if ui
                .checkbox(&mut self.auto_update_enabled, "Check for updates on startup, and install automatically")
                .changed()
            {
                let _ = store::save_settings(&store::Settings { auto_update_enabled: self.auto_update_enabled });
            }
            if !is_appimage {
                ui.label(
                    egui::RichText::new("(only takes effect in the packaged AppImage build - nothing to self-update here)")
                        .weak(),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(is_appimage && !self.update_checking, egui::Button::new("Check for Updates"))
                    .on_hover_text("Checks GitHub for a newer release and installs it immediately if found")
                    .clicked()
                {
                    self.update_checking = true;
                    self.status = "Checking for updates...".to_string();
                    let tx = self.update_tx.clone();
                    thread::spawn(move || run_update_check(tx));
                }
                if self.update_checking {
                    ui.spinner();
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label("Configuration");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.remotes.is_empty(), egui::Button::new("Export All"))
                    .on_hover_text("Save every remote to a JSON file under exports/")
                    .clicked()
                {
                    match store::export_all(&self.remotes) {
                        Ok(path) => {
                            self.status = format!("Exported all remotes to {}", path.display())
                        }
                        Err(e) => self.status = format!("Export failed: {e:#}"),
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Import from file:");
                ui.text_edit_singleline(&mut self.import_path);
                if ui.button("Import").clicked() {
                    match store::import(&self.import_path) {
                        Ok(mut imported) => {
                            let n = imported.len();
                            self.remotes.append(&mut imported);
                            self.new_button_names
                                .resize(self.remotes.len(), String::new());
                            let _ = store::save(&self.remotes);
                            self.status =
                                format!("Imported {n} remote(s) from {}.", self.import_path);
                        }
                        Err(e) => self.status = format!("Import failed: {e:#}"),
                    }
                }
            });
            ui.label(
                "Imported remotes are added alongside your existing ones (nothing is overwritten).",
            );
        });
    }

    fn render_about(&self, ui: &mut egui::Ui) {
        const REPO: &str = "https://github.com/Gamedirection/master-ir-blaster";
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.heading("IR Blaster");
            ui.label("by GameDirection");
            ui.add_space(8.0);
            ui.hyperlink_to("Changelog", format!("{REPO}/blob/main/CHANGELOG.md"));
            ui.hyperlink_to("License (MIT)", format!("{REPO}/blob/main/LICENSE"));
            ui.add_space(14.0);

            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("\u{2b50} Star this project on GitHub").size(15.0),
                    )
                    .fill(egui::Color32::from_rgb(36, 41, 46))
                    .min_size(egui::vec2(260.0, 32.0)),
                )
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::same_tab(REPO));
            }
            ui.add_space(6.0);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Buy me a coffee")
                            .color(egui::Color32::BLACK)
                            .size(15.0),
                    )
                    .fill(egui::Color32::from_rgb(255, 221, 0))
                    .min_size(egui::vec2(260.0, 32.0)),
                )
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::same_tab(
                    "https://buymeacoffee.com/gamedirection",
                ));
            }

            ui.add_space(18.0);
            ui.label(egui::RichText::new("Creditation").strong());
            // Plain (non-wrapping) horizontal group so its measured width
            // shrinks to content - `horizontal_wrapped` claims the full
            // available width for wrap detection, which defeats the parent
            // `vertical_centered`'s centering.
            ui.horizontal(|ui| {
                ui.hyperlink_to("Facebook", "https://www.facebook.com/GameDirection");
                ui.hyperlink_to(
                    "Instagram",
                    "https://www.instagram.com/gamedirection_network/",
                );
                ui.hyperlink_to("LinkedIn", "https://www.linkedin.com/company/91366950/");
                ui.hyperlink_to(
                    "YouTube",
                    "https://www.youtube.com/channel/UCLoulV2vXP-XWWIryuggYmg?view_as=subscriber",
                );
                ui.hyperlink_to("X", "https://x.com/gamedirectionus");
                ui.hyperlink_to("Bluesky", "https://bsky.app/profile/gamedirection.net");
            });

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("Credits: Alex Sierputowski @ GameDirection.net").italics(),
            );
            ui.hyperlink_to("gamedirection.net", "https://gamedirection.net");
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_channels();
        self.drive_sweep();
        ctx.request_repaint_after(Duration::from_millis(150));

        egui::TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .default_height(180.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.show_log, "Show debug log");
                    if ui.button("Clear log").clicked() {
                        self.log.clear();
                    }
                });
                if self.show_log {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.log {
                                ui.monospace(line);
                            }
                        });
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Main, "Main");
                ui.selectable_value(&mut self.active_tab, Tab::Settings, "Settings");
                ui.selectable_value(&mut self.active_tab, Tab::About, "About");
            });
            ui.separator();

            match self.active_tab {
                Tab::Settings => self.render_settings(ui),
                Tab::About => self.render_about(ui),
                Tab::Main => {
      ui.heading("IR Blaster");
      ui.horizontal(|ui| {
        ui.label("Device: Tiqiaa TView USB IR transceiver (10c4:8468)");
        if ui.add_enabled(!self.busy, egui::Button::new("Refresh Device"))
          .on_hover_text("Force-reset and reopen the USB connection - try this if the device stops replying")
          .clicked()
        {
          self.busy = true;
          self.status = "Refreshing device connection...".to_string();
          let _ = self.req_tx.send(DeviceRequest::RefreshDevice);
        }
      });
      ui.horizontal(|ui| {
        ui.label("Send carrier frequency:");
        egui::ComboBox::from_id_salt("carrier_freq")
          .selected_text(format!("{}Hz", tiqiaa::CARRIER_FREQUENCIES[self.send_freq_index as usize]))
          .show_ui(ui, |ui| {
            for (i, hz) in tiqiaa::CARRIER_FREQUENCIES.iter().enumerate() {
              ui.selectable_value(&mut self.send_freq_index, i as u8, format!("{hz}Hz"));
            }
          });
        ui.label("(try alternates like 36000Hz if 38000Hz doesn't work - cheap remotes often differ)");
      });
      ui.horizontal(|ui| {
        ui.label("Multi-pass captures:");
        ui.add(egui::DragValue::new(&mut self.multi_pass_count).range(1..=5));
        ui.label("(capture the same button this many times, then test each and keep the one that works)");
      });
      ui.horizontal(|ui| {
        if ui
          .add_enabled(!self.remotes.is_empty(), egui::Button::new("Export All"))
          .on_hover_text("Save every remote to a JSON file under exports/")
          .clicked()
        {
          match store::export_all(&self.remotes) {
            Ok(path) => self.status = format!("Exported all remotes to {}", path.display()),
            Err(e) => self.status = format!("Export failed: {e:#}"),
          }
        }
        ui.label("(per-remote export button is next to each remote's name below)");
      });

      if let Some((pos, total, button_name, hz, paused)) = self.sweep.as_ref().map(|s| {
        (
          s.pos,
          s.pairs.len(),
          s.pairs[s.pos].button_name.clone(),
          tiqiaa::CARRIER_FREQUENCIES[s.pairs[s.pos].freq_index as usize],
          s.paused,
        )
      }) {
        ui.group(|ui| {
          ui.label(format!(
            "Full sweep: step {}/{total} - \"{button_name}\" @ {hz}Hz{}",
            pos + 1,
            if paused { " (PAUSED - watch for a reaction)" } else { "" }
          ));
          ui.add(egui::ProgressBar::new(pos as f32 / total.max(1) as f32));
          ui.horizontal(|ui| {
            if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
              self.sweep.as_mut().unwrap().paused = !paused;
            }
            if ui.add_enabled(paused, egui::Button::new("Back 5")).clicked() {
              let s = self.sweep.as_mut().unwrap();
              s.pos = s.pos.saturating_sub(5);
            }
            if ui.add_enabled(paused, egui::Button::new("Back 1")).clicked() {
              let s = self.sweep.as_mut().unwrap();
              s.pos = s.pos.saturating_sub(1);
            }
            if ui.add_enabled(paused && pos + 1 < total, egui::Button::new("Skip 1")).clicked() {
              let s = self.sweep.as_mut().unwrap();
              s.pos = (s.pos + 1).min(s.pairs.len() - 1);
            }
            if ui.button("Stop Sweep").clicked() {
              self.sweep = None;
              self.busy = false;
              self.status = "Sweep stopped.".to_string();
            }
          });
        });
      } else {
        ui.horizontal(|ui| {
          let can_sweep = !self.busy && self.remotes.iter().any(|r| !r.buttons.is_empty());
          if ui.add_enabled(can_sweep, egui::Button::new("Start Full Sweep"))
            .on_hover_text("Try every recorded button at every known carrier frequency, one at a time - pausable and reversible. Watch the target device the whole time.")
            .clicked()
          {
            let pairs: Vec<SweepPair> = self
              .remotes
              .iter()
              .flat_map(|r| r.buttons.iter())
              .flat_map(|b| {
                (0..tiqiaa::CARRIER_FREQUENCIES.len() as u8).map(move |freq_index| SweepPair {
                  button_name: b.name.clone(),
                  codes: b.codes.clone(),
                  freq_index,
                })
              })
              .collect();
            if !pairs.is_empty() {
              self.status = format!("Starting full sweep: {} combinations...", pairs.len());
              self.sweep = Some(SweepState { pairs, pos: 0, paused: false, next_fire_at: Instant::now() });
            }
          }
          ui.label("(every recorded button × every carrier frequency - pause any time to zero in on a reaction)");
        });
      }
      ui.separator();

      ui.horizontal(|ui| {
        ui.label("New remote name:");
        ui.text_edit_singleline(&mut self.new_remote_name);
        let can_add = !self.new_remote_name.trim().is_empty();
        if ui.add_enabled(can_add, egui::Button::new("+ Add Remote")).clicked() {
          self.remotes.push(Remote { name: self.new_remote_name.trim().to_string(), buttons: Vec::new() });
          self.new_button_names.push(String::new());
          self.new_remote_name.clear();
          let _ = store::save(&self.remotes);
        }
      });

      ui.separator();
      ui.horizontal(|ui| {
        ui.label(&self.status);
        if self.busy {
          ui.spinner();
        }
      });
      ui.horizontal(|ui| {
        if self.recording_started.is_some() {
          ui.label(egui::RichText::new("REC").color(egui::Color32::from_rgb(210, 60, 60)).size(22.0));
          ui.label("Press the button NOW - one quick tap, then release (don't hold; holding tends to only catch a \"still pressed\" repeat ping, not the real command).");
        } else {
          ui.label(egui::RichText::new("IDLE").color(egui::Color32::GRAY).size(22.0));
          ui.label("Idle - click Record to arm the receiver.");
        }
      });
      if let Some(started) = self.recording_started {
        let elapsed = started.elapsed().as_secs_f32();
        let total = RECORD_TIMEOUT.as_secs_f32();
        let fraction = (elapsed / total).clamp(0.0, 1.0);
        let color = if self.receiving {
          egui::Color32::from_rgb(60, 180, 75) // green - device is sending reply bytes
        } else {
          egui::Color32::GRAY // waiting, nothing arriving yet
        };
        let label = if self.receiving {
          format!("Receiving... ({elapsed:.1}s / {total:.0}s)")
        } else {
          format!("Listening... ({elapsed:.1}s / {total:.0}s)")
        };
        ui.add(
          egui::ProgressBar::new(fraction)
            .fill(color)
            .text(label)
            .desired_width(ui.available_width()),
        );
      }

      if !self.busy {
        if self.multi_pass.is_some() {
          // Pull out everything the panel needs to display up front so the
          // closure below never needs to borrow `self` - all resulting
          // actions are applied afterward instead.
          let base_name = self.multi_pass.as_ref().unwrap().base_name.clone();
          let target_passes = self.multi_pass.as_ref().unwrap().target_passes;
          let done = self.multi_pass.as_ref().unwrap().candidates.len();
          let candidates_summary: Vec<(usize, u8)> = self
            .multi_pass
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .map(|c| (c.codes.len(), confidence_score(&c.codes)))
            .collect();
          let testing_active = self.multi_pass.as_ref().unwrap().testing.is_some();

          let mut start_pass = false;
          let mut cancel = false;
          let mut test_index: Option<usize> = None;
          let mut keep_index: Option<usize> = None;
          let mut preview_index: Option<usize> = None;

          ui.group(|ui| {
            if done < target_passes as usize {
              ui.label(format!("Multi-pass capture for \"{base_name}\": {done}/{target_passes} done."));
              ui.horizontal(|ui| {
                if ui.button(format!("Start Pass {}", done + 1)).clicked() {
                  start_pass = true;
                }
                if ui.button("Cancel").clicked() {
                  cancel = true;
                }
              });
            } else {
              ui.label(format!(
                "All {target_passes} passes captured for \"{base_name}\" - test each and keep the one that works:"
              ));
              for (i, (pulses, confidence)) in candidates_summary.iter().enumerate() {
                ui.horizontal(|ui| {
                  let conf_color = if *confidence >= 70 {
                    egui::Color32::from_rgb(60, 180, 75)
                  } else if *confidence >= 40 {
                    egui::Color32::from_rgb(220, 170, 30)
                  } else {
                    egui::Color32::from_rgb(210, 70, 60)
                  };
                  ui.label(format!("Pass {}: {pulses} pulses", i + 1));
                  ui.colored_label(conf_color, format!("{confidence}%"));
                  if ui.add_enabled(!testing_active, egui::Button::new("Test")).clicked() {
                    test_index = Some(i);
                  }
                  if ui.button("Preview").clicked() {
                    preview_index = Some(i);
                  }
                  if ui.add_enabled(!testing_active, egui::Button::new("Keep This")).clicked() {
                    keep_index = Some(i);
                  }
                });
              }
              if ui.button("Discard all, cancel").clicked() {
                cancel = true;
              }
            }
          });

          if cancel {
            self.multi_pass = None;
            self.status = "Multi-pass capture cancelled.".to_string();
          }
          if start_pass {
            let info = self
              .multi_pass
              .as_ref()
              .map(|s| (s.remote_index, s.base_name.clone(), s.candidates.len()));
            if let Some((remote_index, base_name, done)) = info {
              self.busy = true;
              self.recording_started = Some(Instant::now());
              self.receiving = false;
              self.status = format!("Waiting for IR signal - pass {}...", done + 1);
              let name = format!("{base_name} (pass {})", done + 1);
              let _ = self.req_tx.send(DeviceRequest::Record { remote_index, name, overwrite_index: None });
            }
          }
          if let Some(i) = preview_index {
            let codes = self.multi_pass.as_ref().map(|s| s.candidates[i].codes.clone());
            if let Some(codes) = codes {
              self.set_preview(format!("{base_name} - pass {}", i + 1), codes, None);
            }
          }
          if let Some(i) = test_index {
            let info = self.multi_pass.as_ref().map(|s| (s.candidates[i].codes.clone(), s.candidates.len()));
            if let Some((codes, total)) = info {
              self.busy = true;
              self.status = format!("Testing pass {}/{total}...", i + 1);
              if let Some(session) = self.multi_pass.as_mut() {
                session.testing = Some(i);
              }
              let _ = self.req_tx.send(DeviceRequest::Send {
                remote_index: usize::MAX,
                button_index: usize::MAX,
                codes,
                freq_index: self.send_freq_index,
              });
            }
          }
          if let Some(i) = keep_index {
            if let Some(session) = self.multi_pass.take() {
              let codes = session.candidates[i].codes.clone();
              if let Some(remote) = self.remotes.get_mut(session.remote_index) {
                match session.overwrite_index {
                  Some(bi) if bi < remote.buttons.len() => remote.buttons[bi].codes = codes,
                  _ => remote.buttons.push(Button { name: session.base_name.clone(), codes, color: None }),
                }
                let _ = store::save(&self.remotes);
              }
              self.status = format!("Kept pass {} as \"{}\".", i + 1, session.base_name);
            }
          }
        }
      }

      if !self.preview_codes.is_empty() {
        ui.separator();
        ui.label(format!("Waveform: {}", self.preview_label));
        draw_waveform(ui, &self.preview_codes, Some((self.crop_start, self.crop_end.saturating_sub(self.crop_start))));

        if let Some(cs) = self.crop_suggestion {
          ui.label(format!(
            "Detected a repeating pattern: pulses {}..{} repeat starting at {} - likely a resend or trailing noise.",
            cs.start, cs.start + cs.len, cs.repeat_start
          ));
        } else {
          ui.label("No repeating pattern detected - drag the crop range below if you still want to trim it.");
        }

        let max_index = self.preview_codes.len();
        ui.horizontal(|ui| {
          ui.label("Crop start:");
          let mut start = self.crop_start;
          if ui.add(egui::DragValue::new(&mut start).range(0..=self.crop_end.saturating_sub(1))).changed() {
            self.crop_start = start;
          }
          ui.label("Crop end:");
          let mut end = self.crop_end;
          if ui.add(egui::DragValue::new(&mut end).range((self.crop_start + 1)..=max_index)).changed() {
            self.crop_end = end;
          }
          ui.label(format!("({} of {} pulses selected)", self.crop_end - self.crop_start, max_index));

          if ui.button("Reset to full").clicked() {
            self.crop_start = 0;
            self.crop_end = max_index;
          }
          if self.crop_suggestion.is_some() && ui.button("Reset to detected").clicked() {
            if let Some(cs) = self.crop_suggestion {
              self.crop_start = cs.start;
              self.crop_end = cs.start + cs.len;
            }
          }

          let can_apply = self.preview_location.is_some() && self.crop_end > self.crop_start && self.crop_end - self.crop_start < max_index;
          if ui.add_enabled(can_apply, egui::Button::new("Apply Crop")).clicked() {
            if let Some((r, b)) = self.preview_location {
              let cropped: Vec<i32> = self.preview_codes[self.crop_start..self.crop_end].to_vec();
              if let Some(button) = self.remotes.get_mut(r).and_then(|remote| remote.buttons.get_mut(b)) {
                button.codes = cropped.clone();
              }
              let _ = store::save(&self.remotes);
              self.status = "Cropped to the selected range.".to_string();
              let label = self.preview_label.clone();
              self.set_preview(label, cropped, Some((r, b)));
            }
          }
        });
      }
      ui.separator();

      egui::ScrollArea::vertical().show(ui, |ui| {
        let mut delete_remote: Option<usize> = None;
        let mut delete_button: Option<(usize, usize)> = None;
        let mut preview: Option<(String, Vec<i32>, usize, usize)> = None;
        let mut rerecord: Option<(usize, usize, String)> = None;
        let mut reorder_button: Option<(usize, usize, usize)> = None; // (remote, from, to)
        let mut rename_remote_commit: Option<(usize, String)> = None;
        let mut rename_button_commit: Option<(usize, usize, String)> = None;
        let mut set_color: Option<(usize, usize, Option<[u8; 3]>)> = None;

        for r in 0..self.remotes.len() {
          let remote_name = self.remotes[r].name.clone();
          let collapsing_id = ui.make_persistent_id(("remote_hdr", r));
          let header = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, true).show_header(ui, |ui| {
            if self.renaming_remote == Some(r) {
              let resp = ui.add(
                egui::TextEdit::singleline(&mut self.rename_buffer).id(egui::Id::new(("rename_remote", r))),
              );
              resp.request_focus();
              let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
              let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
              if enter {
                rename_remote_commit = Some((r, self.rename_buffer.clone()));
                self.renaming_remote = None;
              } else if escape || resp.lost_focus() {
                // clicked away / escape - discard the edit, don't save
                self.renaming_remote = None;
              }
            } else if ui
              .add(egui::Label::new(&remote_name).sense(egui::Sense::click()))
              .on_hover_text("Click to rename")
              .clicked()
            {
              self.renaming_remote = Some(r);
              self.rename_buffer = remote_name.clone();
            }
          });
          header.body(|ui| {
            ui.horizontal(|ui| {
              ui.label("New button name:");
              ui.text_edit_singleline(&mut self.new_button_names[r]);
              let can_record = !self.busy && !self.new_button_names[r].trim().is_empty();
              if ui.add_enabled(can_record, egui::Button::new("Record"))
                .on_hover_text("Point the remote at the receiver and press a button within 15s")
                .clicked()
              {
                let name = self.new_button_names[r].trim().to_string();
                let existing = self.remotes[r].buttons.iter().position(|b| b.name == name);
                self.busy = true;
                self.recording_started = Some(Instant::now());
                self.receiving = false;
                self.status = match existing {
                  Some(_) => format!("Waiting for IR signal to overwrite \"{name}\"..."),
                  None => "Waiting for IR signal (aim remote, press a button)...".to_string(),
                };
                let _ = self.req_tx.send(DeviceRequest::Record {
                  remote_index: r,
                  name,
                  overwrite_index: existing,
                });
                self.new_button_names[r].clear();
              }
              let can_multi = !self.busy && self.multi_pass.is_none() && !self.new_button_names[r].trim().is_empty();
              if ui.add_enabled(can_multi, egui::Button::new("Multi-pass"))
                .on_hover_text("Capture this button several times, then test each and keep the working one")
                .clicked()
              {
                let base_name = self.new_button_names[r].trim().to_string();
                self.multi_pass = Some(MultiPassSession {
                  remote_index: r,
                  overwrite_index: None,
                  base_name,
                  target_passes: self.multi_pass_count,
                  candidates: Vec::new(),
                  testing: None,
                });
                self.new_button_names[r].clear();
              }
              if ui
                .button("Export")
                .on_hover_text(format!("Save just \"{remote_name}\" to a JSON file under exports/"))
                .clicked()
              {
                match store::export_remote(&self.remotes[r]) {
                  Ok(path) => self.status = format!("Exported \"{remote_name}\" to {}", path.display()),
                  Err(e) => self.status = format!("Export failed: {e:#}"),
                }
              }
              if ui.add_enabled(!self.busy, egui::Button::new("Delete Remote")).clicked() {
                delete_remote = Some(r);
              }
            });

            for b in 0..self.remotes[r].buttons.len() {
              let button = &self.remotes[r].buttons[b];
              let button_name = button.name.clone();
              let pulse_count = button.codes.len();
              let button_color = button.color;
              let bg = button_color
                .map(|c| egui::Color32::from_rgb(c[0], c[1], c[2]))
                .unwrap_or(egui::Color32::TRANSPARENT);
              egui::Frame::none().fill(bg).inner_margin(3.0).show(ui, |ui| {
              ui.horizontal(|ui| {
                let mut swatch = button_color.unwrap_or([120, 120, 120]);
                if ui.color_edit_button_srgb(&mut swatch).on_hover_text("Pick a background color for this button").changed() {
                  set_color = Some((r, b, Some(swatch)));
                }
                if button_color.is_some() && ui.small_button("x").on_hover_text("Clear color").clicked() {
                  set_color = Some((r, b, None));
                }

                let drag_id = egui::Id::new(("btn_drag", r, b));
                let drag_resp = ui.dnd_drag_source(drag_id, b, |ui| {
                  ui.label("::");
                }).response;
                drag_resp.clone().on_hover_text("Drag to reorder");
                if let Some(_hover) = drag_resp.dnd_hover_payload::<usize>() {
                  let rect = drag_resp.rect;
                  if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let y = if pointer.y < rect.center().y { rect.top() } else { rect.bottom() };
                    ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(2.0_f32, egui::Color32::YELLOW));
                  }
                  if let Some(dragged) = drag_resp.dnd_release_payload::<usize>() {
                    reorder_button = Some((r, *dragged, b));
                  }
                }

                let confidence = confidence_score(&button.codes);
                let conf_color = if confidence >= 70 {
                  egui::Color32::from_rgb(60, 180, 75)
                } else if confidence >= 40 {
                  egui::Color32::from_rgb(220, 170, 30)
                } else {
                  egui::Color32::from_rgb(210, 70, 60)
                };
                ui.colored_label(conf_color, format!("{confidence}%"))
                  .on_hover_text("Confidence this is a real captured code vs. noise/incomplete - based on pulse count, leader burst, and pulse-width spread");

                if self.renaming_button == Some((r, b)) {
                  let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_buffer).id(egui::Id::new(("rename_button", r, b))),
                  );
                  resp.request_focus();
                  let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                  let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                  if enter {
                    rename_button_commit = Some((r, b, self.rename_buffer.clone()));
                    self.renaming_button = None;
                  } else if escape || resp.lost_focus() {
                    // clicked away / escape - discard the edit, don't save
                    self.renaming_button = None;
                  }
                } else if ui
                  .add({
                    let text = format!("{button_name} ({pulse_count} pulses)");
                    let rich = match button_color {
                        Some(c) => egui::RichText::new(text).color(contrasting_text_color(c)),
                        None => egui::RichText::new(text),
                    };
                    egui::Label::new(rich).sense(egui::Sense::click())
                  })
                  .on_hover_text("Click to rename")
                  .clicked()
                {
                  self.renaming_button = Some((r, b));
                  self.rename_buffer = button_name.clone();
                }

                if ui.add_enabled(!self.busy, egui::Button::new("Send")).clicked() {
                  self.busy = true;
                  self.status = format!("Sending \"{button_name}\"...");
                  let _ = self.req_tx.send(DeviceRequest::Send {
                    remote_index: r,
                    button_index: b,
                    codes: button.codes.clone(),
                    freq_index: self.send_freq_index,
                  });
                }
                if ui.button("Preview").clicked() {
                  preview = Some((format!("{remote_name} / {button_name}"), button.codes.clone(), r, b));
                }
                if ui.add_enabled(!self.busy, egui::Button::new("Re-record"))
                  .on_hover_text("Overwrite this button with a new capture")
                  .clicked()
                {
                  rerecord = Some((r, b, button_name.clone()));
                }
                if ui.add_enabled(!self.busy && self.multi_pass.is_none(), egui::Button::new("Multi-pass"))
                  .on_hover_text("Capture this button several times, then test each and keep the working one")
                  .clicked()
                {
                  self.multi_pass = Some(MultiPassSession {
                    remote_index: r,
                    overwrite_index: Some(b),
                    base_name: button_name.clone(),
                    target_passes: self.multi_pass_count,
                    candidates: Vec::new(),
                    testing: None,
                  });
                }
                if ui.add_enabled(!self.busy, egui::Button::new("Delete")).clicked() {
                  delete_button = Some((r, b));
                }
              });
              });
            }
          });
        }

        if let Some((r, b, color)) = set_color {
          if let Some(button) = self.remotes.get_mut(r).and_then(|remote| remote.buttons.get_mut(b)) {
            button.color = color;
            let _ = store::save(&self.remotes);
          }
        }
        if let Some((r, name)) = rename_remote_commit {
          let trimmed = name.trim();
          if !trimmed.is_empty() {
            if let Some(remote) = self.remotes.get_mut(r) {
              remote.name = trimmed.to_string();
              let _ = store::save(&self.remotes);
            }
          }
        }
        if let Some((r, b, name)) = rename_button_commit {
          let trimmed = name.trim();
          if !trimmed.is_empty() {
            if let Some(button) = self.remotes.get_mut(r).and_then(|remote| remote.buttons.get_mut(b)) {
              button.name = trimmed.to_string();
              let _ = store::save(&self.remotes);
            }
          }
        }
        if let Some((r, from, to)) = reorder_button {
          if let Some(remote) = self.remotes.get_mut(r) {
            if from != to && from < remote.buttons.len() {
              let item = remote.buttons.remove(from);
              let to = to.min(remote.buttons.len());
              remote.buttons.insert(to, item);
              let _ = store::save(&self.remotes);
            }
          }
        }
        if let Some((label, codes, r, b)) = preview {
          self.set_preview(label, codes, Some((r, b)));
        }
        if let Some((r, b, name)) = rerecord {
          self.busy = true;
          self.recording_started = Some(Instant::now());
          self.receiving = false;
          self.status = format!("Waiting for IR signal to overwrite \"{name}\"...");
          let _ = self.req_tx.send(DeviceRequest::Record {
            remote_index: r,
            name,
            overwrite_index: Some(b),
          });
        }
        if let Some((r, b)) = delete_button {
          if let Some(remote) = self.remotes.get_mut(r) {
            if b < remote.buttons.len() {
              let removed = remote.buttons.remove(b);
              self.status = format!("Deleted \"{}\".", removed.name);
              let _ = store::save(&self.remotes);
            }
          }
        }
        if let Some(r) = delete_remote {
          if r < self.remotes.len() {
            let removed = self.remotes.remove(r);
            if r < self.new_button_names.len() {
              self.new_button_names.remove(r);
            }
            self.status = format!("Deleted remote \"{}\".", removed.name);
            let _ = store::save(&self.remotes);
          }
        }
      });
                }
            }
    });
    }
}

/// egui's own default fonts have no emoji/symbol coverage, so any emoji typed
/// into a button/remote name (or used in our own labels) would render as a
/// blank box. Layer in whatever emoji-capable fonts this system actually has
/// - egui's rasterizer only handles traditional vector outline glyphs, not
/// colored bitmap/COLR fonts, so a monochrome symbols font is included as a
/// fallback for anything the color font can't display.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: &[(&str, &str)] = &[
        (
            "noto_color_emoji",
            "/usr/share/fonts/noto/NotoColorEmoji.ttf",
        ),
        (
            "noto_sans_symbols2",
            "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        ),
    ];
    for (name, path) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert((*name).to_owned(), egui::FontData::from_owned(bytes).into());
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push((*name).to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../img/fc_bk.png");
    let image = image::load_from_memory(bytes)
        .expect("bundled icon is valid PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(load_icon())
            // On Wayland/KDE the window decoration often resolves its titlebar
            // icon by matching this app-id against an installed .desktop file
            // rather than using the raw icon pixels directly.
            .with_app_id("ir-blaster"),
        ..Default::default()
    };
    eframe::run_native(
        "IR Blaster",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

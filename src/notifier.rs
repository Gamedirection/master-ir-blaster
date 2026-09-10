use chrono::{Duration as ChronoDuration, Local};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Timestamp of the last "device missing" notification, shared between the
/// background presence watcher and the device worker so both respect the
/// same once-a-day throttle.
pub type Throttle = Arc<Mutex<Option<chrono::DateTime<Local>>>>;

pub fn new_throttle() -> Throttle {
    Arc::new(Mutex::new(None))
}

const MIN_INTERVAL_HOURS: i64 = 24;
const POLL_INTERVAL: Duration = Duration::from_secs(300);

/// Sends the "IR transmitter not detected" desktop notification.
///
/// Passive background checks (`force: false`) are throttled to at most once
/// every 24 hours. An actual attempted Record/Send that just failed because
/// the device is missing (`force: true`) always notifies immediately - the
/// user is actively trying to use it right now - and also resets the
/// throttle window so the background watcher doesn't repeat it right after.
pub fn notify_device_missing(throttle: &Throttle, force: bool) {
    {
        let mut last = throttle.lock().unwrap();
        let now = Local::now();
        if !force {
            if let Some(prev) = *last {
                if now - prev < ChronoDuration::hours(MIN_INTERVAL_HOURS) {
                    return;
                }
            }
        }
        *last = Some(now);
    }
    let _ = notify_rust::Notification::new()
        .summary("IR Blaster")
        .body("IR transmitter not detected - is it plugged in?")
        .icon("ir-blaster")
        .timeout(notify_rust::Timeout::Milliseconds(8000))
        .show();
}

/// Background thread: periodically checks whether the device is present and
/// fires a throttled notification if not. Re-reads `settings.json` on every
/// tick (rather than needing a channel from the GUI thread) so toggling the
/// setting takes effect without a restart.
pub fn spawn_presence_watcher(throttle: Throttle) {
    thread::spawn(move || loop {
        let settings = crate::store::load_settings();
        if settings.notify_device_missing && !crate::tiqiaa::is_present() {
            notify_device_missing(&throttle, false);
        }
        thread::sleep(POLL_INTERVAL);
    });
}

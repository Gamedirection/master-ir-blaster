# Changelog

## [0.1.0] - 2026-09-08

Initial release.

### Core

- USB communication with the Tiqiaa TView IR transceiver (`10c4:8468`) over raw bulk transfers, implementing its packet framing (fragmented reports, ST/EN-wrapped commands).
- Record and Send commands, with a tick-run-length encoder/decoder for the device's mark/space byte format.
- egui/eframe desktop GUI: remotes containing named buttons, each recordable and sendable independently.
- Local JSON storage (`remotes.json`) alongside the project, loaded on startup and saved on every change.

### Reliability fixes (found through extensive real-hardware testing)

- Added a udev rule so the device is usable without root.
- Fixed a libusb quirk where a near-zero timeout gets truncated to 0ms, which libusb treats as "wait forever" instead of "return immediately" — this caused the app to hang indefinitely under certain timing conditions.
- Discovered the device doesn't reliably ack every command (mode switches especially); switched those to a "fire and best-effort drain" pattern instead of a strict request/reply, matching the behavior of the community C reference driver.
- Discovered that repeatedly re-issuing the `Output` request (polling) corrupts the device's internal state — every write after the first hangs for a flat 2s and never recovers for the rest of the session. Switched recording to a single `Output` request with one long wait instead.
- Added a "Refresh Device" action (USB reset + full close/reopen) and automatic endpoint `clear_halt` on open, to recover a device left in a bad state by a prior ungraceful shutdown.
- Fixed the real root cause of most "nothing happens" reports: the receiver doesn't cleanly demodulate the IR carrier, so a raw capture is mostly un-merged ~32µs carrier ripple. Added carrier-removal logic (coalescing any short burst, mark or space, into one continuous mark) so captures reflect the actual signal envelope.
- Cross-verified the protocol implementation (USB framing, 30-entry carrier-frequency table, 16µs tick size) against the official vendor Android app's decompiled/disassembled native code — all matched byte-for-byte (see `phone-app-report.md`).

### Features

- Live waveform preview during recording, including every rejected/noisy attempt, so you can see what the receiver is actually picking up.
- A "press the button NOW" indicator (grey when idle, red while armed and listening) with recommended technique (quick tap, not a hold).
- Crop tool: detects a repeating/noisy tail in a capture and suggests (or lets you manually drag) a crop range to trim to just the clean signal.
- Multi-pass capture: record the same button N times (1-5, configurable), then test each capture individually against the real device and keep only the one that works.
- Confidence score (0-100%) heuristic shown per button, based on pulse count, leading-burst presence, and pulse-width spread.
- Carrier-frequency sweep across all 30 known frequencies, pausable and reversible (step back to re-check a promising range), for remotes that don't use the common 38kHz default.
- Drag-to-reorder buttons within a remote (via a drag handle), click-to-rename any remote or button (Enter saves, clicking away or Escape cancels).
- Per-button color-coded backgrounds via a color picker, with automatic black/white text contrast based on the chosen color's brightness.
- Export all remotes, or a single remote, to a timestamped JSON file under `exports/`.
- Debug log panel showing every raw USB exchange live, plus a persistent log file for troubleshooting.
- Desktop launcher entry so the app shows up in the system application search.

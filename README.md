<p align="center">
  <img src="img/fc_bk.png" alt="IR Blaster logo" width="150">
</p>

<h1 align="center">IR Blaster</h1>

<p align="center">by <a href="https://gamedirection.net">GameDirection</a></p>

IR Blaster is a Rust and egui desktop app. It records and replays infrared remote signals. It uses a Tiqiaa TView USB IR transceiver (USB ID `10c4:8468`). Sellers also sell this transceiver under other names, such as ZaZaRemote and ElkSmart.

Point the dongle at a remote control. Record a button press. Replay the signal later to control the device the remote controls, such as an RGB light or a TV.

![Main view](img/screenshots/Main.png)

## Features

- **Record and Send**: record and send individual buttons. Group buttons into named remotes. Drag a button to reorder it, or click its name to rename it.
- **Live waveform preview**: the app shows a live waveform while it listens for a signal. A "press now" indicator tells you when to press the remote button.

  ![Recording a signal](img/screenshots/Rec.png)

- **Crop tool**: the app detects repeating or noisy sections in a capture. Trim the capture to keep only the real signal. Drag the crop range, or accept the app's suggestion.

  ![Waveform and crop tool](img/screenshots/Waveform.png)

- **Multi-pass capture**: record the same button several times. Test each capture on its own. Keep only the capture that works.
- **Confidence score**: a score from 0 to 100 percent. It estimates whether a capture is a real signal, noise, or an incomplete capture.
- **Carrier frequency sweep**: cycle a signal through all 30 known carrier frequencies. Use this to find the right frequency for a remote that does not use the common 38kHz default. You can pause the sweep or reverse it.
- **Color-coded rows**: pick a background color for each button. The app picks a matching text color for contrast. Use colors to group buttons visually.
- **Export and Import**: save all remotes, or one remote, to a JSON file. Import the file again later.
- **Auto-update**: turn this setting on in Settings, and the app checks GitHub for a newer release at startup. The app installs a new release automatically. Restart the app to finish the update. You can also click the "Check for Updates" button in Settings at any time. Auto-update works only in the packaged AppImage build.
- **Minimize to tray, start at login, and run hidden**: the app can run in the background with a system tray icon. Use the tray icon to show the window or to quit the app. You can turn on automatic launch at login. You can start the app hidden. The app allows only one running instance. If you launch the app again, it shows the existing window instead of opening a new one.
- **Device-missing notifications**: turn on this setting to get a desktop notification when the app cannot detect the IR transmitter. A passive check sends at most one notification per day. If you try to record or send and the device is missing, the app sends a notification right away.
- **Refresh Device**: click this button to force a USB reset and reopen the connection, if the dongle stops responding.
- **Debug log panel**: the app shows every raw USB exchange live. Use this panel for troubleshooting.

## Requirements

- Linux (with `libusb-1.0`), Windows 10/11, or macOS 11 or later (Apple Silicon).
- Rust (stable toolchain). You need Rust only if you build the app from source. The packaged builds need nothing but the dongle (plus, on Windows, the one-time driver step below).
- A Tiqiaa TView-compatible USB IR transceiver (`10c4:8468`).

## Getting the app

### Linux

To install the packaged AppImage:

1. Download the latest `IR-Blaster-x86_64.AppImage` file (or the `aarch64` build, on ARM64) from the [Releases page](https://github.com/Gamedirection/master-ir-blaster/releases).
2. Mark the file executable and run it:

```sh
chmod +x IR-Blaster-x86_64.AppImage
./IR-Blaster-x86_64.AppImage
```

### Windows

1. Download the latest `IR-Blaster-windows-x86_64.zip` from the [Releases page](https://github.com/Gamedirection/master-ir-blaster/releases).
2. Extract it, then follow the one-time driver setup in the "Windows setup" section below before running `IR-Blaster.exe`.

### macOS

1. Download the latest `IR-Blaster-macos-aarch64.dmg` from the [Releases page](https://github.com/Gamedirection/master-ir-blaster/releases).
2. Open the `.dmg` and drag **IR Blaster** into Applications.
3. This build is not code-signed or notarized yet, so the first launch needs one extra step - see the "macOS setup" section below.

### Building from source (any platform)

```sh
cargo build --release
./target/release/ir-blaster
```

## Setup

### Linux

The app needs a udev rule to access the device without root permission.

Before you create the rule, check your user's group. If your user is not in the `users` group, change `GROUP="users"` in the rule below to your own group.

1. Create the file `/etc/udev/rules.d/99-tiqiaa-ir.rules`. Add this rule:

```
SUBSYSTEM=="usb", ATTRS{idVendor}=="10c4", ATTRS{idProduct}=="8468", MODE="0660", GROUP="users"
```

2. Reload udev and reconnect the device. Run these commands:

```sh
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb
```

### Windows

Windows needs a WinUSB driver bound to the device before the app can see it - a one-time step per machine, similar to the Linux udev rule above. Full steps are in `packaging/windows/README-first.txt` (also included in the downloaded zip):

1. Plug in the IR transceiver.
2. Install [Zadig](https://zadig.akeo.ie), then use it to bind the WinUSB driver to the Tiqiaa/TView device (`VID_10C4&PID_8468`) - not any other device in the list.
3. Run `IR-Blaster.exe`.

If the app still reports the device as not found afterward, unplug and replug the transceiver and try again.

### macOS

This build is not code-signed or notarized yet, so Gatekeeper blocks a normal double-click the first time. Full steps are in `packaging/macos/README-first.txt` (also included in the `.dmg`):

1. Right-click (or Control-click) **IR Blaster.app** in Applications and choose Open.
2. Click Open in the dialog that appears. You only need to do this once.

Or from a terminal: `xattr -cr "/Applications/IR Blaster.app"`, then open it normally.

## Usage notes

### Recording a button

1. Click **Record**.
2. Wait for the "Press the button now" indicator.
3. Give the remote one quick tap. Do not hold the button down. A held button usually sends only the remote's repeat signal, not the real command.

### Troubleshooting

- If a capture looks noisy or incomplete, check its confidence score and its waveform. Use the crop tool to trim the capture. Or use Multi-pass to record several tries and keep the one that works.
- If Send does nothing, try the frequency sweep. Also check that the capture looks like a real signal: it should have a clear leading burst and pulses of different widths, not a short repeating ping.
- If the device stops responding, click **Refresh Device** first. If that does not fix it, unplug the device and plug it back in.

## Data storage

The app stores remotes, exports, and settings in the standard per-OS location: `~/.local/share/ir-blaster/` on Linux, `%APPDATA%\ir-blaster\` on Windows, `~/Library/Application Support/ir-blaster/` on macOS. Older Linux builds stored `remotes.json` next to the source tree instead. The app migrates this old file automatically the first time you run a newer build.

## Protocol notes

The vendor does not document this device's protocol. The community reverse-engineered it instead. See `src/tiqiaa.rs` for references to this earlier work.

Real hardware units of this device behave in unreliable ways that other drivers do not fully handle:

- Mode-switch commands do not always send an acknowledgment.
- A single `Output` request returns whatever data is currently buffered. It does not wait for a real button press.
- The receiver does not fully demodulate the IR carrier. It sends raw carrier-rate ripple during a burst, instead of one clean mark.

This app works around each of these issues. See the comments in `src/tiqiaa.rs` for the details.

This project cross-checked its protocol implementation against the official vendor Android app (ZaZaRemote, package `com.tiqiaa.remote`) through decompilation and disassembly. The USB framing, the carrier-frequency table, and the tick constant all matched.

# Cross-platform packaging plan

## Status

Phase 1 (Linux ARM64) is done: see `.github/workflows/appimage.yml`, a
matrix build (`ubuntu-22.04` x86_64 + `ubuntu-24.04-arm` aarch64) with a
separate `release` job that attaches every matrix artifact to one GitHub
Release on tags.

Phase 2 (Windows x86_64) is mostly done, with one deliberate scope trim:

- Done: `Cargo.toml` restructured with target-conditional dependencies
  (`ksni`/`rumqttc` Linux-only, `tray-icon`/`winreg` Windows/macOS,
  `rusb`'s `vendored` feature on Windows/macOS so no system libusb install
  is needed there - confirmed by fully cross-compiling and linking a real
  Windows .exe locally via mingw before this ever touched CI).
- Done: `src/tray/`, `src/autostart/`, `src/single_instance.rs` (now a
  cross-platform loopback-TCP lock, no `#[cfg]` needed), `store::data_dir()`
  (via the `directories` crate), `setup_fonts()`, the XWayland hack, and
  Reactive Integrations (`teams.rs`) are all cross-platform-gated per the
  plan below.
- Done: CI builds a Windows x86_64 release and packages it as a portable
  zip; `packaging/windows/README-first.txt` documents the one-time WinUSB
  driver step (also in the README's "Windows setup" section).
- Scope trim: no MSI installer yet. `cargo-wix`'s default template installs
  per-machine (needs admin elevation) and WiX itself can't be exercised at
  all outside a real Windows environment, so hand-customizing it for a
  per-user install would have been untested, unverifiable XML. Shipping the
  already-verified portable zip now and adding the MSI as a fast-follow
  (once there's a real Windows CI run to iterate against) was the better
  risk trade.
- Not done: the "distinguish not-plugged-in from no-driver-installed"
  in-app detection originally planned turned out to not be reliably
  possible - libusb's Windows backend can only see WinUSB-bound devices at
  all, so a device with no driver looks identical to "not plugged in" to
  `rusb`. The device-not-found message is Windows-specific instead,
  leading with the driver as the likely cause rather than claiming a
  distinction that isn't actually detectable.

Phase 3 (macOS, Apple Silicon arm64) is done, with one important caveat:

- Done: CI builds an `IR Blaster.app` bundle (Info.plist generated from
  `packaging/macos/Info.plist` with the version substituted in, `.icns`
  built via `iconutil` from the same 250x250 source PNG the other
  platforms use - the largest icon sizes are upscaled from that and will
  look soft, a known limitation of the current source art, not a
  packaging bug) and wraps it in a `.dmg` via `hdiutil`, alongside a
  `README-first.txt` covering the Gatekeeper workaround (also in the
  README's "macOS setup" section). Resizing for the `.iconset` uses `sips`
  (built into macOS) - the first real CI run caught that ImageMagick,
  unlike on the Linux/Windows runners, isn't actually preinstalled here.
- Done: `src/autostart/macos.rs` (a LaunchAgent plist, loaded/unloaded
  immediately via `launchctl bootstrap`/`bootout` so the Settings toggle
  doesn't need a logout) and `src/tray/desktop.rs` already cover macOS via
  the same `cfg(any(windows, macos))` gate used for Windows - see Phase 2.
- Not verified locally, unlike Phase 2: Apple's toolchain can't be
  cross-compiled to from Linux without a full osxcross setup (and Apple's
  SDK has real redistribution restrictions, so that wasn't set up for
  this). `ring` (pulled in transitively via `ureq`) fails immediately when
  cross-compiling its C code without a macOS SDK, so not even `cargo
  check` completes locally for this target. Everything in this phase is
  written against the platform APIs' documented behavior and Phase 2's
  already-proven cfg-gating pattern, but the real GitHub-hosted `macos-14`
  CI run is the first actual test of any of it - the same position Linux
  ARM64 (Phase 1) was in, since QEMU/cross-compilation wasn't attempted
  there either.
- Still unverified either way, since it needs a real Mac or Apple
  Developer docs access to check: whether `egui`/`ab_glyph` can load a
  face out of Apple Color Emoji's `.ttc` file for `setup_fonts()`. If it
  can't, the existing "missing file -> skip" fallback means macOS just
  gets no color emoji, not a crash.

Phase 4 (Windows ARM64, macOS Intel x86_64) is done: two new matrix rows
(`windows-11-arm` / `aarch64-pc-windows-msvc`, `macos-13` /
`x86_64-apple-darwin`), no code or packaging-step changes needed - both
reused every cross-platform module and CI step from Phases 2-3 unchanged,
confirming the plan's expectation that these would be pure CI-matrix
additions.

All six planned platform/arch combinations now build successfully in CI:
Linux x86_64 and ARM64, Windows x86_64 and ARM64, macOS Apple Silicon and
Intel.

## Remaining follow-ups

- An MSI installer for Windows (currently a portable zip only - see
  Phase 2's scope-trim note above).
- Code signing and macOS notarization (currently shipping unsigned on
  both platforms - a budget/priority decision for the project owner, not
  a technical blocker).
- Verifying the Apple Color Emoji `.ttc` question and the WinUSB
  driver-detection message in practice, on real hardware.

## Context

IR Blaster currently ships only as a Linux x86_64 AppImage. The goal is to
add Windows (installer + portable zip), macOS (.dmg), and ARM64 builds for
all three OSes, alongside the existing Linux x86_64 build, so the app can
be used on more of the user's machines (and shared more broadly).

The codebase was built Linux-first with no platform abstraction: the tray
icon (`ksni`, D-Bus StatusNotifierItem), autostart (`~/.config/autostart`),
single-instance lock (a Unix domain socket), and a Wayland-specific
XWayland-forcing hack are all unconditional Linux/Unix code today. One
feature, Reactive Integrations (Teams status -> light color), depends on
`teams-for-linux`'s local MQTT publisher, which has no Windows/macOS
equivalent at all. Getting to six target builds (3 OS x 2 arch) means
making this code conditionally-compiled per platform, then extending CI
from one Linux-only job into a matrix.

## Resolved decisions

- **Code signing**: ship unsigned Windows/macOS builds first. CI signing
  steps are `if:`-gated on secrets being present, so signing turns on
  later just by adding certs/an Apple Developer account, no workflow
  rewrite needed.
- **ARM64 scope**: build all three (Linux, Windows, macOS ARM64).
- **Reactive Integrations (Teams via MQTT)**: compile it out entirely on
  Windows/macOS (`#[cfg(target_os = "linux")]`) rather than a permanently
  greyed-out toggle. No Settings section for it appears on those builds.
- **App identity**: macOS bundle id `com.gamedirection.irblaster`, Windows
  installer publisher "GameDirection".
- **Self-update** (`updater.rs`): stays Linux-AppImage-only. Windows/macOS
  users re-download new versions from Releases manually; no in-app
  auto-update for those platforms in this plan's scope.

## Current state (as of Phase 1)

- `Cargo.toml`: one flat `[dependencies]` block, nothing target-conditional.
- Only one existing `#[cfg(unix)]` in the whole codebase
  (`src/updater.rs:95`, setting the exec bit on a downloaded AppImage).
  Everything else Linux-specific is unconditional code that will not
  compile, or will silently misbehave, off Linux:
  - `src/single_instance.rs`: unconditional `std::os::unix::net` - hard
    compile failure on Windows today.
  - `src/autostart.rs` + `store::data_dir()`: hand-rolled `$HOME` path
    building, no Windows/macOS equivalents.
  - `src/main.rs` `setup_fonts()`: hardcoded Linux font paths for
    emoji/symbol fallback.
  - `src/main.rs` main(): unconditional `WAYLAND_DISPLAY` removal
    (harmless elsewhere, but should be explicit).
  - `src/tray.rs`: `ksni`-based, Linux/D-Bus only, no abstraction.
  - `src/teams.rs`: Linux-only by nature (see Resolved decisions).
- Icons: `img/*.png` (250x250) + matching `.svg`, no `.ico`/`.icns` yet.
- Windows driver reality: `rusb`/libusb needs a WinUSB driver bound to
  VID 0x10c4 / PID 0x8468 via Zadig or a bundled `.inf` - this is a real,
  unavoidable one-time manual step for Windows users (see Phase 2).

## Implementation plan

### Phase 1: Linux ARM64 (done)

- Added a second matrix row (`ubuntu-24.04-arm`, target
  `aarch64-unknown-linux-gnu`) to the CI workflow.
- Parameterized the AppDir/linuxdeploy/appimagetool steps by arch:
  `linuxdeploy`/`appimagetool` ship arch-specific binaries
  (`-x86_64`/`-aarch64`), so the download step picks the right one per
  `matrix.arch`. Artifact is `IR-Blaster-aarch64.AppImage`.
- Cache key includes the target triple so the two Linux rows don't
  collide on one cache entry.
- Split the release-upload step into a separate `release` job
  (`needs: build`, `if: startsWith(github.ref, 'refs/tags/v')`) that
  downloads all matrix artifacts and calls
  `softprops/action-gh-release@v2` once - needed once there's more than
  one upload racing to attach to the same release.
- No `src/` changes in this phase.

### Phase 2: Windows x86_64 (not started)

**Cargo.toml**: move platform-specific deps into
`[target.'cfg(...)'.dependencies]` blocks:
- `ksni` -> Linux only.
- `rumqttc` -> Linux only (feeds `teams.rs`, which is Linux-only per the
  resolved decision).
- Add `tray-icon = "0.19"` for `cfg(any(windows, macos))`.
- Add `winreg = "0.52"` for `cfg(windows)` (autostart registry key).
- Add `directories = "5"` (unconditional) to replace hand-rolled
  `$HOME`/`%APPDATA%` path logic.

**`src/tray.rs` -> `src/tray/{mod,linux,desktop}.rs`**: split into a
directory module. `mod.rs` re-exports `spawn(ctx, icon) -> Result<()>` and
`show_window(ctx)` per-OS, so `main.rs` and `single_instance.rs` don't
change how they call into it. `linux.rs` is the current `ksni` code moved
as-is. `desktop.rs` is a new `tray-icon`-based implementation for
Windows/macOS: simpler icon handling than `ksni` (plain RGBA, no ARGB byte
rotation needed), two menu items (Show, Quit). On Windows, `tray-icon`'s
Win32 backend needs its own message pump - run it on a dedicated
background thread (same shape as `ksni`'s existing "blocking" thread, just
a different platform primitive), not inside eframe's own event loop.

**`src/single_instance.rs`**: replace `std::os::unix::net::{UnixListener,
UnixStream}` with `std::net::TcpListener`/`TcpStream` bound to
`127.0.0.1:<fixed high port>`. This makes the module fully OS-agnostic (no
`#[cfg]` needed at all) with the identical connect-fails-so-bind logic
already in place. Pick an unusual high port to avoid collisions; the
existing "bind fails for some other reason -> log and continue without
the lock" fallback already handles that gracefully.

**`src/autostart.rs` -> `src/autostart/{mod,linux,macos,windows}.rs`** +
**`store::data_dir()`**: replace hand-rolled path building with
`directories::ProjectDirs::from("", "", "ir-blaster")`. Per-OS autostart
mechanism:
- Linux (unchanged): XDG `.desktop` in `~/.config/autostart/`.
- Windows (new): `HKEY_CURRENT_USER\...\Run` registry value via `winreg`
  (simpler than a COM-based Startup-folder `.lnk` shortcut).
- macOS (Phase 3): LaunchAgent plist, see below.

**`setup_fonts()`**: split the hardcoded Linux font-path array into three
small `#[cfg]`-gated per-OS candidate arrays. Windows: `seguiemj.ttf` +
`seguisym.ttf` from `C:\Windows\Fonts\`. The existing "file not found ->
skip" fallback means this is a pure data change, no new error handling.

**XWayland hack**: wrap the existing `WAYLAND_DISPLAY` removal in
`#[cfg(target_os = "linux")]`.

**`updater.rs`, `teams.rs`**: gate both modules and their Settings-UI call
sites behind `#[cfg(target_os = "linux")]` per the resolved decisions
above.

**Windows driver UX** (the biggest unavoidable rough edge, no full fix
exists):
1. Bundle a pre-generated WinUSB driver package (`.inf`, generated once via
   Zadig's "Save extracted driver files" or `libwdi`) in
   `packaging/windows/driver/`, rather than making every user run Zadig
   interactively and risk binding the wrong device.
2. Document it as a required one-time manual step, the same way the
   existing README already documents the Linux udev rule as one.
3. In-app: on Windows, if opening the device fails, check via
   `rusb::devices()` whether it's enumerated at all, to distinguish "not
   plugged in" from "present but no driver" and show a specific message +
   link to the driver setup instructions for the latter.
4. Add a "Windows setup" README section parallel to the existing Linux
   "Setup" section.
5. Explicitly not attempting: silent driver auto-install, or a
   Microsoft-signed WHQL driver - both disproportionate to this project.

**Packaging**: `cargo-wix` for an MSI installer (per-user install path
under `%LOCALAPPDATA%\Programs\IR Blaster`, avoiding a UAC prompt since
there's no signing yet) plus a plain portable `.zip` of the exe as a
no-install fallback. Generate the `.ico` via ImageMagick (already a CI
dependency) with `-define icon:auto-resize=...` for a multi-resolution
icon in one pass.

**Research spike to do at implementation time, not assumed**: confirm
whether `rusb` needs a system libusb on Windows (via `vcpkg`) or can
vendor/statically link one - this determines whether a Windows apt-get
equivalent step is needed at all.

### Phase 3: macOS (not started; Apple Silicon arm64 first, then Intel x86_64)

Reuses all the cross-platform module work from Phase 2 largely unchanged.
This phase is mostly packaging plus two verification tasks:
- LaunchAgent plist (`~/Library/LaunchAgents/com.gamedirection.irblaster.plist`)
  written the same way the `.desktop` file is today (plain templated XML
  text, no new dependency); toggling in Settings also shells out to
  `launchctl bootstrap`/`bootout` so it takes effect immediately, not just
  at next login.
- **Verify** whether `egui`/`ab_glyph` can load a face out of Apple Color
  Emoji's `.ttc` (TrueType Collection) for `setup_fonts()`, or whether it
  only accepts bare `.ttf`/`.otf`. If not, the existing "missing file ->
  skip" fallback means macOS just gets no color emoji, not a crash - not a
  blocker either way, just confirm which.

**Packaging**: assemble `IR Blaster.app/Contents/{MacOS,Resources}` by
hand (same "manually assemble a bundle directory" pattern as the existing
Linux AppDir step), with an `Info.plist` (`CFBundleIdentifier =
com.gamedirection.irblaster`, `CFBundleName = IR Blaster`,
`NSHighResolutionCapable = true`). Generate `.icns` via the macOS-native
`iconutil` (build an `.iconset` dir of required sizes via ImageMagick
resizes, then `iconutil -c icns`). Wrap in a `.dmg` via `hdiutil create`
(built into every macOS runner, no extra tool needed for a first pass).

Ship unsigned/un-notarized per the resolved decision - README documents
the right-click-Open / `xattr -cr` workaround for Gatekeeper.

### Phase 4: Remaining ARM64 gap-fills (not started; Windows ARM64, macOS Intel x86_64)

By this point all cross-platform code exists and is proven on both
Windows and macOS. These two rows are close to pure CI-matrix additions
(new runner labels: `windows-11-arm`, `macos-13`; same build/package
steps, same code) - bundle together as the final phase since neither is
likely to surface new application-code issues.

## CI matrix (final shape, after all four phases)

| OS/arch | Runner | Target triple | Package |
|---|---|---|---|
| Linux x86_64 | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | AppImage |
| Linux ARM64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | AppImage |
| Windows x86_64 | `windows-2022` | `x86_64-pc-windows-msvc` | MSI + zip |
| Windows ARM64 | `windows-11-arm` | `aarch64-pc-windows-msvc` | MSI + zip |
| macOS ARM64 | `macos-14` | `aarch64-apple-darwin` | .dmg |
| macOS x86_64 | `macos-13` | `x86_64-apple-darwin` | .dmg |

One job, one `strategy.matrix`, native runners for every row (no
cross-compiling `rusb`'s libusb C dependency anywhere). A final
`needs: build` job, gated on `v*` tags, downloads all six artifacts and
attaches them to one GitHub Release via a single `softprops/action-gh-release@v2`
call.

Note: ARM64 hosted-runner availability/naming is a recent addition to
GitHub's fleet - confirm current runner labels against GitHub's docs at
implementation time rather than trusting the table to stay accurate
indefinitely.

## Critical files

- `Cargo.toml` - target-conditional dependency blocks.
- `.github/workflows/appimage.yml` - the matrix workflow.
- `src/tray.rs` -> `src/tray/{mod,linux,desktop}.rs`.
- `src/single_instance.rs` -> TCP-loopback rewrite, no `#[cfg]` needed.
- `src/autostart.rs` -> `src/autostart/{mod,linux,macos,windows}.rs`.
- `src/store.rs` - `data_dir()` via `directories` crate.
- `src/main.rs` - `setup_fonts()` per-OS arrays, XWayland hack cfg-gated,
  `updater`/`teams` modules and their Settings UI sections cfg-gated to
  Linux.
- New: `packaging/windows/driver/` (bundled WinUSB `.inf`),
  `packaging/windows/main.wxs` (cargo-wix template),
  `packaging/macos/Info.plist`.

## Verification

- Phase 1: tag a test release, confirm the Linux ARM64 AppImage builds,
  uploads, and attaches to the same GitHub Release as the x86_64 one.
- Phase 2: build locally on a Windows machine/VM (or via the new CI job),
  confirm the app launches, tray icon and menu work, autostart registry
  key gets created/removed via the Settings toggle, single-instance lock
  works (launch twice, confirm the second exits and the first shows
  itself), and the MSI installs/uninstalls cleanly. Confirm the in-app
  driver-missing message appears correctly with the WinUSB driver not yet
  installed, then confirm the device works after installing the bundled
  driver package.
- Phase 3: same checks on macOS (a real Apple Silicon Mac or macOS CI
  run), plus confirm the LaunchAgent toggle takes effect immediately
  without a logout, and check whether the emoji font loaded (per the
  `.ttc` verification task).
- Phase 4: confirm the two new CI rows build and package successfully;
  no new manual device testing expected since the code paths are already
  proven on the same OS in a different phase.
- Whole-project regression: after each phase, confirm the existing Linux
  x86_64 build still compiles and its existing behavior (tray, autostart,
  single-instance, Teams integration, self-update) is unaffected by the
  new `#[cfg]` gating - the Linux branch of every split module should be
  byte-for-byte the same logic as before, just moved into a `linux.rs`
  file or wrapped in a `#[cfg(target_os = "linux")]` block.

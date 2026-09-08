# ZazaRemote (com.tiqiaa.remote) investigation report

## Why we did this

Our own IR capture/decode pipeline (this repo) could never get the "Yaokongdeng IR Switch 203" RGB puck light to react, despite many capture attempts, carrier-frequency sweeps, and a verified-working transmit path (confirmed via phone camera and by controlling an LG TV). The user found that the official Android app **ZazaRemote** ("YaoYao Management" feature), using the *same physical dongle*, successfully controls both the TV and the puck's color. We investigated the app to get ground-truth data and/or verify our own protocol implementation against the vendor's.

## Device / app identified

- Phone: Google Pixel 7 Pro (`cheetah`), ADB serial `29081FDH300JFW`.
- App package: **`com.tiqiaa.remote`** - confirmed to be the "ZazaRemote YaoYao Management" app the user used.
- Puck light in the app's library: device name **"Yaokongdeng IR Switch 203"**, button **"#RED"** (shown with a "Length 176" field in the app UI).
- Dongle: same USB device as ours, VID/PID `0x10c4:0x8468` ("Tiqiaa TView"), confirmed by decompiled constants (`F=4292`/`0x10C4`, `H=33896`/`0x8468`; a second accepted VID `G=1118`/`0x045E` also exists in the app).

## Steps taken

1. **Enabled ADB access.** User enabled Developer Options + USB debugging on the phone, connected via USB, authorized this PC. Verified with `adb devices -l`.
2. **Found the package**: `adb shell pm list packages | grep -i zaza` → `com.tiqiaa.remote`.
3. **Checked for root**: `adb shell su -c id` → no `su` binary, phone is not rooted.
4. **Checked for debuggable access**: `adb shell run-as com.tiqiaa.remote` → `run-as: package not debuggable`. This means the app's private data directory (`/data/data/com.tiqiaa.remote/`, where any downloaded/cached IR-code database would live) is **not accessible** without root.
5. **Pulled the APK**: `adb shell pm path com.tiqiaa.remote` to find the path, then `adb pull` → `base.apk` (~47.9 MB).
6. **Decompiled it**: installed `jadx` (`sudo pacman -S jadx`, which also pulled in OpenJDK) and ran `jadx --no-res -d decompiled tiqiaa_remote.apk` to get readable (if partially obfuscated) Java source. Also plain-`unzip`ed the raw APK to inspect `assets/`/`res/` directly.
7. **Checked bundled assets** (`assets/file/tiqiaa.txt`, `assets/file/category.txt`) - these turned out to be unrelated video/movie category metadata (a leftover from a media-remote feature), not IR code data. `assets/Server.apk` is a nested companion APK, not explored further.
8. **Found the relevant Java classes**:
  - `com/icontrol/dev/TiqiaaUsbController.java` - handles this exact dongle over raw USB bulk transfers. Its `run()` method (the read loop) parses incoming packets with: report_id must be `1`, fragment size is `byte[1] - 3`, and it recognizes the last fragment via `frag_size < 56 || frag_count == frag_idx`. **This is byte-for-byte identical to our own `Report2Header`/`recv_packet` framing logic in `src/tiqiaa.rs`** - strong confirmation our low-level USB protocol handling is correct.
  - `com/icontrol/dev/IrData.java` - exposes `getYaoyaoData(context, i, bytes)` which calls a native method `bo(context, 0, i, bytes)` (mode `0` = "Yaoyao"). This is the code path matching the "YaoYao Management" feature name the user actually used, confirming it's the right one to study. The real encode/decode logic is implemented natively (JNI), not in Java.
9. **Located the native library**: `lib/arm64-v8a/libtiqiaadev.so` (and an `armeabi-v7a` copy) - a stripped ARM64 ELF built with NDK r25c. Exported JNI symbols include `Java_com_icontrol_dev_TiqiaaUsbController_{d,l,o,s,t,x}` and `Java_com_icontrol_dev_IrData_{bo,c,d,p,pi,po,si,so,st}`.
10. **Extracted the carrier-frequency table** by scanning the `.so` binary for a run of 32-bit little-endian integers starting at `38000`. Found an exact match at file offset `36912`: all 30 values, in the same order, are **byte-for-byte identical** to the `CARRIER_FREQUENCIES` table already used in our `src/tiqiaa.rs` (originally sourced from the `cclairmont/tiqiaa_lirc` C reference driver). This rules out any frequency-table/index mismatch.
11. **Disassembled the encoder function** (`Java_com_icontrol_dev_IrData_bo`) using `aarch64-linux-gnu-objdump` (installed via `sudo pacman -S aarch64-linux-gnu-binutils`, since the default `objdump` lacks ARM64 support). Found:
  - The same frequency-table lookup pattern, with a fallback default of `#0x9470` (38000 decimal) when the index is out of range - matches expected structure.
  - A literal `mov w1, #0x10` (16 decimal) in the buffer-setup code right before the actual pulse-encoding call - consistent with our `TICK_SIZE_US = 16` assumption.

## What this proved

Our own protocol implementation (USB packet framing, the 30-entry carrier-frequency table, and the 16µs tick-size constant) matches the official vendor app's native code **exactly**, on every layer we were able to verify. This is strong evidence the *transport/encoding* side of our tool (`src/tiqiaa.rs`) is correct.

## What we could not get

The actual **pulse-width data** for "Yaokongdeng IR Switch 203" → "#RED" (or any other library entry). That data is not bundled in the APK - it's fetched from Tiqiaa's server and cached in the app's private storage (a database under `/data/data/com.tiqiaa.remote/`), which requires either:
- **Root** on the phone (e.g. via Magisk) to read that directory directly, or
- **Network capture** of the app's API call that downloads/looks up the code (the app's assets reference a "grs_sdk" cloud config, suggesting a real server-backed lookup) - this would need a MITM proxy (e.g. `mitmproxy`) with a trusted CA installed on the phone, and would only work if the app doesn't pin its TLS certificates (not yet checked).

Neither was attempted this session.

## If we revisit this

- The pulled APK and decompiled source were left under this session's temp scratchpad directory and are **not preserved** - re-run steps 5–6 above (`adb pull` + `jadx`) to regenerate them if needed.
- Given the transport layer is now verified correct, the highest-value next step for actually getting the puck working is either (a) rooting the phone to pull the exact "#RED" pulse data as ground truth, or (b) continuing to refine our own **Record** capture technique (short single taps, not holds) now that we have confidence the rest of the pipeline is sound.

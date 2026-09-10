//! Protocol driver for the Tiqiaa/ZaZaRemote/ElkSmart USB IR transceiver family
//! (VID 0x10c4, PID 0x8468, sold rebranded as "TView").
//!
//! Protocol reverse-engineered by XenRE (https://gitlab.com/XenRE/tiqiaa-usb-ir),
//! reimplemented here from the normanr/tiqiaa-usb-ir-py (Python) and
//! cclairmont/tiqiaa_lirc (C/LIRC) reference drivers.

use anyhow::{anyhow, bail, Context, Result};
use rusb::{DeviceHandle, GlobalContext};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

fn hex(buf: &[u8]) -> String {
    buf.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

const VENDOR_ID: u16 = 0x10c4;
const PRODUCT_ID: u16 = 0x8468;

/// Cheap presence check (device enumeration only, no open/claim) so the tray
/// notification watcher can poll without disturbing a handle the device
/// worker might currently hold.
pub fn is_present() -> bool {
    rusb::devices()
        .map(|list| {
            list.iter().any(|d| {
                d.device_descriptor()
                    .map(|desc| desc.vendor_id() == VENDOR_ID && desc.product_id() == PRODUCT_ID)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

const MAX_USB_FRAG_SIZE: usize = 56;
const MAX_PACKET_IDX: u8 = 15;
const MAX_CMD_ID: u8 = 0x7f;
const READ_REPORT_ID: u8 = 1;
const WRITE_REPORT_ID: u8 = 2;
const PACKET_START: &[u8; 2] = b"ST";
const PACKET_END: &[u8; 2] = b"EN";

const CMD_VERSION: u8 = b'V';
const CMD_IDLE_MODE: u8 = b'L';
const CMD_SEND_MODE: u8 = b'S';
const CMD_RECV_MODE: u8 = b'R';
const CMD_DATA: u8 = b'D';
const CMD_OUTPUT: u8 = b'O';

const TICK_SIZE_US: i32 = 16;
const PULSE_BIT: u8 = 0x80;
const PULSE_MASK: u8 = 0x7f;

/// Carrier frequency table (index -> Hz), matching the device firmware's own
/// table (from the cclairmont/tiqiaa_lirc C reference driver). Index 0
/// (38000Hz) is the overwhelmingly common default, but cheap remotes often
/// use a nearby frequency instead.
pub const CARRIER_FREQUENCIES: [u32; 30] = [
    38000, 37900, 37917, 36000, 40000, 39700, 35750, 36400, 36700, 37000, 37700, 38380, 38400,
    38462, 38740, 39200, 42000, 43600, 44000, 33000, 33500, 34000, 34500, 35000, 40500, 41000,
    41500, 42500, 43000, 45000,
];

/// Encode a signal (positive = mark/pulse µs, negative = space µs) into the
/// device's tick-run-length byte format.
pub fn codes_to_tiqiaa(codes: &[i32]) -> Vec<u8> {
    let mut data = Vec::new();
    for &c in codes {
        let mut d = (c.abs() / TICK_SIZE_US).unsigned_abs() as u32;
        loop {
            let b = d.min(PULSE_MASK as u32);
            d -= b;
            let mut byte = b as u8;
            if c > 0 {
                byte |= PULSE_BIT;
            }
            data.push(byte);
            if d == 0 {
                break;
            }
        }
    }
    data
}

/// Decode the device's tick-run-length byte format back into a signal
/// (positive = mark/pulse µs, negative = space µs).
///
/// Real units of this device don't cleanly demodulate the IR carrier: during
/// an active burst they emit a long run of tiny alternating mark/space bytes
/// at roughly the carrier rate (e.g. repeating ~32µs blips) instead of one
/// continuous mark. Naively merging only same-polarity runs (as the
/// unfinished Python reference driver does - it has a `# TODO: add carrier
/// removal` it never implemented) leaves that carrier ripple as hundreds of
/// spurious tiny pulses, which mangles timing badly enough that no real
/// receiver can decode it back. So: any byte at/under `CARRIER_RIPPLE_US`,
/// mark or space, is treated as still being inside an active burst and
/// folded into one growing mark; only a space-flagged byte bigger than that
/// is a genuine gap between bursts.
pub fn tiqiaa_to_codes(data: &[u8]) -> Vec<i32> {
    const CARRIER_RIPPLE_US: i32 = 100;
    let mut codes: Vec<i32> = Vec::new();
    let mut mark_accum: i32 = 0;
    for &raw in data {
        let lvl = raw & PULSE_BIT;
        let mag_us = (raw & PULSE_MASK) as i32 * TICK_SIZE_US;
        if lvl != 0 || mag_us <= CARRIER_RIPPLE_US {
            mark_accum += mag_us;
        } else {
            if mark_accum > 0 {
                codes.push(mark_accum);
                mark_accum = 0;
            }
            codes.push(-mag_us);
        }
    }
    if mark_accum > 0 {
        codes.push(mark_accum);
    }
    codes
}

struct Report2Header {
    report_id: u8,
    frag_size: u8,
    packet_idx: u8,
    frag_count: u8,
    frag_idx: u8,
}

impl Report2Header {
    const SIZE: usize = 5;

    fn pack(&self) -> [u8; Self::SIZE] {
        [
            self.report_id,
            self.frag_size,
            self.packet_idx,
            self.frag_count,
            self.frag_idx,
        ]
    }

    fn unpack(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            bail!("short report header");
        }
        Ok(Self {
            report_id: buf[0],
            frag_size: buf[1],
            packet_idx: buf[2],
            frag_count: buf[3],
            frag_idx: buf[4],
        })
    }
}

pub struct TiqiaaDevice {
    handle: DeviceHandle<GlobalContext>,
    iface: u8,
    ep_in: u8,
    ep_out: u8,
    cmd_id: u8,
    packet_idx: u8,
    detached_kernel_driver: bool,
    log_tx: Sender<String>,
    start: Instant,
}

impl TiqiaaDevice {
    pub fn open(log_tx: Sender<String>) -> Result<Self> {
        let log = |m: String| {
            eprintln!("{m}");
            let _ = log_tx.send(m);
        };
        log(format!(
            "Opening device {VENDOR_ID:04x}:{PRODUCT_ID:04x}..."
        ));
        let handle = rusb::open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID)
            .context("Tiqiaa TView device (10c4:8468) not found - is it plugged in?")?;

        let device = handle.device();
        let config = device.active_config_descriptor()?;
        let interface = config
            .interfaces()
            .next()
            .ok_or_else(|| anyhow!("device has no interfaces"))?;
        let iface = interface.number();
        let descriptor = interface
            .descriptors()
            .next()
            .ok_or_else(|| anyhow!("device interface has no descriptor"))?;

        let mut ep_in = None;
        let mut ep_out = None;
        for ep in descriptor.endpoint_descriptors() {
            if ep.transfer_type() != rusb::TransferType::Bulk {
                continue;
            }
            match ep.direction() {
                rusb::Direction::In => ep_in = Some(ep.address()),
                rusb::Direction::Out => ep_out = Some(ep.address()),
            }
        }
        let ep_in = ep_in.ok_or_else(|| anyhow!("no bulk IN endpoint found"))?;
        let ep_out = ep_out.ok_or_else(|| anyhow!("no bulk OUT endpoint found"))?;
        log(format!(
            "Found bulk endpoints: IN=0x{ep_in:02x} OUT=0x{ep_out:02x}"
        ));

        let mut detached_kernel_driver = false;
        if handle.kernel_driver_active(iface).unwrap_or(false) {
            handle.detach_kernel_driver(iface).context(
                "failed to detach kernel HID driver - try running with udev rule installed",
            )?;
            detached_kernel_driver = true;
            log("Detached kernel HID driver".to_string());
        }
        handle
            .claim_interface(iface)
            .context("failed to claim USB interface (check udev permissions)")?;
        log(format!("Claimed interface {iface}"));

        // A prior ungraceful shutdown (or the device just being flaky) can
        // leave a bulk endpoint halted/stalled, which makes every transfer
        // on it silently time out. Clearing halt is cheap and harmless when
        // the endpoint was fine already, so just always do it on open.
        match handle.clear_halt(ep_out) {
            Ok(()) => log(format!("Cleared halt on OUT endpoint 0x{ep_out:02x}")),
            Err(e) => log(format!("clear_halt(OUT) failed (may be harmless): {e:?}")),
        }
        match handle.clear_halt(ep_in) {
            Ok(()) => log(format!("Cleared halt on IN endpoint 0x{ep_in:02x}")),
            Err(e) => log(format!("clear_halt(IN) failed (may be harmless): {e:?}")),
        }

        Ok(Self {
            handle,
            iface,
            ep_in,
            ep_out,
            cmd_id: 0,
            packet_idx: 0,
            detached_kernel_driver,
            log_tx,
            start: Instant::now(),
        })
    }

    fn log(&self, msg: impl Into<String>) {
        let msg = format!(
            "[+{:>7.3}s] {}",
            self.start.elapsed().as_secs_f32(),
            msg.into()
        );
        eprintln!("{msg}");
        let _ = self.log_tx.send(msg);
    }

    fn next_cmd_id(&mut self) -> u8 {
        self.cmd_id = if self.cmd_id < MAX_CMD_ID {
            self.cmd_id + 1
        } else {
            1
        };
        self.cmd_id
    }

    fn next_packet_idx(&mut self) -> u8 {
        self.packet_idx = if self.packet_idx < MAX_PACKET_IDX {
            self.packet_idx + 1
        } else {
            1
        };
        self.packet_idx
    }

    /// Frame `payload` as ST..EN, split into <=56-byte fragments, each
    /// prefixed with a 5-byte report header, and write them out over bulk OUT.
    fn send_report(&mut self, payload: &[u8]) -> Result<()> {
        let mut report = Vec::with_capacity(payload.len() + 4);
        report.extend_from_slice(PACKET_START);
        report.extend_from_slice(payload);
        report.extend_from_slice(PACKET_END);

        let packet_idx = self.next_packet_idx();
        let frag_count = report.len().div_ceil(MAX_USB_FRAG_SIZE) as u8;

        for (i, chunk) in report.chunks(MAX_USB_FRAG_SIZE).enumerate() {
            let hdr = Report2Header {
                report_id: WRITE_REPORT_ID,
                frag_size: (chunk.len() + 3) as u8,
                packet_idx,
                frag_count,
                frag_idx: (i + 1) as u8,
            };
            let mut buf = hdr.pack().to_vec();
            buf.extend_from_slice(chunk);
            self.log(format!("→ TX {} bytes: {}", buf.len(), hex(&buf)));
            let t0 = Instant::now();
            let write_result = self
                .handle
                .write_bulk(self.ep_out, &buf, Duration::from_secs(2));
            let elapsed = t0.elapsed();
            match &write_result {
                Ok(n) => {
                    if elapsed > Duration::from_millis(50) {
                        self.log(format!(
                            " (write_bulk took {:.3}s, wrote {n} bytes)",
                            elapsed.as_secs_f32()
                        ));
                    }
                }
                Err(e) => self.log(format!(
                    " (write_bulk FAILED after {:.3}s: {e:?})",
                    elapsed.as_secs_f32()
                )),
            }
            write_result.context("USB write failed")?;
        }
        Ok(())
    }

    /// Read one full ST..EN packet (possibly split over several bulk-IN
    /// fragments) and return (cmd_id, cmd_type, body).
    fn recv_packet(&mut self, timeout: Duration) -> Result<(u8, u8, Vec<u8>)> {
        self.recv_packet_progress(timeout, || {})
    }

    /// Same as `recv_packet`, but calls `on_fragment` the moment each raw
    /// fragment arrives (before the packet is fully reassembled) - lets a
    /// caller show a live "data is arriving" indicator during a long wait.
    fn recv_packet_progress(
        &mut self,
        timeout: Duration,
        mut on_fragment: impl FnMut(),
    ) -> Result<(u8, u8, Vec<u8>)> {
        // libusb treats a 0ms timeout as "wait forever", not "return
        // immediately" - never let a near-zero Duration reach it.
        let timeout = timeout.max(Duration::from_millis(50));
        let mut packet = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            let n = self
                .handle
                .read_bulk(self.ep_in, &mut buf, timeout)
                .context("USB read timed out or failed")?;
            on_fragment();
            self.log(format!("← RX {} bytes: {}", n, hex(&buf[..n])));
            if n < Report2Header::SIZE {
                bail!(
                    "short read from device ({n} bytes, need >= {})",
                    Report2Header::SIZE
                );
            }
            let hdr = Report2Header::unpack(&buf[..n])?;
            if hdr.report_id != READ_REPORT_ID {
                bail!(
                    "unexpected report id {} (expected {READ_REPORT_ID})",
                    hdr.report_id
                );
            }
            let data_len = hdr.frag_size as usize;
            if data_len < 3 || data_len - 3 > n - Report2Header::SIZE {
                bail!(
                    "malformed fragment size: frag_size={} available={} ",
                    hdr.frag_size,
                    n - Report2Header::SIZE
                );
            }
            let frag_len = data_len - 3;
            packet.extend_from_slice(&buf[Report2Header::SIZE..Report2Header::SIZE + frag_len]);
            if hdr.frag_idx == hdr.frag_count {
                break;
            }
        }

        if packet.len() < 4
            || &packet[..2] != PACKET_START
            || &packet[packet.len() - 2..] != PACKET_END
        {
            bail!("packet framing (ST/EN) mismatch: {}", hex(&packet));
        }
        let body = &packet[2..packet.len() - 2];
        if body.len() < 3 {
            bail!("packet body too short: {}", hex(body));
        }
        let cmd_id = body[0];
        let cmd_type = body[1];
        // last byte is the device state (Idle=3/Send=9/Recv=19); data is in between.
        let data = body[2..body.len() - 1].to_vec();
        self.log(format!(
            "← packet: cmd_id={cmd_id} type={} state={} data_len={}",
            cmd_type as char,
            body[body.len() - 1],
            data.len()
        ));
        Ok((cmd_id, cmd_type, data))
    }

    fn send_cmd_and_wait(
        &mut self,
        cmd_type: u8,
        cmd_data: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        self.send_cmd_and_wait_progress(cmd_type, cmd_data, timeout, || {})
    }

    /// Same as `send_cmd_and_wait`, but calls `on_fragment` as soon as the
    /// reply starts arriving (see `recv_packet_progress`).
    fn send_cmd_and_wait_progress(
        &mut self,
        cmd_type: u8,
        cmd_data: &[u8],
        timeout: Duration,
        on_fragment: impl FnMut(),
    ) -> Result<Vec<u8>> {
        let cmd_id = self.next_cmd_id();
        self.log(format!(
            "→ cmd {} (id={cmd_id}, {} data bytes, timeout={:.1}s)",
            cmd_type as char,
            cmd_data.len(),
            timeout.as_secs_f32()
        ));
        let mut payload = vec![cmd_id, cmd_type];
        payload.extend_from_slice(cmd_data);
        self.send_report(&payload)?;

        let (_reply_id, _reply_type, data) = self.recv_packet_progress(timeout, on_fragment)?;
        Ok(data)
    }

    /// Send a command without requiring a reply. The reference C driver for
    /// this device (cclairmont/tiqiaa_lirc) never treats a missing ack for
    /// mode-switch/data commands as an error - it does one best-effort short
    /// read and proceeds regardless. Real units of this device are flaky
    /// about acking these, so we do the same: fire the command, try a brief
    /// drain, and continue either way.
    fn send_cmd_fire_and_forget(&mut self, cmd_type: u8, cmd_data: &[u8]) -> Result<()> {
        const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
        let cmd_id = self.next_cmd_id();
        self.log(format!(
            "→ cmd {} (id={cmd_id}, {} data bytes, fire-and-forget)",
            cmd_type as char,
            cmd_data.len()
        ));
        let mut payload = vec![cmd_id, cmd_type];
        payload.extend_from_slice(cmd_data);
        self.send_report(&payload)?;

        match self.recv_packet(DRAIN_TIMEOUT) {
            Ok(_) => {}
            Err(e) => self.log(format!("← no immediate reply ({e:#}) - continuing anyway")),
        }
        Ok(())
    }

    pub fn version(&mut self) -> Result<Vec<u8>> {
        self.send_cmd_and_wait(CMD_VERSION, &[], Duration::from_secs(2))
    }

    fn idle(&mut self) -> Result<()> {
        self.send_cmd_fire_and_forget(CMD_IDLE_MODE, &[])
    }

    /// Arm the receiver and block until the device replies to a single
    /// `Output` request (or `timeout` elapses waiting for the user to press a
    /// button on the remote). Re-issuing `Output` before a reply lands
    /// appears to corrupt the device's internal state on real units (every
    /// subsequent write then hangs for a flat 2s and never recovers for the
    /// rest of the session) - likely each `Output` call starts a fresh
    /// capture window rather than checking a continuously-armed receiver, so
    /// resending it just keeps invalidating that window. So: exactly one
    /// request, one wait, whatever comes back (even if it looks like ambient
    /// noise - the caller/UI can inspect and re-record if so).
    pub fn record_signal(
        &mut self,
        timeout: Duration,
        on_attempt: impl FnOnce(&[i32]),
        on_receiving: impl FnMut(),
    ) -> Result<Vec<i32>> {
        self.log("Entering RECV mode...");
        self.send_cmd_fire_and_forget(CMD_RECV_MODE, &[])?;
        self.log("Device armed. Listening for IR - aim remote and press a button now.");

        let timeout = timeout.max(Duration::from_millis(200));
        let data = self.send_cmd_and_wait_progress(CMD_OUTPUT, &[], timeout, on_receiving);
        self.idle().ok();
        let data = data?;

        let codes = tiqiaa_to_codes(&data);
        on_attempt(&codes);
        self.log(format!(
            "Captured {} raw bytes -> {} pulses",
            data.len(),
            codes.len()
        ));
        Ok(codes)
    }

    /// Force a USB port-level reset. Real units of this device can end up in
    /// a wedged state (e.g. after the host process was killed mid-transfer,
    /// skipping graceful cleanup) where it stops replying to anything - this
    /// gives the user a way to recover without physically unplugging it.
    pub fn hardware_reset(&mut self) -> Result<()> {
        self.log("Resetting USB device...");
        self.handle.reset().context("USB reset failed")?;
        self.log("USB reset complete");
        Ok(())
    }

    /// Transmit a previously recorded/decoded signal. `freq_index` indexes
    /// `CARRIER_FREQUENCIES` (0 = 38000Hz, the common default).
    pub fn send_signal(&mut self, codes: &[i32], freq_index: u8) -> Result<()> {
        self.log("Entering SEND mode...");
        self.send_cmd_fire_and_forget(CMD_SEND_MODE, &[])?;
        let mut cdata = vec![freq_index];
        cdata.extend_from_slice(&codes_to_tiqiaa(codes));
        let hz = CARRIER_FREQUENCIES
            .get(freq_index as usize)
            .copied()
            .unwrap_or(38000);
        self.log(format!(
            "Transmitting {} pulses ({} encoded bytes) at {hz}Hz",
            codes.len(),
            cdata.len() - 1
        ));
        self.send_cmd_fire_and_forget(CMD_DATA, &cdata)?;
        self.idle().ok();
        Ok(())
    }
}

impl Drop for TiqiaaDevice {
    fn drop(&mut self) {
        self.idle().ok();
        let _ = self.handle.release_interface(self.iface);
        if self.detached_kernel_driver {
            let _ = self.handle.attach_kernel_driver(self.iface);
        }
    }
}

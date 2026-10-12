use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use crate::{MAX_STREAM, REPORT_IN, REPORT_OUT, REPORT_SIZE, VID};

/// RACE command ranges that are never sent: flash storage and firmware update.
const FORBIDDEN_CMDS: [(u16, u16); 2] = [(0x0400, 0x04FF), (0x1C00, 0x1CFF)];

/// Product IDs seen speaking this protocol.
pub fn known_product(pid: u16) -> Option<&'static str> {
    match pid {
        0x2287 => Some("Stealth Pro II PC transmitter"),
        0x2288 => Some("Stealth Pro II transmitter"),
        0x2289 => Some("Stealth Pro II PC headset (USB-C)"),
        0x229B => Some("Stealth Pro II transmitter dock"),
        0x229E => Some("Stealth Pro II headset (USB-C)"),
        _ => None,
    }
}

/// A Turtle Beach hidraw node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub pid: u16,
    pub name: String,
    /// Whether the report descriptor has the `0xFF13` vendor collection this protocol runs on.
    pub vendor_collection: bool,
}

/// Every Turtle Beach hidraw node, sorted by path; known products with the vendor collection first.
pub fn discover() -> io::Result<Vec<DeviceInfo>> {
    let mut found = Vec::new();
    for entry in fs::read_dir("/sys/class/hidraw")? {
        let node = entry?.path();
        if let Some(info) = probe(&node) {
            found.push(info);
        }
    }
    found.sort_by_key(|d| (!(d.vendor_collection && known_product(d.pid).is_some()), d.path.clone()));
    Ok(found)
}

fn probe(node: &Path) -> Option<DeviceInfo> {
    let uevent = fs::read_to_string(node.join("device/uevent")).ok()?;
    let mut id = None;
    let mut name = String::new();
    for line in uevent.lines() {
        if let Some(v) = line.strip_prefix("HID_ID=") {
            // 0003:000010F5:00002287
            let mut parts = v.split(':').skip(1);
            let vid = u32::from_str_radix(parts.next()?, 16).ok()?;
            let pid = u32::from_str_radix(parts.next()?, 16).ok()?;
            id = Some((vid, pid));
        } else if let Some(v) = line.strip_prefix("HID_NAME=") {
            name = v.to_string();
        }
    }
    let (vid, pid) = id?;
    if vid != u32::from(VID) {
        return None;
    }
    let rdesc = fs::read(node.join("device/report_descriptor")).unwrap_or_default();
    Some(DeviceInfo {
        path: Path::new("/dev").join(node.file_name()?),
        pid: pid as u16,
        name,
        // Usage Page (0xFF13)
        vendor_collection: rdesc.windows(3).any(|w| w == [0x06, 0x13, 0xFF]),
    })
}

/// Report-level I/O, so the protocol layers can run against a real device or a test double.
pub trait ReportIo {
    /// Send one RACE packet (already encoded) in an output report.
    fn send_race(&mut self, target: u8, race: &[u8]) -> io::Result<()>;
    /// Fetch one input report; an idle report has a zero length byte.
    fn read_report(&mut self) -> io::Result<Vec<u8>>;
}

/// An open `/dev/hidrawN`.
pub struct HidrawDevice {
    file: File,
    /// Print every report to stderr (set `RACE_HID_TRACE=1`).
    trace: bool,
}

impl HidrawDevice {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let trace = std::env::var_os("RACE_HID_TRACE").is_some();
        Ok(Self { file, trace })
    }

    /// GET_REPORT(Input, 0x07) over the control endpoint, through HIDIOCGINPUT.
    fn get_input_report(&self) -> io::Result<Vec<u8>> {
        let mut buf = [0u8; REPORT_SIZE];
        buf[0] = REPORT_IN;
        // _IOC(_IOC_READ | _IOC_WRITE, 'H', 0x0A, len)
        let request = (3 << 30) | ((REPORT_SIZE as u64) << 16) | ((b'H' as u64) << 8) | 0x0A;
        // SAFETY: buf is REPORT_SIZE bytes, matching the size encoded in the request.
        let n = unsafe { libc::ioctl(self.file.as_raw_fd(), request as _, buf.as_mut_ptr()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buf[..(n as usize).min(REPORT_SIZE)].to_vec())
    }
}

impl ReportIo for HidrawDevice {
    fn send_race(&mut self, target: u8, race: &[u8]) -> io::Result<()> {
        if race.len() > MAX_STREAM {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "RACE packet longer than one report"));
        }
        if race.len() >= 6 {
            let cmd = u16::from_le_bytes([race[4], race[5]]);
            if FORBIDDEN_CMDS.iter().any(|&(lo, hi)| (lo..=hi).contains(&cmd)) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("refusing to send RACE command {cmd:04x}"),
                ));
            }
        }
        let mut report = [0u8; REPORT_SIZE];
        report[0] = REPORT_OUT;
        report[1] = race.len() as u8;
        report[2] = target;
        report[3..3 + race.len()].copy_from_slice(race);
        if self.trace {
            eprintln!("race-hid >> {}", hex(&report[..3 + race.len()]));
        }
        self.file.write_all(&report)
    }

    fn read_report(&mut self) -> io::Result<Vec<u8>> {
        match self.get_input_report() {
            Ok(r) => {
                if self.trace && r.get(1).is_some_and(|&n| n > 0) {
                    eprintln!("race-hid << {}", hex(&r[..(3 + usize::from(r[1])).min(r.len())]));
                }
                Ok(r)
            }
            // The transmitter now and then fails a control transfer while busy: treat it as an idle poll.
            Err(e) if matches!(e.raw_os_error(), Some(libc::EPROTO | libc::EPIPE | libc::ETIMEDOUT | libc::EAGAIN)) => {
                Ok(vec![REPORT_IN, 0, 0])
            }
            Err(e) => Err(e),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

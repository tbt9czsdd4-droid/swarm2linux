//! Airoha RACE packets over USB HID, as spoken by the Turtle Beach Stealth Pro II.
//!
//! HID framing on the vendor collection (usage page `0xFF13`):
//!
//! ```text
//! out: 06 <n> <target> <n bytes of RACE stream>   padded to 62 bytes, sent with write()
//! in:  07 <n> <source> <n bytes of RACE stream>   fetched with GET_REPORT, n = 0 when idle
//! ```
//!
//! The input side is a byte stream: a RACE packet can start mid-report and span several reports, so
//! [`Reassembler`] cuts packets by their own length field. See `docs/protocol.md`.

mod device;
mod race;

pub use device::{DeviceInfo, HidrawDevice, ReportIo, discover, known_product};
pub use race::{RacePacket, RaceType, Reassembler};

/// Turtle Beach's USB vendor ID.
pub const VID: u16 = 0x10F5;
/// Output report carrying host-to-device RACE bytes.
pub const REPORT_OUT: u8 = 0x06;
/// Input report carrying device-to-host RACE bytes.
pub const REPORT_IN: u8 = 0x07;
/// Report size including the report ID.
pub const REPORT_SIZE: usize = 62;
/// RACE bytes that fit in one report after the report ID, length and target bytes.
pub const MAX_STREAM: usize = REPORT_SIZE - 3;

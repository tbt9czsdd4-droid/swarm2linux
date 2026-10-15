//! The Turtle Beach Stealth Pro II settings protocol: JSON key/value records tunnelled in Airoha RACE
//! command `0x9902`. See `docs/protocol.md` for the wire format and the record catalogue.

mod client;
mod doc;
mod frame;
pub mod keys;
mod state;
pub mod values;

pub use client::{Client, open};
pub use doc::{DocKind, Document};
pub use frame::{CMD_KVP, Message, MessageKind, Request, Token};
pub use state::{CustomPreset, Equalizer, State, Transmitter, TransmitterState};

/// Records that answer a read (`R`) but not a subscription (`S`).
pub const READ_ONCE: &[&str] = &["BT"];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("device I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("no Turtle Beach device with the vendor HID collection found")]
    NoDevice,
    #[error("no answer from the headset to {0}")]
    Timeout(String),
    #[error("key {0:#x} is not known to be writable")]
    NotWritable(u16),
    #[error("{value:?} is not a value Swarm II was seen writing to {name}")]
    Invalid { name: &'static str, value: String },
    #[error("the headset did not acknowledge the write to {0:#x}")]
    Nack(u16),
    #[error("unexpected answer to {0}")]
    BadReply(String),
    #[error("{0}")]
    Preset(String),
}

pub type Result<T> = std::result::Result<T, Error>;

//! Keys of the records, and the ones that may be written.
//!
//! Only keys Swarm II was seen writing are writable, within the ranges it used (see `docs/protocol.md`).

// Inf
pub const FIRMWARE: u16 = 0x130;
pub const TRANSMITTER_COUNT: u16 = 0x140;
pub const ACTIVE_TRANSMITTER: u16 = 0x150;
// GSI
pub const NAME: u16 = 0x220;
pub const BATTERY: u16 = 0x240;
pub const CHARGING: u16 = 0x250;
pub const POWER_STATE: u16 = 0x290;
pub const VOLUME: u16 = 0x2A0;
// TX1..TX4
pub const TRANSMITTERS: [u16; 4] = [0x400, 0x420, 0x440, 0x460];
// Mic
pub const MIC_MUTED: u16 = 0x600;
pub const MIC_VOLUME: u16 = 0x610;
pub const NOISE_GATE: u16 = 0x620;
pub const AI_NOISE_REDUCTION: u16 = 0x630;
// SAF
pub const MIC_MONITOR: u16 = 0x700;
pub const MIC_MONITOR_VOLUME: u16 = 0x710;
pub const SUPERHUMAN_HEARING: u16 = 0x720;
pub const SUPERHUMAN_LEVEL: u16 = 0x730;
pub const SUPERHUMAN_VOLUME: u16 = 0x740;
pub const ANC: u16 = 0x750;
pub const ANC_LEVEL: u16 = 0x760;
// AQG (game EQ) and AQM (mic EQ): preset, 10 bands, preset name
pub const GAME_EQ_PRESET: u16 = 0x1210;
pub const GAME_EQ_BANDS: u16 = 0x1220;
pub const GAME_EQ_NAME: u16 = 0x12C0;
pub const MIC_EQ_PRESET: u16 = 0x1310;
pub const MIC_EQ_BANDS: u16 = 0x1320;
pub const MIC_EQ_NAME: u16 = 0x13C0;
/// Bands are 10 keys, 0x10 apart.
pub const BAND_COUNT: usize = 10;

/// Built-in game EQ presets (`AQG.1210`); `0` means custom bands.
pub const GAME_EQ_PRESETS: [(i64, &str); 4] =
    [(1, "Signature Sound"), (2, "Bass Boost"), (3, "Bass & Treble Boost"), (4, "Vocal Boost")];

/// A key that may be written, with the range Swarm II used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub key: u16,
    pub name: &'static str,
    pub min: i64,
    pub max: i64,
}

const fn setting(key: u16, name: &'static str, min: i64, max: i64) -> Setting {
    Setting { key, name, min, max }
}

/// Every writable key.
pub const WRITABLE: &[Setting] = &[
    setting(GAME_EQ_PRESET, "game EQ preset", 1, 4),
    setting(GAME_EQ_BANDS, "game EQ band 1", -90, 90),
    setting(GAME_EQ_BANDS + 0x10, "game EQ band 2", -90, 90),
    setting(GAME_EQ_BANDS + 0x20, "game EQ band 3", -90, 90),
    setting(GAME_EQ_BANDS + 0x30, "game EQ band 4", -90, 90),
    setting(GAME_EQ_BANDS + 0x40, "game EQ band 5", -90, 90),
    setting(GAME_EQ_BANDS + 0x50, "game EQ band 6", -90, 90),
    setting(GAME_EQ_BANDS + 0x60, "game EQ band 7", -90, 90),
    setting(GAME_EQ_BANDS + 0x70, "game EQ band 8", -90, 90),
    setting(GAME_EQ_BANDS + 0x80, "game EQ band 9", -90, 90),
    setting(GAME_EQ_BANDS + 0x90, "game EQ band 10", -90, 90),
    setting(MIC_VOLUME, "mic volume", 0, 100),
    setting(NOISE_GATE, "noise gate", 0, 100),
    setting(AI_NOISE_REDUCTION, "A.I. noise reduction", 0, 1),
    setting(MIC_MONITOR, "mic monitoring", 0, 1),
    setting(MIC_MONITOR_VOLUME, "mic monitoring volume", 0, 100),
    setting(SUPERHUMAN_HEARING, "Superhuman Hearing", 0, 1),
    setting(SUPERHUMAN_LEVEL, "Superhuman Hearing level", 0, 2),
    setting(SUPERHUMAN_VOLUME, "Superhuman Hearing volume", 0, 100),
    setting(ANC, "noise cancellation", 0, 1),
    setting(ANC_LEVEL, "noise cancellation level", 0, 100),
];

pub fn writable(key: u16) -> Option<&'static Setting> {
    WRITABLE.iter().find(|s| s.key == key)
}

/// A readable name for a key, where it is known.
pub fn label(key: u16) -> Option<&'static str> {
    if let Some(s) = writable(key) {
        return Some(s.name);
    }
    Some(match key {
        0x100 => "vendor ID",
        0x110 => "headset product ID",
        0x120 => "serial number",
        FIRMWARE => "firmware",
        TRANSMITTER_COUNT => "connected transmitters",
        ACTIVE_TRANSMITTER => "active transmitter",
        NAME => "name",
        0x230 => "link state",
        BATTERY => "battery",
        CHARGING => "charging",
        POWER_STATE => "power state",
        VOLUME => "volume",
        0x320 => "Bluetooth signal",
        0x400 => "transmitter 1",
        0x420 => "transmitter 2",
        0x440 => "transmitter 3",
        0x460 => "transmitter 4",
        MIC_MUTED => "mic muted",
        GAME_EQ_NAME => "game EQ name",
        MIC_EQ_PRESET => "mic EQ preset",
        MIC_EQ_NAME => "mic EQ name",
        _ => return None,
    })
}

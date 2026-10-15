//! Keys of the records, and the ones that may be written.
//!
//! Only keys Swarm II was seen writing are writable, with the values it used (see `docs/protocol.md`). Names
//! follow Swarm II's own setting names where its device module gives them (`Sensitivity_Level`, `SHH_Preset`, ...).

// Inf
pub const FIRMWARE: u16 = 0x130;
pub const TRANSMITTER_COUNT: u16 = 0x140;
pub const ACTIVE_TRANSMITTER: u16 = 0x150;
// GSI
pub const NAME: u16 = 0x220;
pub const BATTERY: u16 = 0x240;
pub const CHARGING: u16 = 0x250;
/// `Power_Options`: 0 never, 1 after 5, 2 after 10, 3 after 20, 4 after 30 minutes.
pub const AUTO_SHUTOFF: u16 = 0x280;
pub const POWER_STATE: u16 = 0x290;
pub const WAKE_ON_MOTION: u16 = 0x2E0;
/// `Master_Volume_Level`, also moved by the left wheel.
pub const VOLUME: u16 = 0x2A0;
// TX1..TX4
pub const TRANSMITTERS: [u16; 4] = [0x400, 0x420, 0x440, 0x460];
/// Elements of TX1's `control` array, written one by one: `0x401` = LED 1, `0x402` = LED 2 brightness of the
/// charging hub (TX1 is the hub's built-in transmitter here).
pub const HUB_LED_1: u16 = 0x401;
pub const HUB_LED_2: u16 = 0x402;
// Mic
pub const MIC_MUTED: u16 = 0x600;
pub const MIC_SENSITIVITY: u16 = 0x610;
pub const MONITORING_LEVEL: u16 = 0x620;
pub const AI_NOISE_REDUCTION: u16 = 0x630;
/// Probably `Monitoring_Enable`; never written by Swarm II's UI (it only has a hotkey for it).
pub const MONITORING_ENABLE: u16 = 0x640;
// SAF
pub const NOISE_GATE: u16 = 0x700;
pub const NOISE_GATE_LEVEL: u16 = 0x710;
pub const SUPERHUMAN_HEARING: u16 = 0x720;
/// `SHH_Preset`: 0 Legacy, 1 Footsteps, 2 Gunshots.
pub const SUPERHUMAN_MODE: u16 = 0x730;
/// `SHH_Level`, 0–100.
pub const SUPERHUMAN_LEVEL: u16 = 0x740;
pub const ANC: u16 = 0x750;
/// `ANC_Level`: 0 = ambient (outside sound let in) … 100 = full noise cancellation.
pub const ANC_LEVEL: u16 = 0x760;
// Btn, Enc: the Assignment page
/// What the button above the wheels on the left earcup does: see [`BUTTON_FUNCTIONS`].
pub const BUTTON_FUNCTION: u16 = 0xB20;
/// What the assignable wheel does: see [`WHEEL_FUNCTIONS`].
pub const WHEEL_FUNCTION: u16 = 0xA20;
// AQG (game EQ) and AQM (mic EQ): preset, 10 bands, preset name
pub const GAME_EQ_PRESET: u16 = 0x1210;
pub const GAME_EQ_BANDS: u16 = 0x1220;
pub const GAME_EQ_NAME: u16 = 0x12C0;
pub const MIC_EQ_PRESET: u16 = 0x1310;
pub const MIC_EQ_BANDS: u16 = 0x1320;
pub const MIC_EQ_NAME: u16 = 0x13C0;
/// Bands are 10 keys, 0x10 apart.
pub const BAND_COUNT: usize = 10;
// CEC: writing a custom preset's name here deletes it
pub const GAME_EQ_DELETE: u16 = 0x1610;
pub const MIC_EQ_DELETE: u16 = 0x1630;
/// Custom preset slots `CG1..CG5` (game) and `CM1..CM5` (mic); slot n is preset number `FIRST_CUSTOM_PRESET + n - 1`.
pub const GAME_CUSTOM_SLOTS: [u16; 5] = [0x1700, 0x1720, 0x1740, 0x1760, 0x1780];
pub const MIC_CUSTOM_SLOTS: [u16; 5] = [0x1800, 0x1820, 0x1840, 0x1860, 0x1880];
pub const FIRST_CUSTOM_PRESET: i64 = 16;

/// Built-in game EQ presets (`AQG.1210`); `0` means custom bands.
pub const GAME_EQ_PRESETS: [(i64, &str); 4] =
    [(1, "Signature Sound"), (2, "Bass Boost"), (3, "Bass & Treble Boost"), (4, "Vocal Boost")];
/// Built-in mic EQ presets (`AQM.1310`).
pub const MIC_EQ_PRESETS: [(i64, &str); 4] = [(1, "Signature Sound"), (2, "Full"), (3, "Clarity"), (4, "Smooth")];
/// Band centre frequencies as Swarm II labels them.
pub const GAME_EQ_FREQUENCIES: [&str; 10] =
    ["32 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"];
pub const MIC_EQ_FREQUENCIES: [&str; 10] =
    ["100 Hz", "160 Hz", "250 Hz", "400 Hz", "630 Hz", "1 kHz", "1.6 kHz", "2.5 kHz", "4 kHz", "6.3 kHz"];
/// Superhuman Hearing modes (`SAF.730`).
pub const SUPERHUMAN_MODES: [&str; 3] = ["Legacy", "Footsteps", "Gunshots"];
/// `Btn.b20` values, as Swarm II lists them.
pub const BUTTON_FUNCTIONS: [(i64, &str); 3] =
    [(0, "Active noise cancellation on/off"), (1, "Cycle game presets"), (2, "Noise gate on/off")];
/// `Enc.a20` values, as Swarm II lists them.
pub const WHEEL_FUNCTIONS: [(i64, &str); 5] = [
    (1, "Mic monitoring volume"),
    (2, "Game and chat mix"),
    (3, "Bass boost level"),
    (4, "Treble boost level"),
    (5, "Noise gate volume"),
];
/// `GSI.280` values.
pub const AUTO_SHUTOFF_OPTIONS: [(i64, &str); 5] =
    [(0, "never"), (1, "5 minutes"), (2, "10 minutes"), (3, "20 minutes"), (4, "30 minutes")];

/// The keys of one equalizer (game or mic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EqKeys {
    pub what: &'static str,
    pub preset: u16,
    pub bands: u16,
    pub name: u16,
    pub delete: u16,
    pub slots: [u16; 5],
    pub presets: [(i64, &'static str); 4],
    pub frequencies: [&'static str; 10],
}

impl EqKeys {
    pub fn band(&self, n: usize) -> u16 {
        self.bands + n as u16 * 0x10
    }
}

pub const GAME_EQ: EqKeys = EqKeys {
    what: "game EQ",
    preset: GAME_EQ_PRESET,
    bands: GAME_EQ_BANDS,
    name: GAME_EQ_NAME,
    delete: GAME_EQ_DELETE,
    slots: GAME_CUSTOM_SLOTS,
    presets: GAME_EQ_PRESETS,
    frequencies: GAME_EQ_FREQUENCIES,
};

pub const MIC_EQ: EqKeys = EqKeys {
    what: "mic EQ",
    preset: MIC_EQ_PRESET,
    bands: MIC_EQ_BANDS,
    name: MIC_EQ_NAME,
    delete: MIC_EQ_DELETE,
    slots: MIC_CUSTOM_SLOTS,
    presets: MIC_EQ_PRESETS,
    frequencies: MIC_EQ_FREQUENCIES,
};

/// The values a writable key accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Values {
    Range(i64, i64),
    OneOf(&'static [i64]),
    /// Printable ASCII text up to this many characters.
    Text(usize),
}

impl Values {
    pub fn accepts(&self, v: i64) -> bool {
        match *self {
            Values::Range(lo, hi) => (lo..=hi).contains(&v),
            Values::OneOf(set) => set.contains(&v),
            Values::Text(_) => false,
        }
    }

    pub fn accepts_text(&self, s: &str) -> bool {
        match *self {
            Values::Text(max) => !s.is_empty() && s.len() <= max && s.bytes().all(|b| (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\'),
            _ => false,
        }
    }
}

/// A key that may be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub key: u16,
    pub name: &'static str,
    pub values: Values,
}

const fn range(key: u16, name: &'static str, lo: i64, hi: i64) -> Setting {
    Setting { key, name, values: Values::Range(lo, hi) }
}

const fn one_of(key: u16, name: &'static str, set: &'static [i64]) -> Setting {
    Setting { key, name, values: Values::OneOf(set) }
}

const fn text(key: u16, name: &'static str) -> Setting {
    // the whole set_kvp JSON must fit one report: {"0x12c0":"<name>"} leaves 18 characters
    Setting { key, name, values: Values::Text(16) }
}

/// Built-in presets plus the five custom slots.
const PRESET_NUMBERS: &[i64] = &[1, 2, 3, 4, 16, 17, 18, 19, 20];

macro_rules! bands {
    ($base:expr, $what:literal) => {
        [
            range($base, concat!($what, " band 1"), -90, 90),
            range($base + 0x10, concat!($what, " band 2"), -90, 90),
            range($base + 0x20, concat!($what, " band 3"), -90, 90),
            range($base + 0x30, concat!($what, " band 4"), -90, 90),
            range($base + 0x40, concat!($what, " band 5"), -90, 90),
            range($base + 0x50, concat!($what, " band 6"), -90, 90),
            range($base + 0x60, concat!($what, " band 7"), -90, 90),
            range($base + 0x70, concat!($what, " band 8"), -90, 90),
            range($base + 0x80, concat!($what, " band 9"), -90, 90),
            range($base + 0x90, concat!($what, " band 10"), -90, 90),
        ]
    };
}

const GAME_BANDS: [Setting; 10] = bands!(GAME_EQ_BANDS, "game EQ");
const MIC_BANDS: [Setting; 10] = bands!(MIC_EQ_BANDS, "mic EQ");

const OTHER: &[Setting] = &[
    one_of(GAME_EQ_PRESET, "game EQ preset", PRESET_NUMBERS),
    text(GAME_EQ_NAME, "game EQ preset name"),
    text(GAME_EQ_DELETE, "delete game EQ preset"),
    one_of(MIC_EQ_PRESET, "mic EQ preset", PRESET_NUMBERS),
    text(MIC_EQ_NAME, "mic EQ preset name"),
    text(MIC_EQ_DELETE, "delete mic EQ preset"),
    range(MIC_SENSITIVITY, "mic sensitivity", 0, 100),
    range(MONITORING_LEVEL, "mic monitoring level", 0, 100),
    range(AI_NOISE_REDUCTION, "A.I. noise reduction", 0, 1),
    range(NOISE_GATE, "noise gate", 0, 1),
    range(NOISE_GATE_LEVEL, "noise gate level", 0, 100),
    range(SUPERHUMAN_HEARING, "Superhuman Hearing", 0, 1),
    range(SUPERHUMAN_MODE, "Superhuman Hearing mode", 0, 2),
    range(SUPERHUMAN_LEVEL, "Superhuman Hearing level", 0, 100),
    range(ANC, "noise cancellation", 0, 1),
    range(ANC_LEVEL, "noise cancellation level", 0, 100),
    range(BUTTON_FUNCTION, "button function", 0, 2),
    range(WHEEL_FUNCTION, "wheel function", 1, 5),
    range(AUTO_SHUTOFF, "auto shut-off", 0, 4),
    range(WAKE_ON_MOTION, "wake on motion", 0, 1),
    range(HUB_LED_1, "charging hub LED 1", 0, 100),
    range(HUB_LED_2, "charging hub LED 2", 0, 100),
];

/// Find the writable setting for a key.
pub fn writable(key: u16) -> Option<&'static Setting> {
    GAME_BANDS.iter().chain(MIC_BANDS.iter()).chain(OTHER.iter()).find(|s| s.key == key)
}

/// Every writable key.
pub fn all_writable() -> impl Iterator<Item = &'static Setting> {
    GAME_BANDS.iter().chain(MIC_BANDS.iter()).chain(OTHER.iter())
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
        MONITORING_ENABLE => "mic monitoring enabled",
        0x1600 => "game custom presets in use",
        0x1620 => "mic custom presets in use",
        0x1700 => "game custom preset 1",
        0x1720 => "game custom preset 2",
        0x1740 => "game custom preset 3",
        0x1760 => "game custom preset 4",
        0x1780 => "game custom preset 5",
        0x1800 => "mic custom preset 1",
        0x1820 => "mic custom preset 2",
        0x1840 => "mic custom preset 3",
        0x1860 => "mic custom preset 4",
        0x1880 => "mic custom preset 5",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_values() {
        assert!(writable(GAME_EQ_PRESET).unwrap().values.accepts(16));
        assert!(!writable(GAME_EQ_PRESET).unwrap().values.accepts(5));
        assert!(writable(MIC_EQ_BANDS + 0x90).unwrap().values.accepts(-90));
        assert!(writable(GAME_EQ_NAME).unwrap().values.accepts_text("TEST"));
        assert!(!writable(GAME_EQ_NAME).unwrap().values.accepts_text("a\"b"));
        assert!(!writable(GAME_EQ_NAME).unwrap().values.accepts_text("seventeen chars!!"));
        assert!(writable(NAME).is_none());
        assert!(writable(MONITORING_ENABLE).is_none());
    }
}

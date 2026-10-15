//! The headset's state as named, typed values: the vocabulary shared by the daemon's D-Bus API, the GUI and the
//! CLI. Names are stable; add new ones rather than renaming.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::keys::{self, EqKeys};
use crate::{State, TransmitterState};

/// A typed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Bool(bool),
    Int(i64),
    Text(String),
    Ints(Vec<i64>),
    Texts(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Int,
    Text,
}

/// Values backed by exactly one key.
const SIMPLE: &[(&str, u16, Kind)] = &[
    ("name", keys::NAME, Kind::Text),
    ("firmware", keys::FIRMWARE, Kind::Text),
    ("charging", keys::CHARGING, Kind::Bool),
    ("volume", keys::VOLUME, Kind::Int),
    ("active_transmitter", keys::ACTIVE_TRANSMITTER, Kind::Int),
    ("mic_muted", keys::MIC_MUTED, Kind::Bool),
    ("mic_sensitivity", keys::MIC_SENSITIVITY, Kind::Int),
    ("monitoring_level", keys::MONITORING_LEVEL, Kind::Int),
    ("monitoring_enabled", keys::MONITORING_ENABLE, Kind::Bool),
    ("ai_noise_reduction", keys::AI_NOISE_REDUCTION, Kind::Bool),
    ("noise_gate", keys::NOISE_GATE, Kind::Bool),
    ("noise_gate_level", keys::NOISE_GATE_LEVEL, Kind::Int),
    ("superhuman", keys::SUPERHUMAN_HEARING, Kind::Bool),
    ("superhuman_mode", keys::SUPERHUMAN_MODE, Kind::Int),
    ("superhuman_level", keys::SUPERHUMAN_LEVEL, Kind::Int),
    ("anc", keys::ANC, Kind::Bool),
    ("anc_level", keys::ANC_LEVEL, Kind::Int),
    ("button_function", keys::BUTTON_FUNCTION, Kind::Int),
    ("wheel_function", keys::WHEEL_FUNCTION, Kind::Int),
    ("auto_shutoff", keys::AUTO_SHUTOFF, Kind::Int),
    ("wake_on_motion", keys::WAKE_ON_MOTION, Kind::Bool),
    ("game_eq_preset", keys::GAME_EQ_PRESET, Kind::Int),
    ("game_eq_name", keys::GAME_EQ_NAME, Kind::Text),
    ("mic_eq_preset", keys::MIC_EQ_PRESET, Kind::Int),
    ("mic_eq_name", keys::MIC_EQ_NAME, Kind::Text),
];

/// Every value known in `state`, by name. Values whose keys have not been seen are left out.
///
/// Besides the simple ones: `battery` (-1 while unknown), `game_eq_bands` / `mic_eq_bands` (10 values in tenths
/// of a dB), `game_eq_slots` / `mic_eq_slots` (the 5 custom preset names, "" for a free slot; slot n is preset
/// `15 + n`), `hub_led_1` / `hub_led_2`, and `transmitters` (one "<slot> <product id> <active|standby>" entry per
/// connected transmitter).
pub fn values(state: &State) -> BTreeMap<&'static str, Typed> {
    let mut out = BTreeMap::new();
    for &(name, key, kind) in SIMPLE {
        let v = match kind {
            Kind::Bool => state.flag(key).map(Typed::Bool),
            Kind::Int => state.int(key).map(Typed::Int),
            Kind::Text => state.text(key).map(|s| Typed::Text(s.to_string())),
        };
        if let Some(v) = v {
            out.insert(name, v);
        }
    }
    if state.raw(keys::BATTERY).is_some() {
        out.insert("battery", Typed::Int(state.battery().map_or(-1, i64::from)));
    }
    for (eq, bands, slots) in [(&keys::GAME_EQ, "game_eq_bands", "game_eq_slots"), (&keys::MIC_EQ, "mic_eq_bands", "mic_eq_slots")] {
        let e = state.equalizer(eq);
        if e.bands.iter().all(Option::is_some) {
            out.insert(bands, Typed::Ints(e.bands.iter().flatten().copied().collect()));
        }
        if let Some(names) = slot_names(state, eq) {
            out.insert(slots, Typed::Texts(names));
        }
    }
    let tx: Vec<String> = state
        .transmitters()
        .iter()
        .filter_map(|t| {
            let what = match t.state {
                TransmitterState::Active => "active",
                TransmitterState::Standby => "standby",
                _ => return None,
            };
            Some(format!("{} {} {what}", t.slot, t.product_id.to_lowercase()))
        })
        .collect();
    if state.raw(keys::TRANSMITTERS[0]).is_some() {
        out.insert("transmitters", Typed::Texts(tx));
    }
    if let Some(control) = state.raw(keys::TRANSMITTERS[0]).and_then(|v| v.get("control")).and_then(Value::as_array) {
        for (i, name) in [(1, "hub_led_1"), (2, "hub_led_2")] {
            if let Some(v) = control.get(i).and_then(Value::as_str).and_then(|s| s.parse().ok()) {
                out.insert(name, Typed::Int(v));
            }
        }
    }
    out
}

fn slot_names(state: &State, eq: &EqKeys) -> Option<Vec<String>> {
    eq.slots
        .iter()
        .map(|key| Some(state.raw(*key)?.get("name")?.as_str()?.to_string()))
        .collect()
}

/// The key and number to write for `Set(name, value)`. `None` if the name is unknown or not a single
/// writable key (bands and presets have their own operations; read-only values have none).
pub fn to_write(name: &str, value: &Typed) -> Option<(u16, i64)> {
    let key = match name {
        "hub_led_1" => keys::HUB_LED_1,
        "hub_led_2" => keys::HUB_LED_2,
        _ => SIMPLE.iter().find(|(n, _, _)| *n == name).map(|&(_, key, _)| key)?,
    };
    keys::writable(key)?;
    let number = match value {
        Typed::Bool(b) => i64::from(*b),
        Typed::Int(n) => *n,
        _ => return None,
    };
    Some((key, number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use serde_json::json;

    #[test]
    fn names_values_and_writes() {
        let mut s = State::new();
        for v in [
            json!({"OR": "GSI", "KVP": {"220": "My Headset", "240": "255", "2e0": "0"}}),
            json!({"OR": "SAF", "KVP": {"750": "1", "760": "100"}}),
            json!({"OR": "TX1", "KVP": {"400": {"info": [], "control": ["0", "40", "100"]}}}),
            json!({"OR": "AQG", "KVP": {"1210": "1", "1220": "0", "1230": "0", "1240": "0", "1250": "0", "1260": "0",
                                        "1270": "0", "1280": "0", "1290": "0", "12a0": "0", "12b0": "5"}}),
        ] {
            s.apply(&Document::from_json(&v).unwrap());
        }
        let v = values(&s);
        assert_eq!(v["battery"], Typed::Int(-1));
        assert_eq!(v["anc"], Typed::Bool(true));
        assert_eq!(v["wake_on_motion"], Typed::Bool(false));
        assert_eq!(v["hub_led_1"], Typed::Int(40));
        assert_eq!(v["game_eq_bands"], Typed::Ints(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 5]));
        assert!(!v.contains_key("mic_eq_bands"));

        assert_eq!(to_write("anc", &Typed::Bool(false)), Some((keys::ANC, 0)));
        assert_eq!(to_write("hub_led_2", &Typed::Int(10)), Some((keys::HUB_LED_2, 10)));
        assert_eq!(to_write("battery", &Typed::Int(10)), None);
        assert_eq!(to_write("name", &Typed::Text("x".into())), None);
    }
}

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::doc::Document;
use crate::keys;

/// Everything known about the headset: the values of all keys, kept current by applying documents.
///
/// Keys are unique across records (each record owns a range), so values live in one flat map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    values: BTreeMap<u16, Value>,
    records: BTreeMap<String, Vec<u16>>,
}

/// A CrossPlay transmitter slot (`TX1`..`TX4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmitter {
    pub slot: u8,
    pub state: TransmitterState,
    pub product_id: String,
    pub firmware: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransmitterState {
    Empty,
    /// Connected, but audio comes from another transmitter.
    Standby,
    /// The transmitter the headset currently plays from.
    Active,
    Unknown(i64),
}

/// An equalizer as the headset reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equalizer {
    /// `0` for unsaved custom bands, `1..=4` a built-in preset, `16..=20` a saved custom preset.
    pub preset: Option<i64>,
    pub name: Option<String>,
    /// Band gains in tenths of a dB.
    pub bands: Vec<Option<i64>>,
}

/// A saved custom EQ preset (`CG1..CG5`, `CM1..CM5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomPreset {
    /// The preset number that selects it: 16 for slot 1 … 20 for slot 5.
    pub number: i64,
    pub name: String,
    /// Band gains in tenths of a dB.
    pub bands: Vec<i64>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge a document; returns the keys whose value changed.
    pub fn apply(&mut self, doc: &Document) -> Vec<u16> {
        let mut changed = Vec::new();
        for (k, v) in &doc.kvp {
            let Ok(key) = u16::from_str_radix(k, 16) else { continue };
            let keys = self.records.entry(doc.record.clone()).or_default();
            if !keys.contains(&key) {
                keys.push(key);
            }
            if self.values.get(&key) != Some(v) {
                self.values.insert(key, v.clone());
                changed.push(key);
            }
        }
        changed
    }

    pub fn raw(&self, key: u16) -> Option<&Value> {
        self.values.get(&key)
    }

    pub fn text(&self, key: u16) -> Option<&str> {
        self.raw(key)?.as_str()
    }

    pub fn int(&self, key: u16) -> Option<i64> {
        self.text(key)?.trim().parse().ok()
    }

    pub fn flag(&self, key: u16) -> Option<bool> {
        self.int(key).map(|v| v != 0)
    }

    /// The record names seen, sorted by name.
    pub fn records(&self) -> impl Iterator<Item = &str> {
        self.records.keys().map(String::as_str)
    }

    /// The state as `{record: {hex key: value}}`, the shape of a snapshot.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        for (record, keys) in &self.records {
            let mut kvp = Map::new();
            for key in keys {
                if let Some(v) = self.values.get(key) {
                    kvp.insert(format!("{key:x}"), v.clone());
                }
            }
            out.insert(record.clone(), Value::Object(kvp));
        }
        Value::Object(out)
    }

    /// Battery percent; `None` when unknown (the headset reports 255 while it measures a fresh battery).
    pub fn battery(&self) -> Option<u8> {
        self.int(keys::BATTERY).filter(|v| (0..=100).contains(v)).map(|v| v as u8)
    }

    pub fn equalizer(&self, eq: &keys::EqKeys) -> Equalizer {
        Equalizer {
            preset: self.int(eq.preset),
            name: self.text(eq.name).map(str::to_string),
            bands: (0..keys::BAND_COUNT).map(|i| self.int(eq.band(i))).collect(),
        }
    }

    /// The saved custom presets; empty slots (no name) are left out.
    pub fn custom_presets(&self, eq: &keys::EqKeys) -> Vec<CustomPreset> {
        let mut out = Vec::new();
        for (i, key) in eq.slots.iter().enumerate() {
            let Some(slot) = self.raw(*key) else { continue };
            let name = slot.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let bands = slot.get("bands").and_then(Value::as_array).map(|b| {
                b.iter().filter_map(|v| v.as_str().and_then(|s| s.parse().ok())).collect()
            });
            out.push(CustomPreset {
                number: keys::FIRST_CUSTOM_PRESET + i as i64,
                name: name.to_string(),
                bands: bands.unwrap_or_default(),
            });
        }
        out
    }

    /// The preset number of the first empty custom slot, if the slots are known and one is free.
    pub fn free_custom_slot(&self, eq: &keys::EqKeys) -> Option<i64> {
        eq.slots.iter().enumerate().find_map(|(i, key)| {
            let name = self.raw(*key)?.get("name")?.as_str()?;
            name.is_empty().then_some(keys::FIRST_CUSTOM_PRESET + i as i64)
        })
    }

    pub fn transmitters(&self) -> Vec<Transmitter> {
        let mut out = Vec::new();
        for (i, key) in keys::TRANSMITTERS.iter().enumerate() {
            let Some(info) = self.raw(*key).and_then(|v| v.get("info")).and_then(Value::as_array) else {
                continue;
            };
            let field = |n: usize| info.get(n).and_then(Value::as_str).unwrap_or_default().to_string();
            let state = match field(0).parse::<i64>().unwrap_or(-1) {
                0 => TransmitterState::Empty,
                1 => TransmitterState::Standby,
                2 => TransmitterState::Active,
                n => TransmitterState::Unknown(n),
            };
            out.push(Transmitter {
                slot: i as u8 + 1,
                state,
                product_id: field(6),
                firmware: field(7),
                address: field(8),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;
    use serde_json::json;

    fn doc(v: Value) -> Document {
        Document::from_json(&v).unwrap()
    }

    #[test]
    fn applies_replies_and_updates() {
        let mut s = State::new();
        s.apply(&doc(json!({"OR": "GSI", "KVP": {"220": "My Headset", "240": "65", "250": "0"}})));
        assert_eq!(s.battery(), Some(65));
        assert_eq!(s.text(keys::NAME), Some("My Headset"));
        let changed = s.apply(&doc(json!({"UP": "GSI", "KVP": {"240": "255", "250": "0"}})));
        assert_eq!(changed, vec![keys::BATTERY]);
        assert_eq!(s.battery(), None);
        assert_eq!(s.flag(keys::CHARGING), Some(false));
    }

    #[test]
    fn reads_transmitters_and_equalizer() {
        let mut s = State::new();
        s.apply(&doc(json!({"OR": "TX2", "KVP": {"420": {"info": ["1","33","0","0","1","10F5","2288","4.107.703.0","CB:AF:2C:41:F2:F3"], "control": ["0","100","100"]}}})));
        s.apply(&doc(json!({"UP": "AQG", "KVP": {"1210": "2", "1220": "50", "1230": "50", "1240": "35", "12c0": "Bass Boost"}})));
        let tx = s.transmitters();
        assert_eq!(tx.len(), 1);
        assert_eq!((tx[0].slot, tx[0].state, tx[0].product_id.as_str()), (2, TransmitterState::Standby, "2288"));
        let eq = s.equalizer(&keys::GAME_EQ);
        assert_eq!(eq.preset, Some(2));
        assert_eq!(eq.name.as_deref(), Some("Bass Boost"));
        assert_eq!(&eq.bands[..4], &[Some(50), Some(50), Some(35), None]);
    }

    #[test]
    fn lists_custom_presets_and_free_slots() {
        let mut s = State::new();
        s.apply(&doc(json!({"OR": "CG1", "KVP": {"1700": {"name": "TEST", "bands": ["0","0","0","0","65","0","0","0","0","0"]}}})));
        s.apply(&doc(json!({"OR": "CG2", "KVP": {"1720": {"name": "", "bands": ["0","0","0","0","0","0","0","0","0","0"]}}})));
        let presets = s.custom_presets(&keys::GAME_EQ);
        assert_eq!(presets.len(), 1);
        assert_eq!((presets[0].number, presets[0].name.as_str(), presets[0].bands[4]), (16, "TEST", 65));
        assert_eq!(s.free_custom_slot(&keys::GAME_EQ), Some(17));
        assert_eq!(s.free_custom_slot(&keys::MIC_EQ), None);
    }

    #[test]
    fn snapshot_shape_round_trips_records() {
        let mut s = State::new();
        s.apply(&doc(json!({"OR": "SAF", "KVP": {"700": "1", "760": "100"}})));
        assert_eq!(s.to_json(), json!({"SAF": {"700": "1", "760": "100"}}));
    }
}

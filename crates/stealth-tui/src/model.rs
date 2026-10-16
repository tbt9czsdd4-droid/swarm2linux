//! The values from stealthd, and the controls of the Settings and Device pages: what each shows and changes.

use std::collections::BTreeMap;

use stealth_dbus::view::capitalized;
use tb_proto::keys;
use tb_proto::values::Typed;

/// Every value by name, as stealthd serves them.
#[derive(Debug, Default, Clone)]
pub struct Values(pub BTreeMap<String, Typed>);

impl Values {
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.0.get(name) {
            Some(Typed::Int(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.0.get(name), Some(Typed::Bool(true)))
    }

    pub fn text(&self, name: &str) -> &str {
        match self.0.get(name) {
            Some(Typed::Text(s)) => s,
            _ => "",
        }
    }

    pub fn ints(&self, name: &str) -> &[i64] {
        match self.0.get(name) {
            Some(Typed::Ints(v)) => v,
            _ => &[],
        }
    }

    pub fn texts(&self, name: &str) -> &[String] {
        match self.0.get(name) {
            Some(Typed::Texts(v)) => v,
            _ => &[],
        }
    }

    pub fn set(&mut self, name: &str, value: Typed) {
        self.0.insert(name.to_string(), value);
    }

    /// One band of an equalizer's `*_eq_bands`, in tenths of a dB.
    pub fn set_band(&mut self, eq: &str, band: usize, tenths: i64) {
        let name = format!("{eq}_eq_bands");
        let mut bands = self.ints(&name).to_vec();
        if bands.len() < 10 {
            bands.resize(10, 0);
        }
        bands[band] = tenths;
        self.0.insert(name, Typed::Ints(bands));
    }
}

/// A control on the Settings or Device page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    Volume,
    Superhuman,
    SuperhumanMode,
    SuperhumanLevel,
    Anc,
    AncLevel,
    Sensitivity,
    Monitoring,
    NoiseGate,
    NoiseGateLevel,
    AiNoiseReduction,
    AutoShutoff,
    WakeOnMotion,
    HubLed1,
    HubLed2,
    BatteryPopups,
}

/// What kind of control it is, and the value it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Slider { value: &'static str, min: i64, max: i64, centred: bool, ends: Option<(&'static str, &'static str)> },
    Toggle(&'static str),
    Choice(&'static str),
    /// Off | On
    Segmented(&'static str),
}

impl Control {
    pub fn label(self) -> &'static str {
        match self {
            Control::Volume => "Master volume",
            Control::Superhuman => "Superhuman Hearing",
            Control::SuperhumanMode => "Mode",
            Control::SuperhumanLevel => "Level",
            Control::Anc => "Active noise cancellation",
            Control::AncLevel => "Ambient ↔ noise cancellation",
            Control::Sensitivity => "Sensitivity",
            Control::Monitoring => "Monitoring",
            Control::NoiseGate => "Noise gate",
            Control::NoiseGateLevel => "Level",
            Control::AiNoiseReduction => "AI noise reduction",
            Control::AutoShutoff => "Auto shut-off",
            Control::WakeOnMotion => "Wake on motion",
            Control::HubLed1 => "LED 1",
            Control::HubLed2 => "LED 2",
            Control::BatteryPopups => "Battery pop-ups",
        }
    }

    pub fn kind(self) -> Kind {
        let slider = |value| Kind::Slider { value, min: 0, max: 100, centred: false, ends: None };
        match self {
            Control::Volume => slider("sink_volume"),
            Control::Superhuman => Kind::Toggle("superhuman"),
            Control::SuperhumanMode => Kind::Choice("superhuman_mode"),
            Control::SuperhumanLevel => slider("superhuman_level"),
            Control::Anc => Kind::Toggle("anc"),
            Control::AncLevel => {
                Kind::Slider { value: "anc_level", min: 0, max: 100, centred: true, ends: Some(("AMB", "ANC")) }
            }
            Control::Sensitivity => slider("mic_sensitivity"),
            Control::Monitoring => slider("monitoring_level"),
            Control::NoiseGate => Kind::Toggle("noise_gate"),
            Control::NoiseGateLevel => slider("noise_gate_level"),
            Control::AiNoiseReduction => Kind::Segmented("ai_noise_reduction"),
            Control::AutoShutoff => Kind::Choice("auto_shutoff"),
            Control::WakeOnMotion => Kind::Toggle("wake_on_motion"),
            Control::HubLed1 => slider("hub_led_1"),
            Control::HubLed2 => slider("hub_led_2"),
            // stealthd's own setting, not a headset value
            Control::BatteryPopups => Kind::Toggle("battery_notifications"),
        }
    }

    /// The options of a choice, as (value, label).
    pub fn options(self) -> Vec<(i64, String)> {
        match self {
            Control::SuperhumanMode => {
                keys::SUPERHUMAN_MODES.iter().enumerate().map(|(i, l)| (i as i64, capitalized(l))).collect()
            }
            Control::AutoShutoff => keys::AUTO_SHUTOFF_OPTIONS.iter().map(|&(v, l)| (v, capitalized(l))).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether the headset's volume can be set: stealthd knows it from PipeWire (`sink_volume` ≥ 0). Otherwise the
    /// headset's own reading is shown, read-only.
    pub fn read_only(self, values: &Values) -> bool {
        self == Control::Volume && values.int("sink_volume").is_none_or(|v| v < 0)
    }
}

/// The value a slider shows.
pub fn slider_value(control: Control, values: &Values) -> i64 {
    match control.kind() {
        _ if control.read_only(values) => values.int("volume").unwrap_or(0),
        Kind::Slider { value, .. } => values.int(value).unwrap_or(0),
        _ => 0,
    }
}

/// A row of a card on the Settings or Device page.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Control(Control),
    /// a muted explanation, wrapped
    Hint(String),
    /// a label and a value; the colour of an optional dot before the value
    Info(&'static str, String, Option<u32>),
    /// the battery bar
    Battery,
    /// a small heading inside a card
    Heading(&'static str),
    /// plain text
    Text(String),
    /// an empty line between groups
    Gap,
}

/// A card: a titled box of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

impl Card {
    pub fn controls(&self) -> impl Iterator<Item = Control> + '_ {
        self.rows.iter().filter_map(|r| if let Row::Control(c) = r { Some(*c) } else { None })
    }
}

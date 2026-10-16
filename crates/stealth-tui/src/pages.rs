//! What each page holds, apart from how it is drawn: the cards of the Settings and Device pages, the equalizer's
//! presets, and the earcup's controls and their functions.

use stealth_dbus::view::{self, capitalized};
use tb_proto::keys;

use crate::app::{App, EqTab, Page};
use crate::model::{Card, Control, Row, Values};
use crate::theme;

/// The cards of the Settings or Device page, and which go into the left and right column.
pub fn cards(page: Page, app: &App) -> (Vec<Card>, &'static [&'static [usize]]) {
    let v = &app.values;
    match page {
        Page::Settings => (settings(v), &[&[0], &[1]]),
        Page::Device => (device(app), &[&[0, 1], &[2, 3, 4]]),
        _ => (Vec::new(), &[]),
    }
}

/// The controls of a Settings or Device page, in focus order.
pub fn focus_list(page: Page, app: &App) -> Vec<Control> {
    let (cards, columns) = cards(page, app);
    columns.iter().flat_map(|col| col.iter()).flat_map(|&i| cards[i].controls().collect::<Vec<_>>()).collect()
}

fn settings(v: &Values) -> Vec<Card> {
    let volume_hint = if Control::Volume.read_only(v) {
        "Turn the volume wheel on the left earcup to change it."
    } else {
        "The headset's volume in the system's sound settings. The volume wheel on the left earcup changes it too."
    };
    let (mic, mic_colour) = if v.flag("mic_muted") { ("Muted", theme::BAD) } else { ("Live", theme::GOOD) };
    vec![
        Card {
            title: "Audio",
            rows: vec![
                Row::Control(Control::Volume),
                Row::Hint(volume_hint.into()),
                Row::Gap,
                Row::Control(Control::Superhuman),
                Row::Control(Control::SuperhumanMode),
                Row::Control(Control::SuperhumanLevel),
                Row::Gap,
                Row::Control(Control::Anc),
                Row::Control(Control::AncLevel),
                Row::Hint("Towards AMB lets surrounding sound in, towards ANC blocks it.".into()),
            ],
        },
        Card {
            title: "Microphone",
            rows: vec![
                Row::Info("Microphone", mic.into(), Some(mic_colour)),
                Row::Hint("Flip the microphone up to mute it, down to talk.".into()),
                Row::Gap,
                Row::Control(Control::Sensitivity),
                Row::Control(Control::Monitoring),
                Row::Gap,
                Row::Control(Control::NoiseGate),
                Row::Control(Control::NoiseGateLevel),
                Row::Gap,
                Row::Control(Control::AiNoiseReduction),
            ],
        },
    ]
}

fn device(app: &App) -> Vec<Card> {
    let v = &app.values;
    let connected = v.flag("connected");
    let headset = if !connected {
        "–"
    } else if v.flag("headset_link") {
        "connected"
    } else {
        "off or out of range"
    };
    let (lines, hub) = view::transmitter_lines(v.texts("transmitters"));
    let hub_colour = match hub.as_str() {
        "Active" => theme::GOOD,
        "Standby" => theme::WARN,
        _ => theme::DIM,
    };
    let mut status = vec![
        Row::Battery,
        Row::Gap,
        Row::Info("Transmitter", if connected { v.text("device").to_string() } else { "not found".into() }, None),
        Row::Info("Headset", headset.into(), None),
        Row::Info("Name", v.text("name").to_string(), None),
        Row::Info("Firmware", v.text("firmware").to_string(), None),
    ];
    if !lines.is_empty() {
        status.push(Row::Gap);
        status.push(Row::Heading("CrossPlay"));
        status.extend(lines.into_iter().map(Row::Text));
    }
    vec![
        Card { title: "Device status", rows: status },
        Card { title: "Power options", rows: vec![Row::Control(Control::AutoShutoff), Row::Control(Control::WakeOnMotion)] },
        Card {
            title: "Charging hub",
            rows: vec![
                Row::Info("Status", hub, Some(hub_colour)),
                Row::Gap,
                Row::Control(Control::HubLed1),
                Row::Control(Control::HubLed2),
            ],
        },
        Card {
            title: "Notifications",
            rows: vec![
                Row::Control(Control::BatteryPopups),
                Row::Hint("stealthd warns at 15% and 5% while the headset is not charging.".into()),
            ],
        },
        Card {
            title: "About",
            rows: vec![
                Row::Text(format!("swarm2linux {}", env!("CARGO_PKG_VERSION"))),
                Row::Hint(
                    "An unofficial Linux app for the Turtle Beach Stealth Pro II, not affiliated with Turtle Beach."
                        .into(),
                ),
                Row::Text("github.com/tbt9czsdd4-droid/swarm2linux".into()),
            ],
        },
    ]
}

/// A preset in the equalizer's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub number: i64,
    pub label: String,
    pub custom: bool,
}

/// The onboard presets, then the saved custom ones.
pub fn presets(tab: EqTab, values: &Values) -> Vec<Preset> {
    let eq = tab.keys();
    let onboard = eq.presets.iter().map(|&(number, label)| Preset { number, label: label.into(), custom: false });
    let custom = values.texts(&format!("{}_eq_slots", tab.name())).iter().enumerate().filter(|(_, n)| !n.is_empty()).map(
        |(i, name)| Preset { number: keys::FIRST_CUSTOM_PRESET + i as i64, label: name.clone(), custom: true },
    );
    onboard.chain(custom).collect()
}

/// Whether all five custom preset slots are in use.
pub fn slots_full(tab: EqTab, values: &Values) -> bool {
    let slots = values.texts(&format!("{}_eq_slots", tab.name()));
    slots.len() >= 5 && slots.iter().all(|s| !s.is_empty())
}

/// The left earcup's controls: title, and what it does (`None` for the two assignable ones, which show their
/// function).
pub const EARCUP: [(&str, Option<&str>); 5] = [
    ("Mode button", None),
    ("Volume wheel", Some("Master volume")),
    ("Assignable wheel", None),
    ("USB-C", Some("Charging and wired audio")),
    ("Power button", Some("Hold to switch on or off, press for Superhuman Hearing")),
];

/// The earcup rows that can be chosen: the mode button and the assignable wheel.
pub const ASSIGNABLE: [usize; 2] = [0, 2];

/// The value name and the functions of the mode button (0) or the assignable wheel (1).
pub fn functions(control: usize) -> (&'static str, Vec<(i64, String)>) {
    let (name, list): (_, &[(i64, &str)]) = if control == 0 {
        ("button_function", &keys::BUTTON_FUNCTIONS)
    } else {
        ("wheel_function", &keys::WHEEL_FUNCTIONS)
    };
    (name, list.iter().map(|&(v, l)| (v, capitalized(l))).collect())
}

/// The label of the function a control has now.
pub fn function_label(control: usize, values: &Values) -> String {
    let (name, list) = functions(control);
    let current = values.int(name);
    list.into_iter().find(|(v, _)| Some(*v) == current).map(|(_, l)| l).unwrap_or_else(|| "–".into())
}

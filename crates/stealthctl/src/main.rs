//! `stealthctl`: control a Turtle Beach Stealth Pro II from Linux.
//!
//! Talks to `stealthd` when it is running (it owns the device then), and to the device directly otherwise.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use race_hid::{HidrawDevice, discover, known_product};
use serde_json::Value;
use stealth_dbus::DaemonClient;
use tb_proto::keys::{self, EqKeys};
use tb_proto::values::{self, Typed};
use tb_proto::{Client, State};

#[derive(Parser)]
#[command(name = "stealthctl", version, about = "Control a Turtle Beach Stealth Pro II from Linux")]
struct Cli {
    /// hidraw node to use directly (default: go through stealthd if it runs, else the first device found)
    #[arg(long, global = true)]
    device: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List Turtle Beach hidraw devices
    Devices,
    /// Show battery, connection and settings
    Status,
    /// Print changes as they happen
    Watch {
        /// stop after this many seconds (default: run until interrupted)
        #[arg(long)]
        seconds: Option<u64>,
    },
    /// Game equalizer
    Eq {
        #[command(subcommand)]
        cmd: EqCmd,
    },
    /// Microphone equalizer
    MicEq {
        #[command(subcommand)]
        cmd: EqCmd,
    },
    /// Superhuman Hearing
    Shh {
        #[command(subcommand)]
        cmd: ShhCmd,
    },
    /// Active noise cancellation
    Anc {
        #[command(subcommand)]
        cmd: AncCmd,
    },
    /// Mic monitoring (hearing your own voice)
    Monitor {
        #[command(subcommand)]
        cmd: MonitorCmd,
    },
    /// Microphone
    Mic {
        #[command(subcommand)]
        cmd: MicCmd,
    },
    /// What the button above the wheels on the left earcup does
    Button { function: ButtonFunction },
    /// What the second wheel on the left earcup does
    Wheel { function: WheelFunction },
    /// Auto shut-off and wake on motion
    Power {
        #[command(subcommand)]
        cmd: PowerCmd,
    },
    /// Charging hub LEDs
    Hub {
        #[command(subcommand)]
        cmd: HubCmd,
    },
    /// Print every record as JSON (direct access only)
    Dump,
    /// Print one record as JSON, e.g. GSI or SAF (direct access only)
    Get { record: String },
    /// Write a raw key such as 0x1210 (direct access only; only keys Swarm II was seen writing)
    Set {
        #[arg(value_parser = parse_key)]
        key: u16,
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// Save all records to a JSON file (direct access only)
    Snapshot { file: PathBuf },
    /// Write back the settings saved in a snapshot (direct access only)
    Restore { file: PathBuf },
}

#[derive(Subcommand)]
enum EqCmd {
    /// Show the presets and the current bands
    List,
    /// Select a preset: a built-in one (game: signature, bass, bass-treble, vocal; mic: signature, full,
    /// clarity, smooth) or a saved custom preset by name
    Preset { preset: String },
    /// Set one band in dB (-9.0 to 9.0, steps of 0.5); switches to custom bands
    Band {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=10))]
        band: u8,
        #[arg(allow_hyphen_values = true)]
        db: f64,
    },
    /// Save the current bands as a custom preset (up to 5, names up to 16 characters)
    Save { name: String },
    /// Delete a custom preset
    Delete { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

impl OnOff {
    fn typed(self) -> Typed {
        Typed::Bool(matches!(self, OnOff::On))
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ShhMode {
    Legacy,
    Footsteps,
    Gunshots,
}

#[derive(Subcommand)]
enum ShhCmd {
    On,
    Off,
    /// Legacy, Footsteps or Gunshots
    Mode { mode: ShhMode },
    /// Level 0 to 100
    Level { level: i64 },
}

#[derive(Subcommand)]
enum AncCmd {
    On,
    Off,
    /// 0 = ambient (lets outside sound in) … 100 = full noise cancellation
    Level { level: i64 },
}

#[derive(Subcommand)]
enum MonitorCmd {
    /// How loud you hear your own voice, 0 to 100
    Level { level: i64 },
}

#[derive(Subcommand)]
enum MicCmd {
    /// Mic sensitivity 0 to 100
    Sensitivity { level: i64 },
    /// Noise gate on or off
    Gate { state: OnOff },
    /// Noise gate level 0 to 100
    GateLevel { level: i64 },
    /// A.I. noise reduction on or off
    Ai { state: OnOff },
}

#[derive(Clone, Copy, ValueEnum)]
enum ButtonFunction {
    /// Active noise cancellation on/off (default)
    Anc,
    /// Cycle game EQ presets
    CycleEq,
    /// Noise gate on/off
    NoiseGate,
}

#[derive(Clone, Copy, ValueEnum)]
enum WheelFunction {
    /// Mic monitoring volume (default)
    Monitoring,
    /// Game and chat mix
    ChatMix,
    /// Bass boost level
    Bass,
    /// Treble boost level
    Treble,
    /// Noise gate volume
    NoiseGate,
}

#[derive(Subcommand)]
enum PowerCmd {
    /// Turn the headset off after this many idle minutes: never, 5, 10, 20 or 30
    AutoOff { minutes: String },
    /// Wake the headset when it is picked up
    WakeOnMotion { state: OnOff },
}

#[derive(Subcommand)]
enum HubCmd {
    /// LED 1 brightness 0 to 100
    Led1 { level: i64 },
    /// LED 2 brightness 0 to 100
    Led2 { level: i64 },
}

fn parse_key(s: &str) -> Result<u16, String> {
    let digits = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(digits, 16).map_err(|e| format!("not a hex key: {e}"))
}

type Device = Client<HidrawDevice>;
type Values = BTreeMap<String, Typed>;

/// Where commands go: the running daemon, or the device itself.
enum Backend {
    Daemon(DaemonClient),
    Direct(Box<Device>),
}

impl Backend {
    fn open(device: Option<&Path>) -> Result<Self> {
        if device.is_none()
            && let Some(daemon) = DaemonClient::connect().context("checking for stealthd")?
        {
            return Ok(Backend::Daemon(daemon));
        }
        Ok(Backend::Direct(Box::new(tb_proto::open(device).context("opening the headset")?)))
    }

    /// The device, for commands that need raw access.
    fn direct(&mut self) -> Result<&mut Device> {
        match self {
            Backend::Direct(client) => Ok(client),
            Backend::Daemon(_) => bail!(
                "stealthd is running and owns the headset; stop it for raw access \
                 (systemctl --user stop stealthd), or use the named commands"
            ),
        }
    }

    fn values(&mut self) -> Result<Values> {
        match self {
            Backend::Daemon(d) => Ok(d.values()?),
            Backend::Direct(client) => {
                let state = read_all(client)?;
                let mut v: Values = values::values(&state).into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                v.insert("connected".into(), Typed::Bool(true));
                Ok(v)
            }
        }
    }

    fn set(&mut self, name: &str, value: Typed) -> Result<()> {
        match self {
            Backend::Daemon(d) => Ok(d.set(name, &value)?),
            Backend::Direct(client) => {
                let (key, number) = values::to_write(name, &value).with_context(|| format!("{name} is not writable"))?;
                client.set(key, number)?;
                show_updates(client)
            }
        }
    }

    fn set_band(&mut self, eq: &EqKeys, band: u8, tenths: i64) -> Result<()> {
        match self {
            Backend::Daemon(d) => Ok(d.set_band(eq_name(eq), u32::from(band), tenths as i32)?),
            Backend::Direct(client) => {
                client.set(eq.band(usize::from(band - 1)), tenths)?;
                show_updates(client)
            }
        }
    }

    fn save_preset(&mut self, eq: &EqKeys, name: &str) -> Result<()> {
        match self {
            Backend::Daemon(d) => Ok(d.save_preset(eq_name(eq), name)?),
            Backend::Direct(client) => {
                let state = read_eq(client, eq)?;
                client.save_new_preset(&state, eq, name)?;
                show_updates(client)
            }
        }
    }

    fn delete_preset(&mut self, eq: &EqKeys, name: &str) -> Result<()> {
        match self {
            Backend::Daemon(d) => Ok(d.delete_preset(eq_name(eq), name)?),
            Backend::Direct(client) => {
                let state = read_eq(client, eq)?;
                client.delete_named_preset(&state, eq, name)?;
                show_updates(client)
            }
        }
    }
}

fn eq_name(eq: &EqKeys) -> &'static str {
    if eq.preset == keys::GAME_EQ_PRESET { "game" } else { "mic" }
}

fn main() -> Result<()> {
    // end quietly when the output is cut off (`stealthctl eq list | head`) instead of panicking
    // SAFETY: setting a signal disposition to its default before any threads exist.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    if let Cmd::Devices = cli.cmd {
        return devices();
    }
    let mut backend = Backend::open(cli.device.as_deref())?;
    let b = &mut backend;

    match cli.cmd {
        Cmd::Devices => unreachable!(),
        Cmd::Status => status(b),
        Cmd::Watch { seconds } => watch(b, seconds.map(Duration::from_secs)),
        Cmd::Eq { cmd } => equalizer(b, &keys::GAME_EQ, cmd),
        Cmd::MicEq { cmd } => equalizer(b, &keys::MIC_EQ, cmd),
        Cmd::Shh { cmd } => match cmd {
            ShhCmd::On => b.set("superhuman", Typed::Bool(true)),
            ShhCmd::Off => b.set("superhuman", Typed::Bool(false)),
            ShhCmd::Mode { mode } => b.set("superhuman_mode", Typed::Int(mode as i64)),
            ShhCmd::Level { level } => b.set("superhuman_level", Typed::Int(level)),
        },
        Cmd::Anc { cmd } => match cmd {
            AncCmd::On => b.set("anc", Typed::Bool(true)),
            AncCmd::Off => b.set("anc", Typed::Bool(false)),
            AncCmd::Level { level } => b.set("anc_level", Typed::Int(level)),
        },
        Cmd::Monitor { cmd } => match cmd {
            MonitorCmd::Level { level } => b.set("monitoring_level", Typed::Int(level)),
        },
        Cmd::Mic { cmd } => match cmd {
            MicCmd::Sensitivity { level } => b.set("mic_sensitivity", Typed::Int(level)),
            MicCmd::Gate { state } => b.set("noise_gate", state.typed()),
            MicCmd::GateLevel { level } => b.set("noise_gate_level", Typed::Int(level)),
            MicCmd::Ai { state } => b.set("ai_noise_reduction", state.typed()),
        },
        Cmd::Button { function } => b.set("button_function", Typed::Int(function as i64)),
        Cmd::Wheel { function } => b.set("wheel_function", Typed::Int(function as i64 + 1)),
        Cmd::Power { cmd } => match cmd {
            PowerCmd::AutoOff { minutes } => {
                let n = keys::AUTO_SHUTOFF_OPTIONS
                    .iter()
                    .find(|(_, label)| label.split(' ').next() == Some(minutes.as_str()))
                    .map(|(n, _)| *n)
                    .context("auto-off takes never, 5, 10, 20 or 30")?;
                b.set("auto_shutoff", Typed::Int(n))
            }
            PowerCmd::WakeOnMotion { state } => b.set("wake_on_motion", state.typed()),
        },
        Cmd::Hub { cmd } => match cmd {
            HubCmd::Led1 { level } => b.set("hub_led_1", Typed::Int(level)),
            HubCmd::Led2 { level } => b.set("hub_led_2", Typed::Int(level)),
        },
        Cmd::Dump => {
            let state = read_all(b.direct()?)?;
            println!("{}", serde_json::to_string_pretty(&state.to_json())?);
            Ok(())
        }
        Cmd::Get { record } => {
            let doc = b.direct()?.subscribe(&record)?;
            println!("{}", serde_json::to_string_pretty(&Value::Object(doc.kvp))?);
            Ok(())
        }
        Cmd::Set { key, value } => {
            let client = b.direct()?;
            match value.parse::<i64>() {
                Ok(n) => client.set(key, n)?,
                Err(_) => client.set_text(key, &value)?,
            }
            show_updates(client)
        }
        Cmd::Snapshot { file } => {
            let state = read_all(b.direct()?)?;
            fs::write(&file, serde_json::to_string_pretty(&state.to_json())? + "\n")?;
            println!("saved {}", file.display());
            Ok(())
        }
        Cmd::Restore { file } => restore(b.direct()?, &file),
    }
}

fn devices() -> Result<()> {
    for d in discover()? {
        let what = known_product(d.pid).unwrap_or("unknown product");
        let usable = if d.vendor_collection { "" } else { " (no vendor collection)" };
        println!("{}  10f5:{:04x}  {}  [{what}]{usable}", d.path.display(), d.pid, d.name);
    }
    if DaemonClient::connect().ok().flatten().is_some() {
        println!("stealthd is running and owns the headset");
    }
    Ok(())
}

/// Subscribe to every record the device lists.
fn read_all(client: &mut Device) -> Result<State> {
    let mut state = State::new();
    for record in client.index()? {
        match client.subscribe(&record) {
            Ok(doc) => {
                state.apply(&doc);
            }
            Err(e) => eprintln!("warning: {record}: {e}"),
        }
    }
    for doc in client.take_updates() {
        state.apply(&doc);
    }
    Ok(state)
}

/// Subscribe to the records an equalizer needs.
fn read_eq(client: &mut Device, eq: &EqKeys) -> Result<State> {
    let mut state = State::new();
    let records: &[&str] = if eq.preset == keys::GAME_EQ_PRESET {
        &["AQG", "CEC", "CG1", "CG2", "CG3", "CG4", "CG5"]
    } else {
        &["AQM", "CEC", "CM1", "CM2", "CM3", "CM4", "CM5"]
    };
    for record in records {
        state.apply(&client.subscribe(record)?);
    }
    Ok(state)
}

/// Show what the headset pushed back after a direct write.
fn show_updates(client: &mut Device) -> Result<()> {
    for doc in client.wait_updates(Duration::from_millis(400))? {
        for (k, v) in &doc.kvp {
            println!("{} = {}", describe_key(k), show_json(v));
        }
    }
    Ok(())
}

fn int(v: &Values, name: &str) -> Option<i64> {
    match v.get(name) {
        Some(Typed::Int(n)) => Some(*n),
        _ => None,
    }
}

fn text<'a>(v: &'a Values, name: &str) -> Option<&'a str> {
    match v.get(name) {
        Some(Typed::Text(s)) => Some(s),
        _ => None,
    }
}

fn on(v: &Values, name: &str) -> &'static str {
    match v.get(name) {
        Some(Typed::Bool(true)) => "on",
        Some(Typed::Bool(false)) => "off",
        _ => "?",
    }
}

fn num(v: &Values, name: &str) -> String {
    int(v, name).map_or("?".to_string(), |n| n.to_string())
}

fn pick(v: &Values, name: &str, options: &[(i64, &'static str)]) -> &'static str {
    int(v, name).and_then(|n| options.iter().find(|(o, _)| *o == n)).map_or("?", |(_, label)| label)
}

fn bands_text(v: &Values, name: &str) -> String {
    match v.get(name) {
        Some(Typed::Ints(b)) => b.iter().map(|x| format!("{:+.1}", *x as f64 / 10.0)).collect::<Vec<_>>().join(" "),
        _ => "?".to_string(),
    }
}

/// Custom preset slots: (preset number, name) for the used ones.
fn custom_presets(v: &Values, eq: &EqKeys) -> Vec<(i64, String)> {
    match v.get(&format!("{}_eq_slots", eq_name(eq))) {
        Some(Typed::Texts(names)) => names
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_empty())
            .map(|(i, n)| (keys::FIRST_CUSTOM_PRESET + i as i64, n.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

fn preset_name(v: &Values, eq: &EqKeys) -> String {
    match int(v, &format!("{}_eq_preset", eq_name(eq))) {
        Some(0) => "custom (unsaved)".to_string(),
        Some(n) => eq
            .presets
            .iter()
            .find(|(p, _)| *p == n)
            .map(|(_, name)| name.to_string())
            .or_else(|| custom_presets(v, eq).into_iter().find(|(p, _)| *p == n).map(|(_, name)| format!("{name} (custom)")))
            .unwrap_or_else(|| format!("preset {n}")),
        None => "?".to_string(),
    }
}

fn status(b: &mut Backend) -> Result<()> {
    let v = b.values()?;
    if v.get("connected") != Some(&Typed::Bool(true)) {
        println!("No headset connected (stealthd is waiting for one).");
        return Ok(());
    }
    let via = match b {
        Backend::Daemon(_) => format!("via stealthd, {}", text(&v, "device").unwrap_or("?")),
        Backend::Direct(_) => "direct".to_string(),
    };
    println!("Connection    {via}");
    println!("Firmware      {}", text(&v, "firmware").unwrap_or("?"));
    let battery = match int(&v, "battery") {
        Some(n) if n >= 0 => format!("{n}%"),
        _ => "unknown".to_string(),
    };
    let charging = if on(&v, "charging") == "on" { ", charging" } else { "" };
    let link = match v.get("headset_link") {
        Some(Typed::Bool(false)) => " (headset off or out of range)",
        _ => "",
    };
    println!("Headset       {}: battery {battery}{charging}{link}", text(&v, "name").unwrap_or("?"));
    let tx = match v.get("transmitters") {
        Some(Typed::Texts(t)) if !t.is_empty() => t.iter().map(|e| format!("TX{e}")).collect::<Vec<_>>().join(", "),
        _ => "none".to_string(),
    };
    println!("Transmitters  {tx}");
    println!("Volume        {}", num(&v, "volume"));
    println!("Game EQ       {}  [{}] dB", preset_name(&v, &keys::GAME_EQ), bands_text(&v, "game_eq_bands"));
    println!("Mic EQ        {}  [{}] dB", preset_name(&v, &keys::MIC_EQ), bands_text(&v, "mic_eq_bands"));
    let mode = int(&v, "superhuman_mode").and_then(|m| keys::SUPERHUMAN_MODES.get(m as usize).copied()).unwrap_or("?");
    println!("Superhuman    {} ({mode}, level {})", on(&v, "superhuman"), num(&v, "superhuman_level"));
    println!("ANC           {} (level {}: 0 ambient … 100 full ANC)", on(&v, "anc"), num(&v, "anc_level"));
    let muted = if on(&v, "mic_muted") == "on" { "muted" } else { "live" };
    println!(
        "Mic           {muted}, sensitivity {}, A.I. noise reduction {}",
        num(&v, "mic_sensitivity"),
        on(&v, "ai_noise_reduction")
    );
    println!("Noise gate    {} (level {})", on(&v, "noise_gate"), num(&v, "noise_gate_level"));
    println!("Monitoring    level {}", num(&v, "monitoring_level"));
    println!("Button        {}", pick(&v, "button_function", &keys::BUTTON_FUNCTIONS));
    println!("Wheel         {}", pick(&v, "wheel_function", &keys::WHEEL_FUNCTIONS));
    println!(
        "Power         auto shut-off {}, wake on motion {}",
        pick(&v, "auto_shutoff", &keys::AUTO_SHUTOFF_OPTIONS),
        on(&v, "wake_on_motion")
    );
    println!("Charging hub  LED 1 {}, LED 2 {}", num(&v, "hub_led_1"), num(&v, "hub_led_2"));
    Ok(())
}

fn equalizer(b: &mut Backend, eq: &EqKeys, cmd: EqCmd) -> Result<()> {
    let name = eq_name(eq);
    match cmd {
        EqCmd::List => {
            let v = b.values()?;
            println!("{}: {}", eq.what, preset_name(&v, eq));
            let bands = match v.get(&format!("{name}_eq_bands")) {
                Some(Typed::Ints(list)) => list.clone(),
                _ => Vec::new(),
            };
            for (i, freq) in eq.frequencies.iter().enumerate() {
                let db = bands.get(i).map_or("?".to_string(), |x| format!("{:+.1} dB", *x as f64 / 10.0));
                println!("  band {:2}  {freq:>8}  {db}", i + 1);
            }
            println!("built-in presets:");
            for (n, preset) in eq.presets {
                println!("  {n:2}  {preset}");
            }
            let custom = custom_presets(&v, eq);
            println!("custom presets:{}", if custom.is_empty() { " none" } else { "" });
            for (n, preset) in custom {
                println!("  {n:2}  {preset}");
            }
            Ok(())
        }
        EqCmd::Preset { preset } => {
            let number = match find_builtin(eq, &preset) {
                Some(n) => n,
                None => {
                    let v = b.values()?;
                    match custom_presets(&v, eq).into_iter().find(|(_, p)| p.eq_ignore_ascii_case(&preset)) {
                        Some((n, _)) => n,
                        None => bail!("no {} preset called {preset:?}; see `list`", eq.what),
                    }
                }
            };
            b.set(&format!("{name}_eq_preset"), Typed::Int(number))
        }
        EqCmd::Band { band, db } => b.set_band(eq, band, ((db * 10.0) / 5.0).round() as i64 * 5),
        EqCmd::Save { name } => b.save_preset(eq, &name),
        EqCmd::Delete { name } => b.delete_preset(eq, &name),
    }
}

fn find_builtin(eq: &EqKeys, wanted: &str) -> Option<i64> {
    let wanted = wanted.to_ascii_lowercase().replace(['-', '_', '&', ' '], "");
    eq.presets.iter().find_map(|(n, name)| {
        let name = name.to_ascii_lowercase().replace(['&', ' '], "").replace("sound", "");
        (name == wanted || name.starts_with(&wanted) || n.to_string() == wanted).then_some(*n)
    })
}

fn watch(b: &mut Backend, limit: Option<Duration>) -> Result<()> {
    let start = Instant::now();
    match b {
        Backend::Daemon(d) => {
            eprintln!("watching through stealthd; Ctrl-C to stop");
            for changed in d.changes()? {
                for (name, value) in changed {
                    println!("{:8.1}s  {name} = {}", start.elapsed().as_secs_f64(), show_typed(&value));
                }
                if limit.is_some_and(|l| start.elapsed() >= l) {
                    break;
                }
            }
        }
        Backend::Direct(client) => {
            let mut state = read_all(client)?;
            let mut link = client.headset_link();
            eprintln!("watching; Ctrl-C to stop");
            while limit.is_none_or(|l| start.elapsed() < l) {
                let docs = client.wait_updates(Duration::from_millis(500))?;
                if client.headset_link() != link {
                    link = client.headset_link();
                    let what = if link == Some(true) { "up" } else { "down" };
                    println!("{:8.1}s  headset link {what}", start.elapsed().as_secs_f64());
                }
                for doc in docs {
                    let before = state.clone();
                    for key in state.apply(&doc) {
                        let old = before.raw(key).map_or("-".to_string(), show_json);
                        let new = state.raw(key).map_or("-".to_string(), show_json);
                        let k = format!("{key:x}");
                        println!("{:8.1}s  {:4} {}: {old} -> {new}", start.elapsed().as_secs_f64(), doc.record, describe_key(&k));
                    }
                }
            }
        }
    }
    Ok(())
}

fn restore(client: &mut Device, file: &PathBuf) -> Result<()> {
    let snap: Value = serde_json::from_str(&fs::read_to_string(file)?)?;
    let saved = |key: u16| -> Option<i64> {
        snap.as_object()?.values().find_map(|kvp| kvp.get(format!("{key:x}"))?.as_str()?.parse().ok())
    };
    let current = read_all(client)?;

    // Selecting an EQ preset rewrites the bands and turns Superhuman Hearing off, so the EQs go first and
    // Superhuman Hearing last. Custom presets themselves are not recreated.
    let mut keys_in_order = Vec::new();
    for eq in [&keys::GAME_EQ, &keys::MIC_EQ] {
        match saved(eq.preset) {
            Some(0) => keys_in_order.extend((0..keys::BAND_COUNT).map(|i| eq.band(i))),
            Some(_) => keys_in_order.push(eq.preset),
            None => {}
        }
    }
    let is_eq = |k: u16| (keys::GAME_EQ_PRESET..=keys::MIC_EQ_DELETE).contains(&k);
    for s in keys::all_writable() {
        if !is_eq(s.key) && s.key != keys::SUPERHUMAN_HEARING && !matches!(s.values, keys::Values::Text(_)) {
            keys_in_order.push(s.key);
        }
    }
    keys_in_order.push(keys::SUPERHUMAN_HEARING);

    let todo: Vec<(u16, i64)> = keys_in_order
        .into_iter()
        .filter_map(|key| saved(key).filter(|&v| current.int(key) != Some(v)).map(|v| (key, v)))
        .collect();
    if todo.is_empty() {
        println!("already matches the snapshot");
        return Ok(());
    }
    for &(key, value) in &todo {
        println!("{} -> {value}", describe_key(&format!("{key:x}")));
        client.set(key, value).with_context(|| format!("writing {key:#x} = {value}"))?;
    }
    show_updates(client)
}

fn describe_key(hex: &str) -> String {
    match u16::from_str_radix(hex, 16).ok().and_then(keys::label) {
        Some(label) => format!("{label} ({hex})"),
        None => hex.to_string(),
    }
}

fn show_json(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn show_typed(v: &Typed) -> String {
    match v {
        Typed::Bool(b) => b.to_string(),
        Typed::Int(n) => n.to_string(),
        Typed::Text(s) => s.clone(),
        Typed::Ints(list) => format!("{list:?}"),
        Typed::Texts(list) => format!("{list:?}"),
    }
}

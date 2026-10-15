//! `stealthctl`: control a Turtle Beach Stealth Pro II from Linux.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use race_hid::{HidrawDevice, discover, known_product};
use serde_json::Value;
use tb_proto::keys::{self, EqKeys};
use tb_proto::{Client, State, TransmitterState};

#[derive(Parser)]
#[command(name = "stealthctl", version, about = "Control a Turtle Beach Stealth Pro II from Linux")]
struct Cli {
    /// hidraw node to use (default: the first Turtle Beach transmitter or headset)
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
    /// Print every record as JSON
    Dump,
    /// Print one record as JSON, e.g. GSI or SAF
    Get { record: String },
    /// Print changes as the headset pushes them
    Watch {
        /// stop after this many seconds (default: run until interrupted)
        #[arg(long)]
        seconds: Option<u64>,
    },
    /// Write a raw key such as 0x1210 (only keys Swarm II was seen writing)
    Set {
        #[arg(value_parser = parse_key)]
        key: u16,
        #[arg(allow_hyphen_values = true)]
        value: String,
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
    /// Save all records to a JSON file
    Snapshot { file: PathBuf },
    /// Write back the settings saved in a snapshot
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
    fn value(self) -> i64 {
        match self {
            OnOff::On => 1,
            OnOff::Off => 0,
        }
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

fn parse_key(s: &str) -> Result<u16, String> {
    let digits = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(digits, 16).map_err(|e| format!("not a hex key: {e}"))
}

type Headset = Client<HidrawDevice>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Devices = cli.cmd {
        return devices();
    }
    let mut client = tb_proto::open(cli.device.as_deref()).context("opening the headset")?;

    match cli.cmd {
        Cmd::Devices => unreachable!(),
        Cmd::Status => status(&mut client),
        Cmd::Dump => {
            let state = read_all(&mut client)?;
            println!("{}", serde_json::to_string_pretty(&state.to_json())?);
            Ok(())
        }
        Cmd::Get { record } => {
            let doc = client.subscribe(&record)?;
            println!("{}", serde_json::to_string_pretty(&Value::Object(doc.kvp))?);
            Ok(())
        }
        Cmd::Watch { seconds } => watch(&mut client, seconds.map(Duration::from_secs)),
        Cmd::Set { key, value } => {
            match value.parse::<i64>() {
                Ok(n) => client.set(key, n)?,
                Err(_) => client.set_text(key, &value)?,
            }
            show_updates(&mut client)
        }
        Cmd::Eq { cmd } => equalizer(&mut client, &keys::GAME_EQ, cmd),
        Cmd::MicEq { cmd } => equalizer(&mut client, &keys::MIC_EQ, cmd),
        Cmd::Shh { cmd } => match cmd {
            ShhCmd::On => write(&mut client, &[(keys::SUPERHUMAN_HEARING, 1)]),
            ShhCmd::Off => write(&mut client, &[(keys::SUPERHUMAN_HEARING, 0)]),
            ShhCmd::Mode { mode } => write(&mut client, &[(keys::SUPERHUMAN_MODE, mode as i64)]),
            ShhCmd::Level { level } => write(&mut client, &[(keys::SUPERHUMAN_LEVEL, level)]),
        },
        Cmd::Anc { cmd } => match cmd {
            AncCmd::On => write(&mut client, &[(keys::ANC, 1)]),
            AncCmd::Off => write(&mut client, &[(keys::ANC, 0)]),
            AncCmd::Level { level } => write(&mut client, &[(keys::ANC_LEVEL, level)]),
        },
        Cmd::Monitor { cmd } => match cmd {
            MonitorCmd::Level { level } => write(&mut client, &[(keys::MONITORING_LEVEL, level)]),
        },
        Cmd::Mic { cmd } => match cmd {
            MicCmd::Sensitivity { level } => write(&mut client, &[(keys::MIC_SENSITIVITY, level)]),
            MicCmd::Gate { state } => write(&mut client, &[(keys::NOISE_GATE, state.value())]),
            MicCmd::GateLevel { level } => write(&mut client, &[(keys::NOISE_GATE_LEVEL, level)]),
            MicCmd::Ai { state } => write(&mut client, &[(keys::AI_NOISE_REDUCTION, state.value())]),
        },
        Cmd::Snapshot { file } => {
            let state = read_all(&mut client)?;
            fs::write(&file, serde_json::to_string_pretty(&state.to_json())? + "\n")?;
            println!("saved {}", file.display());
            Ok(())
        }
        Cmd::Restore { file } => restore(&mut client, &file),
    }
}

fn devices() -> Result<()> {
    for d in discover()? {
        let what = known_product(d.pid).unwrap_or("unknown product");
        let usable = if d.vendor_collection { "" } else { " (no vendor collection)" };
        println!("{}  10f5:{:04x}  {}  [{what}]{usable}", d.path.display(), d.pid, d.name);
    }
    Ok(())
}

/// Subscribe to every record the device lists.
fn read_all(client: &mut Headset) -> Result<State> {
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
fn read_eq(client: &mut Headset, eq: &EqKeys) -> Result<State> {
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

/// Write keys in order, then show what the headset pushed back.
fn write(client: &mut Headset, writes: &[(u16, i64)]) -> Result<()> {
    for &(key, value) in writes {
        client.set(key, value).with_context(|| format!("writing {key:#x} = {value}"))?;
    }
    show_updates(client)
}

fn show_updates(client: &mut Headset) -> Result<()> {
    for doc in client.wait_updates(Duration::from_millis(400))? {
        for (k, v) in &doc.kvp {
            println!("{} = {}", describe_key(k), show_value(v));
        }
    }
    Ok(())
}

fn equalizer(client: &mut Headset, eq: &EqKeys, cmd: EqCmd) -> Result<()> {
    match cmd {
        EqCmd::List => {
            let s = read_eq(client, eq)?;
            let current = s.equalizer(eq);
            println!("{}: {}", eq.what, preset_name(eq, &s, current.preset));
            for (i, freq) in eq.frequencies.iter().enumerate() {
                let db = current.bands[i].map_or("?".to_string(), |v| format!("{:+.1} dB", v as f64 / 10.0));
                println!("  band {:2}  {freq:>8}  {db}", i + 1);
            }
            println!("built-in presets:");
            for (n, name) in eq.presets {
                println!("  {n:2}  {name}");
            }
            let custom = s.custom_presets(eq);
            println!("custom presets:{}", if custom.is_empty() { " none" } else { "" });
            for p in custom {
                let bands: Vec<String> = p.bands.iter().map(|v| format!("{:+.1}", *v as f64 / 10.0)).collect();
                println!("  {:2}  {}  [{}]", p.number, p.name, bands.join(" "));
            }
            Ok(())
        }
        EqCmd::Preset { preset } => {
            let number = match find_builtin(eq, &preset) {
                Some(n) => n,
                None => {
                    let s = read_eq(client, eq)?;
                    match s.custom_presets(eq).iter().find(|p| p.name.eq_ignore_ascii_case(&preset)) {
                        Some(p) => p.number,
                        None => bail!("no {} preset called {preset:?}; see `list`", eq.what),
                    }
                }
            };
            write(client, &[(eq.preset, number)])
        }
        EqCmd::Band { band, db } => {
            let value = ((db * 10.0) / 5.0).round() as i64 * 5;
            write(client, &[(eq.band(usize::from(band - 1)), value)])
        }
        EqCmd::Save { name } => {
            let s = read_eq(client, eq)?;
            if s.custom_presets(eq).iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
                bail!("a {} preset called {name:?} already exists", eq.what);
            }
            let slot = s.free_custom_slot(eq).context("all 5 custom preset slots are in use")?;
            let mut bands = [0i64; keys::BAND_COUNT];
            for (i, b) in s.equalizer(eq).bands.iter().enumerate() {
                bands[i] = b.context("current bands unknown")?;
            }
            client.save_preset(eq, slot, &bands, &name)?;
            show_updates(client)
        }
        EqCmd::Delete { name } => {
            let s = read_eq(client, eq)?;
            let Some(p) = s.custom_presets(eq).into_iter().find(|p| p.name.eq_ignore_ascii_case(&name)) else {
                bail!("no custom {} preset called {name:?}", eq.what);
            };
            client.delete_preset(eq, &p.name)?;
            show_updates(client)
        }
    }
}

fn find_builtin(eq: &EqKeys, wanted: &str) -> Option<i64> {
    let wanted = wanted.to_ascii_lowercase().replace(['-', '_', '&', ' '], "");
    eq.presets.iter().find_map(|(n, name)| {
        let name = name.to_ascii_lowercase().replace(['&', ' '], "").replace("sound", "");
        (name == wanted || name.starts_with(&wanted) || n.to_string() == wanted).then_some(*n)
    })
}

fn preset_name(eq: &EqKeys, s: &State, preset: Option<i64>) -> String {
    match preset {
        Some(0) => "custom (unsaved)".to_string(),
        Some(n) => eq
            .presets
            .iter()
            .find(|(p, _)| *p == n)
            .map(|(_, name)| name.to_string())
            .or_else(|| s.custom_presets(eq).into_iter().find(|p| p.number == n).map(|p| format!("{} (custom)", p.name)))
            .unwrap_or_else(|| format!("preset {n}")),
        None => "?".to_string(),
    }
}

fn status(client: &mut Headset) -> Result<()> {
    let s = read_all(client)?;
    let on = |key| match s.flag(key) {
        Some(true) => "on",
        Some(false) => "off",
        None => "?",
    };
    let num = |key| s.int(key).map_or("?".to_string(), |v| v.to_string());

    println!("Firmware      {}", s.text(keys::FIRMWARE).unwrap_or("?"));
    let battery = s.battery().map_or("unknown".to_string(), |b| format!("{b}%"));
    let charging = if s.flag(keys::CHARGING) == Some(true) { ", charging" } else { "" };
    println!("Headset       {}: battery {battery}{charging}", s.text(keys::NAME).unwrap_or("?"));

    let tx: Vec<String> = s
        .transmitters()
        .iter()
        .filter(|t| t.state != TransmitterState::Empty)
        .map(|t| {
            let state = match t.state {
                TransmitterState::Active => "active".to_string(),
                TransmitterState::Standby => "standby".to_string(),
                TransmitterState::Empty => "empty".to_string(),
                TransmitterState::Unknown(n) => format!("state {n}"),
            };
            format!("TX{} 10f5:{} {state}", t.slot, t.product_id.to_lowercase())
        })
        .collect();
    println!("Transmitters  {}", if tx.is_empty() { "none".to_string() } else { tx.join(", ") });
    println!("Volume        {}", num(keys::VOLUME));

    for eq in [&keys::GAME_EQ, &keys::MIC_EQ] {
        let e = s.equalizer(eq);
        let bands: Vec<String> = e.bands.iter().map(|b| b.map_or("?".into(), |v| format!("{:+.1}", v as f64 / 10.0))).collect();
        let label = if eq.preset == keys::GAME_EQ_PRESET { "Game EQ" } else { "Mic EQ" };
        println!("{label:<13} {}  [{}] dB", preset_name(eq, &s, e.preset), bands.join(" "));
    }
    let mode = s.int(keys::SUPERHUMAN_MODE).and_then(|m| keys::SUPERHUMAN_MODES.get(m as usize).copied()).unwrap_or("?");
    println!("Superhuman    {} ({mode}, level {})", on(keys::SUPERHUMAN_HEARING), num(keys::SUPERHUMAN_LEVEL));
    println!("ANC           {} (level {}: 0 ambient … 100 full ANC)", on(keys::ANC), num(keys::ANC_LEVEL));
    let muted = if s.flag(keys::MIC_MUTED) == Some(true) { "muted" } else { "live" };
    println!("Mic           {muted}, sensitivity {}, A.I. noise reduction {}", num(keys::MIC_SENSITIVITY), on(keys::AI_NOISE_REDUCTION));
    println!("Noise gate    {} (level {})", on(keys::NOISE_GATE), num(keys::NOISE_GATE_LEVEL));
    println!("Monitoring    level {} (enabled: {})", num(keys::MONITORING_LEVEL), on(keys::MONITORING_ENABLE));
    Ok(())
}

fn watch(client: &mut Headset, limit: Option<Duration>) -> Result<()> {
    let mut state = read_all(client)?;
    let start = Instant::now();
    eprintln!("watching; Ctrl-C to stop");
    while limit.is_none_or(|l| start.elapsed() < l) {
        for doc in client.wait_updates(Duration::from_millis(500))? {
            let before = state.clone();
            for key in state.apply(&doc) {
                let old = before.raw(key).map_or("-".to_string(), show_value);
                let new = state.raw(key).map_or("-".to_string(), show_value);
                println!("{:8.1}s  {:4} {}: {old} -> {new}", start.elapsed().as_secs_f64(), doc.record, describe_key(&format!("{key:x}")));
            }
        }
    }
    Ok(())
}

fn restore(client: &mut Headset, file: &PathBuf) -> Result<()> {
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
    for (key, value) in &todo {
        println!("{} -> {value}", describe_key(&format!("{key:x}")));
    }
    write(client, &todo)
}

fn describe_key(hex: &str) -> String {
    match u16::from_str_radix(hex, 16).ok().and_then(keys::label) {
        Some(label) => format!("{label} ({hex})"),
        None => hex.to_string(),
    }
}

fn show_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

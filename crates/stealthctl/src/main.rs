//! `stealthctl`: control a Turtle Beach Stealth Pro II from Linux.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use race_hid::{HidrawDevice, discover, known_product};
use serde_json::Value;
use tb_proto::{Client, State, TransmitterState, keys};

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
        value: i64,
    },
    /// Game equalizer
    Eq {
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
    /// Select a built-in preset
    Preset { preset: Preset },
    /// Set one of the 10 bands in dB (-9.0 to 9.0, steps of 0.5); switches to custom
    Band {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=10))]
        band: u8,
        #[arg(allow_hyphen_values = true)]
        db: f64,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Preset {
    Signature,
    Bass,
    BassTreble,
    Vocal,
}

#[derive(Subcommand)]
enum ShhCmd {
    On,
    Off,
    /// Level 1 to 3
    Level {
        #[arg(value_parser = clap::value_parser!(i64).range(1..=3))]
        level: i64,
    },
    /// Volume 0 to 100
    Volume { volume: i64 },
}

#[derive(Subcommand)]
enum AncCmd {
    On,
    Off,
    /// Level 0 to 100
    Level { level: i64 },
}

#[derive(Subcommand)]
enum MonitorCmd {
    On,
    Off,
    /// Volume 0 to 100
    Volume { volume: i64 },
}

#[derive(Subcommand)]
enum MicCmd {
    /// Mic volume 0 to 100
    Volume { volume: i64 },
    /// Noise gate level 0 to 100
    Gate { level: i64 },
    /// A.I. noise reduction raw value 0 or 1 (which one means "on" is not confirmed yet)
    Ai { value: i64 },
}

fn parse_key(s: &str) -> Result<u16, String> {
    let digits = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(digits, 16).map_err(|e| format!("not a hex key: {e}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Devices = cli.cmd {
        return devices();
    }
    let mut client = tb_proto::open(cli.device.as_deref()).context("opening the headset")?;

    match cli.cmd {
        Cmd::Devices => unreachable!(),
        Cmd::Status => status(&mut client, cli.device),
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
        Cmd::Set { key, value } => write(&mut client, &[(key, value)]),
        Cmd::Eq { cmd } => match cmd {
            EqCmd::Preset { preset } => {
                let n = match preset {
                    Preset::Signature => 1,
                    Preset::Bass => 2,
                    Preset::BassTreble => 3,
                    Preset::Vocal => 4,
                };
                write(&mut client, &[(keys::GAME_EQ_PRESET, n)])
            }
            EqCmd::Band { band, db } => {
                let value = ((db * 10.0) / 5.0).round() as i64 * 5;
                write(&mut client, &[(keys::GAME_EQ_BANDS + u16::from(band - 1) * 0x10, value)])
            }
        },
        Cmd::Shh { cmd } => match cmd {
            ShhCmd::On => write(&mut client, &[(keys::SUPERHUMAN_HEARING, 1)]),
            ShhCmd::Off => write(&mut client, &[(keys::SUPERHUMAN_HEARING, 0)]),
            ShhCmd::Level { level } => write(&mut client, &[(keys::SUPERHUMAN_LEVEL, level - 1)]),
            ShhCmd::Volume { volume } => write(&mut client, &[(keys::SUPERHUMAN_VOLUME, volume)]),
        },
        Cmd::Anc { cmd } => match cmd {
            AncCmd::On => write(&mut client, &[(keys::ANC, 1)]),
            AncCmd::Off => write(&mut client, &[(keys::ANC, 0)]),
            AncCmd::Level { level } => write(&mut client, &[(keys::ANC_LEVEL, level)]),
        },
        Cmd::Monitor { cmd } => match cmd {
            MonitorCmd::On => write(&mut client, &[(keys::MIC_MONITOR, 1)]),
            MonitorCmd::Off => write(&mut client, &[(keys::MIC_MONITOR, 0)]),
            MonitorCmd::Volume { volume } => write(&mut client, &[(keys::MIC_MONITOR_VOLUME, volume)]),
        },
        Cmd::Mic { cmd } => match cmd {
            MicCmd::Volume { volume } => write(&mut client, &[(keys::MIC_VOLUME, volume)]),
            MicCmd::Gate { level } => write(&mut client, &[(keys::NOISE_GATE, level)]),
            MicCmd::Ai { value } => write(&mut client, &[(keys::AI_NOISE_REDUCTION, value)]),
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
fn read_all(client: &mut Client<HidrawDevice>) -> Result<State> {
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

/// Write keys in order, then show what the headset pushed back.
fn write(client: &mut Client<HidrawDevice>, writes: &[(u16, i64)]) -> Result<()> {
    for &(key, value) in writes {
        client.set(key, value).with_context(|| format!("writing {key:#x} = {value}"))?;
    }
    for doc in client.wait_updates(Duration::from_millis(400))? {
        for (k, v) in &doc.kvp {
            println!("{} = {}", describe_key(k), show_value(v));
        }
    }
    Ok(())
}

fn status(client: &mut Client<HidrawDevice>, device: Option<PathBuf>) -> Result<()> {
    let s = read_all(client)?;
    let on = |key| match s.flag(key) {
        Some(true) => "on",
        Some(false) => "off",
        None => "?",
    };
    let num = |key| s.int(key).map_or("?".to_string(), |v| v.to_string());

    let dev = device.map_or_else(|| "auto".to_string(), |p| p.display().to_string());
    println!("Device        {dev}, firmware {}", s.text(keys::FIRMWARE).unwrap_or("?"));
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

    let eq = s.equalizer(keys::GAME_EQ_PRESET, keys::GAME_EQ_BANDS, keys::GAME_EQ_NAME);
    let bands: Vec<String> = eq.bands.iter().map(|b| b.map_or("?".into(), |v| format!("{:+.1}", v as f64 / 10.0))).collect();
    let name = match (eq.preset, eq.name.as_deref()) {
        (Some(0), _) | (_, Some("")) => "custom".to_string(),
        (_, Some(n)) => n.to_string(),
        _ => "?".to_string(),
    };
    println!("Game EQ       {name}  [{}] dB", bands.join(" "));
    println!(
        "Superhuman    {} (level {}, volume {})",
        on(keys::SUPERHUMAN_HEARING),
        s.int(keys::SUPERHUMAN_LEVEL).map_or("?".to_string(), |v| (v + 1).to_string()),
        num(keys::SUPERHUMAN_VOLUME)
    );
    println!("ANC           {} (level {})", on(keys::ANC), num(keys::ANC_LEVEL));
    println!("Monitoring    {} (volume {})", on(keys::MIC_MONITOR), num(keys::MIC_MONITOR_VOLUME));
    let muted = if s.flag(keys::MIC_MUTED) == Some(true) { "muted" } else { "live" };
    println!(
        "Mic           {muted}, volume {}, noise gate {}, A.I. noise reduction raw {}",
        num(keys::MIC_VOLUME),
        num(keys::NOISE_GATE),
        num(keys::AI_NOISE_REDUCTION)
    );
    Ok(())
}

fn watch(client: &mut Client<HidrawDevice>, limit: Option<Duration>) -> Result<()> {
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

fn restore(client: &mut Client<HidrawDevice>, file: &PathBuf) -> Result<()> {
    let snap: Value = serde_json::from_str(&fs::read_to_string(file)?)?;
    let saved = |key: u16| -> Option<i64> {
        snap.as_object()?.values().find_map(|kvp| kvp.get(format!("{key:x}"))?.as_str()?.parse().ok())
    };
    let current = read_all(client)?;

    // Selecting an EQ preset rewrites the bands and turns Superhuman Hearing off, so the EQ goes first.
    let mut writes = Vec::new();
    match saved(keys::GAME_EQ_PRESET) {
        Some(0) => {
            for i in 0..keys::BAND_COUNT as u16 {
                writes.push(keys::GAME_EQ_BANDS + i * 0x10);
            }
        }
        Some(_) => writes.push(keys::GAME_EQ_PRESET),
        None => {}
    }
    for s in keys::WRITABLE {
        let is_eq = (keys::GAME_EQ_PRESET..=keys::GAME_EQ_BANDS + 0x90).contains(&s.key);
        if !is_eq && s.key != keys::SUPERHUMAN_HEARING {
            writes.push(s.key);
        }
    }
    writes.push(keys::SUPERHUMAN_HEARING);

    let mut todo = Vec::new();
    for key in writes {
        if let Some(value) = saved(key)
            && current.int(key) != Some(value)
        {
            todo.push((key, value));
        }
    }
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


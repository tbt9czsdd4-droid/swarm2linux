//! `stealthd`: owns the Stealth Pro II and serves it on the session bus.
//!
//! One thread owns the device: it finds it, follows unplugging, keeps a session with every record subscribed and
//! carries out writes. D-Bus calls reach it through a queue; changes go out as the `Changed` signal.
//!
//! Bus name `io.github.swarm2linux.Stealthd`, object `/io/github/swarm2linux/Headset`, interface
//! `io.github.swarm2linux.Headset1`. Value names are those of `tb_proto::values`.
//!
//! Its own settings are kept in `~/.config/swarm2linux/stealthd.toml`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use race_hid::{HidrawDevice, discover, known_product};
use serde::{Deserialize, Serialize};
use stealth_dbus::{BUS_NAME, INTERFACE, PATH, from_value, to_owned_value, to_value};
use tb_proto::keys::{self, EqKeys};
use tb_proto::values::{self, Typed};
use tb_proto::{Client, State};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{fdo, interface};


/// How long the device thread waits for a request before polling the device again.
const IDLE_POLL: Duration = Duration::from_millis(200);
/// How often to look for a device while none is connected.
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
/// After the headset's link came back: how long to wait before subscribing again, and how often to try.
const RESUBSCRIBE_DELAY: Duration = Duration::from_millis(1500);
const RESUBSCRIBE_TRIES: u32 = 5;
/// How long to wait for the headset's pushes after a write, and the quiet time that ends them early.
const PUSH_WAIT: Duration = Duration::from_millis(800);
const PUSH_QUIET: Duration = Duration::from_millis(150);
/// Battery levels that trigger a notification while discharging.
const LOW_BATTERY: [i64; 2] = [15, 5];

/// What the D-Bus side asks the device thread to do.
enum Op {
    Set(String, Typed),
    SetBand { eq: &'static EqKeys, band: usize, tenths: i64 },
    SavePreset { eq: &'static EqKeys, name: String },
    DeletePreset { eq: &'static EqKeys, name: String },
}

struct Request {
    op: Op,
    reply: async_channel::Sender<Result<(), String>>,
}

/// What the D-Bus side reads.
#[derive(Default)]
struct Shared {
    device: Option<String>,
    values: BTreeMap<&'static str, Typed>,
    settings: Settings,
}

/// stealthd's own settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// Warn about a low battery with a desktop notification.
    battery_notifications: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { battery_notifications: true }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(config.join("swarm2linux").join("stealthd.toml"))
    }

    fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("stealthd: ignoring {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::other("no home directory"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let partial = path.with_extension("toml.new");
        fs::write(&partial, toml::to_string(self).map_err(io::Error::other)?)?;
        fs::rename(partial, path)
    }
}

struct Headset {
    shared: Arc<Mutex<Shared>>,
    requests: mpsc::Sender<Request>,
}

impl Headset {
    async fn submit(&self, op: Op) -> fdo::Result<()> {
        let (reply, answer) = async_channel::bounded(1);
        self.requests.send(Request { op, reply }).map_err(|_| fdo::Error::Failed("device thread stopped".into()))?;
        match answer.recv().await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(fdo::Error::Failed(e)),
            Err(_) => Err(fdo::Error::Failed("no answer from the device thread".into())),
        }
    }
}

#[interface(name = "io.github.swarm2linux.Headset1")]
impl Headset {
    /// Every known value by name, plus `connected` and `device`.
    async fn values(&self) -> HashMap<String, OwnedValue> {
        let shared = self.shared.lock().unwrap();
        let mut out: HashMap<String, OwnedValue> =
            shared.values.iter().map(|(k, v)| (k.to_string(), to_owned_value(v))).collect();
        out.insert("connected".into(), to_owned_value(&Typed::Bool(shared.device.is_some())));
        out.insert("device".into(), to_owned_value(&Typed::Text(shared.device.clone().unwrap_or_default())));
        out
    }

    /// Set one value by name, e.g. ("anc", true) or ("anc_level", 60).
    async fn set(&self, name: String, value: OwnedValue) -> fdo::Result<()> {
        let typed = match from_value(&value) {
            Some(t @ (Typed::Bool(_) | Typed::Int(_))) => t,
            _ => return Err(fdo::Error::InvalidArgs("expected a boolean or an integer".into())),
        };
        if values::to_write(&name, &typed).is_none() {
            return Err(fdo::Error::InvalidArgs(format!("{name} cannot be set this way")));
        }
        self.submit(Op::Set(name, typed)).await
    }

    /// Set one band (1–10) of the "game" or "mic" equalizer, in tenths of a dB (-90 to 90).
    async fn set_band(&self, eq: String, band: u32, tenths: i32) -> fdo::Result<()> {
        let eq = which_eq(&eq)?;
        if !(1..=10).contains(&band) {
            return Err(fdo::Error::InvalidArgs("band is 1 to 10".into()));
        }
        self.submit(Op::SetBand { eq, band: band as usize - 1, tenths: i64::from(tenths) }).await
    }

    /// Save the current bands of the "game" or "mic" equalizer as a custom preset.
    async fn save_preset(&self, eq: String, name: String) -> fdo::Result<()> {
        let eq = which_eq(&eq)?;
        self.submit(Op::SavePreset { eq, name }).await
    }

    /// Delete a custom preset of the "game" or "mic" equalizer by name.
    async fn delete_preset(&self, eq: String, name: String) -> fdo::Result<()> {
        let eq = which_eq(&eq)?;
        self.submit(Op::DeletePreset { eq, name }).await
    }

    /// Whether a headset is connected.
    #[zbus(property)]
    async fn connected(&self) -> bool {
        self.shared.lock().unwrap().device.is_some()
    }

    /// Whether to warn about a low battery with a desktop notification.
    #[zbus(property)]
    async fn battery_notifications(&self) -> bool {
        self.shared.lock().unwrap().settings.battery_notifications
    }

    #[zbus(property)]
    async fn set_battery_notifications(&mut self, on: bool) -> fdo::Result<()> {
        let settings = {
            let mut shared = self.shared.lock().unwrap();
            shared.settings.battery_notifications = on;
            shared.settings.clone()
        };
        settings.save().map_err(|e| fdo::Error::Failed(format!("saving the setting: {e}")))
    }
}

fn which_eq(name: &str) -> fdo::Result<&'static EqKeys> {
    match name {
        "game" => Ok(&keys::GAME_EQ),
        "mic" => Ok(&keys::MIC_EQ),
        _ => Err(fdo::Error::InvalidArgs("equalizer is \"game\" or \"mic\"".into())),
    }
}

fn main() -> Result<()> {
    let shared = Arc::new(Mutex::new(Shared { settings: Settings::load(), ..Shared::default() }));
    let (requests, queue) = mpsc::channel();
    let conn = zbus::blocking::connection::Builder::session()?
        .name(BUS_NAME)
        .context("is another stealthd already running?")?
        .serve_at(PATH, Headset { shared: shared.clone(), requests })?
        .build()?;
    eprintln!("stealthd: serving {INTERFACE} at {PATH} as {BUS_NAME}");

    let mut daemon = Daemon { conn, shared, queue, notified_low: None };
    loop {
        match pick_device() {
            Some((path, label)) => {
                eprintln!("stealthd: using {} ({label})", path.display());
                if let Err(e) = daemon.run(&path, &label) {
                    eprintln!("stealthd: {} lost: {e:#}", path.display());
                }
                daemon.publish(None, &State::new(), None);
                daemon.fail_pending("the headset disconnected");
            }
            None => {
                daemon.fail_pending("no headset connected");
                sleep(SCAN_INTERVAL);
            }
        }
    }
}

/// The best device to use: a transmitter before the headset's own cable, then by path.
fn pick_device() -> Option<(PathBuf, String)> {
    let mut found: Vec<_> = discover().ok()?.into_iter().filter(|d| d.vendor_collection).collect();
    found.sort_by_key(|d| (known_product(d.pid).is_none_or(|p| p.contains("headset")), d.path.clone()));
    let d = found.into_iter().next()?;
    let label = format!("10f5:{:04x} {}", d.pid, known_product(d.pid).unwrap_or(&d.name));
    Some((d.path, label))
}

struct Daemon {
    conn: Connection,
    shared: Arc<Mutex<Shared>>,
    queue: mpsc::Receiver<Request>,
    /// The lowest battery threshold already notified about while discharging.
    notified_low: Option<i64>,
}

impl Daemon {
    /// Serve one device until it goes away.
    fn run(&mut self, path: &Path, label: &str) -> Result<()> {
        let mut client = tb_proto::open(Some(path))?;
        let (mut state, problems) = client.subscribe_all()?;
        for p in problems {
            eprintln!("stealthd: {p}");
        }
        self.publish(Some(label.to_string()), &state, client.headset_link());
        // when to subscribe again after the headset came back, and how many tries are left
        let mut resubscribe: Option<(Instant, u32)> = None;

        loop {
            match self.queue.recv_timeout(IDLE_POLL) {
                Ok(req) => {
                    let result = execute(&mut client, &state, req.op).map_err(|e| format!("{e:#}"));
                    if result.is_ok() {
                        // answer only once the headset has pushed the new values, so a read right after a
                        // write sees them
                        for doc in await_pushes(&mut client)? {
                            state.apply(&doc);
                        }
                        self.publish(Some(label.to_string()), &state, client.headset_link());
                    }
                    let _ = req.reply.send_blocking(result);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            let link = client.headset_link();
            let docs = client.poll()?;
            if !docs.is_empty() || client.headset_link() != link {
                for doc in &docs {
                    state.apply(doc);
                }
                self.publish(Some(label.to_string()), &state, client.headset_link());
            }
            if client.take_link_up() {
                // the headset restarted and forgot every subscription; give it a moment to settle
                resubscribe = Some((Instant::now() + RESUBSCRIBE_DELAY, RESUBSCRIBE_TRIES));
            }
            if let Some((when, tries)) = resubscribe
                && Instant::now() >= when
            {
                match client.subscribe_all() {
                    Ok((fresh, problems)) if problems.is_empty() || tries <= 1 => {
                        eprintln!("stealthd: headset back, subscribed again");
                        state = fresh;
                        resubscribe = None;
                        self.publish(Some(label.to_string()), &state, client.headset_link());
                    }
                    Ok((_, problems)) => {
                        eprintln!("stealthd: headset not ready yet ({}), trying again", problems.join("; "));
                        resubscribe = Some((Instant::now() + RESUBSCRIBE_DELAY, tries - 1));
                    }
                    Err(e @ tb_proto::Error::Timeout(_)) if tries > 1 => {
                        eprintln!("stealthd: headset not ready yet ({e}), trying again");
                        resubscribe = Some((Instant::now() + RESUBSCRIBE_DELAY, tries - 1));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }

    /// Store the new values and signal the ones that changed.
    fn publish(&mut self, device: Option<String>, state: &State, link: Option<bool>) {
        let mut new = values::values(state);
        // the link indication when one arrived, else whether the transmitter reports an active link
        if let Some(up) = link.or_else(|| state.int(keys::ACTIVE_TRANSMITTER).map(|t| t > 0)) {
            new.insert("headset_link", Typed::Bool(up));
        }
        let mut changed: HashMap<String, Value<'static>> = HashMap::new();
        {
            let mut shared = self.shared.lock().unwrap();
            for (name, v) in &new {
                if shared.values.get(name) != Some(v) {
                    changed.insert(name.to_string(), to_value(v));
                }
            }
            if shared.device != device {
                changed.insert("connected".into(), Value::from(device.is_some()));
                changed.insert("device".into(), Value::from(device.clone().unwrap_or_default()));
            }
            shared.device = device;
            shared.values = new.clone();
        }
        if changed.is_empty() {
            return;
        }
        if let Err(e) = self.conn.emit_signal(None::<&str>, PATH, INTERFACE, "Changed", &(changed,)) {
            eprintln!("stealthd: emitting Changed: {e}");
        }
        self.check_battery(&new);
    }

    fn check_battery(&mut self, v: &BTreeMap<&'static str, Typed>) {
        let charging = v.get("charging") == Some(&Typed::Bool(true));
        let Some(&Typed::Int(level)) = v.get("battery") else { return };
        if charging || level < 0 {
            self.notified_low = None;
            return;
        }
        let Some(threshold) = LOW_BATTERY.iter().copied().filter(|&t| level <= t).min() else {
            self.notified_low = None;
            return;
        };
        if self.notified_low.is_some_and(|n| n <= threshold) {
            return;
        }
        self.notified_low = Some(threshold);
        if !self.shared.lock().unwrap().settings.battery_notifications {
            return;
        }
        notify(&self.conn, "Headset battery low", &format!("Stealth Pro II battery at {level}%"));
    }

    /// Answer queued requests that cannot be served now.
    fn fail_pending(&self, why: &str) {
        while let Ok(req) = self.queue.try_recv() {
            let _ = req.reply.send_blocking(Err(why.to_string()));
        }
    }
}

/// The documents the headset pushes after a write. They follow the ack within about 200 ms and can come in
/// several batches (a saved preset: the preset change first, the slot record a little later), so this returns
/// once some arrived and nothing more came for `PUSH_QUIET`, or after `PUSH_WAIT`.
fn await_pushes(client: &mut Client<HidrawDevice>) -> Result<Vec<tb_proto::Document>> {
    let start = Instant::now();
    let mut last = None;
    let mut docs = client.take_updates();
    if !docs.is_empty() {
        last = Some(start);
    }
    loop {
        let got = client.poll()?;
        let now = Instant::now();
        if !got.is_empty() {
            docs.extend(got);
            last = Some(now);
        } else if last.is_some_and(|t| now - t >= PUSH_QUIET) || now - start >= PUSH_WAIT {
            return Ok(docs);
        } else {
            sleep(Duration::from_millis(30));
        }
    }
}

fn execute(client: &mut Client<HidrawDevice>, state: &State, op: Op) -> Result<()> {
    match op {
        Op::Set(name, value) => {
            let (key, number) = values::to_write(&name, &value).context("not writable")?;
            client.set(key, number)?;
        }
        Op::SetBand { eq, band, tenths } => client.set(eq.band(band), tenths)?,
        Op::SavePreset { eq, name } => client.save_new_preset(state, eq, &name)?,
        Op::DeletePreset { eq, name } => client.delete_named_preset(state, eq, &name)?,
    }
    Ok(())
}

fn notify(conn: &Connection, summary: &str, body: &str) {
    let hints: HashMap<&str, Value> = HashMap::from([("urgency", Value::from(2u8))]);
    let result = conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("Stealth Pro II", 0u32, "audio-headset", summary, body, Vec::<&str>::new(), hints, -1i32),
    );
    if let Err(e) = result {
        eprintln!("stealthd: notification failed: {e}");
    }
}

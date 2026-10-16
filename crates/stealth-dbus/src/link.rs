//! A running link to stealthd for the apps. One thread passes on its `Changed` signals, one follows whether it
//! runs and fetches every value when it starts, and one carries out writes in order. They report through a
//! callback, from their own threads.

use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};
use std::thread;

use tb_proto::values::Typed;

use crate::DaemonClient;

/// A write for stealthd.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Set(String, Typed),
    SetBand(String, u32, i32),
    SavePreset(String, String),
    DeletePreset(String, String),
    BatteryNotifications(bool),
}

/// What the link reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Every value and stealthd's `BatteryNotifications` setting; no values while stealthd is not running (an older
    /// stealthd has no setting).
    Snapshot(Option<BTreeMap<String, Typed>>, Option<bool>),
    /// The values that changed.
    Changed(BTreeMap<String, Typed>),
    /// stealthd stopped.
    Stopped,
    /// A write failed, with stealthd's message. A snapshot follows, so a view that showed the new value goes back.
    WriteFailed(String),
    /// The link itself cannot follow stealthd (no session bus, for instance).
    Failed(String),
}

/// Sends writes to the link's writer thread.
#[derive(Clone)]
pub struct Link {
    commands: mpsc::Sender<Command>,
}

impl Link {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// Start the link's threads; `report` gets every event.
pub fn start(report: impl Fn(Event) + Send + Sync + 'static) -> zbus::Result<Link> {
    let client = DaemonClient::new()?;
    let report: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(report);

    let (c, r) = (client.clone(), report.clone());
    spawn("daemon-signals", move || {
        // listen first, then fetch, so no change falls in between
        let changes = match c.changes() {
            Ok(changes) => changes,
            Err(e) => return r(Event::Failed(format!("cannot follow stealthd's changes: {e}"))),
        };
        r(snapshot(&c));
        for values in changes {
            r(Event::Changed(values));
        }
    });

    let (c, r) = (client.clone(), report.clone());
    spawn("daemon-watch", move || {
        let owners = match c.owner_changes() {
            Ok(owners) => owners,
            Err(e) => return r(Event::Failed(format!("cannot follow whether stealthd runs: {e}"))),
        };
        for running in owners {
            r(if running { snapshot(&c) } else { Event::Stopped });
        }
    });

    let (commands, queue) = mpsc::channel();
    spawn("daemon-writes", move || {
        for command in queue {
            let result = match &command {
                Command::Set(name, value) => client.set(name, value),
                Command::SetBand(eq, band, tenths) => client.set_band(eq, *band, *tenths),
                Command::SavePreset(eq, name) => client.save_preset(eq, name),
                Command::DeletePreset(eq, name) => client.delete_preset(eq, name),
                Command::BatteryNotifications(on) => client.set_battery_notifications(*on),
            };
            if let Err(e) = result {
                report(Event::WriteFailed(describe(&e)));
                // the view already shows the new value; put back what the headset has
                report(snapshot(&client));
            }
        }
    });

    Ok(Link { commands })
}

/// Every value and stealthd's setting.
fn snapshot(client: &DaemonClient) -> Event {
    Event::Snapshot(client.values().ok(), client.battery_notifications().ok())
}

/// The message of a failed call: for errors stealthd returns, its own text.
pub fn describe(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(message), _) => message.clone(),
        e => e.to_string(),
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    thread::Builder::new().name(name.into()).spawn(f).expect("starting a thread");
}

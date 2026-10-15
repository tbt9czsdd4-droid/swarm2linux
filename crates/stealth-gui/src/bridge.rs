//! The link to stealthd. One thread applies its `Changed` signals, one follows whether it runs and fetches every
//! value when it starts, and one carries out writes in order. Everything reaches the window and the tray through
//! the event loop.

use std::sync::mpsc;
use std::thread;

use slint::{ComponentHandle, Weak};
use stealth_dbus::DaemonClient;
use tb_proto::values::Typed;

use crate::{AppWindow, Headset, Tray, model, tray};

/// A write for stealthd.
pub enum Command {
    Set(String, Typed),
    SetBand(String, u32, i32),
    SavePreset(String, String),
    DeletePreset(String, String),
    BatteryNotifications(bool),
}

/// Sends writes to the worker thread.
#[derive(Clone)]
pub struct Bridge {
    commands: mpsc::Sender<Command>,
}

impl Bridge {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// Where values are shown: the window and the tray each have their own `Headset` global.
#[derive(Clone)]
pub struct Views {
    pub window: Weak<AppWindow>,
    pub tray: Weak<Tray>,
}

impl Views {
    /// Run `f` on every view's `Headset` global, on the UI thread.
    fn update(&self, f: impl Fn(&Headset<'_>) + Send + 'static) {
        let views = self.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = views.window.upgrade() {
                f(&w.global::<Headset>());
            }
            if let Some(t) = views.tray.upgrade() {
                f(&t.global::<Headset>());
                tray::refresh(&t);
            }
        });
    }
}

pub fn start(ui: Views) -> zbus::Result<Bridge> {
    let client = DaemonClient::new()?;

    let (c, u) = (client.clone(), ui.clone());
    spawn("daemon-signals", move || {
        // listen first, then fetch, so no change falls in between
        let changes = match c.changes() {
            Ok(changes) => changes,
            Err(e) => return eprintln!("stealth-gui: cannot follow stealthd's changes: {e}"),
        };
        refresh(&c, &u);
        for values in changes {
            u.update(move |h| model::apply(h, &values));
        }
    });

    let (c, u) = (client.clone(), ui.clone());
    spawn("daemon-watch", move || {
        let owners = match c.owner_changes() {
            Ok(owners) => owners,
            Err(e) => return eprintln!("stealth-gui: cannot follow whether stealthd runs: {e}"),
        };
        for running in owners {
            if running {
                refresh(&c, &u);
            } else {
                u.update(|h| h.set_daemon_running(false));
            }
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
                let message = describe(&e);
                ui.update(move |h| h.set_error(message.as_str().into()));
                // the UI already shows the new value; put back what the headset has
                refresh(&client, &ui);
            }
        }
    });

    Ok(Bridge { commands })
}

/// Fetch every value and stealthd's setting, and show them.
fn refresh(client: &DaemonClient, ui: &Views) {
    let values = client.values().ok();
    // an older stealthd does not have the setting
    let notifications = client.battery_notifications().ok();
    ui.update(move |h| {
        h.set_daemon_running(values.is_some());
        if let Some(values) = &values {
            model::apply(h, values);
        }
        if let Some(on) = notifications {
            h.set_battery_notifications(on);
        }
    });
}

/// The message of a failed call: for errors stealthd returns, its own text.
fn describe(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(message), _) => message.clone(),
        e => e.to_string(),
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    thread::Builder::new().name(name.into()).spawn(f).expect("starting a thread");
}

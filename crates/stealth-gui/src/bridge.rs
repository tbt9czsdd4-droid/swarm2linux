//! The link to stealthd. One thread applies its `Changed` signals, one follows whether it runs and fetches every
//! value when it starts, and one carries out writes in order. Everything reaches the UI through the event loop.

use std::sync::mpsc;
use std::thread;

use slint::{ComponentHandle, Weak};
use stealth_dbus::DaemonClient;
use tb_proto::values::Typed;

use crate::{AppWindow, Headset, model};

/// A write for stealthd.
pub enum Command {
    Set(String, Typed),
    SetBand(String, u32, i32),
    SavePreset(String, String),
    DeletePreset(String, String),
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

pub fn start(ui: Weak<AppWindow>) -> zbus::Result<Bridge> {
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
            let _ = u.upgrade_in_event_loop(move |w| model::apply(&w.global::<Headset>(), &values));
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
                let _ = u.upgrade_in_event_loop(|w| w.global::<Headset>().set_daemon_running(false));
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
            };
            if let Err(e) = result {
                let message = describe(&e);
                let _ = ui.upgrade_in_event_loop(move |w| w.global::<Headset>().set_error(message.into()));
                // the UI already shows the new value; put back what the headset has
                refresh(&client, &ui);
            }
        }
    });

    Ok(Bridge { commands })
}

/// Fetch every value and show it.
fn refresh(client: &DaemonClient, ui: &Weak<AppWindow>) {
    let values = client.values();
    let _ = ui.upgrade_in_event_loop(move |w| {
        let h = w.global::<Headset>();
        h.set_daemon_running(values.is_ok());
        if let Ok(values) = values {
            model::apply(&h, &values);
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

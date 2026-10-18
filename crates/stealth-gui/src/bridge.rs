//! The link to stealthd (`stealth_dbus::link`), shown in the window and the tray through the event loop. It keeps
//! what stealthd said last, so a window that is made later starts with it.

use std::cell::RefCell;
use std::collections::BTreeMap;

use slint::{ComponentHandle, Weak};
use stealth_dbus::link::{self, Event, Link};
use tb_proto::values::Typed;

use crate::{AppWindow, Headset, Tray, model, tray};

/// Where values are shown: the tray and, once it is made, the window each have their own `Headset` global.
#[derive(Default)]
struct Views {
    tray: Option<Weak<Tray>>,
    window: Option<Weak<AppWindow>>,
    last: Last,
}

/// What stealthd said last, as the views show it.
#[derive(Default)]
struct Last {
    running: bool,
    values: BTreeMap<String, Typed>,
    notifications: Option<bool>,
}

thread_local! {
    // only used on the UI thread
    static VIEWS: RefCell<Views> = RefCell::default();
}

/// Connect to stealthd and show what it says in the tray, and in the window once `add_window` was called.
pub fn start(tray: &Tray) -> zbus::Result<Link> {
    VIEWS.with_borrow_mut(|v| v.tray = Some(tray.as_weak()));
    link::start(|event| {
        if let Event::Failed(message) = &event {
            eprintln!("stealth-gui: {message}");
            return;
        }
        let _ = slint::invoke_from_event_loop(move || VIEWS.with_borrow_mut(|v| v.show(&event)));
    })
}

/// Show the values in a newly made window too, starting with what stealthd said last.
pub fn add_window(window: &AppWindow) {
    VIEWS.with_borrow_mut(|v| {
        let h = window.global::<Headset>();
        h.set_daemon_running(v.last.running);
        model::apply(&h, &v.last.values);
        if let Some(on) = v.last.notifications {
            h.set_battery_notifications(on);
        }
        v.window = Some(window.as_weak());
    });
}

impl Views {
    fn show(&mut self, event: &Event) {
        self.last.remember(event);
        if let Some(w) = self.window.as_ref().and_then(Weak::upgrade) {
            apply(&w.global::<Headset>(), event);
        }
        if let Some(t) = self.tray.as_ref().and_then(Weak::upgrade) {
            apply(&t.global::<Headset>(), event);
            tray::refresh(&t);
        }
    }
}

impl Last {
    fn remember(&mut self, event: &Event) {
        match event {
            Event::Snapshot(values, notifications) => {
                self.running = values.is_some();
                if let Some(values) = values {
                    self.values.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
                if notifications.is_some() {
                    self.notifications = *notifications;
                }
            }
            Event::Changed(values) => self.values.extend(values.iter().map(|(k, v)| (k.clone(), v.clone()))),
            Event::Stopped => self.running = false,
            Event::WriteFailed(_) | Event::Failed(_) => {}
        }
    }
}

fn apply(h: &Headset<'_>, event: &Event) {
    match event {
        Event::Snapshot(values, notifications) => {
            h.set_daemon_running(values.is_some());
            if let Some(values) = values {
                model::apply(h, values);
            }
            if let Some(on) = notifications {
                h.set_battery_notifications(*on);
            }
        }
        Event::Changed(values) => model::apply(h, values),
        Event::Stopped => h.set_daemon_running(false),
        Event::WriteFailed(message) => h.set_error(message.as_str().into()),
        Event::Failed(_) => {}
    }
}

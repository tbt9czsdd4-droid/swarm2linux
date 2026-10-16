//! The link to stealthd (`stealth_dbus::link`), shown in the window and the tray through the event loop.

use slint::{ComponentHandle, Weak};
use stealth_dbus::link::{self, Event, Link};

use crate::{AppWindow, Headset, Tray, model, tray};

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

pub fn start(ui: Views) -> zbus::Result<Link> {
    link::start(move |event| match event {
        Event::Snapshot(values, notifications) => ui.update(move |h| {
            h.set_daemon_running(values.is_some());
            if let Some(values) = &values {
                model::apply(h, values);
            }
            if let Some(on) = notifications {
                h.set_battery_notifications(on);
            }
        }),
        Event::Changed(values) => ui.update(move |h| model::apply(h, &values)),
        Event::Stopped => ui.update(|h| h.set_daemon_running(false)),
        Event::WriteFailed(message) => ui.update(move |h| h.set_error(message.as_str().into())),
        Event::Failed(message) => eprintln!("stealth-gui: {message}"),
    })
}

//! `stealth-gui`: the desktop app for the Stealth Pro II. It is a client of stealthd and never touches the
//! device itself.
//!
//! `--hidden` starts it in the tray (when the tray is on). A second start shows the running app's window.

mod bridge;
mod instance;
mod model;
mod prefs;
mod tray;

slint::include_modules!();

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use slint::ComponentHandle;
use stealth_dbus::link::{Command, Link};
use stealth_dbus::view::EqEdit;
use tb_proto::values::Typed;

/// The app's id: the Wayland app id, the desktop file's name and the icon's name.
const APP_ID: &str = "io.github.swarm2linux";

fn main() -> Result<()> {
    let hidden = std::env::args().skip(1).any(|a| a == "--hidden");
    // development aids: STEALTH_GUI_PAGE=<n> opens page n (0-3); STEALTH_GUI_SNAPSHOT=<file.png> saves the window
    // as an image after STEALTH_GUI_SNAPSHOT_AFTER seconds (default 2) and quits
    let snapshot_to = std::env::var_os("STEALTH_GUI_SNAPSHOT").map(PathBuf::from);

    // The tray comes first, and the window is made only when it is first opened: on Wayland a made window exists
    // even while hidden, and Plasma can then show an empty entry for it in the task bar.
    let tray = Tray::new()?;
    // needs the platform, which the first component sets up, and must come before the window is made
    slint::set_xdg_app_id(APP_ID)?;
    let _instance = match snapshot_to {
        Some(_) => None,
        None => match instance::claim(tray.as_weak())? {
            Some(conn) => Some(conn),
            // the running app shows its window
            None => return Ok(()),
        },
    };

    model::init(&tray.global::<Headset>());
    let bridge = bridge::start(&tray)?;
    connect_writes(&tray.global::<Headset>(), &bridge);
    let window = Rc::new(Window { made: OnceCell::new(), bridge: bridge.clone(), tray: tray.as_weak() });
    connect_tray(&tray, &window, &bridge);

    if let Some(path) = snapshot_to {
        let after = std::env::var("STEALTH_GUI_SNAPSHOT_AFTER").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let ui = window.get()?;
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_secs_f64(after), move || {
            if let Some(ui) = weak.upgrade()
                && let Err(e) = snapshot(&ui, &path)
            {
                eprintln!("stealth-gui: snapshot: {e:#}");
            }
            let _ = slint::quit_event_loop();
        });
        ui.show()?;
    } else {
        let saved = prefs::load();
        if saved.tray {
            tray.show()?;
        }
        if !(hidden && saved.tray) {
            window.get()?.show()?;
        }
    }

    // runs until the window is closed while the tray is off, or Quit is chosen in the tray
    slint::run_event_loop()?;
    Ok(())
}

/// The main window, made the first time it is opened (see `main`).
struct Window {
    made: OnceCell<AppWindow>,
    bridge: Link,
    tray: slint::Weak<Tray>,
}

impl Window {
    /// The window, made and connected on first use.
    fn get(&self) -> Result<&AppWindow> {
        if self.made.get().is_none() {
            let _ = self.made.set(self.make()?);
        }
        Ok(self.made.get().expect("made above"))
    }

    fn make(&self) -> Result<AppWindow> {
        let ui = AppWindow::new()?;
        model::init(&ui.global::<Headset>());
        ui.global::<About>().set_version(env!("CARGO_PKG_VERSION").into());
        connect_writes(&ui.global::<Headset>(), &self.bridge);
        connect_equalizer(&ui, &self.bridge);
        connect_prefs(&ui, self.tray.clone());
        if let Some(page) = std::env::var("STEALTH_GUI_PAGE").ok().and_then(|s| s.parse().ok()) {
            ui.set_page(page);
        }
        bridge::add_window(&ui);
        Ok(ui)
    }

    fn show(&self) {
        if let Err(e) = self.get().and_then(|ui| Ok(ui.show()?)) {
            eprintln!("stealth-gui: opening the window: {e:#}");
        }
    }

    fn toggle(&self) {
        match self.made.get() {
            Some(ui) if ui.window().is_visible() => {
                let _ = ui.hide();
            }
            _ => self.show(),
        }
    }
}

/// The app's own preferences on the Device page: the tray, starting with the session and the EQ grid.
fn connect_prefs(ui: &AppWindow, tray: slint::Weak<Tray>) {
    let p = ui.global::<Prefs>();
    let saved = prefs::load();
    p.set_show_grid(saved.show_grid);
    p.set_tray(saved.tray);
    p.set_autostart(prefs::autostart());
    let weak = ui.as_weak();
    p.on_changed(move || {
        let (Some(ui), Some(tray)) = (weak.upgrade(), tray.upgrade()) else { return };
        let p = ui.global::<Prefs>();
        if let Err(e) = prefs::save(&prefs::Preferences { show_grid: p.get_show_grid(), tray: p.get_tray() }) {
            eprintln!("stealth-gui: saving preferences: {e}");
        }
        let shown = if p.get_tray() { tray.show() } else { tray.hide() };
        if let Err(e) = shown {
            eprintln!("stealth-gui: tray: {e}");
        }
        if p.get_autostart() != prefs::autostart()
            && let Err(e) = prefs::set_autostart(p.get_autostart())
        {
            ui.global::<Headset>().set_error(format!("Starting with the session: {e}").into());
            p.set_autostart(prefs::autostart());
        }
    });
}

/// Send the writes a view asks for to stealthd.
fn connect_writes(h: &Headset<'_>, bridge: &Link) {
    let b = bridge.clone();
    h.on_set_bool(move |name, v| b.send(Command::Set(name.into(), Typed::Bool(v))));
    let b = bridge.clone();
    h.on_set_int(move |name, v| b.send(Command::Set(name.into(), Typed::Int(v.into()))));
    let b = bridge.clone();
    h.on_set_band(move |eq, band, tenths| b.send(Command::SetBand(eq.into(), band as u32, tenths)));
    let b = bridge.clone();
    h.on_save_preset(move |eq, name| b.send(Command::SavePreset(eq.into(), name.into())));
    let b = bridge.clone();
    h.on_delete_preset(move |eq, name| b.send(Command::DeletePreset(eq.into(), name.into())));
    let b = bridge.clone();
    h.on_set_battery_notifications(move |on| b.send(Command::BatteryNotifications(on)));
}

/// The equalizer page's name check and Cancel.
fn connect_equalizer(ui: &AppWindow, bridge: &Link) {
    let h = ui.global::<Headset>();
    let weak = ui.as_weak();
    h.on_preset_name_problem(move |eq, name| {
        weak.upgrade().map(|ui| model::preset_name_problem(&ui.global::<Headset>(), &eq, &name)).unwrap_or("").into()
    });

    // the preset and bands of each equalizer from before the first change
    let before: Rc<RefCell<HashMap<String, EqEdit>>> = Rc::default();
    let (weak, saved) = (ui.as_weak(), before.clone());
    h.on_eq_edit_started(move |eq| {
        if let Some(ui) = weak.upgrade() {
            saved.borrow_mut().insert(eq.to_string(), model::equalizer(&ui.global::<Headset>(), &eq));
        }
    });
    let (weak, b) = (ui.as_weak(), bridge.clone());
    h.on_eq_edit_cancelled(move |eq| {
        let (Some(ui), Some(edit)) = (weak.upgrade(), before.borrow_mut().remove(eq.as_str())) else {
            return;
        };
        let h = ui.global::<Headset>();
        for command in edit.cancel_writes(&eq, &model::equalizer(&h, &eq).bands) {
            b.send(command);
        }
        model::show_equalizer(&h, &eq, edit.preset, &edit.bands);
    });
}

fn connect_tray(tray: &Tray, window: &Rc<Window>, bridge: &Link) {
    let w = window.clone();
    tray.on_open(move || w.show());
    let w = window.clone();
    tray.on_toggle_window(move || w.toggle());
    let b = bridge.clone();
    tray.on_preset_chosen(move |preset| b.send(Command::Set("game_eq_preset".into(), Typed::Int(preset.into()))));
    tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    tray::refresh(tray);
}

fn snapshot(ui: &AppWindow, path: &Path) -> Result<()> {
    let image = ui.window().take_snapshot()?;
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(image.as_bytes())?;
    Ok(())
}

//! `stealth-gui`: the desktop app for the Stealth Pro II. It is a client of stealthd and never touches the
//! device itself.

mod bridge;
mod model;
mod prefs;

slint::include_modules!();

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use bridge::Command;
use slint::ComponentHandle;
use tb_proto::values::Typed;

fn main() -> Result<()> {
    let ui = AppWindow::new()?;
    let headset = ui.global::<Headset>();
    model::init(&headset);

    let bridge = bridge::start(ui.as_weak())?;
    let b = bridge.clone();
    headset.on_set_bool(move |name, v| b.send(Command::Set(name.into(), Typed::Bool(v))));
    let b = bridge.clone();
    headset.on_set_int(move |name, v| b.send(Command::Set(name.into(), Typed::Int(v.into()))));
    let b = bridge.clone();
    headset.on_set_band(move |eq, band, tenths| b.send(Command::SetBand(eq.into(), band as u32, tenths)));
    let b = bridge.clone();
    headset.on_save_preset(move |eq, name| b.send(Command::SavePreset(eq.into(), name.into())));
    let b = bridge.clone();
    headset.on_delete_preset(move |eq, name| b.send(Command::DeletePreset(eq.into(), name.into())));
    let weak = ui.as_weak();
    headset.on_preset_name_problem(move |eq, name| {
        weak.upgrade().map(|ui| model::preset_name_problem(&ui.global::<Headset>(), &eq, &name)).unwrap_or("").into()
    });

    // the preset and bands of each equalizer from before the first change, for Cancel
    let before: Rc<RefCell<HashMap<String, model::Equalizer>>> = Rc::default();
    let (weak, saved) = (ui.as_weak(), before.clone());
    headset.on_eq_edit_started(move |eq| {
        if let Some(ui) = weak.upgrade() {
            saved.borrow_mut().insert(eq.to_string(), model::equalizer(&ui.global::<Headset>(), &eq));
        }
    });
    let (weak, b) = (ui.as_weak(), bridge);
    headset.on_eq_edit_cancelled(move |eq| {
        let (Some(ui), Some((preset, bands))) = (weak.upgrade(), before.borrow_mut().remove(eq.as_str())) else {
            return;
        };
        let h = ui.global::<Headset>();
        if preset != 0 {
            b.send(Command::Set(format!("{eq}_eq_preset"), Typed::Int(preset.into())));
        } else {
            let (_, now) = model::equalizer(&h, &eq);
            for (band, (&old, &new)) in bands.iter().zip(&now).enumerate() {
                if old != new {
                    b.send(Command::SetBand(eq.to_string(), band as u32 + 1, old));
                }
            }
        }
        model::show_equalizer(&h, &eq, preset, &bands);
    });

    let saved = prefs::load();
    let p = ui.global::<Prefs>();
    p.set_show_grid(saved.show_grid);
    p.set_tray(saved.tray);
    let weak = ui.as_weak();
    p.on_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        let p = ui.global::<Prefs>();
        if let Err(e) = prefs::save(&prefs::Preferences { show_grid: p.get_show_grid(), tray: p.get_tray() }) {
            eprintln!("stealth-gui: saving preferences: {e}");
        }
    });

    // development aids: STEALTH_GUI_PAGE=<n> opens page n (0-3); STEALTH_GUI_SNAPSHOT=<file.png> saves the window
    // as an image after STEALTH_GUI_SNAPSHOT_AFTER seconds (default 2) and quits
    if let Some(page) = std::env::var("STEALTH_GUI_PAGE").ok().and_then(|s| s.parse().ok()) {
        ui.set_page(page);
    }
    if let Some(path) = std::env::var_os("STEALTH_GUI_SNAPSHOT").map(PathBuf::from) {
        let after = std::env::var("STEALTH_GUI_SNAPSHOT_AFTER").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_secs_f64(after), move || {
            if let Some(ui) = weak.upgrade()
                && let Err(e) = snapshot(&ui, &path)
            {
                eprintln!("stealth-gui: snapshot: {e:#}");
            }
            let _ = slint::quit_event_loop();
        });
    }

    ui.run()?;
    Ok(())
}

fn snapshot(ui: &AppWindow, path: &Path) -> Result<()> {
    let image = ui.window().take_snapshot()?;
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(image.as_bytes())?;
    Ok(())
}

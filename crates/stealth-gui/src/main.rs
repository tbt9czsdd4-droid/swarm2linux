//! `stealth-gui`: the desktop app for the Stealth Pro II. It is a client of stealthd and never touches the
//! device itself.

mod bridge;
mod model;

slint::include_modules!();

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
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
    let b = bridge;
    headset.on_delete_preset(move |eq, name| b.send(Command::DeletePreset(eq.into(), name.into())));
    headset.on_valid_preset_name(|name| model::valid_preset_name(&name));

    // development aid: STEALTH_GUI_SNAPSHOT=<file.png> saves the window as an image after
    // STEALTH_GUI_SNAPSHOT_AFTER seconds (default 2) and quits
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

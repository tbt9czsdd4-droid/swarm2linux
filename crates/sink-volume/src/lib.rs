//! The headset's playback volume as PipeWire keeps it: the volume that KDE's sound settings change, and the
//! volume wheel too (it sends the system's volume keys). It is given in percent the way KDE shows it.
//!
//! The headset reports this volume itself (`volume`, key `0x2a0`), on its own scale, and Swarm II was never seen
//! writing that key. So the volume is read with `pw-dump` and set with `wpctl`, on the PipeWire sink of the
//! headset's USB sound card.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The ALSA card number of the sound card on the same USB device as a hidraw node such as `/dev/hidraw7`.
pub fn sound_card(hidraw: &Path) -> Option<u32> {
    let hid = fs::canonicalize(Path::new("/sys/class/hidraw").join(hidraw.file_name()?).join("device")).ok()?;
    // …/1-3.4/1-3.4:1.3/0003:10F5:2287.0008: the HID device, its USB interface, then the USB device
    let usb_device = hid.parent()?.parent()?;
    fs::read_dir("/sys/class/sound").ok()?.flatten().find_map(|entry| {
        let card = entry.file_name().to_str()?.strip_prefix("card")?.parse().ok()?;
        // a USB sound card's device is one of the USB device's interfaces
        let interface: PathBuf = fs::canonicalize(entry.path().join("device")).ok()?;
        (interface.parent()? == usb_device).then_some(card)
    })
}

/// The headset's sink in PipeWire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sink {
    /// The PipeWire object id, as `wpctl` takes it.
    pub id: u32,
    /// The volume in percent as KDE shows it: the cube root of the linear volume. Above 100 is amplification.
    pub percent: i64,
    pub muted: bool,
}

/// The sink of ALSA card `card`, if PipeWire has one.
pub fn find(card: u32) -> io::Result<Option<Sink>> {
    let out = run(Command::new("pw-dump"))?;
    let objects: Value = serde_json::from_slice(&out).map_err(io::Error::other)?;
    Ok(sink_of(&objects, card))
}

/// Set the volume of sink `id`, in percent (0 to 100).
pub fn set_percent(id: u32, percent: i64) -> io::Result<()> {
    let level = format!("{:.2}", percent.clamp(0, 100) as f64 / 100.0);
    let mut cmd = Command::new("wpctl");
    cmd.args(["set-volume", &id.to_string(), &level]);
    run(cmd).map(drop)
}

fn run(mut cmd: Command) -> io::Result<Vec<u8>> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => io::Error::new(e.kind(), format!("{program} is not installed (it comes with PipeWire)")),
        _ => io::Error::new(e.kind(), format!("running {program}: {e}")),
    })?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(io::Error::other(format!("{program} failed: {why}")));
    }
    Ok(out.stdout)
}

/// The lowest-numbered `Audio/Sink` node of card `card` in a `pw-dump` listing.
fn sink_of(objects: &Value, card: u32) -> Option<Sink> {
    objects
        .as_array()?
        .iter()
        .filter_map(|o| {
            let info = &o["info"];
            let props = &info["props"];
            let on_card = ["api.alsa.pcm.card", "alsa.card", "api.alsa.card"]
                .iter()
                .any(|k| number(&props[*k]) == Some(i64::from(card)));
            if o["type"] != "PipeWire:Interface:Node" || props["media.class"] != "Audio/Sink" || !on_card {
                return None;
            }
            let p = info["params"]["Props"].as_array()?.iter().find(|p| p["channelVolumes"].is_array())?;
            let linear = p["channelVolumes"].as_array()?.iter().filter_map(Value::as_f64).reduce(f64::max)?;
            Some(Sink {
                id: u32::try_from(o["id"].as_u64()?).ok()?,
                percent: (linear.max(0.0).cbrt() * 100.0).round() as i64,
                muted: p["mute"].as_bool().unwrap_or(false),
            })
        })
        .min_by_key(|s| s.id)
}

/// A number given as a JSON number or a string.
fn number(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, class: &str, card: Value, volumes: &[f64], mute: bool) -> Value {
        serde_json::json!({
            "id": id,
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": { "media.class": class, "api.alsa.pcm.card": card },
                "params": { "Props": [ { "volume": 1.0 }, { "channelVolumes": volumes, "mute": mute } ] }
            }
        })
    }

    #[test]
    fn finds_the_cards_sink_and_its_volume() {
        let dump = serde_json::json!([
            { "id": 51, "type": "PipeWire:Interface:Device", "info": { "props": { "api.alsa.card": "3" } } },
            node(67, "Audio/Source", Value::from(3), &[0.97], false),
            node(72, "Audio/Sink", Value::from(1), &[0.064], false),
            node(66, "Audio/Sink", Value::from(3), &[0.512006, 0.512006], false),
            node(90, "Audio/Sink", Value::from("3"), &[1.0], true),
        ]);
        assert_eq!(sink_of(&dump, 3), Some(Sink { id: 66, percent: 80, muted: false }));
        assert_eq!(sink_of(&dump, 1).map(|s| s.percent), Some(40));
        assert_eq!(sink_of(&dump, 2), None);
        // unequal channels count by the louder one, as KDE shows them
        let dump = serde_json::json!([node(5, "Audio/Sink", Value::from("2"), &[0.125, 0.216], true)]);
        assert_eq!(sink_of(&dump, 2), Some(Sink { id: 5, percent: 60, muted: true }));
    }
}

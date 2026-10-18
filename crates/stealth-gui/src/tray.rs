//! The tray icon: a headset whose ear cups fill up with the battery level, and its menu.

use slint::{Image, Model, ModelRc, VecModel};
use tb_proto::keys;

use crate::{Choice, Headset, Tray};

/// Bring the tray's icon and preset menu up to date with its `Headset` global.
pub fn refresh(tray: &Tray) {
    let h = tray.global::<Headset>();
    let linked = h.get_daemon_running() && h.get_connected() && h.get_headset_link();
    let known = linked && h.get_battery() >= 0;
    let level = known.then(|| h.get_battery());
    let charging = known && h.get_charging();
    let muted = linked && h.get_mic_muted();
    let icon_key = format!("{level:?} {charging} {muted}");
    if tray.get_icon_key() != icon_key.as_str() {
        tray.set_battery_icon(icon(level, charging, muted));
        tray.set_icon_key(icon_key.into());
    }

    let slots: Vec<String> = h.get_game_eq_slots().iter().map(|s| s.to_string()).collect();
    let menu_key = format!("{slots:?}");
    if tray.get_menu_key() != menu_key.as_str() {
        show_presets(tray, &slots);
        tray.set_menu_key(menu_key.into());
    }

    // Slint hands changed tray properties (icon, tooltip) to the tray only at the start of an event loop iteration.
    // Without a window, nothing starts the next one until the next update from stealthd, so the tray would show the
    // state before this one: wake the loop.
    slint::Timer::single_shot(std::time::Duration::ZERO, || {});
}

/// The preset menu: the onboard presets and the saved custom ones.
fn show_presets(tray: &Tray, slots: &[String]) {
    let onboard = keys::GAME_EQ.presets.iter().map(|&(value, label)| (value as i32, label.to_string()));
    let custom = slots
        .iter()
        .enumerate()
        .filter(|(_, name)| !name.is_empty())
        .map(|(i, name)| (keys::FIRST_CUSTOM_PRESET as i32 + i as i32, name.clone()));
    let choices: Vec<Choice> = onboard.chain(custom).map(|(value, label)| Choice { value, label: label.into() }).collect();
    tray.set_presets(ModelRc::new(VecModel::from(choices)));
}

/// The icon: grey and empty while the level is unknown, green cups (red at 15% and below) otherwise, with a bolt
/// while charging, and a red badge with a crossed-out microphone under the headband while the mic is muted.
fn icon(level: Option<i32>, charging: bool, muted: bool) -> Image {
    let stroke = if level.is_some() { "#d5d9de" } else { "#7d838c" };
    let fill = match level {
        Some(l) if l <= 15 && !charging => "#e5484d",
        _ => "#35c47f",
    };
    let height = level.map_or(0.0, |l| 11.5 * l.clamp(0, 100) as f32 / 100.0);
    let y = 29.0 - height;
    let bolt = if charging { r##"<path d="M17.5 17 l-4.5 7 h3 l-1.5 6 l5 -7.5 h-3 z" fill="#f5c542"/>"## } else { "" };
    let mute = if muted {
        r##"<circle cx="16" cy="10.2" r="5.6" fill="#e5484d"/>
<rect x="14.8" y="6.6" width="2.4" height="4.4" rx="1.2" fill="#fff"/>
<path d="M13.5 9.6 a2.5 2.5 0 0 0 5 0 M16 12.1 v1.2" fill="none" stroke="#fff" stroke-width="1" stroke-linecap="round"/>
<path d="M12.9 7.1 l6.2 6.2" stroke="#fff" stroke-width="1.1" stroke-linecap="round"/>
"##
    } else {
        ""
    };
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 32 32">
<path d="M5 17v-3a11 11 0 0 1 22 0v3" fill="none" stroke="{stroke}" stroke-width="2.2" stroke-linecap="round"/>
<rect x="4.6" y="{y}" width="5.8" height="{height}" fill="{fill}"/>
<rect x="21.6" y="{y}" width="5.8" height="{height}" fill="{fill}"/>
<rect x="3.5" y="16.5" width="8" height="13.5" rx="2" fill="none" stroke="{stroke}" stroke-width="2"/>
<rect x="20.5" y="16.5" width="8" height="13.5" rx="2" fill="none" stroke="{stroke}" stroke-width="2"/>
{bolt}{mute}</svg>"##
    );
    Image::load_from_svg_data(svg.as_bytes()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws() {
        for level in [None, Some(0), Some(10), Some(72), Some(100)] {
            for charging in [false, true] {
                for muted in [false, true] {
                    let size = icon(level, charging, muted).size();
                    assert_eq!((size.width, size.height), (64, 64), "{level:?} {charging} {muted}");
                }
            }
        }
    }
}

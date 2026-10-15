//! The tray icon: a headset whose ear cups fill up with the battery level, and its menu.

use slint::{Image, Model, ModelRc, VecModel};
use tb_proto::keys;

use crate::{Choice, Headset, Tray};

/// Bring the tray's icon and preset menu up to date with its `Headset` global.
pub fn refresh(tray: &Tray) {
    let h = tray.global::<Headset>();
    let known = h.get_daemon_running() && h.get_connected() && h.get_headset_link() && h.get_battery() >= 0;
    let level = known.then(|| h.get_battery());
    let charging = known && h.get_charging();
    let icon_key = format!("{level:?} {charging}");
    if tray.get_icon_key() != icon_key.as_str() {
        tray.set_battery_icon(icon(level, charging));
        tray.set_icon_key(icon_key.into());
    }

    let slots: Vec<String> = h.get_game_eq_slots().iter().map(|s| s.to_string()).collect();
    let menu_key = format!("{slots:?}");
    if tray.get_menu_key() != menu_key.as_str() {
        show_presets(tray, &slots);
        tray.set_menu_key(menu_key.into());
    }
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
/// while charging.
fn icon(level: Option<i32>, charging: bool) -> Image {
    let stroke = if level.is_some() { "#d5d9de" } else { "#7d838c" };
    let fill = match level {
        Some(l) if l <= 15 && !charging => "#e5484d",
        _ => "#35c47f",
    };
    let height = level.map_or(0.0, |l| 11.5 * l.clamp(0, 100) as f32 / 100.0);
    let y = 29.0 - height;
    let bolt = if charging { r##"<path d="M17.5 17 l-4.5 7 h3 l-1.5 6 l5 -7.5 h-3 z" fill="#f5c542"/>"## } else { "" };
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 32 32">
<path d="M5 17v-3a11 11 0 0 1 22 0v3" fill="none" stroke="{stroke}" stroke-width="2.2" stroke-linecap="round"/>
<rect x="4.6" y="{y}" width="5.8" height="{height}" fill="{fill}"/>
<rect x="21.6" y="{y}" width="5.8" height="{height}" fill="{fill}"/>
<rect x="3.5" y="16.5" width="8" height="13.5" rx="2" fill="none" stroke="{stroke}" stroke-width="2"/>
<rect x="20.5" y="16.5" width="8" height="13.5" rx="2" fill="none" stroke="{stroke}" stroke-width="2"/>
{bolt}</svg>"##
    );
    Image::load_from_svg_data(svg.as_bytes()).unwrap_or_default()
}

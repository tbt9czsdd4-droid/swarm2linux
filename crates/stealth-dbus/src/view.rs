//! What the apps show and check, without any UI code: shared by the desktop app and the terminal app.

use tb_proto::keys;

use crate::link::Command;

/// The first letter in upper case: option labels in `tb_proto::keys` are written in lower case after the first word.
pub fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Whether the headset takes `name` as a custom preset name.
pub fn valid_preset_name(name: &str) -> bool {
    keys::writable(keys::GAME_EQ_NAME).is_some_and(|s| s.values.accepts_text(name))
}

/// Why `name` cannot be used for a new preset next to the saved ones (`taken`, empty for free slots), or "" if it
/// can (or is still empty).
pub fn preset_name_problem<S: AsRef<str>>(taken: &[S], name: &str) -> &'static str {
    if name.is_empty() {
        ""
    } else if !valid_preset_name(name) {
        "Up to 16 characters, without quotes or backslashes"
    } else if taken.iter().any(|s| s.as_ref().eq_ignore_ascii_case(name)) {
        "There is already a preset with this name"
    } else {
        ""
    }
}

/// A line per transmitter from `transmitters` entries like "1 2287 active", and the charging hub's state: that of
/// slot 1.
pub fn transmitter_lines<S: AsRef<str>>(entries: &[S]) -> (Vec<String>, String) {
    let mut lines = Vec::new();
    let mut hub = "Not connected".to_string();
    for entry in entries {
        let mut parts = entry.as_ref().split_whitespace();
        let (Some(slot), Some(product), Some(state)) = (parts.next(), parts.next(), parts.next()) else { continue };
        // the headset sometimes reports product 0
        lines.push(match product {
            "0" => format!("Transmitter {slot} · {state}"),
            _ => format!("Transmitter {slot} · 10f5:{product} · {state}"),
        });
        if slot == "1" {
            hub = capitalized(state);
        }
    }
    (lines, hub)
}

/// An equalizer's preset number and bands (tenths of a dB) from before an edit, to go back to with Cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqEdit {
    pub preset: i32,
    pub bands: Vec<i32>,
}

impl EqEdit {
    /// The writes that bring the "game" or "mic" equalizer back from the bands it has `now`: the preset number,
    /// or, for bands that were unsaved custom ones already, the bands that changed.
    pub fn cancel_writes(&self, eq: &str, now: &[i32]) -> Vec<Command> {
        if self.preset != 0 {
            return vec![Command::Set(format!("{eq}_eq_preset"), tb_proto::values::Typed::Int(self.preset.into()))];
        }
        self.bands
            .iter()
            .zip(now)
            .enumerate()
            .filter(|(_, (old, new))| old != new)
            .map(|(band, (&old, _))| Command::SetBand(eq.to_string(), band as u32 + 1, old))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transmitters() {
        let (lines, hub) = transmitter_lines(&["2 229b standby", "1 2287 active", "3 0 standby", "bad"]);
        assert_eq!(
            lines,
            ["Transmitter 2 · 10f5:229b · standby", "Transmitter 1 · 10f5:2287 · active", "Transmitter 3 · standby"]
        );
        assert_eq!(hub, "Active");
        assert_eq!(transmitter_lines::<&str>(&[]).1, "Not connected");
    }

    #[test]
    fn labels_and_names() {
        assert_eq!(capitalized("never"), "Never");
        assert_eq!(capitalized(""), "");
        assert!(valid_preset_name("My Preset"));
        assert!(!valid_preset_name(""));
        assert!(!valid_preset_name("a name that is far too long"));
        assert!(!valid_preset_name("quote\""));
        let taken = ["Bassy", "", "", "", ""];
        assert_eq!(preset_name_problem(&taken, ""), "");
        assert_eq!(preset_name_problem(&taken, "Treble"), "");
        assert_eq!(preset_name_problem(&taken, "bassy"), "There is already a preset with this name");
        assert!(preset_name_problem(&taken, "back\\slash").starts_with("Up to 16"));
    }

    #[test]
    fn cancel_goes_back_to_the_preset_or_the_old_bands() {
        let edit = EqEdit { preset: 2, bands: vec![0; 10] };
        assert_eq!(edit.cancel_writes("game", &[5; 10]), [Command::Set("game_eq_preset".into(), tb_proto::values::Typed::Int(2))]);
        let mut old = vec![0; 10];
        old[3] = -15;
        let edit = EqEdit { preset: 0, bands: old };
        let mut now = vec![0; 10];
        now[3] = 10;
        now[7] = 5;
        assert_eq!(
            edit.cancel_writes("mic", &now),
            [Command::SetBand("mic".into(), 4, -15), Command::SetBand("mic".into(), 8, 0)]
        );
    }
}

//! The app's own preferences, in `~/.config/swarm2linux/gui.toml`. Headset settings live on the headset.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// The equalizer's fine grid.
    pub show_grid: bool,
    /// The tray icon, and closing the window to it.
    pub tray: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { show_grid: false, tray: true }
    }
}

fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("swarm2linux").join("gui.toml"))
}

/// The saved preferences, or the defaults.
pub fn load() -> Preferences {
    let Some(path) = path() else { return Preferences::default() };
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("stealth-gui: ignoring {}: {e}", path.display());
            Preferences::default()
        }),
        Err(_) => Preferences::default(),
    }
}

pub fn save(prefs: &Preferences) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("no home directory"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(prefs).map_err(io::Error::other)?;
    let partial = path.with_extension("toml.new");
    fs::write(&partial, text)?;
    fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_gaps() {
        let p = Preferences { show_grid: true, tray: false };
        assert_eq!(toml::from_str::<Preferences>(&toml::to_string(&p).unwrap()).unwrap(), p);
        assert_eq!(toml::from_str::<Preferences>("show_grid = true").unwrap(), Preferences { show_grid: true, tray: true });
    }
}

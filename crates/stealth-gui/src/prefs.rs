//! The app's own preferences, in `~/.config/swarm2linux/gui.toml`, and starting with the session through
//! `~/.config/autostart`. Headset settings live on the headset.

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

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("swarm2linux").join("gui.toml"))
}

fn autostart_entry() -> Option<PathBuf> {
    Some(config_dir()?.join("autostart").join("io.github.swarm2linux.desktop"))
}

/// Whether the app starts with the session.
pub fn autostart() -> bool {
    autostart_entry().is_some_and(|p| p.exists())
}

/// Start the app (this binary, with `--hidden`) with the session, or stop doing so.
pub fn set_autostart(on: bool) -> io::Result<()> {
    let entry = autostart_entry().ok_or_else(|| io::Error::other("no home directory"))?;
    if !on {
        return match fs::remove_file(&entry) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    let exe = exe.to_str().ok_or_else(|| io::Error::other("the program's path is not UTF-8"))?;
    // these would need escaping in Exec=
    if exe.contains(['"', '`', '$', '\\', '\n']) {
        return Err(io::Error::other(format!("cannot start {exe} from an autostart entry")));
    }
    if let Some(dir) = entry.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        entry,
        format!(
            "[Desktop Entry]\nType=Application\nName=swarm2linux\nComment=Stealth Pro II settings in the tray\n\
             Exec=\"{exe}\" --hidden\nIcon=io.github.swarm2linux\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ),
    )
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

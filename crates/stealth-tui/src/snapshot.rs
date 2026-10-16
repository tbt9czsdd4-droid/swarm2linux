//! A development aid like the desktop app's: `STEALTH_TUI_SNAPSHOT=file.svg` draws one frame with stealthd's
//! values and saves it as an SVG, then quits. `STEALTH_TUI_PAGE=0`–`3` picks the page, `STEALTH_TUI_SIZE=120x36`
//! the size, and `STEALTH_TUI_KEYS` keys to press first (`\t` is Tab). Keys only move around: nothing is written
//! to the headset.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use stealth_dbus::DaemonClient;
use stealth_dbus::link::Event;

use crate::app::{App, Page};
use crate::{theme, ui};

const CELL_W: f64 = 8.4;
const CELL_H: f64 = 17.0;
const BACKGROUND: &str = "#111419";
const FOREGROUND: &str = "#e9ebee";

pub fn run(path: PathBuf) -> Result<()> {
    theme::force_truecolor();
    let now = Instant::now();
    let mut app = App::new();
    match DaemonClient::connect()? {
        Some(client) => app.daemon_event(Event::Snapshot(client.values().ok(), client.battery_notifications().ok()), now),
        None => app.daemon_event(Event::Stopped, now),
    }
    if let Some(page) = std::env::var("STEALTH_TUI_PAGE").ok().and_then(|p| p.parse::<usize>().ok()) {
        app.page = Page::ALL[page.min(3)];
    }
    for c in std::env::var("STEALTH_TUI_KEYS").unwrap_or_default().chars() {
        let code = if c == '\t' { KeyCode::Tab } else { KeyCode::Char(c) };
        app.key(KeyEvent::new(code, KeyModifiers::NONE), now);
    }
    let (width, height) = std::env::var("STEALTH_TUI_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((120, 36));
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    fs::write(&path, svg(terminal.backend().buffer())).with_context(|| format!("writing {}", path.display()))
}

fn colour(c: Color, default: &str) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => default.to_string(),
        Color::Gray => "#a0a4ab".into(),
        Color::DarkGray => "#5b626c".into(),
        _ => default.to_string(),
    }
}

/// The buffer as an SVG: one rectangle per coloured background, one text per character.
pub fn svg(buffer: &Buffer) -> String {
    let area = buffer.area;
    let (w, h) = (f64::from(area.width) * CELL_W, f64::from(area.height) * CELL_H);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="DejaVu Sans Mono, Noto Sans Mono, monospace" font-size="14">
<rect width="{w}" height="{h}" fill="{BACKGROUND}"/>
"#
    );
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            let (px, py) = (f64::from(x) * CELL_W, f64::from(y) * CELL_H);
            if cell.bg != Color::Reset {
                let _ = writeln!(out, r#"<rect x="{px}" y="{py}" width="{CELL_W}" height="{CELL_H}" fill="{}"/>"#, colour(cell.bg, BACKGROUND));
            }
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }
            let text = symbol.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let weight = if cell.modifier.contains(Modifier::BOLD) { r#" font-weight="bold""# } else { "" };
            let underline = if cell.modifier.contains(Modifier::UNDERLINED) { r#" text-decoration="underline""# } else { "" };
            let _ = writeln!(
                out,
                r#"<text x="{px}" y="{}" fill="{}"{weight}{underline}>{text}</text>"#,
                py + CELL_H * 0.78,
                colour(cell.fg, FOREGROUND)
            );
        }
    }
    out.push_str("</svg>\n");
    out
}

//! Drawing pieces in the desktop app's style: sliders with a purple→pink fill, toggles, option pickers, an Off/On
//! switch, list rows with a teal mark for the active entry, cards and the battery.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};

use crate::theme::{self, rgb};

/// A card: a rounded box with an upper-case title, purple while it holds the focus.
pub fn card(title: &str, focused: bool) -> Block<'static> {
    let border = if focused { theme::fg(theme::ACCENT) } else { theme::fg(theme::LINE) };
    let title_style = if focused { theme::bold().fg(rgb(theme::ACCENT)) } else { theme::bold() };
    Block::bordered().border_type(BorderType::Rounded).border_style(border).title(Span::styled(format!(" {} ", title.to_uppercase()), title_style))
}

/// How a slider looks.
#[derive(Debug, Clone, Copy)]
pub struct SliderLook<'a> {
    pub ends: (&'a str, &'a str),
    pub centred: bool,
    /// false for a read-only meter
    pub knob: bool,
    pub enabled: bool,
}

/// A slider of `width` cells; also the track's first column (relative) and width, for the mouse.
pub fn slider(value: i64, min: i64, max: i64, look: SliderLook, width: u16) -> (Line<'static>, u16, u16) {
    let (left, right) = look.ends;
    // an end label takes its width and a space
    let end_width = |s: &str| if s.is_empty() { 0 } else { s.chars().count() as u16 + 1 };
    let (lw, rw) = (end_width(left), end_width(right));
    let track = width.saturating_sub(lw + rw).max(3);
    let span = (max - min).max(1) as f64;
    let ratio = ((value - min) as f64 / span).clamp(0.0, 1.0);
    let knob = (ratio * f64::from(track - 1)).round() as u16;
    let mid = (track - 1) / 2;
    let label = if look.enabled { theme::muted() } else { theme::dim() };
    let mut spans = Vec::new();
    if !left.is_empty() {
        spans.push(Span::styled(format!("{left} "), label));
    }
    for i in 0..track {
        let filled = if look.centred { (i >= knob.min(mid) && i <= knob.max(mid)) && i != knob } else { i < knob };
        let span = if i == knob && look.knob {
            Span::styled("●", if look.enabled { theme::fg(theme::KNOB) } else { theme::dim() })
        } else if filled || (i == knob && !look.knob) {
            let colour = if look.enabled { theme::fill_at(f64::from(i) / f64::from(track - 1)) } else { rgb(theme::DIM) };
            Span::styled("━", Style::new().fg(colour))
        } else if look.centred && i == mid {
            Span::styled("┼", theme::fg(theme::DIM))
        } else {
            Span::styled("─", theme::fg(theme::TRACK))
        };
        spans.push(span);
    }
    if !right.is_empty() {
        spans.push(Span::styled(format!(" {right}"), label));
    }
    (Line::from(spans), lw, track)
}

/// The slider value under column `x` of a track (relative to its first column).
pub fn slider_value_at(x: u16, track: u16, min: i64, max: i64) -> i64 {
    let ratio = f64::from(x.min(track - 1)) / f64::from((track - 1).max(1));
    min + (ratio * (max - min) as f64).round() as i64
}

/// A toggle: `━━● On` or `●── Off`.
pub fn toggle(on: bool, enabled: bool) -> Vec<Span<'static>> {
    let (fill, knob, text) = if !enabled {
        (theme::dim(), theme::dim(), theme::dim())
    } else if on {
        (theme::fg(theme::ACCENT), theme::fg(theme::KNOB), Style::new())
    } else {
        (theme::fg(theme::TRACK), theme::muted(), theme::muted())
    };
    if on {
        vec![Span::styled("━━", fill), Span::styled("●", knob), Span::styled(" On ", text)]
    } else {
        vec![Span::styled("●", knob), Span::styled("──", fill), Span::styled(" Off", text)]
    }
}

/// An option picker: `‹ Legacy ›`.
pub fn choice(label: &str, focused: bool, enabled: bool) -> Vec<Span<'static>> {
    let arrows = if !enabled {
        theme::dim()
    } else if focused {
        theme::fg(theme::ACCENT)
    } else {
        theme::muted()
    };
    let text = if enabled { Style::new() } else { theme::dim() };
    vec![Span::styled("‹ ", arrows), Span::styled(label.to_string(), text), Span::styled(" ›", arrows)]
}

/// An Off | On switch with the active half lit.
pub fn segmented(on: bool, enabled: bool) -> Vec<Span<'static>> {
    let lit = if enabled { Style::new().bg(rgb(theme::FIELD)).add_modifier(Modifier::BOLD) } else { theme::dim().bg(rgb(theme::FIELD)) };
    let unlit = if enabled { theme::muted() } else { theme::dim() };
    let (off, on_) = if on { (unlit, lit) } else { (lit, unlit) };
    vec![Span::styled("  Off  ", off), Span::styled("│", theme::fg(theme::LINE)), Span::styled("  On  ", on_)]
}

/// A list entry: a teal dot and label for the active one, a purple bar for the cursor.
pub fn list_item(label: &str, active: bool, cursor: bool, enabled: bool) -> Line<'static> {
    let bar = if cursor { Span::styled("▌", theme::fg(theme::ACCENT)) } else { Span::raw(" ") };
    let (dot, style) = if !enabled {
        (Span::styled(if active { " ● " } else { " ○ " }, theme::dim()), theme::dim())
    } else if active {
        (Span::styled(" ● ", theme::fg(theme::SELECTED)), theme::fg(theme::SELECTED))
    } else {
        (Span::styled(" ○ ", theme::dim()), Style::new())
    };
    let style = if cursor { style.add_modifier(Modifier::BOLD) } else { style };
    Line::from(vec![bar, dot, Span::styled(label.to_uppercase(), style)])
}

/// The battery: `▰▰▰▰▱ 83%`, red at 15% and below unless charging, with a bolt while charging.
pub fn battery(level: i64, charging: bool, width: usize) -> Vec<Span<'static>> {
    let colour = if level <= 15 && !charging { theme::BAD } else { theme::GOOD };
    let filled = (level.clamp(0, 100) as usize * width + 50) / 100;
    let mut spans = vec![
        Span::styled("▰".repeat(filled), theme::fg(colour)),
        Span::styled("▱".repeat(width - filled), theme::fg(theme::TRACK)),
        Span::raw(format!(" {level}%")),
    ];
    if charging {
        spans.push(Span::styled(" ↯", theme::fg(theme::WARN)));
    }
    spans
}

/// Words of `text` wrapped to `width` columns.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn slider_positions() {
        let look = SliderLook { ends: ("0", "100"), centred: false, knob: true, enabled: true };
        let (line, start, track) = slider(50, 0, 100, look, 26);
        assert_eq!((start, track), (2, 20));
        assert_eq!(text(&line), format!("0 {}●{} 100", "━".repeat(10), "─".repeat(9)));
        assert_eq!(slider_value_at(0, track, 0, 100), 0);
        assert_eq!(slider_value_at(19, track, 0, 100), 100);
        assert_eq!(slider_value_at(40, track, 0, 100), 100);
        let centred = SliderLook { ends: ("AMB", "ANC"), centred: true, knob: true, enabled: true };
        let (line, _, _) = slider(100, 0, 100, centred, 29);
        let bare = SliderLook { ends: ("", ""), centred: false, knob: false, enabled: true };
        let (line2, start, track) = slider(0, 0, 100, bare, 10);
        assert_eq!((text(&line2), start, track), ("━─────────".to_string(), 0, 10));
        assert!(text(&line).starts_with("AMB ─"), "{}", text(&line));
        assert!(text(&line).ends_with("━● ANC"));
    }

    #[test]
    fn wraps_words() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert!(wrap("", 5).is_empty());
    }

    #[test]
    fn battery_bar() {
        let t = |spans: Vec<Span>| spans.iter().map(|s| s.content.to_string()).collect::<String>();
        assert_eq!(t(battery(83, false, 5)), "▰▰▰▰▱ 83%");
        assert_eq!(t(battery(0, true, 5)), "▱▱▱▱▱ 0% ↯");
    }
}

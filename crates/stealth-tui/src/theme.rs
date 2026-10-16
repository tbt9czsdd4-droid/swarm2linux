//! The desktop app's palette. Plain text keeps the terminal's own colours, so light and dark terminals both work.
//! Truecolor is used when `COLORTERM` says the terminal has it, otherwise the nearest of the 256 colours.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: u32 = 0x7b45f5;
pub const SELECTED: u32 = 0x3dd6b8;
pub const MUTED: u32 = 0x8d949e;
pub const DIM: u32 = 0x5b626c;
pub const LINE: u32 = 0x3a3f48;
pub const TRACK: u32 = 0x3a3f48;
pub const FIELD: u32 = 0x252b34;
pub const GOOD: u32 = 0x35c47f;
pub const WARN: u32 = 0xe3a23f;
pub const BAD: u32 = 0xe5484d;
pub const KNOB: u32 = 0xffffff;
/// the slider fill runs from the first to the second
pub const FILL: (u32, u32) = (0x6b3ef0, 0xe2459c);

static TRUECOLOR: OnceLock<bool> = OnceLock::new();

/// Use truecolor whatever the environment says (for snapshots).
pub fn force_truecolor() {
    let _ = TRUECOLOR.set(true);
}

fn truecolor() -> bool {
    *TRUECOLOR.get_or_init(|| {
        std::env::var("COLORTERM").is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"))
    })
}

/// A palette colour as the terminal can show it.
pub fn rgb(hex: u32) -> Color {
    let (r, g, b) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if truecolor() { Color::Rgb(r, g, b) } else { Color::Indexed(nearest_256(r, g, b)) }
}

/// The colour `t` (0 to 1) of the way along the slider fill.
pub fn fill_at(t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |shift: u32| {
        let (a, b) = (f64::from((FILL.0 >> shift) & 0xff), f64::from((FILL.1 >> shift) & 0xff));
        (a + (b - a) * t).round() as u32
    };
    rgb(mix(16) << 16 | mix(8) << 8 | mix(0))
}

pub fn fg(hex: u32) -> Style {
    Style::new().fg(rgb(hex))
}

pub fn muted() -> Style {
    fg(MUTED)
}

pub fn dim() -> Style {
    fg(DIM)
}

pub fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// The nearest colour of the xterm 256-colour palette (its 6×6×6 cube and grey ramp).
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| LEVELS.iter().enumerate().min_by_key(|(_, l)| (i32::from(**l) - i32::from(v)).abs()).unwrap().0;
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (16 + 36 * ri + 6 * gi + bi) as u8;
    let distance = |c: (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(c.0, r) + d(c.1, g) + d(c.2, b)
    };
    let cube_rgb = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let avg = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let grey_index = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let grey = 8 + 10 * grey_index;
    if distance((grey, grey, grey)) < distance(cube_rgb) { 232 + grey_index } else { cube }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_colours() {
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(0x80, 0x80, 0x80), 244);
        // the accent purple lands on a purple
        assert_eq!(nearest_256(0x7b, 0x45, 0xf5), 99);
    }
}

//! The Equalizer page: the game or mic EQ as a braille curve over ±9 dB with its ten bands, and the presets.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};

use super::cards::TWO_COLUMNS;
use super::widgets;
use crate::app::{App, EqTab, Hit};
use crate::pages;
use crate::theme::{self, rgb};

const PRESETS_WIDTH: u16 = 34;
/// The plot's range in tenths of a dB, a little beyond ±9 dB so the dots at the ends show whole.
pub const RANGE: f64 = 100.0;

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    if area.width >= TWO_COLUMNS {
        let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(PRESETS_WIDTH)]).spacing(2).areas(area);
        graph_card(f, left, app);
        presets_card(f, right, app);
        return;
    }
    // one column: the focused card first when both don't fit
    let presets = presets_height(app).min(area.height);
    let graph = 20.min(area.height);
    if graph + 1 + presets <= area.height || app.eq.pane == 0 {
        let [top, rest] = Layout::vertical([Constraint::Length(graph), Constraint::Fill(1)]).spacing(1).areas(area);
        graph_card(f, top, app);
        if rest.height >= 5 {
            presets_card(f, rest, app);
        }
    } else {
        let [top, rest] = Layout::vertical([Constraint::Length(presets), Constraint::Fill(1)]).spacing(1).areas(area);
        presets_card(f, top, app);
        if rest.height >= 8 {
            graph_card(f, rest, app);
        }
    }
}

/// A band's label in few characters: "32 Hz" → "32", "1.6 kHz" → "1.6k".
fn short(freq: &str) -> String {
    freq.replace(" kHz", "k").replace(" Hz", "")
}

/// A band's value in dB: "0", "+1.5", "−9.0".
fn db(tenths: i64) -> String {
    match tenths {
        0 => "0".into(),
        t if t > 0 => format!("+{:.1}", t as f64 / 10.0),
        t => format!("−{:.1}", -t as f64 / 10.0),
    }
}

fn graph_card(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.eq.pane == 0;
    let tab = app.eq.tab;
    let block = widgets::card("Equalizer", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let [tabs, _, plot_rows, freqs, values, _, buttons] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // GAME | MIC, and what the equalizer is set to
    let mut x = tabs.x;
    for t in [EqTab::Game, EqTab::Mic] {
        let label = t.name().to_uppercase();
        let style = if t == tab { theme::bold().fg(rgb(theme::ACCENT)).add_modifier(Modifier::UNDERLINED) } else { theme::muted() };
        let width = label.len() as u16;
        f.buffer_mut().set_string(x, tabs.y, &label, style);
        app.hits.push((Rect { x, y: tabs.y, width, height: 1 }, Hit::EqTab(t)));
        x += width + 3;
    }
    f.buffer_mut().set_string(x, tabs.y, "t", theme::dim());
    let preset = app.preset(tab);
    let (state, colour) = match pages::presets(tab, &app.values).into_iter().find(|p| p.number == preset) {
        Some(p) => (p.label, theme::SELECTED),
        None => ("Custom (unsaved)".to_string(), theme::WARN),
    };
    f.render_widget(Paragraph::new(Span::styled(state, theme::fg(colour))).alignment(Alignment::Right), tabs);

    // the plot, with dB labels in a gutter on the left
    let gutter = 6;
    let plot = Rect { x: plot_rows.x + gutter, width: plot_rows.width.saturating_sub(gutter), ..plot_rows };
    if plot.height < 3 || plot.width < 20 {
        return;
    }
    let row_of = |tenths: f64| plot.y + ((RANGE - tenths) / (2.0 * RANGE) * f64::from(plot.height - 1)).round() as u16;
    for (tenths, label) in [(90.0, "+9 dB"), (50.0, "+5"), (0.0, "0"), (-50.0, "−5"), (-90.0, "−9 dB")] {
        let style = if tenths == 0.0 { theme::fg(theme::SELECTED) } else { theme::muted() };
        f.buffer_mut().set_stringn(plot_rows.x, row_of(tenths), format!("{label:>5}"), 5, style);
    }
    let bands = app.bands(tab);
    let selected = app.eq.band;
    let enabled = app.available();
    let canvas = Canvas::default().marker(Marker::Braille).x_bounds([0.0, 10.0]).y_bounds([-RANGE, RANGE]).paint(|ctx| {
        for (tenths, colour) in [(90.0, theme::LINE), (50.0, theme::LINE), (0.0, theme::DIM), (-50.0, theme::LINE), (-90.0, theme::LINE)] {
            ctx.draw(&CanvasLine::new(0.0, tenths, 10.0, tenths, rgb(colour)));
        }
        for i in 0..10 {
            let x = i as f64 + 0.5;
            let colour = if i == selected && focused { theme::ACCENT } else { theme::LINE };
            ctx.draw(&CanvasLine::new(x, -90.0, x, 90.0, rgb(colour)));
        }
        ctx.layer();
        for i in 0..9 {
            let colour = if enabled { theme::fill_at(i as f64 / 8.0) } else { rgb(theme::DIM) };
            ctx.draw(&CanvasLine::new(i as f64 + 0.5, bands[i] as f64, i as f64 + 1.5, bands[i + 1] as f64, colour));
        }
        for (i, &b) in bands.iter().enumerate() {
            let style = match (i == selected, enabled) {
                (_, false) => theme::dim(),
                (true, true) => theme::fg(theme::SELECTED).add_modifier(Modifier::BOLD),
                (false, true) => theme::fg(theme::KNOB),
            };
            ctx.print(i as f64 + 0.5, b as f64, Span::styled("●", style));
        }
    });
    f.render_widget(canvas, plot);
    app.hits.push((plot, Hit::EqGraph(plot)));

    // frequencies and values under their bands
    let freq_names = tab.keys().frequencies;
    for i in 0..10 {
        let centre = plot.x + ((2 * i as u16 + 1) * plot.width) / 20;
        let style = if i == selected { theme::fg(theme::SELECTED).add_modifier(Modifier::BOLD) } else { theme::muted() };
        let value_style = if i == selected { theme::fg(theme::SELECTED).add_modifier(Modifier::BOLD) } else { Style::new() };
        for (row, text, style) in [(freqs, short(freq_names[i]), style), (values, db(bands[i]), value_style)] {
            let w = text.chars().count() as u16;
            f.buffer_mut().set_string(centre.saturating_sub(w / 2), row.y, text, style);
        }
    }
    f.buffer_mut().set_string(plot_rows.x, freqs.y, "   Hz", theme::dim());
    f.buffer_mut().set_string(plot_rows.x, values.y, "   dB", theme::dim());

    // the buttons: Undo and Save as new preset
    let full = pages::slots_full(tab, &app.values);
    let mut x = buttons.x + gutter;
    for (key, label, on) in [('u', "Undo", app.edited(tab) && enabled), ('s', "Save as new preset", enabled && !full)] {
        let text = format!("[ {key} {label} ]");
        let style = if on { Style::new() } else { theme::dim() };
        let width = text.chars().count() as u16;
        f.buffer_mut().set_string(x, buttons.y, &text, style);
        if on {
            f.buffer_mut().set_string(x + 2, buttons.y, key.to_string(), theme::fg(theme::ACCENT));
        }
        app.hits.push((Rect { x, y: buttons.y, width, height: 1 }, Hit::EqButton(key)));
        x += width + 3;
    }
    if full {
        f.buffer_mut().set_string(x, buttons.y, "All five custom presets are in use.", theme::muted());
    }
}

fn presets_height(app: &App) -> u16 {
    let custom = pages::presets(app.eq.tab, &app.values).iter().filter(|p| p.custom).count().max(2) as u16;
    // border, ONBOARD, 4 presets, gap, CUSTOM, unsaved, custom ones, gap, hint
    2 + 1 + 4 + 1 + 1 + 1 + custom + 2
}

fn presets_card(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.eq.pane == 1;
    let tab = app.eq.tab;
    let block = widgets::card("Presets", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let enabled = app.available();
    let active = app.preset(tab);
    let list = pages::presets(tab, &app.values);
    let item = |i: usize| {
        let p = &list[i];
        (widgets::list_item(&p.label, p.number == active, focused && app.eq.cursor == i, enabled), Some(i))
    };
    // each row, and the preset it chooses
    let mut rows: Vec<(Line, Option<usize>)> = vec![(Line::from(Span::styled(" ONBOARD", theme::muted())), None)];
    rows.extend((0..list.len()).filter(|&i| !list[i].custom).map(item));
    rows.push((Line::from(""), None));
    rows.push((Line::from(Span::styled(" CUSTOM", theme::muted())), None));
    if active == 0 {
        rows.push((widgets::list_item("Custom (unsaved)", true, false, enabled), None));
    }
    let customs: Vec<usize> = (0..list.len()).filter(|&i| list[i].custom).collect();
    if customs.is_empty() && active != 0 {
        for text in widgets::wrap("None yet. Shape the curve, then press s to save it.", inner.width as usize - 1) {
            rows.push((Line::from(Span::styled(format!(" {text}"), theme::muted())), None));
        }
    }
    rows.extend(customs.into_iter().map(item));
    if focused && list.get(app.eq.cursor).is_some_and(|p| p.custom) {
        rows.push((Line::from(""), None));
        let hint = vec![Span::styled(" dd", theme::fg(theme::ACCENT)), Span::styled(" delete this preset", theme::muted())];
        rows.push((Line::from(hint), None));
    }
    for (k, (line, preset)) in rows.into_iter().enumerate() {
        let y = inner.y + k as u16;
        if y >= inner.bottom() {
            break;
        }
        let rect = Rect { y, height: 1, ..inner };
        f.render_widget(Paragraph::new(line), rect);
        if let Some(i) = preset {
            app.hits.push((rect, Hit::EqPreset(i)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(short("32 Hz"), "32");
        assert_eq!(short("1.6 kHz"), "1.6k");
        assert_eq!(db(0), "0");
        assert_eq!(db(15), "+1.5");
        assert_eq!(db(-90), "−9.0");
    }
}

//! The Settings and Device pages: cards of rows, in two columns at 100 columns and more, otherwise in one that
//! scrolls a card at a time to keep the focus in view.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::widgets::{self, SliderLook};
use crate::app::{App, Hit};
use crate::model::{self, Card, Control, Kind, Row};
use crate::pages;
use crate::theme::{self, rgb};

/// Columns from this width on.
pub const TWO_COLUMNS: u16 = 100;

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let (cards, columns) = pages::cards(app.page, app);
    let order = pages::focus_list(app.page, app);
    let focused = app.focused();
    let columns: Vec<Vec<usize>> =
        if area.width >= TWO_COLUMNS { columns.iter().map(|c| c.to_vec()).collect() } else { vec![columns.concat()] };
    let areas = Layout::horizontal(vec![Constraint::Fill(1); columns.len()]).spacing(2).split(area);
    for (column, &rect) in columns.iter().zip(areas.iter()) {
        let heights: Vec<u16> = column.iter().map(|&c| height(&cards[c], rect.width)).collect();
        let holds_focus = |c: &usize| cards[*c].controls().any(|x| Some(x) == focused);
        let start = first_shown(&heights, column.iter().position(holds_focus), rect.height);
        let mut y = rect.y;
        for (k, &c) in column.iter().enumerate().skip(start) {
            let h = heights[k].min(rect.bottom().saturating_sub(y));
            if h < 3 {
                break;
            }
            let card_area = Rect { y, height: h, ..rect };
            draw_card(f, card_area, &cards[c], app, focused, &order);
            y += h + 1;
        }
    }
}

/// The first of a stack of cards to show so the card at `focus` fits in `space`.
pub fn first_shown(heights: &[u16], focus: Option<usize>, space: u16) -> usize {
    let Some(focus) = focus else { return 0 };
    let mut start = 0;
    while start < focus {
        let needed: u16 = heights[start..=focus].iter().sum::<u16>() + (focus - start) as u16;
        if needed <= space {
            break;
        }
        start += 1;
    }
    start
}

/// The inner width of a card: its border and a column of padding on each side.
fn inner_width(width: u16) -> u16 {
    width.saturating_sub(4)
}

fn row_height(row: &Row, width: u16) -> u16 {
    match row {
        Row::Control(c) if matches!(c.kind(), Kind::Slider { .. }) => 2,
        Row::Battery => 2,
        Row::Hint(text) => widgets::wrap(text, width as usize).len() as u16,
        _ => 1,
    }
}

pub fn height(card: &Card, width: u16) -> u16 {
    2 + card.rows.iter().map(|r| row_height(r, inner_width(width))).sum::<u16>()
}

fn draw_card(f: &mut Frame, area: Rect, card: &Card, app: &mut App, focused: Option<Control>, order: &[Control]) {
    let holds_focus = card.controls().any(|c| Some(c) == focused);
    let block = widgets::card(card.title, holds_focus);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner_width(area.width), ..inner };
    let mut y = inner.y;
    for row in &card.rows {
        let h = row_height(row, inner.width);
        if y + h > inner.bottom() {
            break;
        }
        let rect = Rect { y, height: h, ..inner };
        match row {
            Row::Control(c) => {
                let index = order.iter().position(|x| x == c).unwrap_or(0);
                draw_control(f, rect, *c, app, Some(*c) == focused, index);
            }
            Row::Hint(text) => {
                let lines: Vec<Line> = widgets::wrap(text, inner.width as usize).into_iter().map(|l| Line::from(Span::styled(l, theme::muted()))).collect();
                f.render_widget(Paragraph::new(lines), rect);
            }
            Row::Info(label, value, dot) => {
                f.render_widget(Paragraph::new(Span::styled(label.to_uppercase(), theme::muted())), rect);
                let mut spans = Vec::new();
                if let Some(colour) = dot {
                    spans.push(Span::styled("● ", theme::fg(*colour)));
                }
                spans.push(Span::raw(value.clone()));
                f.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Right), rect);
            }
            Row::Battery => draw_battery(f, rect, app),
            Row::Heading(text) => f.render_widget(Paragraph::new(Span::styled(text.to_uppercase(), theme::muted())), rect),
            Row::Text(text) => f.render_widget(Paragraph::new(text.clone()), rect),
            Row::Gap => {}
        }
        y += h;
    }
}

fn draw_battery(f: &mut Frame, rect: Rect, app: &App) {
    let v = &app.values;
    let level = v.int("battery").unwrap_or(-1);
    let text = if level < 0 {
        "unknown".to_string()
    } else {
        format!("{level}%{}", if v.flag("charging") { ", charging" } else { "" })
    };
    let [top, bar] = Layout::vertical([Constraint::Length(1); 2]).areas(rect);
    f.render_widget(Paragraph::new(Span::styled("BATTERY", theme::muted())), top);
    f.render_widget(Paragraph::new(text).alignment(Alignment::Right), top);
    let width = bar.width as usize;
    let filled = if level < 0 { 0 } else { (level.min(100) as usize * width + 50) / 100 };
    let colour = if (0..=15).contains(&level) && !v.flag("charging") { theme::BAD } else { theme::GOOD };
    let line = Line::from(vec![Span::styled("━".repeat(filled), theme::fg(colour)), Span::styled("─".repeat(width - filled), theme::fg(theme::TRACK))]);
    f.render_widget(Paragraph::new(line), bar);
}

/// One control: its label, its widget, and a purple bar at the left while it has the focus.
fn draw_control(f: &mut Frame, rect: Rect, control: Control, app: &mut App, focused: bool, index: usize) {
    let enabled = app.enabled(control);
    let read_only = control.read_only(&app.values);
    let label_style = match (focused, enabled || read_only) {
        (true, _) => theme::bold().fg(rgb(theme::ACCENT)),
        (false, true) => Style::new(),
        (false, false) => theme::dim(),
    };
    if focused {
        for y in rect.y..rect.bottom() {
            f.buffer_mut().set_string(rect.x - 1, y, "▌", theme::fg(theme::ACCENT));
        }
    }
    let top = Rect { height: 1, ..rect };
    f.render_widget(Paragraph::new(Span::styled(control.label().to_uppercase(), label_style)), top);
    let mut track = None;
    let right: Vec<Span> = match control.kind() {
        Kind::Slider { min, max, centred, ends, .. } => {
            let value = model::slider_value(control, &app.values);
            let shown = if control == Control::Volume && !read_only { format!("{value}%") } else { value.to_string() };
            // the value is on the label line; only the AMB-ANC slider names its ends
            let look = SliderLook {
                ends: ends.unwrap_or(("", "")),
                centred,
                knob: !read_only,
                enabled: enabled || read_only,
            };
            let (line, start, width) = widgets::slider(value, min, max, look, rect.width);
            let bar = Rect { y: rect.y + 1, height: 1, ..rect };
            f.render_widget(Paragraph::new(line), bar);
            if enabled {
                track = Some((rect.x + start, width));
            }
            vec![Span::styled(shown, if enabled || read_only { theme::muted() } else { theme::dim() })]
        }
        Kind::Toggle(_) => widgets::toggle(app.is_on(control), enabled),
        Kind::Segmented(_) => widgets::segmented(app.is_on(control), enabled),
        Kind::Choice(name) => {
            let current = app.values.int(name);
            let label = control.options().into_iter().find(|(v, _)| Some(*v) == current).map(|(_, l)| l).unwrap_or_else(|| "–".into());
            widgets::choice(&label, focused, enabled)
        }
    };
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right).style(Style::new().remove_modifier(Modifier::BOLD)), top);
    app.hits.push((rect, Hit::Control { index, track }));
}

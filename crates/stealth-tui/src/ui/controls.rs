//! The Controls page: the left earcup's controls, and the functions for the mode button or the assignable wheel.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::cards::TWO_COLUMNS;
use super::widgets;
use crate::app::{App, Hit};
use crate::pages::{self, ASSIGNABLE, EARCUP};
use crate::theme::{self, rgb};

const HINT: &str = "Choose the mode button or the assignable wheel, then pick what it does. The wheel on the right earcup sets the Bluetooth volume.";

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    if area.width >= TWO_COLUMNS {
        let [left, right] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).spacing(2).areas(area);
        let functions = pages::functions(app.controls.control).1.len() as u16 + 2;
        earcup_card(f, Rect { height: earcup_height(left.width, false).min(left.height), ..left }, app, false);
        functions_card(f, Rect { height: functions.min(right.height), ..right }, app);
        return;
    }
    // one column: the earcup without gaps and the hint, so the functions fit below
    let earcup = earcup_height(area.width, true).min(area.height);
    let functions = (pages::functions(app.controls.control).1.len() as u16 + 2).min(area.height);
    if earcup + 1 + functions <= area.height || app.controls.pane == 0 {
        let [top, rest] = Layout::vertical([Constraint::Length(earcup), Constraint::Fill(1)]).spacing(1).areas(area);
        earcup_card(f, top, app, true);
        if rest.height >= 3 {
            functions_card(f, rest, app);
        }
    } else {
        let [top, rest] = Layout::vertical([Constraint::Length(functions), Constraint::Fill(1)]).spacing(1).areas(area);
        functions_card(f, top, app);
        if rest.height >= 6 {
            earcup_card(f, rest, app, true);
        }
    }
}

fn earcup_height(width: u16, compact: bool) -> u16 {
    if compact {
        // border, five controls of two lines
        2 + 5 * 2
    } else {
        // border, five controls of two lines with gaps, the hint
        2 + 5 * 3 + widgets::wrap(HINT, width.saturating_sub(4) as usize).len() as u16
    }
}

fn earcup_card(f: &mut Frame, area: Rect, app: &mut App, compact: bool) {
    let focused = app.controls.pane == 0;
    let block = widgets::card("Left earcup", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let enabled = app.available();
    let selected = ASSIGNABLE[app.controls.control];
    let mut y = inner.y;
    for (row, (title, function)) in EARCUP.iter().enumerate() {
        if y + 2 > inner.bottom() {
            return;
        }
        let assignable = ASSIGNABLE.iter().position(|&r| r == row);
        let function = match (function, assignable) {
            (Some(text), _) => text.to_string(),
            (None, Some(k)) => pages::function_label(k, &app.values),
            (None, None) => String::new(),
        };
        let title_style = match assignable {
            _ if !enabled => theme::dim(),
            Some(_) if row == selected => theme::bold().fg(rgb(theme::SELECTED)),
            Some(_) => theme::bold(),
            None => theme::muted(),
        };
        let mark = match assignable {
            Some(_) if row == selected => Span::styled("● ", theme::fg(if enabled { theme::SELECTED } else { theme::DIM })),
            Some(_) => Span::styled("○ ", theme::dim()),
            None => Span::raw("  "),
        };
        let rect = Rect { y, height: 2, ..inner };
        if focused && row == selected {
            for line in y..y + 2 {
                f.buffer_mut().set_string(rect.x - 1, line, "▌", theme::fg(theme::ACCENT));
            }
        }
        let lines = vec![
            Line::from(vec![mark, Span::styled(title.to_uppercase(), title_style)]),
            Line::from(Span::styled(
                format!("  {}", fit(&function, inner.width as usize - 2)),
                if assignable.is_some() && enabled { Style::new() } else { theme::muted() },
            )),
        ];
        f.render_widget(Paragraph::new(lines), rect);
        if let Some(k) = assignable {
            app.hits.push((rect, Hit::Earcup(k)));
        }
        y += if compact { 2 } else { 3 };
    }
    if compact {
        return;
    }
    let hint: Vec<Line> = widgets::wrap(HINT, inner.width as usize).into_iter().map(|l| Line::from(Span::styled(l, theme::muted()))).collect();
    f.render_widget(Paragraph::new(hint), Rect { y, height: inner.bottom().saturating_sub(y), ..inner });
}

/// `text` cut to `width` characters, with an ellipsis when it was longer.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        text.chars().take(width.saturating_sub(1)).chain(['…']).collect()
    }
}

fn functions_card(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.controls.pane == 1;
    let title = if app.controls.control == 0 { "Button functions" } else { "Wheel functions" };
    let block = widgets::card(title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let (name, functions) = pages::functions(app.controls.control);
    let current = app.values.int(name);
    let enabled = app.available();
    for (i, (value, label)) in functions.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.bottom() {
            break;
        }
        let rect = Rect { y, height: 1, ..inner };
        f.render_widget(Paragraph::new(widgets::list_item(label, Some(*value) == current, focused && app.controls.cursor == i, enabled)), rect);
        app.hits.push((rect, Hit::Function(i)));
    }
}

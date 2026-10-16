//! Drawing: the header with the page tabs, battery and connection; a banner when the headset can't be reached;
//! the page; the status bar with key hints and messages; and popups. Clickable places are recorded in `App::hits`.

mod cards;
mod controls;
mod equalizer;
pub mod widgets;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::app::{App, Daemon, Hit, Page, Popup};
use crate::theme::{self, rgb};

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!("Make the terminal at least {MIN_WIDTH}×{MIN_HEIGHT} (it is {}×{}).", area.width, area.height);
        let y = area.y + area.height / 2;
        f.render_widget(Paragraph::new(text).alignment(Alignment::Center).wrap(Wrap { trim: true }), Rect { y, height: area.height - (y - area.y), ..area });
        return;
    }
    let [header, banner, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    draw_header(f, header, app);
    let body = body.inner(Margin { horizontal: 1, vertical: 0 });
    match app.daemon {
        Daemon::Stopped => not_running(f, body),
        Daemon::Waiting => {
            let y = body.y + body.height / 2;
            f.render_widget(Paragraph::new("Connecting to stealthd…").style(theme::muted()).alignment(Alignment::Center), Rect { y, height: 1, ..body });
        }
        Daemon::Running => {
            if let Some(problem) = app.problem() {
                let line = Line::from(vec![Span::styled(" ▲ ", theme::fg(theme::WARN)), Span::styled(problem, theme::fg(theme::WARN))]);
                f.render_widget(Paragraph::new(line), banner);
            }
            match app.page {
                Page::Settings | Page::Device => cards::draw(f, body, app),
                Page::Equalizer => equalizer::draw(f, body, app),
                Page::Controls => controls::draw(f, body, app),
            }
        }
    }
    draw_status(f, status, app);
    draw_popup(f, area, app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &mut App) {
    let mut spans = vec![Span::styled(" STEALTH", theme::bold()), Span::styled(" PRO II ", theme::muted()), Span::raw("  ")];
    let mut x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
    for page in Page::ALL {
        let label = format!(" {} ", page.title());
        let width = label.chars().count() as u16;
        let style = if page == app.page {
            Style::new().fg(rgb(theme::KNOB)).bg(rgb(theme::ACCENT)).add_modifier(Modifier::BOLD)
        } else {
            theme::muted()
        };
        app.hits.push((Rect { x, y: area.y, width, height: 1 }, Hit::Tab(page)));
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
        x += width + 1;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // battery and connection on the right, shorter when the tabs leave little room
    let room = (area.x + area.width).saturating_sub(x);
    let v = &app.values;
    let battery = (app.daemon == Daemon::Running && v.flag("connected") && v.flag("headset_link"))
        .then(|| v.int("battery").filter(|&b| b >= 0))
        .flatten();
    let (dot, text) = match app.daemon {
        Daemon::Waiting => (theme::DIM, "Connecting"),
        Daemon::Stopped => (theme::BAD, "stealthd stopped"),
        Daemon::Running if !v.flag("connected") => (theme::BAD, "No transmitter"),
        Daemon::Running if !v.flag("headset_link") => (theme::WARN, "Headset off"),
        Daemon::Running => (theme::GOOD, "Connected"),
    };
    let charging = v.flag("charging");
    let variants = [(5, true), (0, true), (0, false)];
    for (bar, with_text) in variants {
        let mut right = Vec::new();
        if let Some(level) = battery {
            if bar > 0 {
                right.extend(widgets::battery(level, charging, bar));
            } else {
                right.push(Span::raw(format!("{level}%{}", if charging { " ↯" } else { "" })));
            }
            right.push(Span::raw("   "));
        }
        right.push(Span::styled("● ", theme::fg(dot)));
        if with_text {
            right.push(Span::raw(text));
        }
        right.push(Span::raw(" "));
        let line = Line::from(right);
        if (line.width() as u16) < room || !with_text {
            f.render_widget(Paragraph::new(line).alignment(Alignment::Right), area);
            break;
        }
    }
}

fn not_running(f: &mut Frame, area: Rect) {
    let card = centered(area, 64, 9);
    f.render_widget(Clear, card);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme::fg(theme::LINE)).title(Span::styled(" STEALTHD IS NOT RUNNING ", theme::bold()));
    let inner = block.inner(card).inner(Margin { horizontal: 2, vertical: 1 });
    f.render_widget(block, card);
    let lines = vec![
        Line::from("This app talks to the headset through the stealthd service."),
        Line::from("Start it with:"),
        Line::from(""),
        Line::from(Span::styled(" systemctl --user start stealthd ", Style::new().bg(rgb(theme::FIELD)))),
        Line::from(""),
        Line::from(Span::styled("This screen connects by itself once it runs.", theme::muted())),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

/// Key hints for where the focus is, a message, or the keys typed so far.
fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let pending = app.keys.pending();
    if let Some(command) = &app.keys.command {
        f.render_widget(Paragraph::new(format!(":{command}")), area);
        return;
    }
    let left = match &app.toast {
        Some(t) => Line::from(Span::styled(format!(" {}", t.text), theme::fg(if t.error { theme::BAD } else { theme::SELECTED }))),
        None => Line::from(hints(app).into_iter().flat_map(|(k, what)| [Span::styled(format!(" {k}"), theme::fg(theme::ACCENT)), Span::styled(format!(" {what} "), theme::muted())]).collect::<Vec<_>>()),
    };
    f.render_widget(Paragraph::new(left), area);
    if !pending.is_empty() {
        f.render_widget(Paragraph::new(format!("{pending} ")).alignment(Alignment::Right), area);
    }
}

fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut h = match (app.daemon, app.page) {
        (Daemon::Running, Page::Settings | Page::Device) => vec![("j/k", "move"), ("h/l", "change"), ("Tab", "next card")],
        (Daemon::Running, Page::Equalizer) if app.eq.pane == 0 => {
            vec![("h/l", "band"), ("j/k", "lower/raise"), ("t", "game/mic"), ("u", "undo"), ("s", "save"), ("Tab", "presets")]
        }
        (Daemon::Running, Page::Equalizer) => vec![("j/k", "move"), ("Enter", "choose"), ("dd", "delete"), ("Tab", "graph")],
        (Daemon::Running, Page::Controls) => vec![("j/k", "move"), ("Enter", "choose"), ("Tab", "switch list")],
        _ => vec![],
    };
    h.extend([("gt", "next page"), ("?", "help"), ("q", "quit")]);
    h
}

fn draw_popup(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(popup) = app.popup.clone() else { return };
    match popup {
        Popup::Help(scroll) => {
            let lines = help_lines();
            let height = (lines.len() as u16 + 2).min(area.height - 2);
            let rect = centered(area, 72, height);
            let max = (lines.len() as u16 + 2).saturating_sub(height);
            let scroll = scroll.min(max);
            if let Some(Popup::Help(s)) = &mut app.popup {
                *s = scroll;
            }
            let block = popup_block(" KEYS ").title_bottom(Line::from(Span::styled(" j/k scroll · Esc close ", theme::muted())).right_aligned());
            f.render_widget(Clear, rect);
            f.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), rect);
        }
        Popup::Choose { control, cursor } => {
            let options = control.options();
            let rect = centered(area, 40, options.len() as u16 + 2);
            f.render_widget(Clear, rect);
            let block = popup_block(&format!(" {} ", control.label().to_uppercase()));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let current = control.kind();
            let active = match current {
                crate::model::Kind::Choice(name) => app.values.int(name),
                _ => None,
            };
            for (i, (value, label)) in options.iter().enumerate() {
                let row = Rect { y: inner.y + i as u16, height: 1, ..inner };
                app.hits.push((row, Hit::Option(i)));
                f.render_widget(Paragraph::new(widgets::list_item(label, Some(*value) == active, i == cursor, true)), row);
            }
        }
        Popup::SaveName { tab, input } => {
            let rect = centered(area, 50, 8);
            f.render_widget(Clear, rect);
            let block = popup_block(&format!(" SAVE {} EQ AS NEW PRESET ", tab.name().to_uppercase()));
            let inner = block.inner(rect).inner(Margin { horizontal: 1, vertical: 1 });
            f.render_widget(block, rect);
            let [label, field, problem, keys] = Layout::vertical([Constraint::Length(1); 4]).areas(inner);
            f.render_widget(Paragraph::new(Span::styled("NAME", theme::muted())), label);
            let field_style = Style::new().bg(rgb(theme::FIELD));
            f.render_widget(Paragraph::new(format!(" {}", input.text)).style(field_style), field);
            f.set_cursor_position((field.x + 1 + input.cursor as u16, field.y));
            let text = app.name_problem(tab, input.text.trim());
            f.render_widget(Paragraph::new(Span::styled(text, theme::fg(theme::BAD))), problem);
            f.render_widget(Paragraph::new(Line::from(vec![Span::styled("Enter", theme::fg(theme::ACCENT)), Span::styled(" save  ", theme::muted()), Span::styled("Esc", theme::fg(theme::ACCENT)), Span::styled(" cancel", theme::muted())])), keys);
        }
        Popup::ConfirmDelete { name, .. } => {
            let rect = centered(area, 50, 5);
            f.render_widget(Clear, rect);
            let block = popup_block(" DELETE PRESET ");
            let inner = block.inner(rect).inner(Margin { horizontal: 1, vertical: 0 });
            f.render_widget(block, rect);
            let lines = vec![
                Line::from(format!("Delete the preset “{name}”?")),
                Line::from(""),
                Line::from(vec![Span::styled("y", theme::fg(theme::ACCENT)), Span::styled(" delete  ", theme::muted()), Span::styled("n", theme::fg(theme::ACCENT)), Span::styled(" keep", theme::muted())]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
    }
}

fn popup_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::fg(theme::ACCENT))
        .title(Span::styled(title.to_string(), theme::bold()))
}

/// A rectangle of the given size in the middle of `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 2, width, height }
}

fn help_lines() -> Vec<Line<'static>> {
    let section = |s: &'static str| Line::from(Span::styled(s, theme::bold()));
    let key = |k: &'static str, what: &'static str| Line::from(vec![Span::styled(format!("  {k:<22}"), theme::fg(theme::ACCENT)), Span::raw(what)]);
    vec![
        section("Everywhere"),
        key("j k  ↓ ↑", "next / previous control (with a count: 3j)"),
        key("h l  ← →", "change: −1 / +1, previous / next option, off / on"),
        key("H L  Shift-← Shift-→", "change by 10"),
        key("0  $", "minimum / maximum ($ does not apply to master volume)"),
        key("gg G  Home End", "first / last control"),
        key("Enter  Space", "toggle, or open the options"),
        key("Tab  Shift-Tab", "next / previous card"),
        key("gt gT  ] [", "next / previous page"),
        key("2gt", "page 2 (Settings 1, Equalizer 2, Controls 3, Device 4)"),
        key("?", "this help"),
        key("q  :q  Ctrl-c", "quit"),
        key("Esc", "close, or forget a count"),
        Line::from(""),
        section("Equalizer graph"),
        key("h l  ← →", "choose a band"),
        key("k j  ↑ ↓", "raise / lower it by 0.5 dB"),
        key("K J", "raise / lower it by 3 dB"),
        key("0 $", "first / last band"),
        key("=", "set the band to 0 dB"),
        key("t", "switch between the game and mic EQ"),
        key("u", "undo the changes (back to the preset)"),
        key("s", "save as a new preset"),
        Line::from(""),
        section("Equalizer presets"),
        key("j k  Enter", "move, choose"),
        key("dd  x", "delete a custom preset"),
        Line::from(""),
        section("Mouse"),
        key("click", "tabs, controls, options, presets, toggles"),
        key("wheel", "change the slider or band under the pointer"),
        key("drag", "sliders and EQ bands"),
    ]
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use stealth_dbus::link::Event;

    use super::*;
    use crate::app::tests::connected;
    use crate::app::EqTab;
    use crate::line_input::LineInput;
    use crate::model::Control;

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_page_and_popup_draws_at_both_sizes() {
        for (w, h) in [(80, 24), (140, 40)] {
            let mut app = connected();
            for (page, expected) in [
                (Page::Settings, ["MASTER VOLUME", "80%"]),
                (Page::Equalizer, ["+9 dB", "Signature Sound"]),
                (Page::Controls, ["MODE BUTTON", "ACTIVE NOISE CANCELLATION ON/OFF"]),
                (Page::Device, ["BATTERY", "82%"]),
            ] {
                app.page = page;
                let s = screen(&mut app, w, h);
                for text in expected {
                    assert!(s.contains(text), "{page:?} at {w}×{h} lacks {text:?}:\n{s}");
                }
                assert!(!app.hits.is_empty());
            }
            app.page = Page::Settings;
            for (popup, expected) in [
                (Popup::Help(0), "KEYS"),
                (Popup::Choose { control: Control::AutoShutoff, cursor: 0 }, "30 MINUTES"),
                (Popup::SaveName { tab: EqTab::Game, input: LineInput { text: "Bassy".into(), cursor: 5 } }, "already a preset"),
                (Popup::ConfirmDelete { tab: EqTab::Game, name: "Bassy".into() }, "Delete the preset “Bassy”?"),
            ] {
                app.popup = Some(popup);
                let s = screen(&mut app, w, h);
                assert!(s.contains(expected), "popup at {w}×{h} lacks {expected:?}:\n{s}");
            }
            // the presets come first on a narrow screen when they have the focus
            let mut app = connected();
            app.page = Page::Equalizer;
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), Instant::now());
            assert!(screen(&mut app, w, h).contains("SIGNATURE SOUND"));
        }
    }

    #[test]
    fn other_states() {
        let mut app = connected();
        assert!(screen(&mut app, 60, 20).contains("Make the terminal at least 80×24"));
        app.daemon_event(Event::Stopped, Instant::now());
        let s = screen(&mut app, 80, 24);
        assert!(s.contains("STEALTHD IS NOT RUNNING") && s.contains("systemctl --user start stealthd"), "{s}");
        let mut app = connected();
        app.daemon_event(
            Event::Changed([("headset_link".to_string(), tb_proto::values::Typed::Bool(false))].into()),
            Instant::now(),
        );
        let s = screen(&mut app, 120, 30);
        assert!(s.contains("The headset is off or out of range") && s.contains("Headset off"), "{s}");
        assert!(screen(&mut App::new(), 80, 24).contains("Connecting to stealthd"));
    }
}

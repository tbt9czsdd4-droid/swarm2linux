//! The app's state and what keys, the mouse and stealthd do to it. Writes for stealthd collect in an outbox:
//! toggles, options and presets go out at once, sliders and EQ bands once they have rested for `DEBOUNCE`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use stealth_dbus::link::{Command, Event};
use stealth_dbus::view::{self, EqEdit};
use tb_proto::keys::{self, EqKeys};
use tb_proto::values::Typed;

use crate::input::{self, Action, KeyState, Outcome};
use crate::line_input::LineInput;
use crate::model::{self, Control, Kind, Values};
use crate::pages;
use crate::ui::{equalizer::RANGE, widgets};

/// How long a slider or EQ band rests before its value is written: the terminal's "write on release".
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// How long a message stays in the status bar.
pub const TOAST: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Settings,
    Equalizer,
    Controls,
    Device,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Settings, Page::Equalizer, Page::Controls, Page::Device];

    pub fn title(self) -> &'static str {
        match self {
            Page::Settings => "Settings",
            Page::Equalizer => "Equalizer",
            Page::Controls => "Controls",
            Page::Device => "Device",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daemon {
    /// before the first answer
    Waiting,
    Running,
    Stopped,
}

/// The game or the mic equalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EqTab {
    Game,
    Mic,
}

impl EqTab {
    pub fn name(self) -> &'static str {
        match self {
            EqTab::Game => "game",
            EqTab::Mic => "mic",
        }
    }

    pub fn keys(self) -> &'static EqKeys {
        match self {
            EqTab::Game => &keys::GAME_EQ,
            EqTab::Mic => &keys::MIC_EQ,
        }
    }
}

/// Where the Equalizer page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EqView {
    pub tab: EqTab,
    pub band: usize,
    /// 0: the graph, 1: the preset list
    pub pane: usize,
    pub cursor: usize,
}

/// Where the Controls page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlsView {
    /// 0: the mode button, 1: the assignable wheel
    pub control: usize,
    /// 0: the earcup, 1: the function list
    pub pane: usize,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Help(u16),
    Choose { control: Control, cursor: usize },
    SaveName { tab: EqTab, input: LineInput },
    ConfirmDelete { tab: EqTab, name: String },
}

/// Something the mouse can hit, recorded while drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(Page),
    /// a control of the Settings or Device page, by its place in the focus order; for a slider, the track's x
    /// range
    Control { index: usize, track: Option<(u16, u16)> },
    /// the EQ graph: its plot area
    EqGraph(Rect),
    EqTab(EqTab),
    EqPreset(usize),
    EqButton(char),
    Earcup(usize),
    Function(usize),
    /// a row of the option popup
    Option(usize),
}

/// What the left mouse button is dragging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    /// a slider: its control and track (first column, width)
    Slider(Control, u16, u16),
    /// an EQ band, along the plot's height
    Band(usize, Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum PendingKey {
    Value(String),
    Band(EqTab, usize),
}

#[derive(Debug, Clone, PartialEq)]
struct Pending {
    key: PendingKey,
    command: Command,
    due: Instant,
}

/// A change asked of a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    By(i64),
    Min,
    Max,
    Activate,
    To(i64),
}

pub struct App {
    pub values: Values,
    pub daemon: Daemon,
    pub battery_notifications: Option<bool>,
    pub page: Page,
    /// the focused control of the Settings and Device pages (others unused)
    pub focus: [usize; 4],
    pub eq: EqView,
    pub controls: ControlsView,
    pub popup: Option<Popup>,
    pub toast: Option<Toast>,
    pub keys: KeyState,
    pub hits: Vec<(Rect, Hit)>,
    /// what the left mouse button is dragging
    pub drag: Option<Drag>,
    pending: Vec<Pending>,
    edits: HashMap<EqTab, EqEdit>,
    outbox: Vec<Command>,
    pub quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            values: Values::default(),
            daemon: Daemon::Waiting,
            battery_notifications: None,
            page: Page::Settings,
            focus: [0; 4],
            eq: EqView { tab: EqTab::Game, band: 0, pane: 0, cursor: 0 },
            controls: ControlsView { control: 0, pane: 0, cursor: 0 },
            popup: None,
            toast: None,
            keys: KeyState::default(),
            hits: Vec::new(),
            drag: None,
            pending: Vec::new(),
            edits: HashMap::new(),
            outbox: Vec::new(),
            quit: false,
        }
    }

    /// Whether the headset can be changed: stealthd runs, the transmitter is there and the headset is linked.
    pub fn available(&self) -> bool {
        self.daemon == Daemon::Running && self.values.flag("connected") && self.values.flag("headset_link")
    }

    /// The banner text when the headset can't be reached, as in the desktop app.
    pub fn problem(&self) -> Option<&'static str> {
        if self.daemon != Daemon::Running || self.available() {
            None
        } else if !self.values.flag("connected") {
            Some("No transmitter found. Plug in the USB transmitter or the headset's cable.")
        } else {
            Some("The headset is off or out of range. Settings show their last known values until it reconnects.")
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool, now: Instant) {
        self.toast = Some(Toast { text: text.into(), error, until: now + TOAST });
    }

    /// Whether an equalizer has an edit Cancel (`u`) can undo.
    pub fn edited(&self, tab: EqTab) -> bool {
        self.edits.contains_key(&tab)
    }

    // ---- stealthd ----

    pub fn daemon_event(&mut self, event: Event, now: Instant) {
        match event {
            Event::Snapshot(Some(values), notifications) => {
                self.daemon = Daemon::Running;
                self.values = Values(values);
                if notifications.is_some() {
                    self.battery_notifications = notifications;
                }
                self.after_update();
            }
            Event::Snapshot(None, _) | Event::Stopped => self.daemon = Daemon::Stopped,
            Event::Changed(values) => {
                for (name, value) in values {
                    self.values.0.insert(name, value);
                }
                self.after_update();
            }
            Event::WriteFailed(message) | Event::Failed(message) => self.toast(message, true, now),
        }
    }

    /// After new values: a chosen preset ends an edit, and writes still waiting keep their value on screen.
    fn after_update(&mut self) {
        for tab in [EqTab::Game, EqTab::Mic] {
            if self.values.int(&format!("{}_eq_preset", tab.name())).is_some_and(|p| p != 0) {
                self.edits.remove(&tab);
            }
        }
        for p in &self.pending {
            match &p.command {
                Command::Set(name, value) => self.values.set(name, value.clone()),
                Command::SetBand(eq, band, tenths) => {
                    self.values.set_band(eq, *band as usize - 1, i64::from(*tenths));
                    self.values.set(&format!("{eq}_eq_preset"), Typed::Int(0));
                }
                _ => {}
            }
        }
    }

    // ---- writes ----

    fn write(&mut self, command: Command) {
        self.outbox.push(command);
    }

    fn write_later(&mut self, key: PendingKey, command: Command, now: Instant) {
        self.pending.retain(|p| p.key != key);
        self.pending.push(Pending { key, command, due: now + DEBOUNCE });
    }

    /// The writes due by `now`, in order.
    pub fn take_commands(&mut self, now: Instant) -> Vec<Command> {
        let mut due: Vec<Pending> = Vec::new();
        self.pending.retain(|p| {
            if p.due <= now {
                due.push(p.clone());
                false
            } else {
                true
            }
        });
        due.sort_by_key(|p| p.due);
        let mut out = std::mem::take(&mut self.outbox);
        out.extend(due.into_iter().map(|p| p.command));
        out
    }

    /// When something is due next: a waiting write or the end of a message.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.iter().map(|p| p.due).chain(self.toast.as_ref().map(|t| t.until)).min()
    }

    pub fn tick(&mut self, now: Instant) {
        if self.toast.as_ref().is_some_and(|t| t.until <= now) {
            self.toast = None;
        }
    }

    // ---- terminal events ----

    pub fn terminal_event(&mut self, event: TermEvent, now: Instant) {
        match event {
            TermEvent::Key(key) if key.kind != KeyEventKind::Release => self.key(key, now),
            TermEvent::Mouse(mouse) => self.mouse(mouse, now),
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent, now: Instant) {
        if self.popup.is_some() {
            return self.popup_key(key, now);
        }
        match self.keys.key(key) {
            Outcome::Action(action) => self.action(action, now),
            Outcome::Command(line) => match input::command(&line) {
                Ok(action) => self.action(action, now),
                Err(message) if !message.is_empty() => self.toast(message, true, now),
                Err(_) => {}
            },
            Outcome::Pending => {}
        }
    }

    pub fn action(&mut self, action: Action, now: Instant) {
        match action {
            Action::Quit => self.quit = true,
            Action::Help => self.popup = Some(Popup::Help(0)),
            Action::Page(n) => {
                if let Some(&page) = Page::ALL.get(n.wrapping_sub(1)) {
                    self.page = page;
                }
            }
            Action::NextPage => self.page = Page::ALL[(self.page.index() + 1) % 4],
            Action::PrevPage => self.page = Page::ALL[(self.page.index() + 3) % 4],
            Action::Escape => self.keys.clear(),
            _ if self.daemon != Daemon::Running => {}
            _ => match self.page {
                Page::Settings | Page::Device => self.list_action(action, now),
                Page::Equalizer => self.eq_action(action, now),
                Page::Controls => self.controls_action(action, now),
            },
        }
    }

    // ---- Settings and Device ----

    pub fn focused(&self) -> Option<Control> {
        pages::focus_list(self.page, self).get(self.focus[self.page.index()]).copied()
    }

    fn list_action(&mut self, action: Action, now: Instant) {
        let list = pages::focus_list(self.page, self);
        if list.is_empty() {
            return;
        }
        let i = self.page.index();
        let last = list.len() as i64 - 1;
        let focus = self.focus[i] as i64;
        let control = list[self.focus[i].min(list.len() - 1)];
        match action {
            Action::Move(n) => self.focus[i] = (focus + n).clamp(0, last) as usize,
            Action::BigMove(n) => self.focus[i] = (focus + 5 * n).clamp(0, last) as usize,
            Action::First => self.focus[i] = 0,
            Action::Last => self.focus[i] = last as usize,
            Action::NextPane | Action::PrevPane => {
                let (cards, columns) = pages::cards(self.page, self);
                // the first control of each card, in focus order
                let mut starts = Vec::new();
                let mut index = 0;
                for &c in columns.iter().flat_map(|col| col.iter()) {
                    let n = cards[c].controls().count();
                    if n > 0 {
                        starts.push(index);
                    }
                    index += n;
                }
                let current = starts.iter().rposition(|&s| s <= self.focus[i]).unwrap_or(0);
                let next = if action == Action::NextPane {
                    (current + 1) % starts.len()
                } else {
                    (current + starts.len() - 1) % starts.len()
                };
                self.focus[i] = starts[next];
            }
            Action::Adjust(n) => self.change(control, Change::By(n), now),
            Action::BigAdjust(n) => self.change(control, Change::By(10 * n), now),
            Action::Min => self.change(control, Change::Min, now),
            Action::Max => self.change(control, Change::Max, now),
            Action::Activate => self.change(control, Change::Activate, now),
            _ => {}
        }
    }

    /// Whether a control takes changes now.
    pub fn enabled(&self, control: Control) -> bool {
        match control {
            Control::BatteryPopups => self.daemon == Daemon::Running && self.battery_notifications.is_some(),
            Control::SuperhumanMode | Control::SuperhumanLevel => self.available() && self.values.flag("superhuman"),
            Control::Volume => self.available() && !control.read_only(&self.values),
            _ => self.available(),
        }
    }

    /// Whether a toggle or Off/On control is on.
    pub fn is_on(&self, control: Control) -> bool {
        match (control, control.kind()) {
            (Control::BatteryPopups, _) => self.battery_notifications == Some(true),
            (_, Kind::Toggle(name) | Kind::Segmented(name)) => self.values.flag(name),
            _ => false,
        }
    }

    pub fn change(&mut self, control: Control, change: Change, now: Instant) {
        if !self.enabled(control) {
            return;
        }
        match control.kind() {
            Kind::Slider { value, min, max, .. } => {
                let current = model::slider_value(control, &self.values);
                let new = match change {
                    Change::By(n) => current + n,
                    Change::To(n) => n,
                    Change::Min => min,
                    // full volume never comes from one keypress
                    Change::Max if control == Control::Volume => {
                        return self.toast("Turn the volume up with l or L", false, now);
                    }
                    Change::Max => max,
                    Change::Activate => return,
                }
                .clamp(min, max);
                if new != current {
                    self.values.set(value, Typed::Int(new));
                    self.write_later(
                        PendingKey::Value(value.into()),
                        Command::Set(value.into(), Typed::Int(new)),
                        now,
                    );
                }
            }
            Kind::Toggle(name) | Kind::Segmented(name) => {
                let current = self.is_on(control);
                let on = match change {
                    Change::By(n) | Change::To(n) => n > 0,
                    Change::Min => false,
                    Change::Max => true,
                    Change::Activate => !current,
                };
                if on == current {
                    return;
                }
                if control == Control::BatteryPopups {
                    self.battery_notifications = Some(on);
                    self.write(Command::BatteryNotifications(on));
                } else {
                    self.values.set(name, Typed::Bool(on));
                    self.write(Command::Set(name.into(), Typed::Bool(on)));
                }
            }
            Kind::Choice(name) => {
                let options = control.options();
                let current = self.values.int(name);
                let at = options.iter().position(|(v, _)| Some(*v) == current).unwrap_or(0) as i64;
                let last = options.len() as i64 - 1;
                let to = match change {
                    Change::By(n) => (at + n).clamp(0, last),
                    Change::To(n) => n.clamp(0, last),
                    Change::Min => 0,
                    Change::Max => last,
                    Change::Activate => {
                        self.popup = Some(Popup::Choose { control, cursor: at as usize });
                        return;
                    }
                };
                let value = options[to as usize].0;
                if Some(value) != current {
                    self.values.set(name, Typed::Int(value));
                    self.write(Command::Set(name.into(), Typed::Int(value)));
                }
            }
        }
    }

    // ---- Equalizer ----

    pub fn bands(&self, tab: EqTab) -> Vec<i64> {
        let mut bands = self.values.ints(&format!("{}_eq_bands", tab.name())).to_vec();
        bands.resize(10, 0);
        bands
    }

    pub fn preset(&self, tab: EqTab) -> i64 {
        self.values.int(&format!("{}_eq_preset", tab.name())).unwrap_or(0)
    }

    fn eq_action(&mut self, action: Action, now: Instant) {
        let tab = self.eq.tab;
        match action {
            Action::NextPane | Action::PrevPane => {
                self.eq.pane = 1 - self.eq.pane;
                if self.eq.pane == 1 {
                    // start at the active preset
                    let active = self.preset(tab);
                    self.eq.cursor = pages::presets(tab, &self.values).iter().position(|p| p.number == active).unwrap_or(0);
                }
            }
            Action::Key('t') => {
                self.eq.tab = if tab == EqTab::Game { EqTab::Mic } else { EqTab::Game };
                self.eq.cursor = 0;
            }
            Action::Key('u') => self.undo(tab, now),
            Action::Key('s') => self.start_save(tab, now),
            _ if self.eq.pane == 0 => self.graph_action(action, now),
            _ => self.preset_action(action, now),
        }
    }

    fn graph_action(&mut self, action: Action, now: Instant) {
        let tab = self.eq.tab;
        let band = self.eq.band as i64;
        let current = self.bands(tab)[self.eq.band];
        match action {
            Action::Adjust(n) => self.eq.band = (band + n).clamp(0, 9) as usize,
            Action::BigAdjust(n) => self.eq.band = (band + 5 * n).clamp(0, 9) as usize,
            Action::Min | Action::First => self.eq.band = 0,
            Action::Max | Action::Last => self.eq.band = 9,
            // down lowers the band, up raises it: 0.5 dB a step, 3 dB a big step
            Action::Move(n) => self.set_band(tab, self.eq.band, current - 5 * n, now),
            Action::BigMove(n) => self.set_band(tab, self.eq.band, current - 30 * n, now),
            Action::Key('=') => self.set_band(tab, self.eq.band, 0, now),
            _ => {}
        }
    }

    /// Set one band (tenths of a dB, kept to ±9 dB in 0.5 dB steps); the first change remembers the equalizer for
    /// Cancel.
    pub fn set_band(&mut self, tab: EqTab, band: usize, tenths: i64, now: Instant) {
        if !self.available() {
            return;
        }
        let tenths = (tenths.clamp(-90, 90) as f64 / 5.0).round() as i64 * 5;
        let bands = self.bands(tab);
        if bands[band] == tenths {
            return;
        }
        let preset = self.preset(tab);
        self.edits.entry(tab).or_insert_with(|| EqEdit {
            preset: preset as i32,
            bands: bands.iter().map(|&b| b as i32).collect(),
        });
        let eq = tab.name();
        self.values.set_band(eq, band, tenths);
        // the headset switches to unsaved custom bands; show that right away
        self.values.set(&format!("{eq}_eq_preset"), Typed::Int(0));
        self.write_later(PendingKey::Band(tab, band), Command::SetBand(eq.into(), band as u32 + 1, tenths as i32), now);
    }

    fn preset_action(&mut self, action: Action, now: Instant) {
        let tab = self.eq.tab;
        let list = pages::presets(tab, &self.values);
        let last = list.len() as i64 - 1;
        let cursor = self.eq.cursor as i64;
        match action {
            Action::Move(n) => self.eq.cursor = (cursor + n).clamp(0, last) as usize,
            Action::BigMove(n) => self.eq.cursor = (cursor + 3 * n).clamp(0, last) as usize,
            Action::First | Action::Min => self.eq.cursor = 0,
            Action::Last | Action::Max => self.eq.cursor = last as usize,
            Action::Activate | Action::Adjust(1..) => self.choose_preset(tab, self.eq.cursor, now),
            Action::Adjust(_) => self.eq.pane = 0,
            Action::Delete => {
                if let Some(p) = list.get(self.eq.cursor).filter(|p| p.custom)
                    && self.available()
                {
                    self.popup = Some(Popup::ConfirmDelete { tab, name: p.label.clone() });
                }
            }
            _ => {}
        }
    }

    pub fn choose_preset(&mut self, tab: EqTab, index: usize, _now: Instant) {
        let Some(preset) = pages::presets(tab, &self.values).get(index).cloned() else { return };
        if !self.available() {
            return;
        }
        self.eq.cursor = index;
        // band writes still waiting would change the preset just chosen
        self.pending.retain(|p| !matches!(p.key, PendingKey::Band(t, _) if t == tab));
        self.edits.remove(&tab);
        let name = format!("{}_eq_preset", tab.name());
        self.values.set(&name, Typed::Int(preset.number));
        self.write(Command::Set(name, Typed::Int(preset.number)));
    }

    /// Cancel: go back to the preset and bands from before the first change.
    pub fn undo(&mut self, tab: EqTab, now: Instant) {
        let Some(edit) = self.edits.remove(&tab) else {
            return self.toast("Nothing to undo", false, now);
        };
        if !self.available() {
            self.edits.insert(tab, edit);
            return;
        }
        self.pending.retain(|p| !matches!(p.key, PendingKey::Band(t, _) if t == tab));
        let now_bands: Vec<i32> = self.bands(tab).iter().map(|&b| b as i32).collect();
        for command in edit.cancel_writes(tab.name(), &now_bands) {
            self.write(command);
        }
        let eq = tab.name();
        self.values.set(&format!("{eq}_eq_preset"), Typed::Int(edit.preset.into()));
        self.values.set(&format!("{eq}_eq_bands"), Typed::Ints(edit.bands.iter().map(|&b| i64::from(b)).collect()));
    }

    pub fn start_save(&mut self, tab: EqTab, now: Instant) {
        if !self.available() {
            return;
        }
        if pages::slots_full(tab, &self.values) {
            return self.toast("All five custom presets are in use.", true, now);
        }
        self.popup = Some(Popup::SaveName { tab, input: LineInput::default() });
    }

    /// Why the name being typed can't be saved, or "".
    pub fn name_problem(&self, tab: EqTab, name: &str) -> &'static str {
        view::preset_name_problem(self.values.texts(&format!("{}_eq_slots", tab.name())), name)
    }

    // ---- Controls ----

    fn controls_action(&mut self, action: Action, now: Instant) {
        let (_, functions) = pages::functions(self.controls.control);
        let last = functions.len() as i64 - 1;
        let cursor = self.controls.cursor as i64;
        match (self.controls.pane, action) {
            (_, Action::NextPane | Action::PrevPane) => self.controls_pane(1 - self.controls.pane),
            (0, Action::Move(n) | Action::BigMove(n)) => self.controls.control = (self.controls.control as i64 + n).clamp(0, 1) as usize,
            (0, Action::First | Action::Min) => self.controls.control = 0,
            (0, Action::Last | Action::Max) => self.controls.control = 1,
            (0, Action::Activate | Action::Adjust(1..)) => self.controls_pane(1),
            (1, Action::Move(n) | Action::BigMove(n)) => self.controls.cursor = (cursor + n).clamp(0, last) as usize,
            (1, Action::First | Action::Min) => self.controls.cursor = 0,
            (1, Action::Last | Action::Max) => self.controls.cursor = last as usize,
            (1, Action::Activate | Action::Adjust(1..)) => self.choose_function(self.controls.cursor, now),
            (1, Action::Adjust(_)) => self.controls_pane(0),
            _ => {}
        }
    }

    /// Switch between the earcup and the function list; the list starts at the active function.
    pub fn controls_pane(&mut self, pane: usize) {
        self.controls.pane = pane;
        if pane == 1 {
            let (name, functions) = pages::functions(self.controls.control);
            let current = self.values.int(name);
            self.controls.cursor = functions.iter().position(|(v, _)| Some(*v) == current).unwrap_or(0);
        }
    }

    pub fn choose_function(&mut self, index: usize, _now: Instant) {
        let (name, functions) = pages::functions(self.controls.control);
        let Some(&(value, _)) = functions.get(index) else { return };
        if !self.available() {
            return;
        }
        self.controls.cursor = index;
        if self.values.int(name) != Some(value) {
            self.values.set(name, Typed::Int(value));
            self.write(Command::Set(name.into(), Typed::Int(value)));
        }
    }

    // ---- popups ----

    fn popup_key(&mut self, key: KeyEvent, now: Instant) {
        let Some(popup) = self.popup.take() else { return };
        let plain = |c: char| key.code == KeyCode::Char(c);
        let close = key.code == KeyCode::Esc || plain('q');
        if key.code == KeyCode::Char('c') && key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        self.popup = match popup {
            Popup::Help(scroll) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => Some(Popup::Help(scroll.saturating_add(1))),
                KeyCode::Char('k') | KeyCode::Up => Some(Popup::Help(scroll.saturating_sub(1))),
                KeyCode::Char('g') | KeyCode::Home => Some(Popup::Help(0)),
                _ if close || plain('?') || key.code == KeyCode::Enter => None,
                _ => Some(Popup::Help(scroll)),
            },
            Popup::Choose { control, cursor } => {
                let last = control.options().len().saturating_sub(1);
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => Some(Popup::Choose { control, cursor: (cursor + 1).min(last) }),
                    KeyCode::Char('k') | KeyCode::Up => Some(Popup::Choose { control, cursor: cursor.saturating_sub(1) }),
                    KeyCode::Char('g') | KeyCode::Home => Some(Popup::Choose { control, cursor: 0 }),
                    KeyCode::Char('G') | KeyCode::End => Some(Popup::Choose { control, cursor: last }),
                    KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right => {
                        self.change(control, Change::To(cursor as i64), now);
                        None
                    }
                    _ if close || plain('h') || key.code == KeyCode::Left => None,
                    _ => Some(Popup::Choose { control, cursor }),
                }
            }
            Popup::SaveName { tab, mut input } => match key.code {
                KeyCode::Esc => None,
                KeyCode::Enter => {
                    let name = input.text.trim().to_string();
                    let problem = self.name_problem(tab, &name);
                    if name.is_empty() || !problem.is_empty() {
                        Some(Popup::SaveName { tab, input })
                    } else {
                        self.write(Command::SavePreset(tab.name().into(), name));
                        None
                    }
                }
                _ => {
                    input.key(key);
                    Some(Popup::SaveName { tab, input })
                }
            },
            Popup::ConfirmDelete { tab, name } => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    self.write(Command::DeletePreset(tab.name().into(), name));
                    None
                }
                _ if close || plain('n') => None,
                _ => Some(Popup::ConfirmDelete { tab, name }),
            },
        };
    }

    // ---- mouse ----

    /// The topmost clickable thing at a cell: the one drawn last.
    fn hit_at(&self, x: u16, y: u16) -> Option<(Rect, Hit)> {
        self.hits.iter().rev().find(|(r, _)| x >= r.x && x < r.right() && y >= r.y && y < r.bottom()).copied()
    }

    pub fn mouse(&mut self, mouse: MouseEvent, now: Instant) {
        let (x, y) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(x, y, now),
            MouseEventKind::Drag(MouseButton::Left) => match self.drag {
                Some(Drag::Slider(control, start, width)) => self.slide(control, start, width, x, now),
                Some(Drag::Band(band, plot)) => self.set_band(self.eq.tab, band, tenths_at(plot, y), now),
                None => {}
            },
            MouseEventKind::Up(_) => self.drag = None,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let step = if mouse.kind == MouseEventKind::ScrollUp { 1 } else { -1 };
                self.scroll(x, y, step, now);
            }
            _ => {}
        }
    }

    fn click(&mut self, x: u16, y: u16, now: Instant) {
        let hit = self.hit_at(x, y);
        match &self.popup {
            Some(Popup::Choose { control, .. }) => {
                let control = *control;
                if let Some((_, Hit::Option(i))) = hit {
                    self.change(control, Change::To(i as i64), now);
                }
                self.popup = None;
                return;
            }
            Some(Popup::Help(_)) => {
                self.popup = None;
                return;
            }
            // typing and confirming take keys
            Some(_) => return,
            None => {}
        }
        let Some((rect, hit)) = hit else { return };
        self.keys.clear();
        match hit {
            Hit::Tab(page) => self.page = page,
            _ if self.daemon != Daemon::Running => {}
            Hit::Control { index, track } => {
                self.focus[self.page.index()] = index;
                let Some(control) = self.focused() else { return };
                match (control.kind(), track) {
                    // the bar is the control's second line
                    (Kind::Slider { .. }, Some((start, width))) if y == rect.y + 1 => {
                        self.drag = Some(Drag::Slider(control, start, width));
                        self.slide(control, start, width, x, now);
                    }
                    (Kind::Slider { .. }, _) => {}
                    _ => self.change(control, Change::Activate, now),
                }
            }
            Hit::EqGraph(plot) => {
                self.eq.pane = 0;
                let band = (((x - plot.x) as usize * 10) / plot.width.max(1) as usize).min(9);
                self.eq.band = band;
                self.drag = Some(Drag::Band(band, plot));
                self.set_band(self.eq.tab, band, tenths_at(plot, y), now);
            }
            Hit::EqTab(tab) => self.eq.tab = tab,
            Hit::EqPreset(i) => {
                self.eq.pane = 1;
                self.choose_preset(self.eq.tab, i, now);
            }
            Hit::EqButton('u') => self.undo(self.eq.tab, now),
            Hit::EqButton(_) => self.start_save(self.eq.tab, now),
            Hit::Earcup(k) => {
                self.controls.control = k;
                self.controls.pane = 0;
            }
            Hit::Function(i) => {
                self.controls.pane = 1;
                self.choose_function(i, now);
            }
            Hit::Option(_) => {}
        }
    }

    /// Set a slider to the value under column `x` of its track.
    fn slide(&mut self, control: Control, start: u16, width: u16, x: u16, now: Instant) {
        if let Kind::Slider { min, max, .. } = control.kind() {
            let value = widgets::slider_value_at(x.saturating_sub(start), width, min, max);
            self.change(control, Change::To(value), now);
        }
    }

    /// The wheel changes the slider or EQ band under the pointer, and moves through the option popup.
    fn scroll(&mut self, x: u16, y: u16, step: i64, now: Instant) {
        if let Some(Popup::Choose { control, cursor }) = self.popup {
            let last = control.options().len().saturating_sub(1) as i64;
            self.popup = Some(Popup::Choose { control, cursor: (cursor as i64 - step).clamp(0, last) as usize });
            return;
        }
        if self.popup.is_some() || self.daemon != Daemon::Running {
            return;
        }
        match self.hit_at(x, y) {
            Some((_, Hit::Control { index, .. })) => {
                self.focus[self.page.index()] = index;
                if let Some(control) = self.focused().filter(|c| matches!(c.kind(), Kind::Slider { .. })) {
                    self.change(control, Change::By(step), now);
                }
            }
            Some((_, Hit::EqGraph(plot))) => {
                let band = (((x - plot.x) as usize * 10) / plot.width.max(1) as usize).min(9);
                self.eq.band = band;
                self.eq.pane = 0;
                let current = self.bands(self.eq.tab)[band];
                self.set_band(self.eq.tab, band, current + 5 * step, now);
            }
            _ => {}
        }
    }
}

/// The band value (tenths of a dB) at row `y` of the EQ plot.
fn tenths_at(plot: Rect, y: u16) -> i64 {
    let row = f64::from(y.saturating_sub(plot.y).min(plot.height.saturating_sub(1)));
    (RANGE - row / f64::from(plot.height.saturating_sub(1).max(1)) * 2.0 * RANGE).round() as i64
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    /// An app connected to a headset with the user's usual settings.
    pub fn connected() -> App {
        let mut v = BTreeMap::new();
        let mut put = |k: &str, t: Typed| {
            v.insert(k.to_string(), t);
        };
        put("connected", Typed::Bool(true));
        put("headset_link", Typed::Bool(true));
        put("device", Typed::Text("10f5:2287 Stealth Pro II PC transmitter".into()));
        put("battery", Typed::Int(82));
        put("sink_volume", Typed::Int(80));
        put("volume", Typed::Int(71));
        put("anc", Typed::Bool(false));
        put("anc_level", Typed::Int(100));
        put("superhuman", Typed::Bool(false));
        put("superhuman_mode", Typed::Int(0));
        put("mic_sensitivity", Typed::Int(100));
        put("auto_shutoff", Typed::Int(4));
        put("button_function", Typed::Int(0));
        put("wheel_function", Typed::Int(1));
        put("game_eq_preset", Typed::Int(1));
        put("game_eq_bands", Typed::Ints(vec![0; 10]));
        put("game_eq_slots", Typed::Texts(vec!["Bassy".into(), "".into(), "".into(), "".into(), "".into()]));
        put("mic_eq_preset", Typed::Int(1));
        put("mic_eq_bands", Typed::Ints(vec![0; 10]));
        put("transmitters", Typed::Texts(vec!["1 2287 active".into()]));
        let mut app = App::new();
        app.daemon_event(Event::Snapshot(Some(v), Some(true)), Instant::now());
        app
    }

    fn press(app: &mut App, keys: &str, now: Instant) {
        for c in keys.chars() {
            let code = match c {
                '\t' => KeyCode::Tab,
                '\n' => KeyCode::Enter,
                '\x1b' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            app.key(KeyEvent::new(code, KeyModifiers::NONE), now);
        }
    }

    fn set(name: &str, value: Typed) -> Command {
        Command::Set(name.into(), value)
    }

    #[test]
    fn moves_with_counts_and_ends() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "3j", t);
        assert_eq!(app.focused(), Some(Control::SuperhumanLevel));
        press(&mut app, "G", t);
        assert_eq!(app.focused(), Some(Control::AiNoiseReduction));
        press(&mut app, "gg", t);
        assert_eq!(app.focused(), Some(Control::Volume));
        press(&mut app, "\t", t);
        assert_eq!(app.focused(), Some(Control::Sensitivity), "Tab goes to the next card");
        press(&mut app, "gt", t);
        assert_eq!(app.page, Page::Equalizer);
        press(&mut app, "4gt", t);
        assert_eq!(app.page, Page::Device);
        press(&mut app, "gT", t);
        assert_eq!(app.page, Page::Controls);
        assert!(app.take_commands(t).is_empty(), "moving writes nothing");
    }

    #[test]
    fn toggles_write_at_once() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "4j", t); // active noise cancellation
        assert_eq!(app.focused(), Some(Control::Anc));
        press(&mut app, "l", t);
        assert_eq!(app.take_commands(t), [set("anc", Typed::Bool(true))]);
        press(&mut app, "l", t);
        assert!(app.take_commands(t).is_empty(), "already on");
        press(&mut app, " ", t);
        assert_eq!(app.take_commands(t), [set("anc", Typed::Bool(false))]);
    }

    #[test]
    fn sliders_write_once_they_rest() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "hhh", t);
        assert!(app.take_commands(t).is_empty());
        assert_eq!(app.values.int("sink_volume"), Some(77), "shown at once");
        press(&mut app, "H", t + Duration::from_millis(200));
        assert!(app.take_commands(t + Duration::from_millis(400)).is_empty(), "the last change restarts the wait");
        assert_eq!(app.take_commands(t + Duration::from_millis(500)), [set("sink_volume", Typed::Int(67))]);
        // a change from the headset while a write waits does not undo the shown value
        press(&mut app, "l", t);
        app.daemon_event(Event::Changed(BTreeMap::from([("sink_volume".into(), Typed::Int(67))])), t);
        assert_eq!(app.values.int("sink_volume"), Some(68));
    }

    #[test]
    fn full_volume_needs_more_than_one_key() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "$", t);
        assert_eq!(app.values.int("sink_volume"), Some(80));
        assert!(app.toast.is_some());
        press(&mut app, "0", t);
        assert_eq!(app.take_commands(t + DEBOUNCE), [set("sink_volume", Typed::Int(0))]);
        // other sliders do go to their maximum
        press(&mut app, "j", t);
        assert_eq!(app.focused(), Some(Control::Superhuman));
    }

    #[test]
    fn options_and_disabled_controls() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "2jl", t); // Superhuman Hearing mode, while it is off
        assert!(app.take_commands(t).is_empty());
        press(&mut app, "kl", t); // switch it on
        press(&mut app, "jl", t);
        assert_eq!(
            app.take_commands(t),
            [set("superhuman", Typed::Bool(true)), set("superhuman_mode", Typed::Int(1))]
        );
        press(&mut app, "\n", t);
        assert!(matches!(app.popup, Some(Popup::Choose { control: Control::SuperhumanMode, cursor: 1 })));
        press(&mut app, "j\n", t);
        assert_eq!(app.take_commands(t), [set("superhuman_mode", Typed::Int(2))]);
        assert!(app.popup.is_none());
    }

    #[test]
    fn nothing_is_written_while_the_headset_is_away() {
        let mut app = connected();
        let t = Instant::now();
        app.daemon_event(Event::Changed(BTreeMap::from([("headset_link".into(), Typed::Bool(false))])), t);
        assert!(app.problem().is_some());
        press(&mut app, "hh5jl", t);
        press(&mut app, "gtk s", t);
        assert!(app.take_commands(t + DEBOUNCE).is_empty());
        assert!(app.popup.is_none());
        // stealthd's own setting still works
        press(&mut app, "4gtgg\t\th", t);
        assert_eq!((app.page, app.focused()), (Page::Device, Some(Control::BatteryPopups)));
        assert_eq!(app.take_commands(t), [Command::BatteryNotifications(false)]);
    }

    #[test]
    fn eq_edit_and_undo() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "gt", t);
        press(&mut app, "lkkK", t); // band 2 up by 0.5 + 0.5 + 3 dB
        assert_eq!(app.bands(EqTab::Game)[1], 40);
        assert_eq!(app.preset(EqTab::Game), 0, "unsaved custom bands right away");
        assert!(app.edited(EqTab::Game));
        assert_eq!(app.take_commands(t + DEBOUNCE), [Command::SetBand("game".into(), 2, 40)]);
        press(&mut app, "9K", t);
        assert_eq!(app.bands(EqTab::Game)[1], 90, "kept to +9 dB");
        press(&mut app, "u", t);
        assert_eq!(app.take_commands(t + DEBOUNCE), [set("game_eq_preset", Typed::Int(1))], "the waiting band write is dropped");
        assert_eq!((app.preset(EqTab::Game), app.bands(EqTab::Game)[1]), (1, 0));
        assert!(!app.edited(EqTab::Game));
        press(&mut app, "$=", t);
        assert_eq!(app.eq.band, 9);
    }

    #[test]
    fn eq_presets_save_and_delete() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "gt\t", t);
        assert_eq!((app.eq.pane, app.eq.cursor), (1, 0), "the list starts at the active preset");
        press(&mut app, "j\n", t);
        assert_eq!(app.take_commands(t), [set("game_eq_preset", Typed::Int(2))]);
        press(&mut app, "G", t);
        press(&mut app, "dd", t);
        assert!(matches!(&app.popup, Some(Popup::ConfirmDelete { name, .. }) if name == "Bassy"));
        press(&mut app, "y", t);
        assert_eq!(app.take_commands(t), [Command::DeletePreset("game".into(), "Bassy".into())]);
        press(&mut app, "s", t);
        press(&mut app, "bassy\n", t);
        assert!(app.popup.is_some(), "a taken name is refused");
        assert!(app.take_commands(t).is_empty());
        app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL), t);
        press(&mut app, "Treble\n", t);
        assert_eq!(app.take_commands(t), [Command::SavePreset("game".into(), "Treble".into())]);
        assert!(app.popup.is_none());
    }

    #[test]
    fn controls_page() {
        let mut app = connected();
        let t = Instant::now();
        press(&mut app, "3gtj\t", t);
        assert_eq!((app.controls.control, app.controls.pane, app.controls.cursor), (1, 1, 0), "wheel function 1 is first");
        press(&mut app, "jj\n", t);
        assert_eq!(app.take_commands(t), [set("wheel_function", Typed::Int(3))]);
    }

    #[test]
    fn quits() {
        for keys in ["q", ":q\n"] {
            let mut app = connected();
            press(&mut app, keys, Instant::now());
            assert!(app.quit, "{keys:?}");
        }
        let mut app = connected();
        press(&mut app, ":nope\n", Instant::now());
        assert!(!app.quit);
        assert!(app.toast.as_ref().is_some_and(|t| t.error));
    }
}

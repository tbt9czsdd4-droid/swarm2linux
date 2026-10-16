//! `stealth-tui`: the terminal app for the Stealth Pro II, with the desktop app's pages. Like the desktop app it is
//! a client of stealthd and never touches the device itself.
//!
//! One channel carries the terminal's events (from an input thread) and stealthd's (from `stealth_dbus::link`);
//! the main thread applies them, sends the writes that are due and draws.

mod app;
mod input;
mod line_input;
mod model;
mod pages;
mod snapshot;
mod theme;
mod ui;

use std::io::stdout;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event as TermEvent};
use ratatui::crossterm::execute;
use stealth_dbus::link::{self, Event, Link};

use crate::app::App;

enum Input {
    Terminal(TermEvent),
    Daemon(Event),
}

const USAGE: &str = "Usage: stealth-tui

The terminal app for the Turtle Beach Stealth Pro II. It needs stealthd running.
Press ? inside for the keys; q quits.";

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-V" | "--version") => {
            println!("stealth-tui {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some(other) => {
            eprintln!("stealth-tui: unknown argument {other}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
    if let Some(path) = std::env::var_os("STEALTH_TUI_SNAPSHOT") {
        return snapshot::run(path.into());
    }

    let (tx, rx) = mpsc::channel();
    let daemon = tx.clone();
    let link = link::start(move |event| {
        let _ = daemon.send(Input::Daemon(event));
    })?;

    // ratatui's panic hook restores the terminal and then calls this one, which lets go of the mouse
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    thread::Builder::new().name("terminal-input".into()).spawn(move || {
        while let Ok(event) = event::read() {
            if tx.send(Input::Terminal(event)).is_err() {
                break;
            }
        }
    })?;
    let result = run(&mut terminal, &rx, &link);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, inputs: &Receiver<Input>, link: &Link) -> Result<()> {
    let mut app = App::new();
    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let now = Instant::now();
        let wait = app.next_deadline().map_or(Duration::from_secs(1), |d| d.saturating_duration_since(now));
        match inputs.recv_timeout(wait.min(Duration::from_secs(1))) {
            Ok(input) => apply(&mut app, input),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(input) = inputs.try_recv() {
            apply(&mut app, input);
        }
        let now = Instant::now();
        app.tick(now);
        for command in app.take_commands(now) {
            link.send(command);
        }
        if app.quit {
            break;
        }
    }
    Ok(())
}

fn apply(app: &mut App, input: Input) {
    let now = Instant::now();
    match input {
        Input::Terminal(event) => app.terminal_event(event, now),
        Input::Daemon(event) => app.daemon_event(event, now),
    }
}

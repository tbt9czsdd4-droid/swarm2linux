//! Keys to actions, vim style: counts (`3j`), the `g` prefix (`gg`, `gt`, `2gt`), `dd`, and `:` commands. The
//! arrow keys, Home/End and Enter do the same as their vim counterparts.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key asks for; pages decide what it means for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `j`/`k`, `↓`/`↑`: down (positive) or up, `count` times.
    Move(i64),
    /// `J`/`K`: a bigger move down or up.
    BigMove(i64),
    /// `h`/`l`, `←`/`→`: left (negative) or right.
    Adjust(i64),
    /// `H`/`L`, `Shift-←`/`Shift-→`: a bigger step.
    BigAdjust(i64),
    /// `gg`, `Home`
    First,
    /// `G`, `End`
    Last,
    /// `0`
    Min,
    /// `$`
    Max,
    /// `Enter`, `Space`
    Activate,
    /// `Tab`, `Shift-Tab`
    NextPane,
    PrevPane,
    /// `{n}gt`
    Page(usize),
    /// `gt`, `]`, `Ctrl-PageDown`; `gT`, `[`, `Ctrl-PageUp`
    NextPage,
    PrevPage,
    /// `dd`, `x`
    Delete,
    /// other keys a page may use: `t`, `u`, `s`, `=`
    Key(char),
    Help,
    Quit,
    /// `Esc`: closes or cancels
    Escape,
}

/// Keys typed so far that don't make an action yet.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KeyState {
    count: Option<u32>,
    g: bool,
    d: bool,
    /// the `:` command line while it is being typed
    pub command: Option<String>,
}

/// What a key did in normal mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Action(Action),
    /// a `:` command was entered
    Command(String),
    /// more keys are needed, or the key means nothing
    Pending,
}

impl KeyState {
    /// What is typed but not done yet, for the status bar.
    pub fn pending(&self) -> String {
        if let Some(c) = &self.command {
            return format!(":{c}");
        }
        let mut s = self.count.map(|n| n.to_string()).unwrap_or_default();
        if self.g {
            s.push('g');
        }
        if self.d {
            s.push('d');
        }
        s
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Take one key in normal mode.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.clear();
            return Outcome::Action(Action::Quit);
        }
        if let Some(line) = &mut self.command {
            return match key.code {
                KeyCode::Enter => {
                    let line = std::mem::take(line);
                    self.clear();
                    Outcome::Command(line)
                }
                KeyCode::Esc => {
                    self.clear();
                    Outcome::Pending
                }
                KeyCode::Backspace => {
                    if line.pop().is_none() {
                        self.clear();
                    }
                    Outcome::Pending
                }
                KeyCode::Char(c) => {
                    line.push(c);
                    Outcome::Pending
                }
                _ => Outcome::Pending,
            };
        }

        let count = i64::from(self.count.unwrap_or(1));
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let (g, d) = (self.g, self.d);
        self.g = false;
        self.d = false;

        let action = match key.code {
            // a count: 1-9 start it, 0 continues it
            KeyCode::Char(c @ '0'..='9') if c != '0' || self.count.is_some() => {
                let digit = c.to_digit(10).unwrap_or(0);
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(digit).min(999));
                return Outcome::Pending;
            }
            KeyCode::Char('g') if g => Action::First,
            KeyCode::Char('t') if g => match self.count {
                Some(n) => Action::Page(n as usize),
                None => Action::NextPage,
            },
            KeyCode::Char('T') if g => Action::PrevPage,
            KeyCode::Char('g') => {
                self.g = true;
                return Outcome::Pending;
            }
            KeyCode::Char('d') if d => Action::Delete,
            KeyCode::Char('d') => {
                self.d = true;
                return Outcome::Pending;
            }
            KeyCode::Char(':') => {
                self.clear();
                self.command = Some(String::new());
                return Outcome::Pending;
            }
            KeyCode::Char('j') | KeyCode::Down => Action::Move(count),
            KeyCode::Char('k') | KeyCode::Up => Action::Move(-count),
            KeyCode::Char('J') => Action::BigMove(count),
            KeyCode::Char('K') => Action::BigMove(-count),
            KeyCode::Left if shift => Action::BigAdjust(-count),
            KeyCode::Right if shift => Action::BigAdjust(count),
            KeyCode::Char('h') | KeyCode::Left => Action::Adjust(-count),
            KeyCode::Char('l') | KeyCode::Right => Action::Adjust(count),
            KeyCode::Char('H') => Action::BigAdjust(-count),
            KeyCode::Char('L') => Action::BigAdjust(count),
            KeyCode::Char('G') | KeyCode::End => Action::Last,
            KeyCode::Home => Action::First,
            KeyCode::Char('0') => Action::Min,
            KeyCode::Char('$') => Action::Max,
            KeyCode::Enter | KeyCode::Char(' ') => Action::Activate,
            KeyCode::Tab => Action::NextPane,
            KeyCode::BackTab => Action::PrevPane,
            KeyCode::Char(']') => Action::NextPage,
            KeyCode::Char('[') => Action::PrevPage,
            KeyCode::PageDown if ctrl => Action::NextPage,
            KeyCode::PageUp if ctrl => Action::PrevPage,
            KeyCode::Char('x') => Action::Delete,
            KeyCode::Char('?') => Action::Help,
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc => Action::Escape,
            KeyCode::Char(c @ ('t' | 'u' | 's' | '=')) => Action::Key(c),
            _ => {
                self.clear();
                return Outcome::Pending;
            }
        };
        self.count = None;
        Outcome::Action(action)
    }
}

/// What a `:` command does: quit or help, or an error message.
pub fn command(line: &str) -> Result<Action, String> {
    match line.trim() {
        "q" | "q!" | "qa" | "qa!" | "quit" | "wq" | "x" => Ok(Action::Quit),
        "h" | "help" => Ok(Action::Help),
        "" => Err(String::new()),
        other => Err(format!("Not a command: :{other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(state: &mut KeyState, typed: &str) -> Vec<Outcome> {
        typed.chars().map(|c| state.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))).collect()
    }

    fn actions(typed: &str) -> Vec<Action> {
        let mut state = KeyState::default();
        keys(&mut state, typed)
            .into_iter()
            .filter_map(|o| if let Outcome::Action(a) = o { Some(a) } else { None })
            .collect()
    }

    #[test]
    fn motions_and_counts() {
        assert_eq!(actions("jk"), [Action::Move(1), Action::Move(-1)]);
        assert_eq!(actions("3j"), [Action::Move(3)]);
        assert_eq!(actions("12l"), [Action::Adjust(12)]);
        assert_eq!(actions("10h"), [Action::Adjust(-10)], "0 continues a count");
        assert_eq!(actions("0"), [Action::Min], "0 alone is the minimum");
        assert_eq!(actions("$H"), [Action::Max, Action::BigAdjust(-1)]);
        assert_eq!(actions("2K"), [Action::BigMove(-2)]);
    }

    #[test]
    fn prefixes() {
        assert_eq!(actions("gg"), [Action::First]);
        assert_eq!(actions("G"), [Action::Last]);
        assert_eq!(actions("gt"), [Action::NextPage]);
        assert_eq!(actions("gT"), [Action::PrevPage]);
        assert_eq!(actions("3gt"), [Action::Page(3)]);
        assert_eq!(actions("dd"), [Action::Delete]);
        assert_eq!(actions("dj"), [Action::Move(1)], "d then another key is just that key");
        assert_eq!(actions("x"), [Action::Delete]);
        assert_eq!(actions("tus="), [Action::Key('t'), Action::Key('u'), Action::Key('s'), Action::Key('=')]);
    }

    #[test]
    fn pending_shows_what_is_typed() {
        let mut state = KeyState::default();
        keys(&mut state, "2g");
        assert_eq!(state.pending(), "2g");
        assert_eq!(state.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)), Outcome::Action(Action::Escape));
        assert_eq!(state.pending(), "");
    }

    #[test]
    fn arrows_do_what_hjkl_do() {
        let mut state = KeyState::default();
        let key = |code, m| KeyEvent::new(code, m);
        assert_eq!(state.key(key(KeyCode::Down, KeyModifiers::NONE)), Outcome::Action(Action::Move(1)));
        assert_eq!(state.key(key(KeyCode::Left, KeyModifiers::NONE)), Outcome::Action(Action::Adjust(-1)));
        assert_eq!(state.key(key(KeyCode::Right, KeyModifiers::SHIFT)), Outcome::Action(Action::BigAdjust(1)));
        assert_eq!(state.key(key(KeyCode::Home, KeyModifiers::NONE)), Outcome::Action(Action::First));
        assert_eq!(state.key(key(KeyCode::PageDown, KeyModifiers::CONTROL)), Outcome::Action(Action::NextPage));
        assert_eq!(state.key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Outcome::Action(Action::Quit));
    }

    #[test]
    fn command_line() {
        let mut state = KeyState::default();
        let out = keys(&mut state, ":q");
        assert_eq!(state.pending(), ":q");
        assert!(out.iter().all(|o| *o == Outcome::Pending));
        assert_eq!(state.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Outcome::Command("q".into()));
        assert_eq!(command("q"), Ok(Action::Quit));
        assert_eq!(command("help"), Ok(Action::Help));
        assert_eq!(command("foo"), Err("Not a command: :foo".into()));
    }
}

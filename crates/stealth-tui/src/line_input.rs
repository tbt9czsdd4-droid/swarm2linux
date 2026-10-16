//! A one-line text field: typing, Backspace/Delete, ←/→, Home/End, Ctrl-w (word) and Ctrl-u (all).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LineInput {
    pub text: String,
    /// in characters
    pub cursor: usize,
}

impl LineInput {
    fn byte(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Edit with a key; false if the key is not an editing key.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('w') if ctrl => {
                let end = self.byte(self.cursor);
                let before = &self.text[..end];
                let start = before.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.cursor = self.text[..start].chars().count();
                self.text.replace_range(start..end, "");
            }
            KeyCode::Char('u') if ctrl => {
                let end = self.byte(self.cursor);
                self.text.replace_range(..end, "");
                self.cursor = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let at = self.byte(self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let at = self.byte(self.cursor - 1);
                self.text.remove(at);
                self.cursor -= 1;
            }
            KeyCode::Delete if self.cursor < self.len() => {
                let at = self.byte(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut LineInput, code: KeyCode, modifiers: KeyModifiers) {
        input.key(KeyEvent::new(code, modifiers));
    }

    #[test]
    fn edits() {
        let mut i = LineInput::default();
        for c in "Bass ünd".chars() {
            press(&mut i, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!((i.text.as_str(), i.cursor), ("Bass ünd", 8));
        press(&mut i, KeyCode::Left, KeyModifiers::NONE);
        press(&mut i, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(i.text, "Bass üd");
        press(&mut i, KeyCode::End, KeyModifiers::NONE);
        press(&mut i, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!((i.text.as_str(), i.cursor), ("Bass ", 5));
        press(&mut i, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!((i.text.as_str(), i.cursor), ("", 0));
        assert!(!i.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    }
}

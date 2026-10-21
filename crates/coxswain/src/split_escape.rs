//! An Esc read together with the key after it. When Esc and, say, F10 arrive in one read
//! (`\x1b\x1b[21~`, as macOS Terminal sends them to an app that was busy), crossterm takes the
//! second Esc as part of the first and hands the rest over as the letters `[21~`. This puts the
//! key back together.

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// After an Esc press: the events that follow it in the same read (`next` gives them, `None` when
/// none is waiting), with a key whose escape crossterm swallowed put back together. Events that
/// are no such key come back as they were.
pub(crate) fn after_esc(mut next: impl FnMut() -> Option<Event>) -> Vec<Event> {
    let mut got = Vec::new();
    let mut text = String::new();
    while let Some(e) = next() {
        let c = match &e {
            Event::Key(KeyEvent { code: KeyCode::Char(c), modifiers, kind: KeyEventKind::Press, .. })
                if (*modifiers - KeyModifiers::SHIFT).is_empty() =>
            {
                Some(*c)
            }
            _ => None,
        };
        got.push(e);
        let Some(c) = c else { return got };
        if text.is_empty() && !matches!(c, '[' | 'O') {
            return got;
        }
        text.push(c);
        let done = match text.as_bytes() {
            [b'O', _] => true,
            [b'[', rest @ ..] => rest.last().is_some_and(|b| !b.is_ascii_digit() && *b != b';'),
            _ => false,
        };
        if done {
            return match key(&text) {
                Some(k) => vec![Event::Key(k)],
                None => got,
            };
        }
        if text.len() > 12 {
            return got;
        }
    }
    got
}

/// `[21~`, `[1;5A`, `OP` and the like (an escape sequence without its Esc) as the key it is.
fn key(seq: &str) -> Option<KeyEvent> {
    let (body, last) = seq.split_at(seq.len() - 1);
    let last = last.chars().next()?;
    let mut nums = body.get(1..)?.split(';');
    let first = nums.next().filter(|s| !s.is_empty());
    let modifiers = match nums.next() {
        Some(m) => {
            let bits = m.parse::<u8>().ok()?.checked_sub(1)?;
            let mut out = KeyModifiers::NONE;
            if bits & 1 != 0 {
                out |= KeyModifiers::SHIFT;
            }
            if bits & 2 != 0 {
                out |= KeyModifiers::ALT;
            }
            if bits & 4 != 0 {
                out |= KeyModifiers::CONTROL;
            }
            out
        }
        None => KeyModifiers::NONE,
    };
    let code = match (seq.as_bytes()[0], last) {
        (b'[', '~') => match first?.parse::<u8>().ok()? {
            1 | 7 => KeyCode::Home,
            2 => KeyCode::Insert,
            3 => KeyCode::Delete,
            4 | 8 => KeyCode::End,
            5 => KeyCode::PageUp,
            6 => KeyCode::PageDown,
            n @ 11..=15 => KeyCode::F(n - 10),
            n @ 17..=21 => KeyCode::F(n - 11),
            n @ 23..=24 => KeyCode::F(n - 12),
            _ => return None,
        },
        (_, 'A') => KeyCode::Up,
        (_, 'B') => KeyCode::Down,
        (_, 'C') => KeyCode::Right,
        (_, 'D') => KeyCode::Left,
        (_, 'H') => KeyCode::Home,
        (_, 'F') => KeyCode::End,
        (_, 'Z') => return Some(KeyEvent::new(KeyCode::BackTab, modifiers | KeyModifiers::SHIFT)),
        (_, c @ 'P'..='S') => KeyCode::F(c as u8 - b'P' + 1),
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What crossterm makes of `bytes` once the Esc it reads first has taken the next byte too:
    /// the rest, byte by byte, as letters (the way its parser hands out `[21~`).
    fn rest_as_letters(bytes: &[u8]) -> Vec<Event> {
        assert_eq!(&bytes[..2], b"\x1b\x1b");
        bytes[2..].iter().map(|&b| Event::Key(KeyEvent::new(KeyCode::Char(b as char), KeyModifiers::NONE))).collect()
    }

    fn feed(bytes: &[u8]) -> Vec<Event> {
        let mut q = rest_as_letters(bytes).into_iter();
        after_esc(|| q.next())
    }

    #[test]
    fn esc_then_f10_in_one_read_is_f10() {
        assert_eq!(feed(b"\x1b\x1b[21~"), vec![Event::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE))]);
    }

    #[test]
    fn other_keys_and_modifiers() {
        let k = |c, m| vec![Event::Key(KeyEvent::new(c, m))];
        assert_eq!(feed(b"\x1b\x1bOP"), k(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(feed(b"\x1b\x1b[A"), k(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(feed(b"\x1b\x1b[19;2~"), k(KeyCode::F(8), KeyModifiers::SHIFT));
        assert_eq!(feed(b"\x1b\x1b[1;5C"), k(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(feed(b"\x1b\x1b[24~"), k(KeyCode::F(12), KeyModifiers::NONE));
        assert_eq!(feed(b"\x1b\x1b[3~"), k(KeyCode::Delete, KeyModifiers::NONE));
    }

    #[test]
    fn text_after_esc_stays_text() {
        // Esc, then letters that are no key: given back as they came.
        assert_eq!(feed(b"\x1b\x1bab"), rest_as_letters(b"\x1b\x1ba"));
        assert_eq!(feed(b"\x1b\x1b[x"), rest_as_letters(b"\x1b\x1b[x"));
        assert_eq!(feed(b"\x1b\x1b[2"), rest_as_letters(b"\x1b\x1b[2"));
        assert!(feed(b"\x1b\x1b").is_empty());
    }
}

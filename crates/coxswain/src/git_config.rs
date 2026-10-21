//! The git config, full screen (Alt+C, F9 → Git config): every key with the value git uses,
//! its scope and file, the values set earlier dimmed below it, secrets hidden until Enter.
//! Read once when it opens (coxswain-core's `git_config`); the desktop app shows the same.
//! Read-only: F4 opens a line's file in the editor, Alt+G the user's git tool.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::git_config::Entry;
use coxswain_core::t;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::collections::HashSet;
use std::path::PathBuf;
use crate::ui::Cols;

use crate::bom::faint;
use crate::ui::{fit, fit_left, sty};

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Close, and open this file in the editor.
    Edit(PathBuf),
    /// Put this line on the clipboard.
    Copy(String),
    /// Close, and open the repository in the user's git tool.
    Tool,
    /// Say this on the status line.
    Say(String),
}

enum Row {
    Heading(String),
    Entry(usize),
}

pub struct Viewer {
    title: String,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    /// The entries whose secret is shown.
    shown: HashSet<usize>,
    /// The row under the cursor: always an entry's, when there is one.
    cursor: usize,
    outside: bool,
    /// Say once how a hidden secret is shown (`menu::once`).
    hint: bool,
    /// The key line and the line about changing it, with the keys in.
    key_line: String,
    change_line: String,
}

impl Viewer {
    pub fn new(entries: Vec<Entry>, repo: Option<String>, labels: [&str; 3], hint: bool) -> Viewer {
        let mut rows = vec![];
        for (i, e) in entries.iter().enumerate() {
            if i == 0 || entries[i - 1].section != e.section {
                rows.push(Row::Heading(e.section.clone()));
            }
            rows.push(Row::Entry(i));
        }
        let cursor = rows
            .iter()
            .position(|r| matches!(r, Row::Entry(_)))
            .unwrap_or(0);
        let [edit, copy, tool] = labels;
        Viewer {
            title: match &repo {
                Some(r) => t!("gitconfig.title", "repo" => r),
                None => t!("gitconfig.title_outside"),
            },
            outside: repo.is_none(),
            hint,
            entries,
            rows,
            shown: HashSet::new(),
            cursor,
            key_line: t!("gitconfig.keys", "edit" => edit, "copy" => copy, "tool" => tool),
            change_line: t!("gitconfig.change", "key" => tool),
        }
    }

    fn current(&self) -> Option<usize> {
        match self.rows.get(self.cursor) {
            Some(Row::Entry(i)) => Some(*i),
            _ => None,
        }
    }

    /// The cursor `by` entries on (back when negative), skipping headings.
    fn step(&mut self, by: isize) {
        let entries: Vec<usize> = (0..self.rows.len())
            .filter(|&r| matches!(self.rows[r], Row::Entry(_)))
            .collect();
        let Some(at) = entries.iter().position(|&r| r == self.cursor) else {
            return;
        };
        let to = (at as isize + by).clamp(0, entries.len() as isize - 1) as usize;
        self.cursor = entries[to];
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            _ if action == Some(Action::Quit) => return Outcome::Close,
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::PageUp => self.step(-15),
            KeyCode::PageDown => self.step(15),
            KeyCode::Home => self.step(isize::MIN / 2),
            KeyCode::End => self.step(isize::MAX / 2),
            KeyCode::Enter => {
                if let Some(i) = self.current().filter(|&i| self.entries[i].hidden.is_some())
                    && !self.shown.remove(&i)
                {
                    self.shown.insert(i);
                }
            }
            _ if action == Some(Action::GitTool) => return Outcome::Tool,
            _ if action == Some(Action::Edit) => {
                if let Some(i) = self.current() {
                    let e = &self.entries[i];
                    return match &e.file {
                        Some(f) => Outcome::Edit(f.clone()),
                        None => Outcome::Say(t!("gitconfig.no_file", "origin" => &e.origin)),
                    };
                }
            }
            _ if action == Some(Action::Copy) => {
                if let Some(i) = self.current() {
                    return Outcome::Copy(self.entries[i].copy(self.shown.contains(&i)));
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// Where an entry comes from: `global · included  ~/.gitconfig-work`.
    fn origin(e: &Entry) -> (String, String) {
        let mut scope = t!(&format!("gitconfig.scope_{}", e.scope));
        if e.included {
            scope = format!("{scope} · {}", t!("gitconfig.included"));
        }
        if e.earlier {
            scope = format!("{scope} · {}", t!("gitconfig.earlier"));
        }
        let from = e
            .file
            .as_ref()
            .map_or_else(|| e.origin.clone(), |f| f.display().to_string());
        (scope, from)
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(bg))
            .style(base)
            .title(Line::from(format!(" {} ", self.title)).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area).inner(Margin::new(1, 0));
        f.render_widget(block, area);
        let w = inner.width as usize;

        let mut head = vec![Line::styled(fit(&t!("gitconfig.about"), w), dim)];
        if self.outside {
            head.push(Line::styled(fit(&t!("gitconfig.outside"), w), dim));
        }
        if self.hint {
            head.push(Line::styled(
                fit(&t!("hint.git_config_show", "key" => "Enter"), w),
                dim,
            ));
        }
        let [head_a, _, body, rule, change, foot] = Layout::vertical([
            Constraint::Length(head.len() as u16),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        f.render_widget(Paragraph::new(head), head_a);

        if self.entries.is_empty() {
            f.render_widget(Paragraph::new(t!("gitconfig.empty")).style(dim), body);
        }
        let name_w = self
            .entries
            .iter()
            .map(|e| e.name.width())
            .max()
            .unwrap_or(0)
            .min(24);
        let h = body.height as usize;
        let top = self.cursor.saturating_sub(h.saturating_sub(1));
        for (k, row) in self.rows.iter().enumerate().skip(top).take(h) {
            let y = body.y + (k - top) as u16;
            let at = Rect {
                y,
                height: 1,
                ..body
            };
            let i = match row {
                Row::Heading(s) => {
                    let bold = base.add_modifier(Modifier::BOLD);
                    f.render_widget(Paragraph::new(fit(&format!("[{s}]"), w)).style(bold), at);
                    continue;
                }
                Row::Entry(i) => *i,
            };
            let e = &self.entries[i];
            let value = if self.shown.contains(&i) {
                e.value.as_deref()
            } else {
                e.hidden.as_deref().or(e.value.as_deref())
            };
            let left = format!("  {} {}", fit(&e.name, name_w), value.unwrap_or("true"));
            let (scope, from) = Self::origin(e);
            let right_w = (w / 2).min(scope.width() + 2 + from.width());
            let from_w = right_w.saturating_sub(scope.width() + 2);
            let right = format!("{scope}  {}", fit_left(&from, from_w));
            let line = format!(
                "{} {right}",
                fit(&left, w.saturating_sub(right.width() + 1))
            );
            let style = if k == self.cursor {
                sty(&theme.dialog_input)
            } else if e.earlier {
                dim
            } else {
                base
            };
            f.render_widget(Paragraph::new(fit(&line, w)).style(style), at);
        }
        f.render_widget(
            Paragraph::new("─".repeat(w)).style(sty(&theme.dialog_border)),
            rule,
        );
        f.render_widget(Paragraph::new(fit(&self.change_line, w)).style(dim), change);
        f.render_widget(Paragraph::new(fit(&self.key_line, w)).style(dim), foot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn entry(key: &str, value: &str, hidden: Option<&str>, earlier: bool) -> Entry {
        let (section, name) = key.rsplit_once('.').unwrap();
        Entry {
            key: key.into(),
            value: Some(value.into()),
            hidden: hidden.map(Into::into),
            section: section.into(),
            name: name.into(),
            scope: "global".into(),
            file: Some("/h/.gitconfig".into()),
            origin: String::new(),
            included: false,
            earlier,
            line: format!("{key}={}", hidden.unwrap_or(value)),
        }
    }

    fn key(code: KeyCode) -> Key {
        Key {
            code,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    #[test]
    fn enter_shows_a_secret_and_a_copy_follows() {
        let v = vec![
            entry("sendemail.smtppass", "hush", Some("••••"), false),
            entry("user.name", "Ada", None, false),
        ];
        let mut v = Viewer::new(v, Some("rocket".into()), ["F4", "F5", "Alt+G"], true);
        assert!(
            matches!(v.key(key(KeyCode::F(5)), Some(Action::Copy)), Outcome::Copy(l) if l == "sendemail.smtppass=••••")
        );
        v.key(key(KeyCode::Enter), None);
        assert!(
            matches!(v.key(key(KeyCode::F(5)), Some(Action::Copy)), Outcome::Copy(l) if l == "sendemail.smtppass=hush")
        );
        v.key(key(KeyCode::Down), None);
        assert!(
            matches!(v.key(key(KeyCode::F(4)), Some(Action::Edit)), Outcome::Edit(p) if p == Path::new("/h/.gitconfig"))
        );
        assert!(matches!(
            v.key(key(KeyCode::Char('g')), Some(Action::GitTool)),
            Outcome::Tool
        ));
        // Shown with Enter above, so drawn in full.
        let backend = ratatui::backend::TestBackend::new(80, 20);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            screen.contains("[sendemail]") && screen.contains("Ada") && screen.contains("hush")
        );
    }
}

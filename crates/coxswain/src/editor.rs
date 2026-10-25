//! The built-in editor (Shift+F4, and F4 when no editor is set): a text file full screen,
//! highlighted as the viewer highlights it, with the cursor, a selection, undo, find and
//! replace, and go to line. Reading, saving and the text itself are `coxswain_core::edit`'s;
//! this draws it and takes the keys.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::edit::{Buffer, Doc, Encoding, SaveError, Stamp};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use syntect::parsing::SyntaxReference;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::bom::faint;
use crate::ui::{fit, sty};
use crate::viewer::{Pieces, Point, Slots, paint, syntax_for};

/// Highlighting keeps where it stands every this many lines, to start from there again.
const STEP: usize = 64;
/// Further than this past the last point kept, highlighting starts afresh a little above.
const FARTHEST: usize = 4000;
/// Columns a tab takes.
const TAB: usize = 4;

/// What the editor wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Put this text on the clipboard (Ctrl+C, Ctrl+X).
    Copy(String),
}

/// A line typed at the bottom.
enum Typing {
    Find(String),
    /// Replace: what to find, then (`Some`) what with.
    Replace(String, Option<String>),
    Line(String),
}

/// A question waiting for its key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Asking {
    /// Close with changes not saved: save, throw away, or go on editing.
    Close,
    /// The file changed on the disk since it was opened: save over it, read it again, or not.
    Changed,
}

pub struct Editor {
    pub path: PathBuf,
    title: String,
    pub buf: Buffer,
    encoding: Encoding,
    crlf: bool,
    stamp: Stamp,
    syntax: Option<&'static SyntaxReference>,
    /// Points highlighting has passed, every `STEP` lines from the start.
    points: Vec<Point>,
    slots: Slots,
    top: usize,
    /// The first column shown, when lines are wider than the screen.
    left: usize,
    typing: Option<Typing>,
    asking: Option<Asking>,
    /// The last search.
    query: String,
    said: Option<String>,
    /// What Ctrl+C or Ctrl+X took, for Ctrl+V (the terminal's own paste comes as a paste).
    clip: String,
    /// Rows of text on the screen, as last drawn.
    page: usize,
    key_line: String,
    /// Who changed each line, and a function's history (`Blame`, `LineHistory`), in a repository.
    git: Option<crate::blame::Git>,
    /// A function's history, shown over the text until Esc.
    sub: Option<Box<crate::viewer::Viewer>>,
}

impl Editor {
    pub fn open(path: &Path) -> Result<Editor, String> {
        let Doc { text, encoding, crlf, stamp } = coxswain_core::edit::open(path)?;
        let head: String = text.chars().take(8192).collect();
        let syntax = syntax_for(path, &head);
        let git = path.parent().is_some_and(coxswain_core::blame::in_repository);
        Ok(Editor {
            title: path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
            path: path.to_path_buf(),
            buf: Buffer::new(&text),
            encoding,
            crlf,
            stamp,
            points: syntax.map(|s| vec![Point::new(s)]).unwrap_or_default(),
            syntax,
            slots: HashMap::new(),
            top: 0,
            left: 0,
            typing: None,
            asking: None,
            query: String::new(),
            said: None,
            clip: String::new(),
            page: 20,
            key_line: t!("editor.keys"),
            git: git.then(Default::default),
            sub: None,
        })
    }

    /// The keys of blame and a function's history on the key line, for a file in a repository.
    pub fn git_keys(mut self, blame: &str, history: &str) -> Editor {
        if self.git.is_some() {
            self.key_line = format!("{} · {}", self.key_line, t!("viewer.keys_git", "blame" => blame, "history" => history));
        }
        self
    }

    /// Text the terminal pasted: put in as it is, with no indent added.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        match &mut self.typing {
            Some(Typing::Find(s) | Typing::Line(s) | Typing::Replace(s, None) | Typing::Replace(_, Some(s))) => s.push_str(text.lines().next().unwrap_or("")),
            None if self.asking.is_none() => self.buf.insert(&text),
            None => {}
        }
        self.touched();
    }

    /// Scroll by `by` lines (the mouse wheel), the cursor kept on the screen.
    pub fn scroll(&mut self, by: isize) {
        let n = self.buf.lines.len();
        self.top = (self.top as isize + by).clamp(0, n.saturating_sub(1) as isize) as usize;
        let line = self.buf.cursor.line.clamp(self.top, (self.top + self.page).saturating_sub(1).min(n - 1));
        if line != self.buf.cursor.line {
            self.buf.vertical(line as isize - self.buf.cursor.line as isize, false);
        }
    }

    /// Highlighting from where the text changed is no longer known.
    fn touched(&mut self) {
        if let Some(line) = self.buf.take_touched() {
            self.points.truncate(line / STEP + 1);
            // The lines moved: who changed them is no longer known.
            if let Some(g) = self.git.as_mut().filter(|g| g.blame.is_some()) {
                g.blame = None;
                self.said = Some(t!("blame.edited"));
            }
        }
    }

    /// Whether blame or a history is being read, so the screen is drawn again soon.
    pub fn busy(&self) -> bool {
        self.git.as_ref().is_some_and(crate::blame::Git::busy) || self.sub.as_ref().is_some_and(|v| v.busy())
    }

    /// What came of blame or a history.
    pub fn poll(&mut self) {
        if let Some(v) = &mut self.sub {
            v.poll();
        }
        match self.git.as_mut().map(crate::blame::Git::poll) {
            Some(crate::blame::Came::Said(s)) => self.said = Some(s),
            Some(crate::blame::Came::History(h)) => {
                let title = crate::blame::title(&h, &self.path);
                self.sub = Some(Box::new(crate::viewer::Viewer::text(title, &h.text())));
            }
            _ => {}
        }
    }

    /// Save; `force` over a file changed on the disk since.
    fn save(&mut self, force: bool) -> bool {
        let text = self.buf.text();
        match coxswain_core::edit::save(&self.path, &text, self.encoding, self.crlf, (!force).then_some(&self.stamp)) {
            Ok(stamp) => {
                self.stamp = stamp;
                self.buf.saved();
                self.said = Some(t!("editor.saved", "name" => &self.title));
                true
            }
            Err(SaveError::Changed) => {
                self.asking = Some(Asking::Changed);
                false
            }
            Err(e) => {
                self.said = Some(t!("editor.not_saved", "error" => e));
                false
            }
        }
    }

    /// The file read again from the disk, the changes here let go.
    fn reload(&mut self) {
        match Editor::open(&self.path) {
            Ok(mut fresh) => {
                fresh.top = self.top.min(fresh.buf.lines.len().saturating_sub(1));
                fresh.query = std::mem::take(&mut self.query);
                fresh.clip = std::mem::take(&mut self.clip);
                fresh.said = Some(t!("editor.reloaded"));
                *self = fresh;
            }
            Err(e) => self.said = Some(e),
        }
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        let out = self.key_inner(key, action);
        self.touched();
        out
    }

    fn key_inner(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if let Some(v) = &mut self.sub {
            if let crate::viewer::Outcome::Close = v.key(key, action) {
                self.sub = None;
            }
            return Outcome::Stay;
        }
        // A letter as typed: Shift made ASCII letters lower case with `shift` (`Key::new`).
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt || key.ctrl && key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        let ctrl = |c: char| key.ctrl && !key.alt && key.code == KeyCode::Char(c);
        if let Some(asking) = self.asking {
            self.asking = None;
            match (asking, ch) {
                (Asking::Close, Some('y' | 'Y')) => {
                    if self.save(false) {
                        return Outcome::Close;
                    }
                }
                (Asking::Close, Some('n' | 'N')) => return Outcome::Close,
                (Asking::Changed, Some('o' | 'O')) => {
                    self.save(true);
                }
                (Asking::Changed, Some('r' | 'R')) => self.reload(),
                _ if key.code == KeyCode::Esc => {}
                _ => self.asking = Some(asking),
            }
            return Outcome::Stay;
        }
        if let Some(typing) = &mut self.typing {
            let field = match typing {
                Typing::Find(s) | Typing::Line(s) | Typing::Replace(s, None) | Typing::Replace(_, Some(s)) => s,
            };
            match key.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Backspace => drop(field.pop()),
                KeyCode::Enter => match self.typing.take() {
                    Some(Typing::Find(s)) => {
                        self.query = s;
                        self.find(false);
                    }
                    Some(Typing::Replace(s, None)) => self.typing = Some(Typing::Replace(s, Some(String::new()))),
                    Some(Typing::Replace(q, Some(with))) => {
                        let n = self.buf.replace_all(&q, &with);
                        self.said = Some(if n == 0 { t!("viewer.not_found", "text" => &q) } else { tn!("editor.replaced", n) });
                        self.query = q;
                    }
                    Some(Typing::Line(s)) => {
                        if let Ok(n) = s.trim().parse::<usize>() {
                            self.buf.go_to(n);
                            self.top = self.buf.cursor.line.saturating_sub(self.page / 3);
                        }
                    }
                    None => {}
                },
                _ if ch.is_some() => field.extend(ch),
                _ => {}
            }
            return Outcome::Stay;
        }
        self.said = None;
        if key.code == KeyCode::Esc && self.git.as_mut().is_some_and(crate::blame::Git::stop) {
            self.said = Some(t!("blame.stopped"));
            return Outcome::Stay;
        }
        let page = self.page.max(1) as isize;
        let sel = key.shift;
        match key.code {
            _ if matches!(action, Some(Action::Blame | Action::LineHistory)) && self.git.is_none() => self.said = Some(t!("blame.not_here")),
            _ if action == Some(Action::Blame) && self.buf.dirty() && !self.git.as_ref().is_some_and(|g| g.blame.is_some()) => self.said = Some(t!("blame.save_first")),
            _ if action == Some(Action::Blame) => {
                let path = self.path.clone();
                self.said = self.git.as_mut().map(|g| g.toggle_blame(&path));
            }
            // The function the cursor is in, or the lines picked.
            _ if action == Some(Action::LineHistory) => {
                let (a, b) = self.buf.selection().map_or((self.buf.cursor.line, self.buf.cursor.line), |(a, b)| (a.line, if b.col == 0 && b.line > a.line { b.line - 1 } else { b.line }));
                let (path, text) = (self.path.clone(), self.buf.text());
                self.said = self.git.as_mut().map(|g| g.trace(&path, text, a, b));
            }
            _ if key.code == KeyCode::Esc || action == Some(Action::Quit) => {
                if !self.buf.dirty() {
                    return Outcome::Close;
                }
                self.asking = Some(Asking::Close);
            }
            _ if ctrl('s') || key.code == KeyCode::F(2) && !key.ctrl && !key.alt => {
                self.save(false);
            }
            _ if ctrl('z') && !key.shift => {
                if !self.buf.undo() {
                    self.said = Some(t!("editor.nothing_to_undo"));
                }
            }
            _ if ctrl('y') || ctrl('z') && key.shift => {
                if !self.buf.redo() {
                    self.said = Some(t!("editor.nothing_to_redo"));
                }
            }
            _ if ctrl('a') => self.buf.select_all(),
            _ if ctrl('c') => return self.buf.selected().map_or(Outcome::Stay, |s| self.copied(s)),
            _ if ctrl('x') => return self.buf.cut().map_or(Outcome::Stay, |s| self.copied(s)),
            _ if ctrl('v') => {
                let clip = self.clip.clone();
                if !clip.is_empty() {
                    self.buf.insert(&clip);
                }
            }
            _ if ctrl('f') || key.code == KeyCode::F(7) && !key.shift => self.typing = Some(Typing::Find(String::new())),
            KeyCode::F(7) => self.find(false),
            _ if ctrl('r') => self.typing = Some(Typing::Replace(String::new(), None)),
            _ if ctrl('g') => self.typing = Some(Typing::Line(String::new())),
            KeyCode::Left if key.ctrl => self.buf.word(false, sel),
            KeyCode::Right if key.ctrl => self.buf.word(true, sel),
            KeyCode::Left => self.buf.left(sel),
            KeyCode::Right => self.buf.right(sel),
            KeyCode::Up => self.buf.vertical(-1, sel),
            KeyCode::Down => self.buf.vertical(1, sel),
            KeyCode::PageUp => {
                self.buf.vertical(-page, sel);
                self.top = self.top.saturating_sub(page as usize);
            }
            KeyCode::PageDown => {
                self.buf.vertical(page, sel);
                self.top = (self.top + page as usize).min(self.buf.lines.len().saturating_sub(1));
            }
            KeyCode::Home if key.ctrl => self.buf.edge(false, sel),
            KeyCode::End if key.ctrl => self.buf.edge(true, sel),
            KeyCode::Home => self.buf.home(sel),
            KeyCode::End => self.buf.end(sel),
            KeyCode::Enter => self.buf.newline(),
            KeyCode::Tab if !key.shift => {
                let indent = self.buf.indent.clone();
                self.buf.insert(&indent);
            }
            KeyCode::Backspace => self.buf.backspace(),
            KeyCode::Delete => self.buf.delete(),
            _ => {
                if let Some(c) = ch {
                    self.buf.insert(&c.to_string());
                }
            }
        }
        Outcome::Stay
    }

    fn copied(&mut self, s: String) -> Outcome {
        self.clip = s.clone();
        Outcome::Copy(s)
    }

    /// The next hit of the last search (or the one before, `back`), selected and on the screen.
    fn find(&mut self, back: bool) {
        if self.query.is_empty() {
            self.typing = Some(Typing::Find(String::new()));
        } else if !self.buf.find(&self.query, back) {
            self.said = Some(t!("viewer.not_found", "text" => &self.query));
        }
    }

    /// Lines `a..a+n` in pieces, each with the theme slot it is coloured with.
    fn highlighted(&mut self, a: usize, n: usize) -> Vec<Pieces> {
        let end = (a + n).min(self.buf.lines.len());
        let Some(syntax) = self.syntax else {
            return self.buf.lines[a.min(end)..end].iter().map(|l| vec![(None, l.clone())]).collect();
        };
        let kept = (a / STEP).min(self.points.len() - 1);
        let (mut at, mut point, exact) =
            if a - kept * STEP <= FARTHEST { (kept * STEP, self.points[kept].clone(), true) } else { (a.saturating_sub(STEP), Point::new(syntax), false) };
        let mut out = vec![];
        while at < end {
            if exact && at % STEP == 0 && at / STEP == self.points.len() {
                self.points.push(point.clone());
            }
            let pieces = paint(&mut point, &self.buf.lines[at], &mut self.slots);
            if at >= a {
                out.push(pieces);
            }
            at += 1;
        }
        out
    }

    /// The cursor's column on the screen, in line `line` up to byte `col`, tabs counted out.
    fn column(line: &str, col: usize) -> usize {
        let mut w = 0;
        for c in line[..col].chars() {
            w += match c {
                '\t' => TAB - w % TAB,
                c if c.is_control() => 1,
                c => c.width().unwrap_or(0),
            };
        }
        w
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        if let Some(v) = &mut self.sub {
            return v.draw(f, theme);
        }
        let area = f.area();
        let base = sty(&theme.panel);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let count = self.buf.lines.len();
        let grammar = self.syntax.map_or_else(|| t!("viewer.plain"), |s| s.name.clone());
        let eol = if self.crlf { "CRLF" } else { "LF" };
        let changed = if self.buf.dirty() { format!(" · {}", t!("editor.modified")) } else { String::new() };
        let title = format!(" {} · {grammar} · {} · {eol} · {}{changed} ", self.title, self.encoding.name(), tn!("viewer.lines", count));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.border).bg(bg))
            .style(base)
            .title(Line::from(title).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let foot_h = if self.key_line.width() > inner.width as usize { 2 } else { 1 };
        let [body, info, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(foot_h)]).areas(inner);
        let h = (body.height as usize).max(1);
        self.page = h;
        // A space either side of the line numbers, so they never touch the frame.
        let margin = if self.git.as_ref().is_some_and(|g| g.blame.is_some()) { crate::blame::WIDTH } else { 0 };
        let gutter = count.to_string().len() + 2 + margin;
        let w = (body.width as usize).saturating_sub(gutter).max(1);
        // The cursor on the screen: the text scrolled to it.
        let cur = self.buf.cursor;
        if cur.line < self.top {
            self.top = cur.line;
        } else if cur.line >= self.top + h {
            self.top = cur.line + 1 - h;
        }
        // No empty screen under the last line while there are lines above to show.
        self.top = self.top.min(count.saturating_sub(h));
        let cx = Editor::column(&self.buf.lines[cur.line], cur.col);
        if cx < self.left {
            self.left = cx.saturating_sub(8);
        } else if cx >= self.left + w {
            self.left = (cx + 8).saturating_sub(w).min(cx);
        }
        let lines = self.highlighted(self.top, h);
        let slots: HashMap<&str, Style> = theme.slots().into_iter().map(|(n, s)| (n, sty(s))).collect();
        let token = |slot: Option<&str>| match slot.and_then(|s| slots.get(s)).and_then(|s| s.fg) {
            Some(fg) if slot == Some("hidden") => base.fg(fg).add_modifier(Modifier::ITALIC),
            Some(fg) => base.fg(fg),
            None => base,
        };
        let marked = sty(&theme.cursor);
        let selection = self.buf.selection();
        let mut rows: Vec<Line> = vec![];
        for (k, pieces) in lines.into_iter().enumerate() {
            let no = self.top + k;
            let mut cells: Vec<(char, Style)> = vec![];
            let mut at = 0;
            for (slot, s) in &pieces {
                for c in s.chars() {
                    let picked = selection.is_some_and(|(a, b)| (no, at) >= (a.line, a.col) && (no, at) < (b.line, b.col));
                    let style = if picked { marked } else { token(*slot) };
                    at += c.len_utf8();
                    match c {
                        '\t' => {
                            let n = TAB - cells.len() % TAB;
                            cells.extend(std::iter::repeat_n((' ', style), n));
                        }
                        c if c.is_control() => cells.push(('·', style)),
                        c => {
                            cells.push((c, style));
                            // A wide character takes two columns: the second stays empty.
                            if c.width() == Some(2) {
                                cells.push(('\0', style));
                            }
                        }
                    }
                }
            }
            // The line break of a selected line shows as one marked cell.
            if selection.is_some_and(|(a, b)| (no, at) >= (a.line, a.col) && no < b.line) {
                cells.push((' ', marked));
            }
            let shown = cells.iter().skip(self.left).take(w).filter(|(c, _)| *c != '\0');
            let mut spans = vec![];
            if let Some(m) = self.git.as_ref().and_then(|g| g.margin(no)) {
                spans.push(Span::styled(m, dim));
            }
            spans.push(Span::styled(format!(" {:>width$} ", no + 1, width = gutter - 2 - margin), if no == cur.line { base } else { dim }));
            spans.extend(shown.map(|(c, s)| Span::styled(c.to_string(), *s)));
            rows.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(rows).style(base), body);
        let info_text = match (&self.typing, self.asking) {
            (_, Some(Asking::Close)) => t!("editor.ask_close", "name" => &self.title),
            (_, Some(Asking::Changed)) => t!("editor.ask_changed", "name" => &self.title),
            (Some(Typing::Find(s)), _) => format!("{}: {s}▏", t!("viewer.search")),
            (Some(Typing::Replace(s, None)), _) => format!("{}: {s}▏", t!("editor.replace")),
            (Some(Typing::Replace(q, Some(s))), _) => format!("{} {q} {}: {s}▏", t!("editor.replace"), t!("editor.with")),
            (Some(Typing::Line(s)), _) => format!("{}: {s}▏", t!("viewer.go_to_line")),
            (None, None) => self.said.clone().unwrap_or_else(|| t!("editor.at", "line" => cur.line + 1, "col" => cx + 1)),
        };
        f.render_widget(Paragraph::new(fit(&info_text, inner.width as usize)).style(sty(&theme.status)), info);
        f.render_widget(Paragraph::new(self.key_line.as_str()).wrap(ratatui::widgets::Wrap { trim: true }).style(dim), foot);
        if self.typing.is_none() && self.asking.is_none() {
            let x = body.x + gutter as u16 + (cx - self.left) as u16;
            f.set_cursor_position(Position::new(x.min(body.right().saturating_sub(1)), body.y + (cur.line - self.top) as u16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key::new(code, false, false, false)
    }

    fn ctrl(c: char) -> Key {
        Key::new(KeyCode::Char(c), true, false, false)
    }

    fn typed(e: &mut Editor, s: &str) {
        for c in s.chars() {
            e.key(Key::new(KeyCode::Char(c), false, false, false), None);
        }
    }

    fn screen(e: &mut Editor, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| e.draw(f, &Theme::default())).unwrap();
        term.backend().buffer().content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn typing_saving_and_closing() {
        let d = std::env::temp_dir().join(format!("coxswain-editor-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("engine.rs");
        std::fs::write(&p, "fn main() {\r\n    burn();\r\n}\r\n").unwrap();
        let mut e = Editor::open(&p).unwrap();
        assert_eq!(e.syntax.map(|s| s.name.as_str()), Some("Rust"));
        e.key(key(KeyCode::Down), None);
        e.key(key(KeyCode::End), None);
        e.key(key(KeyCode::Enter), None);
        // Shift+letters come lower case with shift: typed upper case.
        typed(&mut e, "coast");
        e.key(Key::new(KeyCode::Char('x'), false, false, true), None);
        typed(&mut e, "();");
        let s = screen(&mut e, 60, 8);
        assert!(s.contains("3     coastX();"), "{s}");
        assert!(s.contains("modified"), "{s}");
        // Esc with changes asks; Esc again goes back to editing; n throws them away.
        assert!(matches!(e.key(key(KeyCode::Esc), None), Outcome::Stay));
        assert!(screen(&mut e, 80, 8).contains("engine.rs"));
        e.key(key(KeyCode::Esc), None);
        assert!(e.asking.is_none());
        // Ctrl+S saves with the file's line endings.
        e.key(ctrl('s'), None);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "fn main() {\r\n    burn();\r\n    coastX();\r\n}\r\n");
        assert!(matches!(e.key(key(KeyCode::Esc), None), Outcome::Close), "saved: closes at once");
        // Changed elsewhere since: asked, and o saves over it.
        let mut e = Editor::open(&p).unwrap();
        std::fs::write(&p, "changed\n").unwrap();
        typed(&mut e, "// ");
        e.key(ctrl('s'), None);
        assert_eq!(e.asking, Some(Asking::Changed));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "changed\n");
        e.key(key(KeyCode::Char('o')), None);
        assert!(std::fs::read_to_string(&p).unwrap().starts_with("// fn main()"));
        // Find, replace, go to line, and copy and paste.
        e.key(ctrl('r'), None);
        typed(&mut e, "burn");
        e.key(key(KeyCode::Enter), None);
        typed(&mut e, "fire");
        e.key(key(KeyCode::Enter), None);
        assert_eq!(e.buf.lines[1], "    fire();");
        e.key(ctrl('g'), None);
        typed(&mut e, "3");
        e.key(key(KeyCode::Enter), None);
        assert_eq!(e.buf.cursor.line, 2);
        e.key(key(KeyCode::F(7)), None);
        typed(&mut e, "FIRE");
        e.key(key(KeyCode::Enter), None);
        assert!(matches!(e.key(ctrl('c'), None), Outcome::Copy(s) if s == "fire"));
        e.key(Key::new(KeyCode::End, true, false, false), None);
        e.key(ctrl('v'), None);
        assert_eq!(e.buf.lines.last().map(String::as_str), Some("fire"));
        e.paste("a\r\n    b");
        assert_eq!(e.buf.lines[e.buf.lines.len() - 2..], ["firea", "    b"], "pasted as it is");
        e.key(ctrl('z'), None);
        assert_eq!(e.buf.lines.last().map(String::as_str), Some("fire"));
        // y saves and closes.
        e.key(key(KeyCode::Esc), None);
        assert!(matches!(e.key(key(KeyCode::Char('y')), None), Outcome::Close));
        assert!(std::fs::read_to_string(&p).unwrap().ends_with("}\r\nfire"));
        // Not text: refused.
        std::fs::write(d.join("a.bin"), b"\0\x01").unwrap();
        assert!(Editor::open(&d.join("a.bin")).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn wide_text_scrolls_to_the_cursor_and_selections_are_marked() {
        let mut e = Editor::open_text("x".repeat(200) + "\nsecond");
        e.key(key(KeyCode::End), None);
        let s = screen(&mut e, 40, 10);
        assert!(!s.contains("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx "), "scrolled sideways: {s}");
        e.key(Key::new(KeyCode::Home, true, false, false), None);
        e.key(Key::new(KeyCode::Down, false, false, true), None);
        assert_eq!(e.buf.selected().map(|s| s.len()), Some(201));
        let s = screen(&mut e, 40, 10);
        assert!(s.contains("1 xxxx") && s.contains("2 second"), "{s}");
    }

    impl Editor {
        /// An editor on a file of `text` that is never saved.
        fn open_text(text: String) -> Editor {
            let p = std::env::temp_dir().join(format!("coxswain-editor-text-{}.txt", std::process::id()));
            std::fs::write(&p, text).unwrap();
            let e = Editor::open(&p).unwrap();
            std::fs::remove_file(&p).unwrap();
            e
        }
    }
}

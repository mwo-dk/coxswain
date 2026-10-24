//! Compare files (Shift+F2), full screen: two texts side by side at 120 columns or more, else one
//! under the other; a binary file as hex rows; pictures by their facts. What is compared and
//! how is `coxswain_core::compare`'s; this draws it, a screen at a time, and takes the keys.

use coxswain_core::compare::text::{Cell, Options, Row, Tag, View};
use coxswain_core::compare::{Done, Kind, Session, bytes};
use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::edit::SaveError;
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use unicode_width::UnicodeWidthChar;

use crate::bom::faint;
use crate::ui::{fit, sty};
use crate::viewer::{Point, Slots, paint, syntax_for};

/// Columns from which two texts are shown side by side.
const SPLIT_FROM: u16 = 120;

/// Columns a hex row takes with both sides on one line: the offset, then 16 bytes and a bar
/// a side.
const HEX_ROW: u16 = 10 + 2 * (bytes::WIDTH as u16 * 3 + 2);

/// Whether hex rows put the right side under the left: two sides do not fit beside each other.
fn hex_stacked(width: u16) -> bool {
    width < HEX_ROW
}

/// The next difference in a binary compare, looked for on a thread: what it finds, its stop
/// (Esc), and which way it looks.
struct Seeking {
    found: Receiver<Option<u64>>,
    stop: Arc<AtomicBool>,
    back: bool,
}

pub enum Outcome {
    Stay,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Asking {
    /// Close with changes not saved.
    Close,
    /// A side changed on the disk since it was read.
    Changed,
}

pub struct Viewer {
    s: Session,
    /// Side by side or not; `None`: by the width.
    view: Option<View>,
    /// The row under the cursor, and the first in sight.
    cur: usize,
    top: usize,
    left: usize,
    page: usize,
    split: bool,
    said: Option<String>,
    asking: Option<Asking>,
    names: [String; 2],
    slots: Slots,
    /// The cursor was put on the first change, once the compare was ready.
    placed: bool,
    seeking: Option<Seeking>,
}

impl Drop for Viewer {
    /// Closed while looking for the next difference: the looking stops too.
    fn drop(&mut self) {
        if let Some(s) = &self.seeking {
            s.stop.store(true, Ordering::Relaxed);
        }
    }
}

/// A file's name as it can be shown: a control character in it (an escape) would talk to the
/// terminal.
fn name(p: &Path) -> String {
    let n = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    crate::ui::printable(&n).into_owned()
}

impl Viewer {
    pub fn open(left: &Path, right: &Path) -> Viewer {
        Viewer {
            s: Session::open(left, right, Options::default()),
            view: None,
            cur: 0,
            top: 0,
            left: 0,
            page: 20,
            split: true,
            said: None,
            asking: None,
            names: [name(left), name(right)],
            slots: HashMap::new(),
            placed: false,
            seeking: None,
        }
    }

    /// Looking for the next difference: the screen is drawn more often until it is found.
    pub fn busy(&self) -> bool {
        self.seeking.is_some()
    }

    /// The next difference of a binary compare, once its thread has found it (or that there is
    /// none).
    fn poll(&mut self) {
        let Some(s) = &self.seeking else { return };
        let found = match s.found.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let back = s.back;
        self.seeking = None;
        match found {
            Some(r) => {
                self.cur = r as usize;
                self.top = self.cur.saturating_sub(self.page / 3);
                self.said = None;
            }
            None => self.said = Some(t!(if back { "compare.no_earlier" } else { "compare.no_later" })),
        }
    }

    fn view(&self) -> View {
        self.view.unwrap_or(if self.split { View::Split } else { View::Inline })
    }

    fn total(&self) -> usize {
        match &*self.s.done() {
            Some(Done::Text(c)) => c.total(self.view()),
            Some(Done::Bytes(b)) => b.rows_total() as usize,
            _ => 0,
        }
    }

    pub fn scroll(&mut self, by: isize) {
        self.cur = (self.cur as isize + by).clamp(0, self.total().saturating_sub(1) as isize) as usize;
    }

    fn dirty(&self) -> bool {
        matches!(&*self.s.done(), Some(Done::Text(c)) if c.summary().dirty.iter().any(|&d| d))
    }

    /// Save; `force` over a side changed on the disk since.
    fn save(&mut self, force: bool) -> bool {
        let mut done = self.s.done();
        let Some(Done::Text(c)) = &mut *done else { return false };
        if !c.summary().dirty.iter().any(|&d| d) {
            self.said = Some(t!("compare.nothing_to_save"));
            return false;
        }
        let r = c.save(force);
        drop(done);
        match r {
            Ok(()) => {
                // Read again: where the lines of a side written start is known no more.
                let (cur, top) = (self.cur, self.top);
                self.s = self.s.again(self.s.options);
                (self.cur, self.top, self.placed) = (cur, top, true);
                self.said = Some(t!("compare.saved"));
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

    /// Compared again with these options.
    fn again(&mut self, options: Options) {
        if self.dirty() {
            return self.said = Some(t!("compare.save_first"));
        }
        self.s = self.s.again(options);
        self.cur = 0;
        self.top = 0;
        self.placed = false;
    }

    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        let ctrl = |c: char| key.ctrl && !key.alt && key.code == KeyCode::Char(c);
        if let Some(asking) = self.asking.take() {
            match (asking, ch) {
                (Asking::Close, Some('y' | 'Y')) if self.save(false) => return Outcome::Close,
                (Asking::Close, Some('n' | 'N')) => return Outcome::Close,
                (Asking::Changed, Some('o' | 'O')) => {
                    self.save(true);
                }
                _ if key.code == KeyCode::Esc => {}
                _ if self.asking.is_none() => self.asking = Some(asking),
                _ => {}
            }
            return Outcome::Stay;
        }
        // Esc stops looking for the next difference, before it closes anything.
        if let Some(s) = self.seeking.take() {
            if key.code == KeyCode::Esc {
                s.stop.store(true, Ordering::Relaxed);
                self.said = Some(t!("compare.stopped"));
                return Outcome::Stay;
            }
            self.seeking = Some(s);
        }
        self.said = None;
        let page = self.page.max(1) as isize;
        let view = self.view();
        let kind = self.s.kind;
        match key.code {
            _ if key.code == KeyCode::Esc || action == Some(Action::Quit) => {
                if !self.dirty() {
                    return Outcome::Close;
                }
                self.asking = Some(Asking::Close);
            }
            KeyCode::Up => self.scroll(-1),
            KeyCode::Down => self.scroll(1),
            KeyCode::PageUp => self.scroll(-page),
            KeyCode::PageDown => self.scroll(page),
            KeyCode::Home => self.cur = 0,
            KeyCode::End => self.cur = self.total().saturating_sub(1),
            KeyCode::Left if !key.alt => self.left = self.left.saturating_sub(8),
            KeyCode::Right if !key.alt => self.left += 8,
            _ if ch == Some('n') || ch == Some('p') || ch == Some('N') => {
                let back = ch != Some('n');
                let next = match &*self.s.done() {
                    Some(Done::Text(c)) => c.next(view, self.cur, back),
                    // Read a megabyte at a time, maybe to the end of a big file: on a thread,
                    // picked up as the screen is drawn, Esc stopping it.
                    Some(Done::Bytes(b)) => {
                        if self.seeking.is_none() {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let (b, row, stop) = (b.clone(), self.cur as u64, Arc::new(AtomicBool::new(false)));
                            self.seeking = Some(Seeking { found: rx, stop: stop.clone(), back });
                            std::thread::spawn(move || {
                                let _ = tx.send(b.next(row, back, &stop).ok().flatten());
                            });
                        }
                        return Outcome::Stay;
                    }
                    _ => None,
                };
                match next {
                    Some(r) => {
                        self.cur = r;
                        self.top = r.saturating_sub(self.page / 3);
                    }
                    None => self.said = Some(t!(if back { "compare.no_earlier" } else { "compare.no_later" })),
                }
            }
            _ if ch == Some('v') && matches!(kind, Kind::Text | Kind::Document) => {
                self.view = Some(if view == View::Split { View::Inline } else { View::Split });
                self.cur = 0;
                self.top = 0;
            }
            _ if ch == Some('w') => {
                let o = Options { whitespace: !self.s.options.whitespace, ..self.s.options };
                self.again(o);
            }
            _ if ch == Some('c') => {
                let o = Options { case: !self.s.options.case, ..self.s.options };
                self.again(o);
            }
            _ if ch == Some('>') || ch == Some('<') || key.alt && matches!(key.code, KeyCode::Right | KeyCode::Left) => {
                let from = usize::from(ch == Some('<') || key.code == KeyCode::Left);
                let mut done = self.s.done();
                if let Some(Done::Text(c)) = &mut *done {
                    let said = match c.hunk_at(view, self.cur) {
                        None => t!("compare.not_a_change"),
                        Some(h) => c.take(h, from).map_or_else(|why| why, |()| t!("compare.copied")),
                    };
                    self.said = Some(said);
                }
            }
            _ if ctrl('z') => {
                let mut done = self.s.done();
                if let Some(Done::Text(c)) = &mut *done {
                    let r = c.undo(view);
                    match r {
                        Some(r) => self.cur = r,
                        None => self.said = Some(t!("editor.nothing_to_undo")),
                    }
                }
            }
            _ if ctrl('s') || key.code == KeyCode::F(2) => {
                self.save(false);
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        self.poll();
        let area = f.area();
        let base = sty(&theme.panel);
        let bg = base.bg.unwrap_or(Color::Reset);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.border).bg(bg))
            .style(base)
            .title(Line::from(format!(" {} ↔ {} ", self.names[0], self.names[1])).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let keys = t!(match self.s.kind {
            Kind::Binary => "compare.keys_binary",
            Kind::Picture => "compare.keys_picture",
            Kind::Text | Kind::Document => "compare.keys",
        });
        let foot_h = if unicode_width::UnicodeWidthStr::width(keys.as_str()) > inner.width as usize { 2 } else { 1 };
        let [head, body, info, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(foot_h)]).areas(inner);
        self.split = area.width >= SPLIT_FROM;
        self.page = body.height as usize;
        let dim = faint(theme).bg(bg);
        let (summary, lines) = self.body(theme, body.width, body.height as usize);
        f.render_widget(Paragraph::new(fit(&summary, inner.width as usize)).style(sty(&theme.header)), head);
        f.render_widget(Paragraph::new(lines).style(base), body);
        let info_text = match self.asking {
            Some(Asking::Close) => t!("compare.ask_close"),
            Some(Asking::Changed) => t!("compare.ask_changed"),
            None if self.seeking.is_some() => t!("compare.seeking"),
            None => self.said.clone().unwrap_or_default(),
        };
        f.render_widget(Paragraph::new(fit(&info_text, inner.width as usize)).style(sty(&theme.status)), info);
        f.render_widget(Paragraph::new(keys).wrap(ratatui::widgets::Wrap { trim: true }).style(dim), foot);
    }

    /// The summary line and the rows in sight.
    fn body(&mut self, theme: &Theme, width: u16, h: usize) -> (String, Vec<Line<'static>>) {
        let total = self.total();
        // Hex rows of two lines each where the sides do not fit beside each other.
        let stacked = self.s.kind == Kind::Binary && hex_stacked(width);
        let h = if stacked { (h / 2).max(1) } else { h };
        self.page = h;
        if self.cur < self.top {
            self.top = self.cur;
        } else if self.cur >= self.top + h {
            self.top = self.cur + 1 - h;
        }
        let view = self.view();
        let done = self.s.done();
        let (summary, rows) = match &*done {
            None => return (t!("compare.reading", "percent" => self.s.percent()), vec![]),
            Some(Done::Failed(e)) => return (crate::ui::printable(e).into_owned(), vec![]),
            Some(Done::Pictures(p)) => {
                let size = |s: &coxswain_core::compare::Size| format!("{} × {}", s.width, s.height);
                let line = match p.differ {
                    Some(0) => t!("compare.pictures_same"),
                    Some(n) => tn!("compare.pictures_differ", n as usize),
                    None => t!("compare.pictures_sizes"),
                };
                let lines = vec![
                    Line::from(format!(" {}: {}", self.names[0], size(&p.left))),
                    Line::from(format!(" {}: {}", self.names[1], size(&p.right))),
                    Line::from(""),
                    Line::from(format!(" {line}")),
                    Line::from(format!(" {}", t!("compare.pictures_desktop"))),
                ];
                return (line, lines);
            }
            Some(Done::Bytes(b)) => {
                let c = b.counted();
                let summary = if c.finished {
                    match c.differ {
                        0 => t!("compare.same"),
                        n => format!("{} · {}", tn!("compare.bytes_differ", n as usize), t!("compare.first_at", "offset" => format!("{:#x}", c.first.unwrap_or(0)))),
                    }
                } else {
                    format!("{} · {}", tn!("compare.bytes_differ", c.differ as usize), t!("compare.counting", "percent" => coxswain_core::settings::percent(c.done as usize, b.len[0].max(b.len[1]).max(1) as usize)))
                };
                let rows = b.rows(self.top as u64, h).unwrap_or_default();
                let lines = rows.iter().enumerate().flat_map(|(i, r)| self.hex_row(theme, r, self.top + i == self.cur, stacked)).collect();
                return (summary, lines);
            }
            Some(Done::Text(c)) => {
                if !self.placed {
                    self.placed = true;
                    if let Some(r) = c.next(view, 0, false).filter(|_| c.hunk_at(view, 0).is_none()) {
                        self.cur = r;
                        self.top = r.saturating_sub(h / 3);
                    }
                }
                let s = c.summary();
                let mut summary = if s.changes == 0 {
                    t!("compare.same_text")
                } else {
                    format!("{} · {} · {}", tn!("compare.changes", s.changes), tn!("compare.removed", s.removed), tn!("compare.added", s.added))
                };
                if s.kind == Kind::Document {
                    summary += &format!(" · {}", t!("compare.by_text"));
                }
                if s.encoding[0] != s.encoding[1] || s.crlf[0] != s.crlf[1] {
                    let eol = |crlf: bool| if crlf { "CRLF" } else { "LF" };
                    summary += &format!(" · {} {} ↔ {} {}", s.encoding[0], eol(s.crlf[0]), s.encoding[1], eol(s.crlf[1]));
                }
                if s.cut {
                    summary += &format!(" · {}", t!("compare.cut"));
                }
                if self.s.options.whitespace {
                    summary += &format!(" · {}", t!("compare.no_spaces"));
                }
                if self.s.options.case {
                    summary += &format!(" · {}", t!("compare.no_case"));
                }
                if s.dirty.iter().any(|&d| d) {
                    summary += &format!(" · {}", t!("editor.modified"));
                }
                (summary, c.rows(view, self.top, h))
            }
        };
        drop(done);
        let lines = self.text_rows(theme, &rows, view, width, total);
        (summary, lines)
    }

    /// A hex row: the offset, then both sides' bytes, those that differ marked; the right side
    /// on a line of its own under the left, `stacked`.
    fn hex_row(&self, theme: &Theme, r: &bytes::HexRow, cursor: bool, stacked: bool) -> Vec<Line<'static>> {
        let base = sty(&theme.panel);
        let mark = sty(&theme.git_modified).add_modifier(Modifier::REVERSED);
        let gutter = if cursor { sty(&theme.cursor) } else { faint(theme).bg(base.bg.unwrap_or(Color::Reset)) };
        let side = |side: &Vec<u8>, other: &Vec<u8>| {
            let mut spans = vec![];
            for i in 0..bytes::WIDTH as usize {
                let s = match side.get(i) {
                    Some(b) => format!("{b:02x}"),
                    None => "  ".into(),
                };
                let st = if side.get(i) != other.get(i) && side.get(i).is_some() { mark } else { base };
                spans.push(Span::styled(s, st));
                spans.push(Span::styled(" ", base));
            }
            spans.push(Span::styled("│ ", base));
            spans
        };
        let offset = Span::styled(format!(" {:08x} ", r.offset), gutter);
        let (left, right) = (side(&r.left, &r.right), side(&r.right, &r.left));
        if stacked {
            let under = Span::styled(" ".repeat(10), gutter);
            vec![Line::from([vec![offset], left].concat()), Line::from([vec![under], right].concat())]
        } else {
            vec![Line::from([vec![offset], left, right].concat())]
        }
    }

    /// The text rows, coloured as the viewer colours code, the changed words marked.
    fn text_rows(&mut self, theme: &Theme, rows: &[Row], view: View, width: u16, total: usize) -> Vec<Line<'static>> {
        let base = sty(&theme.panel);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let tag_style = |tag: Tag| match tag {
            Tag::Same => dim,
            Tag::Changed => sty(&theme.git_modified).bg(bg),
            Tag::Removed => sty(&theme.git_deleted).bg(bg),
            Tag::Added => sty(&theme.git_added).bg(bg),
            Tag::Taken => sty(&theme.git_renamed).bg(bg),
        };
        let slots: HashMap<&str, Style> = theme.slots().into_iter().map(|(n, s)| (n, sty(s))).collect();
        let token = |slot: Option<&str>| match slot.and_then(|s| slots.get(s)).and_then(|s| s.fg) {
            Some(fg) => base.fg(fg),
            None => base,
        };
        let gutter = total.to_string().len().max(3);
        let syntax = [0, 1].map(|i| syntax_for(Path::new(&self.names[i]), ""));
        let mut points = syntax.map(|s| s.map(Point::new));
        let mut out = vec![];
        let col_w = |w: usize| w.saturating_sub(gutter + 3);
        let half = (width as usize - 1) / 2;
        for (k, r) in rows.iter().enumerate() {
            let cursor = self.top + k == self.cur;
            let mut spans = vec![];
            let mut side = |spans: &mut Vec<Span<'static>>, i: usize, cell: Option<&Cell>, w: usize, slots: &mut Slots| {
                let sign = match (r.tag, i) {
                    (Tag::Same, _) => ' ',
                    (Tag::Taken, _) => '=',
                    (Tag::Changed, _) => '~',
                    (Tag::Removed, _) => '-',
                    (Tag::Added, _) => '+',
                };
                let gut = match cell {
                    Some(c) => format!("{:>gutter$}{sign} ", c.no),
                    None => " ".repeat(gutter + 2),
                };
                spans.push(Span::styled(gut, if cursor { sty(&theme.cursor) } else { tag_style(r.tag) }));
                let Some(c) = cell else {
                    spans.push(Span::styled(" ".repeat(col_w(w) + 1), dim));
                    return;
                };
                let pieces = match &mut points[i] {
                    Some(p) => paint(p, &c.text, slots),
                    None => vec![(None, c.text.clone())],
                };
                let mut cells: Vec<(char, Style)> = vec![];
                let mut at = 0;
                for (slot, s) in &pieces {
                    for ch in s.chars() {
                        let marked = c.marks.iter().any(|&(a, b)| at >= a && at < b);
                        let st = if marked { tag_style(r.tag).add_modifier(Modifier::REVERSED) } else { token(*slot) };
                        at += ch.len_utf8();
                        match ch {
                            '\t' => cells.extend(std::iter::repeat_n((' ', st), 4 - cells.len() % 4)),
                            ch if ch.is_control() => cells.push(('·', st)),
                            ch => {
                                cells.push((ch, st));
                                if ch.width() == Some(2) {
                                    cells.push(('\0', st));
                                }
                            }
                        }
                    }
                }
                let shown: Vec<_> = cells.into_iter().skip(self.left).take(col_w(w)).filter(|(c, _)| *c != '\0').collect();
                let n = shown.len();
                spans.extend(shown.into_iter().map(|(c, s)| Span::styled(c.to_string(), s)));
                spans.push(Span::styled(" ".repeat(col_w(w).saturating_sub(n) + 1), base));
            };
            match view {
                View::Split => {
                    side(&mut spans, 0, r.left.as_ref(), half, &mut self.slots);
                    spans.push(Span::styled("│", sty(&theme.border).bg(bg)));
                    side(&mut spans, 1, r.right.as_ref(), half, &mut self.slots);
                }
                View::Inline => {
                    let (i, cell) = if r.tag == Tag::Added { (1, r.right.as_ref()) } else { (0, r.left.as_ref()) };
                    side(&mut spans, i, cell, width as usize, &mut self.slots);
                }
            }
            out.push(Line::from(spans));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(v: &mut Viewer, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let b = term.backend().buffer();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>() + "\n").collect()
    }

    fn ready(v: &mut Viewer) {
        while v.s.done().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn hex_rows_show_both_sides_whole_at_any_width() {
        assert!(!hex_stacked(HEX_ROW) && hex_stacked(HEX_ROW - 1));
        let d = std::env::temp_dir().join(format!("coxswain-compare-hex-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.bin"), d.join("b.bin"));
        std::fs::write(&a, (0u8..32).collect::<Vec<_>>()).unwrap();
        std::fs::write(&b, (0u8..32).map(|x| x | 0xe0).collect::<Vec<_>>()).unwrap();
        let mut v = Viewer::open(&a, &b);
        ready(&mut v);
        // Beside each other, and one under the other at 80 columns: the right side's last byte
        // of a row is there either way.
        for w in [HEX_ROW + 2, 80] {
            let s = screen(&mut v, w, 12);
            assert!(s.contains("0f │") && s.contains("ef │"), "{w} columns:\n{s}");
        }
        let s = screen(&mut v, 80, 12);
        let row = s.lines().position(|l| l.contains("00000000")).unwrap();
        assert!(s.lines().nth(row + 1).unwrap().contains("e0 e1"), "the right side under the left:\n{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_next_difference_is_looked_for_off_the_screen_and_esc_stops_it() {
        let d = std::env::temp_dir().join(format!("coxswain-compare-seek-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.bin"), d.join("b.bin"));
        let mut x = vec![0u8; 4096];
        std::fs::write(&a, &x).unwrap();
        x[1000] = 1;
        std::fs::write(&b, &x).unwrap();
        let mut v = Viewer::open(&a, &b);
        ready(&mut v);
        // The key line is a binary compare's, not the text compare's.
        let s = screen(&mut v, 120, 20);
        assert!(s.contains(&t!("compare.keys_binary")) && !s.contains(&t!("compare.keys")), "{s}");
        let n = Key::new(KeyCode::Char('n'), false, false, false);
        assert!(matches!(v.key(n, None), Outcome::Stay));
        assert!(v.busy());
        while v.busy() {
            screen(&mut v, 120, 20);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(v.cur, 1000 / bytes::WIDTH as usize);
        // Esc while looking stops the looking, not the window.
        v.key(n, None);
        assert!(v.busy());
        assert!(matches!(v.key(Key::new(KeyCode::Esc, false, false, false), None), Outcome::Stay));
        assert!(!v.busy());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn names_carry_no_escape_to_the_terminal() {
        let n = super::name(std::path::Path::new("/tmp/a\x1b]0;owned\x07\x1b[31m.txt"));
        assert!(!n.chars().any(char::is_control), "{n:?}");
        assert_eq!(n, "a?]0;owned??[31m.txt");
    }
}

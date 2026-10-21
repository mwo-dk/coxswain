//! Chess, full screen (F3 or Alt+K on a PGN, EPD or FEN file): a PGN file's games as a table
//! with a filter, filling while the file is read; a game as a board on coloured squares with
//! its moves beside it, variations and comments in place. An EPD or FEN file: its positions.
//! Reading and playing out are coxswain-core's `chess`; the desktop app shows the same.

use coxswain_core::chess::polyglot::{self, BookMove};
use coxswain_core::chess::{self, Format, Library, Notation, Position, Query, Sort, Token};
use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::t;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use crate::ui::Cols;

use crate::bom::faint;
use crate::ui::{fit, sty};

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
}

/// The filter field being typed into.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Player,
    Opening,
    Years,
    Elo,
}

/// A game open on the board.
struct Open {
    /// Its index in the file, from 0; for an EPD file, the position's.
    index: u32,
    game: chess::Game,
    layout: Vec<Token>,
    /// The move shown; None: the position before the first.
    at: Option<u32>,
}

pub struct Viewer {
    pub path: PathBuf,
    lib: Option<Arc<Library>>,
    /// A game being read, on a thread of its own (a packed file is unpacked up to it).
    reading: Option<Receiver<(u32, std::io::Result<chess::Game>)>>,
    /// A Find hit's game: being looked for in the file, then waited for until the table has it.
    hit: Option<Receiver<Option<u32>>>,
    want: Option<u32>,
    positions: Vec<Position>,
    query: Query,
    /// The games the filter lets through, in order, and how many games there were then.
    ids: Vec<u32>,
    counted: usize,
    asked: Instant,
    cursor: usize,
    offset: usize,
    typing: Option<(Field, String)>,
    open: Option<Open>,
    flip: bool,
    notation: Notation,
    board: [Color; 2],
    said: Option<String>,
    /// The opening book whose moves are shown (`polyglot::find`), and the last position's.
    book: Option<PathBuf>,
    book_moves: Option<(String, Vec<BookMove>)>,
    /// The file is a book: its moves are played on the board from the start, `line` the
    /// positions before, `pick` the move chosen.
    book_only: bool,
    line: Vec<String>,
    pick: usize,
}

fn rgb(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x888888);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// A light square two fifths of the way to the dark one.
fn mix(light: Color, dark: Color) -> Color {
    match (light, dark) {
        (Color::Rgb(a, b, c), Color::Rgb(x, y, z)) => {
            let m = |p: u8, q: u8| ((p as u16 * 3 + q as u16 * 2) / 5) as u8;
            Color::Rgb(m(a, x), m(b, y), m(c, z))
        }
        (l, _) => l,
    }
}

/// A colour halfway to `to`: the squares of the last move.
fn tint(c: Color, to: (u8, u8, u8)) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(((r as u16 + to.0 as u16) / 2) as u8, ((g as u16 + to.1 as u16) / 2) as u8, ((b as u16 + to.2 as u16) / 2) as u8),
        c => c,
    }
}

/// A piece's glyph: the filled ones for both sides, told apart by their colour.
fn glyph(p: char) -> &'static str {
    match p.to_ascii_lowercase() {
        'k' => "♚",
        'q' => "♛",
        'r' => "♜",
        'b' => "♝",
        'n' => "♞",
        _ => "♟",
    }
}

impl Viewer {
    pub fn open(path: &Path, cfg: &coxswain_core::config::Config, theme: &Theme) -> std::io::Result<Viewer> {
        let mut v = Viewer {
            path: path.to_path_buf(),
            lib: None,
            positions: vec![],
            query: Query::default(),
            ids: vec![],
            counted: 0,
            asked: Instant::now(),
            cursor: 0,
            offset: 0,
            typing: None,
            open: None,
            flip: false,
            notation: Notation::from_config(&cfg.chess.notation),
            board: {
                // A character is small on its square: the light squares darker than the desktop
                // app's, so that a white piece stands out on them.
                let [light, dark] = chess::board_colours(theme, &cfg.chess.board).map(|c| rgb(&c));
                [mix(light, dark), dark]
            },
            said: None,
            reading: None,
            hit: None,
            want: None,
            book: polyglot::find(path, &cfg.chess.book),
            book_moves: None,
            book_only: false,
            line: vec![],
            pick: 0,
        };
        match chess::sniff(path) {
            Some(Format::Pgn) => v.lib = Some(Arc::new(Library::open(path)?)),
            Some(Format::Book) => {
                v.book_only = true;
                let game = chess::Game { start: chess::pgn::START.to_string(), ..Default::default() };
                v.open = Some(Open { index: 0, layout: vec![], game, at: None });
            }
            _ => {
                v.positions = chess::read_positions(path)?;
                v.show_position(0);
            }
        }
        Ok(v)
    }

    /// Shows the game a Find hit's snippet is about, once it is found (on a thread: the file is
    /// read from its start for it) and the table has reached it.
    pub fn at_hit(&mut self, snippet: String) {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.path.clone();
        std::thread::spawn(move || tx.send(chess::games::game_of_hit(&path, &snippet)));
        self.hit = Some(rx);
        self.said = Some(t!("common.loading"));
    }

    /// The table follows the reading: asked again when there are more games, twice a second.
    pub fn poll(&mut self) {
        if let Some(r) = self.hit.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.hit = None;
            self.said = None;
            self.want = r;
        }
        if let Some((index, r)) = self.reading.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.reading = None;
            self.said = None;
            match r {
                Ok(game) => {
                    let layout = game.layout();
                    self.said = game.error.as_ref().map(|e| t!("chess.cannot_play", "move" => e));
                    self.open = Some(Open { index, game, layout, at: None });
                }
                Err(e) => self.said = Some(e.to_string()),
            }
        }
        let Some(lib) = &self.lib else { return };
        let p = lib.progress();
        if p.games != self.counted && (self.asked.elapsed().as_millis() >= 500 || p.done || self.counted == 0) {
            self.ids = lib.query(&self.query);
            self.counted = p.games;
            self.asked = Instant::now();
            self.cursor = self.cursor.min(self.ids.len().saturating_sub(1));
        }
        // The hit's game, once the table has it.
        if let Some(n) = self.want
            && (p.games as u32 > n || p.done)
        {
            self.want = None;
            if let Some(i) = self.ids.iter().position(|&g| g == n) {
                self.cursor = i;
            }
            self.show_game(n);
        }
    }

    fn requery(&mut self) {
        if let Some(lib) = &self.lib {
            self.ids = lib.query(&self.query);
            self.counted = lib.progress().games;
            self.asked = Instant::now();
            self.cursor = 0;
            self.offset = 0;
        }
    }

    /// Reads game `index` on a thread; `poll` shows it.
    fn show_game(&mut self, index: u32) {
        let Some(lib) = self.lib.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((index, lib.game(index)));
        });
        self.reading = Some(rx);
        self.said = Some(t!("common.loading"));
    }

    fn show_position(&mut self, i: usize) {
        let Some(p) = self.positions.get(i) else { return };
        let game = chess::Game { start: p.fen.clone().unwrap_or_default(), comment: p.about.clone(), ..Default::default() };
        self.said = p.fen.is_none().then(|| t!("chess.no_position", "line" => p.line));
        self.open = Some(Open { index: i as u32, layout: game.layout(), game, at: None });
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if let Some((field, mut text)) = self.typing.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let num = |s: &str| s.trim().parse::<u16>().ok();
                    match field {
                        Field::Player => self.query.player = text,
                        Field::Opening => self.query.opening = text,
                        Field::Elo => self.query.elo = num(&text),
                        Field::Years => {
                            let (a, b) = text.split_once('-').unwrap_or((&text, &text));
                            (self.query.from, self.query.to) = (num(a), num(b));
                        }
                    }
                    self.requery();
                }
                KeyCode::Backspace => {
                    text.pop();
                    self.typing = Some((field, text));
                }
                KeyCode::Char(c) if !key.ctrl && !key.alt => {
                    text.push(c);
                    self.typing = Some((field, text));
                }
                _ => self.typing = Some((field, text)),
            }
            return Outcome::Stay;
        }
        if action == Some(Action::Quit) {
            return Outcome::Close;
        }
        self.said = None;
        if self.open.is_some() {
            return self.game_key(key);
        }
        let n = self.ids.len();
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(20),
            KeyCode::PageDown => self.cursor = (self.cursor + 20).min(n.saturating_sub(1)),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(&g) = self.ids.get(self.cursor) {
                    self.show_game(g);
                }
            }
            KeyCode::Char('/') => self.typing = Some((Field::Player, self.query.player.clone())),
            KeyCode::Char('o') => self.typing = Some((Field::Opening, self.query.opening.clone())),
            KeyCode::Char('e') => self.typing = Some((Field::Elo, self.query.elo.map(|e| e.to_string()).unwrap_or_default())),
            KeyCode::Char('y') => {
                let years = match (self.query.from, self.query.to) {
                    (None, None) => String::new(),
                    (a, b) => format!("{}-{}", a.map(|y| y.to_string()).unwrap_or_default(), b.map(|y| y.to_string()).unwrap_or_default()),
                };
                self.typing = Some((Field::Years, years));
            }
            KeyCode::Char('r') => {
                let all = ["", "1-0", "0-1", "1/2-1/2"];
                let at = all.iter().position(|r| *r == self.query.result).unwrap_or(0);
                self.query.result = all[(at + 1) % all.len()].to_string();
                self.requery();
            }
            KeyCode::Char('s') => {
                let at = Sort::ALL.iter().position(|s| *s == self.query.sort).unwrap_or(0);
                self.query.sort = Sort::ALL[(at + 1) % Sort::ALL.len()];
                self.requery();
            }
            KeyCode::Char('S') => {
                self.query.desc = !self.query.desc;
                self.requery();
            }
            KeyCode::Char('c') => {
                self.query = Query { sort: self.query.sort, desc: self.query.desc, ..Query::default() };
                self.requery();
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The book's moves for `fen`, read once per position.
    fn book_moves(&mut self, fen: &str) -> &[BookMove] {
        if self.book_moves.as_ref().is_none_or(|(f, _)| f != fen) {
            let moves = self.book.as_deref().map(|b| polyglot::moves_at(b, fen)).unwrap_or_default();
            self.book_moves = Some((fen.to_string(), moves));
        }
        &self.book_moves.as_ref().expect("set above").1
    }

    /// A key in a book of its own: its moves are chosen and played.
    fn book_key(&mut self, key: Key) -> Outcome {
        let Some(fen) = self.open.as_ref().map(|o| o.game.start.clone()) else { return Outcome::Close };
        let n = self.book_moves(&fen).len();
        let mut to = None;
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Down => self.pick = (self.pick + 1).min(n.saturating_sub(1)),
            KeyCode::Up => self.pick = self.pick.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right => {
                let pick = self.pick;
                if let Some(m) = self.book_moves(&fen).get(pick) {
                    to = Some(m.fen.clone());
                    self.line.push(fen);
                }
            }
            KeyCode::Left | KeyCode::Backspace => to = self.line.pop(),
            KeyCode::Home => {
                to = self.line.first().cloned();
                self.line.clear();
            }
            KeyCode::Char('f') => self.flip = !self.flip,
            KeyCode::Char('n') => {
                self.notation = if self.notation == Notation::Figurines { Notation::Letters } else { Notation::Figurines };
            }
            _ => {}
        }
        if let (Some(fen), Some(o)) = (to, self.open.as_mut()) {
            o.game.start = fen;
            self.pick = 0;
        }
        Outcome::Stay
    }

    fn game_key(&mut self, key: Key) -> Outcome {
        if self.book_only {
            return self.book_key(key);
        }
        let epd = self.lib.is_none();
        if epd {
            let i = self.open.as_ref().map_or(0, |o| o.index as usize);
            match key.code {
                KeyCode::Esc => return Outcome::Close,
                KeyCode::Right => self.show_position((i + 1).min(self.positions.len().saturating_sub(1))),
                KeyCode::Left => self.show_position(i.saturating_sub(1)),
                KeyCode::Home => self.show_position(0),
                KeyCode::End => self.show_position(self.positions.len().saturating_sub(1)),
                _ => {}
            }
        }
        let Some(o) = self.open.as_mut() else { return Outcome::Stay };
        let g = &o.game;
        let node = |n: u32| &g.nodes[n as usize];
        // The moves this one could be instead: its parent's children, or the first moves.
        let siblings = |at: u32| -> &[u32] {
            match node(at).parent {
                Some(p) => &node(p).children,
                None => &g.roots,
            }
        };
        match key.code {
            _ if epd && !matches!(key.code, KeyCode::Char(_)) => {}
            KeyCode::Esc => self.open = None,
            KeyCode::Right => {
                o.at = match o.at {
                    None => g.roots.first().copied(),
                    Some(n) => node(n).children.first().copied().or(Some(n)),
                }
            }
            KeyCode::Left => o.at = o.at.and_then(|n| node(n).parent),
            KeyCode::Home => o.at = None,
            KeyCode::End => {
                let mut at = o.at.or(g.roots.first().copied());
                while let Some(next) = at.and_then(|n| node(n).children.first().copied()) {
                    at = Some(next);
                }
                o.at = at;
            }
            // Into the variations of this move and back.
            KeyCode::Down | KeyCode::Up => {
                if let Some(n) = o.at {
                    let s = siblings(n);
                    let i = s.iter().position(|&x| x == n).unwrap_or(0);
                    let j = if key.code == KeyCode::Down { (i + 1).min(s.len() - 1) } else { i.saturating_sub(1) };
                    o.at = Some(s[j]);
                }
            }
            KeyCode::PageDown | KeyCode::PageUp if !epd => {
                let i = self.ids.iter().position(|&x| x == o.index).unwrap_or(0);
                let j = if key.code == KeyCode::PageDown { (i + 1).min(self.ids.len().saturating_sub(1)) } else { i.saturating_sub(1) };
                if let Some(&next) = self.ids.get(j) {
                    self.cursor = j;
                    self.show_game(next);
                }
            }
            KeyCode::Char('f') => self.flip = !self.flip,
            KeyCode::Char('n') => {
                self.notation = if self.notation == Notation::Figurines { Notation::Letters } else { Notation::Figurines };
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let title = self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(bg))
            .style(base)
            .title(Line::from(format!(" ♞ {title} ")).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area).inner(Margin::new(1, 0));
        f.render_widget(block, area);
        if inner.height < 4 || inner.width < 20 {
            return;
        }
        let foot = Rect { y: inner.bottom() - 1, height: 1, ..inner };
        let said = Rect { y: inner.bottom() - 2, height: 1, ..inner };
        let body = Rect { height: inner.height - 2, ..inner };
        let w = inner.width as usize;
        let keys = if self.book_only {
            t!("chess.keys_book")
        } else if self.open.is_some() {
            if self.lib.is_some() { t!("chess.keys_game") } else { t!("chess.keys_position") }
        } else {
            t!("chess.keys_list")
        };
        f.render_widget(Paragraph::new(fit(&keys, w)).style(dim), foot);
        let line = match &self.typing {
            Some((field, text)) => {
                let label = match field {
                    Field::Player => t!("chess.filter_player"),
                    Field::Opening => t!("chess.filter_opening"),
                    Field::Years => t!("chess.filter_years"),
                    Field::Elo => t!("chess.filter_elo"),
                };
                Some((format!("{label}: {text}▏"), sty(&theme.dialog_input)))
            }
            None => self.said.clone().map(|s| (s, dim)),
        };
        if let Some((s, style)) = line {
            f.render_widget(Paragraph::new(fit(&s, w)).style(style), said);
        }
        if self.open.is_some() {
            self.draw_game(f, theme, body);
        } else {
            self.draw_list(f, theme, body);
        }
    }

    fn draw_list(&mut self, f: &mut Frame, theme: &Theme, area: Rect) {
        let base = sty(&theme.dialog);
        let dim = faint(theme).bg(base.bg.unwrap_or(Color::Reset));
        let Some(lib) = &self.lib else { return };
        let p = lib.progress();
        let w = area.width as usize;
        // The state: how many, the filter, the order.
        let mut state = t!("chess.games", "shown" => self.ids.len(), "total" => p.games);
        if !p.done {
            let pct = (p.read * 100).checked_div(p.size).unwrap_or(0);
            state = format!("{state} · {}", t!("chess.reading", "percent" => pct));
        }
        if p.capped {
            state = format!("{state} · {}", t!("chess.capped", "n" => p.games));
        }
        if let Some(e) = &p.error {
            state = format!("{state} · {e}");
        }
        let q = &self.query;
        let mut filter = vec![];
        for (label, v) in [(t!("chess.filter_player"), q.player.clone()), (t!("chess.filter_opening"), q.opening.clone()), (t!("chess.col_result"), q.result.clone())] {
            if !v.is_empty() {
                filter.push(format!("{label} {v}"));
            }
        }
        if q.from.is_some() || q.to.is_some() {
            filter.push(format!("{} {}–{}", t!("chess.filter_years"), q.from.map(|y| y.to_string()).unwrap_or_default(), q.to.map(|y| y.to_string()).unwrap_or_default()));
        }
        if let Some(e) = q.elo {
            filter.push(format!("{} ≥ {e}", t!("chess.filter_elo")));
        }
        if !filter.is_empty() {
            state = format!("{state} · {}", filter.join(", "));
        }
        state = format!("{state} · {} {}{}", t!("chess.sorted_by"), column(q.sort), if q.desc { " ↓" } else { " ↑" });
        f.render_widget(Paragraph::new(fit(&state, w)).style(dim), Rect { height: 1, ..area });
        // Columns: fixed ones, and the names sharing the rest.
        let fixed = [7usize, 5, 5, 7, 10, 4, 6];
        let rest = w.saturating_sub(fixed.iter().sum::<usize>() + 10);
        let name = (rest * 22 / 100).max(6);
        let event = (rest * 20 / 100).max(4);
        let opening = rest.saturating_sub(2 * name + event).max(4);
        let row = |cells: [String; 11]| -> String {
            let widths = [fixed[0], name, fixed[1], name, fixed[2], fixed[3], fixed[4], event, fixed[5], opening, fixed[6]];
            let mut out = String::new();
            for (i, (c, cw)) in cells.iter().zip(widths).enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&fit(c, cw));
            }
            out
        };
        let head = row([
            "#".into(),
            t!("chess.col_white"),
            t!("chess.col_elo"),
            t!("chess.col_black"),
            t!("chess.col_elo"),
            t!("chess.col_result"),
            t!("chess.col_date"),
            t!("chess.col_event"),
            "ECO".into(),
            t!("chess.col_opening"),
            t!("chess.col_moves"),
        ]);
        f.render_widget(Paragraph::new(fit(&head, w)).style(base.add_modifier(Modifier::BOLD)), Rect { y: area.y + 1, height: 1, ..area });
        let h = area.height.saturating_sub(2) as usize;
        if self.ids.is_empty() {
            let s = if p.done { t!("chess.no_games") } else { t!("common.loading") };
            f.render_widget(Paragraph::new(fit(&s, w)).style(dim), Rect { y: area.y + 2, height: 1, ..area });
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        for (k, &id) in self.ids.iter().enumerate().skip(self.offset).take(h) {
            let Some(r) = lib.row(id) else { continue };
            let elo = |e: Option<u16>| e.map(|e| e.to_string()).unwrap_or_default();
            let text = row([
                r.n.to_string(),
                r.white,
                elo(r.white_elo),
                r.black,
                elo(r.black_elo),
                r.result,
                r.date,
                r.event,
                r.eco,
                r.opening,
                r.moves.to_string(),
            ]);
            let style = if k == self.cursor { sty(&theme.dialog_input) } else { base };
            let y = area.y + 2 + (k - self.offset) as u16;
            f.render_widget(Paragraph::new(fit(&text, w)).style(style), Rect { y, height: 1, ..area });
        }
    }

    fn draw_game(&mut self, f: &mut Frame, theme: &Theme, area: Rect) {
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let Some(o) = &self.open else { return };
        let now = match o.at {
            Some(n) => o.game.nodes[n as usize].fen.clone(),
            None => Some(o.game.start.clone()),
        };
        let book: Vec<BookMove> = match &now {
            Some(fen) if self.book.is_some() => self.book_moves(fen).to_vec(),
            _ => vec![],
        };
        let Some(o) = &self.open else { return };
        let g = &o.game;
        // The board: as big as fits, a square twice as wide as high.
        let cell_h = ((area.height.saturating_sub(2)) / 8).clamp(1, 2);
        let mut cell_w = cell_h * 2 + 1;
        while cell_w > 3 && cell_w * 8 + 2 > area.width / 2 {
            cell_w -= 2;
        }
        let fen = match o.at {
            Some(n) => g.nodes[n as usize].fen.clone(),
            None => Some(g.start.clone()),
        };
        let last = o.at.map(|n| (g.nodes[n as usize].from, g.nodes[n as usize].to));
        let squares = chess::squares(fen.as_deref().unwrap_or(""));
        let mut lines: Vec<Line> = vec![];
        for row in 0..8u16 {
            let rank = if self.flip { row } else { 7 - row };
            for sub in 0..cell_h {
                let label = if sub == cell_h / 2 { format!("{} ", rank + 1) } else { "  ".into() };
                let mut spans = vec![Span::styled(label, dim)];
                for col in 0..8u16 {
                    let file = if self.flip { 7 - col } else { col };
                    let sq = (rank * 8 + file) as u8;
                    let mut colour = self.board[((rank + file) % 2 == 0) as usize];
                    if last.is_some_and(|(a, b)| a == Some(sq) || b == Some(sq)) {
                        colour = tint(colour, (0xf6, 0xe0, 0x5a));
                    }
                    let (text, fg) = match squares[sq as usize] {
                        Some(p) if sub == cell_h / 2 => {
                            let pad = " ".repeat(((cell_w - 1) / 2) as usize);
                            let fg = if p.is_ascii_uppercase() { Color::Rgb(255, 255, 255) } else { Color::Rgb(0, 0, 0) };
                            (format!("{pad}{}{pad}", glyph(p)), fg)
                        }
                        _ => (" ".repeat(cell_w as usize), Color::Reset),
                    };
                    spans.push(Span::styled(text, Style::default().bg(colour).fg(fg).add_modifier(Modifier::BOLD)));
                }
                lines.push(Line::from(spans));
            }
        }
        let mut files = String::from("  ");
        for col in 0..8u16 {
            let file = if self.flip { 7 - col } else { col };
            let c = (b'a' + file as u8) as char;
            let pad = ((cell_w - 1) / 2) as usize;
            files.push_str(&format!("{}{c}{}", " ".repeat(pad), " ".repeat(cell_w as usize - 1 - pad)));
        }
        lines.push(Line::styled(files, dim));
        // Whose move, under the board.
        if let Some(fen) = &fen {
            let white = fen.split_whitespace().nth(1) == Some("w");
            lines.push(Line::styled(t!(if white { "chess.white_to_move" } else { "chess.black_to_move" }), dim));
        }
        let board_w = 2 + cell_w * 8;
        f.render_widget(Paragraph::new(lines), Rect { width: board_w.min(area.width), ..area });
        // Beside it: who played, then the moves.
        let right = Rect { x: area.x + board_w + 2, width: area.width.saturating_sub(board_w + 2), ..area };
        if right.width < 10 {
            return;
        }
        let w = right.width as usize;
        let mut head: Vec<Line> = vec![];
        let tag = |k: &str| g.tag(k).filter(|v| !v.is_empty() && *v != "?").unwrap_or("").to_string();
        if self.lib.is_some() {
            let elo = |k: &str| {
                let e = tag(k);
                if e.is_empty() { e } else { format!(" ({e})") }
            };
            head.push(Line::styled(fit(&format!("{}{} – {}{}  {}", tag("White"), elo("WhiteElo"), tag("Black"), elo("BlackElo"), g.result), w), base.add_modifier(Modifier::BOLD)));
            let about = [tag("Event"), tag("Site"), tag("Date"), [tag("ECO"), tag("Opening")].join(" ").trim().to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            head.push(Line::styled(fit(&about, w), dim));
            let total = self.lib.as_ref().map_or(0, |l| l.progress().games);
            head.push(Line::styled(fit(&t!("chess.game_of", "n" => o.index + 1, "total" => total), w), dim));
        } else if self.book_only {
            let entries = polyglot::Book::open(&self.path).map_or(0, |b| b.entries());
            head.push(Line::styled(fit(&t!("chess.book_entries", "n" => entries), w), base.add_modifier(Modifier::BOLD)));
        } else {
            head.push(Line::styled(fit(&t!("chess.position_of", "n" => o.index + 1, "total" => self.positions.len()), w), base.add_modifier(Modifier::BOLD)));
        }
        let pieces = chess::pieces(self.notation);
        // The book's moves for this position: a list to choose from in a book of its own, a
        // line under the game's heading otherwise.
        let book_name = self.book.as_ref().and_then(|b| b.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if self.book_only {
            head.push(Line::from(""));
            if book.is_empty() {
                head.push(Line::styled(fit(&t!("chess.book_none"), w), dim));
            }
            for (k, m) in book.iter().enumerate() {
                let learn = if m.learn > 0 { format!("  {}", t!("chess.book_learn", "n" => m.learn)) } else { String::new() };
                let text = format!("{:<8} {:>5.1} %  {:>5}{learn}", chess::show_san(&m.san, &pieces), m.share, m.weight);
                let style = if k == self.pick { sty(&theme.dialog_input) } else { base };
                head.push(Line::styled(fit(&text, w), style));
            }
            f.render_widget(Paragraph::new(head), right);
            return;
        }
        if self.book.is_some() {
            let text = if book.is_empty() {
                t!("chess.book_out", "name" => book_name)
            } else {
                let list: Vec<String> = book.iter().take(8).map(|m| format!("{} {:.0} %", chess::show_san(&m.san, &pieces), m.share)).collect();
                t!("chess.book", "name" => book_name, "moves" => list.join(" · "))
            };
            head.push(Line::styled(fit(&text, w), dim));
        }
        head.push(Line::from(""));
        // The moves, word by word, wrapped; the line with the move shown is kept in view.
        let mut words: Vec<(String, Style, bool)> = vec![];
        for tok in &o.layout {
            match tok {
                Token::Move { node, number } => {
                    let n = &g.nodes[*node as usize];
                    let (glued, apart) = chess::nags(&n.nags);
                    let text = format!("{}{}{glued}", number.clone().unwrap_or_default(), chess::show_san(&n.san, &pieces));
                    let here = o.at == Some(*node);
                    let mut style = if here { sty(&theme.dialog_input) } else if n.depth > 0 { dim } else { base };
                    if n.depth == 0 && !here {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    words.push((text, style, here));
                    if !apart.is_empty() {
                        words.push((apart, base, false));
                    }
                }
                Token::Comment { text } => {
                    for word in text.split_whitespace() {
                        words.push((word.to_string(), dim.add_modifier(Modifier::ITALIC), false));
                    }
                }
                Token::Open { .. } => words.push(("(".into(), dim, false)),
                Token::Close { .. } => words.push((")".into(), dim, false)),
                Token::Result { text } => words.push((text.clone(), base.add_modifier(Modifier::BOLD), false)),
            }
        }
        let mut body: Vec<Vec<Span>> = vec![vec![]];
        let mut used = 0;
        let mut here_line = 0;
        for (text, style, here) in words {
            let tw = text.width();
            if used > 0 && used + 1 + tw > w {
                body.push(vec![]);
                used = 0;
            }
            let line = body.last_mut().expect("one line at least");
            if used > 0 && text != ")" {
                line.push(Span::styled(" ", base));
                used += 1;
            }
            if here {
                here_line = body.len() - 1;
            }
            used += tw;
            body.last_mut().expect("one line at least").push(Span::styled(text, style));
        }
        let room = (right.height as usize).saturating_sub(head.len());
        let top = here_line.saturating_sub(room.saturating_sub(2));
        head.extend(body.into_iter().skip(top).take(room).map(Line::from));
        f.render_widget(Paragraph::new(head), right);
    }
}

/// A column's name, for the order line.
fn column(s: Sort) -> String {
    match s {
        Sort::Number => "#".into(),
        Sort::White => t!("chess.col_white"),
        Sort::Black => t!("chess.col_black"),
        Sort::WhiteElo => format!("{} {}", t!("chess.col_white"), t!("chess.col_elo")),
        Sort::BlackElo => format!("{} {}", t!("chess.col_black"), t!("chess.col_elo")),
        Sort::Result => t!("chess.col_result"),
        Sort::Date => t!("chess.col_date"),
        Sort::Event => t!("chess.col_event"),
        Sort::Eco => "ECO".into(),
        Sort::Opening => t!("chess.col_opening"),
        Sort::Moves => t!("chess.col_moves"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    #[test]
    fn a_game_stepped_through_and_drawn() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-chess-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pgn");
        std::fs::write(&p, "[White \"Morphy\"]\n[Black \"Duke\"]\n\n1.e4 e5 2.Nf3 d6 (2...Nc6 3.Bb5) 3.d4 {centre} 1-0\n\n[White \"B\"]\n\n1.d4 *\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        assert_eq!(v.ids, vec![0, 1]);
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..4 {
            v.key(key(KeyCode::Right), None);
        }
        // On 2…d6, Down goes into its variation 2…Nc6, Right on to 3.Bb5.
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Right), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.nodes[o.at.unwrap() as usize].san, "Bb5");
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("Morphy") && screen.contains("♘f3") && screen.contains("♗b5") && screen.contains("centre"), "{screen}");
        assert!(screen.contains('♚'));
        // Esc back to the table, a filter, and the table again.
        v.key(key(KeyCode::Esc), None);
        v.key(key(KeyCode::Char('/')), None);
        for c in "morph".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert_eq!(v.ids, vec![0]);
        // A Find hit: found on a thread, then shown.
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        let (o, c) = coxswain_core::store::MARK;
        v.at_hit(format!("{o}B{c}"));
        let start = Instant::now();
        while v.open.is_none() && start.elapsed().as_secs() < 10 {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(v.open.as_ref().map(|o| o.index), Some(1));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_book_played_through() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-book-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Two moves from the start, one after 1.e4: (key, from, to, weight).
        let zobrist = |fen: &str| polyglot::key(&chess::pgn::board(fen).unwrap());
        let sq = |s: &str| (s.as_bytes()[0] - b'a') as u16 | (((s.as_bytes()[1] - b'1') as u16) << 3);
        let start = zobrist(chess::pgn::START);
        let e4 = zobrist("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
        let mut entries = vec![(start, "e2", "e4", 3u16), (start, "d2", "d4", 1), (e4, "c7", "c5", 1)];
        entries.sort_by_key(|e| e.0);
        let mut bytes = vec![];
        for (k, from, to, w) in entries {
            bytes.extend(k.to_be_bytes());
            bytes.extend((sq(to) | (sq(from) << 6)).to_be_bytes());
            bytes.extend(w.to_be_bytes());
            bytes.extend(0u32.to_be_bytes());
        }
        let book = d.join("book.bin");
        std::fs::write(&book, bytes).unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&book, &cfg, &Theme::default()).unwrap();
        let screen = |v: &mut Viewer| {
            let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
            term.draw(|f| v.draw(f, &Theme::default())).unwrap();
            term.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>()
        };
        let s = screen(&mut v);
        assert!(s.contains("e4") && s.contains("75.0 %") && s.contains("d4"), "{s}");
        v.key(key(KeyCode::Enter), None);
        assert!(screen(&mut v).contains("c5"));
        v.key(key(KeyCode::Left), None);
        assert!(screen(&mut v).contains("75.0 %"));
        // A game next to the book: its moves under the heading.
        let pgn = d.join("g.pgn");
        std::fs::write(&pgn, "[White \"A\"]\n\n1.e4 c5 *\n").unwrap();
        let mut v = Viewer::open(&pgn, &cfg, &Theme::default()).unwrap();
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let s = screen(&mut v);
        assert!(s.contains("book.bin") && s.contains("e4 75 %"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}

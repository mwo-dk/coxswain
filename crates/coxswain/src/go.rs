//! Go, full screen (F3 or Alt+K on an SGF file): the file's games as a table, a game as a
//! board of Unicode stones on the grid with its coordinates, the move, captures, variations and
//! comments beside it. A file of one game opens on its board. Reading and playing out are
//! coxswain-core's `go`; the desktop app shows the same.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::go::{self, Collection, Game};
use coxswain_core::mosaic::{Pieces, Shape};
use coxswain_core::t;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::bom::faint;
use crate::chess::Outcome;
use crate::ui::{fit, sty};

/// A game open on the board.
struct Open {
    index: usize,
    game: Game,
    at: u32,
}

pub struct Viewer {
    pub path: PathBuf,
    games: Collection,
    cursor: usize,
    offset: usize,
    open: Option<Open>,
    /// The board's wood: the theme's light square colour.
    wood: Color,
    said: Option<String>,
    /// How the stones are drawn (`[chess] pieces`), and their pictures where the terminal
    /// shows them.
    pieces: Pieces,
    pictures: Option<crate::pictures::Pieces>,
    /// Whether the file was looked at for having one game only.
    single: bool,
    /// A Find hit's game being looked for, on a thread of its own (up to 1 GiB of the file is
    /// read): its answer, and what stops it (Esc).
    finding: Option<(mpsc::Receiver<Option<u32>>, Arc<AtomicBool>)>,
    /// The hit's game, opened once the list has reached it.
    want: Option<usize>,
}

fn rgb(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xc8a060);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Closed while a Find hit's game is looked for: the looking stops.
impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some((_, stop)) = &self.finding {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Viewer {
    pub fn open(path: &Path, cfg: &coxswain_core::config::Config, theme: &Theme) -> std::io::Result<Viewer> {
        let games = Collection::open(path)?;
        let [light, _] = coxswain_core::chess::board_colours(theme, &cfg.chess.board);
        let pieces = crate::pieces::pieces(cfg);
        let pictures = if pieces == Pieces::Pictures { crate::pictures::support().map(crate::pictures::Pieces::new) } else { None };
        let mut v = Viewer { path: path.to_path_buf(), games, cursor: 0, offset: 0, open: None, wood: rgb(&light), said: None, pieces, pictures, single: false, finding: None, want: None };
        if pieces != Pieces::Pictures && cfg.chess.pieces == "pictures" {
            v.said = Some(t!("chess.pictures_none"));
        }
        v.poll();
        Ok(v)
    }

    /// Looks for the game of a Find hit's `snippet` on a thread of its own, and opens it when
    /// found; the table is there meanwhile, and Esc stops the looking.
    pub fn at_hit(&mut self, snippet: String) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (path, s) = (self.path.clone(), stop.clone());
        std::thread::spawn(move || tx.send(go::game_of_hit(&path, &snippet, &s)));
        self.finding = Some((rx, stop));
        self.said = Some(t!("go.finding"));
    }

    /// A file of one game opens on its board, once its list is read; a Find hit's game once it
    /// is found and listed.
    pub fn poll(&mut self) {
        if let Some((rx, _)) = &self.finding {
            match rx.try_recv() {
                Ok(i) => {
                    self.finding = None;
                    self.said = None;
                    self.want = i.map(|i| i as usize);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.finding = None;
                    self.said = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(i) = self.want {
            let p = self.games.progress();
            if p.games > i || p.done {
                self.want = None;
                self.single = true;
                // Not over a game opened meanwhile.
                if i < p.games && self.open.is_none() {
                    self.show(i);
                }
            }
        }
        if !self.single && self.open.is_none() && self.finding.is_none() && self.want.is_none() {
            let p = self.games.progress();
            if p.done {
                self.single = true;
                if p.games == 1 {
                    self.show(0);
                }
            }
        }
    }

    /// Whether a Find hit's game is being looked for.
    pub fn finding(&self) -> bool {
        self.finding.is_some()
    }

    /// The game open on the board (from 0).
    pub fn shown(&self) -> Option<usize> {
        self.open.as_ref().map(|o| o.index)
    }

    /// Opens game `i` (from 0) at its start.
    pub fn show(&mut self, i: usize) {
        match self.games.game(i) {
            Ok(game) => {
                self.said = game.capped.then(|| t!("go.capped", "n" => game.nodes.len()));
                self.open = Some(Open { index: i, game, at: 0 });
                self.cursor = i;
            }
            Err(e) => self.said = Some(e.to_string()),
        }
    }

    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if action == Some(Action::Quit) {
            return Outcome::Close;
        }
        // Esc while a Find hit's game is looked for stops the looking: the table stays. Other
        // keys move in the table meanwhile, and the note stays.
        if let Some((_, stop)) = &self.finding {
            if key.code == KeyCode::Esc {
                stop.store(true, Ordering::Relaxed);
                self.finding = None;
                self.said = None;
                return Outcome::Stay;
            }
        } else {
            self.said = None;
        }
        let n = self.games.progress().games;
        let Some(o) = self.open.as_mut() else {
            match key.code {
                KeyCode::Esc => return Outcome::Close,
                KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
                KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(20),
                KeyCode::PageDown => self.cursor = (self.cursor + 20).min(n.saturating_sub(1)),
                KeyCode::Home => self.cursor = 0,
                KeyCode::End => self.cursor = n.saturating_sub(1),
                KeyCode::Enter if n > 0 => self.show(self.cursor),
                _ => {}
            }
            return Outcome::Stay;
        };
        let g = &o.game;
        match key.code {
            // A file of one game has no table to go back to.
            KeyCode::Esc if n == 1 => return Outcome::Close,
            KeyCode::Esc => self.open = None,
            KeyCode::Right => o.at = g.nodes[o.at as usize].children.first().copied().unwrap_or(o.at),
            KeyCode::Left => o.at = g.nodes[o.at as usize].parent.unwrap_or(o.at),
            KeyCode::Home => o.at = 0,
            KeyCode::End => o.at = g.end_of_line(o.at),
            KeyCode::Down | KeyCode::Up => {
                let s = g.siblings(o.at);
                let i = s.iter().position(|&x| x == o.at).unwrap_or(0);
                let j = if key.code == KeyCode::Down { (i + 1).min(s.len() - 1) } else { i.saturating_sub(1) };
                o.at = s[j];
            }
            KeyCode::PageDown | KeyCode::PageUp => {
                let i = o.index;
                let j = if key.code == KeyCode::PageDown { (i + 1).min(n.saturating_sub(1)) } else { i.saturating_sub(1) };
                if j != i {
                    self.show(j);
                }
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
            .title(Line::from(format!(" ● {title} ")).centered());
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
        let keys = if self.open.is_some() { t!("go.keys_game") } else { t!("go.keys_list") };
        f.render_widget(Paragraph::new(fit(&keys, w)).style(dim), foot);
        if let Some(s) = &self.said {
            f.render_widget(Paragraph::new(fit(s, w)).style(dim), said);
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
        let w = area.width as usize;
        // The list fills as the file is read: what is listed so far.
        let p = self.games.progress();
        let mut state = t!("chess.games", "shown" => p.games, "total" => p.games);
        if !p.done {
            let pct = (p.read * 100).checked_div(p.size).unwrap_or(0);
            state = format!("{state} · {}", t!("chess.reading", "percent" => pct));
        }
        if p.capped {
            state = format!("{state} · {}", t!("chess.capped", "n" => p.games));
        }
        f.render_widget(Paragraph::new(fit(&state, w)).style(dim), Rect { height: 1, ..area });
        let fixed = [5usize, 5, 5, 8, 10, 5, 5, 4, 5];
        let rest = w.saturating_sub(fixed.iter().sum::<usize>() + 11);
        let name = (rest * 30 / 100).max(6);
        let event = rest.saturating_sub(2 * name).max(4);
        let row = |cells: [String; 12]| -> String {
            let widths = [fixed[0], name, fixed[1], name, fixed[2], fixed[3], event, fixed[4], fixed[5], fixed[6], fixed[7], fixed[8]];
            cells.iter().zip(widths).map(|(c, cw)| fit(c, cw)).collect::<Vec<_>>().join(" ")
        };
        let head = row([
            "#".into(),
            t!("chess.col_black"),
            t!("go.col_rank"),
            t!("chess.col_white"),
            t!("go.col_rank"),
            t!("chess.col_result"),
            t!("chess.col_event"),
            t!("chess.col_date"),
            t!("go.col_size"),
            t!("go.col_komi"),
            t!("go.col_handicap"),
            t!("chess.col_moves"),
        ]);
        f.render_widget(Paragraph::new(fit(&head, w)).style(base.add_modifier(Modifier::BOLD)), Rect { y: area.y + 1, height: 1, ..area });
        let h = area.height.saturating_sub(2) as usize;
        if p.games == 0 {
            if !p.done {
                return;
            }
            f.render_widget(Paragraph::new(fit(&t!("go.no_games"), w)).style(dim), Rect { y: area.y + 2, height: 1, ..area });
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        for (k, g) in self.games.games(self.offset, h).into_iter().enumerate().map(|(k, g)| (k + self.offset, g)) {
            let event = [g.event.as_str(), g.name.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            let text = row([
                g.n.to_string(),
                g.black.clone(),
                g.black_rank.clone(),
                g.white.clone(),
                g.white_rank.clone(),
                g.result.clone(),
                event,
                g.date.clone(),
                g.size(),
                g.komi.clone(),
                g.handicap.clone(),
                g.moves.to_string(),
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
        let g = &o.game;
        let p = g.position(o.at);
        let (bw, bh) = (p.width, p.height);
        let branches = g.branches(o.at);
        // The moves that go on from here, marked A, B, … on their points.
        let letter = |x: u8, y: u8| -> Option<char> {
            branches.iter().position(|&c| g.nodes[c as usize].mv.and_then(|m| m.at) == Some((x, y))).map(|i| (b'A' + (i as u8 % 26)) as char)
        };
        let stars = go::stars(bw, bh);
        let wood = Style::default().bg(self.wood);
        let line = Style::default().bg(self.wood).fg(Color::Rgb(70, 50, 30));
        let (pieces, cols, rows) = crate::pieces::points(self.pieces, bw as u16, bh as u16, (area.width * 3 / 5).max(area.width.saturating_sub(40)).saturating_sub(8), area.height.saturating_sub(2));
        let mut lines: Vec<Line> = vec![];
        let mut placed = vec![];
        if pieces != Pieces::Figurines {
            // Stones drawn from block characters, or their pictures, on points of `cols` ×
            // `rows` cells; the letters and numbers over the middles of the points.
            use crate::pieces::{DOWN, LEFT, RIGHT, UP};
            let mut names = String::from("    ");
            for x in 0..bw {
                let name = go::point_name(bw, bh, x, 0);
                let c = name.split(|c: char| c.is_ascii_digit() && bw <= 25 || c == '-').next().unwrap_or("");
                let w = c.chars().count().min(cols as usize);
                let left = (cols as usize - w) / 2;
                names.push_str(&format!("{}{c}{}", " ".repeat(left), " ".repeat(cols as usize - w - left)));
            }
            lines.push(Line::styled(names.clone(), dim));
            for y in 0..bh {
                let row = bh as u32 - y as u32;
                let mut grids = vec![];
                for x in 0..bw {
                    let mut l = 0;
                    if y > 0 {
                        l |= UP;
                    }
                    if y + 1 < bh {
                        l |= DOWN;
                    }
                    if x > 0 {
                        l |= LEFT;
                    }
                    if x + 1 < bw {
                        l |= RIGHT;
                    }
                    let mid = if l == 15 && stars.contains(&(x, y)) { '╋' } else { crate::pieces::junction(l) };
                    let mut grid = crate::pieces::point(cols, rows, l, mid, line);
                    match (p.at(x, y), letter(x, y)) {
                        (s @ (1 | 2), _) => {
                            let disc = Shape::Disc { dark: s == 1 }.draw(crate::pieces::glyphs(pieces), cols as usize, rows as usize);
                            crate::pieces::put(&mut grid, &disc, self.wood);
                            // The last stone played: a red dot in its middle.
                            if p.last == Some((x, y)) {
                                let under = if s == 1 { Color::Rgb(0x16, 0x16, 0x16) } else { crate::pieces::light() };
                                crate::pieces::label(&mut grid, '●', Color::Rgb(200, 30, 30), under);
                            }
                            if pieces == Pieces::Pictures {
                                let pic = crate::pictures::Picture { lines: l, ink: (70, 50, 30), mark: p.last == Some((x, y)), ..crate::pictures::Picture::of(crate::pictures::go(s == 1)) };
                                placed.push((Rect::new(4 + x as u16 * cols, 1 + y as u16 * rows, cols, rows), pic, self.wood));
                            }
                        }
                        (_, Some(c)) => crate::pieces::label(&mut grid, c, Color::Rgb(180, 0, 0), self.wood),
                        _ => {}
                    }
                    grids.push(grid);
                }
                for sub in 0..rows as usize {
                    let mid = sub == rows as usize / 2;
                    let mut spans = vec![Span::styled(if mid { format!("{row:>3} ") } else { "    ".into() }, dim)];
                    spans.extend(crate::pieces::spans(&grids, sub));
                    spans.push(Span::styled(if mid { format!(" {row}") } else { String::new() }, dim));
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::styled(names, dim));
        }
        // Column letters above and below.
        let mut cols_line = String::from("    ");
        for x in 0..bw {
            let name = go::point_name(bw, bh, x, 0);
            let c = name.split(|c: char| c.is_ascii_digit() && bw <= 25 || c == '-').next().unwrap_or("");
            cols_line.push_str(&format!("{c:<2}"));
        }
        if pieces == Pieces::Figurines {
            lines.push(Line::styled(cols_line.clone(), dim));
            for y in 0..bh {
                let row = bh as u32 - y as u32;
                let mut spans = vec![Span::styled(format!("{row:>3} "), dim)];
                for x in 0..bw {
                    let stone = p.at(x, y);
                    let last = p.last == Some((x, y));
                    let mark = if last { "◉" } else { "●" };
                    let (glyph, style) = match (stone, letter(x, y)) {
                        (1, _) => (mark.to_string(), wood.fg(Color::Rgb(0, 0, 0)).add_modifier(Modifier::BOLD)),
                        (2, _) => (mark.to_string(), wood.fg(Color::Rgb(255, 255, 255)).add_modifier(Modifier::BOLD)),
                        (_, Some(c)) => (c.to_string(), wood.fg(Color::Rgb(180, 0, 0)).add_modifier(Modifier::BOLD)),
                        _ => {
                            let (l, r, t, b) = (x == 0, x + 1 == bw, y == 0, y + 1 == bh);
                            let g = match (t, b, l, r) {
                                (true, _, true, _) => "┌",
                                (true, _, _, true) => "┐",
                                (_, true, true, _) => "└",
                                (_, true, _, true) => "┘",
                                (true, ..) => "┬",
                                (_, true, ..) => "┴",
                                (_, _, true, _) => "├",
                                (_, _, _, true) => "┤",
                                _ if stars.contains(&(x, y)) => "╋",
                                _ => "┼",
                            };
                            (g.to_string(), line)
                        }
                    };
                    spans.push(Span::styled(glyph, style));
                    // The line on to the next point.
                    spans.push(Span::styled(if x + 1 < bw { "─" } else { " " }, line));
                }
                spans.push(Span::styled(format!(" {row}"), dim));
                lines.push(Line::from(spans));
            }
            lines.push(Line::styled(cols_line, dim));
        }
        let board_w = 4 + cols * bw as u16 + 4;
        f.render_widget(Paragraph::new(lines), Rect { width: board_w.min(area.width), ..area });
        if let Some(pics) = self.pictures.as_mut() {
            pics.place(f.buffer_mut(), (area.x, area.y), area, placed);
        }
        // Beside it: the game, then the move and what goes on from it.
        let right = Rect { x: area.x + board_w + 1, width: area.width.saturating_sub(board_w + 1), ..area };
        if right.width < 12 {
            return;
        }
        let w = right.width as usize;
        let i = &g.info;
        let mut out: Vec<Line> = vec![];
        let players = if i.black.is_empty() && i.white.is_empty() { String::new() } else { i.players() };
        out.push(Line::styled(fit(format!("{players}  {}", i.result).trim(), w), base.add_modifier(Modifier::BOLD)));
        let about = [i.name.clone(), i.event.clone(), i.round.clone(), i.place.clone(), i.date.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        if !about.is_empty() {
            out.push(Line::styled(fit(&about, w), dim));
        }
        let mut facts = vec![format!("{} {}", t!("go.col_size"), i.size())];
        if !i.komi.is_empty() {
            facts.push(format!("{} {}", t!("go.col_komi"), i.komi));
        }
        if !i.handicap.is_empty() && i.handicap != "0" {
            facts.push(format!("{} {}", t!("go.col_handicap"), i.handicap));
        }
        if !i.rules.is_empty() {
            facts.push(i.rules.clone());
        }
        out.push(Line::styled(fit(&facts.join(" · "), w), dim));
        out.push(Line::styled(fit(&t!("chess.game_of", "n" => o.index + 1, "total" => self.games.progress().games), w), dim));
        out.push(Line::from(""));
        let total = g.nodes[g.end_of_line(o.at) as usize].number;
        let mv = g.move_text(o.at);
        let now = if mv.is_empty() { t!("go.start") } else { format!("{mv}  ({})", t!("go.move_of", "n" => p.number, "total" => total)) };
        out.push(Line::styled(fit(&now, w), sty(&theme.dialog_input)));
        out.push(Line::styled(fit(&t!(if p.black_next { "chess.black_to_move" } else { "chess.white_to_move" }), w), dim));
        out.push(Line::styled(fit(&t!("go.captures", "black" => p.captures[0], "white" => p.captures[1]), w), base));
        let sibs = g.siblings(o.at);
        if sibs.len() > 1 {
            let k = sibs.iter().position(|&s| s == o.at).unwrap_or(0);
            out.push(Line::styled(fit(&t!("go.variation_of", "n" => k + 1, "total" => sibs.len()), w), base));
        }
        if !branches.is_empty() {
            let list: Vec<String> = branches.iter().enumerate().map(|(k, &c)| format!("{} {}", (b'A' + (k as u8 % 26)) as char, g.move_text(c))).collect();
            out.push(Line::styled(fit(&t!("go.branches", "list" => list.join(", ")), w), base));
        }
        let node = &g.nodes[o.at as usize];
        if !node.name.is_empty() {
            out.push(Line::styled(fit(&node.name, w), base.add_modifier(Modifier::BOLD)));
        }
        let used = out.len() as u16;
        f.render_widget(Paragraph::new(out), Rect { height: used.min(right.height), ..right });
        if !node.comment.is_empty() && right.height > used + 1 {
            let at = Rect { y: right.y + used + 1, height: right.height - used - 1, ..right };
            f.render_widget(Paragraph::new(node.comment.as_str()).style(dim.add_modifier(Modifier::ITALIC)).wrap(Wrap { trim: false }), at);
        }
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
        let d = std::env::temp_dir().join(format!("coxswain-tui-go-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.sgf");
        std::fs::write(&p, "(;SZ[9]PB[Kim]PW[Lee]KM[6.5];B[ee];W[ec]C[A comment](;B[cc])(;B[gc]))(;SZ[13]PB[Two];B[aa])").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        // The list is read on a thread: all of it, then the table.
        v.games.wait(usize::MAX);
        v.poll();
        assert!(v.open.is_none(), "two games: the table first");
        v.key(key(KeyCode::Enter), None);
        for _ in 0..2 {
            v.key(key(KeyCode::Right), None);
        }
        v.pieces = Pieces::Figurines;
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("Kim") && screen.contains("A comment") && screen.contains("2 ○ E7") && screen.contains("A 3 ● C7"), "{screen}");
        assert!(screen.contains('●') && screen.contains('┼') && screen.contains('A'));
        // With room: stones of block characters on points of 7 × 3, the next moves' letters.
        v.pieces = Pieces::Mosaic(coxswain_core::mosaic::Glyphs::Sextants);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let b = term.backend().buffer();
        let s: String = (0..40).map(|y| (0..160).map(|x| b[(x, y)].symbol()).collect::<String>() + "\n").collect();
        assert!(s.contains('●') && s.contains('╋') && s.contains("─┼─") && s.contains('A') && s.chars().any(|c| ('\u{1FB00}'..='\u{1FB3B}').contains(&c)), "{s}");
        v.pieces = Pieces::Figurines;
        // Into the variation and on.
        v.key(key(KeyCode::Right), None);
        v.key(key(KeyCode::Down), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.move_text(o.at), "3 ● G7");
        v.key(key(KeyCode::PageDown), None);
        assert_eq!(v.open.as_ref().unwrap().game.info.black, "Two");
        v.key(key(KeyCode::Esc), None);
        assert!(v.open.is_none());
        assert!(matches!(v.key(key(KeyCode::Esc), None), Outcome::Close));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

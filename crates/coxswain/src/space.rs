//! Space, full screen (Alt+U, F9 → Space view): where the bytes under a folder go. The folder
//! is walked on a thread (coxswain-core's `space`), with its count on screen and Esc to stop;
//! then a treemap in coloured half-blocks, every file a rectangle by its bytes, and beside it the
//! bytes by type or the largest files. The desktop app shows the same.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::settings::{count, human};
use coxswain_core::space::{self, Kind, Opts, Progress, Rect as SRect, Tile, Tree, TypeRow};
use coxswain_core::t;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::bom::faint;
use crate::ui::{fit, sty};

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Close, and put the panel's cursor on this file.
    Go(PathBuf),
    /// Hand this file to View, Edit or Delete; the view comes back after.
    Hand(Action, PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Map,
    Types,
    Largest,
}

/// The layout for one size and zoom, kept until either changes.
struct Laid {
    size: (u16, u16),
    zoom: u32,
    tiles: Vec<Tile>,
    types: Vec<TypeRow>,
    largest: Vec<u32>,
}

pub struct Viewer {
    root: PathBuf,
    scan: Option<(Arc<Progress>, mpsc::Receiver<Option<Tree>>, Instant)>,
    tree: Option<Tree>,
    took: Duration,
    zoom: u32,
    /// The tile under the cursor: its id and kind (a small block's id is its folder's).
    cursor: Option<(u32, Kind)>,
    focus: Focus,
    /// The list beside the map: by type, or the largest files.
    side: Focus,
    rows: [usize; 2],
    /// The extension whose rectangles stand out.
    highlight: Option<String>,
    laid: Option<Laid>,
    /// The keys of View, Edit, Delete and Refresh, for the key line.
    labels: [String; 4],
}

/// How many files the Largest list has.
const LARGEST: usize = 100;

impl Viewer {
    pub fn new(root: PathBuf, labels: [String; 4]) -> Viewer {
        let mut v = Viewer {
            root,
            scan: None,
            tree: None,
            took: Duration::ZERO,
            zoom: space::ROOT,
            cursor: None,
            focus: Focus::Map,
            side: Focus::Types,
            rows: [0, 0],
            highlight: None,
            laid: None,
            labels,
        };
        v.measure();
        v
    }

    /// Walk the folder anew, on a thread.
    fn measure(&mut self) {
        let p = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let (root, progress) = (self.root.clone(), p.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Tree::scan(&root, &progress));
        });
        self.scan = Some((p, rx, Instant::now()));
    }

    /// The walk has ended: its tree in.
    fn poll(&mut self) {
        let Some((_, rx, at)) = &self.scan else { return };
        if let Ok(tree) = rx.try_recv() {
            self.took = at.elapsed();
            self.scan = None;
            if let Some(tree) = tree {
                self.tree = Some(tree);
                self.zoom = space::ROOT;
                self.cursor = None;
                self.laid = None;
            }
        }
    }

    pub fn stop(&self) {
        if let Some((p, ..)) = &self.scan {
            p.stop.store(true, Ordering::Relaxed);
        }
    }

    /// `path` is gone: out of the tree, and the map laid out again.
    pub fn gone(&mut self, path: &Path) {
        if let Some(t) = &mut self.tree
            && t.remove(path)
        {
            self.cursor = None;
            self.laid = None;
        }
    }

    fn tiles(&self) -> &[Tile] {
        self.laid.as_ref().map_or(&[], |l| &l.tiles)
    }

    fn tile(&self) -> Option<&Tile> {
        let (id, kind) = self.cursor?;
        self.tiles().iter().find(|t| t.id == id && t.kind == kind)
    }

    /// The deepest file or small block at (x, y).
    fn at(&self, x: f32, y: f32) -> Option<&Tile> {
        self.tiles().iter().rev().find(|t| t.kind != Kind::Folder && t.rect.contains(x, y))
    }

    /// The cursor to the next rectangle that way.
    fn step(&mut self, dx: f32, dy: f32) {
        let Some(t) = self.tile().copied() else {
            self.cursor = self.tiles().iter().find(|t| t.kind != Kind::Folder).map(|t| (t.id, t.kind));
            return;
        };
        let r = t.rect;
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = match (dx, dy) {
            (d, _) if d > 0.0 => (r.x + r.w + 0.5, cy),
            (d, _) if d < 0.0 => (r.x - 0.5, cy),
            (_, d) if d > 0.0 => (cx, r.y + r.h + 0.5),
            _ => (cx, r.y - 0.5),
        };
        if let Some(n) = self.at(x, y) {
            self.cursor = Some((n.id, n.kind));
        }
    }

    /// The file the keys are about: the cursor's on the map, the row's in Largest.
    fn file(&self) -> Option<PathBuf> {
        let t = self.tree.as_ref()?;
        match self.focus {
            Focus::Largest => self.laid.as_ref()?.largest.get(self.rows[1]).map(|&i| t.path(i)),
            _ => match self.cursor? {
                (id, Kind::File) => Some(t.path(id)),
                _ => None,
            },
        }
    }

    /// One level into what is under the cursor; a small block's folder.
    fn zoom_in(&mut self) {
        let (Some(t), Some((id, _))) = (&self.tree, self.cursor) else { return };
        if let Some(next) = t.toward(self.zoom, id).filter(|&n| t.node(n).dir).or((id != self.zoom && t.node(id).dir).then_some(id)) {
            self.zoom = next;
            self.laid = None;
        }
    }

    fn zoom_out(&mut self) {
        if let Some(t) = &self.tree
            && self.zoom != space::ROOT
        {
            self.zoom = t.node(self.zoom).parent;
            self.laid = None;
            self.cursor = None;
        }
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if self.scan.is_some() {
            if key.code == KeyCode::Esc || action == Some(Action::Quit) {
                self.stop();
                return Outcome::Close;
            }
            return Outcome::Stay;
        }
        let list = |v: &Viewer| v.laid.as_ref().map_or(0, |l| if v.focus == Focus::Types { l.types.len() } else { l.largest.len() });
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
            _ => None,
        };
        match key.code {
            KeyCode::Esc if self.highlight.is_some() => self.highlight = None,
            KeyCode::Esc => return Outcome::Close,
            _ if action == Some(Action::Quit) => return Outcome::Close,
            KeyCode::Tab => {
                self.focus = match (self.focus, key.shift) {
                    (Focus::Map, false) | (Focus::Largest, true) => Focus::Types,
                    (Focus::Types, false) | (Focus::Map, true) => Focus::Largest,
                    _ => Focus::Map,
                };
                if self.focus != Focus::Map {
                    self.side = self.focus;
                }
            }
            KeyCode::Backspace => {
                self.zoom_out();
            }
            _ if ch == Some('-') => {
                self.zoom_out();
            }
            _ if ch == Some('+') || ch == Some('=') => {
                if self.focus == Focus::Map {
                    self.zoom_in();
                }
            }
            _ if action == Some(Action::Refresh) => {
                self.measure();
            }
            _ if matches!(action, Some(Action::View | Action::Edit | Action::Delete)) => {
                if let (Some(a), Some(p)) = (action, self.file()) {
                    return Outcome::Hand(a, p);
                }
            }
            KeyCode::Enter => match self.focus {
                Focus::Types => {
                    let ext = self.laid.as_ref().and_then(|l| l.types.get(self.rows[0])).map(|r| r.ext.clone());
                    self.highlight = if self.highlight == ext { None } else { ext };
                }
                Focus::Largest => {
                    if let Some(p) = self.file() {
                        return Outcome::Go(p);
                    }
                }
                Focus::Map => match self.cursor {
                    Some((_, Kind::Small)) => self.zoom_in(),
                    // Another disk: Space on it, walked now.
                    Some((id, Kind::Mount)) => {
                        if let Some(t) = &self.tree {
                            self.root = t.path(id);
                            self.tree = None;
                            self.measure();
                        }
                    }
                    _ => {
                        if let Some(p) = self.file() {
                            return Outcome::Go(p);
                        }
                    }
                },
            },
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right if self.focus == Focus::Map => match key.code {
                KeyCode::Up => self.step(0.0, -1.0),
                KeyCode::Down => self.step(0.0, 1.0),
                KeyCode::Left => self.step(-1.0, 0.0),
                _ => self.step(1.0, 0.0),
            },
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
                let n = list(self);
                let i = &mut self.rows[(self.focus == Focus::Largest) as usize];
                let by: isize = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::PageUp => -15,
                    KeyCode::PageDown => 15,
                    KeyCode::Home => isize::MIN / 2,
                    _ => isize::MAX / 2,
                };
                *i = (*i as isize + by).clamp(0, n.saturating_sub(1) as isize) as usize;
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The layout for a map of `w` × `h` cells, made when the size or the zoom changed.
    fn lay(&mut self, w: u16, h: u16) {
        let Some(t) = &self.tree else { return };
        if self.laid.as_ref().is_some_and(|l| l.size == (w, h) && l.zoom == self.zoom) {
            return;
        }
        // In half-cells, which are near square: a cell is about twice as tall as wide.
        let area = SRect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 * 2.0 };
        let tiles = t.layout(self.zoom, area, &Opts { pad: 0.0, head: 2.0, min_area: 4.0 });
        // The lists hang on the zoom alone: a resize keeps them.
        let (types, largest) = match self.laid.take() {
            Some(l) if l.zoom == self.zoom => (l.types, l.largest),
            _ => (t.types(self.zoom), t.largest(self.zoom, LARGEST)),
        };
        self.rows = [self.rows[0].min(types.len().saturating_sub(1)), self.rows[1].min(largest.len().saturating_sub(1))];
        self.laid = Some(Laid { size: (w, h), zoom: self.zoom, tiles, types, largest });
        if self.tile().is_none() {
            self.cursor = self.tiles().iter().find(|t| t.kind != Kind::Folder).map(|t| (t.id, t.kind));
        }
    }

    /// `path` shown from the folder Space was opened on: `~/projects/rocket`.
    fn shown(&self, path: &Path) -> String {
        let home = coxswain_core::dirs::home_dir().unwrap_or_default();
        match path.strip_prefix(&home) {
            Ok(rest) if !home.as_os_str().is_empty() => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()).trim_end_matches(std::path::MAIN_SEPARATOR).to_string(),
            _ => path.display().to_string(),
        }
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        self.poll();
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let title = match &self.tree {
            Some(t) if self.scan.is_none() => {
                let n = t.node(space::ROOT);
                t!("space.title", "folder" => self.shown(&self.root), "size" => human(n.size), "files" => coxswain_core::tn!("space.files", n.files as usize, "n" => count(n.files as usize)))
            }
            _ => t!("space.title_measuring", "folder" => self.shown(&self.root)),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(bg))
            .style(base)
            .title(Line::from(format!(" {title} ")).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area);
        f.render_widget(block, area);
        // The map takes 20 columns and its gap one: what is left is the side's.
        if inner.height < 6 || inner.width < 22 {
            return;
        }
        let w = inner.width as usize;
        let foot = Rect { y: inner.y + inner.height - 1, height: 1, ..inner };
        let info = Rect { y: foot.y - 1, ..foot };
        let body = Rect { y: inner.y + 1, height: inner.height - 3, ..inner };
        let crumbs = Rect { height: 1, ..inner };

        if let Some((p, _, at)) = &self.scan {
            let (files, bytes) = p.now();
            let said = format!("{} · {} s", t!("space.measuring", "files" => count(files as usize), "size" => human(bytes)), at.elapsed().as_secs());
            let mid = Rect { y: body.y + body.height / 2, height: 1, ..body };
            f.render_widget(Paragraph::new(said).style(base).centered(), mid);
            f.render_widget(Paragraph::new(fit(&format!(" {}", t!("space.keys_measuring")), w)).style(dim), foot);
            return;
        }
        if self.tree.is_none() {
            f.render_widget(Paragraph::new(t!("space.stopped")).style(dim).centered(), body);
            f.render_widget(Paragraph::new(fit(&format!(" {}", t!("space.keys_stopped", "refresh" => &self.labels[3])), w)).style(dim), foot);
            return;
        }

        let side_w = (inner.width / 3).clamp(24, 48).min(inner.width.saturating_sub(20));
        let map = Rect { width: inner.width - side_w - 1, ..body };
        let side = Rect { x: map.x + map.width + 1, width: side_w, ..body };
        self.lay(map.width, map.height);

        // Where the zoom is, from the top.
        let t = self.tree.as_ref().unwrap();
        let zoomed = t.node(self.zoom);
        let mut crumb = self.shown(&t.path(self.zoom));
        if self.zoom != space::ROOT {
            crumb = format!("{crumb}  ·  {}", human(zoomed.size));
        }
        f.render_widget(Paragraph::new(fit(&format!(" {crumb}"), w)).style(base.add_modifier(Modifier::BOLD)), crumbs);

        // On the panels' colour: Classic blue's blue, not the dialogs' grey.
        self.paint(f.buffer_mut(), map, sty(&theme.panel).bg.unwrap_or(bg));
        self.draw_side(f, side, base, dim, theme);

        let t = self.tree.as_ref().unwrap();
        let about = match (self.focus, self.cursor) {
            (Focus::Largest, _) => self.laid.as_ref().and_then(|l| l.largest.get(self.rows[1])).map(|&i| describe(t, i)),
            (_, Some((id, Kind::File))) => Some(describe(t, id)),
            (_, Some((id, Kind::Mount))) => Some(format!("{} · {}", t!("space.other_disk", "name" => self.shown(&t.path(id))), t!("space.other_disk_tip"))),
            (_, Some((id, _))) => Some(t!("space.small", "folder" => self.shown(&t.path(id)))),
            _ => None,
        };
        let mut about = about.unwrap_or_default();
        if t.online > 0 {
            about = format!("{about}  ·  {}", t!("space.online", "count" => count(t.online as usize)));
        }
        f.render_widget(Paragraph::new(fit(&format!(" {about}"), w)).style(base), info);
        let keys = t!("space.keys", "view" => &self.labels[0], "edit" => &self.labels[1], "delete" => &self.labels[2], "refresh" => &self.labels[3]);
        f.render_widget(Paragraph::new(fit(&format!(" {keys}"), w)).style(dim), foot);
    }

    /// The treemap, a flat colour per rectangle: whole cells, so no half-block banding. Each
    /// file's last column is a darker edge and its last row ends in a darker line (▁), so
    /// neighbours of one colour stay apart; folders carry a lighter title row.
    fn paint(&self, buf: &mut Buffer, map: Rect, bg: Color) {
        let (w, h) = (map.width as usize, map.height as usize);
        let folder = blend(rgb(bg), (255, 255, 255), 0.34);
        // Each cell: its colour, and a line along its bottom (a darker colour) or none.
        let mut cells: Vec<(Rgb, Option<Rgb>)> = vec![(rgb(bg), None); w * h];
        let tiles = self.tiles();
        let cursor = self.tile().copied();
        let t = self.tree.as_ref().unwrap();
        // The layout is in half-cells (rows count twice): back to whole cells.
        let cell = |r: &SRect| {
            let (x0, y0) = ((r.x.round().max(0.0) as usize).min(w), ((r.y / 2.0).round().max(0.0) as usize).min(h));
            let (x1, y1) = (((r.x + r.w).round() as usize).min(w), (((r.y + r.h) / 2.0).round() as usize).min(h));
            (x0, y0, x1, y1)
        };
        for tile in tiles {
            let (x0, y0, x1, y1) = cell(&tile.rect);
            let lit = cursor.is_some_and(|c| c.id == tile.id && c.kind == tile.kind);
            let off = self.highlight.as_ref().is_some_and(|e| tile.kind != Kind::File || *e != t.ext(tile.id));
            let mut colour = match tile.kind {
                Kind::Folder => folder,
                Kind::Small => (70, 78, 88),
                // Another disk: blue-grey, not to scale.
                Kind::Mount => (74, 92, 138),
                Kind::File => rgb_hex(tile.family.colour()),
            };
            if off {
                colour = blend(colour, (0, 0, 0), 0.7);
            }
            if lit {
                colour = blend(colour, (255, 255, 255), 0.5);
            }
            let edge = shade(colour, 0.62);
            let framed = tile.kind != Kind::Folder;
            for y in y0..y1 {
                for x in x0..x1 {
                    let right = framed && x + 1 == x1 && x1 - x0 > 1;
                    let low = framed && y + 1 == y1 && y1 - y0 > 1;
                    cells[y * w + x] = (if right { edge } else { colour }, low.then_some(edge));
                }
            }
        }
        for row in 0..h {
            for col in 0..w {
                let (back, line) = cells[row * w + col];
                if let Some(cell) = buf.cell_mut((map.x + col as u16, map.y + row as u16)) {
                    match line {
                        Some(l) => cell.set_symbol("▁").set_style(Style::default().fg(col_of(l)).bg(col_of(back))),
                        None => cell.set_symbol(" ").set_style(Style::default().bg(col_of(back))),
                    };
                }
            }
        }
        // Names only where they fit: on a folder's title row, on files and blocks wide enough.
        for tile in tiles {
            let (x0, y0, x1, y1) = cell(&tile.rect);
            if y1 <= y0 || x1 <= x0 + 3 || (tile.kind == Kind::Folder && tile.rect.h < 6.0) {
                continue;
            }
            let n = t.node(tile.id);
            let room = x1.saturating_sub(x0 + 1 + (tile.kind != Kind::Folder) as usize);
            let label = match tile.kind {
                Kind::Mount => fitting(&t!("space.other_disk", "name" => n.label()), "", room),
                Kind::Small => fitting(&coxswain_core::tn!("space.files", tile.files as usize, "n" => count(tile.files as usize)), "", room),
                Kind::Folder => fitting(&format!("{} {} {}", '\u{f07b}', t.label(tile.id), human(n.size)), &format!("{} {}", '\u{f07b}', t.label(tile.id)), room),
                Kind::File => fitting(&format!("{} {}", t.label(tile.id), human(n.size)), &t.label(tile.id), room),
            };
            let Some(label) = label else { continue };
            let (back, _) = cells[y0 * w + x0 + 1];
            let fg = if luma(back) > 140.0 { Color::Black } else { Color::White };
            buf.set_stringn(map.x + x0 as u16 + 1, map.y + y0 as u16, &label, room, Style::default().fg(fg).bg(col_of(back)));
        }
    }

    fn draw_side(&self, f: &mut Frame, side: Rect, base: Style, dim: Style, theme: &Theme) {
        let Some(laid) = &self.laid else { return };
        let t = self.tree.as_ref().unwrap();
        let w = side.width as usize;
        let tab = |on: bool, s: String| if on { ratatui::text::Span::styled(format!(" {s} "), sty(&theme.dialog_input)) } else { ratatui::text::Span::styled(format!(" {s} "), base) };
        let head = Line::from(vec![tab(self.side == Focus::Types, t!("space.by_type")), ratatui::text::Span::styled("│", dim), tab(self.side == Focus::Largest, t!("space.largest"))]);
        f.render_widget(Paragraph::new(head), Rect { height: 1, ..side });
        let rows = Rect { y: side.y + 1, height: side.height.saturating_sub(1), ..side };
        let h = rows.height as usize;
        let focused = self.focus == self.side;
        let i = self.rows[(self.side == Focus::Largest) as usize];
        let top = i.saturating_sub(h.saturating_sub(1));
        let n = if self.side == Focus::Types { laid.types.len() } else { laid.largest.len() };
        for k in top..n.min(top + h) {
            let at = Rect { y: rows.y + (k - top) as u16, height: 1, ..rows };
            let style = if k == i && focused { sty(&theme.dialog_input) } else { base };
            let (swatch, text) = if self.side == Focus::Types {
                let r = &laid.types[k];
                let ext = if r.ext.is_empty() { t!("space.no_ext") } else { format!(".{}", r.ext) };
                let mark = if self.highlight.as_deref() == Some(&r.ext) { "▶" } else { " " };
                let right = format!("{:>9} {:>5}", human(r.bytes), percent(r.share));
                (r.family.colour(), format!("{mark}{} {right}", fit(&ext, w.saturating_sub(right.len() + 4))))
            } else {
                let id = laid.largest[k];
                let size = format!("{:>9} ", human(t.node(id).size));
                let rel = t.path(id).strip_prefix(t.path(self.zoom)).map(|p| p.display().to_string()).unwrap_or_default();
                // A model store's blob: the model it is (the line under the map has its path).
                let rel = if t.label(id) != t.node(id).label() { t.label(id).into_owned() } else { rel };
                (t.family(id).colour(), format!(" {size}{}", crate::ui::fit_left(&rel, w.saturating_sub(size.len() + 3))))
            };
            let line = Line::from(vec![ratatui::text::Span::styled("■", style.fg(col_of(rgb_hex(swatch)))), ratatui::text::Span::styled(fit(&text, w.saturating_sub(1)), style)]);
            f.render_widget(Paragraph::new(line), at);
        }
    }
}

/// A label in `room` columns: `full` (a name and its size) when it fits, else `name` whole, else
/// `name` cut with at least eight letters left; none at all rather than a crumb like "Music 68…".
fn fitting(full: &str, name: &str, room: usize) -> Option<String> {
    use unicode_width::UnicodeWidthStr;
    let name = if name.is_empty() { full } else { name };
    if full.width() <= room {
        return Some(full.into());
    }
    if name.width() <= room {
        return Some(name.into());
    }
    let cut = fit(name, room);
    let cut = cut.trim_end();
    (cut.chars().count() > 8).then(|| cut.to_string())
}

/// A share in per cent for By type: one decimal under 10 %, "<0.1%" rather than 0 for a sliver.
fn percent(share: f64) -> String {
    let p = share * 100.0;
    if share > 0.0 && p < 0.1 {
        "<0.1%".into()
    } else if p < 10.0 && p > 0.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// A file in one line: its path, size and kind.
fn describe(t: &Tree, id: u32) -> String {
    let n = t.node(id);
    let ext = t.ext(id);
    let kind = t.family(id).label();
    let kind = if ext.is_empty() { kind } else { format!("{kind} (.{ext})") };
    let model = if t.label(id) != n.label() { format!("{}  ·  ", t.label(id)) } else { String::new() };
    format!("{model}{}  ·  {}  ·  {kind}", t.path(id).display(), human(n.size))
}

type Rgb = (u8, u8, u8);

fn rgb(c: Color) -> Rgb {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn rgb_hex(h: &str) -> Rgb {
    let n = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0);
    ((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn col_of((r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

fn shade((r, g, b): Rgb, k: f32) -> Rgb {
    let s = |c: u8| (c as f32 * k).clamp(0.0, 255.0) as u8;
    (s(r), s(g), s(b))
}

fn blend(a: Rgb, b: Rgb, k: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn luma((r, g, b): Rgb) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    #[test]
    fn measures_then_moves_zooms_and_hands_files_on() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-space-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("photos")).unwrap();
        fs::write(d.join("film.mkv"), vec![0u8; 8000]).unwrap();
        fs::write(d.join("photos/a.jpg"), vec![0u8; 3000]).unwrap();
        fs::write(d.join("photos/b.png"), vec![0u8; 1000]).unwrap();
        let labels = ["F3", "F4", "F8", "Ctrl+R"].map(String::from);
        let mut v = Viewer::new(d.clone(), labels);
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let start = Instant::now();
        while v.tree.is_none() && start.elapsed() < Duration::from_secs(20) {
            term.draw(|f| v.draw(f, &Theme::default())).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("film.mkv") && screen.contains(".jpg") && !screen.contains("▀"), "{screen}");
        // The cursor starts at the top left: the largest file.
        assert!(matches!(v.key(key(KeyCode::Enter), None), Outcome::Go(p) if p == d.join("film.mkv")));
        assert!(matches!(v.key(key(KeyCode::F(8)), Some(Action::Delete)), Outcome::Hand(Action::Delete, p) if p == d.join("film.mkv")));
        // Right: into photos; + zooms there, Backspace back.
        v.key(key(KeyCode::Right), None);
        assert!(matches!(v.file(), Some(p) if p.starts_with(d.join("photos"))));
        v.key(key(KeyCode::Char('+')), None);
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        assert_eq!(v.tiles().iter().filter(|t| t.kind == Kind::File).count(), 2);
        v.key(key(KeyCode::Backspace), None);
        assert_eq!(v.zoom, space::ROOT);
        // Largest, by Tab twice; Enter goes to the row's file.
        v.key(key(KeyCode::Tab), None);
        v.key(key(KeyCode::Tab), None);
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        v.key(key(KeyCode::Down), None);
        assert!(matches!(v.key(key(KeyCode::Enter), None), Outcome::Go(p) if p == d.join("photos/a.jpg")));
        // A delete outside: gone from the map and the totals.
        fs::remove_file(d.join("film.mkv")).unwrap();
        v.gone(&d.join("film.mkv"));
        assert_eq!(v.tree.as_ref().unwrap().node(space::ROOT).size, 4000);
        // Narrow terminals: drawn without a panic, at every width from 20 columns to 30.
        for w in 20..=30 {
            let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 12)).unwrap();
            term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        }
        assert!(matches!(v.key(key(KeyCode::Esc), None), Outcome::Close));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn labels_fit_whole_or_not_at_all_and_slivers_show_a_share() {
        assert_eq!(fitting("Music 687 MB", "Music", 20).as_deref(), Some("Music 687 MB"));
        assert_eq!(fitting("Music 687 MB", "Music", 9).as_deref(), Some("Music"), "the size goes first, not cut");
        assert_eq!(fitting("old-laptop-2019 761 MB", "old-laptop-2019", 12).as_deref(), Some("old-laptop-…"));
        assert_eq!(fitting("06 Track six.flac 46 MB", "06 Track six.flac", 6), None, "no crumbs");
        assert_eq!(percent(0.3), "30%");
        assert_eq!(percent(0.006), "0.6%");
        assert_eq!(percent(0.000_01), "<0.1%");
        assert_eq!(percent(0.0), "0%");
    }
}

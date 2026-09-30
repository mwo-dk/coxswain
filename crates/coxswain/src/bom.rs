//! The BOM viewer (docs/design/bom-viewer.md): F3 on a CycloneDX BOM opens it full screen as a
//! rated tree, or as a sunburst drawn with half blocks, with filters, details and a compare with
//! the file in the other panel. F3 again shows the source in the pager, as before.
//!
//! Reading and rating are coxswain-core's `bom`; the desktop app shows the same thing
//! (gui/src/BomView.svelte), with the same catalogue texts.

use coxswain_core::bom::assess::{Reason, Unresolved};
use coxswain_core::bom::diff::{self, Change, Diff};
use coxswain_core::bom::view::{self, Arc, Filter, Loaded};
use coxswain_core::bom::{self as cbom, tree, EdgeKind, NodeKind, Status, TreeMode};
use coxswain_core::config::{Key, KeyCode, Theme};
use coxswain_core::{t, tn};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

use crate::ui::{fit, sty};

/// The statuses keys 1 to 7 toggle, worst first, as the desktop app's chips.
const STATUS_KEYS: [Status; 7] =
    [Status::Broken, Status::Disallowed, Status::Deprecated, Status::Unknown, Status::Acceptable, Status::Safe, Status::NotRated];
/// What `k` cycles through, after "all kinds".
const KINDS: [NodeKind; 5] = [NodeKind::Algorithm, NodeKind::Certificate, NodeKind::Protocol, NodeKind::Material, NodeKind::Component];
/// Children a node shows before a "more" row.
const PAGE: usize = 500;

/// Ratings keep their colours in every theme, and the status word says it too.
fn color(s: Status) -> Color {
    use coxswain_core::bom::status::Color as C;
    match s.color() {
        C::Green => Color::Rgb(0x43, 0xa0, 0x47),
        C::Yellow => Color::Rgb(0xf9, 0xa8, 0x25),
        C::Red => Color::Rgb(0xe5, 0x39, 0x35),
        C::Grey | C::Muted => Color::Rgb(0x9e, 0x9e, 0x9e),
    }
}

/// Secondary text: the theme's colour for hidden files, on the dialog's own background (the
/// hidden style's background is the panel's).
fn faint(theme: &Theme) -> Style {
    sty(&theme.hidden).fg.map_or_else(Style::default, |c| Style::default().fg(c))
}

fn status_word(s: Status) -> String {
    t!(&format!("bom.status.{}", s.name()))
}

fn kind_key(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Application => "application",
        NodeKind::Group => "group",
        NodeKind::Component => "component",
        NodeKind::Algorithm => "algorithm",
        NodeKind::Certificate => "certificate",
        NodeKind::Protocol => "protocol",
        NodeKind::Material => "material",
    }
}

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Show the BOM's source in the pager, and come back.
    Source,
    /// Close, and put the panel's cursor on this file.
    Reveal(PathBuf),
    /// Compare with the file under the other panel's cursor.
    Compare,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Node(u32),
    /// The "n more" row under a node.
    More(u32),
}

struct Row {
    at: Cursor,
    depth: usize,
    /// For a "more" row: how many more.
    more: usize,
}

struct Compared {
    /// The older version, and what the compare found (or why there is nothing to compare).
    old: Option<PathBuf>,
    name: String,
    result: Result<Diff, String>,
}

pub struct Viewer {
    pub path: PathBuf,
    l: Loaded,
    /// Per tree node: its name as shown, and what the search looks in.
    labels: Vec<String>,
    text: Vec<String>,
    open: HashSet<u32>,
    shown: HashMap<u32, usize>,
    cursor: Cursor,
    offset: usize,
    filter: Filter,
    /// 0 is all kinds, else `KINDS[kind - 1]`.
    kind: usize,
    hide: bool,
    /// The search line has the keys.
    typing: bool,
    only_changes: bool,
    sunburst: bool,
    zoom: u32,
    compare: Option<Compared>,
    show_removed: bool,
    matches: Vec<bool>,
    keep: Vec<bool>,
    rows: Vec<Row>,
    details_scroll: u16,
}

impl Viewer {
    pub fn open(path: &Path) -> Result<Viewer, cbom::Error> {
        let l = Loaded::open(path, None)?;
        let mut v = Viewer {
            path: path.to_path_buf(),
            l,
            labels: vec![],
            text: vec![],
            open: HashSet::new(),
            shown: HashMap::new(),
            cursor: Cursor::Node(0),
            offset: 0,
            filter: Filter::default(),
            kind: 0,
            hide: false,
            typing: false,
            only_changes: false,
            sunburst: false,
            zoom: 0,
            compare: None,
            show_removed: false,
            matches: vec![],
            keep: vec![],
            rows: vec![],
            details_scroll: 0,
        };
        v.laid_out();
        Ok(v)
    }

    /// After the tree changed (a new file or grouping): names, what is open, filters.
    fn laid_out(&mut self) {
        let n = self.l.tree.len() as u32;
        self.labels = (0..n).map(|i| self.label(i)).collect();
        self.text = (0..n).map(|i| format!("{}\n{}", self.l.search_text(i), self.labels[i as usize].to_lowercase())).collect();
        // The first two levels, unless that makes a long list.
        let kids = &self.l.tree.children[0];
        let second: usize = kids.len() + kids.iter().map(|&c| self.l.tree.children[c as usize].len().min(PAGE)).sum::<usize>();
        self.open = if second <= 1000 { std::iter::once(0).chain(kids.iter().copied()).collect() } else { [0].into() };
        self.shown.clear();
        self.cursor = Cursor::Node(0);
        self.zoom = 0;
        self.refresh();
    }

    fn label(&self, i: u32) -> String {
        if let Some((kind, alg)) = self.l.key_of(i) {
            let key = format!("bom.material.{kind}");
            let name = t!(&key);
            let name = if name == key { t!("bom.material.key") } else { name };
            return t!("bom.key_of", "type" => name, "algorithm" => alg);
        }
        if let Some(k) = self.l.group_kind(i) {
            return t!(&format!("bom.kind.{}", kind_key(k)));
        }
        self.l.node(i).label.clone()
    }

    fn change(&self, i: u32) -> Change {
        match &self.compare {
            Some(Compared { result: Ok(d), .. }) => d.change.get(i as usize).copied().unwrap_or_default(),
            _ => Change::Unchanged,
        }
    }

    fn filtering(&self) -> bool {
        self.filter.is_active() || self.only_changes
    }

    /// Filters, then the rows the tree shows.
    fn refresh(&mut self) {
        self.filter.kind = if self.kind == 0 { HashSet::new() } else { [KINDS[self.kind - 1]].into() };
        let (mut matches, mut keep) = view::mask(&self.l, &self.filter, &self.text);
        if self.only_changes {
            for (i, m) in matches.iter_mut().enumerate() {
                *m = *m && view::is_crypto(self.l.node(i as u32).kind) && self.change(i as u32) != Change::Unchanged;
            }
            keep = matches.clone();
            for &i in self.l.tree.order.iter().rev() {
                if keep[i as usize]
                    && let Some(p) = self.l.tree.parent[i as usize]
                {
                    keep[p as usize] = true;
                }
            }
        }
        (self.matches, self.keep) = (matches, keep);
        self.rows = self.visible_rows();
        if !self.rows.iter().any(|r| r.at == self.cursor) {
            self.cursor = self.rows.first().map_or(Cursor::Node(0), |r| r.at);
        }
    }

    fn hiding(&self) -> bool {
        self.hide && self.filtering()
    }

    fn is_open(&self, i: u32) -> bool {
        self.open.contains(&i)
            || (self.hiding() && i != 0 && self.keep[i as usize] && self.l.tree.children[i as usize].iter().any(|&c| self.keep[c as usize]))
    }

    fn visible_rows(&self) -> Vec<Row> {
        let mut out = vec![];
        let mut stack = vec![(Cursor::Node(0), 0, 0)];
        while let Some((at, depth, more)) = stack.pop() {
            out.push(Row { at, depth, more });
            let Cursor::Node(i) = at else { continue };
            if !self.is_open(i) {
                continue;
            }
            let kids: Vec<u32> =
                self.l.tree.children[i as usize].iter().copied().filter(|&c| !self.hiding() || self.keep[c as usize]).collect();
            let limit = self.shown.get(&i).copied().unwrap_or(PAGE);
            if kids.len() > limit {
                stack.push((Cursor::More(i), depth + 1, kids.len() - limit));
            }
            for &c in kids.iter().take(limit).rev() {
                stack.push((Cursor::Node(c), depth + 1, 0));
            }
        }
        out
    }

    fn selected(&self) -> u32 {
        match self.cursor {
            Cursor::Node(i) | Cursor::More(i) => i,
        }
    }

    /// Selects a node and opens what it sits in, so the tree shows it.
    fn select(&mut self, i: u32) {
        let mut p = self.l.tree.parent[i as usize];
        while let Some(q) = p {
            self.open.insert(q);
            p = self.l.tree.parent[q as usize];
        }
        self.cursor = Cursor::Node(i);
        self.details_scroll = 0;
        self.refresh();
    }

    fn row_index(&self) -> usize {
        self.rows.iter().position(|r| r.at == self.cursor).unwrap_or(0)
    }

    fn go(&mut self, to: isize) {
        let to = to.clamp(0, self.rows.len() as isize - 1) as usize;
        if let Some(r) = self.rows.get(to) {
            self.cursor = r.at;
            self.details_scroll = 0;
        }
    }

    /// The first file of the selected node that is next to the BOM.
    fn file_of(&self, i: u32) -> Option<PathBuf> {
        let node = self.l.node(i);
        node.occurrences
            .iter()
            .find_map(|o| view::on_disk(&self.path, &o.location))
            .or_else(|| node.group.as_ref().and_then(|g| g.path.as_deref()).and_then(|p| view::on_disk(&self.path, p)))
    }

    fn set_mode(&mut self, mode: TreeMode) {
        let keep = self.l.node(self.selected()).key.clone();
        self.l = Loaded::new(self.l.bom.clone(), Some(mode));
        let old = self.compare.take();
        self.laid_out();
        if let Some(i) = (0..self.l.tree.len() as u32).find(|&i| self.l.node(i).key == keep) {
            self.select(i);
        }
        // A compare is by tree node, so it is made again for the new tree.
        if let Some(Compared { old: Some(old), .. }) = old {
            self.compare(&old);
        }
    }

    /// Compares with an older version of this BOM: `old` is the "before".
    pub fn compare(&mut self, old: &Path) {
        let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let result = Loaded::open(old, Some(self.l.mode)).map_err(|e| e.to_string()).map(|before| diff::diff(&before.side(), &self.l.side()));
        self.compare = Some(Compared { old: Some(old.to_path_buf()), name, result });
        self.refresh();
    }

    /// Shows why there is nothing to compare with.
    pub fn cannot_compare(&mut self, why: String) {
        self.compare = Some(Compared { old: None, name: String::new(), result: Err(why) });
    }

    fn stop_compare(&mut self) {
        self.compare = None;
        self.only_changes = false;
        self.show_removed = false;
        self.refresh();
    }

    /// Handles a key. `quit` is whether the key is bound to Quit.
    pub fn key(&mut self, key: Key, quit: bool) -> Outcome {
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        if self.typing {
            match (key.code, ch) {
                (KeyCode::Enter | KeyCode::Esc | KeyCode::Down | KeyCode::Tab, _) => self.typing = false,
                (KeyCode::Backspace, _) => {
                    self.filter.query.pop();
                }
                (_, Some(c)) => self.filter.query.push(c),
                _ => {}
            }
            self.refresh();
            return Outcome::Stay;
        }
        match (key.code, ch) {
            (KeyCode::Esc, _) => return Outcome::Close,
            _ if quit => return Outcome::Close,
            (KeyCode::F(3), _) | (_, Some('s')) => return Outcome::Source,
            (KeyCode::Tab, _) => {
                self.sunburst = !self.sunburst;
                if !self.sunburst {
                    self.select(self.selected());
                }
            }
            (_, Some(c @ '1'..='7')) => {
                let s = STATUS_KEYS[c as usize - '1' as usize];
                if !self.filter.status.remove(&s) {
                    self.filter.status.insert(s);
                }
                self.refresh();
            }
            (_, Some('0')) => {
                self.filter = Filter::default();
                self.kind = 0;
                self.only_changes = false;
                self.refresh();
            }
            (_, Some('k')) => {
                self.kind = (self.kind + 1) % (KINDS.len() + 1);
                self.refresh();
            }
            (_, Some('/')) => self.typing = true,
            (_, Some('h')) => {
                self.hide = !self.hide;
                self.refresh();
            }
            (_, Some('m')) => {
                let modes = tree::modes(&self.l.bom);
                let next = modes[(modes.iter().position(|&m| m == self.l.mode).unwrap_or(0) + 1) % modes.len()];
                if next != self.l.mode {
                    self.set_mode(next);
                }
            }
            (_, Some('c')) if self.compare.is_some() => self.stop_compare(),
            (_, Some('c')) => return Outcome::Compare,
            (_, Some('n')) if self.compare.is_some() => {
                self.only_changes = !self.only_changes;
                self.refresh();
            }
            (_, Some('r')) if self.compare.is_some() => self.show_removed = !self.show_removed,
            (_, Some('d')) => self.details_scroll += 3,
            (_, Some('u')) => self.details_scroll = self.details_scroll.saturating_sub(3),
            _ if self.sunburst => self.sunburst_key(key.code),
            _ => return self.tree_key(key.code),
        }
        Outcome::Stay
    }

    fn tree_key(&mut self, code: KeyCode) -> Outcome {
        let at = self.row_index() as isize;
        let i = self.selected();
        let has_kids = !self.l.tree.children[i as usize].is_empty();
        match code {
            KeyCode::Down => self.go(at + 1),
            KeyCode::Up => self.go(at - 1),
            KeyCode::PageDown => self.go(at + 15),
            KeyCode::PageUp => self.go(at - 15),
            KeyCode::Home => self.go(0),
            KeyCode::End => self.go(self.rows.len() as isize - 1),
            KeyCode::Right if matches!(self.cursor, Cursor::Node(_)) && has_kids => {
                if self.is_open(i) {
                    self.go(at + 1);
                } else {
                    self.open.insert(i);
                    self.refresh();
                }
            }
            KeyCode::Left => {
                if matches!(self.cursor, Cursor::Node(_)) && self.is_open(i) && i != 0 {
                    self.open.remove(&i);
                    self.refresh();
                } else if let Some(p) = if matches!(self.cursor, Cursor::More(_)) { Some(i) } else { self.l.tree.parent[i as usize] } {
                    self.cursor = Cursor::Node(p);
                }
            }
            KeyCode::Enter => match self.cursor {
                Cursor::More(p) => {
                    *self.shown.entry(p).or_insert(PAGE) += PAGE;
                    self.refresh();
                }
                Cursor::Node(i) => match self.file_of(i) {
                    Some(f) => return Outcome::Reveal(f),
                    None if has_kids => {
                        if !self.open.remove(&i) {
                            self.open.insert(i);
                        }
                        self.refresh();
                    }
                    None => {}
                },
            },
            _ => {}
        }
        Outcome::Stay
    }

    /// In the sunburst the cursor walks the tree: ←→ between neighbours, ↑ out, ↓ in; Enter zooms
    /// into the selected node and Backspace out.
    fn sunburst_key(&mut self, code: KeyCode) {
        let i = self.selected();
        let tree = &self.l.tree;
        let weight = self.leaves();
        let siblings = |p: u32| -> Vec<u32> { tree.children[p as usize].iter().copied().filter(|&c| weight[c as usize] > 0.0).collect() };
        match code {
            KeyCode::Left | KeyCode::Right => {
                if let Some(p) = tree.parent[i as usize] {
                    let s = siblings(p);
                    if let Some(at) = s.iter().position(|&x| x == i) {
                        let n = s.len();
                        let to = if code == KeyCode::Right { (at + 1) % n } else { (at + n - 1) % n };
                        self.cursor = Cursor::Node(s[to]);
                    }
                }
            }
            KeyCode::Up => {
                if i == self.zoom
                    && let Some(p) = tree.parent[i as usize]
                {
                    self.zoom = p;
                }
                if let Some(p) = tree.parent[i as usize] {
                    self.cursor = Cursor::Node(p);
                }
            }
            KeyCode::Down => {
                if let Some(&c) = siblings(i).first() {
                    self.cursor = Cursor::Node(c);
                }
            }
            KeyCode::Enter if !siblings(i).is_empty() => self.zoom = i,
            KeyCode::Backspace => {
                if let Some(p) = tree.parent[self.zoom as usize] {
                    self.cursor = Cursor::Node(self.zoom);
                    self.zoom = p;
                }
            }
            _ => return,
        }
        self.details_scroll = 0;
    }

    fn leaves(&self) -> Vec<f64> {
        view::leaf_counts(&self.l.tree, self.hiding().then_some(self.keep.as_slice()))
    }

    // ------------------------------------------------------------ drawing

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(ratatui::layout::Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shape = if self.sunburst { t!("bom.sunburst") } else { t!("preview.tree") };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(base.bg.unwrap_or(Color::Reset)))
            .style(base)
            .title(Line::from(format!(" {name} · {shape} ")).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
        f.render_widget(block, area);

        let compare_h = self.compare.is_some() as u16;
        let details_h = (inner.height / 3).max(5);
        let [chips, controls, compare, body, rule, details, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(compare_h),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(details_h),
            Constraint::Length(1),
        ])
        .areas(inner);

        f.render_widget(Paragraph::new(self.chips_line(theme)), chips);
        f.render_widget(Paragraph::new(self.controls_line(theme)), controls);
        if let Some(c) = &self.compare {
            f.render_widget(Paragraph::new(self.compare_line(c, theme)), compare);
        }
        if self.sunburst {
            self.draw_sunburst(f, body, theme);
        } else {
            self.draw_tree(f, body, theme);
        }
        f.render_widget(Paragraph::new("─".repeat(rule.width as usize)).style(sty(&theme.dialog_border)), rule);
        let lines = if self.show_removed { self.removed_lines(theme) } else { self.details(theme) };
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.details_scroll, 0)), details);
        let hint = if self.sunburst { t!("tui.bom.sun_keys") } else { t!("tui.bom.keys") };
        f.render_widget(Paragraph::new(fit(&hint, foot.width as usize)).style(faint(theme).bg(base.bg.unwrap_or(Color::Reset))), foot);
    }

    fn counts(&self) -> HashMap<Status, usize> {
        let mut by = HashMap::new();
        for i in 0..self.l.bom.nodes.len() as u32 {
            if view::is_crypto(self.l.node(i).kind) {
                *by.entry(self.l.status(i)).or_insert(0) += 1;
            }
        }
        by
    }

    fn chips_line(&self, theme: &Theme) -> Line<'static> {
        let counts = self.counts();
        let on = sty(&theme.dialog_input);
        let mut spans = vec![];
        for (k, s) in STATUS_KEYS.iter().enumerate() {
            let Some(n) = counts.get(s) else { continue };
            let chosen = self.filter.status.contains(s);
            let style = if chosen { on } else { Style::default() };
            spans.push(Span::styled(format!("{} ", k + 1), faint(theme)));
            spans.push(Span::styled("●", style.fg(color(*s))));
            spans.push(Span::styled(format!(" {} {n}", status_word(*s)), style));
            spans.push(Span::raw("  "));
        }
        if counts.is_empty() {
            spans.push(Span::raw(t!("bom.no_crypto")));
        }
        Line::from(spans)
    }

    fn controls_line(&self, theme: &Theme) -> Line<'static> {
        let dim = faint(theme);
        let kind = if self.kind == 0 { t!("tui.bom.all_kinds") } else { t!(&format!("bom.kind.{}", kind_key(KINDS[self.kind - 1]))) };
        let search = if self.typing {
            Span::styled(format!(" {}▏", self.filter.query), sty(&theme.dialog_input))
        } else if self.filter.query.is_empty() {
            Span::styled(format!(" {}", t!("bom.search")), dim)
        } else {
            Span::raw(format!(" {}", self.filter.query))
        };
        Line::from(vec![
            Span::styled("k ", dim),
            Span::raw(kind),
            Span::styled("   / ", dim),
            search,
            Span::styled("   h ", dim),
            Span::raw(if self.hide { t!("bom.hide") } else { t!("bom.dim") }),
            Span::styled("   m ", dim),
            Span::raw(t!(&format!("bom.mode.{}", mode_key(self.l.mode)))),
        ])
    }

    fn compare_line(&self, c: &Compared, theme: &Theme) -> Line<'static> {
        let dim = faint(theme);
        let mut spans = vec![Span::raw(t!("bom.compared", "name" => c.name)), Span::raw(" ")];
        match &c.result {
            Err(e) => spans.push(Span::styled(e.clone(), Style::default().fg(color(Status::Broken)))),
            Ok(d) => {
                let k = d.counts;
                spans.push(Span::styled(tn!("bom.change.risk", k.new_risks), Style::default().fg(color(Status::Broken))));
                for text in [
                    tn!("bom.change.fixed", k.fixed),
                    tn!("bom.change.added", k.added),
                    tn!("bom.change.worsened", k.worsened),
                    tn!("bom.change.improved", k.improved),
                    tn!("bom.change.removed", k.removed),
                ] {
                    spans.push(Span::styled(" · ", dim));
                    spans.push(Span::raw(text));
                }
                spans.push(Span::styled(format!("   n {}  r {}  c ×", t!("tui.bom.only_changes"), t!("tui.bom.removed_list")), dim));
            }
        }
        Line::from(spans)
    }

    fn draw_tree(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let h = area.height as usize;
        let at = self.row_index();
        if at < self.offset {
            self.offset = at;
        } else if at >= self.offset + h {
            self.offset = at + 1 - h;
        }
        let dim = faint(theme);
        let dirs = Style::default().add_modifier(Modifier::BOLD);
        let selected = sty(&theme.dialog_input);
        let w = area.width as usize;
        for (k, row) in self.rows.iter().enumerate().skip(self.offset).take(h) {
            let y = area.y + (k - self.offset) as u16;
            let indent = "  ".repeat(row.depth);
            let line = match row.at {
                Cursor::More(_) => Line::from(vec![Span::raw(format!("{indent}  ")), Span::styled(tn!("bom.more", row.more), dim)]),
                Cursor::Node(i) => {
                    let node = self.l.node(i);
                    let has = !self.l.tree.children[i as usize].is_empty();
                    let twist = if !has { "  " } else if self.is_open(i) { "▾ " } else { "▸ " };
                    let s = self.l.status(i);
                    // faded, unless it is the cursor's row, whose colours would swallow it
                    let faded = self.filtering() && !self.hide && !self.matches[i as usize] && row.at != self.cursor;
                    let mut left = vec![Span::raw(format!("{indent}{twist}")), Span::styled("● ", Style::default().fg(color(s)))];
                    match self.change(i) {
                        Change::Added => left.push(Span::styled("+ ", Style::default().add_modifier(Modifier::BOLD))),
                        Change::Worsened => left.push(Span::styled("▲ ", Style::default().fg(color(Status::Broken)))),
                        Change::Improved => left.push(Span::styled("▼ ", Style::default().fg(color(Status::Acceptable)))),
                        Change::Unchanged => {}
                    }
                    let group = matches!(node.kind, NodeKind::Group | NodeKind::Application | NodeKind::Component);
                    left.push(Span::styled(self.labels[i as usize].clone(), if group { dirs } else { Style::default() }));
                    if view::is_crypto(node.kind) {
                        left.push(Span::styled(format!("  {}", status_word(s)), Style::default().fg(color(s))));
                    }
                    let place = node.occurrences.first().map(|o| {
                        let file = o.location.rsplit('/').next().unwrap_or(&o.location);
                        o.line.map_or_else(|| file.to_string(), |l| format!("{file}:{l}"))
                    });
                    let used: usize = left.iter().map(|s| s.content.width()).sum();
                    if let Some(p) = place.filter(|p| used + p.width() + 2 < w) {
                        left.push(Span::raw(" ".repeat(w - used - p.width())));
                        left.push(Span::styled(p, dim));
                    }
                    let mut line = Line::from(left);
                    if faded {
                        line = line.style(dim);
                    }
                    line
                }
            };
            let style = if row.at == self.cursor { selected } else { Style::default() };
            f.render_widget(Paragraph::new(line).style(style), Rect { y, height: 1, ..area });
        }
        if self.filtering() && !self.keep[0] {
            f.render_widget(Paragraph::new(t!("bom.no_match")).style(dim), Rect { y: area.y, height: 1, ..area });
        }
    }

    /// Two pixels a cell (▀ with fore- and background), each coloured by the arc under it.
    fn draw_sunburst(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let crumbs: Vec<String> = {
            let mut v = vec![];
            let mut at = Some(self.zoom);
            while let Some(i) = at {
                v.push(self.labels[i as usize].clone());
                at = self.l.tree.parent[i as usize];
            }
            v.reverse();
            v
        };
        let [top, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        f.render_widget(Paragraph::new(fit(&crumbs.join(" › "), top.width as usize)).style(faint(theme)), top);

        let leaves = self.leaves();
        let arcs = view::sunburst(&self.l, &leaves, self.zoom, view::RINGS, view::MIN_ANGLE);
        let rings = arcs.iter().map(|a| a.depth).max().unwrap_or(1);
        let mut by_ring: Vec<Vec<&Arc>> = vec![vec![]; rings + 1];
        for a in &arcs {
            by_ring[a.depth].push(a);
        }
        for r in &mut by_ring {
            r.sort_by(|a, b| a.a0.total_cmp(&b.a0));
        }
        let bg = sty(&theme.dialog).bg.unwrap_or(Color::Reset);
        let (w, h) = (rest.width as f64, rest.height as f64 * 2.0);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let r_max = (w / 2.0).min(h / 2.0) - 0.5;
        let r0 = r_max * 0.2;
        let ring_w = (r_max - r0) / rings as f64;
        let selected = self.selected();
        // Grey (unknown) would vanish on a light grey dialog, as the Norton theme's: darker there.
        let (br, bgc, bb) = rgb(bg);
        let light = 0.299 * br + 0.587 * bgc + 0.114 * bb > 128.0 && bg != Color::Reset;
        let paint = |s: Status| match (s.color(), light) {
            (coxswain_core::bom::status::Color::Grey | coxswain_core::bom::status::Color::Muted, true) => Color::Rgb(0x61, 0x61, 0x61),
            _ => color(s),
        };
        let dimmed = |a: &Arc| self.filtering() && !self.hide && a.node.is_some_and(|i| !self.keep[i as usize]);

        let pixel = |x: f64, y: f64| -> Color {
            let (dx, dy) = (x + 0.5 - cx, y + 0.5 - cy);
            let r = (dx * dx + dy * dy).sqrt();
            if r > r_max {
                return bg;
            }
            if r < r0 {
                return paint(self.l.status(self.zoom));
            }
            let ring = (((r - r0) / ring_w) as usize + 1).min(rings);
            let mut angle = dx.atan2(-dy).to_degrees();
            if angle < 0.0 {
                angle += 360.0;
            }
            let list = &by_ring[ring];
            let at = list.partition_point(|a| a.a1 <= angle);
            let Some(a) = list.get(at).filter(|a| a.a0 <= angle) else { return bg };
            let mut c = paint(a.status);
            if at % 2 == 1 {
                c = shade(c, 0.82);
            }
            if a.node.is_none() {
                c = shade(c, 0.6);
            }
            if dimmed(a) {
                c = shade(c, 0.3);
            }
            if a.node == Some(selected) {
                c = tint(c);
            }
            c
        };
        let buf = f.buffer_mut();
        for row in 0..rest.height {
            for col in 0..rest.width {
                let (top, bottom) = (pixel(col as f64, row as f64 * 2.0), pixel(col as f64, row as f64 * 2.0 + 1.0));
                if let Some(cell) = buf.cell_mut((rest.x + col, rest.y + row)) {
                    cell.set_symbol("▀").set_fg(top).set_bg(bottom);
                }
            }
        }
    }

    /// Why the selected node has its rating, what to do, and where it is found.
    fn details(&self, theme: &Theme) -> Vec<Line<'static>> {
        let dim = faint(theme);
        let Cursor::Node(i) = self.cursor else { return vec![] };
        let node = self.l.node(i);
        let a = self.l.assessed.explain(i);
        let mut out = vec![Line::from(vec![
            Span::styled("● ", Style::default().fg(color(a.status))),
            Span::styled(self.labels[i as usize].clone(), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(format!(" · {}", status_word(a.status)), dim),
        ])];
        let p = coxswain_core::bom::policy::policy();
        let mut advice = vec![];
        if !a.reasons.is_empty() {
            out.push(Line::styled(t!("bom.why"), dim));
        }
        for r in &a.reasons {
            out.push(Line::from(format!("  • {}", self.why(r))));
            if let Reason::Rule { params, evaluation } = r {
                if let Some(s) = &evaluation.source {
                    out.push(Line::styled(format!("    {}", t!("bom.why.source", "source" => s.text)), dim));
                }
                if !matches!(evaluation.status, Status::Safe | Status::Acceptable) {
                    advice.extend(p.remediation(params, &self.l.ctx.profile).into_iter().map(|x| x.summary.clone()));
                }
            }
        }
        advice.dedup();
        if !advice.is_empty() {
            out.push(Line::styled(t!("bom.advice"), dim));
            out.extend(advice.into_iter().map(|x| Line::from(format!("  • {x}"))));
        }
        if !node.occurrences.is_empty() {
            out.push(Line::styled(t!("bom.found_in"), dim));
            let mut first = true;
            for o in &node.occurrences {
                let place = o.line.map_or_else(|| o.location.clone(), |l| format!("{}:{l}", o.location));
                let here = view::on_disk(&self.path, &o.location).is_some();
                let note = if here && first && !self.sunburst {
                    first = false;
                    format!("  ← {}", t!("tui.bom.enter_shows"))
                } else if here {
                    String::new()
                } else {
                    format!("  ({})", t!("bom.not_next_to_bom"))
                };
                out.push(Line::from(vec![Span::raw(format!("  {place}")), Span::styled(note, dim)]));
                if let Some(c) = &o.context {
                    out.push(Line::styled(format!("    {c}"), dim));
                }
            }
        }
        if i == 0 {
            let catalog = p.catalog();
            let profile = catalog.profiles.get(&self.l.ctx.profile).map_or(self.l.ctx.profile.clone(), |x| x.name.clone());
            let key = if catalog.last_reviewed.is_some() { "bom.ratings_note_reviewed" } else { "bom.ratings_note" };
            out.push(Line::styled(t!(key, "profile" => profile, "year" => self.l.ctx.year), dim));
            let issues: Vec<_> = self.l.bom.issues.iter().chain(&self.l.issues).collect();
            if !issues.is_empty() {
                out.push(Line::styled(tn!("bom.issues", issues.len()), dim));
                out.extend(issues.iter().take(50).map(|x| Line::styled(format!("  {}", x.message), dim)));
            }
        }
        out
    }

    fn removed_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let Some(Compared { result: Ok(d), .. }) = &self.compare else { return vec![] };
        let dim = faint(theme);
        let mut out = vec![Line::styled(tn!("bom.change.removed", d.removed.len()), dim)];
        for r in d.removed.iter().take(500) {
            out.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(color(r.status))),
                Span::raw(r.label.clone()),
                Span::styled(format!("  {}", status_word(r.status)), Style::default().fg(color(r.status))),
                Span::styled(format!("  {}", r.place), dim),
            ]));
        }
        out
    }

    /// One reason, in words: the desktop app's (BomView.svelte), from the same catalogue keys.
    fn why(&self, r: &Reason) -> String {
        let name = |i: &u32| self.labels[*i as usize].clone();
        match r {
            Reason::Rule { params, evaluation } => {
                let p = coxswain_core::bom::policy::policy();
                let algorithm = p.algorithm(&params.algorithm_id).map_or_else(|| params.algorithm_id.clone(), |a| a.name.clone());
                if !evaluation.missing.is_empty() {
                    let what: Vec<String> = evaluation.missing.iter().map(|m| t!(&format!("bom.missing.{}", param_key(*m)))).collect();
                    return t!("bom.why.missing", "algorithm" => algorithm, "what" => what.join(", "));
                }
                let mut parts = vec![];
                if let Some(b) = params.key_bits {
                    parts.push(t!("bom.why.key_bits", "bits" => b));
                }
                if let Some(b) = params.security_bits {
                    parts.push(t!("bom.why.security_bits", "bits" => b));
                }
                if let Some(s) = &params.param_set {
                    parts.push(t!("bom.why.param_set", "set" => s));
                }
                let algorithm = if parts.is_empty() { algorithm } else { format!("{algorithm} ({})", parts.join(", ")) };
                t!("bom.why.rule", "algorithm" => algorithm, "status" => status_word(evaluation.status))
            }
            Reason::NotRated { primitive: Some(p) } => t!("bom.why.not_rated_primitive", "primitive" => p),
            Reason::NotRated { primitive: None } => t!("bom.why.not_rated"),
            Reason::Unresolved { why } => t!(match why {
                Unresolved::UnknownAlgorithm => "bom.why.unknown-algorithm",
                Unresolved::KeyNamesNoAlgorithm => "bom.why.key-names-no-algorithm",
                Unresolved::NoCertificateFacts => "bom.why.no-certificate-facts",
                Unresolved::NoCipherSuites => "bom.why.no-cipher-suites",
            }),
            Reason::Inherited { node, status } => t!("bom.why.inherited", "name" => name(node), "status" => status_word(*status)),
            Reason::Reference { edge, node, status } => {
                let key = match edge {
                    EdgeKind::SignedWith => "bom.why.signedWith",
                    EdgeKind::HasKey => "bom.why.hasKey",
                    _ => "bom.why.uses",
                };
                t!(key, "name" => name(node), "status" => status_word(*status))
            }
            Reason::Lifecycle { days_left, .. } if *days_left < 0 => tn!("bom.why.expired", -days_left),
            Reason::Lifecycle { days_left, .. } => tn!("bom.why.expires", *days_left),
            Reason::Rollup { node, status } => t!("bom.why.rollup", "name" => name(node), "status" => status_word(*status)),
        }
    }
}

fn param_key(p: coxswain_core::bom::policy::Param) -> &'static str {
    use coxswain_core::bom::policy::Param;
    match p {
        Param::KeyBits => "keyBits",
        Param::SecurityBits => "securityBits",
        Param::ParamSet => "paramSet",
    }
}

fn mode_key(m: TreeMode) -> &'static str {
    match m {
        TreeMode::Dependencies => "dependencies",
        TreeMode::Files => "files",
        TreeMode::Flat => "flat",
    }
}

fn rgb(c: Color) -> (f64, f64, f64) {
    match c {
        Color::Rgb(r, g, b) => (r as f64, g as f64, b as f64),
        _ => (158.0, 158.0, 158.0),
    }
}

/// Darker by `k` (0 to 1).
fn shade(c: Color, k: f64) -> Color {
    let (r, g, b) = rgb(c);
    Color::Rgb((r * k) as u8, (g * k) as u8, (b * k) as u8)
}

/// Halfway to white: the selected arc.
fn tint(c: Color) -> Color {
    let (r, g, b) = rgb(c);
    Color::Rgb(((r + 255.0) / 2.0) as u8, ((g + 255.0) / 2.0) as u8, ((b + 255.0) / 2.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../coxswain-core/src/bom/testdata").join(name)
    }

    /// The screen as text, one line a row.
    fn screen(v: &mut Viewer, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| v.draw(f, &Theme::nc())).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    fn press(v: &mut Viewer, code: KeyCode) -> Outcome {
        v.key(Key::new(code, false, false, false), false)
    }

    fn has(lines: &[String], text: &str) -> bool {
        lines.iter().any(|l| l.contains(text))
    }

    #[test]
    fn tree_filters_and_details() {
        let mut v = Viewer::open(&fixture("cbomkit/keycloak.cdx.json")).unwrap();
        let s = screen(&mut v, 120, 40);
        assert!(has(&s, "keycloak.cdx.json"));
        assert!(has(&s, "▾ ● keycloak"));
        assert!(has(&s, "Disallowed 3"));
        assert!(has(&s, "The worst below it is DSA: Disallowed"), "{}", s.join("\n"));

        // 2 shows what is disallowed; in hide mode only that and what it sits in
        press(&mut v, KeyCode::Char('2'));
        press(&mut v, KeyCode::Char('h'));
        let s = screen(&mut v, 120, 40);
        assert!(has(&s, "DSA  Disallowed"), "{}", s.join("\n"));
        assert!(!has(&s, "RSA-2048"));

        // down to DSA: its reason, source and advice
        while !matches!(v.cursor, Cursor::Node(i) if v.l.node(i).label == "DSA") {
            press(&mut v, KeyCode::Down);
        }
        let s = screen(&mut v, 120, 40);
        assert!(has(&s, "DSA (2048-bit key): Disallowed"), "{}", s.join("\n"));
        assert!(has(&s, "Source: FIPS 186-5"), "{}", s.join("\n"));
        assert!(has(&s, "What to do"));
        assert!(has(&s, "(Not next to this BOM)"));

        // the search, and 0 clears everything
        press(&mut v, KeyCode::Char('0'));
        press(&mut v, KeyCode::Char('/'));
        for c in "hmac".chars() {
            press(&mut v, KeyCode::Char(c));
        }
        press(&mut v, KeyCode::Enter);
        assert!(v.matches.iter().filter(|&&m| m).count() >= 2);
        assert!(matches!(press(&mut v, KeyCode::F(3)), Outcome::Source));
        assert!(matches!(press(&mut v, KeyCode::Esc), Outcome::Close));
    }

    #[test]
    fn sunburst_is_drawn_in_half_blocks() {
        let mut v = Viewer::open(&fixture("cbomkit/keycloak.cdx.json")).unwrap();
        press(&mut v, KeyCode::Tab);
        let s = screen(&mut v, 100, 40);
        assert!(s.iter().filter(|l| l.contains('▀')).count() > 10);
        // ↓ goes into the first ring, Enter zooms in, Backspace out
        press(&mut v, KeyCode::Down);
        let first = v.selected();
        assert_eq!(v.l.tree.parent[first as usize], Some(0));
        press(&mut v, KeyCode::Enter);
        assert_eq!(v.zoom, first);
        let s = screen(&mut v, 100, 40);
        assert!(has(&s, &format!("keycloak › {}", v.labels[first as usize])), "{}", s[3]);
        press(&mut v, KeyCode::Backspace);
        assert_eq!(v.zoom, 0);
        press(&mut v, KeyCode::Tab);
        assert!(!v.sunburst);
    }

    #[test]
    fn compare_with_an_older_scan() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-tui-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture("cbomkit/keycloak.cdx.json")).unwrap()).unwrap();
        // a month ago there was no DSA, nor the two keys for it
        let dsa = |c: &serde_json::Value| {
            c["name"] == "DSA" || c["evidence"]["occurrences"][0]["location"].as_str().is_some_and(|l| l.ends_with("DSAKeyValueType.java"))
        };
        doc["components"].as_array_mut().unwrap().retain(|c| !dsa(c));
        let old = dir.join("old.cdx.json");
        std::fs::write(&old, doc.to_string()).unwrap();

        let mut v = Viewer::open(&fixture("cbomkit/keycloak.cdx.json")).unwrap();
        assert!(matches!(press(&mut v, KeyCode::Char('c')), Outcome::Compare));
        v.compare(&old);
        let s = screen(&mut v, 140, 40);
        assert!(has(&s, "Compared with old.cdx.json: 3 new risks · 0 fixed · 3 added"), "{}", s.join("\n"));
        press(&mut v, KeyCode::Char('n'));
        let changed: Vec<&str> = (0..v.l.tree.len()).filter(|&i| v.matches[i]).map(|i| v.l.node(i as u32).label.as_str()).collect();
        assert_eq!(changed, ["key@b627000e-ed4e-449c-acb9-4e9547d6ee93", "DSA", "key@58af0705-bf7c-4abc-9f49-5b3ed8dd31ca"]);
        // another grouping compares again
        press(&mut v, KeyCode::Char('m'));
        assert!(matches!(&v.compare, Some(Compared { result: Ok(d), .. }) if d.change.len() == v.l.tree.len()));
        press(&mut v, KeyCode::Char('c'));
        assert!(v.compare.is_none());
        v.cannot_compare("nothing there".into());
        assert!(has(&screen(&mut v, 140, 40), "nothing there"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

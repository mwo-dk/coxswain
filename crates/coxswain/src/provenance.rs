//! The provenance viewer (docs/design/provenance-viewer.md): F3 on build provenance opens it
//! full screen as inputs → build → outputs, with the signer, the checks against the disk, and a
//! compare with the file in the other panel. F3 again shows the source in the pager, as before.
//!
//! Reading and checking are coxswain-core's `provenance`; the desktop app shows the same thing
//! (gui/src/ProvenanceView.svelte), with the same catalogue texts. Nothing is verified.

use coxswain_core::config::{Key, KeyCode, Theme};
use coxswain_core::provenance::check::{self, CannotCheck, Source, Subject};
use coxswain_core::provenance::diff::{self, Area, Diff, Kind};
use coxswain_core::provenance::view::{self, Fact};
use coxswain_core::provenance::{self, Attestations, Entry, Predicate, Provenance, Resource};
use coxswain_core::{t, tn};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use crate::bom::{faint, is_light};
use crate::ui::{date, fit, sty};

/// Inputs shown before a "more" row.
const INPUTS_SHOWN: usize = 8;
/// Columns side by side from this width; narrower, they are stacked.
const WIDE: u16 = 100;

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Show the file's source in the pager, and come back.
    Source,
    /// Close, and put the panel's cursor on this file.
    Reveal(PathBuf),
    /// Close, and show this folder (a checkout) in the panel.
    Cd(PathBuf),
    /// Compare with the file under the other panel's cursor.
    Compare,
    /// Open this CycloneDX file (a predicate written out) in the BOM viewer.
    Bom(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    In,
    Build,
    Out,
}

/// Fixed colours, as the BOM viewer's ratings, with a glyph beside each so colour is never the
/// only signal; darker on a light dialog.
#[derive(Clone, Copy)]
enum Tone {
    Green,
    Red,
    Grey,
}

fn tone(t: Tone, theme: &Theme) -> Style {
    let light = is_light(theme);
    Style::default().fg(match (t, light) {
        (Tone::Green, false) => Color::Rgb(0x43, 0xa0, 0x47),
        (Tone::Red, false) => Color::Rgb(0xe5, 0x39, 0x35),
        (Tone::Grey, false) => Color::Rgb(0x9e, 0x9e, 0x9e),
        (Tone::Green, true) => Color::Rgb(0x1b, 0x5e, 0x20),
        (Tone::Red, true) => Color::Rgb(0xb7, 0x1c, 0x1c),
        (Tone::Grey, true) => Color::Rgb(0x42, 0x42, 0x42),
    })
}

enum Msg {
    Subject(usize, Subject),
    Sources(Vec<Option<Source>>),
}

struct Compared {
    name: String,
    result: Result<Diff, String>,
}

pub struct Viewer {
    pub path: PathBuf,
    a: Attestations,
    at: usize,
    col: Col,
    row: usize,
    more_inputs: bool,
    /// The other panel's folder, where subjects are looked for too.
    other_dir: Option<PathBuf>,
    /// Per subject: its check, or none while it runs.
    subjects: Vec<Option<Subject>>,
    /// Per dependency: where its commit is (git sources, once looked up).
    sources: Vec<Option<Source>>,
    facts: Vec<Fact>,
    tx: Sender<(u64, Msg)>,
    rx: Receiver<(u64, Msg)>,
    /// Results of an earlier statement's checks are dropped.
    generation: u64,
    cancel: Arc<AtomicBool>,
    statement: bool,
    statement_lines: Vec<String>,
    scroll: u16,
    details_scroll: u16,
    compare: Option<Compared>,
    changes: bool,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Viewer {
    pub fn open(path: &Path, other_dir: Option<PathBuf>) -> Result<Viewer, provenance::Error> {
        let a = provenance::load(path)?;
        let (tx, rx) = mpsc::channel();
        let other_dir = other_dir.filter(|d| Some(d.as_path()) != path.parent());
        let mut v = Viewer {
            path: path.to_path_buf(),
            a,
            at: 0,
            col: Col::Build,
            row: 0,
            more_inputs: false,
            other_dir,
            subjects: vec![],
            sources: vec![],
            facts: vec![],
            tx,
            rx,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            statement: false,
            statement_lines: vec![],
            scroll: 0,
            details_scroll: 0,
            compare: None,
            changes: false,
        };
        v.show(0);
        Ok(v)
    }

    fn entry(&self) -> &Entry {
        &self.a.entries[self.at]
    }

    fn prov(&self) -> Option<&Provenance> {
        match &self.entry().statement.predicate {
            Predicate::Provenance(p) => Some(p),
            _ => None,
        }
    }

    /// Shows statement `at` and starts its checks; those of the last one stop.
    fn show(&mut self, at: usize) {
        self.at = at;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        let e = self.entry().clone();
        self.facts = view::facts(&e);
        self.statement_lines = serde_json::to_string_pretty(&e.statement.raw).unwrap_or_default().lines().map(str::to_string).collect();
        self.col = if self.prov().is_some() { Col::Build } else { Col::Out };
        (self.row, self.scroll, self.details_scroll, self.more_inputs) = (0, 0, 0, false);
        self.subjects = vec![None; e.statement.subjects.len()];
        self.sources = vec![];
        let (path, other, cancel, tx, generation) = (self.path.clone(), self.other_dir.clone(), self.cancel.clone(), self.tx.clone(), self.generation);
        let subjects = e.statement.subjects.clone();
        std::thread::spawn(move || {
            for (i, s) in subjects.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let r = check::subject(&path, other.as_deref(), s, Some(check::AUTO_LIMIT), &cancel);
                if tx.send((generation, Msg::Subject(i, r))).is_err() {
                    return;
                }
            }
        });
        if let Some(p) = self.prov().cloned() {
            let (path, other, tx) = (self.path.clone(), self.other_dir.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let s = p.dependencies.iter().map(|d| check::git_source(d).map(|g| check::source(&path, other.as_deref(), &g))).collect();
                let _ = tx.send((generation, Msg::Sources(s)));
            });
        }
    }

    /// A large subject, checked because the user asked.
    fn check_now(&mut self, i: usize) {
        self.subjects[i] = None;
        let (path, other, cancel, tx, generation) = (self.path.clone(), self.other_dir.clone(), self.cancel.clone(), self.tx.clone(), self.generation);
        let s = self.entry().statement.subjects[i].clone();
        std::thread::spawn(move || {
            let r = check::subject(&path, other.as_deref(), &s, None, &cancel);
            let _ = tx.send((generation, Msg::Subject(i, r)));
        });
    }

    /// Results of the checks, between events.
    pub fn poll(&mut self) {
        while let Ok((generation, msg)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            match msg {
                Msg::Subject(i, r) => {
                    if let Some(slot) = self.subjects.get_mut(i) {
                        *slot = Some(r);
                    }
                }
                Msg::Sources(s) => self.sources = s,
            }
        }
    }

    pub fn compare(&mut self, old: &Path) {
        let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let result = provenance::load(old).map(|before| diff::diff(&before, &self.a)).map_err(|e| e.to_string());
        self.compare = Some(Compared { name, result });
        self.changes = true;
        self.details_scroll = 0;
    }

    pub fn cannot_compare(&mut self, why: String) {
        self.compare = Some(Compared { name: String::new(), result: Err(why) });
    }

    // ------------------------------------------------------------ rows

    /// Dependencies by index, git sources first.
    fn inputs(&self) -> Vec<usize> {
        let Some(p) = self.prov() else { return vec![] };
        let git: Vec<bool> = p.dependencies.iter().map(|d| check::git_source(d).is_some()).collect();
        let mut v: Vec<usize> = (0..git.len()).filter(|&i| git[i]).collect();
        v.extend((0..git.len()).filter(|&i| !git[i]));
        v
    }

    fn shown_inputs(&self) -> (Vec<usize>, usize) {
        let all = self.inputs();
        if self.more_inputs || all.len() <= INPUTS_SHOWN {
            (all, 0)
        } else {
            let more = all.len() - INPUTS_SHOWN;
            (all[..INPUTS_SHOWN].to_vec(), more)
        }
    }

    /// Outputs: the subjects, then the byproducts (by index into each).
    fn outputs(&self) -> Vec<(bool, usize)> {
        let mut v: Vec<(bool, usize)> = (0..self.entry().statement.subjects.len()).map(|i| (false, i)).collect();
        v.extend((0..self.prov().map_or(0, |p| p.byproducts.len())).map(|i| (true, i)));
        v
    }

    fn columns(&self) -> Vec<Col> {
        if self.prov().is_some() { vec![Col::In, Col::Build, Col::Out] } else { vec![Col::Out] }
    }

    fn rows_in(&self, c: Col) -> usize {
        match c {
            Col::In => {
                let (shown, more) = self.shown_inputs();
                shown.len() + (more > 0) as usize
            }
            Col::Build => 1,
            Col::Out => self.outputs().len(),
        }
    }

    // ------------------------------------------------------------ keys

    pub fn key(&mut self, key: Key, quit: bool) -> Outcome {
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
            _ => None,
        };
        let cols = self.columns();
        let ci = cols.iter().position(|c| *c == self.col).unwrap_or(0);
        let rows = self.rows_in(self.col).max(1);
        let move_to = |v: &mut Viewer, c: Col, r: usize| {
            v.col = c;
            v.row = r.min(v.rows_in(c).max(1) - 1);
            v.details_scroll = 0;
        };
        match (key.code, ch) {
            (KeyCode::Esc, _) => return Outcome::Close,
            _ if quit => return Outcome::Close,
            (KeyCode::F(3), _) | (_, Some('s')) => return Outcome::Source,
            (KeyCode::Tab, _) => {
                self.statement = !self.statement;
                self.scroll = 0;
            }
            (KeyCode::Up, _) if self.statement => self.scroll = self.scroll.saturating_sub(1),
            (KeyCode::Down, _) if self.statement => self.scroll = self.scroll.saturating_add(1),
            (KeyCode::PageUp, _) if self.statement => self.scroll = self.scroll.saturating_sub(15),
            (KeyCode::PageDown, _) if self.statement => self.scroll = self.scroll.saturating_add(15),
            (KeyCode::Home, _) if self.statement => self.scroll = 0,
            (KeyCode::End, _) if self.statement => self.scroll = self.statement_lines.len().saturating_sub(1) as u16,
            (KeyCode::Right, _) if ci + 1 < cols.len() => move_to(self, cols[ci + 1], self.row),
            (KeyCode::Left, _) if ci > 0 => move_to(self, cols[ci - 1], self.row),
            (KeyCode::Down, _) => move_to(self, self.col, self.row + 1),
            (KeyCode::Up, _) => move_to(self, self.col, self.row.saturating_sub(1)),
            (KeyCode::PageDown, _) => move_to(self, self.col, self.row + 10),
            (KeyCode::PageUp, _) => move_to(self, self.col, self.row.saturating_sub(10)),
            (KeyCode::Home, _) => move_to(self, self.col, 0),
            (KeyCode::End, _) => move_to(self, self.col, rows - 1),
            (_, Some('[')) if self.at > 0 => self.show(self.at - 1),
            (_, Some(']')) if self.at + 1 < self.a.entries.len() => self.show(self.at + 1),
            (_, Some('c')) => {
                if self.compare.take().is_none() {
                    return Outcome::Compare;
                }
                self.changes = false;
            }
            (_, Some('n')) if self.compare.is_some() => {
                self.changes = !self.changes;
                self.details_scroll = 0;
            }
            (_, Some('o')) => {
                let name = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if let Ok(p) = view::bom_file(self.entry(), &name) {
                    return Outcome::Bom(p);
                }
            }
            (_, Some('d')) => self.details_scroll = self.details_scroll.saturating_add(3),
            (_, Some('u')) => self.details_scroll = self.details_scroll.saturating_sub(3),
            (KeyCode::Enter, _) => return self.activate(),
            _ => {}
        }
        Outcome::Stay
    }

    fn activate(&mut self) -> Outcome {
        match self.col {
            Col::Out => {
                let Some(&(false, i)) = self.outputs().get(self.row) else { return Outcome::Stay };
                match &self.subjects[i] {
                    Some(Subject::Large { .. }) => self.check_now(i),
                    Some(Subject::Matches { path, .. } | Subject::Differs { path, .. } | Subject::Unreadable { path, .. }) => {
                        return Outcome::Reveal(path.clone());
                    }
                    _ => {}
                }
            }
            Col::In => {
                let (shown, more) = self.shown_inputs();
                if self.row >= shown.len() && more > 0 {
                    self.more_inputs = true;
                } else if let Some(c) = shown.get(self.row).and_then(|&d| self.sources.get(d)).and_then(|s| s.as_ref()).and_then(checkout) {
                    return Outcome::Cd(c.to_path_buf());
                }
            }
            Col::Build => {}
        }
        Outcome::Stay
    }

    // ------------------------------------------------------------ drawing

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(ratatui::layout::Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shape = if self.statement { t!("provenance.statement") } else { t!("provenance.flow") };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(base.bg.unwrap_or(Color::Reset)))
            .style(base)
            .title(Line::from(format!(" {name} · {shape} ")).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
        f.render_widget(block, area);

        let head = self.head(theme);
        let details_h = (inner.height / 3).max(5);
        let [head_a, body, rule, details, foot] = Layout::vertical([
            Constraint::Length(head.len() as u16),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(details_h),
            Constraint::Length(1),
        ])
        .areas(inner);
        f.render_widget(Paragraph::new(head), head_a);
        if self.statement {
            let lines: Vec<Line> = self.statement_lines.iter().map(|l| Line::from(l.clone())).collect();
            f.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), body);
        } else if inner.width >= WIDE {
            self.draw_wide(f, body, theme);
        } else {
            self.draw_stacked(f, body, theme);
        }
        f.render_widget(Paragraph::new("─".repeat(rule.width as usize)).style(sty(&theme.dialog_border)), rule);
        let lines = if self.changes && self.compare.is_some() { self.change_lines(theme) } else { self.details(theme) };
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.details_scroll, 0)), details);
        let hint = t!("tui.provenance.keys");
        f.render_widget(Paragraph::new(fit(&hint, foot.width as usize)).style(faint(theme).bg(base.bg.unwrap_or(Color::Reset))), foot);
    }

    fn head(&self, theme: &Theme) -> Vec<Line<'static>> {
        let dim = faint(theme);
        let e = self.entry();
        let kind = match &e.statement.predicate {
            Predicate::Provenance(p) => t!("provenance.kind.slsa", "version" => if p.version == provenance::SlsaVersion::V1 { "1" } else { "0.2" }),
            Predicate::Vsa(_) => t!("provenance.kind.vsa"),
            Predicate::Bom { .. } => t!("provenance.kind.bom"),
            Predicate::Other { .. } => t!("provenance.kind.other", "type" => e.statement.predicate_type),
        };
        let mut first = vec![Span::styled(kind, Style::default().add_modifier(Modifier::BOLD))];
        if self.a.entries.len() > 1 {
            first.push(Span::styled(format!("   [ {} ]", t!("provenance.statement_of", "at" => self.at + 1, "total" => self.a.entries.len())), dim));
        }
        if !self.a.issues.is_empty() {
            first.push(Span::styled(format!("   {}", tn!("provenance.issues", self.a.issues.len())), tone(Tone::Red, theme)));
        }
        if matches!(e.statement.predicate, Predicate::Bom { .. }) {
            first.push(Span::styled(format!("   o {}", t!("provenance.open_bom")), dim));
        }
        let mut signer = vec![];
        if let Some(s) = &e.signer {
            signer.push(Span::raw(t!("provenance.signed_by", "identity" => s.identity)));
            if let Some(i) = s.get(provenance::Claim::Issuer) {
                signer.push(Span::styled(format!(" · {}", t!("provenance.via", "issuer" => i)), dim));
            }
        } else if let Some(k) = e.signatures.iter().find_map(|s| s.keyid.clone()) {
            signer.push(Span::raw(t!("provenance.signed_key", "key" => k)));
        } else if !e.signatures.is_empty() || e.public_key.is_some() {
            signer.push(Span::raw(t!("provenance.signed_nokey")));
        } else {
            signer.push(Span::raw(t!("provenance.unsigned")));
        }
        let mut out = vec![Line::from(first), Line::from(signer)];
        // A long identity fills the signer's line: the log entry has its own.
        if let Some(l) = e.log.first() {
            let when = l.integrated_time.map_or_else(|| "?".to_string(), |s| date(s.max(0) as u64));
            let index = l.index.map_or_else(|| "?".to_string(), |i| i.to_string());
            out.push(Line::styled(t!("provenance.logged", "index" => index, "when" => when), dim));
        }
        out.push(Line::styled(format!("⚠ {}", t!("provenance.not_verified")), tone(Tone::Grey, theme)));
        if let Some(c) = &self.compare {
            out.push(self.compare_line(c, theme));
        }
        out
    }

    fn compare_line(&self, c: &Compared, theme: &Theme) -> Line<'static> {
        let dim = faint(theme);
        match &c.result {
            Err(e) => Line::styled(e.clone(), tone(Tone::Red, theme)),
            Ok(d) => {
                let mut spans = vec![Span::raw(t!("provenance.compared", "name" => c.name)), Span::raw(" ")];
                match d.pairs.iter().find(|p| p.after == self.at) {
                    None => spans.push(Span::styled(tn!("provenance.change.unpaired", 1), dim)),
                    Some(p) if p.changes.is_empty() => spans.push(Span::raw(t!("provenance.change.none"))),
                    Some(p) => {
                        for (area, key) in AREAS {
                            let n = p.changes.iter().filter(|c| c.area == area).count();
                            if n > 0 {
                                spans.push(Span::styled(" · ", dim));
                                spans.push(Span::raw(tn!(&format!("provenance.change.{key}"), n)));
                            }
                        }
                    }
                }
                spans.push(Span::styled(format!("   n {}  c ×", t!("tui.provenance.changes")), dim));
                Line::from(spans)
            }
        }
    }

    fn input_line(&self, d: usize, sel: bool, theme: &Theme) -> Line<'static> {
        let p = self.prov().expect("inputs are provenance's");
        let r = &p.dependencies[d];
        let git = check::git_source(r).is_some();
        let s = self.sources.get(d).and_then(|s| s.as_ref());
        let (mark, t_) = match s {
            Some(Source::OnBranch { .. } | Source::Ahead { .. }) => ("●", Tone::Green),
            Some(Source::Elsewhere { .. }) => ("◐", Tone::Grey),
            Some(_) => ("○", Tone::Grey),
            None => ("·", Tone::Grey),
        };
        let style = |st: Style| if sel { Style::default() } else { st };
        let mut spans = vec![Span::styled(format!("{mark} "), style(tone(t_, theme))), Span::raw(view::input_label(r))];
        if self.changed(Area::Dependency, &dep_key(r)) {
            spans.push(Span::styled(" Δ", style(tone(Tone::Red, theme))));
        }
        if r.config_source {
            spans.push(Span::styled(format!("  {}", t!("provenance.config_source")), style(faint(theme))));
        }
        if git {
            spans.push(Span::styled(format!("  {}", source_word(s)), style(faint(theme))));
        }
        Line::from(spans)
    }

    fn output_line(&self, by: bool, i: usize, sel: bool, theme: &Theme) -> Line<'static> {
        let style = |st: Style| if sel { Style::default() } else { st };
        if by {
            let r = &self.prov().expect("byproducts are provenance's").byproducts[i];
            return Line::from(vec![Span::raw("· "), Span::raw(r.label().to_string()), Span::styled(format!("  {}", t!("provenance.byproduct")), style(faint(theme)))]);
        }
        let s = &self.entry().statement.subjects[i];
        let r = self.subjects.get(i).and_then(|r| r.as_ref());
        let (mark, t_) = match r {
            None => ("…", Tone::Grey),
            Some(Subject::Matches { .. }) => ("✓", Tone::Green),
            Some(Subject::Differs { .. } | Subject::Unreadable { .. }) => (if matches!(r, Some(Subject::Differs { .. })) { "✗" } else { "!" }, Tone::Red),
            Some(Subject::Missing) => ("?", Tone::Grey),
            Some(Subject::Large { .. }) => ("↵", Tone::Grey),
            Some(Subject::CannotCheck { .. }) => ("–", Tone::Grey),
        };
        let mut spans = vec![Span::styled(format!("{mark} "), style(tone(t_, theme))), Span::raw(s.label().to_string())];
        if self.changed(Area::Subject, s.label()) {
            spans.push(Span::styled(" Δ", style(tone(Tone::Red, theme))));
        }
        spans.push(Span::styled(format!("  {}", subject_word(r, s)), style(if matches!(t_, Tone::Grey) { faint(theme) } else { tone(t_, theme) })));
        Line::from(spans)
    }

    fn build_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let Some(p) = self.prov() else { return vec![] };
        let dim = faint(theme);
        let mut out = vec![Line::styled(view::builder_label(&p.builder.id), Style::default().add_modifier(Modifier::BOLD))];
        for k in ["workflow", "trigger", "runner"] {
            if let Some(f) = self.facts.iter().find(|f| fact_key(f) == k) {
                let v = if k == "workflow" { f.value.split('@').next().unwrap_or(&f.value).rsplit('/').next().unwrap_or("").to_string() } else { f.value.clone() };
                out.push(Line::styled(format!("{}: {v}", t!(&format!("provenance.fact.{k}"))), dim));
            }
        }
        if let Some(s) = &p.started {
            // The finish without its date when it is the start's.
            let (s, f) = (when(s), p.finished.as_deref().map(when));
            let f = f.map(|f| match (s.split_once(' '), f.split_once(' ')) {
                (Some((d1, _)), Some((d2, t2))) if d1 == d2 => t2.to_string(),
                _ => f,
            });
            out.push(Line::styled(f.map_or_else(|| s.clone(), |f| format!("{s} → {f}")), dim));
        }
        if self.facts.iter().any(|f| f.conflict.is_some()) {
            out.push(Line::styled(format!("⚠ {}", t!("provenance.conflicts")), tone(Tone::Red, theme)));
        }
        out
    }

    /// One column's lines, and which of them are the selected row.
    fn column(&self, c: Col, theme: &Theme) -> (Vec<Line<'static>>, std::ops::Range<usize>) {
        let sel = |r: usize| self.col == c && self.row == r;
        let mut lines = vec![];
        let mut at = 0..0;
        match c {
            Col::In => {
                let (shown, more) = self.shown_inputs();
                for (k, &d) in shown.iter().enumerate() {
                    if sel(k) {
                        at = lines.len()..lines.len() + 1;
                    }
                    lines.push(self.input_line(d, sel(k), theme));
                }
                if more > 0 {
                    if sel(shown.len()) {
                        at = lines.len()..lines.len() + 1;
                    }
                    lines.push(Line::styled(tn!("provenance.more", more), faint(theme)));
                }
                if shown.is_empty() {
                    lines.push(Line::styled(t!("provenance.no_inputs"), faint(theme)));
                }
            }
            Col::Build => {
                lines = self.build_lines(theme);
                if self.col == Col::Build {
                    at = 0..lines.len();
                }
            }
            Col::Out => {
                for (k, (by, i)) in self.outputs().into_iter().enumerate() {
                    if sel(k) {
                        at = lines.len()..lines.len() + 1;
                    }
                    lines.push(self.output_line(by, i, sel(k), theme));
                }
                if lines.is_empty() {
                    lines.push(Line::styled(t!("provenance.no_outputs"), faint(theme)));
                }
            }
        }
        (lines, at)
    }

    fn title(c: Col) -> String {
        match c {
            Col::In => t!("provenance.inputs"),
            Col::Build => t!("provenance.build"),
            Col::Out => t!("provenance.outputs"),
        }
    }

    /// Lines into `area`, scrolled so the selected ones show, those highlighted.
    fn render(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, sel: std::ops::Range<usize>, theme: &Theme) {
        let h = area.height as usize;
        let top = sel.end.saturating_sub(h);
        for (k, line) in lines.into_iter().enumerate().skip(top).take(h) {
            let style = if sel.contains(&k) { sty(&theme.dialog_input) } else { Style::default() };
            f.render_widget(Paragraph::new(line).style(style), Rect { y: area.y + (k - top) as u16, height: 1, ..area });
        }
    }

    fn draw_wide(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let cols = self.columns();
        let dim = faint(theme);
        let rects: Vec<Rect> = if cols.len() == 1 {
            vec![area]
        } else {
            let [a, _, b, _, c] = Layout::horizontal([
                Constraint::Percentage(34),
                Constraint::Length(3),
                Constraint::Percentage(28),
                Constraint::Length(3),
                Constraint::Min(10),
            ])
            .areas(area);
            // Beside the first rows, where the eye is.
            for x in [a.right() + 1, b.right() + 1] {
                f.render_widget(Paragraph::new("▶").style(dim), Rect { x, y: area.y + 1.min(area.height.saturating_sub(1)), width: 1, height: 1 });
            }
            vec![a, b, c]
        };
        for (c, r) in cols.into_iter().zip(rects) {
            let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(r);
            f.render_widget(Paragraph::new(Self::title(c).to_uppercase()).style(dim.add_modifier(Modifier::BOLD)), head);
            let (lines, sel) = self.column(c, theme);
            Self::render(f, body, lines, sel, theme);
        }
    }

    fn draw_stacked(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let dim = faint(theme);
        let mut lines = vec![];
        let mut sel = 0..0;
        for (k, c) in self.columns().into_iter().enumerate() {
            if k > 0 {
                lines.push(Line::styled("  ▼", dim));
            }
            lines.push(Line::styled(Self::title(c).to_uppercase(), dim.add_modifier(Modifier::BOLD)));
            let (own, at) = self.column(c, theme);
            if !at.is_empty() {
                sel = lines.len() + at.start..lines.len() + at.end;
            }
            lines.extend(own);
        }
        Self::render(f, area, lines, sel, theme);
    }

    fn details(&self, theme: &Theme) -> Vec<Line<'static>> {
        let dim = faint(theme);
        let kv = |k: String, v: String| Line::from(vec![Span::styled(format!("{k}: "), dim), Span::raw(v)]);
        let digests = |r: &Resource| r.digest.iter().map(|(a, d)| kv(a.clone(), d.clone())).collect::<Vec<_>>();
        let e = self.entry();
        let mut out = vec![];
        if let Predicate::Vsa(v) = &e.statement.predicate {
            let (mark, word, t_) = match v.result.as_deref() {
                Some("PASSED") => ("✓", t!("provenance.vsa.passed"), Tone::Green),
                Some("FAILED") => ("✗", t!("provenance.vsa.failed"), Tone::Red),
                other => ("?", other.unwrap_or("?").to_string(), Tone::Grey),
            };
            out.push(Line::styled(format!("{mark} {word}  {}", v.levels.join(" ")), tone(t_, theme).add_modifier(Modifier::BOLD)));
            out.push(kv(t!("provenance.vsa.verifier"), v.verifier.clone()));
            for (k, val) in [("resource", &v.resource), ("policy", &v.policy), ("time", &v.time)] {
                if let Some(val) = val {
                    out.push(kv(t!(&format!("provenance.vsa.{k}")), val.clone()));
                }
            }
            out.push(Line::styled(t!("provenance.vsa.note"), dim));
            if self.col != Col::Out {
                return out;
            }
        } else if matches!(e.statement.predicate, Predicate::Other { .. }) && self.col != Col::Out {
            return vec![Line::styled(t!("provenance.other_note"), dim)];
        }
        match self.col {
            Col::Build => {
                let Some(p) = self.prov() else { return out };
                for f in &self.facts {
                    let value = if matches!(fact_key(f).as_str(), "started" | "finished") { when(&f.value) } else { f.value.clone() };
                    let mut spans = vec![Span::styled(format!("{}: ", t!(&format!("provenance.fact.{}", fact_key(f)))), dim), Span::raw(value)];
                    spans.push(Span::styled(format!("  {}", t!(&format!("provenance.from.{}", from_key(f)))), dim));
                    if let Some(c) = &f.conflict {
                        spans.push(Span::styled(format!("  ⚠ {}", t!("provenance.conflict", "value" => c)), tone(Tone::Red, theme)));
                    }
                    out.push(Line::from(spans));
                }
                out.push(kv(t!("provenance.builder_id"), p.builder.id.clone()));
                out.push(kv(t!("provenance.build_type"), p.build_type.clone()));
                for (k, v) in &p.builder.version {
                    out.push(kv(t!("provenance.builder_version"), format!("{k} {v}")));
                }
                for (k, v) in [("external", &p.external), ("internal", &p.internal)] {
                    if v.as_object().is_some_and(|o| !o.is_empty()) || !(v.is_null() || v.is_object()) {
                        out.push(Line::styled(t!(&format!("provenance.{k}")), dim));
                        out.extend(serde_json::to_string_pretty(v).unwrap_or_default().lines().take(400).map(|l| Line::from(format!("  {l}"))));
                    }
                }
            }
            Col::In => {
                let (shown, _) = self.shown_inputs();
                let Some(&d) = shown.get(self.row) else { return out };
                let r = &self.prov().expect("inputs are provenance's").dependencies[d];
                out.push(kv(t!("provenance.uri"), r.uri.clone().unwrap_or_else(|| r.label().to_string())));
                out.extend(digests(r));
                if let Some(ep) = &r.entry_point {
                    out.push(kv(t!("provenance.fact.workflow"), ep.clone()));
                }
                if check::git_source(r).is_some() {
                    let s = self.sources.get(d).and_then(|s| s.as_ref());
                    let place = s.and_then(checkout).map(|c| format!(" · {}", c.display())).unwrap_or_default();
                    out.push(kv(t!("provenance.checkout"), format!("{}{place}", source_word(s))));
                }
            }
            Col::Out => {
                let Some(&(by, i)) = self.outputs().get(self.row) else { return out };
                let r = if by { &self.prov().expect("byproducts are provenance's").byproducts[i] } else { &self.entry().statement.subjects[i] };
                if let Some(u) = &r.uri {
                    out.push(kv(t!("provenance.uri"), u.clone()));
                }
                match self.subjects.get(i).and_then(|s| s.as_ref()).filter(|_| !by) {
                    Some(Subject::Differs { algorithm, expected, actual, path }) => {
                        out.push(kv(t!("provenance.named"), format!("{algorithm}:{expected}")));
                        out.push(Line::from(vec![Span::styled(format!("{}: ", t!("provenance.on_disk")), dim), Span::styled(format!("{algorithm}:{actual}"), tone(Tone::Red, theme))]));
                        out.push(Line::styled(t!("provenance.differs_note"), tone(Tone::Red, theme)));
                        out.push(kv(t!("provenance.file"), path.display().to_string()));
                    }
                    Some(Subject::Matches { path, .. }) => {
                        out.extend(digests(r));
                        out.push(kv(t!("provenance.file"), path.display().to_string()));
                        out.push(Line::styled(t!("provenance.matches_note"), dim));
                    }
                    Some(Subject::Large { path, .. } | Subject::Unreadable { path, .. }) => {
                        out.extend(digests(r));
                        out.push(kv(t!("provenance.file"), path.display().to_string()));
                    }
                    _ => out.extend(digests(r)),
                }
            }
        }
        out
    }

    fn change_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let dim = faint(theme);
        let Some(Compared { result: Ok(d), .. }) = &self.compare else { return vec![] };
        let Some(p) = d.pairs.iter().find(|p| p.after == self.at) else { return vec![] };
        p.changes
            .iter()
            .take(500)
            .map(|c| {
                let mut spans = vec![Span::styled(format!("{:<12} ", t!(&format!("provenance.area.{}", area_key(c.area)))), dim), Span::raw(format!("{}  ", c.key))];
                let (add, del) = (tone(Tone::Green, theme), tone(Tone::Red, theme));
                match c.kind {
                    Kind::Added => spans.push(Span::styled(format!("＋ {}", c.after.clone().unwrap_or_default()), add)),
                    Kind::Removed => spans.push(Span::styled(format!("− {}", c.before.clone().unwrap_or_default()), del)),
                    Kind::Changed => {
                        spans.push(Span::styled(c.before.clone().unwrap_or_default(), del));
                        spans.push(Span::raw(" → "));
                        spans.push(Span::styled(c.after.clone().unwrap_or_default(), add));
                    }
                }
                Line::from(spans)
            })
            .collect()
    }

    fn changed(&self, area: Area, key: &str) -> bool {
        let Some(Compared { result: Ok(d), .. }) = &self.compare else { return false };
        d.pairs.iter().filter(|p| p.after == self.at).flat_map(|p| &p.changes).any(|c| c.area == area && c.key == key)
    }
}

const AREAS: [(Area, &str); 5] =
    [(Area::Builder, "builder"), (Area::Parameter, "parameters"), (Area::Dependency, "dependencies"), (Area::Subject, "subjects"), (Area::Signer, "signer")];

fn area_key(a: Area) -> &'static str {
    match a {
        Area::Builder => "builder",
        Area::Parameter => "parameter",
        Area::Dependency => "dependency",
        Area::Subject => "subject",
        Area::Signer => "signer",
    }
}

/// A fact's key as the catalogue names it (`provenance.fact.<key>`), as serde writes it.
fn fact_key(f: &Fact) -> String {
    serde_json::to_value(f.key).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn from_key(f: &Fact) -> String {
    serde_json::to_value(f.from).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// A dependency as the diff keys it: its URI without `@ref`, else its name.
fn dep_key(r: &Resource) -> String {
    match r.uri.as_deref() {
        Some(u) if u.starts_with("git+") => u.rsplit_once('@').filter(|(h, _)| h.contains("://")).map_or(u, |(h, _)| h).to_string(),
        Some(u) => u.split('?').next().unwrap_or(u).to_string(),
        None => r.label().to_string(),
    }
}

/// An RFC 3339 time as local time, as the panels show dates; anything else as it is.
fn when(s: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(s).map_or_else(|_| s.to_string(), |t| date(t.timestamp().max(0) as u64))
}

fn checkout(s: &Source) -> Option<&Path> {
    match s {
        Source::OnBranch { checkout, .. } | Source::Ahead { checkout, .. } | Source::Elsewhere { checkout } | Source::Missing { checkout } => Some(checkout),
        Source::NoCheckout => None,
    }
}

fn source_word(s: Option<&Source>) -> String {
    match s {
        None => String::new(),
        Some(Source::OnBranch { behind: 0, .. }) => t!("provenance.source.on_head"),
        Some(Source::OnBranch { behind, .. }) => tn!("provenance.source.behind", *behind),
        Some(Source::Ahead { ahead, .. }) => tn!("provenance.source.ahead", *ahead),
        Some(Source::Elsewhere { .. }) => t!("provenance.source.elsewhere"),
        Some(Source::Missing { .. }) => t!("provenance.source.missing"),
        Some(Source::NoCheckout) => t!("provenance.source.no_checkout"),
    }
}

fn subject_word(r: Option<&Subject>, s: &Resource) -> String {
    match r {
        None => t!("provenance.subject.checking"),
        Some(Subject::Matches { .. }) => t!("provenance.subject.matches"),
        Some(Subject::Differs { .. }) => t!("provenance.subject.differs"),
        Some(Subject::Missing) => t!("provenance.subject.missing"),
        Some(Subject::Large { size, .. }) => t!("provenance.subject.large", "size" => crate::ui::size(*size)),
        Some(Subject::Unreadable { message, .. }) => t!("provenance.subject.unreadable", "message" => message),
        Some(Subject::CannotCheck { why: CannotCheck::Image }) => t!("provenance.subject.image"),
        Some(Subject::CannotCheck { why: CannotCheck::NoDigest }) => t!("provenance.subject.no_digest"),
        Some(Subject::CannotCheck { why: CannotCheck::Algorithms(a) }) => {
            let a = if a.is_empty() { s.digest.keys().cloned().collect() } else { a.clone() };
            t!("provenance.subject.algorithms", "algorithms" => a.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn fixture(p: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../coxswain-core/src/provenance/testdata").join(p)
    }

    fn key(code: KeyCode) -> Key {
        Key::new(code, false, false, false)
    }

    fn screen(v: &mut Viewer, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| v.draw(f, &Theme::nc())).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// Waits for the background checks, as the main loop's ticks would.
    fn settle(v: &mut Viewer) {
        for _ in 0..200 {
            v.poll();
            if v.subjects.iter().all(Option::is_some) && (v.prov().is_none() || !v.sources.is_empty() || v.inputs().is_empty()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn the_flow_with_its_checks_wide_and_stacked() {
        let dir = std::env::temp_dir().join(format!("coxswain-tui-provenance-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        let p = dir.join("rocket.provenance.json");
        std::fs::copy(fixture("made/statement-v1.json"), &p).unwrap();
        std::fs::write(dir.join("dist/rocket-1.4.0.tar.gz"), "foo").unwrap();
        std::fs::write(dir.join("dist/rocket-1.4.0.zip"), "not foo").unwrap();
        let mut v = Viewer::open(&p, None).unwrap();
        settle(&mut v);
        let s = screen(&mut v, 140, 40);
        assert!(s.contains("INPUTS") && s.contains("BUILD") && s.contains("OUTPUTS"), "{s}");
        assert!(s.contains("✓ dist/rocket-1.4.0.tar.gz"), "{s}");
        assert!(s.contains("✗ dist/rocket-1.4.0.zip"), "{s}");
        assert!(s.contains("– ghcr.io/demo/rocket"), "{s}");
        assert!(s.contains("GitHub Actions"), "{s}");
        assert!(s.contains("⚠ "), "the not-verified line: {s}");
        // Stacked when narrow.
        let s = screen(&mut v, 80, 50);
        assert!(s.contains("▼"), "{s}");
        // → to the outputs, ↓ to the zip: its digests side by side.
        v.key(key(KeyCode::Right), false);
        v.key(key(KeyCode::Down), false);
        let s = screen(&mut v, 140, 40);
        assert!(s.contains("sha512:f7fbba6e"), "{s}");
        // Enter reveals the file.
        assert!(matches!(v.key(key(KeyCode::Enter), false), Outcome::Reveal(f) if f == dir.join("dist/rocket-1.4.0.zip")));
        // Tab: the statement as JSON.
        v.key(key(KeyCode::Tab), false);
        assert!(screen(&mut v, 140, 40).contains("\"_type\": \"https://in-toto.io/Statement/v1\""));
        assert!(matches!(v.key(key(KeyCode::F(3)), false), Outcome::Source));
        assert!(matches!(v.key(key(KeyCode::Esc), false), Outcome::Close));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn statements_signer_and_compare() {
        let mut v = Viewer::open(&fixture("made/lenient.intoto.jsonl"), None).unwrap();
        assert!(screen(&mut v, 140, 30).contains("2"), "two statements");
        v.key(key(KeyCode::Char(']')), false);
        assert_eq!(v.at, 1);
        let mut v = Viewer::open(&fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl"), None).unwrap();
        let s = screen(&mut v, 160, 30);
        assert!(s.contains("publish-to-bcr/.github/workflows/publish.yaml"), "{s}");
        assert!(s.contains("#188622862"), "{s}");
        assert!(matches!(v.key(key(KeyCode::Char('c')), false), Outcome::Compare));
        v.compare(&fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl"));
        assert!(screen(&mut v, 160, 30).contains(&t!("provenance.change.none")));
        v.key(key(KeyCode::Char('c')), false);
        assert!(v.compare.is_none());
    }
}

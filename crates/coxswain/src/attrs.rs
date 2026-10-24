//! Permissions and dates in bulk, full screen (Alt+M, F9, the action menu): the fields first,
//! then the plan of every file that changes, before and after, then the run. The work is
//! coxswain-core's `attrs`, on a thread of its own: its progress shows as it goes, Esc stops it.

use coxswain_core::attrs::{self, Applied, Plan, Progress, Spec};
use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::bom::faint;
use crate::ui::{fit, sty};

pub enum Outcome {
    Stay,
    Close,
}

enum Done {
    Plan(Result<Plan, String>),
    Applied(Result<Applied, String>),
}

struct Job {
    stop: Arc<AtomicBool>,
    progress: Arc<Progress>,
    rx: mpsc::Receiver<Done>,
    writing: bool,
}

/// The boxes: the access time (Unix only) and into folders.
const BOXES: usize = if cfg!(windows) { 1 } else { 2 };

pub struct Viewer {
    roots: Vec<PathBuf>,
    base: PathBuf,
    title: String,
    spec: Spec,
    /// mode, date, then the boxes.
    focus: usize,
    plan: Option<Plan>,
    top: usize,
    job: Option<Job>,
    said: Option<String>,
}

impl Viewer {
    pub fn new(roots: Vec<PathBuf>, base: PathBuf) -> Viewer {
        let what = match roots.as_slice() {
            [one] => one.file_name().map_or_else(|| one.display().to_string(), |n| n.to_string_lossy().into_owned()),
            many => tn!("items", many.len()),
        };
        // Into folders by default when only folders are marked: what else would be meant.
        let recursive = roots.iter().all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()));
        Viewer {
            title: t!("attrs.title", "what" => what),
            roots,
            base,
            spec: Spec { recursive, ..Default::default() },
            focus: 0,
            plan: None,
            top: 0,
            job: None,
            said: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start(&mut self, writing: bool) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let (spec, s, p, roots, base) = (self.spec.clone(), stop.clone(), progress.clone(), self.roots.clone(), self.base.clone());
        let plan = self.plan.clone().filter(|_| writing);
        std::thread::spawn(move || {
            let done = if let Some(plan) = plan {
                Done::Applied(attrs::apply(&plan, &s, &p))
            } else {
                Done::Plan(attrs::plan(&roots, &spec, &base, &s, &p))
            };
            let _ = tx.send(done);
        });
        self.said = None;
        self.job = Some(Job { stop, progress, rx, writing });
    }

    /// A run that has ended: what was changed, for the app to say and keep for undo.
    pub fn poll(&mut self) -> Option<Result<Applied, String>> {
        let done = match self.job.as_ref()?.rx.try_recv() {
            Ok(d) => d,
            Err(mpsc::TryRecvError::Empty) => return None,
            // The work ended without a word (it failed): the dialog is free again, and says so.
            Err(mpsc::TryRecvError::Disconnected) => {
                let writing = self.job.take().is_some_and(|j| j.writing);
                return writing.then(|| Err(t!("attrs.failed"))).or_else(|| {
                    self.said = Some(t!("attrs.failed"));
                    None
                });
            }
        };
        self.job = None;
        match done {
            Done::Applied(r) => Some(r),
            Done::Plan(Err(e)) => {
                self.said = Some(e);
                None
            }
            Done::Plan(Ok(plan)) => {
                if plan.total == 0 {
                    self.said = Some(t!("attrs.none"));
                }
                self.top = 0;
                self.plan = Some(plan);
                None
            }
        }
    }

    fn ready(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.total > 0)
    }

    fn field(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.spec.mode),
            1 => Some(&mut self.spec.date),
            _ => None,
        }
    }

    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        let esc = key.code == KeyCode::Esc || action == Some(Action::Quit);
        if let Some(job) = &self.job {
            if esc {
                job.stop.store(true, Ordering::Relaxed);
            }
            return Outcome::Stay;
        }
        let rows = self.plan.as_ref().map_or(0, |p| p.rows.len() + p.refused.len());
        match key.code {
            _ if esc => return Outcome::Close,
            KeyCode::Tab => {
                let n = 2 + BOXES;
                self.focus = if key.shift { (self.focus + n - 1) % n } else { (self.focus + 1) % n };
            }
            KeyCode::Enter if self.ready() => self.start(true),
            KeyCode::Enter => self.start(false),
            KeyCode::Down | KeyCode::PageDown if rows > 0 => self.top = (self.top + if key.code == KeyCode::Down { 1 } else { 10 }).min(rows - 1),
            KeyCode::Up | KeyCode::PageUp => self.top = self.top.saturating_sub(if key.code == KeyCode::Up { 1 } else { 10 }),
            KeyCode::Char(' ') if self.focus >= 2 => {
                let b = if self.focus == 2 && BOXES == 2 { &mut self.spec.accessed } else { &mut self.spec.recursive };
                *b = !*b;
                self.plan = None;
            }
            KeyCode::Backspace => {
                if let Some(f) = self.field() {
                    f.pop();
                    self.plan = None;
                }
            }
            KeyCode::Char('u') if key.ctrl => {
                if let Some(f) = self.field() {
                    f.clear();
                    self.plan = None;
                }
            }
            _ => {
                if let Some(c) = crate::typed(&key)
                    && let Some(f) = self.field()
                {
                    f.push(c);
                    self.plan = None;
                }
            }
        }
        Outcome::Stay
    }

    fn state(&self) -> String {
        if let Some(job) = &self.job {
            let files = tn!("preview.files", job.progress.files.load(Ordering::Relaxed));
            let line = if job.writing { t!("attrs.writing", "files" => files) } else { t!("attrs.looking", "files" => files) };
            let tail = if job.stop.load(Ordering::Relaxed) { t!("attrs.stopping") } else { t!("attrs.stop_key", "key" => "Esc") };
            return format!("{line} · {tail}");
        }
        if let Some(s) = &self.said {
            return s.clone();
        }
        let Some(plan) = &self.plan else { return t!(if cfg!(windows) { "attrs.about_windows" } else { "attrs.about" }) };
        let mut s = tn!("attrs.planned", plan.total);
        if !plan.refused.is_empty() {
            s += &format!(" · {}", t!("attrs.left_out", "n" => plan.refused.len()));
        }
        if plan.stopped {
            s += &format!(" · {}", t!("attrs.stopped_early"));
        }
        s
    }

    fn keys(&self) -> String {
        if self.job.is_some() {
            return t!("attrs.stop_key", "key" => "Esc");
        }
        match &self.plan {
            Some(p) if p.total > 0 => t!("attrs.keys_plan", "go" => tn!("attrs.go", p.total)),
            _ => t!("attrs.keys_fields"),
        }
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let input = sty(&theme.dialog_input);
        let bold = base.add_modifier(Modifier::BOLD);
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
        let [mode_a, date_a, opts_a, state_a, rule1, body, rule2, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        let (lm, ld) = (t!("attrs.mode"), t!("attrs.date"));
        let label_w = lm.chars().count().max(ld.chars().count()) + 1;
        for (k, (label, value, a)) in [(lm, &self.spec.mode, mode_a), (ld, &self.spec.date, date_a)].into_iter().enumerate() {
            let [l, v] = Layout::horizontal([Constraint::Length(label_w as u16), Constraint::Min(1)]).areas(a);
            f.render_widget(Paragraph::new(label).style(base), l);
            let cursor = if self.focus == k && self.job.is_none() { "▏" } else { "" };
            let style = if self.focus == k { input } else { input.add_modifier(Modifier::DIM) };
            f.render_widget(Paragraph::new(crate::ui::fit_left(&format!("{value}{cursor}"), v.width as usize)).style(style), v);
        }
        let mut boxes = vec![];
        // The time of last access: Unix only (Windows keeps its own).
        if BOXES == 2 {
            boxes.push((t!("attrs.accessed"), self.spec.accessed));
        }
        boxes.push((t!("attrs.recursive"), self.spec.recursive));
        let mut x = opts_a.x;
        for (k, (label, on)) in boxes.into_iter().enumerate() {
            let s = format!("[{}] {label}", if on { "x" } else { " " });
            let width = (s.chars().count() as u16).min(opts_a.right().saturating_sub(x));
            let style = if self.focus == k + 2 { input } else { base };
            f.render_widget(Paragraph::new(s).style(style), Rect { x, width, ..opts_a });
            x = (x + width + 3).min(opts_a.right());
        }
        let err = self.said.is_some() && self.plan.is_none();
        f.render_widget(Paragraph::new(fit(&self.state(), w)).style(if err { bold } else { dim }), state_a);
        for r in [rule1, rule2] {
            f.render_widget(Paragraph::new("─".repeat(w)).style(sty(&theme.dialog_border)), r);
        }
        f.render_widget(Paragraph::new(fit(&self.keys(), w)).style(dim), foot);
        let Some(plan) = &self.plan else { return };
        let mut lines: Vec<(String, ratatui::style::Style)> = vec![];
        let shown = |p: &std::path::Path| p.strip_prefix(&self.base).ok().filter(|r| !r.as_os_str().is_empty()).unwrap_or(p).display().to_string();
        let name = |r: &attrs::Row| {
            let mut s = shown(&r.path);
            if r.is_dir {
                s.push(std::path::MAIN_SEPARATOR);
            }
            s
        };
        // The names in one column, so the bits and dates line up.
        let name_w = plan.rows.iter().map(|r| unicode_width::UnicodeWidthStr::width(name(r).as_str())).max().unwrap_or(0).min(w / 2);
        for r in &plan.rows {
            let s = fit(&name(r), name_w);
            let mut parts = vec![];
            if r.before.mode != r.after.mode {
                parts.push(format!("{} → {}", attrs::show_mode(r.before.mode), attrs::show_mode(r.after.mode)));
            }
            if r.before.modified != r.after.modified {
                parts.push(format!("{} → {}", attrs::show_date(r.before.modified), attrs::show_date(r.after.modified)));
            }
            lines.push((format!("{s}   {}", parts.join("   ")), base));
        }
        if plan.total > plan.rows.len() {
            lines.push((tn!("attrs.more", plan.total - plan.rows.len()), dim));
        }
        lines.extend(plan.refused.iter().map(|r| (r.clone(), dim)));
        self.top = self.top.min(lines.len().saturating_sub(1));
        for (i, (text, style)) in lines.iter().skip(self.top).take(body.height as usize).enumerate() {
            f.render_widget(Paragraph::new(fit(text, w)).style(*style), Rect { y: body.y + i as u16, height: 1, ..body });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    fn wait(v: &mut Viewer) -> Option<Result<Applied, String>> {
        for _ in 0..500 {
            if !v.busy() {
                return None;
            }
            if let Some(r) = v.poll() {
                return Some(r);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the run never ended");
    }

    #[test]
    fn plan_then_change() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-attrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.sh"), "x").unwrap();
        let mut v = Viewer::new(vec![d.join("a.sh")], d.clone());
        v.key(key(KeyCode::Tab), None);
        for c in "2020-01-02 03:04".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert!(wait(&mut v).is_none());
        assert_eq!(v.plan.as_ref().unwrap().total, 1);
        let backend = ratatui::backend::TestBackend::new(100, 20);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("a.sh") && screen.contains("→ 2020-01-02 03:04:00"), "{screen}");
        v.key(key(KeyCode::Enter), None);
        let done = wait(&mut v).expect("changed").unwrap();
        assert_eq!(done.changed, 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}

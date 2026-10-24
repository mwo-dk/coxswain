//! Make one side like the other (Compare folders' menu, entries 5 and 6), full screen: the plan of
//! every copy, replacement and deletion first, made at once, then the run. The work is
//! coxswain-core's `compare::sync`, on a thread of its own: its progress shows as it goes, Esc
//! stops it. Space ticks deleting what is only on the target side, and plans again.

use coxswain_core::compare::folders::Mode;
use coxswain_core::compare::sync::{self, Act, Applied, Plan, Progress};
use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::bom::faint;
use crate::ui::{fit, sty, wrap};

pub enum Outcome {
    Stay,
    Close,
}

enum Done {
    Plan(Box<Result<Plan, String>>),
    Applied(Result<Applied, String>),
}

struct Job {
    stop: Arc<AtomicBool>,
    progress: Arc<Progress>,
    rx: mpsc::Receiver<Done>,
    writing: bool,
}

pub struct Viewer {
    from: PathBuf,
    to: PathBuf,
    mode: Mode,
    title: String,
    undo_key: String,
    delete: bool,
    plan: Option<Plan>,
    top: usize,
    job: Option<Job>,
    said: Option<String>,
}

fn name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Viewer {
    /// Make `to` like `from`; the plan starts at once.
    pub fn new(from: PathBuf, to: PathBuf, mode: Mode, undo_key: String) -> Viewer {
        let mut v = Viewer { title: t!("sync.title", "to" => name(&to), "from" => name(&from)), from, to, mode, undo_key, delete: false, plan: None, top: 0, job: None, said: None };
        v.start(false);
        v
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start(&mut self, writing: bool) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let (s, p, from, to, mode, delete) = (stop.clone(), progress.clone(), self.from.clone(), self.to.clone(), self.mode, self.delete);
        let plan = self.plan.clone().filter(|_| writing);
        std::thread::spawn(move || {
            let done = match plan {
                Some(plan) => Done::Applied(sync::apply(&plan, &s, &p)),
                None => Done::Plan(Box::new(sync::plan(&from, &to, mode, delete, &s, &p))),
            };
            let _ = tx.send(done);
        });
        self.said = None;
        self.job = Some(Job { stop, progress, rx, writing });
    }

    /// A run that has ended: what was done, for the app to say and keep for undo.
    pub fn poll(&mut self) -> Option<Result<Applied, String>> {
        let done = match self.job.as_ref()?.rx.try_recv() {
            Ok(d) => d,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                let writing = self.job.take().is_some_and(|j| j.writing);
                return writing.then(|| Err(t!("sync.failed"))).or_else(|| {
                    self.said = Some(t!("sync.failed"));
                    None
                });
            }
        };
        self.job = None;
        match done {
            Done::Applied(r) => Some(r),
            Done::Plan(r) => self.planned(*r),
        }
    }

    fn planned(&mut self, r: Result<Plan, String>) -> Option<Result<Applied, String>> {
        match r {
            Err(e) => {
                self.said = Some(e);
                None
            }
            Ok(plan) => {
                if plan.total == 0 {
                    self.said = Some(t!("sync.none"));
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
            KeyCode::Enter if self.ready() => self.start(true),
            KeyCode::Enter => self.start(false),
            KeyCode::Char(' ') => {
                self.delete = !self.delete;
                self.plan = None;
                self.start(false);
            }
            KeyCode::Down | KeyCode::PageDown if rows > 0 => self.top = (self.top + if key.code == KeyCode::Down { 1 } else { 10 }).min(rows - 1),
            KeyCode::Up | KeyCode::PageUp => self.top = self.top.saturating_sub(if key.code == KeyCode::Up { 1 } else { 10 }),
            _ => {}
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
        let Some(plan) = &self.plan else { return t!("sync.about", "undo" => self.undo_key) };
        plan.summary()
    }

    fn keys(&self) -> String {
        if self.job.is_some() {
            return t!("attrs.stop_key", "key" => "Esc");
        }
        match &self.plan {
            Some(p) if p.total > 0 => t!("sync.keys_plan", "go" => t!("sync.go", "n" => p.total)),
            _ => t!("sync.keys"),
        }
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
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
        // The explanation is longer than a terminal is wide in every language: wrapped.
        let about = wrap(&t!("sync.about", "undo" => self.undo_key), w);
        let [about_a, box_a, state_a, rule1, body, rule2, foot] =
            Layout::vertical([Constraint::Length(about.len() as u16), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
        f.render_widget(Paragraph::new(about.into_iter().map(Line::from).collect::<Vec<_>>()).style(dim), about_a);
        let tick = format!("[{}] {}", if self.delete { "x" } else { " " }, t!("sync.delete", "dir" => name(&self.to)));
        f.render_widget(Paragraph::new(fit(&tick, w)).style(sty(&theme.dialog_input)), box_a);
        let err = self.said.is_some() && self.plan.is_none();
        f.render_widget(Paragraph::new(fit(&self.state(), w)).style(if err { bold } else { dim }), state_a);
        for r in [rule1, rule2] {
            f.render_widget(Paragraph::new("─".repeat(w)).style(sty(&theme.dialog_border)), r);
        }
        f.render_widget(Paragraph::new(fit(&self.keys(), w)).style(dim), foot);
        let Some(plan) = &self.plan else { return };
        let mut lines: Vec<(String, ratatui::style::Style)> = plan.rows.iter().map(|r| (row(r), if r.act == Act::Delete || r.older { bold } else { base })).collect();
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

/// One step as a line: what it does, then the path (a folder with `/`), and why it stands out.
fn row(r: &sync::Row) -> String {
    let act = r.act.label();
    let mut s = format!("{act:<12} {}", r.shown());
    if r.older {
        s += &format!("   ({})", t!("sync.older"));
    }
    s
}

#[cfg(all(test, unix))]
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
    fn plan_then_copy() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("l")).unwrap();
        std::fs::create_dir_all(d.join("r")).unwrap();
        std::fs::write(d.join("l/new.txt"), "x").unwrap();
        std::fs::write(d.join("r/extra.txt"), "x").unwrap();
        let mut v = Viewer::new(d.join("l"), d.join("r"), Mode::Quick, "Ctrl+Z".into());
        assert!(wait(&mut v).is_none());
        assert_eq!(v.plan.as_ref().unwrap().total, 1);
        // Space: deleting what is only on the right too, planned again.
        v.key(key(KeyCode::Char(' ')), None);
        assert!(wait(&mut v).is_none());
        assert_eq!(v.plan.as_ref().unwrap().deletes, 1);
        v.key(key(KeyCode::Char(' ')), None);
        assert!(wait(&mut v).is_none());
        let backend = ratatui::backend::TestBackend::new(100, 20);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("new.txt") && !screen.contains("extra.txt"), "{screen}");
        // The explanation, longer than the screen is wide, is wrapped, not cut: its last words show.
        assert!(screen.contains("undoes it all."), "{screen}");
        v.key(key(KeyCode::Enter), None);
        let done = wait(&mut v).expect("made alike").unwrap();
        assert_eq!(done.copied, 1);
        assert!(d.join("r/new.txt").exists() && d.join("r/extra.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}

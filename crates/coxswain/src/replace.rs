//! Find and replace across files, full screen (Alt+R, F9, the action menu, and from Find's
//! results): the fields and options first, then the preview of every change, any change or file
//! untickable, then the writing. The work is coxswain-core's `replace`, on a thread of its own:
//! its progress shows as it goes, and Esc stops it.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::replace::{self, Applied, Pick, Plan, Progress, Query};
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
    /// Files to write, for "3 of 7".
    all: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Row {
    File(usize),
    Change(usize, usize),
    Head,
    Skipped(usize),
}

impl Row {
    fn height(self) -> usize {
        if matches!(self, Row::Change(..)) { 2 } else { 1 }
    }
}

pub struct Viewer {
    roots: Vec<PathBuf>,
    base: PathBuf,
    title: String,
    query: Query,
    /// The field or box with the focus: find, with, regex, case, word.
    focus: usize,
    plan: Option<Plan>,
    ticks: Vec<Vec<bool>>,
    rows: Vec<Row>,
    cursor: usize,
    top: usize,
    job: Option<Job>,
    said: Option<String>,
}

impl Viewer {
    /// On `roots` (files, and folders walked through), paths shown from `base`; `find` typed in.
    pub fn new(roots: Vec<PathBuf>, base: PathBuf, find: String) -> Viewer {
        let what = match roots.as_slice() {
            [one] => one.file_name().map_or_else(|| one.display().to_string(), |n| n.to_string_lossy().into_owned()),
            many => tn!("items", many.len()),
        };
        Viewer {
            title: crate::ui::printable(&t!("replace.title", "what" => what)).into_owned(),
            roots,
            base,
            query: Query { find, ..Default::default() },
            focus: 0,
            plan: None,
            ticks: vec![],
            rows: vec![],
            cursor: 0,
            top: 0,
            job: None,
            said: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn ticked(&self) -> (usize, usize) {
        let files = self.ticks.iter().filter(|t| t.iter().any(|&b| b)).count();
        (self.ticks.iter().flatten().filter(|&&b| b).count(), files)
    }

    fn start(&mut self, writing: bool) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let (q, s, p) = (self.query.clone(), stop.clone(), progress.clone());
        let mut all = 0;
        if writing {
            let plan = self.plan.as_ref().expect("a plan");
            let picks: Vec<Pick> = (self.ticks.iter().enumerate())
                .filter(|(_, t)| t.iter().any(|&b| b))
                .map(|(file, t)| Pick { file, skip: (0..t.len()).filter(|&j| !t[j]).collect() })
                .collect();
            all = picks.len();
            let plan = plan.clone();
            std::thread::spawn(move || drop(tx.send(Done::Applied(replace::apply(&plan, &picks, &s, &p)))));
        } else {
            let roots = self.roots.clone();
            std::thread::spawn(move || drop(tx.send(Done::Plan(replace::plan(&roots, &q, &s, &p)))));
        }
        self.said = None;
        self.job = Some(Job { stop, progress, rx, writing, all });
    }

    /// A run that has ended: what was written, for the app to say and to keep for undo.
    pub fn poll(&mut self) -> Option<Result<Applied, String>> {
        let done = match self.job.as_ref()?.rx.try_recv() {
            Ok(d) => d,
            Err(mpsc::TryRecvError::Empty) => return None,
            // The work ended without a word (it failed): the dialog is free again, and says so.
            Err(mpsc::TryRecvError::Disconnected) => {
                let writing = self.job.take().is_some_and(|j| j.writing);
                return writing.then(|| Err(t!("replace.failed"))).or_else(|| {
                    self.said = Some(t!("replace.failed"));
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
                self.ticks = plan.files.iter().map(|f| vec![true; f.changes.len()]).collect();
                self.rows = vec![];
                for (i, f) in plan.files.iter().enumerate() {
                    self.rows.push(Row::File(i));
                    self.rows.extend((0..f.changes.len()).map(|j| Row::Change(i, j)));
                }
                if !plan.skipped.is_empty() {
                    self.rows.push(Row::Head);
                    self.rows.extend((0..plan.skipped.len()).map(Row::Skipped));
                }
                (self.cursor, self.top) = (0, 0);
                if plan.files.is_empty() {
                    self.said = Some(t!("replace.none", "looked" => tn!("preview.files", plan.looked)));
                }
                self.plan = Some(plan);
                None
            }
        }
    }

    fn previewing(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| !p.files.is_empty())
    }

    fn field(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.query.find),
            1 => Some(&mut self.query.with),
            _ => None,
        }
    }

    fn toggle(&mut self) {
        match self.rows.get(self.cursor) {
            Some(&Row::File(i)) => {
                let on = !self.ticks[i].iter().all(|&b| b);
                self.ticks[i].iter_mut().for_each(|b| *b = on);
            }
            Some(&Row::Change(i, j)) => self.ticks[i][j] = !self.ticks[i][j],
            _ => {}
        }
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        let esc = key.code == KeyCode::Esc || action == Some(Action::Quit);
        if let Some(job) = &self.job {
            if esc {
                job.stop.store(true, Ordering::Relaxed);
            }
            return Outcome::Stay;
        }
        // The preview, while it has the focus (focus 5).
        if self.focus == 5 && self.previewing() {
            let n = self.rows.len();
            match key.code {
                _ if esc => self.focus = 0,
                KeyCode::Tab => self.focus = if key.shift { 4 } else { 0 },
                KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down => self.cursor = (self.cursor + 1).min(n - 1),
                KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
                KeyCode::PageDown => self.cursor = (self.cursor + 10).min(n - 1),
                KeyCode::Home => self.cursor = 0,
                KeyCode::End => self.cursor = n - 1,
                KeyCode::Char(' ') | KeyCode::Insert => self.toggle(),
                KeyCode::Enter if self.ticked().0 > 0 => self.start(true),
                _ => {}
            }
            return Outcome::Stay;
        }
        match key.code {
            _ if esc => return Outcome::Close,
            KeyCode::Tab => {
                let last = if self.previewing() { 5 } else { 4 };
                self.focus = if key.shift { (self.focus + last) % (last + 1) } else { (self.focus + 1) % (last + 1) };
            }
            KeyCode::Down if self.previewing() => self.focus = 5,
            KeyCode::Enter => {
                self.plan = None;
                self.start(false);
            }
            KeyCode::Char(' ') if self.focus >= 2 => {
                let b = match self.focus {
                    2 => &mut self.query.regex,
                    3 => &mut self.query.case,
                    _ => &mut self.query.word,
                };
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

    fn shown(&self, p: &Path) -> String {
        p.strip_prefix(&self.base).unwrap_or(p).display().to_string()
    }

    /// What the line under the options says: how far a run is, why nothing was found, or what
    /// a replace would do.
    fn state(&self) -> String {
        if let Some(job) = &self.job {
            let (files, changes) = (job.progress.files.load(Ordering::Relaxed), job.progress.changes.load(Ordering::Relaxed));
            let stopping = job.stop.load(Ordering::Relaxed);
            let line = if job.writing {
                t!("replace.writing", "n" => (files + 1).min(job.all), "all" => job.all)
            } else {
                t!("replace.looking", "files" => tn!("preview.files", files), "changes" => tn!("replace.changes", changes))
            };
            return format!("{line} · {}", if stopping { t!("replace.stopping") } else { t!("replace.stop_key", "key" => "Esc") });
        }
        if let Some(s) = &self.said {
            return s.clone();
        }
        let Some(plan) = &self.plan else { return t!("replace.about") };
        let (changes, files) = self.ticked();
        let mut s = t!("replace.ticked", "changes" => tn!("replace.changes", changes), "files" => tn!("preview.files", files));
        if !plan.skipped.is_empty() || plan.binary > 0 {
            s += &format!(" · {}", t!("replace.left_out", "n" => plan.skipped.len() + plan.more + plan.binary));
        }
        if plan.full {
            s += &format!(" · {}", t!("replace.full", "n" => replace::MOST_CHANGES));
        } else if plan.stopped {
            s += &format!(" · {}", t!("replace.stopped_early"));
        }
        s
    }

    fn keys(&self) -> String {
        if self.job.is_some() {
            return t!("replace.stop_key", "key" => "Esc");
        }
        if self.focus == 5 && self.previewing() {
            let (changes, files) = self.ticked();
            return t!("replace.keys_preview", "go" => tn!("replace.go", changes, "files" => tn!("preview.files", files)));
        }
        t!(if self.previewing() { "replace.keys_fields_again" } else { "replace.keys_fields" })
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
        let [find_a, with_a, opts_a, state_a, rule1, body, rule2, foot] = Layout::vertical([
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
        let (lf, lw) = (t!("replace.find"), t!("replace.with"));
        let label_w = lf.chars().count().max(lw.chars().count()) + 1;
        for (k, (label, value, a)) in [(lf, &self.query.find, find_a), (lw, &self.query.with, with_a)].into_iter().enumerate() {
            let [l, v] = Layout::horizontal([Constraint::Length(label_w as u16), Constraint::Min(1)]).areas(a);
            f.render_widget(Paragraph::new(label).style(base), l);
            let cursor = if self.focus == k && self.job.is_none() { "▏" } else { "" };
            let style = if self.focus == k { input } else { input.add_modifier(Modifier::DIM) };
            f.render_widget(Paragraph::new(crate::ui::fit_left(&format!("{value}{cursor}"), v.width as usize)).style(style), v);
        }
        let boxes = [(&t!("replace.regex"), self.query.regex), (&t!("replace.case"), self.query.case), (&t!("replace.word"), self.query.word)];
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
        // Scrolled so the cursor's row shows whole.
        let h = body.height as usize;
        self.top = self.top.min(self.cursor);
        // A plan that found nothing has no rows.
        while !self.rows.is_empty() && self.rows[self.top..=self.cursor.min(self.rows.len() - 1)].iter().map(|r| r.height()).sum::<usize>() > h && self.top < self.cursor {
            self.top += 1;
        }
        let mut y = body.y;
        let focused = self.focus == 5;
        for (k, row) in self.rows.iter().enumerate().skip(self.top) {
            if (y + row.height() as u16) > body.bottom() {
                break;
            }
            let here = k == self.cursor && focused;
            let tick = |on: bool| if on { "[x]" } else { "[ ]" };
            let lines: Vec<(String, ratatui::style::Style)> = match *row {
                Row::File(i) => {
                    let t = &self.ticks[i];
                    let mark = if t.iter().all(|&b| b) { "[x]" } else if t.iter().any(|&b| b) { "[-]" } else { "[ ]" };
                    vec![(format!("{mark} {} ({})", self.shown(&plan.files[i].path), t.len()), bold)]
                }
                Row::Change(i, j) => {
                    let c = &plan.files[i].changes[j];
                    let on = self.ticks[i][j];
                    let num = format!("{:>5}", c.line);
                    vec![
                        (format!("    {} {num} − {}", tick(on), c.before), base),
                        (format!("        {} + {}", " ".repeat(num.len()), c.after), if on { base } else { dim }),
                    ]
                }
                Row::Head => vec![(t!("replace.left_out_head"), bold)],
                Row::Skipped(s) => {
                    let s = &plan.skipped[s];
                    vec![(format!("    {} · {}", self.shown(&s.path), s.why), dim)]
                }
            };
            for (text, style) in lines {
                let style = if here { input } else { style };
                f.render_widget(Paragraph::new(fit(&text, w)).style(style), Rect { y, height: 1, ..body });
                y += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    #[cfg(unix)]
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
    fn a_name_with_control_characters_never_reaches_the_terminal_raw() {
        let v = Viewer::new(vec![PathBuf::from("/tmp/a\x1b[2Jb\x07")], PathBuf::from("/tmp"), String::new());
        assert!(!v.title.chars().any(|c| c.is_control()), "{:?}", v.title);
    }

    #[cfg(unix)]
    #[test]
    fn preview_untick_and_write() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "foo\nfoo\n").unwrap();
        std::fs::write(d.join("b.txt"), "foo\n").unwrap();
        let mut v = Viewer::new(vec![d.clone()], d.clone(), "foo".into());
        v.key(key(KeyCode::Tab), None);
        for c in "bar".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert!(wait(&mut v).is_none());
        assert_eq!(v.ticked(), (3, 2));
        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("a.txt (2)") && screen.contains("+ bar"), "{screen}");
        // Into the list: the second change of a.txt unticked, then b.txt as a whole.
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Char(' ')), None);
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Char(' ')), None);
        assert_eq!(v.ticked(), (1, 1));
        v.key(key(KeyCode::Enter), None);
        let done = wait(&mut v).expect("written").unwrap();
        assert_eq!((done.replaced, done.files), (1, 1));
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "bar\nfoo\n");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "foo\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_preview_that_finds_nothing_is_drawn() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-replace-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "foo\n").unwrap();
        let mut v = Viewer::new(vec![d.clone()], d.clone(), "goldfish".into());
        v.key(key(KeyCode::Enter), None);
        assert!(wait(&mut v).is_none());
        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "foo\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}

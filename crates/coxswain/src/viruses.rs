//! *Scan for viruses* in the terminal app (coxswain-core's `viruses`): asked first, with what
//! it reads, the database and its update, and the first pull; then the status line says how
//! far it is, and the key again stops it. What was found is marked in the panel and listed in a
//! dialog, where each file is left, put in the bin, or moved into quarantine.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use coxswain_core::config::{Action, KeyCode};
use coxswain_core::t;
use coxswain_core::viruses::{self, Choice, Report, Said};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, Wrap};

use crate::{App, Dialog};

/// A scan running on its thread.
pub struct Run {
    started: Instant,
    line: String,
    /// The status line it showed last: another message is not written over.
    shown: String,
    pub stop: Arc<AtomicBool>,
    rx: mpsc::Receiver<Msg>,
}

enum Msg {
    Said(String),
    Done(Result<Report, String>),
}

/// The question worked out on a thread: the targets, and the dialog's text or why not.
pub type Plan = (Vec<PathBuf>, Result<String, String>);

/// What was found, in a dialog: each file with its choice.
pub struct Found {
    report: Report,
    choices: Vec<Choice>,
    cursor: usize,
}

impl App {
    /// The key: stops a scan under way, else asks about scanning the marked entries (or the one
    /// under the cursor; on `..`, the panel's folder).
    pub(crate) fn scan_viruses(&mut self) {
        if let Some(r) = &self.viruses {
            r.stop.store(true, Ordering::Relaxed);
            return self.status = Some(t!("searchable.stopping"));
        }
        let p = self.panel();
        if coxswain_core::archive::split(&p.dir).is_some() || coxswain_core::history::split(&p.dir).is_some() || coxswain_core::history::is_history(&p.dir) {
            return self.status = Some(t!("viruses.none"));
        }
        let mut targets = p.targets();
        if targets.is_empty() {
            targets.push(p.dir.clone());
        }
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            let plan = viruses::plan(&cfg, &targets).map(|p| p.text);
            let _ = tx.send((targets, plan));
        });
        self.viruses_plan = Some(rx);
    }

    pub(crate) fn poll_viruses_plan(&mut self) {
        let Some(rx) = &self.viruses_plan else { return };
        let Ok((targets, plan)) = rx.try_recv() else { return };
        self.viruses_plan = None;
        let title = t!("action.scan_viruses");
        self.dialog = Some(match plan {
            Ok(text) => Dialog::VirusScan { title, text, targets },
            Err(e) => Dialog::Message { title, text: format!("{e}\n\n{}", t!("searchable.settings_key", "key" => self.key_label(Action::Settings))) },
        });
    }

    pub(crate) fn viruses_start(&mut self, targets: Vec<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (cfg, flag) = (self.cfg.preview.clone(), stop.clone());
        std::thread::spawn(move || {
            let image = cfg.images.get(viruses::TOOL).cloned().unwrap_or_default();
            let said = tx.clone();
            let r = viruses::scan(&cfg, &targets, &flag, |s| {
                let line = match s {
                    Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => image)),
                    Said::Updating(l) => format!("{} · {l}", t!("viruses.updating")),
                    Said::Scanned(n, all) => t!("viruses.busy", "n" => n, "all" => all),
                };
                let _ = said.send(Msg::Said(line));
            });
            let _ = tx.send(Msg::Done(r));
        });
        self.viruses = Some(Run { started: Instant::now(), line: t!("viruses.busy_start"), shown: String::new(), stop, rx });
        self.poll_viruses();
    }

    /// How far the scan is, on the status line; when done, what it found.
    pub(crate) fn poll_viruses(&mut self) {
        let key = self.key_label(Action::ScanViruses).to_string();
        let Some(r) = &mut self.viruses else { return };
        let mut done = None;
        while let Ok(msg) = r.rx.try_recv() {
            match msg {
                Msg::Said(l) => r.line = l,
                Msg::Done(d) => done = Some(d),
            }
        }
        let Some(done) = done else {
            let stopping = r.stop.load(Ordering::Relaxed);
            let tail = if stopping { t!("searchable.stopping") } else { t!("searchable.stop_key", "key" => key) };
            let new = format!("{}, {} s · {tail}", r.line, r.started.elapsed().as_secs());
            if self.status.is_none() || self.status.as_ref() == Some(&r.shown) {
                self.status = Some(new.clone());
            }
            r.shown = new;
            return;
        };
        let r = self.viruses.take().expect("a run");
        if let Ok((_, _, image)) = viruses::how(&self.cfg.preview) {
            let mut st = coxswain_core::state::AppState::load();
            coxswain_core::images::used(&mut st, &image, coxswain_core::fs::now());
            let _ = st.save();
        }
        self.status = None;
        let report = match done {
            Ok(report) => report,
            Err(_) if r.stop.load(Ordering::Relaxed) => return self.status = Some(t!("viruses.stopped")),
            Err(e) => return self.dialog = Some(crate::failure(t!("viruses.failed"), &e)),
        };
        // Marked in the panels: each file found there, and each folder with one in it.
        for p in self.panels.iter_mut().filter(|_| !report.found.is_empty()) {
            let hit: Vec<usize> = (0..p.entries.len()).filter(|&i| !p.entries[i].is_parent() && report.found.iter().any(|f| f.path.starts_with(&p.entries[i].path))).collect();
            if !hit.is_empty() {
                p.clear_marks();
                hit.into_iter().for_each(|i| p.toggle_mark(i));
            }
        }
        let notes = notes(&report);
        if report.found.is_empty() {
            let clean = t!("viruses.clean", "n" => coxswain_core::i18n::digits(report.scanned.to_string()));
            if notes.is_empty() {
                return self.status = Some(clean);
            }
            return self.dialog = Some(Dialog::Message { title: t!("action.scan_viruses"), text: format!("{clean}.\n\n{notes}") });
        }
        let choices = vec![Choice::Nothing; report.found.len()];
        self.dialog = Some(Dialog::Viruses(Box::new(Found { report, choices, cursor: 0 })));
    }

    /// A key in the list of what was found.
    pub(crate) fn viruses_key(&mut self, mut v: Box<Found>, code: KeyCode, ch: Option<char>, esc: bool) {
        let n = v.report.found.len();
        match (code, ch.map(|c| c.to_ascii_lowercase())) {
            (KeyCode::Up, _) => v.cursor = v.cursor.saturating_sub(1),
            (KeyCode::Down, _) => v.cursor = (v.cursor + 1).min(n.saturating_sub(1)),
            (KeyCode::Home, _) => v.cursor = 0,
            (KeyCode::End, _) => v.cursor = n.saturating_sub(1),
            (_, Some('n')) => v.choices[v.cursor] = Choice::Nothing,
            (_, Some('b')) => v.choices[v.cursor] = Choice::Bin,
            (_, Some('q')) => v.choices[v.cursor] = Choice::Quarantine,
            (KeyCode::Enter, _) => return self.viruses_apply(&v),
            _ if esc => return,
            _ => {}
        }
        self.dialog = Some(Dialog::Viruses(v));
    }

    fn viruses_apply(&mut self, v: &Found) {
        let (mut bin, mut quarantine, mut failed) = (0, 0, vec![]);
        for (f, c) in v.report.found.iter().zip(&v.choices) {
            match viruses::act(f, *c) {
                Ok(_) if *c == Choice::Bin => bin += 1,
                Ok(_) if *c == Choice::Quarantine => quarantine += 1,
                Ok(_) => {}
                Err(e) => failed.push(e),
            }
        }
        let left = v.report.found.len() - bin - quarantine;
        self.reload();
        let done = t!("viruses.done", "bin" => bin, "quarantine" => quarantine, "left" => left);
        if failed.is_empty() {
            self.status = Some(done);
        } else {
            self.dialog = Some(crate::failure(t!("viruses.failed_act"), &failed.join("\n")));
        }
    }
}

/// What a report says beyond its findings: the database, what was left out, what failed.
fn notes(r: &Report) -> String {
    let mut v = vec![viruses::db_line(r.db)];
    if let Some(e) = &r.update_failed {
        v.push(t!("viruses.update_failed", "error" => e));
    }
    if r.left_out > 0 {
        v.push(t!("viruses.left_out", "n" => r.left_out));
    }
    if r.more {
        v.push(t!("viruses.more"));
    }
    if !r.problems.is_empty() {
        v.push(format!("{}\n{}", t!("viruses.problems"), r.problems.join("\n")));
    }
    // Only the database line: nothing to say.
    if v.len() == 1 && r.update_failed.is_none() {
        return String::new();
    }
    v.join("\n")
}

fn choice(c: Choice) -> String {
    t!(match c {
        Choice::Nothing => "viruses.choice.nothing",
        Choice::Bin => "viruses.choice.bin",
        Choice::Quarantine => "viruses.choice.quarantine",
    })
}

/// The list of what was found, in a box: a line of what it is about, a row a file (its
/// choice, its name, what was found), the notes, and the keys.
pub(crate) fn draw(f: &mut Frame, app: &App, full: Rect, v: &Found) {
    let t = &app.theme;
    let quarantine = viruses::quarantine_dir().map(|d| d.display().to_string()).unwrap_or_default();
    let head = format!(
        "{} {}",
        t!("viruses.found", "n" => coxswain_core::i18n::digits(v.report.scanned.to_string())),
        t!("viruses.quarantine_at", "path" => quarantine)
    );
    let notes = notes(&v.report);
    let w = full.width.min(100);
    let head_p = Paragraph::new(head.as_str()).wrap(Wrap { trim: true });
    let notes_p = Paragraph::new(notes.as_str()).wrap(Wrap { trim: true });
    let (hh, nh) = (head_p.line_count(w.saturating_sub(4)) as u16, if notes.is_empty() { 0 } else { notes_p.line_count(w.saturating_sub(4)) as u16 });
    let rows = (v.report.found.len() as u16).min(full.height.saturating_sub(hh + nh + 8)).max(1);
    let area = crate::ui::centered(full, w, hh + nh + rows + 5);
    let inner = crate::ui::frame(f, app, area, &t!("viruses.title")).inner(ratatui::layout::Margin::new(1, 0));
    let [a, _, list, b, _, keys] = Layout::vertical([Constraint::Length(hh), Constraint::Length(1), Constraint::Length(rows), Constraint::Length(nh), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    f.render_widget(head_p, a);
    let cw = [Choice::Nothing, Choice::Bin, Choice::Quarantine].map(|c| unicode_width::UnicodeWidthStr::width(choice(c).as_str())).into_iter().max().unwrap_or(8);
    let skip = v.cursor.saturating_sub(rows as usize - 1);
    for (i, (found, c)) in v.report.found.iter().zip(&v.choices).enumerate().skip(skip).take(rows as usize) {
        // What was found, then where: inside the panel's folder from there, else in full.
        let name = found.path.strip_prefix(&app.panel().dir).unwrap_or(&found.path).display().to_string();
        let line = format!(" {} {} · {}", crate::ui::fit(&choice(*c), cw), found.name, name.chars().map(|c| if c.is_control() { '?' } else { c }).collect::<String>());
        let style = if i == v.cursor { crate::ui::sty(&t.dialog_input) } else { crate::ui::dstyle(t) };
        f.render_widget(Paragraph::new(crate::ui::fit(&line, list.width as usize)).style(style), Rect { y: list.y + (i - skip) as u16, height: 1, ..list });
    }
    if !notes.is_empty() {
        f.render_widget(notes_p, b);
    }
    f.render_widget(Paragraph::new(t!("viruses.keys", "verb" => t!("viruses.apply"), "cancel" => t!("common.close"))).centered(), keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viruses_list_takes_a_choice_per_file_and_esc_changes_nothing() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-viruses-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.com"), "x").unwrap();
        let cfg = coxswain_core::config::Config { check_updates: false, ..Default::default() };
        let index = crate::test_index(&cfg);
        let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
        let found = vec![viruses::Finding { path: d.join("a.com"), name: "Eicar-Test-Signature".into(), id: viruses::Ident::of(&d.join("a.com")) }];
        let report = Report { found, scanned: 1, files: 1, ..Default::default() };
        let v = Box::new(Found { report, choices: vec![Choice::Nothing], cursor: 0 });
        app.viruses_key(v, KeyCode::Char('q'), Some('q'), false);
        let Some(Dialog::Viruses(v)) = app.dialog.take() else { panic!("still open") };
        assert_eq!(v.choices, [Choice::Quarantine]);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        app.dialog = Some(Dialog::Viruses(v));
        term.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let screen: String = (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect();
        assert!(screen.contains("Eicar-Test-Signature"), "{screen}");
        let Some(Dialog::Viruses(v)) = app.dialog.take() else { panic!() };
        app.viruses_key(v, KeyCode::Esc, None, true);
        assert!(app.dialog.is_none() && d.join("a.com").exists(), "Esc: closed, nothing done");
        let _ = std::fs::remove_dir_all(d);
    }
}

//! Coxswain TUI: two panels, a command line and a function-key bar, Norton Commander style.

mod bom;
mod ui;

use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Config, Glyphs, Key, KeyCode};
use coxswain_core::fs::{self as bfs, resolve, Entry, SortKey};
use coxswain_core::git;
use coxswain_core::history::{self, Lasts};
use coxswain_core::helper::{self, Client};
use coxswain_core::index::{self, Results, State};
use ratatui::crossterm::event::{self, Event, KeyCode as CK, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::crossterm::{cursor, execute, terminal};
use ratatui::layout::Rect;
use ratatui::DefaultTerminal;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub struct Panel {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub offset: usize,
    /// Rows visible at the last draw, for paging.
    pub page: usize,
    pub marked: HashSet<PathBuf>,
    /// Bytes of the marked entries, for the info line: kept up to date, not summed over
    /// every entry at every frame.
    pub marked_bytes: u64,
    pub sort: SortKey,
    pub reverse: bool,
    pub git: Option<git::Status>,
    pub error: Option<String>,
    /// Folder sizes, measured in the background.
    pub sizes: HashMap<PathBuf, u64>,
    /// The last commit of each entry, when git has answered.
    pub last: Option<Arc<Lasts>>,
}

impl Panel {
    fn new(dir: PathBuf, show_hidden: bool) -> Panel {
        // A file opens its folder with the cursor on it.
        let file = dir.is_file().then(|| dir.file_name().map(|n| n.to_string_lossy().into_owned())).flatten();
        let dir = if file.is_some() { dir.parent().map_or(dir.clone(), Path::to_path_buf) } else { dir };
        let mut p = Panel {
            dir,
            entries: vec![],
            cursor: 0,
            offset: 0,
            page: 20,
            marked: HashSet::new(),
            marked_bytes: 0,
            sort: SortKey::Name,
            reverse: false,
            git: None,
            error: None,
            sizes: HashMap::new(),
            last: None,
        };
        p.load(show_hidden);
        if let Some(name) = file {
            p.select_name(&name);
        }
        p
    }

    /// Re-read the directory, keeping the cursor on the same name.
    fn load(&mut self, show_hidden: bool) {
        let keep = self.current().map(|e| e.name.clone());
        // Walk up until something is listable (the directory may have been deleted).
        loop {
            match bfs::list(&self.dir, show_hidden) {
                Ok(v) => {
                    self.entries = v;
                    self.error = None;
                    break;
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    match self.dir.parent() {
                        Some(p) => self.dir = p.to_path_buf(),
                        None => {
                            self.entries.clear();
                            break;
                        }
                    }
                }
            }
        }
        self.fill(keep);
    }

    /// Sort what was just listed, keep the marks still there and put the cursor on `keep`.
    fn fill(&mut self, keep: Option<String>) {
        bfs::sort(&mut self.entries, self.sort, self.reverse);
        // Inside an archive a folder's size comes with the listing; there is nothing to measure.
        if coxswain_core::archive::split(&self.dir).is_some() {
            self.sizes.extend(self.entries.iter().filter(|e| e.is_dir && !e.is_parent()).map(|e| (e.path.clone(), e.size)));
        }
        let names: HashSet<&Path> = self.entries.iter().map(|e| e.path.as_path()).collect();
        self.marked.retain(|p| names.contains(p.as_path()));
        self.recount_marks();
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        if let Some(n) = keep {
            self.select_name(&n);
        }
    }

    /// Sort the entries as they are, keeping the cursor on the same name: no reread, which
    /// on a big folder takes longer than the sort.
    fn resort(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        bfs::sort(&mut self.entries, self.sort, self.reverse);
        if let Some(n) = keep {
            self.select_name(&n);
        }
    }

    /// Mark the entry at `i`, or unmark it. `..` is never marked.
    fn toggle_mark(&mut self, i: usize) {
        let Some(e) = self.entries.get(i).filter(|e| !e.is_parent()) else { return };
        if self.marked.remove(&e.path) {
            self.marked_bytes -= e.size;
        } else {
            self.marked.insert(e.path.clone());
            self.marked_bytes += e.size;
        }
    }

    fn clear_marks(&mut self) {
        self.marked.clear();
        self.marked_bytes = 0;
    }

    fn recount_marks(&mut self) {
        self.marked_bytes = self.entries.iter().filter(|e| self.marked.contains(&e.path)).map(|e| e.size).sum();
    }

    /// Show `dir`, not listed yet: returns the name to put the cursor on once it is.
    fn enter(&mut self, dir: PathBuf) -> Option<String> {
        let from = std::mem::replace(&mut self.dir, dir);
        self.clear_marks();
        self.sizes.clear();
        self.entries.clear();
        self.cursor = 0;
        self.offset = 0;
        self.git = None;
        self.last = None;
        // Coming up out of a directory (or a history): the cursor on it, as NC does.
        from.strip_prefix(&self.dir).ok().and_then(|r| r.components().next()).map(|n| n.as_os_str().to_string_lossy().into_owned())
    }

    fn select_name(&mut self, name: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.cursor = i;
        }
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    /// Marked entries, or the one under the cursor.
    fn targets(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            self.current().filter(|e| !e.is_parent()).map(|e| vec![e.path.clone()]).unwrap_or_default()
        } else {
            self.entries.iter().filter(|e| self.marked.contains(&e.path)).map(|e| e.path.clone()).collect()
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let max = self.entries.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }
}

pub enum Prompt {
    Copy(Vec<PathBuf>),
    Move(Vec<PathBuf>),
    Extract(Vec<PathBuf>),
    Pack(Vec<PathBuf>),
    /// A password for a new zip or 7z (none: empty), shown as stars; then typed again.
    PackPassword(Vec<PathBuf>, PathBuf),
    PackConfirm(Vec<PathBuf>, PathBuf, String),
    /// A locked archive's password, to run the copy, move or extract again with; shown as
    /// stars and kept for that run only.
    Password(Transfer, Vec<PathBuf>, PathBuf),
    /// A locked archive's password, to look into it (in that panel) with.
    Unlock(usize, PathBuf),
    /// A locked archive's password, to view the file inside it with (F3).
    Peek(PathBuf),
    Mkdir,
    Goto(usize),
    Select(bool),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Transfer {
    Copy,
    Move,
    Extract,
    /// Delete, for good with `true` (inside an archive always: it is taken out).
    Delete(bool),
    /// A new folder, the one path given.
    Mkdir,
}

pub enum MenuRun {
    Action(Action),
    User(usize),
}

pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub run: MenuRun,
}

pub enum Dialog {
    Input { title: String, label: String, value: String, prompt: Prompt },
    /// Delete `paths`; `forever` skips the trash.
    Confirm { title: String, text: String, paths: Vec<PathBuf>, forever: bool },
    /// `mode`: 0 names everywhere, 1 names in this folder, 2 the text of files, 3 Ask (its
    /// questions and answers are in `App::chat`).
    Search { query: String, mode: u8, results: Results, cursor: usize, offset: usize },
    /// `direct`: a typed key runs the item with that key (F2); otherwise it filters (F9).
    Menu { title: String, filter: String, items: Vec<MenuItem>, cursor: usize, direct: bool },
    Help { scroll: u16 },
    Message { title: String, text: String },
    /// A CycloneDX BOM, full screen (F3 on one).
    Bom(Box<bom::Viewer>),
}

/// A question asked in Find file, the sources it was answered from, and the answer so far.
#[derive(Default)]
pub struct Turn {
    pub question: String,
    pub sources: Vec<PathBuf>,
    pub answer: String,
    pub error: Option<String>,
}

/// What an Ask sends back while it runs.
enum AskMsg {
    Sources(Vec<PathBuf>),
    Piece(String),
    Done(Result<(), String>),
}

/// What git tells of a panel's folder, as it comes: the status, then each entry's last commit.
enum Git {
    Status(Option<git::Status>),
    Last(Option<Arc<Lasts>>),
}

/// Work done by the main loop after a frame: what needs the real terminal, and what takes a
/// while, so the status line says so first.
enum Run {
    Shell { cmd: String, dir: PathBuf, wait: bool },
    ShowOutput,
}

pub struct App {
    pub cfg: Config,
    keymap: HashMap<Key, Action>,
    pub theme: config::Theme,
    pub glyphs: Glyphs,
    pub panels: [Panel; 2],
    pub active: usize,
    pub cmdline: String,
    pub dialog: Option<Dialog>,
    pub status: Option<String>,
    pub quick: Option<String>,
    pub show_hidden: bool,
    pub index: Arc<Client>,
    /// Panel rectangles from the last draw, for mouse hits.
    pub areas: [Rect; 2],
    git_tx: mpsc::Sender<(PathBuf, Git)>,
    git_rx: mpsc::Receiver<(PathBuf, Git)>,
    update_rx: mpsc::Receiver<String>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// Folder sizes arrive here: the panel's folder, the folder measured, its bytes.
    sizes_tx: mpsc::Sender<(PathBuf, PathBuf, u64)>,
    sizes_rx: mpsc::Receiver<(PathBuf, PathBuf, u64)>,
    /// Searches to run, (generation, query, mode, folder), and their answers by generation.
    search_tx: mpsc::Sender<(u64, String, u8, PathBuf)>,
    search_rx: mpsc::Receiver<(u64, Results)>,
    search_gen: u64,
    /// Ask in Find file: the turns so far, the answer on its way, and its stop flag. Closing
    /// Find file forgets them.
    pub chat: Vec<Turn>,
    ask_rx: Option<mpsc::Receiver<AskMsg>>,
    ask_stop: Arc<std::sync::atomic::AtomicBool>,
    /// When `tell` last looked.
    told: Instant,
    /// The stop flag of each panel's measuring.
    measuring: [Arc<std::sync::atomic::AtomicBool>; 2],
    run: Option<Run>,
    /// The copy, move, delete, extract or pack running on its thread; one at a time.
    job: Option<Job>,
    /// A history's commits, listed on a thread (git can take seconds): the folder, the name
    /// to put the cursor on, the listing.
    list_tx: mpsc::Sender<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    list_rx: mpsc::Receiver<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    last_click: Option<(Instant, u16, u16)>,
    quit: bool,
}

/// A file operation on its thread. It says how far it is (the item it is on), then what failed.
struct Job {
    op: Option<Transfer>,
    src: Vec<PathBuf>,
    dst: PathBuf,
    password: Option<String>,
    /// The panel and the name to put its cursor on when done.
    select: Option<(usize, String)>,
    /// What the status line says meanwhile.
    line: String,
    rx: mpsc::Receiver<JobMsg>,
}

enum JobMsg {
    At(usize),
    Done(Vec<(PathBuf, std::io::Error)>),
}

/// The searching thread: it runs the newest search asked for, after a pause for more typing,
/// and skips the ones typed over meanwhile.
#[allow(clippy::type_complexity)]
fn searcher(index: Arc<Client>, max: usize) -> (mpsc::Sender<(u64, String, u8, PathBuf)>, mpsc::Receiver<(u64, Results)>) {
    let (ask, asked) = mpsc::channel::<(u64, String, u8, PathBuf)>();
    let (tell, told) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(mut job) = asked.recv() {
            // Typing goes on: wait for it to pause, and take the last word of it.
            loop {
                while let Ok(newer) = asked.try_recv() {
                    job = newer;
                }
                std::thread::sleep(Duration::from_millis(80));
                match asked.try_recv() {
                    Ok(newer) => job = newer,
                    Err(_) => break,
                }
            }
            let (generation, query, mode, dir) = job;
            let found = match mode {
                2 => index.search_text(&query, max),
                _ => index.search(&query, (mode == 1).then_some(dir.as_path()), max),
            };
            if tell.send((generation, found)).is_err() {
                return;
            }
        }
    });
    (ask, told)
}

/// What a key does to a command line that has text.
#[derive(Debug, PartialEq)]
enum LineKey {
    Run,
    Erase,
    Clear,
    Type(char),
    /// An editing key the line cannot use (it edits at its end only): swallowed, so Delete
    /// never deletes the entry under the cursor while typing a command.
    Keep,
}

/// `None`: the key is the panel's after all (a chord such as Ctrl+Enter, or an action key).
fn line_key(key: Key, plain: Option<char>) -> Option<LineKey> {
    let bare = !key.ctrl && !key.alt;
    Some(match (key.code, plain) {
        (KeyCode::Enter, _) if bare => LineKey::Run,
        (KeyCode::Backspace, _) if bare => LineKey::Erase,
        (KeyCode::Esc, _) => LineKey::Clear,
        (KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End, _) if bare => LineKey::Keep,
        (_, Some(c)) => LineKey::Type(c),
        _ => return None,
    })
}

fn shell() -> (String, &'static str) {
    if cfg!(windows) {
        ("cmd".into(), "/C")
    } else {
        (std::env::var("SHELL").unwrap_or_else(|_| "sh".into()), "-c")
    }
}

impl App {
    fn new(cfg: Config, left: PathBuf, right: PathBuf) -> Result<App, String> {
        let index = Client::start(&cfg.search);
        App::with_index(cfg, left, right, index)
    }

    fn with_index(cfg: Config, left: PathBuf, right: PathBuf, index: Arc<Client>) -> Result<App, String> {
        let keymap = cfg.keymap()?;
        let (git_tx, git_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let (sizes_tx, sizes_rx) = mpsc::channel();
        let (list_tx, list_rx) = mpsc::channel();
        let (search_tx, search_rx) = searcher(index.clone(), cfg.search.max_results);
        if cfg.check_updates {
            std::thread::spawn(move || {
                if let Some(v) = coxswain_core::update::check() {
                    let _ = update_tx.send(v);
                }
            });
        }
        let show_hidden = cfg.show_hidden;
        let app = App {
            keymap,
            theme: cfg.theme(),
            glyphs: cfg.glyphs(),
            panels: [Panel::new(left, show_hidden), Panel::new(right, show_hidden)],
            active: 0,
            cmdline: String::new(),
            dialog: None,
            status: None,
            quick: None,
            show_hidden,
            index: index.clone(),
            areas: [Rect::default(); 2],
            git_tx,
            git_rx,
            update_rx,
            sizer: Arc::new(coxswain_core::sizes::Sizer::new(Some(index.clone()))),
            sizes_tx,
            sizes_rx,
            search_tx,
            search_rx,
            search_gen: 0,
            chat: vec![],
            ask_rx: None,
            ask_stop: Arc::default(),
            told: Instant::now(),
            measuring: Default::default(),
            run: None,
            job: None,
            list_tx,
            list_rx,
            last_click: None,
            quit: false,
            cfg,
        };
        let mut app = app;
        app.refresh_git();
        app.measure(0);
        app.measure(1);
        Ok(app)
    }

    pub fn panel(&self) -> &Panel {
        &self.panels[self.active]
    }

    fn panel_mut(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    pub fn key_label(&self, a: Action) -> &str {
        self.cfg.key_for(a).unwrap_or("")
    }

    /// git status, and the last commit of each entry, run on a thread; results arrive
    /// through `git_rx`.
    fn refresh_git(&self) {
        let dirs: HashSet<PathBuf> = self.panels.iter().map(|p| p.dir.clone()).collect();
        let last = self.cfg.git.last_commit;
        for dir in dirs {
            let tx = self.git_tx.clone();
            std::thread::spawn(move || {
                let st = git::Status::read(&dir);
                let repo = st.is_some() || history::is_history(&dir);
                if tx.send((dir.clone(), Git::Status(st))).is_ok() && last && repo {
                    let _ = tx.send((dir.clone(), Git::Last(history::last_changes(&dir))));
                }
            });
        }
    }

    /// These were added, removed or written: the folders they are in are measured again.
    fn changed(&mut self, paths: &[PathBuf]) {
        for p in paths {
            self.sizer.forget(p);
            self.panels.iter_mut().for_each(|panel| panel.sizes.retain(|folder, _| !p.starts_with(folder)));
        }
    }

    /// Measure the folders in a panel on a thread, the first ones first; results arrive
    /// through `sizes_rx`. The run before it in that panel stops.
    fn measure(&mut self, side: usize) {
        use std::sync::atomic::{AtomicBool, Ordering};
        // A history's folders are not on disk.
        if !self.cfg.folder_sizes || history::is_history(&self.panels[side].dir) {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        std::mem::replace(&mut self.measuring[side], stop.clone()).store(true, Ordering::Relaxed);
        let p = &self.panels[side];
        let (dir, tx, sizer) = (p.dir.clone(), self.sizes_tx.clone(), self.sizer.clone());
        let folders: Vec<PathBuf> = p.entries.iter().filter(|e| e.is_dir && !e.is_parent() && !p.sizes.contains_key(&e.path)).map(|e| e.path.clone()).collect();
        std::thread::spawn(move || {
            for f in folders {
                let Some((bytes, _)) = sizer.measure(&f, &stop) else { continue };
                if stop.load(Ordering::Relaxed) || tx.send((dir.clone(), f, bytes)).is_err() {
                    return;
                }
            }
        });
    }

    fn reload(&mut self) {
        self.load(0, None);
        self.load(1, None);
        self.refresh_git();
        self.measure(0);
        self.measure(1);
    }

    fn cd(&mut self, side: usize, dir: PathBuf) {
        let came = self.panels[side].enter(dir.clone());
        self.load(side, came);
        self.refresh_git();
        self.measure(side);
        // A locked archive, looked into: its password, kept for this run, then again.
        if self.panels[side].error.as_deref().is_some_and(|e| e.contains(coxswain_core::archive::LOCKED)) {
            self.input(&t!("archive.locked_title"), t!("archive.locked_label"), String::new(), Prompt::Unlock(side, dir));
        }
    }

    /// List a panel's folder again, the cursor on `select` (or where it is). A history's
    /// commits come from git, which can take seconds on a big repository: on a thread, the
    /// status line saying so meanwhile.
    fn load(&mut self, side: usize, select: Option<String>) {
        let h = self.show_hidden;
        let p = &mut self.panels[side];
        if !history::is_history(&p.dir) {
            p.load(h);
            if let Some(n) = select {
                p.select_name(&n);
            }
            return;
        }
        let keep = select.or_else(|| p.current().map(|e| e.name.clone()));
        let (dir, tx) = (p.dir.clone(), self.list_tx.clone());
        self.status = Some(t!("status.busy", "what" => Self::describe(std::slice::from_ref(&dir))));
        std::thread::spawn(move || {
            let r = bfs::list(&dir, h);
            let _ = tx.send((dir, keep, r));
        });
    }

    /// A listing from the thread, for the panels still showing its folder. One that failed
    /// walks up, as `Panel::load` does, and the status line says why.
    fn listed(&mut self, dir: PathBuf, keep: Option<String>, r: std::io::Result<Vec<Entry>>) {
        let busy = t!("status.busy", "what" => Self::describe(std::slice::from_ref(&dir)));
        if self.status.as_deref() == Some(busy.as_str()) {
            self.status = None;
        }
        for side in 0..2 {
            let p = &mut self.panels[side];
            if p.dir != dir {
                continue;
            }
            match &r {
                Ok(v) => {
                    p.entries = v.clone();
                    p.error = None;
                    p.fill(keep.clone());
                }
                Err(e) => {
                    p.error = Some(e.to_string());
                    self.status = p.error.clone();
                    if let Some(up) = p.dir.parent() {
                        p.dir = up.to_path_buf();
                        self.load(side, None);
                    }
                }
            }
        }
    }

    fn input(&mut self, title: &str, label: String, value: String, prompt: Prompt) {
        self.dialog = Some(Dialog::Input { title: title.into(), label, value, prompt });
    }

    fn describe(paths: &[PathBuf]) -> String {
        match paths {
            [one] => format!("\"{}\"", one.file_name().unwrap_or_default().to_string_lossy()),
            _ => tn!("items", paths.len()),
        }
    }

    // ------------------------------------------------------------ input

    fn on_key(&mut self, ev: KeyEvent) {
        let Some(key) = to_key(ev) else { return };
        self.status = None;
        if self.dialog.is_some() {
            return self.dialog_key(key);
        }
        let plain_char = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        // Quick search: Alt+letter starts it; further letters extend it.
        if let Some(q) = &mut self.quick {
            match (key.code, plain_char) {
                (_, Some(c)) => {
                    q.push(c);
                    return self.quick_jump();
                }
                (KeyCode::Char(c), None) if key.alt && !key.ctrl => {
                    q.push(c);
                    return self.quick_jump();
                }
                (KeyCode::Backspace, _) => {
                    q.pop();
                    return;
                }
                (KeyCode::Esc | KeyCode::Enter, _) => {
                    self.quick = None;
                    if key.code == KeyCode::Esc {
                        return;
                    }
                }
                _ => self.quick = None,
            }
        }
        let action = self.keymap.get(&key).copied();
        if let (KeyCode::Char(c), true, false, None) = (key.code, key.alt, key.ctrl, action) {
            self.quick = Some(c.to_string());
            return self.quick_jump();
        }
        if let Some(what) = (!self.cmdline.is_empty()).then(|| line_key(key, plain_char)).flatten() {
            return match what {
                LineKey::Run => self.run_cmdline(),
                LineKey::Erase => drop(self.cmdline.pop()),
                LineKey::Clear => self.cmdline.clear(),
                LineKey::Type(c) => self.cmdline.push(c),
                LineKey::Keep => {}
            };
        }
        match (action, plain_char) {
            (Some(a), _) => self.act(a),
            (None, Some(c)) => self.cmdline.push(c),
            _ => {}
        }
    }

    fn quick_jump(&mut self) {
        let q = self.quick.clone().unwrap_or_default().to_lowercase();
        let p = self.panel_mut();
        if let Some(i) = p.entries.iter().position(|e| e.name.to_lowercase().starts_with(&q)) {
            p.cursor = i;
        }
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::Quit => self.quit = true,
            Action::Up => self.panel_mut().move_cursor(-1),
            Action::Down => self.panel_mut().move_cursor(1),
            Action::PageUp => {
                let n = self.panel().page as isize - 1;
                self.panel_mut().move_cursor(-n.max(1))
            }
            Action::PageDown => {
                let n = self.panel().page as isize - 1;
                self.panel_mut().move_cursor(n.max(1))
            }
            Action::Home => self.panel_mut().cursor = 0,
            Action::End => self.panel_mut().move_cursor(isize::MAX / 2),
            Action::SwitchPanel => self.active ^= 1,
            Action::Open => self.open(),
            // Where `..` leads: out of a history's commits, back to the folder on disk.
            Action::Parent => {
                let up = self.panel().entries.first().filter(|e| e.is_parent()).map(|e| e.path.clone());
                if let Some(p) = up.or_else(|| self.panel().dir.parent().map(Path::to_path_buf)) {
                    self.cd(self.active, p);
                }
            }
            Action::History => self.history(),
            Action::Mark => {
                let p = self.panel_mut();
                p.toggle_mark(p.cursor);
                p.move_cursor(1);
            }
            Action::SelectGroup | Action::UnselectGroup => {
                let sel = a == Action::SelectGroup;
                let title = if sel { t!("tui.select") } else { t!("tui.unselect") };
                self.input(&title, t!("tui.files_matching"), "*".into(), Prompt::Select(sel));
            }
            Action::InvertSelection => {
                let p = self.panel_mut();
                let files: Vec<usize> = (0..p.entries.len()).filter(|&i| !p.entries[i].is_dir).collect();
                files.into_iter().for_each(|i| p.toggle_mark(i));
            }
            Action::Refresh => {
                self.reload();
                self.status = Some(t!("status.reread"));
            }
            Action::SwapPanels => {
                self.panels.swap(0, 1);
                self.active ^= 1;
            }
            Action::TogglePanels => self.run = Some(Run::ShowOutput),
            Action::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.reload();
            }
            Action::GotoLeft | Action::GotoRight => {
                let side = (a == Action::GotoRight) as usize;
                let cur = self.panels[side].dir.to_string_lossy().into_owned();
                let title = if side == 0 { t!("tui.left_panel") } else { t!("tui.right_panel") };
                self.input(&title, t!("tui.goto"), cur, Prompt::Goto(side));
            }
            Action::SameDir => {
                let d = self.panel().dir.clone();
                self.cd(self.active ^ 1, d);
            }
            Action::SortName | Action::SortExt | Action::SortTime | Action::SortSize => {
                let key = match a {
                    Action::SortName => SortKey::Name,
                    Action::SortExt => SortKey::Ext,
                    Action::SortTime => SortKey::Time,
                    _ => SortKey::Size,
                };
                let p = self.panel_mut();
                // Picking the current key again reverses it.
                p.reverse = p.sort == key && !p.reverse;
                p.sort = key;
                p.resort();
            }
            Action::CopyPath => {
                if let Some(e) = self.panel().current().filter(|e| !e.is_parent()) {
                    let q = config::quote(&e.name);
                    if !self.cmdline.is_empty() && !self.cmdline.ends_with(' ') {
                        self.cmdline.push(' ');
                    }
                    self.cmdline += &q;
                    self.cmdline.push(' ');
                }
            }
            Action::View | Action::Edit => {
                if let Some(e) = self.panel().current().filter(|e| !e.is_dir).map(|e| e.path.clone()) {
                    // Inside an archive the file is not on disk: F3 views a copy of it, and
                    // editing waits until it is copied out.
                    if let Some((archive, _)) = coxswain_core::archive::split(&self.panel().dir) {
                        if a == Action::View {
                            return self.peek(e);
                        }
                        return self.status = Some(t!("archive.copy_out_hint", "archive" => archive.file_name().unwrap_or_default().to_string_lossy()));
                    }
                    // In a history: viewed as it was then, from a copy; never edited.
                    if history::is_history(&e) {
                        if a == Action::Edit {
                            return self.status = Some(t!("history.read_only"));
                        }
                        return match history::peek(&e) {
                            Ok(copy) => self.view_or_edit(a, &copy),
                            Err(err) => self.status = Some(err.to_string()),
                        };
                    }
                    if a == Action::View && self.cfg.bom_viewer && coxswain_core::bom::sniff(&e) {
                        match bom::Viewer::open(&e) {
                            Ok(v) => return self.dialog = Some(Dialog::Bom(Box::new(v))),
                            // Not readable as a BOM after all: the pager shows it as it is.
                            Err(err) => self.status = Some(err.to_string()),
                        }
                    }
                    self.view_or_edit(a, &e);
                }
            }
            Action::Copy | Action::Move => {
                let src = self.panel().targets();
                if src.is_empty() {
                    return;
                }
                let dst = self.panels[self.active ^ 1].dir.to_string_lossy().into_owned();
                let (title, label) = if a == Action::Copy { ("dialog.copy", "dialog.copy_to") } else { ("dialog.move", "dialog.move_to") };
                let label = t!(label, "what" => Self::describe(&src));
                let prompt = if a == Action::Copy { Prompt::Copy(src) } else { Prompt::Move(src) };
                self.input(&t!(title), label, dst, prompt);
            }
            Action::Extract => {
                let src: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| coxswain_core::archive::is_archive(p)).collect();
                if src.is_empty() {
                    return self.status = Some(t!("app.not_archive"));
                }
                let dst = self.panels[1 - self.active].dir.display().to_string();
                let label = t!("app.extract_into", "what" => Self::describe(&src));
                self.input(&t!("app.extract"), label, dst, Prompt::Extract(src));
            }
            Action::Pack => {
                let src = self.panel().targets();
                let Some(first) = src.first() else { return };
                let name = if src.len() == 1 { first.file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { self.panel().dir.file_name().map_or("archive".into(), |n| n.to_string_lossy().into_owned()) };
                let dst = self.panels[1 - self.active].dir.join(format!("{name}.zip")).display().to_string();
                let label = t!("archive.pack_into", "what" => Self::describe(&src));
                self.input(&t!("archive.pack"), label, dst, Prompt::Pack(src));
            }
            Action::Mkdir => self.input(&t!("dialog.new_folder"), t!("tui.mkdir_label"), String::new(), Prompt::Mkdir),
            Action::Delete | Action::DeleteForever => {
                let paths = self.panel().targets();
                if paths.is_empty() {
                    return;
                }
                // Inside an archive there is no trash: it is taken out of the archive, written anew.
                let inside = coxswain_core::archive::split(&self.panel().dir);
                let forever = a == Action::DeleteForever || inside.is_some();
                if self.cfg.confirm_delete {
                    let text = match inside {
                        Some((archive, _)) => t!("confirm.archive_remove", "what" => Self::describe(&paths), "archive" => archive.file_name().unwrap_or_default().to_string_lossy()),
                        None => t!(if forever { "confirm.delete_forever" } else { "confirm.trash" }, "what" => Self::describe(&paths)),
                    };
                    self.dialog = Some(Dialog::Confirm { title: t!("dialog.delete"), text, paths, forever });
                } else {
                    self.delete(paths, forever);
                }
            }
            Action::Search => {
                self.dialog = Some(Dialog::Search { query: String::new(), mode: 0, results: Results::default(), cursor: 0, offset: 0 });
            }
            Action::UserMenu => {
                let items = self
                    .cfg
                    .user_menu
                    .iter()
                    .enumerate()
                    .map(|(i, u)| MenuItem { key: u.key.clone(), label: u.label.clone(), run: MenuRun::User(i) })
                    .collect();
                self.dialog = Some(Dialog::Menu { title: t!("tui.user_menu"), filter: String::new(), items, cursor: 0, direct: true });
            }
            Action::Menu => {
                let items = Action::ALL
                    .iter()
                    .filter(|&&x| !x.gui_only() && !matches!(x, Action::Menu | Action::Up | Action::Down))
                    .map(|&x| MenuItem { key: self.key_label(x).to_string(), label: x.label().to_string(), run: MenuRun::Action(x) })
                    .collect();
                self.dialog = Some(Dialog::Menu { title: t!("menu.commands"), filter: String::new(), items, cursor: 0, direct: false });
            }
            Action::Help => self.dialog = Some(Dialog::Help { scroll: 0 }),
            // Asked for: measure afresh, whatever is remembered and whether or not sizes are on.
            Action::DirSizes => {
                if coxswain_core::archive::split(&self.panel().dir).is_some() {
                    return;
                }
                let (side, on) = (self.active, std::mem::replace(&mut self.cfg.folder_sizes, true));
                for e in self.panels[side].entries.iter().filter(|e| e.is_dir) {
                    self.sizer.forget(&e.path);
                }
                self.panels[side].sizes.clear();
                self.measure(side);
                self.cfg.folder_sizes = on;
            }
            a => self.status = Some(t!("tui.gui_only", "action" => a.label())),
        }
    }

    /// F3 on a file inside an archive: a copy of it in the viewer, or its password asked for.
    fn peek(&mut self, path: PathBuf) {
        match coxswain_core::archive::peek(&path) {
            Ok(copy) => self.view_or_edit(Action::View, &copy),
            Err(e) if e.to_string().contains(coxswain_core::archive::LOCKED) => {
                self.input(&t!("archive.locked_title"), t!("archive.locked_label"), String::new(), Prompt::Peek(path));
            }
            Err(e) => self.status = Some(e.to_string()),
        }
    }

    fn open(&mut self) {
        let Some(e) = self.panel().current().cloned() else { return };
        // An archive opens like a folder; its files are copied out with F5. An archive inside
        // one is a file like the others.
        let inside = coxswain_core::archive::split(&self.panel().dir);
        if e.is_dir || (inside.is_none() && coxswain_core::archive::is_archive(&e.path)) {
            return self.cd(self.active, e.path);
        }
        if let Some((archive, _)) = inside {
            return self.status = Some(t!("archive.copy_out_hint", "archive" => archive.file_name().unwrap_or_default().to_string_lossy()));
        }
        if history::is_history(&self.panel().dir) {
            return self.status = Some(t!("history.file_hint"));
        }
        let dir = self.panel().dir.clone();
        if e.is_exec {
            let cmd = if cfg!(windows) { config::quote(&e.name) } else { format!("./{}", config::quote(&e.name)) };
            self.run = Some(Run::Shell { cmd, dir, wait: true });
            return;
        }
        self.status = Some(match bfs::open_default(&e.path) {
            Ok(()) => t!("status.opened", "name" => e.name),
            Err(err) => t!("status.open_failed", "error" => err),
        });
    }

    /// Into the history of the entry under the cursor (of this folder on `..`): its commits.
    fn history(&mut self) {
        let p = self.panel();
        let target = match p.current() {
            Some(e) if !e.is_parent() => e.path.clone(),
            _ => p.dir.clone(),
        };
        if history::is_history(&target) || coxswain_core::archive::split(&target).is_some() || !target.exists() {
            return self.status = Some(t!("history.not_here"));
        }
        if p.git.is_none() {
            return self.status = Some(t!("history.no_repo"));
        }
        self.cd(self.active, history::path(&target, None));
    }

    fn view_or_edit(&mut self, a: Action, file: &Path) {
        let env = |v: &str| std::env::var(v).ok().filter(|s| !s.is_empty());
        let prog = if a == Action::View {
            self.cfg.viewer.clone().or_else(|| env("PAGER")).unwrap_or_else(|| if cfg!(windows) { "more".into() } else { "less".into() })
        } else {
            self.cfg.editor.clone().or_else(|| env("VISUAL")).or_else(|| env("EDITOR")).unwrap_or_else(|| {
                if cfg!(windows) { "notepad".into() } else { "vi".into() }
            })
        };
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let cmd = format!("{prog} {}", config::quote(&file.to_string_lossy()));
        self.run = Some(Run::Shell { cmd, dir, wait: false });
    }

    fn run_cmdline(&mut self) {
        let cmd = std::mem::take(&mut self.cmdline);
        let t = cmd.trim();
        // `cd` has to change our own directory, so it is built in.
        if t == "cd" || t.starts_with("cd ") {
            let arg = t[2..].trim().trim_matches(['"', '\'']);
            let target = if arg.is_empty() { std::env::home_dir().unwrap_or_default() } else { resolve(&self.panel().dir, arg) };
            if target.is_dir() {
                self.cd(self.active, std::fs::canonicalize(&target).unwrap_or(target));
            } else {
                self.status = Some(t!("status.no_dir", "dir" => arg));
            }
            return;
        }
        self.run = Some(Run::Shell { cmd, dir: self.panel().dir.clone(), wait: false });
    }

    fn delete(&mut self, paths: Vec<PathBuf>, forever: bool) {
        self.transfer(Transfer::Delete(forever), paths, PathBuf::new(), None, None);
    }

    /// Copy, move or extract `src` to `dst`, delete `src`, or make the folder `src`, on a
    /// thread: keys keep working, and the status line says which item it is on. Then the
    /// cursor goes on `select`.
    fn transfer(&mut self, op: Transfer, src: Vec<PathBuf>, dst: PathBuf, password: Option<String>, select: Option<String>) {
        self.start(Some(op), src, dst, password, select);
    }

    /// Run a file operation (`None`: pack `src` into `dst`) on its thread. One at a time: while
    /// one runs, another is not started and the status line says why.
    fn start(&mut self, op: Option<Transfer>, src: Vec<PathBuf>, dst: PathBuf, password: Option<String>, select: Option<String>) {
        if let Some(j) = &self.job {
            return self.status = Some(j.line.clone());
        }
        if matches!(op, Some(Transfer::Move | Transfer::Delete(_) | Transfer::Mkdir)) {
            self.changed(&src);
        } else if op.is_none() {
            self.changed(std::slice::from_ref(&dst));
        } else {
            self.changed(&[dst.join("new")]);
        }
        let (tx, rx) = mpsc::channel();
        let (items, to, pw) = (src.clone(), dst.clone(), password.clone());
        std::thread::spawn(move || {
            let pw = pw.as_deref();
            let Some(op) = op else {
                // A 7z's names are hidden too: what the desktop app does by default.
                let failed = coxswain_core::archive::create_locked(&to, &items, pw, true).err().map(|e| (to.clone(), e));
                return drop(tx.send(JobMsg::Done(failed.into_iter().collect())));
            };
            let mut failed = vec![];
            for (i, p) in items.iter().enumerate() {
                let _ = tx.send(JobMsg::At(i));
                let r = match op {
                    Transfer::Copy => bfs::copy_locked(p, &to, pw).map(drop),
                    Transfer::Move => bfs::rename_locked(p, &to, pw).map(drop),
                    Transfer::Extract => coxswain_core::archive::extract_locked(p, &to, pw).map(drop),
                    Transfer::Delete(true) => bfs::delete_locked(p, pw),
                    Transfer::Delete(false) => bfs::trash_locked(p, pw),
                    Transfer::Mkdir => bfs::mkdir_locked(p, pw),
                };
                if let Err(e) = r {
                    failed.push((p.clone(), e));
                }
            }
            let _ = tx.send(JobMsg::Done(failed));
        });
        let line = t!("status.busy", "what" => Self::describe(&src));
        self.status = Some(line.clone());
        let select = select.map(|n| (self.active, n));
        self.job = Some(Job { op, src, dst, password, select, line, rx });
    }

    /// How far the job is; when it is done, what came of it.
    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let mut done = None;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                JobMsg::At(i) if job.src.len() > 1 => {
                    let what = Self::describe(std::slice::from_ref(&job.src[i]));
                    let line = t!("status.busy_of", "what" => what, "n" => i + 1, "all" => job.src.len());
                    if self.status.as_ref() == Some(&job.line) || self.status.is_none() {
                        self.status = Some(line.clone());
                    }
                    job.line = line;
                }
                JobMsg::At(_) => {}
                JobMsg::Done(failed) => done = Some(failed),
            }
        }
        // A key clears the status line; while the job runs, it comes back.
        if self.status.is_none() {
            self.status = Some(job.line.clone());
        }
        let Some(failed) = done else { return };
        let job = self.job.take().expect("a job");
        self.status = None;
        self.finish(job, failed);
    }

    /// When only locked archives were in the way, ask for the password and run again with it,
    /// for just what was locked.
    fn finish(&mut self, job: Job, failed: Vec<(PathBuf, std::io::Error)>) {
        let Job { op, src, dst, password, select, .. } = job;
        let what = Self::describe(&src);
        let Some(op) = op else {
            let errors = failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect();
            return self.after_op(t!("archive.packed", "what" => what), errors);
        };
        if !failed.is_empty() && failed.iter().all(|(_, e)| e.to_string().contains(coxswain_core::archive::LOCKED)) {
            let label = t!(if password.is_none() { "archive.locked_label" } else { "archive.locked_again" });
            let again = failed.into_iter().map(|(p, _)| p).collect();
            return self.input(&t!("archive.locked_title"), label, String::new(), Prompt::Password(op, again, dst));
        }
        let errors = failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect();
        let ok = match op {
            Transfer::Copy => t!("status.copied", "what" => what),
            Transfer::Move => t!("status.moved", "what" => what),
            Transfer::Extract => t!("app.extracted", "what" => what),
            Transfer::Delete(true) => t!("status.deleted", "what" => what),
            Transfer::Delete(false) => t!("status.trashed", "what" => what),
            Transfer::Mkdir => t!("status.created", "what" => src.first().map(|p| p.display().to_string()).unwrap_or_default()),
        };
        self.after_op(ok, errors);
        if let Some((side, n)) = select {
            self.panels[side].select_name(&n);
        }
    }

    fn after_op(&mut self, ok: String, errors: Vec<String>) {
        self.panels.iter_mut().for_each(Panel::clear_marks);
        self.reload();
        if errors.is_empty() {
            self.status = Some(ok);
        } else {
            self.dialog = Some(Dialog::Message { title: t!("dialog.error"), text: errors.join("\n") });
        }
    }

    fn pack(&mut self, src: Vec<PathBuf>, to: PathBuf, password: Option<String>) {
        self.start(None, src, to, password, None);
    }

    fn submit(&mut self, prompt: Prompt, value: String) {
        let base = self.panel().dir.clone();
        match prompt {
            Prompt::Copy(src) | Prompt::Move(src) | Prompt::Extract(src) | Prompt::Pack(src) if value.trim().is_empty() => drop(src),
            Prompt::Copy(src) => self.transfer(Transfer::Copy, src, resolve(&base, &value), None, None),
            Prompt::Extract(src) => self.transfer(Transfer::Extract, src, resolve(&base, &value), None, None),
            Prompt::Password(op, src, dst) => self.transfer(op, src, dst, Some(value), None),
            Prompt::Peek(path) => {
                if let Some((archive, _)) = coxswain_core::archive::split(&path) {
                    coxswain_core::archive::remember(&archive, &value);
                }
                self.peek(path);
            }
            Prompt::Unlock(side, dir) => {
                if let Some((archive, _)) = coxswain_core::archive::split(&dir) {
                    coxswain_core::archive::remember(&archive, &value);
                }
                self.cd(side, dir);
            }
            Prompt::Move(src) => {
                let dst = resolve(&base, &value);
                let one = (src.len() == 1 && dst.parent() == Some(base.as_path())).then(|| dst.file_name().unwrap_or_default().to_string_lossy().into_owned());
                self.transfer(Transfer::Move, src, dst, None, one);
            }
            Prompt::Pack(src) => {
                let to = resolve(&base, &value);
                if coxswain_core::archive::takes_password(&to) {
                    self.input(&t!("archive.pack"), t!("archive.pack_password"), String::new(), Prompt::PackPassword(src, to));
                } else {
                    self.pack(src, to, None);
                }
            }
            Prompt::PackPassword(src, to) if value.is_empty() => self.pack(src, to, None),
            Prompt::PackPassword(src, to) => self.input(&t!("archive.pack"), t!("archive.pack_confirm"), String::new(), Prompt::PackConfirm(src, to, value)),
            Prompt::PackConfirm(src, to, pw) if pw == value => self.pack(src, to, Some(pw)),
            Prompt::PackConfirm(..) => self.status = Some(t!("archive.pack_mismatch")),
            Prompt::Mkdir if value.trim().is_empty() => {}
            Prompt::Mkdir => {
                let d = resolve(&base, &value);
                let first = d.strip_prefix(&base).ok().and_then(|r| r.components().next()).map(|c| c.as_os_str().to_string_lossy().into_owned());
                self.transfer(Transfer::Mkdir, vec![d], PathBuf::new(), None, first);
            }
            Prompt::Goto(side) => {
                let d = resolve(&self.panels[side].dir, &value);
                let d = std::fs::canonicalize(&d).unwrap_or(d);
                if d.is_dir() {
                    self.cd(side, d);
                } else {
                    self.status = Some(t!("status.not_dir", "dir" => d.display()));
                }
            }
            Prompt::Select(sel) => {
                let pats: Vec<Vec<u8>> = value.split([' ', ';', ',']).filter(|s| !s.is_empty()).map(|s| s.to_lowercase().into_bytes()).collect();
                let p = self.panel_mut();
                for e in p.entries.iter().filter(|e| !e.is_dir) {
                    if pats.iter().any(|g| index::glob(g, e.name.to_lowercase().as_bytes())) {
                        if sel {
                            p.marked.insert(e.path.clone());
                        } else {
                            p.marked.remove(&e.path);
                        }
                    }
                }
                p.recount_marks();
            }
        }
    }

    /// Every few seconds: the terminal's title (the version and the kinds of search on), and
    /// once, a notice of what can be turned on, in the status line. The terminal app has no
    /// dismiss button: a notice shown once counts as seen.
    fn tell(&mut self) {
        if self.told.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.told = Instant::now();
        let now = self.index.status();
        let title = t!("title.window", "version" => coxswain_core::update::VERSION, "search" => coxswain_core::notices::search_level(&self.cfg, &now));
        let _ = execute!(std::io::stdout(), terminal::SetTitle(title));
        if self.status.is_some() || self.dialog.is_some() || now.state != State::Ready {
            return;
        }
        // The desktop app shares the state file: it is written only when something changed.
        let mut st = coxswain_core::state::AppState::load();
        let mut changed = coxswain_core::notices::started(&mut st);
        // The repositories looked into, as the desktop app keeps them (the history notice).
        for g in self.panels.iter().filter_map(|p| p.git.as_ref()) {
            if !st.recent_repos.contains(&g.root) {
                changed |= st.touch_repo(&g.root);
            }
        }
        if let Some(n) = coxswain_core::notices::next(&self.cfg, &now, &st, true) {
            self.status = Some(match n.url {
                Some(url) => format!("{} {url}", n.text),
                None => n.text,
            });
            coxswain_core::notices::dismiss(&mut st, &n.id);
            changed = true;
        }
        if changed {
            let _ = st.save();
        }
    }

    /// Ask for the search in the dialog on the searching thread; the answer comes in `tick`.
    /// Keys are never held up by a search, and a search that a newer one replaces is dropped.
    fn search_now(&mut self) {
        if let Some(Dialog::Search { query, mode, .. }) = &self.dialog {
            self.search_gen += 1;
            let _ = self.search_tx.send((self.search_gen, query.clone(), *mode, self.panel().dir.clone()));
        }
    }

    /// Ask the question in Find file on a thread of its own; the answer comes in `tick`.
    fn ask(&mut self, question: String) {
        use std::sync::atomic::Ordering;
        // The one before stops writing; its turn stays as far as it came.
        self.ask_stop.store(true, Ordering::SeqCst);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.ask_stop = stop.clone();
        let earlier: Vec<coxswain_core::meaning::Turn> = self.chat.iter().filter(|t| t.error.is_none() && !t.answer.is_empty()).map(|t| (t.question.clone(), t.answer.clone())).collect();
        self.chat.push(Turn { question: question.clone(), ..Turn::default() });
        let (tx, rx) = mpsc::channel();
        self.ask_rx = Some(rx);
        let (index, cfg) = (self.index.clone(), self.cfg.search.clone());
        std::thread::spawn(move || {
            // A follow-up is looked up with the question before it, which it often leans on.
            let lookup = earlier.last().map_or(question.clone(), |(q, _)| format!("{q} {question}"));
            let sources = index.passages(&lookup, 10);
            let done = if sources.is_empty() {
                Err(t!("search.ask_nothing"))
            } else if stop.load(Ordering::SeqCst) {
                // Stopped while searching: the model is not asked.
                Ok(())
            } else {
                let _ = tx.send(AskMsg::Sources(sources.iter().map(|(p, _)| p.clone()).collect()));
                coxswain_core::meaning::ask(&cfg, &earlier, &question, &sources, |text| !stop.load(Ordering::SeqCst) && (text.is_empty() || tx.send(AskMsg::Piece(text.to_string())).is_ok()))
            };
            let _ = tx.send(AskMsg::Done(done));
        });
    }

    /// Find file is closed: an answer being written stops, and the questions are forgotten.
    fn forget_chat(&mut self) {
        self.ask_stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.ask_rx = None;
        self.chat.clear();
    }

    fn dialog_key(&mut self, key: Key) {
        let action = self.keymap.get(&key).copied();
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
            _ => None,
        };
        let esc = key.code == KeyCode::Esc || action == Some(Action::Quit);
        let Some(dialog) = self.dialog.take() else { return };
        match dialog {
            Dialog::Input { title, label, mut value, prompt } => match (key.code, ch) {
                _ if esc => {}
                (KeyCode::Enter, _) => self.submit(prompt, value),
                (KeyCode::Backspace, _) => {
                    value.pop();
                    self.dialog = Some(Dialog::Input { title, label, value, prompt });
                }
                (KeyCode::Char('u'), None) if key.ctrl => {
                    value.clear();
                    self.dialog = Some(Dialog::Input { title, label, value, prompt });
                }
                (_, c) => {
                    value.extend(c);
                    self.dialog = Some(Dialog::Input { title, label, value, prompt });
                }
            },
            Dialog::Confirm { title, text, paths, forever } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.delete(paths, forever),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Confirm { title, text, paths, forever }),
            },
            Dialog::Search { mut query, mode: 3, results, mut cursor, offset } => {
                let sources = self.chat.last().map(|t| t.sources.clone()).unwrap_or_default();
                let source = sources.get(cursor).cloned();
                match (key.code, ch) {
                    _ if esc => return self.forget_chat(),
                    (KeyCode::Enter, _) if !query.trim().is_empty() && self.cfg.search.meaning && !self.cfg.search.ask_model.is_empty() => {
                        self.ask(std::mem::take(&mut query).trim().to_string());
                        cursor = 0;
                    }
                    (KeyCode::Enter, _) => {
                        if let Some(path) = source {
                            self.forget_chat();
                            if history::is_history(&path) {
                                return self.cd(self.active, path);
                            }
                            if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
                                self.cd(self.active, dir.to_path_buf());
                                self.panel_mut().select_name(&name.to_string_lossy());
                            }
                            return;
                        }
                    }
                    _ if matches!(action, Some(Action::View | Action::Edit)) => {
                        if let Some(path) = source {
                            self.view_or_edit(action.unwrap(), &path);
                        }
                    }
                    (KeyCode::Tab, _) => {
                        self.dialog = Some(Dialog::Search { query, mode: 0, results: Results::default(), cursor: 0, offset: 0 });
                        return self.search_now();
                    }
                    (KeyCode::Up, _) => cursor = cursor.saturating_sub(1),
                    (KeyCode::Down, _) => cursor = (cursor + 1).min(sources.len().saturating_sub(1)),
                    (KeyCode::Backspace, _) => drop(query.pop()),
                    (_, Some(c)) => query.push(c),
                    _ => {}
                }
                self.dialog = Some(Dialog::Search { query, mode: 3, results, cursor, offset });
            }
            Dialog::Search { mut query, mut mode, results, mut cursor, offset } => {
                let hit = results.hits.get(cursor).cloned();
                let page = 10isize;
                let mut requery = false;
                match (key.code, ch) {
                    _ if esc => return self.forget_chat(),
                    (KeyCode::Enter, _) => {
                        self.forget_chat();
                        // A commit: its folder as it was then.
                        if let Some(h) = hit.as_ref().filter(|h| history::is_history(&h.path)) {
                            self.cd(self.active, h.path.clone());
                            return;
                        }
                        if let Some(h) = hit {
                            let (dir, name) = match (h.path.parent(), h.path.file_name()) {
                                (Some(d), Some(n)) => (d.to_path_buf(), n.to_string_lossy().into_owned()),
                                _ => return,
                            };
                            self.cd(self.active, dir);
                            self.panel_mut().select_name(&name);
                        }
                        return;
                    }
                    _ if matches!(action, Some(Action::View | Action::Edit)) => {
                        if let Some(h) = hit.filter(|h| !h.is_dir && !history::is_history(&h.path)) {
                            self.view_or_edit(action.unwrap(), &h.path);
                        }
                    }
                    (KeyCode::Tab, _) => {
                        mode += 1;
                        requery = mode < 3;
                    }
                    (KeyCode::Up, _) => cursor = cursor.saturating_sub(1),
                    (KeyCode::Down, _) => cursor += 1,
                    (KeyCode::PageUp, _) => cursor = (cursor as isize - page).max(0) as usize,
                    (KeyCode::PageDown, _) => cursor += page as usize,
                    (KeyCode::Backspace, _) => requery = query.pop().is_some(),
                    (_, Some(c)) => {
                        query.push(c);
                        requery = true;
                    }
                    _ => {}
                }
                cursor = cursor.min(results.hits.len().saturating_sub(1));
                self.dialog = Some(Dialog::Search { query, mode, results, cursor, offset });
                if requery {
                    self.search_now();
                }
            }
            Dialog::Menu { title, mut filter, items, mut cursor, direct } => {
                let visible: Vec<usize> = (0..items.len())
                    .filter(|&i| items[i].label.to_lowercase().contains(&filter.to_lowercase()))
                    .collect();
                let run = match (key.code, ch) {
                    _ if esc => return,
                    (KeyCode::Enter, _) => visible.get(cursor).copied(),
                    (_, Some(c)) if direct => items.iter().position(|it| it.key == c.to_string()),
                    (KeyCode::Up, _) => {
                        cursor = cursor.saturating_sub(1);
                        None
                    }
                    (KeyCode::Down, _) => {
                        cursor = (cursor + 1).min(visible.len().saturating_sub(1));
                        None
                    }
                    (KeyCode::Backspace, _) => {
                        filter.pop();
                        cursor = 0;
                        None
                    }
                    (_, Some(c)) => {
                        filter.push(c);
                        cursor = 0;
                        None
                    }
                    _ => None,
                };
                match run.map(|i| &items[i].run) {
                    Some(MenuRun::Action(a)) => self.act(*a),
                    Some(MenuRun::User(i)) => self.run_user(*i),
                    None => self.dialog = Some(Dialog::Menu { title, filter, items, cursor, direct }),
                }
            }
            Dialog::Help { scroll } => match key.code {
                KeyCode::Up => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down => self.dialog = Some(Dialog::Help { scroll: scroll + 1 }),
                KeyCode::PageUp => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown => self.dialog = Some(Dialog::Help { scroll: scroll + 10 }),
                _ => {}
            },
            Dialog::Message { .. } => {}
            Dialog::Bom(mut v) => match v.key(key, action == Some(Action::Quit)) {
                bom::Outcome::Stay => self.dialog = Some(Dialog::Bom(v)),
                bom::Outcome::Close => {}
                bom::Outcome::Source => {
                    self.view_or_edit(Action::View, &v.path.clone());
                    self.dialog = Some(Dialog::Bom(v));
                }
                bom::Outcome::Reveal(file) => {
                    if let (Some(dir), Some(name)) = (file.parent(), file.file_name()) {
                        self.cd(self.active, dir.to_path_buf());
                        self.panel_mut().select_name(&name.to_string_lossy());
                    }
                }
                bom::Outcome::Compare => {
                    match self.panels[self.active ^ 1].current().filter(|e| !e.is_dir && e.path != v.path) {
                        Some(e) => v.compare(&e.path.clone()),
                        None => v.cannot_compare(t!("tui.bom.no_other")),
                    }
                    self.dialog = Some(Dialog::Bom(v));
                }
            },
        }
    }

    fn run_user(&mut self, i: usize) {
        let u = &self.cfg.user_menu[i];
        let p = self.panel();
        let file = p.current().filter(|e| !e.is_parent()).map(|e| e.path.as_path());
        let marked: Vec<PathBuf> = p.entries.iter().filter(|e| p.marked.contains(&e.path)).map(|e| e.path.clone()).collect();
        let cmd = u.expand(&p.dir, file, &marked);
        self.run = Some(Run::Shell { cmd, dir: p.dir.clone(), wait: u.wait });
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let Some(side) = (0..2).find(|&i| self.areas[i].contains((m.column, m.row).into())) else { return };
        if self.dialog.is_some() {
            return;
        }
        let area = self.areas[side];
        match m.kind {
            MouseEventKind::ScrollUp => self.panels[side].move_cursor(-3),
            MouseEventKind::ScrollDown => self.panels[side].move_cursor(3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.active = side;
                // Rows start below the border and the column header.
                let row = m.row.saturating_sub(area.y + 2) as usize;
                let p = &mut self.panels[side];
                let i = p.offset + row;
                if m.row >= area.y + 2 && i < p.entries.len() {
                    p.cursor = i;
                    let double = self.last_click.is_some_and(|(t, x, y)| t.elapsed() < Duration::from_millis(400) && (x, y) == (m.column, m.row));
                    self.last_click = Some((Instant::now(), m.column, m.row));
                    if double {
                        self.last_click = None;
                        self.open();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.active = side;
                let i = self.panels[side].offset + m.row.saturating_sub(area.y + 2) as usize;
                self.panels[side].toggle_mark(i);
            }
            _ => {}
        }
    }

    /// Git results and index progress, polled between events.
    fn tick(&mut self, last_state: &mut State) {
        if let Ok(v) = self.update_rx.try_recv() {
            let how = coxswain_core::update::upgrade_hint().unwrap_or(coxswain_core::update::RELEASES_URL);
            self.status = Some(t!("status.update", "version" => v, "how" => how));
        }
        while let Ok((dir, folder, bytes)) = self.sizes_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                p.sizes.insert(folder.clone(), bytes);
            }
        }
        while let Ok((generation, found)) = self.search_rx.try_recv() {
            if let Some(Dialog::Search { results, cursor, offset, .. }) = &mut self.dialog {
                if generation == self.search_gen {
                    *results = found;
                    *cursor = 0;
                    *offset = 0;
                }
            }
        }
        while let Some(msg) = self.ask_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            let Some(turn) = self.chat.last_mut() else { break };
            match msg {
                AskMsg::Sources(paths) => turn.sources = paths,
                AskMsg::Piece(text) => turn.answer.push_str(&text),
                AskMsg::Done(done) => {
                    turn.error = done.err();
                    self.ask_rx = None;
                }
            }
        }
        while let Ok((dir, keep, r)) = self.list_rx.try_recv() {
            self.listed(dir, keep, r);
        }
        self.poll_job();
        while let Ok((dir, news)) = self.git_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                match &news {
                    Git::Status(st) => p.git = st.clone(),
                    Git::Last(last) => p.last = last.clone(),
                }
            }
        }
        let state = self.index.state();
        if state != *last_state {
            *last_state = state;
            if matches!(self.dialog, Some(Dialog::Search { .. })) {
                self.search_now();
            }
        }
        // Last, so a notice it shows is not written over in the same round.
        self.tell();
    }
}

fn to_key(ev: KeyEvent) -> Option<Key> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let m = ev.modifiers;
    let (ctrl, alt, shift) = (m.contains(KeyModifiers::CONTROL), m.contains(KeyModifiers::ALT), m.contains(KeyModifiers::SHIFT));
    let code = match ev.code {
        CK::F(n) => KeyCode::F(n),
        CK::Char(c) => KeyCode::Char(c),
        CK::Enter => KeyCode::Enter,
        CK::Esc => KeyCode::Esc,
        CK::Tab => KeyCode::Tab,
        CK::BackTab => return Some(Key::new(KeyCode::Tab, ctrl, alt, true)),
        CK::Backspace => KeyCode::Backspace,
        CK::Delete => KeyCode::Delete,
        CK::Insert => KeyCode::Insert,
        CK::Home => KeyCode::Home,
        CK::End => KeyCode::End,
        CK::PageUp => KeyCode::PageUp,
        CK::PageDown => KeyCode::PageDown,
        CK::Up => KeyCode::Up,
        CK::Down => KeyCode::Down,
        CK::Left => KeyCode::Left,
        CK::Right => KeyCode::Right,
        _ => return None,
    };
    Some(Key::new(code, ctrl, alt, shift))
}

/// Leave the TUI, run `f` on the plain terminal, and come back.
fn suspended(term: &mut DefaultTerminal, f: impl FnOnce()) -> std::io::Result<()> {
    use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    let mut out = std::io::stdout();
    execute!(out, DisableMouseCapture, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;
    f();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, EnableMouseCapture)?;
    term.clear()
}

fn run_shell(cmd: &str, dir: &Path, wait: bool) {
    let (sh, flag) = shell();
    println!("{}> {cmd}", dir.display());
    if let Err(e) = run_in(&sh, flag, cmd, dir, wait) {
        println!("coxswain: {sh}: {e}");
    }
}

/// `cmd` through the shell `sh`, on the plain terminal. Ctrl+C there interrupts the whole
/// foreground process group, Coxswain included: it is ignored here meanwhile, and the child
/// gets it as usual.
fn run_in(sh: &str, flag: &str, cmd: &str, dir: &Path, wait: bool) -> std::io::Result<()> {
    let mut c = coxswain_core::tools::command(sh);
    c.arg(flag).arg(cmd).current_dir(dir);
    #[cfg(unix)]
    let before = unsafe {
        use std::os::unix::process::CommandExt;
        c.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGQUIT, libc::SIG_DFL);
            Ok(())
        });
        (libc::signal(libc::SIGINT, libc::SIG_IGN), libc::signal(libc::SIGQUIT, libc::SIG_IGN))
    };
    let status = c.status();
    if wait && status.is_ok() {
        print!("\n{}", t!("tui.press_enter"));
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGINT, before.0);
        libc::signal(libc::SIGQUIT, before.1);
    }
    status.map(drop)
}

fn main_loop(term: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut state = app.index.state();
    // A file operation still running when asked to quit finishes first: cut short, it
    // would leave half a file behind.
    while !app.quit || app.job.is_some() {
        term.draw(|f| ui::draw(f, app))?;
        match app.run.take() {
            Some(Run::Shell { cmd, dir, wait }) => {
                suspended(term, || run_shell(&cmd, &dir, wait))?;
                app.reload();
            }
            Some(Run::ShowOutput) => suspended(term, || {
                // Any key returns to the panels.
                let _ = terminal::enable_raw_mode();
                while !matches!(event::read(), Ok(Event::Key(k)) if k.kind == KeyEventKind::Press) {}
                let _ = terminal::disable_raw_mode();
            })?,
            None => {}
        }
        // Quicker while a search is out, so its answer shows as soon as it comes.
        let waiting = matches!(app.dialog, Some(Dialog::Search { .. }));
        if event::poll(Duration::from_millis(if waiting { 30 } else { 200 }))? {
            match event::read()? {
                Event::Key(k) => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
        }
        app.tick(&mut state);
    }
    Ok(())
}

const USAGE: &str = "coxswain [LEFT] [RIGHT]      a folder, or a file to open its folder with the cursor on it
  --dump-config   print the full default config (redirect it to the config file to customise)
  --config-path   print where the config file is read from
  --paths         print where everything is kept: config, state, index, search store, model
  --index-service on|off   start the search helper with your session, or stop doing so
  --meaning on|off|delete  search by meaning: download the model and turn it on, turn it off,
                           or turn it off and delete the model
  --meaning ollama [MODEL] the vectors from Ollama here (bge-m3 unless named; pulled if missing)
  --meaning server URL MODEL  the vectors from a server with the OpenAI API (Lemonade, LM Studio)
  --meaning builtin        back to the built-in model
  --meaning ask MODEL|off  Ask in Find file: the chat model on that server (Ollama here with the
                           built-in model) that answers questions from your files
  --version
  --help";

/// `--index-service on|off`: register the search helper with the system, or unregister it; the
/// helper running now makes way for the right one.
fn index_service(on: Option<&str>) {
    use coxswain_core::service;
    let done = match on {
        Some("on") => coxswain_core::tools::this_app().and_then(|exe| service::install(&exe)),
        Some("off") => service::uninstall(),
        _ => return println!("{}", if service::installed() { "on" } else { "off" }),
    };
    if let Err(e) = done {
        eprintln!("coxswain: {e}");
        std::process::exit(1)
    }
    Client::start(&Config::load().map(|c| c.search).unwrap_or_default()).restart();
}

/// `--meaning on|off|delete`: search by meaning, as Settings in the desktop app does it.
fn meaning(what: Option<&str>, rest: &[String]) {
    use coxswain_core::meaning;
    let fail = |e: String| -> ! {
        eprintln!("coxswain: {e}");
        std::process::exit(1)
    };
    let save = |key: &str, value: &str| drop(Config::save_value(&["search", key], value.into()).unwrap_or_else(|e| fail(e)));
    match what {
        // Ollama on this machine makes the vectors; the model is pulled when it is not there.
        Some("ollama") => {
            let model = rest.first().map_or("bge-m3", String::as_str);
            let have = meaning::server_models(false, "", None).unwrap_or_else(|e| fail(e));
            if !have.iter().any(|m| m == model || m.split(':').next() == Some(model)) {
                eprintln!("{}", t!("tui.meaning_pulling", "model" => model));
                meaning::ollama_pull("", model, &meaning::Progress::default()).unwrap_or_else(|e| fail(e.to_string()));
            }
            save("meaning_engine", "ollama");
            save("meaning_url", "");
            save("meaning_model", model);
            Config::save_value(&["search", "meaning"], true.into()).unwrap_or_else(|e| fail(e));
        }
        // Any server with the OpenAI API: `--meaning server http://evo:8000/api/v1 <model>`.
        Some("server") => {
            let (Some(url), Some(model)) = (rest.first(), rest.get(1)) else { fail(t!("tui.meaning_server_usage")) };
            let key = meaning::key_of(&Config::load().map(|c| c.search).unwrap_or_default());
            meaning::server_models(true, url, key.as_deref()).unwrap_or_else(|e| fail(e));
            save("meaning_engine", "openai");
            save("meaning_url", url);
            save("meaning_model", model);
            Config::save_value(&["search", "meaning"], true.into()).unwrap_or_else(|e| fail(e));
        }
        Some("builtin") => {
            save("meaning_engine", "builtin");
            return meaning(Some("on"), &[]);
        }
        // The model is downloaded for the built-in engine; a server needs none.
        Some("on") => {
            if !meaning::installed() && Config::load().is_ok_and(|c| c.search.meaning_engine == "builtin") {
                let p = std::sync::Arc::new(meaning::Progress::default());
                let q = p.clone();
                let shown = std::thread::spawn(move || {
                    use std::sync::atomic::Ordering;
                    while q.total.load(Ordering::Relaxed) == 0 || q.done.load(Ordering::Relaxed) < q.total.load(Ordering::Relaxed) {
                        let (done, total) = (q.done.load(Ordering::Relaxed), q.total.load(Ordering::Relaxed).max(1));
                        eprint!("\r{}", t!("tui.meaning_downloading", "percent" => done * 100 / total));
                        if q.cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    eprintln!();
                });
                let done = meaning::download(&p);
                p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = shown.join();
                done.unwrap_or_else(|e| fail(e.to_string()));
            }
            Config::save_value(&["search", "meaning"], true.into()).unwrap_or_else(|e| fail(e));
        }
        Some("off") => drop(Config::save_value(&["search", "meaning"], false.into()).unwrap_or_else(|e| fail(e))),
        Some("delete") => {
            Config::save_value(&["search", "meaning"], false.into()).unwrap_or_else(|e| fail(e));
            meaning::remove().unwrap_or_else(|e| fail(e.to_string()));
        }
        // Ask: a chat model on the server (Ollama on this machine when the vectors are built in).
        Some("ask") => {
            let Some(model) = rest.first() else { fail(t!("tui.ask_usage")) };
            if model == "off" {
                return save("ask_model", "");
            }
            let search = Config::load().map(|c| c.search).unwrap_or_default();
            let openai = search.meaning_engine == "openai";
            let url = if search.meaning_engine == "builtin" { "" } else { search.meaning_url.as_str() };
            let have = meaning::server_models(openai, url, meaning::key_of(&search).as_deref()).unwrap_or_else(|e| fail(e));
            if !have.iter().any(|m| m == model || m.split(':').next() == Some(model)) {
                if openai {
                    fail(t!("tui.ask_no_model", "model" => model));
                }
                eprintln!("{}", t!("tui.meaning_pulling", "model" => model));
                meaning::ollama_pull(url, model, &meaning::Progress::default()).unwrap_or_else(|e| fail(e.to_string()));
            }
            return save("ask_model", model);
        }
        _ => {
            let on = Config::load().is_ok_and(|c| c.search.meaning && (c.search.meaning_engine != "builtin" || meaning::installed()));
            return println!("{}", if on { "on" } else { "off" });
        }
    }
    Client::start(&Config::load().map(|c| c.search).unwrap_or_default()).restart();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // Started by an app, not by hand: hold the file name index for all of them.
        Some(helper::ARG) => return drop(helper::serve()),
        Some("--help" | "-h") => return println!("{USAGE}"),
        Some("--version" | "-V") => return println!("coxswain {}", coxswain_core::update::VERSION),
        Some("--dump-config") => return print!("{}", Config::default().to_toml()),
        Some("--config-path") => return println!("{}", Config::path().map(|p| p.display().to_string()).unwrap_or_default()),
        Some("--paths") => {
            for (what, path) in Config::paths() {
                println!("{what:<13} {}", path.map(|p| p.display().to_string()).unwrap_or_default());
            }
            return;
        }
        Some("--index-service") => return index_service(args.get(1).map(String::as_str)),
        Some("--meaning") => return meaning(args.get(1).map(String::as_str), &args[2.min(args.len())..]),
        _ => {}
    }
    let cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("coxswain: {e}");
        std::process::exit(2)
    });
    coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&cfg.language));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let dir = |i: usize| {
        let d = args.get(i).map(|a| resolve(&cwd, a)).unwrap_or_else(|| cwd.clone());
        std::fs::canonicalize(&d).unwrap_or(d)
    };
    let mut app = App::new(cfg, dir(0), dir(1)).unwrap_or_else(|e| {
        eprintln!("coxswain: {e}");
        std::process::exit(2)
    });
    let mut term = ratatui::init();
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::EnableMouseCapture);
    let res = main_loop(&mut term, &mut app);
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::DisableMouseCapture);
    ratatui::restore();
    if let Err(e) = res {
        eprintln!("coxswain: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_argument_opens_its_folder_on_the_file() {
        let d = std::env::temp_dir().join(format!("coxswain-test-panel-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for f in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let p = Panel::new(d.join("b.txt"), false);
        assert_eq!(p.dir, d);
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("b.txt"));
        assert_eq!(Panel::new(d.clone(), false).dir, d);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_typed_command_line_keeps_the_editing_keys() {
        let k = |code, ctrl, shift| Key::new(code, ctrl, false, shift);
        assert_eq!(line_key(k(KeyCode::Delete, false, false), None), Some(LineKey::Keep));
        assert_eq!(line_key(k(KeyCode::Delete, false, true), None), Some(LineKey::Keep));
        assert_eq!(line_key(k(KeyCode::Enter, false, false), None), Some(LineKey::Run));
        // Ctrl+Enter puts the path on the line; it does not run it.
        assert_eq!(line_key(k(KeyCode::Enter, true, false), None), None);
        assert_eq!(line_key(k(KeyCode::Char('a'), false, false), Some('a')), Some(LineKey::Type('a')));
        assert_eq!(line_key(k(KeyCode::F(5), false, false), None), None);
    }

    /// An app on two folders, with its own index (no helper) and no update check.
    fn app(left: PathBuf, right: PathBuf) -> App {
        let cfg = Config { check_updates: false, ..Config::default() };
        let index = Client::with(None, &cfg.search, || {});
        App::with_index(cfg, left, right, index).unwrap()
    }

    #[test]
    fn a_copy_runs_on_its_thread_one_at_a_time() {
        let d = std::env::temp_dir().join(format!("coxswain-test-job-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for f in ["one.txt", "two.txt"] {
            std::fs::write(a.join(f), f).unwrap();
        }
        let mut app = app(a.clone(), b.clone());
        app.transfer(Transfer::Copy, vec![a.join("one.txt"), a.join("two.txt")], b.clone(), None, None);
        assert!(app.job.is_some(), "it runs on its own; keys are taken meanwhile");
        // Another while it runs is not started, and the status line says what is going on.
        app.transfer(Transfer::Delete(true), vec![a.join("one.txt")], PathBuf::new(), None, None);
        assert!(app.status.as_deref().is_some_and(|s| s.contains('2')), "{:?}", app.status);
        let t = Instant::now();
        while app.job.is_some() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
            app.poll_job();
        }
        assert!(app.job.is_none());
        assert_eq!(std::fs::read_to_string(b.join("two.txt")).unwrap(), "two.txt");
        assert!(a.join("one.txt").exists(), "the delete asked for meanwhile did not run");
        assert_eq!(app.status, Some(t!("status.copied", "what" => tn!("items", 2))));
        assert!(app.panels[1].entries.iter().any(|e| e.name == "one.txt"), "the panels are read again");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_history_is_listed_on_a_thread_and_walks_up_when_there_is_none() {
        let d = std::env::temp_dir().join(format!("coxswain-test-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let mut app = app(d.clone(), d.clone());
        let h = history::path(&d.join("sub"), None);
        app.cd(0, h.clone());
        assert_eq!((app.panels[0].dir.as_path(), app.panels[0].entries.len()), (h.as_path(), 0), "not listed yet");
        assert!(app.status.is_some(), "the status line says it is being read");
        while app.panels[0].dir == h {
            let (dir, keep, r) = app.list_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            app.listed(dir, keep, r);
        }
        assert_eq!(app.panels[0].dir, d.join("sub"), "not a repository: back to the folder");
        assert!(app.status.is_some(), "and says why");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn an_interrupt_meant_for_the_command_leaves_the_app_running() {
        // The child interrupts its parent (this process); ignored here, the test goes on.
        run_in("sh", "-c", "kill -INT $PPID", Path::new("/"), false).unwrap();
    }
}

/// Benchmarks on a big folder, ignored by default: `cargo test --release -p coxswain
/// --bin coxswain -- --ignored --nocapture perf_`. `COXSWAIN_BENCH_DIR` is the folder with
/// the data `coxswain-core`'s `tests/perf.rs` makes (`flat-100000` in it).
#[cfg(test)]
mod perf {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    #[ignore]
    fn perf_terminal_app_100k() {
        let Some(dir) = std::env::var_os("COXSWAIN_BENCH_DIR").map(|d| PathBuf::from(d).join("flat-100000")).filter(|d| d.is_dir()) else {
            return println!("no COXSWAIN_BENCH_DIR/flat-100000: run coxswain-core's perf_list_and_sort_100k first");
        };
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let mut app = App::new(Config::default(), dir.clone(), dir).unwrap();
        println!("App::new with 100k in both panels (list, sort): {:.0} ms", ms(t));
        let mut term = Terminal::new(TestBackend::new(200, 50)).unwrap();
        let t = Instant::now();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        println!("first frame: {:.2} ms", ms(t));
        let t = Instant::now();
        for _ in 0..100 {
            term.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
        println!("frame, nothing changed: {:.2} ms", ms(t) / 100.0);
        let t = Instant::now();
        for _ in 0..100 {
            app.act(Action::Down);
            term.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
        println!("Down + frame: {:.2} ms", ms(t) / 100.0);
        for e in app.panels[0].entries.iter().skip(1) {
            app.panels[0].marked.insert(e.path.clone());
        }
        let t = Instant::now();
        for _ in 0..100 {
            term.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
        println!("frame with every entry marked: {:.2} ms", ms(t) / 100.0);
        let t = Instant::now();
        app.act(Action::SortSize);
        println!("sort by size: {:.0} ms", ms(t));
    }
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    #[test]
    fn marks_keep_their_bytes_and_a_sort_keeps_the_cursor() {
        let d = std::env::temp_dir().join(format!("coxswain-test-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (f, n) in [("a.txt", 1), ("b.txt", 20), ("c.txt", 300)] {
            std::fs::write(d.join(f), vec![0u8; n]).unwrap();
        }
        let mut p = Panel::new(d.clone(), false);
        p.toggle_mark(0); // `..` is never marked
        p.toggle_mark(2);
        p.toggle_mark(3);
        assert_eq!((p.marked.len(), p.marked_bytes), (2, 320));
        p.toggle_mark(2);
        assert_eq!((p.marked.len(), p.marked_bytes), (1, 300));
        p.cursor = 3;
        p.sort = SortKey::Size;
        p.resort();
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("c.txt"));
        assert_eq!(p.cursor, 1);
        assert_eq!(p.marked_bytes, 300);
        p.load(false);
        assert_eq!(p.marked_bytes, 300, "a reread counts again");
        p.clear_marks();
        assert_eq!((p.marked.len(), p.marked_bytes), (0, 0));
        std::fs::remove_dir_all(d).unwrap();
    }
}

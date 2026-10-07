//! Coxswain TUI: two panels, a command line and a function-key bar, Norton Commander style.

mod bom;
mod provenance;
mod guide;
mod settings;
mod setup;
mod ui;

use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Config, Glyphs, Key, KeyCode};
use coxswain_core::fs::{self as bfs, resolve, Entry, SortKey};
use coxswain_core::git;
use coxswain_core::history::{self, Lasts};
use coxswain_core::helper::{self, Client};
use coxswain_core::find::{self, Found, Kind, Off, Row};
use coxswain_core::index::{self, State};
use coxswain_core::undo::{Kind as UndoKind, Record};
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
    /// The ZFS dataset the folder is on (or whose snapshots it lists), when `zfs` has answered.
    pub zfs: Option<coxswain_core::zfs::Facts>,
    /// The package whose files the folder lists.
    pub package: Option<String>,
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
            zfs: None,
            package: None,
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
        // Walk up until something is listable (the directory may have been deleted). Why the
        // folder asked for was not is kept: a locked 7z asks for its password by it.
        let mut why = None;
        loop {
            match bfs::list(&self.dir, show_hidden) {
                Ok(v) => {
                    self.entries = v;
                    self.error = why;
                    break;
                }
                Err(e) => {
                    why.get_or_insert_with(|| e.to_string());
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
        bfs::sort_in(&self.dir, &mut self.entries, self.sort, self.reverse);
        // Inside an archive a folder's size comes with the listing; there is nothing to measure.
        // A snapshot's is its own space.
        if coxswain_core::archive::split(&self.dir).is_some() || coxswain_core::zfs::split(&self.dir).is_some() {
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
        bfs::sort_in(&self.dir, &mut self.entries, self.sort, self.reverse);
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

    /// Mark every entry but `..`, as a file explorer's select all does.
    fn mark_all(&mut self) {
        for i in 0..self.entries.len() {
            if !self.marked.contains(&self.entries[i].path) {
                self.toggle_mark(i);
            }
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
        self.zfs = None;
        self.package = None;
        // Coming up out of a directory (or a history): the cursor on it, as NC does.
        from.strip_prefix(&self.dir).ok().and_then(|r| r.components().next()).map(|n| n.as_os_str().to_string_lossy().into_owned())
    }

    fn select_name(&mut self, name: &str) {
        // A commit's or a branch's folder is named by its commit id, listed by its subject.
        if let Some(i) = self.entries.iter().position(|e| e.name == name).or_else(|| self.entries.iter().position(|e| e.path.file_name().is_some_and(|n| n == name))) {
            self.cursor = i;
        }
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    /// What the action menu and the hints are about: the marked entries (or the one under the
    /// cursor), and where they are.
    fn subject(&self) -> coxswain_core::menu::Subject {
        use coxswain_core::menu::{Place, Subject};
        let cur = self.current().filter(|e| !e.is_parent());
        let list: Vec<&Entry> = if self.marked.is_empty() { cur.into_iter().collect() } else { self.entries.iter().filter(|e| self.marked.contains(&e.path)).collect() };
        let at = history::split(&self.dir);
        let place = if coxswain_core::archive::split(&self.dir).is_some() {
            Place::Archive
        } else if at.as_ref().is_some_and(|at| at.view == history::View::Branches && at.commit.is_none()) {
            Place::Branches
        } else if at.is_some() || history::is_history(&self.dir) {
            Place::History
        } else if coxswain_core::zfs::is_read_only(&self.dir) {
            Place::Snapshot
        } else if coxswain_core::bsd::split(&self.dir).is_some() {
            Place::Package
        } else {
            Place::Disk
        };
        Subject {
            files: list.iter().filter(|e| !e.is_dir).count(),
            folders: list.iter().filter(|e| e.is_dir).count(),
            archives: list.iter().filter(|e| !e.is_dir && coxswain_core::archive::is_archive(&e.path)).count(),
            one: list.len() == 1 && cur.is_some_and(|c| c.path == list[0].path),
            marked: self.marked.len(),
            place,
            git: self.git.is_some(),
            zfs: self.zfs.is_some(),
        }
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
    /// A new name for this entry, in its folder.
    Rename(PathBuf),
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
    /// A new branch's name: in this folder (or list of branches), from this branch of the list.
    NewBranch(PathBuf, Option<String>),
    Goto(usize),
    Mark(bool),
    /// The user flags to set on a file; the others are cleared.
    Flags(PathBuf),
}

impl Prompt {
    /// What Enter does, as its button would say: Copy, Create, Unlock …
    pub fn verb(&self) -> String {
        t!(match self {
            Prompt::Copy(_) => "verb.copy",
            Prompt::Move(_) => "verb.move",
            Prompt::Rename(_) => "verb.rename",
            Prompt::Extract(_) => "verb.extract",
            Prompt::Pack(_) | Prompt::PackPassword(..) | Prompt::PackConfirm(..) => "verb.pack",
            Prompt::Password(..) | Prompt::Unlock(..) | Prompt::Peek(_) => "verb.unlock",
            Prompt::Mkdir | Prompt::NewBranch(..) => "verb.create",
            Prompt::Flags(_) => "common.apply",
            Prompt::Goto(_) => "verb.go",
            Prompt::Mark(true) => "verb.mark",
            Prompt::Mark(false) => "verb.unmark",
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Transfer {
    Copy,
    Move,
    /// A move in place, under a new name (Shift+F6).
    Rename,
    Extract,
    /// Delete, for good with `true` (inside an archive always: it is taken out).
    Delete(bool),
    /// A new folder, the one path given.
    Mkdir,
}

pub enum MenuRun {
    Action(Action),
    User(usize),
    /// A panel to a folder (a boot environment, a jail); none: the line to type a path in.
    Goto(usize, Option<PathBuf>),
    /// Cannot be gone to: why, on the status line.
    Say(String),
}

pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub run: MenuRun,
    /// The heading it is listed under, if the menu has headings.
    pub group: Option<String>,
}

pub enum Dialog {
    Input { title: String, label: String, value: String, prompt: Prompt },
    /// Delete `paths`; `forever` skips the trash.
    Confirm { title: String, text: String, paths: Vec<PathBuf>, forever: bool },
    /// Switch to the branch `entry` of the list of branches `dir`.
    Switch { title: String, text: String, dir: PathBuf, entry: String },
    /// Find: the query, the kind chip, whether it is limited to the panel's folder (`here`),
    /// what was found, the cursor (on a row of `find::rows`, or on a source of the answer), the
    /// first row in sight, and what shows. Ask's questions and answers are in `App::chat`.
    Search { query: String, chip: Kind, here: bool, found: Found, cursor: usize, offset: usize, show: Show },
    /// `direct`: a typed key runs the item with that key (F2); otherwise it filters (F9).
    Menu { title: String, filter: String, items: Vec<MenuItem>, cursor: usize, direct: bool },
    Help { scroll: u16 },
    Message { title: String, text: String },
    /// A CycloneDX BOM, full screen (F3 on one).
    Bom(Box<bom::Viewer>),
    Provenance(Box<provenance::Viewer>),
    /// Settings, full screen (F9 → Settings, `--settings`).
    Settings(Box<settings::Settings>),
    /// The first-run guide, full screen (the first start, F1 → G, Settings → Overview).
    Guide(Box<guide::Guide>),
}

/// An error under `title` (what could not be done): its cause in one line, the raw text under
/// Details when it says more.
fn failure(title: String, raw: &str) -> Dialog {
    let cause = coxswain_core::fs::cause(raw);
    let text = if cause == raw.trim() { cause } else { format!("{cause}\n\n{}:\n{}", t!("dialog.details"), raw.trim()) };
    Dialog::Message { title, text }
}

/// What Find shows under its field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    List,
    /// The questions asked and their answers.
    Answer,
    /// The name syntax and the prefixes (F1).
    Syntax,
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
/// Also its ZFS dataset, and the package whose files it lists.
enum Git {
    Status(Option<Box<git::Status>>),
    Last(Option<Arc<Lasts>>),
    Zfs(Option<Box<coxswain_core::zfs::Facts>>),
    Package(Option<String>),
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
    /// The hint the command line shows while it is empty (`menu::hint`), and what it was
    /// worked out for.
    pub hint: Option<(&'static str, String)>,
    hint_for: Option<coxswain_core::menu::Subject>,
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
    /// Searches to run, and their answers by generation.
    search_tx: mpsc::Sender<FindJob>,
    search_rx: mpsc::Receiver<(u64, Found)>,
    search_gen: u64,
    /// Ask in Find file: the turns so far, the answer on its way, and its stop flag. Closing
    /// Find file forgets them.
    pub chat: Vec<Turn>,
    ask_rx: Option<mpsc::Receiver<AskMsg>>,
    ask_stop: Arc<std::sync::atomic::AtomicBool>,
    /// The chat model Ask last checked, the answer on its way, and why it cannot answer.
    ask_checked: Option<String>,
    ask_check_rx: Option<mpsc::Receiver<Option<String>>>,
    pub ask_problem: Option<String>,
    /// The Find tips sent away for good (Delete), read when Find opens.
    pub find_dismissed: Vec<String>,
    /// When `tell` last looked.
    told: Instant,
    /// The stop flag of each panel's measuring.
    measuring: [Arc<std::sync::atomic::AtomicBool>; 2],
    run: Option<Run>,
    /// The copy, move, delete, extract or pack running on its thread; one at a time.
    job: Option<Job>,
    /// What Ctrl+Z undoes, the newest last.
    undo: coxswain_core::undo::History,
    /// A history's commits, listed on a thread (git can take seconds): the folder, the name
    /// to put the cursor on, the listing.
    list_tx: mpsc::Sender<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    list_rx: mpsc::Receiver<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    last_click: Option<(Instant, u16, u16)>,
    /// Properties being read on a thread.
    props_rx: Option<mpsc::Receiver<Result<bfs::Props, String>>>,
    /// The panels' folders and their repositories' `.git`, watched: what changes on disk is
    /// read again.
    watch: coxswain_core::fs::Watch,
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
    /// What failed, and what can be undone of what did not.
    Done(Vec<(PathBuf, std::io::Error)>, Record),
    Undone(coxswain_core::undo::Undone),
}

/// A search for Find: its generation, the query, the kind chip, the folder it is limited to,
/// and how many hits a group shows.
type FindJob = (u64, String, Kind, Option<PathBuf>, usize);

/// The searching thread: it runs the newest search asked for, after a pause for more typing,
/// and skips the ones typed over meanwhile.
fn searcher(index: Arc<Client>) -> (mpsc::Sender<FindJob>, mpsc::Receiver<(u64, Found)>) {
    let (ask, asked) = mpsc::channel::<FindJob>();
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
            let (generation, query, kind, scope, rows) = job;
            let found = index.find(&query, scope.as_deref(), kind, rows);
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

/// A folder listed by a program (git, zfs, pkg), which can take a while: read on a thread.
fn virtual_list(dir: &Path) -> bool {
    history::is_history(dir) || coxswain_core::zfs::split(dir).is_some() || coxswain_core::bsd::split(dir).is_some()
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
        let (search_tx, search_rx) = searcher(index.clone());
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
            hint: None,
            hint_for: None,
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
            ask_checked: None,
            ask_check_rx: None,
            ask_problem: None,
            find_dismissed: vec![],
            told: Instant::now(),
            measuring: Default::default(),
            run: None,
            job: None,
            undo: Default::default(),
            list_tx,
            list_rx,
            last_click: None,
            props_rx: None,
            watch: Default::default(),
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

    /// The action's name in F1 and F9: Undo says what it would undo.
    pub fn action_label(&self, a: Action) -> String {
        match self.undo.next() {
            Some(r) if a == Action::Undo => t!("undo.menu", "what" => r.label()),
            _ => a.label(),
        }
    }

    /// git status, and the last commit of each entry, run on a thread; results arrive
    /// through `git_rx`.
    fn refresh_git(&self) {
        let dirs: HashSet<PathBuf> = self.panels.iter().map(|p| p.dir.clone()).collect();
        let last = self.cfg.git.last_commit;
        for dir in dirs {
            let tx = self.git_tx.clone();
            let d = dir.clone();
            std::thread::spawn(move || {
                let dir = d;
                let st = git::Status::read(&dir);
                let repo = st.is_some() || history::is_history(&dir);
                if tx.send((dir.clone(), Git::Status(st.map(Box::new)))).is_ok() && last && repo {
                    let _ = tx.send((dir.clone(), Git::Last(history::last_changes(&dir))));
                }
            });
            // `zfs get` and `pkg which` on a thread of their own, so git does not wait for them.
            let tx = self.git_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((dir.clone(), Git::Zfs(coxswain_core::zfs::facts(&dir).map(Box::new))));
                if let Some(file) = coxswain_core::bsd::split(&dir) {
                    let _ = tx.send((dir.clone(), Git::Package(coxswain_core::bsd::package_of(&file))));
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
        // A history's folders are not on disk; a list of snapshots has their sizes, and a
        // package's files are files.
        if !self.cfg.folder_sizes || virtual_list(&self.panels[side].dir) {
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
        if !virtual_list(&p.dir) {
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
        self.status = None;
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
        use unicode_normalization::UnicodeNormalization;
        // NFC on both sides: macOS keeps names decomposed (Hangul syllables, が as か + ゙),
        // typing composes.
        let nfc = |s: &str| s.nfc().collect::<String>().to_lowercase();
        let q = nfc(self.quick.as_deref().unwrap_or_default());
        let p = self.panel_mut();
        if let Some(i) = p.entries.iter().position(|e| nfc(&e.name).starts_with(&q)) {
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
            Action::Snapshots => self.snapshots(),
            Action::Package => self.package(),
            Action::Flags => self.flags(),
            Action::Properties => self.properties(),
            Action::Branches => self.git_view(history::View::Branches),
            Action::Worktrees => self.git_view(history::View::Worktrees),
            Action::SwitchBranch => self.switch_branch(),
            Action::NewBranch => self.new_branch(),
            Action::Mark => {
                let p = self.panel_mut();
                p.toggle_mark(p.cursor);
                p.move_cursor(1);
            }
            Action::MarkGroup | Action::UnmarkGroup => {
                let sel = a == Action::MarkGroup;
                let title = if sel { t!("app.mark_files") } else { t!("app.unmark_files") };
                self.input(&title, t!("tui.files_matching"), "*".into(), Prompt::Mark(sel));
            }
            // As in a file explorer: everything in the folder, files and folders, but `..`.
            Action::MarkAll => self.panel_mut().mark_all(),
            Action::InvertMarks => {
                let p = self.panel_mut();
                let files: Vec<usize> = (0..p.entries.len()).filter(|&i| !p.entries[i].is_dir).collect();
                files.into_iter().for_each(|i| p.toggle_mark(i));
            }
            Action::Refresh => {
                self.reload();
                self.status = Some(t!("status.refreshed"));
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
                let title = if side == 0 { t!("tui.left_panel") } else { t!("tui.right_panel") };
                // On FreeBSD with boot environments or jails, in Termux the phone's folders:
                // those to pick, as NC's drive list, under the line that types a path.
                let places = coxswain_core::bsd::places();
                let phone = coxswain_core::termux::storage();
                if places.is_empty() && phone.is_empty() {
                    return self.goto_prompt(side);
                }
                let mut items = vec![MenuItem { key: String::new(), label: t!("bsd.type_path"), run: MenuRun::Goto(side, None), group: None }];
                items.extend(phone.into_iter().map(|(name, path)| MenuItem { key: String::new(), label: t!("termux.storage", "name" => name), run: MenuRun::Goto(side, Some(path)), group: None }));
                items.extend(places.into_iter().map(|p| {
                    let what = t!(if p.kind == "boot" { "bsd.boot_env" } else { "bsd.jail" });
                    let label = if p.note.is_empty() { format!("{what}: {}", p.name) } else { format!("{what}: {} ({})", p.name, p.note) };
                    let run = match p.path {
                        Some(path) => MenuRun::Goto(side, Some(path)),
                        None => MenuRun::Say(t!(if p.kind == "boot" { "bsd.not_mounted" } else { "bsd.not_readable" }, "name" => p.name)),
                    };
                    MenuItem { key: String::new(), label, run, group: None }
                }));
                self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: false });
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
                    // In a snapshot: viewed as it is there; never edited.
                    if a == Action::Edit && coxswain_core::zfs::in_snapshot(&e).is_some() {
                        return self.status = Some(t!("zfs.read_only"));
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
                    // A database or a Parquet file: its tables and first rows, as text.
                    if a == Action::View && coxswain_core::tables::is_tables(&e) {
                        match coxswain_core::tables::text_copy(&e) {
                            Ok(text) => return self.view_or_edit(a, &text),
                            // Not a database after all: the pager shows it as it is.
                            Err(err) => self.status = Some(err),
                        }
                    }
                    // Asked before the BOM: a statement may carry a CycloneDX predicate.
                    if a == Action::View && self.cfg.provenance_viewer && coxswain_core::provenance::sniff(&e) {
                        let other = Some(self.panels[self.active ^ 1].dir.clone());
                        match provenance::Viewer::open(&e, other) {
                            Ok(v) => return self.dialog = Some(Dialog::Provenance(Box::new(v))),
                            // Not readable as provenance after all: the pager shows it as it is.
                            Err(err) => self.status = Some(err.to_string()),
                        }
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
            // In a ZFS snapshot (or its list) nothing changes: said at once, not after a prompt.
            Action::Move | Action::Rename | Action::NewFolder | Action::Delete | Action::DeleteForever if coxswain_core::zfs::is_read_only(&self.panel().dir) => {
                self.status = Some(t!("zfs.read_only"));
            }
            // Nor in a package's list of files: those belong to pkg.
            Action::Move | Action::Rename | Action::NewFolder | Action::Delete | Action::DeleteForever if coxswain_core::bsd::split(&self.panel().dir).is_some() => {
                self.status = Some(t!("pkg.read_only"));
            }
            Action::Copy | Action::Move => {
                let src = self.panel().targets();
                if src.is_empty() {
                    return;
                }
                let dst = self.panels[self.active ^ 1].dir.to_string_lossy().into_owned();
                let title = t!(if a == Action::Copy { "dialog.copy" } else { "dialog.move" }, "what" => Self::describe(&src));
                let prompt = if a == Action::Copy { Prompt::Copy(src) } else { Prompt::Move(src) };
                self.input(&title, t!("dialog.to"), dst, prompt);
            }
            Action::Extract => {
                let src: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| coxswain_core::archive::is_archive(p)).collect();
                if src.is_empty() {
                    return self.status = Some(t!("app.not_archive"));
                }
                let dst = self.panels[1 - self.active].dir.display().to_string();
                let title = t!("app.extract", "what" => Self::describe(&src));
                self.input(&title, t!("dialog.to"), dst, Prompt::Extract(src));
            }
            Action::Pack => {
                let src = self.panel().targets();
                let Some(first) = src.first() else { return };
                let name = if src.len() == 1 { first.file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { self.panel().dir.file_name().map_or("archive".into(), |n| n.to_string_lossy().into_owned()) };
                let ending = coxswain_core::state::AppState::load().pack_ending().to_string();
                let dst = self.panels[1 - self.active].dir.join(format!("{name}{ending}")).display().to_string();
                let title = t!("archive.pack_title", "what" => Self::describe(&src));
                self.input(&title, t!("tui.pack_into"), dst, Prompt::Pack(src));
            }
            Action::Rename => {
                let Some(e) = self.panel().current().filter(|e| !e.is_parent()).cloned() else { return };
                let title = t!("dialog.rename", "what" => Self::describe(std::slice::from_ref(&e.path)));
                self.input(&title, t!("app.name_label"), e.name, Prompt::Rename(e.path));
            }
            Action::ActionMenu => self.action_menu(),
            Action::NewFolder => self.input(&t!("dialog.new_folder"), t!("tui.mkdir_label"), String::new(), Prompt::Mkdir),
            Action::Undo => self.undo_last(),
            Action::Delete | Action::DeleteForever => {
                let paths = self.panel().targets();
                if paths.is_empty() {
                    return;
                }
                // Inside an archive there is no trash: it is taken out of the archive, written anew.
                let inside = coxswain_core::archive::split(&self.panel().dir);
                let forever = a == Action::DeleteForever || inside.is_some();
                // Coxswain's own trash (Termux, illumos) takes nothing from another filesystem:
                // that is deleted for good, and asked even when deleting is not confirmed.
                let no_trash = !forever && !bfs::trash_takes(&paths);
                let forever = forever || no_trash;
                if self.cfg.confirm_delete || no_trash {
                    let text = match inside {
                        Some((archive, _)) => t!("confirm.archive_remove", "what" => Self::describe(&paths), "archive" => archive.file_name().unwrap_or_default().to_string_lossy()),
                        None if no_trash => t!(if coxswain_core::termux::active() { "termux.no_trash_storage" } else { "confirm.no_trash_here" }, "what" => Self::describe(&paths)),
                        None => t!(if forever { "confirm.delete_forever" } else { "confirm.trash" }, "what" => Self::describe(&paths)),
                    };
                    self.dialog = Some(Dialog::Confirm { title: t!("dialog.delete"), text, paths, forever });
                } else {
                    self.delete(paths, forever);
                }
            }
            Action::Search | Action::SearchText | Action::Ask => {
                let chip = match a {
                    Action::SearchText => Kind::InFiles,
                    Action::Ask => Kind::Ask,
                    _ => Kind::All,
                };
                self.find_dismissed = coxswain_core::state::AppState::load().notices_dismissed;
                let show = if chip == Kind::Ask { Show::Answer } else { Show::List };
                self.dialog = Some(Dialog::Search { query: String::new(), chip, here: false, found: Found::default(), cursor: 0, offset: 0, show });
            }
            Action::UserMenu => {
                let items = self
                    .cfg
                    .user_menu
                    .iter()
                    .enumerate()
                    .map(|(i, u)| MenuItem { key: u.key.clone(), label: u.label.clone(), run: MenuRun::User(i), group: None })
                    .collect();
                self.dialog = Some(Dialog::Menu { title: t!("tui.user_menu"), filter: String::new(), items, cursor: 0, direct: true });
            }
            Action::Menu => {
                let items = Action::ALL
                    .iter()
                    .filter(|&&x| !x.gui_only() && !matches!(x, Action::Menu | Action::Up | Action::Down))
                    .map(|&x| MenuItem { key: self.key_label(x).to_string(), label: self.action_label(x), run: MenuRun::Action(x), group: None })
                    .collect();
                self.dialog = Some(Dialog::Menu { title: t!("menu.commands"), filter: String::new(), items, cursor: 0, direct: false });
            }
            Action::Help => self.dialog = Some(Dialog::Help { scroll: 0 }),
            Action::Settings => self.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(self, "")))),
            // Asked for: measure afresh, whatever is remembered and whether or not sizes are on.
            Action::FolderSizes => {
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

    /// The actions that fit the marked entries (or the one under the cursor), under their
    /// headings, each with its key: Enter runs one, Esc closes.
    fn action_menu(&mut self) {
        let s = self.panel().subject();
        let mut items = vec![];
        for (g, acts) in coxswain_core::menu::actions(&s, false) {
            let group = g.label();
            items.extend(acts.into_iter().map(|a| MenuItem { key: self.key_label(a).to_string(), label: a.label(), run: MenuRun::Action(a), group: Some(group.clone()) }));
        }
        if items.is_empty() {
            return self.status = Some(t!("menu.none"));
        }
        let src = self.panel().targets();
        let what = if src.is_empty() { t!("menu.this_folder") } else { Self::describe(&src) };
        self.dialog = Some(Dialog::Menu { title: t!("menu.title", "what" => what), filter: String::new(), items, cursor: 0, direct: false });
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
        // A file of a package: to its folder, the cursor on it.
        if coxswain_core::bsd::split(&self.panel().dir).is_some() {
            if let (Some(dir), Some(name)) = (e.path.parent(), e.path.file_name()) {
                self.cd(self.active, dir.to_path_buf());
                self.panel_mut().select_name(&name.to_string_lossy());
            }
            return;
        }
        // A file in a snapshot: what changed in it since, in the viewer.
        if coxswain_core::zfs::in_snapshot(&e.path).is_some() {
            return match coxswain_core::zfs::diff_file(&e.path) {
                Ok(Some(diff)) => self.view_or_edit(Action::View, &diff),
                Ok(None) => self.status = Some(t!("zfs.same")),
                Err(err) => self.status = Some(err.to_string()),
            };
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

    /// To the snapshots of the ZFS dataset the folder under the cursor (or this one) is on.
    /// Inside a snapshot: back to the list.
    fn snapshots(&mut self) {
        let p = self.panel();
        if let Some(s) = coxswain_core::zfs::in_snapshot(&p.dir) {
            let live = s.live();
            let to = if live.is_dir() { live } else { s.mountpoint };
            return self.cd(self.active, coxswain_core::zfs::path(&to));
        }
        if virtual_list(&p.dir) || coxswain_core::archive::split(&p.dir).is_some() {
            return self.status = Some(t!("zfs.not_here"));
        }
        let target = match p.current() {
            Some(e) if e.is_dir && !e.is_parent() => e.path.clone(),
            _ => p.dir.clone(),
        };
        if coxswain_core::zfs::dataset_of(&target).is_none() {
            return self.status = Some(t!("zfs.not_zfs", "dir" => target.display()));
        }
        self.cd(self.active, coxswain_core::zfs::path(&target));
    }

    /// To the files of the package the file under the cursor belongs to (FreeBSD).
    fn package(&mut self) {
        let Some(e) = self.panel().current().filter(|e| !e.is_dir).cloned() else { return self.status = Some(t!("pkg.not_here")) };
        if virtual_list(&self.panel().dir) || coxswain_core::archive::split(&self.panel().dir).is_some() {
            return self.status = Some(t!("pkg.not_here"));
        }
        match coxswain_core::bsd::package_of(&e.path) {
            Some(_) => self.cd(self.active, coxswain_core::bsd::path(&e.path)),
            None => self.status = Some(t!("pkg.none", "name" => e.name)),
        }
    }

    /// The user flags of the entry under the cursor, to change in a prompt (FreeBSD, macOS).
    fn flags(&mut self) {
        let Some(e) = self.panel().current().filter(|e| !e.is_parent()).cloned() else { return };
        let f = coxswain_core::flags::read(&e.path).filter(|f| !f.user.is_empty());
        let Some(f) = f else { return self.status = Some(t!("flags.not_here")) };
        if !f.can_set {
            return self.status = Some(t!("flags.not_owner", "name" => e.name));
        }
        let names: Vec<&str> = f.user.iter().map(|(n, _)| *n).collect();
        let on: Vec<&str> = f.user.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect();
        self.input(&t!("action.flags"), t!("flags.prompt", "names" => names.join(" ")), on.join(" "), Prompt::Flags(e.path));
    }

    /// Properties of the entry under the cursor (of this folder on `..`), read on a thread: a
    /// folder's size is a walk of it.
    fn properties(&mut self) {
        let p = self.panel();
        let path = match p.current() {
            Some(e) if !e.is_parent() => e.path.clone(),
            _ => p.dir.clone(),
        };
        if virtual_list(&path) || coxswain_core::zfs::split(&p.dir).is_some() || !path.exists() {
            return self.status = Some(t!("tui.props_not_here"));
        }
        let (tx, rx) = mpsc::channel();
        self.status = Some(t!("app.reading_properties"));
        std::thread::spawn(move || {
            let r = bfs::properties(&path).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.props_rx = Some(rx);
    }

    /// The ZFS footer of a panel: dataset, compression ratio, space; none off ZFS.
    pub fn zfs_line(f: &coxswain_core::zfs::Facts) -> String {
        let size = ui::size;
        if f.quota > 0 {
            t!("zfs.footer_quota", "dataset" => f.dataset, "ratio" => f.compressratio, "used" => size(f.used), "quota" => size(f.quota))
        } else {
            t!("zfs.footer", "dataset" => f.dataset, "ratio" => f.compressratio, "used" => size(f.used), "free" => size(f.available))
        }
    }

    fn goto_prompt(&mut self, side: usize) {
        let cur = self.panels[side].dir.to_string_lossy().into_owned();
        let title = if side == 0 { t!("tui.left_panel") } else { t!("tui.right_panel") };
        self.input(&title, t!("tui.goto"), cur, Prompt::Goto(side));
    }

    /// To the branches or the worktrees of the repository this folder is in.
    fn git_view(&mut self, view: history::View) {
        let p = self.panel();
        // From the branches to the worktrees (and back), and inside a branch.
        let listed = history::split(&p.dir).filter(|at| at.view != history::View::History).map(|at| at.base);
        if listed.is_none() && (history::is_history(&p.dir) || coxswain_core::archive::split(&p.dir).is_some()) {
            return self.status = Some(t!("branches.not_here"));
        }
        let Some(root) = listed.or_else(|| p.git.as_ref().map(|g| g.root.clone())) else { return self.status = Some(t!("branches.no_repo")) };
        self.cd(self.active, history::path_of(&root, view));
    }

    /// The branch under the cursor in the list of branches: asked first, then `git switch`.
    fn switch_branch(&mut self) {
        let p = self.panel();
        let in_list = history::split(&p.dir).is_some_and(|at| at.view == history::View::Branches && at.commit.is_none());
        let Some(e) = p.current().filter(|e| in_list && !e.is_parent()) else {
            return self.status = Some(t!("branches.switch_where", "key" => self.key_label(Action::Branches)));
        };
        let branch = e.name.trim_start_matches("* ").split(' ').next().unwrap_or_default().replace('∕', "/");
        let repo = p.dir.parent().and_then(Path::file_name).unwrap_or_default().to_string_lossy().into_owned();
        let text = t!("branches.switch_text", "branch" => branch, "repo" => repo);
        self.dialog = Some(Dialog::Switch { title: t!("action.switch_branch"), text, dir: p.dir.clone(), entry: e.name.clone() });
    }

    /// A name for a new branch: from the branch under the cursor in the list of branches, or
    /// from the current commit.
    fn new_branch(&mut self) {
        let p = self.panel();
        let in_list = history::split(&p.dir).is_some_and(|at| at.view == history::View::Branches && at.commit.is_none());
        if !in_list && (p.git.is_none() || history::is_history(&p.dir)) {
            return self.status = Some(t!("branches.no_repo"));
        }
        let from = p.current().filter(|e| in_list && !e.is_parent()).map(|e| e.name.clone());
        let label = match &from {
            Some(e) => t!("branches.new_from", "branch" => e.trim_start_matches("* ").split(' ').next().unwrap_or_default().replace('∕', "/")),
            None => t!("branches.new_here"),
        };
        self.input(&t!("action.new_branch"), label, String::new(), Prompt::NewBranch(p.dir.clone(), from));
    }

    /// What git said, on the status line; when it refused, in a dialog under `title` (what could
    /// not be done), as it can be long.
    fn git_said(&mut self, title: String, r: std::io::Result<String>) {
        // Listed again first: a list of branches says it is busy meanwhile, which would hide this.
        self.reload();
        match r {
            Ok(said) => self.status = Some(said.lines().next().unwrap_or_default().to_string()),
            Err(e) => self.dialog = Some(failure(title, &e.to_string())),
        }
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
        if matches!(op, Some(Transfer::Move | Transfer::Rename | Transfer::Delete(_) | Transfer::Mkdir)) {
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
                let mut rec = Record::new(UndoKind::Pack);
                let failed = match coxswain_core::archive::create_locked(&to, &items, pw, true) {
                    Ok(()) => {
                        rec.done(&to, &to);
                        vec![]
                    }
                    Err(e) => vec![(to.clone(), e)],
                };
                return drop(tx.send(JobMsg::Done(failed, rec)));
            };
            // Deleting for good is the one that cannot be undone.
            let mut rec = Record::new(match op {
                Transfer::Copy => UndoKind::Copy,
                Transfer::Move | Transfer::Rename => UndoKind::Move,
                Transfer::Extract => UndoKind::Extract,
                Transfer::Delete(_) => UndoKind::Trash,
                Transfer::Mkdir => UndoKind::Mkdir,
            });
            let mut failed = vec![];
            for (i, p) in items.iter().enumerate() {
                let _ = tx.send(JobMsg::At(i));
                let r = match op {
                    Transfer::Delete(true) => bfs::delete_locked(p, pw),
                    _ => rec.run(p, &to, pw),
                };
                if let Err(e) = r {
                    failed.push((p.clone(), e));
                }
            }
            let _ = tx.send(JobMsg::Done(failed, rec));
        });
        let line = t!("status.busy", "what" => Self::describe(&src));
        self.status = Some(line.clone());
        let select = select.map(|n| (self.active, n));
        self.job = Some(Job { op, src, dst, password, select, line, rx });
    }

    /// How far the job is; when it is done, what came of it.
    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let (mut done, mut undone) = (None, None);
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
                JobMsg::Done(failed, rec) => done = Some((failed, Some(rec))),
                JobMsg::Undone(u) => undone = Some(u),
            }
        }
        if let Some(u) = undone {
            self.job = None;
            self.status = None;
            return self.after_op(t!("undo.done", "what" => u.label), t!("undo.partly", "what" => u.label), u.refused);
        }
        // A key clears the status line; while the job runs, it comes back.
        if self.status.is_none() {
            self.status = Some(job.line.clone());
        }
        let Some((failed, rec)) = done else { return };
        let job = self.job.take().expect("a job");
        self.status = None;
        self.finish(job, failed, rec);
    }

    /// When only locked archives were in the way, ask for the password and run again with it,
    /// for just what was locked.
    fn finish(&mut self, job: Job, failed: Vec<(PathBuf, std::io::Error)>, rec: Option<Record>) {
        let Job { op, src, dst, password, select, .. } = job;
        let what = Self::describe(&src);
        // What can be undone is kept, and the status line says how.
        let key = self.key_label(Action::Undo).to_string();
        let hint = match rec {
            _ if op == Some(Transfer::Delete(false)) && !coxswain_core::undo::trash_restores() => format!(" · {}", t!("undo.no_restore_hint", "key" => key)),
            Some(r) if !r.is_empty() => {
                self.undo.push(r);
                format!(" · {}", t!("undo.hint", "key" => key))
            }
            _ => String::new(),
        };
        let Some(op) = op else {
            let errors = failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect();
            return self.after_op(t!("archive.packed", "what" => what) + &hint, t!("error.pack", "what" => what), errors);
        };
        if !failed.is_empty() && failed.iter().all(|(_, e)| e.to_string().contains(coxswain_core::archive::LOCKED)) {
            let label = t!(if password.is_none() { "archive.locked_label" } else { "archive.locked_again" });
            let again = failed.into_iter().map(|(p, _)| p).collect();
            // Nothing is being worked on while the password is asked for (and after Esc).
            self.status = None;
            return self.input(&t!("archive.locked_title"), label, String::new(), Prompt::Password(op, again, dst));
        }
        let errors = failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect();
        let ok = match op {
            Transfer::Copy => t!("status.copied", "what" => what),
            Transfer::Move => t!("status.moved", "what" => what),
            Transfer::Rename => t!("status.renamed", "what" => what),
            Transfer::Extract => t!("app.extracted", "what" => what),
            Transfer::Delete(true) => t!("status.deleted", "what" => what),
            Transfer::Delete(false) => t!("status.trashed", "what" => what),
            Transfer::Mkdir => t!("status.created", "what" => src.first().map(|p| p.display().to_string()).unwrap_or_default()),
        };
        let fail = match op {
            Transfer::Copy => t!("error.copy", "what" => what),
            Transfer::Move => t!("error.move", "what" => what),
            Transfer::Rename => t!("error.rename", "what" => what),
            Transfer::Extract => t!("error.extract", "what" => what),
            Transfer::Delete(true) => t!("error.delete", "what" => what),
            Transfer::Delete(false) => t!("error.trash", "what" => what),
            Transfer::Mkdir => t!("error.create", "what" => src.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        };
        self.after_op(ok + &hint, fail, errors);
        if let Some((side, n)) = select {
            self.panels[side].select_name(&n);
        }
    }

    /// Done: `ok` on the status line, or the errors under `fail` (what could not be done).
    fn after_op(&mut self, ok: String, fail: String, errors: Vec<String>) {
        self.panels.iter_mut().for_each(Panel::clear_marks);
        self.reload();
        if errors.is_empty() {
            self.status = Some(ok);
        } else {
            self.dialog = Some(failure(fail, &errors.join("\n")));
        }
    }

    /// Ctrl+Z: undo the newest operation in the history, on the job's thread, as it ran.
    fn undo_last(&mut self) {
        if let Some(j) = &self.job {
            return self.status = Some(j.line.clone());
        }
        let Some(rec) = self.undo.pop() else { return self.status = Some(t!("undo.nothing")) };
        self.changed(&rec.touched());
        let line = t!("status.busy", "what" => rec.label());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let u = rec.undo(|i| drop(tx.send(JobMsg::At(i))));
            let _ = tx.send(JobMsg::Undone(u));
        });
        self.status = Some(line.clone());
        self.job = Some(Job { op: None, src: vec![], dst: PathBuf::new(), password: None, select: None, line, rx });
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
            Prompt::Rename(_) if value.trim().is_empty() => {}
            Prompt::Rename(src) => {
                let dst = resolve(&base, &value);
                let one = (dst.parent() == Some(base.as_path())).then(|| dst.file_name().unwrap_or_default().to_string_lossy().into_owned());
                self.transfer(Transfer::Rename, vec![src], dst, None, one);
            }
            Prompt::Move(src) => {
                let dst = resolve(&base, &value);
                let one = (src.len() == 1 && dst.parent() == Some(base.as_path())).then(|| dst.file_name().unwrap_or_default().to_string_lossy().into_owned());
                self.transfer(Transfer::Move, src, dst, None, one);
            }
            Prompt::Pack(src) => {
                let to = resolve(&base, &value);
                // The format is suggested next time, in both apps.
                if let Some(f) = coxswain_core::archive::pack_format(&value) {
                    let mut st = coxswain_core::state::AppState::load();
                    if st.pack_ending != f.endings[0] {
                        st.pack_ending = f.endings[0].into();
                        let _ = st.save();
                    }
                }
                if coxswain_core::archive::takes_password(&to) {
                    self.input(&t!("archive.pack_title", "what" => Self::describe(&src)), t!("archive.pack_password"), String::new(), Prompt::PackPassword(src, to));
                } else {
                    self.pack(src, to, None);
                }
            }
            Prompt::PackPassword(src, to) if value.is_empty() => self.pack(src, to, None),
            Prompt::PackPassword(src, to) => self.input(&t!("archive.pack_title", "what" => Self::describe(&src)), t!("archive.pack_confirm"), String::new(), Prompt::PackConfirm(src, to, value)),
            Prompt::PackConfirm(src, to, pw) if pw == value => self.pack(src, to, Some(pw)),
            Prompt::PackConfirm(..) => self.status = Some(t!("archive.pack_mismatch")),
            Prompt::NewBranch(..) if value.trim().is_empty() => {}
            // ponytail: git runs on the UI thread; a switch in a huge work tree holds the keys meanwhile.
            Prompt::NewBranch(dir, from) => {
                let r = coxswain_core::branches::create(&dir, &value, from.as_deref());
                self.git_said(t!("error.new_branch", "name" => value.trim()), r);
            }
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
            Prompt::Flags(path) => {
                let on: Vec<&str> = value.split([' ', ',']).filter(|s| !s.is_empty()).collect();
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.status = Some(match coxswain_core::flags::set_user(&path, &on) {
                    Ok(()) => t!("flags.set", "name" => name, "flags" => if on.is_empty() { t!("props.flags_none") } else { on.join(", ") }),
                    Err(e) => t!("flags.failed", "name" => name, "error" => e),
                });
            }
            Prompt::Mark(sel) => {
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

    /// Every few seconds: the terminal's title (the version), and
    /// once, a notice of what can be turned on, in the status line. The terminal app has no
    /// dismiss button: a notice shown once counts as seen.
    fn tell(&mut self) {
        if self.told.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.told = Instant::now();
        let now = self.index.status();
        let title = t!("title.window", "version" => coxswain_core::update::VERSION);
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
            self.status = Some(n.text);
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
        if let Some(Dialog::Search { query, chip, here, .. }) = &self.dialog {
            self.search_gen += 1;
            let kind = find::parse(query).0.unwrap_or(*chip);
            let rows = if kind == Kind::All { find::ALL_ROWS } else { self.cfg.search.max_results };
            let _ = self.search_tx.send((self.search_gen, query.clone(), *chip, here.then(|| self.panel().dir.clone()), rows));
        }
    }

    /// Whether Ask can be asked: set up, and its chat model can answer.
    pub fn ask_state(&self) -> Result<(), Off> {
        find::ask_ready(&self.cfg.search)?;
        self.ask_problem.clone().map_or(Ok(()), |p| Err(Off::AskModel(p)))
    }

    /// Find's rows for what it shows now.
    pub fn find_rows(&self, query: &str, chip: Kind, found: &Found) -> Vec<Row> {
        find::rows(query, chip, found, self.ask_state(), |id| self.find_dismissed.iter().any(|d| d == id))
    }

    /// A hit or a source of Find: its folder, with the cursor on it; a commit's folder as it was.
    fn go_to_hit(&mut self, path: &Path) {
        self.forget_chat();
        if history::is_history(path) {
            return self.cd(self.active, path.to_path_buf());
        }
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            self.cd(self.active, dir.to_path_buf());
            self.panel_mut().select_name(&name.to_string_lossy());
        }
    }

    /// The step of a row that says what is missing: start the helper, read the folder too, or
    /// run the setup guide (which needs the terminal: Find closes).
    fn find_step(&mut self, off: Off) {
        match off {
            Off::NoHelper => {
                self.index.restart();
                self.search_now();
            }
            Off::NotRead(dir) => match find::save_read_too(&self.cfg.search, &dir) {
                Ok(cfg) => {
                    self.cfg.search = cfg.search;
                    self.index.restart();
                    self.status = Some(t!("find.reading_too", "folder" => dir.display()));
                    self.search_now();
                }
                Err(e) => self.status = Some(e),
            },
            _ => {
                self.forget_chat();
                self.dialog = None;
                self.setup_guide();
            }
        }
    }

    /// Ask the question in Find file on a thread of its own; the answer comes in `tick`.
    fn ask(&mut self, question: String, scope: Option<PathBuf>) {
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
            // The chat model loads while the sources are looked up.
            coxswain_core::meaning::warm(&cfg);
            // A follow-up is looked up with the question before it, which it often leans on.
            let lookup = earlier.last().map_or(question.clone(), |(q, _)| format!("{q} {question}"));
            let sources = index.passages(&lookup, scope.as_deref(), 10);
            let done = if sources.is_empty() {
                Err(match &scope {
                    Some(dir) => t!("find.ask_nothing_in", "folder" => dir.file_name().unwrap_or_default().to_string_lossy()),
                    None => t!("search.ask_nothing"),
                })
            } else if stop.load(Ordering::SeqCst) {
                // Stopped while searching: the model is not asked.
                Ok(())
            } else {
                let _ = tx.send(AskMsg::Sources(sources.iter().map(|(p, _)| p.clone()).collect()));
                index.answer(&cfg, &earlier, &question, &sources, |text| !stop.load(Ordering::SeqCst) && (text.is_empty() || tx.send(AskMsg::Piece(text.to_string())).is_ok()))
            };
            let _ = tx.send(AskMsg::Done(done));
        });
    }

    /// Find file is closed: an answer being written stops, and the questions are forgotten.
    fn forget_chat(&mut self) {
        self.ask_stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.ask_rx = None;
        self.chat.clear();
        self.ask_checked = None;
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
                // Pack: Tab and Shift+Tab put the next and the previous format's ending on the name.
                (KeyCode::Tab, _) if matches!(prompt, Prompt::Pack(_)) => {
                    value = coxswain_core::archive::cycle_ending(&value, key.shift);
                    self.status = coxswain_core::archive::pack_format(&value).map(|f| t!(&format!("archive.format_{}", f.id)));
                    self.dialog = Some(Dialog::Input { title, label, value, prompt });
                }
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
            Dialog::Switch { title, text, dir, entry } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => {
                    let r = coxswain_core::branches::switch(&dir, &entry);
                    let branch = entry.trim_start_matches("* ").split(' ').next().unwrap_or_default().replace('∕', "/");
                    self.git_said(t!("error.switch", "branch" => branch), r);
                }
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Switch { title, text, dir, entry }),
            },
            Dialog::Search { mut query, mut chip, mut here, found, mut cursor, mut offset, mut show } => {
                let rows = self.find_rows(&query, chip, &found);
                let sources = self.chat.last().map(|t| t.sources.clone()).unwrap_or_default();
                let (prefix, question) = find::parse(&query);
                let question = question.trim().to_string();
                let answer = show == Show::Answer || prefix == Some(Kind::Ask);
                let scope = here.then(|| self.panel().dir.clone());
                let mut requery = false;
                let mut asked = false;
                match (key.code, ch) {
                    _ if esc && show == Show::List => return self.forget_chat(),
                    _ if esc => {
                        show = Show::List;
                        if chip == Kind::Ask {
                            chip = Kind::All;
                        }
                        requery = true;
                    }
                    (KeyCode::F(1), _) => show = if show == Show::Syntax { Show::List } else { Show::Syntax },
                    // The scope: everywhere, or the panel's folder.
                    _ if action == Some(Action::Search) => {
                        here = !here;
                        requery = true;
                    }
                    _ if action == Some(Action::SearchText) => {
                        chip = if chip == Kind::InFiles { Kind::All } else { Kind::InFiles };
                        show = Show::List;
                        requery = true;
                    }
                    // Ask the text typed, from any row: Alt+Enter (Ctrl+Enter where the terminal tells it).
                    _ if action == Some(Action::Ask) || key.code == KeyCode::Enter && (key.alt || key.ctrl) => {
                        if question.is_empty() {
                            chip = Kind::Ask;
                            show = Show::Answer;
                        } else {
                            asked = true;
                        }
                    }
                    (KeyCode::Tab, _) => {
                        chip = chip.next(key.shift);
                        query = find::parse(&query).1.to_string();
                        show = if chip == Kind::Ask { Show::Answer } else { Show::List };
                        requery = true;
                    }
                    (KeyCode::Enter, _) if answer => {
                        if !question.is_empty() {
                            asked = true;
                        } else if let Some(path) = sources.get(cursor) {
                            return self.go_to_hit(&path.clone());
                        }
                    }
                    (KeyCode::Enter, _) => match rows.get(cursor) {
                        Some(Row::Ask { off: None }) => asked = true,
                        Some(Row::Ask { off: Some(off) } | Row::Off { off, .. }) => {
                            let off = off.clone();
                            self.dialog = Some(Dialog::Search { query, chip, here, found, cursor, offset, show });
                            return self.find_step(off);
                        }
                        Some(Row::Hit { hit, .. }) => return self.go_to_hit(&hit.path.clone()),
                        Some(Row::More { group, .. }) => {
                            chip = group.kind();
                            requery = true;
                        }
                        _ => {}
                    },
                    (KeyCode::Delete, _) => {
                        if let Some(id) = rows.get(cursor).and_then(|r| match r {
                            Row::Ask { off: Some(off) } | Row::Off { off, .. } => off.dismiss_id(),
                            _ => None,
                        }) {
                            let mut st = coxswain_core::state::AppState::load();
                            coxswain_core::notices::dismiss(&mut st, id);
                            let _ = st.save();
                            self.find_dismissed.push(id.to_string());
                        }
                    }
                    _ if matches!(action, Some(Action::View | Action::Edit)) => {
                        let path = if answer {
                            sources.get(cursor).cloned()
                        } else {
                            match rows.get(cursor) {
                                Some(Row::Hit { hit, .. }) if !hit.is_dir && !history::is_history(&hit.path) => Some(hit.path.clone()),
                                _ => None,
                            }
                        };
                        if let Some(path) = path {
                            self.view_or_edit(action.unwrap(), &path);
                        }
                    }
                    (KeyCode::Up, _) if answer => cursor = cursor.saturating_sub(1),
                    (KeyCode::Down, _) if answer => cursor = (cursor + 1).min(sources.len().saturating_sub(1)),
                    (KeyCode::Up, _) => cursor = find::step(&rows, cursor, -1),
                    (KeyCode::Down, _) => cursor = find::step(&rows, cursor, 1),
                    (KeyCode::PageUp, _) => cursor = find::step(&rows, cursor, -10),
                    (KeyCode::PageDown, _) => cursor = find::step(&rows, cursor, 10),
                    (KeyCode::Backspace, _) => requery = query.pop().is_some() && !answer,
                    (_, Some(c)) => {
                        query.push(c);
                        requery = show != Show::Answer;
                    }
                    _ => {}
                }
                // Ask not set up: the answer's place says what it needs.
                if asked {
                    if self.ask_state().is_ok() {
                        self.ask(question, scope);
                        query.clear();
                    }
                    (show, cursor) = (Show::Answer, 0);
                }
                if requery {
                    (cursor, offset) = (0, 0);
                }
                self.dialog = Some(Dialog::Search { query, chip, here, found, cursor, offset, show });
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
                    Some(MenuRun::Goto(side, None)) => self.goto_prompt(*side),
                    Some(MenuRun::Goto(side, Some(dir))) => self.cd(*side, dir.clone()),
                    Some(MenuRun::Say(why)) => self.status = Some(why.clone()),
                    None => self.dialog = Some(Dialog::Menu { title, filter, items, cursor, direct }),
                }
            }
            Dialog::Help { .. } if matches!(ch, Some('g' | 'G')) => self.dialog = Some(Dialog::Guide(Box::default())),
            Dialog::Help { scroll } => match key.code {
                KeyCode::Up => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down => self.dialog = Some(Dialog::Help { scroll: scroll + 1 }),
                KeyCode::PageUp => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown => self.dialog = Some(Dialog::Help { scroll: scroll + 10 }),
                _ => {}
            },
            Dialog::Message { .. } => {}
            Dialog::Settings(s) => self.settings_key(s, key, action),
            Dialog::Guide(g) => self.guide_key(g, key, esc),
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
            Dialog::Provenance(mut v) => match v.key(key, action == Some(Action::Quit)) {
                provenance::Outcome::Stay => self.dialog = Some(Dialog::Provenance(v)),
                provenance::Outcome::Close => {}
                provenance::Outcome::Source => {
                    self.view_or_edit(Action::View, &v.path.clone());
                    self.dialog = Some(Dialog::Provenance(v));
                }
                provenance::Outcome::Reveal(file) => {
                    if let (Some(dir), Some(name)) = (file.parent(), file.file_name()) {
                        self.cd(self.active, dir.to_path_buf());
                        self.panel_mut().select_name(&name.to_string_lossy());
                    }
                }
                provenance::Outcome::Cd(dir) => self.cd(self.active, dir),
                provenance::Outcome::Compare => {
                    match self.panels[self.active ^ 1].current().filter(|e| !e.is_dir && e.path != v.path) {
                        Some(e) => v.compare(&e.path.clone()),
                        None => v.cannot_compare(t!("tui.provenance.no_other")),
                    }
                    self.dialog = Some(Dialog::Provenance(v));
                }
                provenance::Outcome::Bom(file) => match bom::Viewer::open(&file) {
                    Ok(b) => self.dialog = Some(Dialog::Bom(Box::new(b))),
                    Err(err) => {
                        self.status = Some(err.to_string());
                        self.dialog = Some(Dialog::Provenance(v));
                    }
                },
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
            // Marks the row, as in NC; or, with `right_click = "menu"`, puts the cursor on it
            // and opens the action menu.
            MouseEventKind::Down(MouseButton::Right) => {
                self.active = side;
                let i = self.panels[side].offset + m.row.saturating_sub(area.y + 2) as usize;
                if self.cfg.right_click != "menu" {
                    return self.panels[side].toggle_mark(i);
                }
                if m.row >= area.y + 2 && i < self.panels[side].entries.len() {
                    self.panels[side].cursor = i;
                    self.action_menu();
                }
            }
            _ => {}
        }
    }

    /// What changed on disk, once it has settled: a panel whose folder changed is read again
    /// (the cursor stays on its name), and git's line when anything did, its `.git` too.
    fn follow_disk(&mut self) {
        let mut want: Vec<PathBuf> = self.panels.iter().map(|p| p.dir.clone()).collect();
        want.extend(self.panels.iter().filter_map(|p| p.git.as_ref()).map(|g| g.root.join(".git")).filter(|g| g.is_dir()));
        want.dedup();
        self.watch.set(want);
        let paths = self.watch.changed();
        if paths.is_empty() {
            return;
        }
        self.changed(&paths);
        for side in 0..2 {
            // macOS names what changed by its real path (`/private/var` for `/var`).
            let dir = &self.panels[side].dir;
            let real = std::fs::canonicalize(dir).ok();
            let here = |p: &Path| p == dir || real.as_deref() == Some(p);
            if paths.iter().any(|p| here(p) || p.parent().is_some_and(here)) {
                self.load(side, None);
                self.measure(side);
            }
        }
        self.refresh_git();
    }

    /// Git results and index progress, polled between events.
    /// The hint for what is under the cursor, worked out again when that changes. Not while
    /// typing, in quick search, a dialog, or with something on the status line: it would be
    /// counted as shown without being seen.
    fn follow_hint(&mut self) {
        if !self.cfg.hints {
            return self.hint = None;
        }
        if self.dialog.is_some() || self.status.is_some() || self.quick.is_some() || !self.cmdline.is_empty() {
            return;
        }
        let s = self.panel().subject();
        if self.hint_for.as_ref() == Some(&s) {
            return;
        }
        let last = self.hint_for.as_ref().zip(self.hint.as_ref().map(|h| h.0));
        self.hint = coxswain_core::menu::hint(&s, &self.cfg, last);
        self.hint_for = Some(s);
    }

    fn tick(&mut self, last_state: &mut State) {
        self.follow_hint();
        if let Ok(v) = self.update_rx.try_recv() {
            let how = coxswain_core::update::upgrade_hint().unwrap_or(coxswain_core::update::RELEASES_URL);
            self.status = Some(t!("status.update", "version" => v, "how" => how));
        }
        while let Ok((dir, folder, bytes)) = self.sizes_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                p.sizes.insert(folder.clone(), bytes);
            }
        }
        while let Ok((generation, now)) = self.search_rx.try_recv() {
            if generation != self.search_gen {
                continue;
            }
            let Some(Dialog::Search { query, chip, .. }) = &self.dialog else { continue };
            let start = find::start(query, &self.find_rows(query, *chip, &now));
            if let Some(Dialog::Search { found, cursor, offset, .. }) = &mut self.dialog {
                (*found, *cursor, *offset) = (now, start, 0);
            }
        }
        // Find open: the chat model is asked whether it can answer, once per opening.
        let model = &self.cfg.search.ask_model;
        if matches!(self.dialog, Some(Dialog::Search { .. })) && find::ask_ready(&self.cfg.search).is_ok() && self.ask_checked.as_ref() != Some(model) {
            self.ask_checked = Some(model.clone());
            self.ask_problem = None;
            let (tx, rx) = mpsc::channel();
            let cfg = self.cfg.search.clone();
            std::thread::spawn(move || tx.send(coxswain_core::meaning::chat_problem(&cfg, false)));
            self.ask_check_rx = Some(rx);
        }
        if let Some(problem) = self.ask_check_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.ask_problem = problem;
            self.ask_check_rx = None;
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
        if let Some(Dialog::Settings(s)) = &mut self.dialog {
            s.poll();
        }
        if let Some(Dialog::Provenance(v)) = &mut self.dialog {
            v.poll();
        }
        if let Some(r) = self.props_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.props_rx = None;
            if self.status.as_deref() == Some(t!("app.reading_properties").as_str()) {
                self.status = None;
            }
            match r {
                Ok(p) => {
                    let title = p.path.file_name().map_or_else(|| p.path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    self.dialog = Some(Dialog::Message { title, text: ui::properties(&p, self.key_label(Action::Snapshots)) });
                }
                Err(e) => self.dialog = Some(failure(t!("action.properties"), &e)),
            }
        }
        while let Ok((dir, keep, r)) = self.list_rx.try_recv() {
            self.listed(dir, keep, r);
        }
        self.poll_job();
        self.follow_disk();
        while let Ok((dir, news)) = self.git_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                match &news {
                    Git::Status(st) => p.git = st.as_deref().cloned(),
                    Git::Last(last) => p.last = last.clone(),
                    Git::Zfs(f) => p.zfs = f.as_deref().cloned(),
                    Git::Package(name) => p.package = name.clone(),
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
        CK::Menu => KeyCode::Menu,
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
                // The setup guide, run from Find, changes the search settings.
                if let Ok(cfg) = Config::load() {
                    app.cfg.search = cfg.search;
                }
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
  --settings[=AREA|OPTION]  start with Settings open (also F9 → Settings): search, previews,
                  looks, behaviour, keys, privacy, or an option such as show_hidden
  --dump-config   print the full default config (redirect it to the config file to customise)
  --config-path   print where the config file is read from
  --paths         print where everything is kept: config, state, index, search store, model
  --setup-search  set up search inside files, by meaning and Ask, step by step: finds the model
                  servers on this machine and what suits it
  --index-service on|off   start the search helper with your session, or stop doing so
  --meaning on|off|delete  search by meaning: download the model and turn it on, turn it off,
                           or turn it off and delete the model
  --meaning ollama [MODEL] meaning read by Ollama here (bge-m3 unless named; pulled if missing)
  --meaning server URL MODEL  meaning read by a server with the OpenAI API (Lemonade, LM Studio)
  --meaning builtin        back to the built-in model
  --meaning cpu|auto       the built-in model on the CPU only, or on the Mac's GPU (Metal)
                           when it has one (auto, the default)
  --meaning ask MODEL|off  Ask in Find: the chat model on that server (Ollama here with the
                           built-in model) that answers questions from your files, or one
                           built in: builtin:qwen3-1.7b, builtin:qwen3-4b (downloaded once);
                           delete: the built-in ones deleted
  --hints reset            show the hints on the command line again, each a few times
  --languages              the languages, by region, and how to help improve a new translation
  --whats-new [all]        what the versions since you last looked brought (all: every version)
  --version
  --help";

/// `--languages`: every language by region, the one in use and the new ones marked.
fn languages() {
    use coxswain_core::i18n::{self, LANGUAGES};
    let cfg = Config::load().unwrap_or_default();
    i18n::set_language(i18n::resolve(&cfg.language));
    let mut group = "";
    for l in LANGUAGES {
        if l.group != group {
            group = l.group;
            println!("{}", t!(group));
        }
        let current = if l.code == i18n::language() { format!("  ({})", t!("app.current")) } else { String::new() };
        let new = if l.new { format!("  {}", t!("news.new")) } else { String::new() };
        println!("  {:<6} {}{new}{current}", l.code, l.name);
    }
    println!("\nlanguage = \"…\" in {}", Config::path().map(|p| p.display().to_string()).unwrap_or_else(|| "config.toml".into()));
    println!("{}\n{}", t!("settings.language_improve"), i18n::IMPROVE_URL);
}

/// `--whats-new [all]`: the changelog, from the versions not read yet (or this one), then read.
fn whats_new(all: bool) {
    use coxswain_core::notices;
    let mut st = coxswain_core::state::AppState::load();
    let unread = notices::unread(&st);
    let shown = if all {
        notices::changes()
    } else if unread.is_empty() {
        notices::changes().into_iter().take(1).collect()
    } else {
        unread
    };
    for c in shown {
        let text: String = c.parts.iter().map(|(t, url)| url.as_ref().map_or_else(|| t.clone(), |u| format!("{t} <{u}>"))).collect();
        println!("{}  {}\n  {text}\n", c.version, c.date);
    }
    notices::read(&mut st);
    let _ = st.save();
}

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
    // A download, with how far it is on one line.
    let download = |get: &dyn Fn(&meaning::Progress) -> std::io::Result<()>| {
        let p = std::sync::Arc::new(meaning::Progress::default());
        let q = p.clone();
        let shown = std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            while q.total.load(Ordering::Relaxed) == 0 || q.done.load(Ordering::Relaxed) < q.total.load(Ordering::Relaxed) {
                let (done, total) = (q.done.load(Ordering::Relaxed), q.total.load(Ordering::Relaxed).max(1));
                eprint!("\r{}", t!("setup.downloading", "percent" => done * 100 / total));
                if q.cancel.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            eprintln!();
        });
        let done = get(&p);
        p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = shown.join();
        done
    };
    // Another model for the vectors reads every file's meaning again: said, and asked, first.
    let confirm = |change: &dyn Fn(&mut coxswain_core::config::SearchConfig)| {
        let old = Config::load().map(|c| c.search).unwrap_or_default();
        let mut new = old.clone();
        change(&mut new);
        let st = Client::start(&old).status();
        if let Some(why) = meaning::change_notice(&old, &new, st.meaning_done, st.meaning_passages) {
            eprint!("{why} {} ", t!("tui.meaning_change_ask"));
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            if !matches!(answer.trim(), "y" | "Y") {
                std::process::exit(0);
            }
        }
    };
    match what {
        // Ollama on this machine makes the vectors; the model is pulled when it is not there.
        Some("ollama") => {
            let model = rest.first().map_or("bge-m3", String::as_str);
            confirm(&|c| (c.meaning_engine, c.meaning_url, c.meaning_model) = ("ollama".into(), String::new(), model.into()));
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
        // Any server with the OpenAI API: `--meaning server http://my-server:8000/api/v1 <model>`.
        Some("server") => {
            let (Some(url), Some(model)) = (rest.first(), rest.get(1)) else { fail(t!("tui.meaning_server_usage")) };
            confirm(&|c| (c.meaning_engine, c.meaning_url, c.meaning_model) = ("openai".into(), url.clone(), model.clone()));
            let key = meaning::key_of(&Config::load().map(|c| c.search).unwrap_or_default());
            meaning::server_models(true, url, key.as_deref()).unwrap_or_else(|e| fail(e));
            save("meaning_engine", "openai");
            save("meaning_url", url);
            save("meaning_model", model);
            Config::save_value(&["search", "meaning"], true.into()).unwrap_or_else(|e| fail(e));
        }
        // Where the built-in model runs: the Mac's GPU when it can (auto), or the CPU only.
        Some(device @ ("cpu" | "auto")) => save("meaning_device", device),
        Some("builtin") => {
            confirm(&|c| c.meaning_engine = "builtin".into());
            save("meaning_engine", "builtin");
            return meaning(Some("on"), &[]);
        }
        // The model is downloaded for the built-in engine; a server needs none.
        Some("on") => {
            if !meaning::installed() && Config::load().is_ok_and(|c| c.search.meaning_engine == "builtin") {
                download(&meaning::download).unwrap_or_else(|e| fail(e.to_string()));
            }
            Config::save_value(&["search", "meaning"], true.into()).unwrap_or_else(|e| fail(e));
        }
        Some("off") => drop(Config::save_value(&["search", "meaning"], false.into()).unwrap_or_else(|e| fail(e))),
        Some("delete") => {
            Config::save_value(&["search", "meaning"], false.into()).unwrap_or_else(|e| fail(e));
            meaning::remove().unwrap_or_else(|e| fail(e.to_string()));
        }
        // Ask: a built-in chat model, or one on the server (Ollama on this machine when the
        // vectors are built in).
        Some("ask") => {
            let Some(model) = rest.first() else { fail(t!("tui.ask_usage")) };
            if model == "off" {
                return save("ask_model", "");
            }
            // The built-in chat models go from the disk, and Ask with them if it used one.
            if model == "delete" {
                if coxswain_core::chat::of(&Config::load().map(|c| c.search.ask_model).unwrap_or_default()).is_some() {
                    save("ask_model", "");
                }
                for m in coxswain_core::chat::MODELS {
                    m.remove().unwrap_or_else(|e| fail(e.to_string()));
                }
                return;
            }
            // A built-in one is downloaded here, once.
            if let Some(m) = coxswain_core::chat::of(model) {
                if !m.installed() {
                    download(&|p| m.download(p)).unwrap_or_else(|e| fail(e.to_string()));
                }
                return save("ask_model", model);
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
            // An embedding model, or one the server will not run, is refused before it is saved.
            if let Some(why) = meaning::chat_problem(&coxswain_core::config::SearchConfig { ask_model: model.clone(), ..search }, true) {
                fail(why);
            }
            return save("ask_model", model);
        }
        _ => {
            let search = Config::load().map(|c| c.search).unwrap_or_default();
            let on = search.meaning && (search.meaning_engine != "builtin" || meaning::installed());
            println!("{}", if on { "on" } else { "off" });
            // The built-in model: on the GPU (Metal) or on the CPU, and why.
            if let Some(runs) = Client::start(&search).status().meaning_runs.filter(|_| on) {
                println!("{}", t!("settings.meaning_runs", "where" => runs.text()));
            }
            return;
        }
    }
    Client::start(&Config::load().map(|c| c.search).unwrap_or_default()).restart();
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--settings[=area|option] [LEFT] [RIGHT]`: start with Settings open, there.
    let open_settings = args.first().and_then(|a| a.strip_prefix("--settings")).filter(|r| r.is_empty() || r.starts_with('=')).map(|r| r.trim_start_matches('=').to_string());
    if open_settings.is_some() {
        args.remove(0);
    }
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
        Some("--languages") => return languages(),
        Some("--hints") if args.get(1).map(String::as_str) == Some("reset") => {
            coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&Config::load().map(|c| c.language).unwrap_or_default()));
            return match coxswain_core::menu::reset() {
                Ok(()) => println!("{}", t!("settings.hints_reset_done")),
                Err(e) => {
                    eprintln!("coxswain: {e}");
                    std::process::exit(1)
                }
            };
        }
        Some("--whats-new") => return whats_new(args.get(1).map(String::as_str) == Some("all")),
        Some("--index-service") => return index_service(args.get(1).map(String::as_str)),
        Some("--setup-search") => {
            coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&Config::load().map(|c| c.language).unwrap_or_default()));
            return setup::run();
        }
        Some("--meaning") => return meaning(args.get(1).map(String::as_str), &args[2.min(args.len())..]),
        _ => {}
    }
    coxswain_core::fs::lock_down();
    // A config.toml of 1.x gets the names of 2.0, once.
    if let Err(e) = coxswain_core::migrate::on_start() {
        eprintln!("coxswain: {e}");
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
    if let Some(section) = open_settings {
        app.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(&app, &section))));
    } else if coxswain_core::state::AppState::load().guide_due() {
        app.dialog = Some(Dialog::Guide(Box::default()));
    }
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
        let index = Client::with(None, &cfg.search, |_| {});
        App::with_index(cfg, left, right, index).unwrap()
    }

    #[test]
    fn quick_search_finds_a_decomposed_name() {
        use unicode_normalization::UnicodeNormalization;
        let d = std::env::temp_dir().join(format!("coxswain-test-quick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // As macOS keeps it: syllables taken apart into their letters.
        let name: String = "한국어.txt".nfd().collect();
        for f in ["a.txt", &name, "z.txt"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let mut app = app(d.clone(), d.clone());
        app.quick = Some("한국".into());
        app.quick_jump();
        assert_eq!(app.panel().current().map(|e| e.name.as_str()), Some(name.as_str()));
        std::fs::remove_dir_all(d).unwrap();
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
        let hint = t!("undo.hint", "key" => "Ctrl+Z");
        assert_eq!(app.status, Some(format!("{} · {hint}", t!("status.copied", "what" => tn!("items", 2)))));
        assert_eq!(app.action_label(Action::Undo), t!("undo.menu", "what" => t!("undo.what.copy", "what" => tn!("items", 2), "dir" => "b")));
        assert!(app.panels[1].entries.iter().any(|e| e.name == "one.txt"), "the panels are read again");
        // The app's threads (sizes, git) may still hold the folder open on Windows.
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ctrl_z_moves_back() {
        let d = std::env::temp_dir().join(format!("coxswain-test-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("one.txt"), "1").unwrap();
        let mut app = app(a.clone(), b.clone());
        let wait = |app: &mut App| {
            let t = Instant::now();
            while app.job.is_some() && t.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(5));
                app.poll_job();
            }
        };
        app.transfer(Transfer::Move, vec![a.join("one.txt")], b.clone(), None, None);
        wait(&mut app);
        assert!(b.join("one.txt").exists());
        app.undo_last();
        wait(&mut app);
        assert!(a.join("one.txt").exists() && !b.join("one.txt").exists());
        assert_eq!(app.status, Some(t!("undo.done", "what" => t!("undo.what.move", "what" => "\"one.txt\"", "dir" => "b"))));
        app.undo_last();
        assert_eq!(app.status, Some(t!("undo.nothing")));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_action_menu_renames_what_is_under_the_cursor() {
        let d = std::env::temp_dir().join(format!("coxswain-test-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("other")).unwrap();
        std::fs::write(d.join("a.txt"), "a").unwrap();
        let mut app = app(d.clone(), d.join("other"));
        app.panels[0].select_name("a.txt");
        // Shift+F10 opens it, its title naming the file, under headings.
        app.on_key(KeyEvent::new(CK::F(10), KeyModifiers::SHIFT));
        let Some(Dialog::Menu { title, items, .. }) = &app.dialog else { panic!("no menu") };
        assert!(title.contains("a.txt"), "{title}");
        let at = items.iter().position(|i| matches!(i.run, MenuRun::Action(Action::Rename))).expect("Rename is listed");
        assert_eq!(items[at].key, "Shift+F6");
        assert!(items.iter().all(|i| i.group.is_some()));
        assert!(!items.iter().any(|i| matches!(i.run, MenuRun::Action(Action::Extract))), "not an archive");
        if let Some(Dialog::Menu { cursor, .. }) = &mut app.dialog {
            *cursor = at;
        }
        app.dialog_key(Key::new(KeyCode::Enter, false, false, false));
        let Some(Dialog::Input { value, .. }) = &mut app.dialog else { panic!("no name asked") };
        assert_eq!(value, "a.txt", "the name to change");
        *value = "b.txt".into();
        app.dialog_key(Key::new(KeyCode::Enter, false, false, false));
        let t = Instant::now();
        while app.job.is_some() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
            app.poll_job();
        }
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "a");
        assert_eq!(app.panel().current().map(|e| e.name.as_str()), Some("b.txt"), "the cursor on the new name");
        // The hint fits the file, and Esc closes the menu without running anything.
        assert!(app.panel().subject().files == 1);
        app.act(Action::ActionMenu);
        app.dialog_key(Key::new(KeyCode::Esc, false, false, false));
        assert!(app.dialog.is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_panel_follows_its_folder_on_disk() {
        let d = std::env::temp_dir().join(format!("coxswain-test-follow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("other")).unwrap();
        std::fs::write(d.join("a.txt"), "").unwrap();
        let mut app = app(d.clone(), d.join("other"));
        app.panels[0].select_name("a.txt");
        app.follow_disk();
        std::fs::write(d.join("b.txt"), "").unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while !app.panels[0].entries.iter().any(|e| e.name == "b.txt") && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
            app.follow_disk();
        }
        assert!(app.panels[0].entries.iter().any(|e| e.name == "b.txt"), "read again");
        assert_eq!(app.panel().current().map(|e| e.name.as_str()), Some("a.txt"), "the cursor stays");
        let _ = std::fs::remove_dir_all(d);
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
        // The app's threads (sizes, git) may still hold the folder open on Windows.
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn find_keys_switch_kind_scope_and_view() {
        let d = std::env::temp_dir();
        let mut app = app(d.clone(), d);
        let k = |c: KeyCode, ctrl: bool, alt: bool, shift: bool| Key::new(c, ctrl, alt, shift);
        let state = |app: &App| match &app.dialog {
            Some(Dialog::Search { chip, here, show, query, .. }) => Some((*chip, *here, *show, query.clone())),
            _ => None,
        };
        app.act(Action::SearchText);
        assert_eq!(state(&app), Some((Kind::InFiles, false, Show::List, String::new())), "Shift+F7 opens at In files");
        app.dialog_key(k(KeyCode::F(7), false, false, true));
        assert_eq!(state(&app).unwrap().0, Kind::All, "pressed again: All");
        app.dialog_key(k(KeyCode::Char('f'), true, false, false));
        assert!(state(&app).unwrap().1, "Ctrl+F: the panel's folder");
        app.dialog_key(k(KeyCode::F(7), false, true, false));
        assert!(!state(&app).unwrap().1, "Alt+F7: everywhere again");
        for want in [Kind::Names, Kind::InFiles, Kind::About, Kind::Ask, Kind::All] {
            app.dialog_key(k(KeyCode::Tab, false, false, false));
            assert_eq!(state(&app).unwrap().0, want);
        }
        app.dialog_key(k(KeyCode::Tab, false, false, true));
        assert_eq!(state(&app).unwrap().0, Kind::Ask, "Shift+Tab goes back");
        app.dialog_key(k(KeyCode::Tab, false, false, false));
        // A prefix goes with Tab.
        for c in "text: fuel".chars() {
            app.dialog_key(k(KeyCode::Char(c), false, false, false));
        }
        app.dialog_key(k(KeyCode::Tab, false, false, false));
        assert_eq!(state(&app).unwrap(), (Kind::Names, false, Show::List, "fuel".into()));
        // Ctrl+F7 with nothing typed: the Ask chip and the answer; Esc back to the list, then closed.
        app.dialog_key(k(KeyCode::Char('u'), true, false, false));
        while state(&app).is_some_and(|s| !s.3.is_empty()) {
            app.dialog_key(k(KeyCode::Backspace, false, false, false));
        }
        app.dialog_key(k(KeyCode::F(7), true, false, false));
        assert_eq!(state(&app).unwrap().0, Kind::Ask);
        assert_eq!(state(&app).unwrap().2, Show::Answer);
        app.dialog_key(k(KeyCode::F(1), false, false, false));
        assert_eq!(state(&app).unwrap().2, Show::Syntax, "F1: the syntax");
        app.dialog_key(k(KeyCode::Esc, false, false, false));
        assert_eq!(state(&app).unwrap().2, Show::List);
        assert_eq!(state(&app).unwrap().0, Kind::All);
        app.dialog_key(k(KeyCode::Esc, false, false, false));
        assert!(app.dialog.is_none(), "Esc in the list closes Find");
    }

    #[test]
    fn tab_in_the_pack_prompt_swaps_the_format() {
        let d = std::env::temp_dir();
        let mut app = app(d.clone(), d);
        app.input("Pack", String::new(), "/x/rocket.zip".into(), Prompt::Pack(vec![]));
        let value = |app: &App| match &app.dialog {
            Some(Dialog::Input { value, .. }) => value.clone(),
            _ => panic!("the prompt stays open"),
        };
        app.dialog_key(Key::new(KeyCode::Tab, false, false, false));
        assert_eq!(value(&app), "/x/rocket.7z");
        assert!(app.status.is_some(), "the format's hint");
        app.dialog_key(Key::new(KeyCode::Tab, false, false, true));
        app.dialog_key(Key::new(KeyCode::Tab, false, false, true));
        assert_eq!(value(&app), "/x/rocket.tar.zst");
    }

    #[test]
    fn a_7z_with_locked_names_asks_for_its_password() {
        let d = std::env::temp_dir().join(format!("coxswain-test-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plan.txt"), "fuel").unwrap();
        coxswain_core::archive::create_locked(&d.join("made.7z"), &[d.join("plan.txt")], Some("rocket"), true).unwrap();
        // Under another name: the password the app packed it with is kept for that path only.
        let z = d.join("secret.7z");
        std::fs::rename(d.join("made.7z"), &z).unwrap();
        let mut app = app(d.clone(), d.clone());
        app.cd(0, z.clone());
        assert_eq!(app.panels[0].dir, d, "nothing to list without it: back in its folder");
        assert!(matches!(&app.dialog, Some(Dialog::Input { prompt: Prompt::Unlock(0, dir), .. }) if *dir == z), "asked for the password");
        let _ = std::fs::remove_dir_all(d);
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
        std::fs::create_dir(d.join("sub")).unwrap();
        p.load(false);
        p.toggle_mark(1);
        p.mark_all();
        assert_eq!(p.marked.len(), 4, "files and the folder, never `..`");
        assert!(!p.marked.iter().any(|m| m.ends_with("..")));
        std::fs::remove_dir_all(d).unwrap();
    }
}

/// Japanese and Korean letters take two columns: panels, the key bar, Find and a dialog must
/// still line up.
#[cfg(test)]
mod wide_letters {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// The screen as cells: a wide letter's second cell is empty.
    fn draw(app: &mut App) -> Vec<Vec<String>> {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect()
    }

    /// The columns where `c` stands in `row`.
    fn at(row: &[String], c: &str) -> Vec<usize> {
        row.iter().enumerate().filter(|(_, s)| *s == c).map(|(x, _)| x).collect()
    }

    /// The rows from `top` to `bottom` all have `c` in the same columns (and somewhere).
    fn same_columns(screen: &[Vec<String>], c: &str, rows: std::ops::Range<usize>) {
        let want = at(&screen[rows.start], c);
        assert!(!want.is_empty(), "no {c} in row {}", rows.start);
        for y in rows {
            assert_eq!(at(&screen[y], c), want, "row {y}: {}", screen[y].concat());
        }
    }

    #[test]
    fn japanese_and_korean_line_up() {
        // The language is the whole process's: the check runs in a process of its own, so
        // no other test reads Japanese.
        if std::env::var_os("COXSWAIN_WIDE_TEST").is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["wide_letters::", "--test-threads=1"])
                .env("COXSWAIN_WIDE_TEST", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            return;
        }
        let d = std::env::temp_dir().join(format!("coxswain-test-wide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("한국어 폴더")).unwrap();
        for f in ["日本語のとても長いファイルの名前です.txt", "보고서.txt", "a.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        for lang in ["ja", "ko"] {
            coxswain_core::i18n::set_language(lang);
            let cfg = Config { check_updates: false, ..Config::default() };
            let index = Client::with(None, &cfg.search, |_| {});
            let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
            app.panels[0].toggle_mark(1);
            let s = draw(&mut app);
            // Panels: the column bars run straight down, the headings are not cut, the marked
            // line is centred.
            same_columns(&s, "│", 2..18);
            assert!(!s[1].concat().contains('…'), "{lang}: {}", s[1].concat());
            let info = &s[20][1..39];
            let text: Vec<usize> = (0..info.len()).filter(|&x| info[x] != " ").collect();
            let (l, r) = (text[0], info.len() - 1 - text[text.len() - 1]);
            assert!(l.abs_diff(r) <= 2, "{lang}: not centred: {}", info.concat());
            // The key bar: each key number in its own slot.
            let bar = &s[23];
            for n in 1..=9 {
                assert_eq!(bar[(n - 1) * 8], n.to_string(), "{lang}: {}", bar.concat());
            }
            // Find, with a Japanese query, and a dialog with a long Korean text.
            app.dialog = Some(Dialog::Search { query: "日本語 보고서".into(), chip: Kind::All, here: false, found: Found::default(), cursor: 0, offset: 0, show: Show::List });
            let s = draw(&mut app);
            same_columns(&s, "║", 3..20);
            app.dialog = Some(Dialog::Confirm { title: t!("dialog.delete"), text: "한국어 폴더와 日本語のファイルを完全に削除しますか".repeat(2), paths: vec![], forever: false });
            let s = draw(&mut app);
            // Its frame: from the corner that is not the panels' (row 0) to the bottom one.
            let top = (1..24).find(|&y| s[y].iter().any(|c| c == "╔")).unwrap();
            let bottom = (top..24).find(|&y| s[y].iter().any(|c| c == "╚")).unwrap();
            same_columns(&s, "║", top + 1..bottom);
            // Settings: the areas' bar and the frame run straight down, in every area.
            app.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(&app, "search"))));
            for _ in 0..settings::AREAS.len() {
                let s = draw(&mut app);
                same_columns(&s, "║", 1..23);
                same_columns(&s, "│", 1..19);
                app.dialog_key(Key::new(KeyCode::Right, false, false, false));
            }
        }
        coxswain_core::i18n::set_language("en-GB");
        // On Windows a git the apps started in the folder (the status, on its thread) may still
        // be running there for a moment, and a process's folder cannot be removed.
        for _ in 0..50 {
            if std::fs::remove_dir_all(&d).is_ok() || !d.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

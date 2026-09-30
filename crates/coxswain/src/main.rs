//! Coxswain TUI: two panels, a command line and a function-key bar, Norton Commander style.

mod ui;

use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Config, Glyphs, Key, KeyCode};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::git;
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
    pub sort: SortKey,
    pub reverse: bool,
    pub git: Option<git::Status>,
    pub error: Option<String>,
    /// Folder sizes, measured in the background.
    pub sizes: HashMap<PathBuf, u64>,
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
            sort: SortKey::Name,
            reverse: false,
            git: None,
            error: None,
            sizes: HashMap::new(),
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
        bfs::sort(&mut self.entries, self.sort, self.reverse);
        let names: HashSet<&Path> = self.entries.iter().map(|e| e.path.as_path()).collect();
        self.marked.retain(|p| names.contains(p.as_path()));
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        if let Some(n) = keep {
            self.select_name(&n);
        }
    }

    fn cd(&mut self, dir: PathBuf, show_hidden: bool) {
        let from = std::mem::replace(&mut self.dir, dir);
        self.marked.clear();
        self.sizes.clear();
        self.cursor = 0;
        self.offset = 0;
        self.git = None;
        self.load(show_hidden);
        // Coming up out of a directory: put the cursor on it, as NC does.
        if from.parent() == Some(self.dir.as_path()) {
            if let Some(n) = from.file_name() {
                self.select_name(&n.to_string_lossy());
            }
        }
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
    Mkdir,
    Goto(usize),
    Select(bool),
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
    /// `mode`: 0 names everywhere, 1 names in this folder, 2 the text of files.
    Search { query: String, mode: u8, results: Results, cursor: usize, offset: usize },
    /// `direct`: a typed key runs the item with that key (F2); otherwise it filters (F9).
    Menu { title: String, filter: String, items: Vec<MenuItem>, cursor: usize, direct: bool },
    Help { scroll: u16 },
    Message { title: String, text: String },
}

/// Work that needs the real terminal, done by the main loop.
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
    git_tx: mpsc::Sender<(PathBuf, Option<git::Status>)>,
    git_rx: mpsc::Receiver<(PathBuf, Option<git::Status>)>,
    update_rx: mpsc::Receiver<String>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// Folder sizes arrive here: the panel's folder, the folder measured, its bytes.
    sizes_tx: mpsc::Sender<(PathBuf, PathBuf, u64)>,
    sizes_rx: mpsc::Receiver<(PathBuf, PathBuf, u64)>,
    /// The stop flag of each panel's measuring.
    measuring: [Arc<std::sync::atomic::AtomicBool>; 2],
    run: Option<Run>,
    last_click: Option<(Instant, u16, u16)>,
    quit: bool,
}

fn resolve(base: &Path, s: &str) -> PathBuf {
    let s = s.trim();
    let p = match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            let home = std::env::home_dir().unwrap_or_default();
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(s),
    };
    if p.is_absolute() { p } else { base.join(p) }
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
        let keymap = cfg.keymap()?;
        let (git_tx, git_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let (sizes_tx, sizes_rx) = mpsc::channel();
        if cfg.check_updates {
            std::thread::spawn(move || {
                if let Some(v) = coxswain_core::update::check() {
                    let _ = update_tx.send(v);
                }
            });
        }
        let index = Client::start(&cfg.search);
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
            measuring: Default::default(),
            run: None,
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

    /// git status runs on a thread; results arrive through `git_rx`.
    fn refresh_git(&self) {
        let dirs: HashSet<PathBuf> = self.panels.iter().map(|p| p.dir.clone()).collect();
        for dir in dirs {
            let tx = self.git_tx.clone();
            std::thread::spawn(move || {
                let st = git::Status::read(&dir);
                let _ = tx.send((dir, st));
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
        if !self.cfg.folder_sizes {
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
        let h = self.show_hidden;
        self.panels.iter_mut().for_each(|p| p.load(h));
        self.refresh_git();
        self.measure(0);
        self.measure(1);
    }

    fn cd(&mut self, side: usize, dir: PathBuf) {
        let h = self.show_hidden;
        self.panels[side].cd(dir, h);
        self.refresh_git();
        self.measure(side);
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
        if !self.cmdline.is_empty() {
            match (key.code, plain_char) {
                (KeyCode::Enter, _) => return self.run_cmdline(),
                (KeyCode::Backspace, _) => {
                    self.cmdline.pop();
                    return;
                }
                (KeyCode::Esc, _) => return self.cmdline.clear(),
                (_, Some(c)) => return self.cmdline.push(c),
                _ => {}
            }
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
        let h = self.show_hidden;
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
            Action::Parent => {
                if let Some(p) = self.panel().dir.parent().map(Path::to_path_buf) {
                    self.cd(self.active, p);
                }
            }
            Action::Mark => {
                let p = self.panel_mut();
                if let Some(e) = p.current().filter(|e| !e.is_parent()).map(|e| e.path.clone()) {
                    if !p.marked.remove(&e) {
                        p.marked.insert(e);
                    }
                }
                p.move_cursor(1);
            }
            Action::SelectGroup | Action::UnselectGroup => {
                let sel = a == Action::SelectGroup;
                let title = if sel { t!("tui.select") } else { t!("tui.unselect") };
                self.input(&title, t!("tui.files_matching"), "*".into(), Prompt::Select(sel));
            }
            Action::InvertSelection => {
                let p = self.panel_mut();
                for e in p.entries.iter().filter(|e| !e.is_dir) {
                    if !p.marked.remove(&e.path) {
                        p.marked.insert(e.path.clone());
                    }
                }
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
                p.load(h);
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
            Action::Mkdir => self.input(&t!("dialog.new_folder"), t!("tui.mkdir_label"), String::new(), Prompt::Mkdir),
            Action::Delete | Action::DeleteForever => {
                let paths = self.panel().targets();
                if paths.is_empty() {
                    return;
                }
                let forever = a == Action::DeleteForever;
                if self.cfg.confirm_delete {
                    let text = t!(if forever { "confirm.delete_forever" } else { "confirm.trash" }, "what" => Self::describe(&paths));
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

    fn open(&mut self) {
        let Some(e) = self.panel().current().cloned() else { return };
        if e.is_dir {
            return self.cd(self.active, e.path);
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
        self.changed(&paths);
        let op = if forever { bfs::delete } else { bfs::trash };
        let errors: Vec<String> = paths.iter().filter_map(|p| op(p).err().map(|e| format!("{}: {e}", p.display()))).collect();
        let verb = if forever { "status.deleted" } else { "status.trashed" };
        self.after_op(t!(verb, "what" => Self::describe(&paths)), errors);
    }

    fn after_op(&mut self, ok: String, errors: Vec<String>) {
        self.panels.iter_mut().for_each(|p| p.marked.clear());
        self.reload();
        if errors.is_empty() {
            self.status = Some(ok);
        } else {
            self.dialog = Some(Dialog::Message { title: t!("dialog.error"), text: errors.join("\n") });
        }
    }

    fn submit(&mut self, prompt: Prompt, value: String) {
        let base = self.panel().dir.clone();
        match prompt {
            Prompt::Copy(src) | Prompt::Move(src) if value.trim().is_empty() => drop(src),
            Prompt::Copy(src) => {
                let dst = resolve(&base, &value);
                self.changed(&[dst.join("new")]);
                let errors = src.iter().filter_map(|p| bfs::copy(p, &dst).err().map(|e| format!("{}: {e}", p.display()))).collect();
                self.after_op(t!("status.copied", "what" => Self::describe(&src)), errors);
            }
            Prompt::Move(src) => {
                let dst = resolve(&base, &value);
                self.changed(&src);
                self.changed(&[dst.join("new")]);
                let errors = src.iter().filter_map(|p| bfs::rename(p, &dst).err().map(|e| format!("{}: {e}", p.display()))).collect();
                self.after_op(t!("status.moved", "what" => Self::describe(&src)), errors);
                if src.len() == 1 && dst.parent() == Some(base.as_path()) {
                    let n = dst.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    self.panel_mut().select_name(&n);
                }
            }
            Prompt::Mkdir if value.trim().is_empty() => {}
            Prompt::Mkdir => {
                let d = resolve(&base, &value);
                let errors = bfs::mkdir(&d).err().map(|e| vec![format!("{}: {e}", d.display())]).unwrap_or_default();
                self.after_op(t!("status.created", "what" => d.display()), errors);
                if let Some(first) = d.strip_prefix(&base).ok().and_then(|r| r.components().next()) {
                    self.panel_mut().select_name(&first.as_os_str().to_string_lossy());
                }
            }
            Prompt::Goto(side) => {
                let d = resolve(&base, &value);
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
            }
        }
    }

    fn search_now(&mut self) {
        let max = self.cfg.search.max_results;
        let dir = self.panel().dir.clone();
        if let Some(Dialog::Search { query, mode, results, cursor, offset }) = &mut self.dialog {
            *results = match *mode {
                2 => self.index.search_text(query, max),
                _ => self.index.search(query, (*mode == 1).then_some(dir.as_path()), max),
            };
            *cursor = 0;
            *offset = 0;
        }
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
            Dialog::Search { mut query, mut mode, results, mut cursor, offset } => {
                let hit = results.hits.get(cursor).cloned();
                let page = 10isize;
                let mut requery = false;
                match (key.code, ch) {
                    _ if esc => return,
                    (KeyCode::Enter, _) => {
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
                        if let Some(h) = hit.filter(|h| !h.is_dir) {
                            self.view_or_edit(action.unwrap(), &h.path);
                        }
                    }
                    (KeyCode::Tab, _) => {
                        mode = (mode + 1) % 3;
                        requery = true;
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
                let p = &mut self.panels[side];
                if let Some(e) = p.entries.get(i).filter(|e| !e.is_parent()).map(|e| e.path.clone()) {
                    if !p.marked.remove(&e) {
                        p.marked.insert(e);
                    }
                }
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
        while let Ok((dir, st)) = self.git_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                p.git = st.clone();
            }
        }
        let state = self.index.state();
        if state != *last_state {
            *last_state = state;
            if matches!(self.dialog, Some(Dialog::Search { .. })) {
                self.search_now();
            }
        }
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
    if let Err(e) = coxswain_core::tools::command(&sh).arg(flag).arg(cmd).current_dir(dir).status() {
        println!("coxswain: {sh}: {e}");
    }
    if wait {
        print!("\n{}", t!("tui.press_enter"));
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn main_loop(term: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut state = app.index.state();
    while !app.quit {
        term.draw(|f| ui::draw(f, app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(k) => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
        }
        app.tick(&mut state);
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
    }
    Ok(())
}

const USAGE: &str = "coxswain [LEFT] [RIGHT]      a folder, or a file to open its folder with the cursor on it
  --dump-config   print the full default config (redirect it to the config file to customise)
  --config-path   print where the config file is read from
  --index-service on|off   start the search helper with your session, or stop doing so
  --meaning on|off|delete  search by meaning: download the model and turn it on, turn it off,
                           or turn it off and delete the model
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
fn meaning(what: Option<&str>) {
    use coxswain_core::meaning;
    let fail = |e: String| -> ! {
        eprintln!("coxswain: {e}");
        std::process::exit(1)
    };
    match what {
        Some("on") => {
            if !meaning::installed() {
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
        _ => return println!("{}", if Config::load().is_ok_and(|c| c.search.meaning) && meaning::installed() { "on" } else { "off" }),
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
        Some("--index-service") => return index_service(args.get(1).map(String::as_str)),
        Some("--meaning") => return meaning(args.get(1).map(String::as_str)),
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
}

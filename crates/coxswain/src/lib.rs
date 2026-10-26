//! Coxswain TUI: two panels, a command line and a function-key bar, Norton Commander style.

mod attrs;
mod sync;
mod bom;
mod chess;
mod pieces;
mod go;
mod ebook;
mod pictures;
mod git_config;
mod provenance;
mod replace;
mod guide;
mod settings;
mod setup;
mod space;
mod ui;
mod blame;
mod viewer;
mod editor;
mod split_escape;
mod compare;
mod viruses;

use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Config, Glyphs, Key, KeyCode};
use coxswain_core::fs::{self as bfs, resolve, Entry, SortKey};
use coxswain_core::git;
use coxswain_core::history::{self, Lasts};
use coxswain_core::helper::{self, Client};
use coxswain_core::find::{self, Found, Kind, Off, Row};
use coxswain_core::index::{self, State};
use coxswain_core::tools::Line;
use coxswain_core::undo::Record;
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
    /// Since when sizes have arrived that a sort by size has not taken in yet: sorted again
    /// a moment later, not at every folder measured.
    resort_due: Option<Instant>,
    /// The last commit of each entry, when git has answered.
    pub last: Option<Arc<Lasts>>,
    /// The ZFS dataset the folder is on (or whose snapshots it lists), when `zfs` has answered.
    pub zfs: Option<coxswain_core::zfs::Facts>,
    /// The package whose files the folder lists.
    pub package: Option<String>,
    /// The git facts the info line shows (`facts_dir`), one line, as they were read for that
    /// folder: `None` while they are read or when it is no repository.
    pub facts: Option<(PathBuf, Option<String>)>,
    /// The folder `facts_dir` named at the last tick: its facts are read once the cursor has
    /// rested there a tick, so holding a key over many repositories runs no git for each.
    facts_seen: Option<PathBuf>,
    /// What the title's badge needs from the disk (`ui::Place`), read once for the folder it
    /// names, and where the symlink under the cursor points, read once for it: drawing looks
    /// at no disk, so a share that stops answering does not freeze the screen.
    pub place: Option<(PathBuf, ui::Place)>,
    pub link: Option<(PathBuf, Option<PathBuf>)>,
    /// The models a store's folder names its entries by (`local_models::names`), read once
    /// for the folder it names.
    pub models: Option<(PathBuf, Option<coxswain_core::local_models::Names>)>,
    /// Compare folders (Ctrl+F2): the compare, this panel's side in it, and whether only the
    /// differences are listed.
    pub cmp: Option<Cmp>,
}

#[derive(Clone)]
pub struct Cmp {
    pub f: Arc<coxswain_core::compare::folders::Folders>,
    pub side: usize,
    pub diff_only: bool,
}

impl Panel {
    /// A panel listed at once, the cursor on the file `dir` names when it is one.
    #[cfg(test)]
    fn new(dir: PathBuf, show_hidden: bool) -> Panel {
        let (mut p, file) = Panel::unlisted(dir);
        p.load(show_hidden);
        if let Some(name) = file {
            p.select_name(&name);
        }
        p
    }

    /// A panel not listed yet (`App::load` lists it), and the name to put the cursor on: a
    /// file opens its folder with the cursor on it.
    fn unlisted(dir: PathBuf) -> (Panel, Option<String>) {
        let file = dir.is_file().then(|| dir.file_name().map(|n| n.to_string_lossy().into_owned())).flatten();
        let dir = if file.is_some() { dir.parent().map_or(dir.clone(), Path::to_path_buf) } else { dir };
        let p = Panel {
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
            resort_due: None,
            last: None,
            zfs: None,
            package: None,
            facts: None,
            facts_seen: None,
            place: None,
            link: None,
            models: None,
            cmp: None,
        };
        (p, file)
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
                    (self.entries, self.link) = (v, None);
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
        // Differences only: what is the same on both sides is left out.
        if let Some(c) = self.cmp.as_ref().filter(|c| c.diff_only) {
            use coxswain_core::compare::folders::State;
            self.entries.retain(|e| e.is_parent() || c.f.state(c.side, &e.path) != Some(State::Same));
        }
        bfs::sort_in(&self.dir, &mut self.entries, self.sort, self.reverse, &self.sizes);
        // Inside an archive a folder's size comes with the listing; there is nothing to measure.
        // A snapshot's is its own space.
        if coxswain_core::archive::split(&self.dir).is_some() || coxswain_core::zfs::split(&self.dir).is_some() {
            self.sizes.extend(self.entries.iter().filter(|e| e.is_dir && !e.is_parent()).map(|e| (e.path.clone(), e.size)));
        }
        if !self.marked.is_empty() {
            let names: HashSet<&Path> = self.entries.iter().map(|e| e.path.as_path()).collect();
            self.marked.retain(|p| names.contains(p.as_path()));
        }
        self.recount_marks();
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        if let Some(n) = keep {
            self.select_name(&n);
        }
    }

    /// Sort the entries as they are, keeping the cursor on the same name: no reread, which
    /// on a big folder takes longer than the sort.
    fn resort(&mut self) {
        self.resort_due = None;
        let keep = self.current().map(|e| e.name.clone());
        bfs::sort_in(&self.dir, &mut self.entries, self.sort, self.reverse, &self.sizes);
        if let Some(n) = keep {
            self.select_name(&n);
        }
    }

    /// Mark the entry at `i`, or unmark it. `..` is never marked.
    fn toggle_mark(&mut self, i: usize) {
        let Some(e) = self.entries.get(i).filter(|e| !e.is_parent()) else { return };
        let bytes = self.bytes(e);
        if self.marked.remove(&e.path) {
            self.marked_bytes -= bytes;
        } else {
            self.marked.insert(e.path.clone());
            self.marked_bytes += bytes;
        }
    }

    /// What an entry adds to the marked total: a folder its measured size (none until then),
    /// not its own entry's.
    fn bytes(&self, e: &Entry) -> u64 {
        if e.is_dir { self.sizes.get(&e.path).copied().unwrap_or(0) } else { e.size }
    }

    /// A folder measured: the marked total takes it in, and a sort by size soon after.
    fn sized(&mut self, folder: PathBuf, bytes: u64) {
        let old = self.sizes.insert(folder.clone(), bytes).unwrap_or(0);
        if self.marked.contains(&folder) {
            self.marked_bytes = self.marked_bytes - old + bytes;
        }
        if self.sort == SortKey::Size {
            self.resort_due.get_or_insert_with(Instant::now);
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
        self.marked_bytes = self.entries.iter().filter(|e| self.marked.contains(&e.path)).map(|e| self.bytes(e)).sum();
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
        self.facts = None;
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

    /// The repository whose git facts the info line shows for the entry under the cursor: on
    /// `..` the one the panel is in, on a folder the folder, when it is a repository's own.
    pub fn facts_dir(&self) -> Option<PathBuf> {
        let e = self.current()?;
        if e.is_parent() {
            self.git.as_ref().map(|g| g.root.clone())
        } else {
            (e.is_dir && !e.online && !virtual_list(&self.dir)).then(|| e.path.clone())
        }
    }

    /// What the action menu and the hints are about: the marked entries (or the one under the
    /// cursor), and where they are.
    fn subject(&self) -> coxswain_core::menu::Subject {
        use coxswain_core::menu::{Place, Subject};
        let cur = self.current().filter(|e| !e.is_parent());
        let list: Vec<&Entry> = if self.marked.is_empty() { cur.into_iter().collect() } else { self.entries.iter().filter(|e| self.marked.contains(&e.path)).collect() };
        let at = history::split(&self.dir);
        let inside = coxswain_core::archive::split(&self.dir);
        let place = if inside.as_ref().is_some_and(|(a, i)| coxswain_core::archive::read_only(a) || coxswain_core::archive::nested(a, i, true)) {
            Place::DiskImage
        } else if inside.is_some() {
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
            one: list.len() == 1 && cur.is_some_and(|c| c.path == list[0].path),
            marked: self.marked.len(),
            place,
            git: self.git.is_some(),
            zfs: self.zfs.is_some(),
            packages: coxswain_core::bsd::packages_here(&self.dir),
            big: place == Place::Disk && match cur.filter(|c| c.is_dir && list.len() == 1) {
                Some(c) => coxswain_core::space::worth_it(&c.path, self.sizes.get(&c.path).copied()),
                None => coxswain_core::space::worth_it(&self.dir, None),
            },
            whisper: false,
            // Only one file's bytes are read: the action menu offers Open games on a single file.
            ..Subject::of(&list)
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
    /// A clip's start and length, for *Convert video or audio* on these files.
    Clip(Vec<PathBuf>),
    /// The longest side in pixels, for *Convert or edit pictures* on these files.
    Resize(Vec<PathBuf>),
    /// The size of the parts to split this file into (`split`).
    SplitSize(PathBuf),
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
            Prompt::Flags(_) | Prompt::Clip(_) | Prompt::Resize(_) | Prompt::SplitSize(_) => "common.apply",
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
    User(config::UserCommand),
    /// A file of the scripts folder, run on the marked files.
    Script(PathBuf),
    /// The scripts folder, made when missing, in the active panel.
    ScriptsFolder,
    /// Open config.toml at `[[user_menu]]`.
    AddUser,
    /// A panel to a folder (a boot environment, a jail); none: the line to type a path in.
    Goto(usize, Option<PathBuf>),
    /// Cannot be gone to: why, on the status line.
    Say(String),
    /// What *Convert video or audio* or *Convert or edit pictures* makes of these files.
    Media(coxswain_core::media::Op, Vec<PathBuf>),
    /// *Convert e-book*: these books into this format (`coxswain_core::ebook::TO`).
    Ebook(&'static str, Vec<PathBuf>),
    /// Make checksums of these with this algorithm (`checksum`).
    Sums(coxswain_core::checksum::Algo, Vec<PathBuf>),
    /// Files dropped on the terminal: copied (or with `true` moved) into the active panel.
    Drop(Vec<PathBuf>, bool),
    /// Split this file into parts of this size; with none, ask for the size.
    Split(PathBuf, Option<u64>),
    /// Join the parts from this first one; `true`: the joined file replaces the one there.
    Join(PathBuf, bool),
    /// What *Export draw.io diagram* makes of these diagrams.
    Export(coxswain_core::export::Op, Vec<PathBuf>),
    /// *Preview with…*: this choice for this kind (`[preview.tools]`; "" for the default).
    PreviewTool(&'static str, &'static str),
    /// A line copied to the clipboard, said on the status line.
    CopyLine(String),
    /// Settings written (*Long files* for a folder), the helper restarted for them.
    Save(serde_json::Map<String, serde_json::Value>, String),
    /// Settings, open at a section.
    SettingsAt(&'static str),
    /// Compare folders' menu (Ctrl+F2 while one is on).
    Folders(FoldersRun),
}

#[derive(Clone, Copy)]
pub enum FoldersRun {
    Mark,
    DiffOnly,
    Again(coxswain_core::compare::folders::Mode),
    Stop,
    /// Make the side given (0 left, 1 right) like the other.
    Make(usize),
}

pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub run: MenuRun,
    /// The heading it is listed under, if the menu has headings.
    pub group: Option<String>,
}

/// The menu's items whose label holds the filter, as indexes: the keys and the drawing agree.
pub(crate) fn menu_visible(items: &[MenuItem], filter: &str) -> Vec<usize> {
    let f = filter.to_lowercase();
    (0..items.len()).filter(|&i| items[i].label.to_lowercase().contains(&f)).collect()
}

pub enum Dialog {
    Input { title: String, label: String, value: String, prompt: Prompt },
    /// Delete `paths`; `forever` skips the trash.
    Confirm { title: String, text: String, paths: Vec<PathBuf>, forever: bool },
    /// Switch to the branch `entry` of the list of branches `dir`.
    Switch { title: String, text: String, dir: PathBuf, entry: String },
    /// Read the history at `to` from the Subversion or CVS server `server`? Asked once per server.
    Server { title: String, text: String, server: String, to: PathBuf, feature: &'static str },
    /// Find: the query, the kind chip, whether it is limited to the panel's folder (`here`),
    /// what was found, the cursor (on a row of `find::rows`, or on a source of the answer), the
    /// first row in sight, and what shows. Ask's questions and answers are in `App::chat`.
    Search { query: String, chip: Kind, here: bool, found: Found, cursor: usize, offset: usize, show: Show },
    /// `direct`: a typed key runs the item with that key (F2); otherwise it filters (F9).
    /// `feature`: what F1 in it shows (`coxswain_core::features`).
    Menu { title: String, filter: String, items: Vec<MenuItem>, cursor: usize, direct: bool, feature: &'static str },
    Help { scroll: u16 },
    /// F1 → Features (Tab in Help, F1 in a dialog): the filter, the features it keeps, the one
    /// under the cursor, what Enter (Show me) said, and the dialog Esc goes back to.
    Features { filter: String, list: Vec<coxswain_core::features::Entry>, cursor: usize, said: Option<String>, back: Option<Box<Dialog>> },
    Message { title: String, text: String },
    /// Mistakes in config.toml, shown at start (`coxswain_core::notices::refused`): Enter opens
    /// the file at `line`, the first one's.
    Refused { text: String, line: usize },
    /// Make `pdfs` searchable? `text` says what it writes, which program runs and what a first
    /// run pulls.
    Searchable { title: String, text: String, pdfs: Vec<PathBuf> },
    /// Make `op` of `files`? `text` says what it writes, which program runs and what a first
    /// run pulls.
    Media { title: String, text: String, op: coxswain_core::media::Op, files: Vec<PathBuf> },
    /// Transcribe `files`? `text` says how much sound, how long it takes here, and what it
    /// fetches first.
    Transcribe { title: String, text: String, files: Vec<PathBuf> },
    /// Make `op` of `files`? `text` says what it writes, which program runs and what a first
    /// run pulls.
    Export { title: String, text: String, op: coxswain_core::export::Op, files: Vec<PathBuf> },
    /// Scan `targets` for viruses? `text` says what it reads, the database and its update,
    /// and what a first run pulls.
    VirusScan { title: String, text: String, targets: Vec<PathBuf> },
    /// What a virus scan found: each file with what to do with it.
    Viruses(Box<viruses::Found>),
    /// A CycloneDX BOM, full screen (F3 on one).
    Bom(Box<bom::Viewer>),
    Provenance(Box<provenance::Viewer>),
    /// The git config, full screen (Alt+C).
    GitConfig(Box<git_config::Viewer>),
    /// Convert `books` into `to`, or (`None`) pull calibre's image to view the book? `text` says
    /// what it writes, which calibre runs and what a first run pulls.
    Ebook { title: String, text: String, to: Option<&'static str>, books: Vec<PathBuf> },
    /// Find and replace across files (`replace`).
    Replace(Box<replace::Viewer>),
    /// Permissions and dates in bulk (`attrs`).
    Attrs(Box<attrs::Viewer>),
    /// Make one side like the other (`compare::sync`).
    Sync(Box<sync::Viewer>),
    /// A file in the built-in viewer, full screen (F3); `back`: the dialog it was opened from
    /// (a BOM's source), shown again when it closes.
    View { v: Box<viewer::Viewer>, back: Option<Box<Dialog>> },
    /// A file in the built-in editor, full screen (Shift+F4; F4 with no editor set); `back` as
    /// for `View`.
    Edit { e: Box<editor::Editor>, back: Option<Box<Dialog>> },
    /// Two files compared, full screen (Shift+F2).
    Compare(Box<compare::Viewer>),
    /// A PGN file's games, an EPD or FEN file's positions, full screen (F3, Alt+K).
    Chess(Box<chess::Viewer>),
    /// An SGF file's Go games, full screen (F3, Alt+K).
    Go(Box<go::Viewer>),
    /// Where the space goes, full screen (Alt+U).
    Space(Box<space::Viewer>),
    /// Settings, full screen (F12, F9 → Settings, `--settings`).
    Settings(Box<settings::Settings>),
    /// The first-run guide, full screen (the first start, F1 → G, Settings → Overview).
    Guide(Box<guide::Guide>),
}

/// The feature a dialog is about (`coxswain_core::features`), for F1 in it.
fn feature_of(d: &Dialog) -> Option<&'static str> {
    Some(match d {
        Dialog::Input { prompt, .. } => match prompt {
            Prompt::Copy(_) => "copy",
            Prompt::Move(_) | Prompt::Rename(_) => "move_and_rename",
            Prompt::Extract(_) | Prompt::Pack(_) | Prompt::PackPassword(..) | Prompt::PackConfirm(..) => "pack_and_extract",
            Prompt::Password(..) | Prompt::Unlock(..) | Prompt::Peek(_) => "archive_passwords",
            Prompt::Mkdir => "new_folder",
            Prompt::NewBranch(..) => "git_branches",
            Prompt::Goto(_) => "moving",
            Prompt::Mark(_) => "marking",
            Prompt::Flags(_) => "properties",
            Prompt::Clip(_) => "convert_media",
            Prompt::Resize(_) => "edit_pictures",
            Prompt::SplitSize(_) => "split_and_join",
        },
        Dialog::Confirm { .. } => "delete",
        Dialog::Searchable { .. } => "searchable_pdf",
        Dialog::Media { op, .. } => media_feature(op.program()),
        Dialog::Ebook { .. } => "ebooks",
        Dialog::Transcribe { .. } => "transcripts",
        Dialog::Export { op, .. } => export_feature(*op),
        Dialog::VirusScan { .. } | Dialog::Viruses(_) => "virus_scan",
        Dialog::Switch { .. } => "git_branches",
        Dialog::Server { feature, .. } => feature,
        Dialog::Menu { feature, .. } => feature,
        Dialog::Search { .. } => "find_file",
        Dialog::Bom(_) => "cbom",
        Dialog::Provenance(_) => "provenance",
        Dialog::GitConfig(_) => "git_config",
        Dialog::Replace(_) => "find_replace",
        Dialog::Attrs(_) => "permissions_and_dates",
        Dialog::Sync(_) => "compare_folders",
        Dialog::View { .. } => "view_and_edit",
        Dialog::Edit { .. } => "editor",
        Dialog::Compare(_) => "compare_files",
        Dialog::Chess(v) => v.feature(),
        Dialog::Go(_) => "go",
        Dialog::Space(_) => "space",
        Dialog::Settings(_) => "settings_window",
        Dialog::Guide(_) => "first_run",
        Dialog::Refused { .. } => "configuration",
        Dialog::Help { .. } | Dialog::Features { .. } | Dialog::Message { .. } => return None,
    })
}

/// `editor`'s program when it is plainly not there: a bare name not on the PATH, or a path to
/// nothing. A command with `$` or `~` is the shell's to read, and left to it.
fn editor_missing(editor: &str) -> Option<String> {
    let prog = editor.split_whitespace().next()?.to_string();
    let gone = if prog.contains(['$', '~', '"', '\'']) {
        false
    } else if prog.contains(std::path::MAIN_SEPARATOR) || prog.contains('/') {
        !Path::new(&prog).exists()
    } else {
        coxswain_core::tools::which(&prog).is_none()
    };
    gone.then_some(prog)
}

/// The pager the built-in viewer hands a file to: `$PAGER`, else less (more on Windows).
fn pager() -> String {
    std::env::var("PAGER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| if cfg!(windows) { "more".into() } else { "less".into() })
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
    /// For each source, how many near copies of it were left out ("and 3 similar files").
    pub similar: Vec<usize>,
    /// How much was read: "12 excerpts from 7 files, about 6000 words".
    pub read: String,
    pub answer: String,
    pub error: Option<String>,
}

/// What an Ask sends back while it runs.
enum AskMsg {
    Sources(Vec<PathBuf>, Vec<usize>, String),
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
    /// The git facts of a repository's folder (`Panel::facts_dir`), one line.
    Facts(PathBuf, Option<String>),
}

/// Work done by the main loop after a frame: what needs the real terminal, and what takes a
/// while, so the status line says so first.
enum Run {
    /// A line Coxswain built, by sh; `typed`: the command line's own, by the user's `$SHELL`.
    Shell { line: Line, typed: bool, dir: PathBuf, wait: bool },
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
    /// What Enter does on the entry under the cursor (`enter::label`), for the entry it was
    /// worked out for: the command line says it while it is empty.
    pub enter_on: Option<(PathBuf, Option<String>)>,
    /// What is under the cursor may have changed (a key, a click, a listing, git): the hint
    /// is worked out again only then.
    hint_stale: bool,
    /// Find shows how to limit it to the panel's folder: the first few times it opens.
    pub find_hint: bool,
    pub quick: Option<String>,
    pub show_hidden: bool,
    pub index: Arc<Client>,
    /// Panel rectangles from the last draw, for mouse hits.
    pub areas: [Rect; 2],
    git_tx: mpsc::Sender<(PathBuf, Git)>,
    git_rx: mpsc::Receiver<(PathBuf, Git)>,
    /// Folders with a git status read on its way; `true` when it must be read once more as
    /// it ends, because something changed while it ran.
    git_busy: Arc<std::sync::Mutex<HashMap<PathBuf, bool>>>,
    update_rx: mpsc::Receiver<String>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// Folder sizes arrive here: the panel's folder, the folder measured, its bytes.
    sizes_tx: mpsc::Sender<(PathBuf, PathBuf, u64)>,
    sizes_rx: mpsc::Receiver<(PathBuf, PathBuf, u64)>,
    /// Searches to run, and their answers by generation.
    search_tx: mpsc::Sender<FindJob>,
    search_rx: mpsc::Receiver<(u64, Found)>,
    search_gen: u64,
    /// The last search whose answer came: behind `search_gen` while one is out.
    search_got: u64,
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
    /// Find's *Ask is slow* row (`setup::poor`) and when it was worked out: again a second
    /// later while Find is open (the machine probe ends in the background), and at once after
    /// a change to the settings.
    slow: (Option<Instant>, Option<coxswain_core::setup::Poor>),
    /// When `tell` last looked.
    told: Instant,
    /// The stop flag of each panel's measuring.
    measuring: [Arc<std::sync::atomic::AtomicBool>; 2],
    run: Option<Run>,
    /// The copy, move, delete, extract or pack running on its thread; one at a time.
    job: Option<Job>,
    /// *Make PDF searchable* running on its thread; one at a time.
    searchable: Option<Searchable>,
    /// Its question being worked out on a thread (podman is asked whether it has the image):
    /// the PDFs, and the dialog's text or why it cannot run.
    searchable_plan: Option<mpsc::Receiver<SearchablePlan>>,
    /// *Convert video or audio* or *Convert or edit pictures* running on its thread, one at a
    /// time, and the action that started it (which stops it); the question being worked out.
    media: Option<(Searchable, Action)>,
    media_plan: Option<mpsc::Receiver<MediaPlan>>,
    /// calibre on its thread (`ebook`), one run at a time: converting books into a format, or
    /// (`None`) making one's text for the viewer; and its question being worked out.
    ebook: Option<(Searchable, Option<&'static str>, Vec<PathBuf>)>,
    ebook_plan: Option<mpsc::Receiver<ebook::EbookPlan>>,
    /// Checksums being made or checked, on their thread.
    work: Option<Work>,
    /// *Transcribe speech*'s question being worked out on a thread (the lengths of the
    /// recordings are read): the files and the dialog's text.
    transcribe_plan: Option<mpsc::Receiver<(Vec<PathBuf>, String)>>,
    /// *Build PDF of AsciiDoc* or *Export draw.io diagram* running on its thread, one at a
    /// time; the question being worked out.
    export: Option<Exporting>,
    export_plan: Option<mpsc::Receiver<ExportPlan>>,
    /// *Scan for viruses* running on its thread, one at a time; and its question being worked
    /// out (the files are counted, podman is asked for the image).
    viruses: Option<viruses::Run>,
    viruses_plan: Option<mpsc::Receiver<viruses::Plan>>,
    /// What Ctrl+Z undoes, the newest last.
    undo: coxswain_core::undo::History,
    /// Folders listed on a thread: a history's commits (git can take seconds), and a folder
    /// not read within `list_wait` (a million files take seconds too). The folder, the name
    /// to put the cursor on, the listing.
    list_tx: mpsc::Sender<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    list_rx: mpsc::Receiver<(PathBuf, Option<String>, std::io::Result<Vec<Entry>>)>,
    /// How long a key waits for a folder to be listed before the panel shows it as being read
    /// and the keys go on.
    list_wait: Duration,
    /// Tests hold the listing threads with it (a write lock) until they have looked at the
    /// panel meanwhile, so a small folder cannot be listed before they look.
    #[cfg(test)]
    list_hold: Option<Arc<std::sync::RwLock<()>>>,
    last_click: Option<(Instant, u16, u16)>,
    /// Properties being read on a thread.
    props_rx: Option<mpsc::Receiver<Result<bfs::Props, String>>>,
    /// The panels' folders and their repositories' `.git`, watched: what changes on disk is
    /// read again.
    watch: coxswain_core::fs::Watch,
    /// Space, put aside while View, Edit or Delete has a file of it: back when that is done,
    /// without the file if it went.
    space_back: Option<(Box<space::Viewer>, PathBuf)>,
    /// What a background job wanted to show while the editor was open: shown once it closes,
    /// so the editor and its unsaved text are never put away by it.
    waiting: Vec<Dialog>,
    /// Compare folders' summary was put on the status line once it was done.
    folders_said: bool,
    /// What a run that made one side like the other did: kept in front of the compare's
    /// summary when it is compared again after it.
    folders_lead: Option<String>,
    quit: bool,
    /// Under the pack, copy or move dialog's field: that the disk may be short of room to write
    /// into an archive (`archive::short_of_room`). Cleared by each new input dialog.
    input_note: Option<String>,
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
    /// Esc on the progress line sets it (`undo::run`); an undo has none.
    stop: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Job {
    /// The progress line: `line`, then that Esc stops it, or that it is stopping.
    fn shown(&self) -> String {
        match &self.stop {
            Some(s) if s.load(std::sync::atomic::Ordering::Relaxed) => format!("{} · {}", self.line, t!("searchable.stopping")),
            Some(_) => format!("{} · {}", self.line, t!("searchable.stop_key", "key" => "Esc")),
            None => self.line.clone(),
        }
    }
}

enum JobMsg {
    At(usize),
    /// What failed, what can be undone of what did not, and when stopped, how far it got.
    Done(Vec<(PathBuf, std::io::Error)>, Record, Option<String>),
    /// An undo has ended: what came of it.
    Undone(coxswain_core::undo::Undone),
}

/// *Make PDF searchable* on its thread (`coxswain_core::searchable`): how far it is, what
/// OCRmyPDF said last, and the flag that stops it.
struct Searchable {
    names: Vec<String>,
    at: usize,
    line: String,
    /// The status line it showed last: another message is not written over.
    shown: String,
    started: Instant,
    stop: Arc<std::sync::atomic::AtomicBool>,
    rx: mpsc::Receiver<SearchableMsg>,
}

/// The PDFs, and the question's text or why it cannot run.
type SearchablePlan = (Vec<PathBuf>, Result<String, String>);

/// Checksums being made or checked on their thread (`checksum`): what it is (its action), how
/// far, and the flag that stops it.
struct Work {
    action: Action,
    progress: coxswain_core::checksum::Progress,
    /// The status line it showed last: another message is not written over.
    shown: String,
    stop: Arc<std::sync::atomic::AtomicBool>,
    rx: mpsc::Receiver<WorkMsg>,
}

enum WorkMsg {
    At(coxswain_core::checksum::Progress),
    Made(std::io::Result<coxswain_core::checksum::Made>),
    /// The list checked, and what came of it (and whether it was stopped).
    Checked(PathBuf, std::io::Result<(Vec<coxswain_core::checksum::Checked>, bool)>),
    Split(std::io::Result<coxswain_core::split::Split>),
    Joined(std::io::Result<coxswain_core::split::Joined>),
}

/// What to make of which files, and the question's text or why it cannot run.
type MediaPlan = (coxswain_core::media::Op, Vec<PathBuf>, Result<String, String>);

/// The feature (F1) and the action of ffmpeg's and ImageMagick's work.
fn media_feature(p: coxswain_core::media::Program) -> &'static str {
    p.feature()
}
fn media_action(p: coxswain_core::media::Program) -> Action {
    p.action()
}

enum SearchableMsg {
    At(usize),
    Said(String),
    Done(Vec<Result<PathBuf, String>>),
}

/// An export on its thread (`coxswain_core::export`): what it makes, of which files, how far
/// it is, what the program said last, and the flag that stops it.
struct Exporting {
    op: coxswain_core::export::Op,
    names: Vec<String>,
    at: usize,
    line: String,
    /// The status line it showed last: another message is not written over.
    shown: String,
    started: Instant,
    stop: Arc<std::sync::atomic::AtomicBool>,
    rx: mpsc::Receiver<ExportMsg>,
}

enum ExportMsg {
    At(usize),
    Said(String),
    Done(Vec<Result<Vec<PathBuf>, String>>),
}

/// What to make of which files, and the question's text or why it cannot run.
type ExportPlan = (coxswain_core::export::Op, Vec<PathBuf>, Result<String, String>);

/// The feature (F1) and the action of an export.
fn export_feature(op: coxswain_core::export::Op) -> &'static str {
    if op == coxswain_core::export::Op::AdocPdf { "adoc_pdf" } else { "drawio_export" }
}
fn export_action(op: coxswain_core::export::Op) -> Action {
    if op == coxswain_core::export::Op::AdocPdf { Action::AdocPdf } else { Action::DrawioExport }
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

/// The operation a job runs (`None`: packing into `dst`; a 7z's names are hidden too, what
/// the desktop app does by default).
fn job_op(op: Option<Transfer>) -> coxswain_core::undo::Op {
    use coxswain_core::undo::Op;
    match op {
        None => Op::Pack { hide_names: true },
        Some(Transfer::Copy) => Op::Copy,
        Some(Transfer::Move | Transfer::Rename) => Op::Move,
        Some(Transfer::Extract) => Op::Extract,
        Some(Transfer::Delete(false)) => Op::Trash,
        Some(Transfer::Delete(true)) => Op::Forever,
        Some(Transfer::Mkdir) => Op::Mkdir,
    }
}

/// A folder listed by a program (git, zfs, pkg), which can take a while: read on a thread.
/// The folder all of `files` are in, for showing their paths short.
fn common_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut dir = files.first()?.parent()?.to_path_buf();
    while !files.iter().all(|f| f.starts_with(&dir)) {
        dir = dir.parent()?.to_path_buf();
    }
    Some(dir)
}

fn virtual_list(dir: &Path) -> bool {
    history::is_history(dir) || coxswain_core::zfs::split(dir).is_some() || coxswain_core::bsd::split(dir).is_some()
}

impl App {
    fn new(cfg: Config, left: PathBuf, right: PathBuf) -> Result<App, String> {
        // The servers a Subversion or CVS history may be read from.
        coxswain_core::wc::allow(&coxswain_core::state::AppState::load().history_servers);
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
            // When it starts, then every hour (a request only once a day): each new version once.
            std::thread::spawn(move || {
                let mut told = String::new();
                loop {
                    if let Some(v) = coxswain_core::update::check().filter(|v| *v != told) {
                        if update_tx.send(v.clone()).is_err() {
                            return;
                        }
                        told = v;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            });
        }
        let show_hidden = cfg.show_hidden;
        // Listed by `load` below, on a thread when that takes long, so the first frame is
        // drawn at once even for a folder of a million files.
        let ((left, left_file), (right, right_file)) = (Panel::unlisted(left), Panel::unlisted(right));
        let app = App {
            keymap,
            theme: cfg.theme(),
            glyphs: cfg.glyphs(),
            panels: [left, right],
            active: 0,
            cmdline: String::new(),
            dialog: None,
            status: None,
            hint: None,
            hint_for: None,
            enter_on: None,
            hint_stale: true,
            find_hint: false,
            quick: None,
            show_hidden,
            index: index.clone(),
            areas: [Rect::default(); 2],
            git_tx,
            git_rx,
            git_busy: Default::default(),
            update_rx,
            sizer: Arc::new(coxswain_core::sizes::Sizer::new(Some(index.clone()))),
            sizes_tx,
            sizes_rx,
            search_tx,
            search_rx,
            search_gen: 0,
            search_got: 0,
            chat: vec![],
            ask_rx: None,
            ask_stop: Arc::default(),
            ask_checked: None,
            ask_check_rx: None,
            ask_problem: None,
            find_dismissed: vec![],
            slow: (None, None),
            told: Instant::now(),
            measuring: Default::default(),
            run: None,
            job: None,
            searchable: None,
            searchable_plan: None,
            media: None,
            media_plan: None,
            ebook: None,
            ebook_plan: None,
            work: None,
            transcribe_plan: None,
            export: None,
            export_plan: None,
            viruses: None,
            viruses_plan: None,
            undo: Default::default(),
            list_tx,
            list_rx,
            #[cfg(test)]
            list_wait: tests::LIST_WAIT.get(),
            #[cfg(not(test))]
            list_wait: Duration::from_millis(150),
            #[cfg(test)]
            list_hold: tests::LIST_HOLD.with_borrow(Clone::clone),
            last_click: None,
            props_rx: None,
            watch: Default::default(),
            space_back: None,
            waiting: Vec::new(),
            folders_said: false,
            folders_lead: None,
            quit: false,
            input_note: None,
            cfg,
        };
        let mut app = app;
        app.load(0, left_file);
        if app.panels[1].dir == app.panels[0].dir && right_file.is_none() {
            // One listing for both: a listing still under way fills both panels when it comes.
            app.panels[1].entries = app.panels[0].entries.clone();
            app.panels[1].error = app.panels[0].error.clone();
            app.panels[1].fill(right_file);
        } else {
            app.load(1, right_file);
        }
        app.refresh_git(&[0, 1]);
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

    /// The key shown for `a`: one the terminal delivers (Settings' F12, not Ctrl+,).
    pub fn key_label(&self, a: Action) -> &str {
        self.cfg.tty_key(a).unwrap_or("")
    }

    /// The action's name in F1 and F9: Undo says what it would undo.
    pub fn action_label(&self, a: Action) -> String {
        match self.undo.next() {
            Some(r) if a == Action::Undo => t!("undo.menu", "what" => r.label()),
            _ => a.label(),
        }
    }

    /// git status, and the last commit of each entry, of the panels `sides`, run on a thread; results arrive
    /// through `git_rx`.
    fn refresh_git(&self, sides: &[usize]) {
        let dirs: HashSet<PathBuf> = sides.iter().map(|&s| self.panels[s].dir.clone()).collect();
        let last = self.cfg.git.last_commit;
        for dir in dirs {
            // One read per folder at a time: a build that keeps writing asks again once,
            // when the read under way ends, instead of starting a pile of them.
            if let Some(again) = self.git_busy.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&dir) {
                *again = true;
            } else {
                self.git_busy.lock().unwrap_or_else(|e| e.into_inner()).insert(dir.clone(), false);
                let (tx, busy, d) = (self.git_tx.clone(), self.git_busy.clone(), dir.clone());
                std::thread::spawn(move || {
                    let dir = d;
                    loop {
                        let st = git::Status::read(&dir);
                        let repo = st.is_some() || history::is_history(&dir);
                        if tx.send((dir.clone(), Git::Status(st.map(Box::new)))).is_ok() && last && repo {
                            let _ = tx.send((dir.clone(), Git::Last(history::last_changes(&dir))));
                        }
                        let mut busy = busy.lock().unwrap_or_else(|e| e.into_inner());
                        if !busy.get(&dir).copied().unwrap_or(false) {
                            busy.remove(&dir);
                            return;
                        }
                        busy.insert(dir.clone(), false);
                    }
                });
            }
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
        let stale = self.forget(paths);
        // A panel showing a folder the change is in, not the folder it is in itself.
        for side in (0..2).filter(|&s| stale[s]) {
            self.measure(side);
        }
    }

    /// The sizes of the folders these are in, forgotten: by each panel that dropped one.
    fn forget(&mut self, paths: &[PathBuf]) -> [bool; 2] {
        let mut stale = [false; 2];
        for p in paths {
            self.sizer.forget(p);
            for (side, panel) in self.panels.iter_mut().enumerate() {
                let had = panel.sizes.len();
                panel.sizes.retain(|folder, _| !p.starts_with(folder));
                stale[side] |= panel.sizes.len() < had;
            }
        }
        stale
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
        // The folder above, only when its size is already known: never walked from here.
        let up = p.entries.iter().find(|e| e.is_parent()).map(|e| e.path.clone()).filter(|u| !p.sizes.contains_key(u));
        std::thread::spawn(move || {
            if let Some(up) = up
                && let Some((bytes, _)) = sizer.known_only(&up)
                && tx.send((dir.clone(), up, bytes)).is_err()
            {
                return;
            }
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
        self.refresh_git(&[0, 1]);
        self.measure(0);
        self.measure(1);
    }

    fn cd(&mut self, side: usize, dir: PathBuf) {
        let came = self.panels[side].enter(dir.clone());
        self.load(side, came);
        self.refresh_git(&[side]);
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
        self.hint_stale = true;
        let h = self.show_hidden;
        let p = &mut self.panels[side];
        let keep = select.or_else(|| p.current().map(|e| e.name.clone()));
        let (dir, tx) = (p.dir.clone(), self.list_tx.clone());
        // The keys wait no longer than `list_wait` for a folder on disk: one of a million files
        // is said to be read meanwhile, and measured once it is listed.
        let virtual_dir = virtual_list(&dir);
        let d = dir.clone();
        let k = keep.clone();
        #[cfg(test)]
        let hold = self.list_hold.clone();
        std::thread::spawn(move || {
            #[cfg(test)]
            drop(hold.as_ref().map(|h| h.read()));
            let r = bfs::list(&d, h);
            let _ = tx.send((d, k, r));
        });
        if !virtual_dir {
            let until = Instant::now() + self.list_wait;
            while let Ok((d, k, r)) = self.list_rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                if d != dir {
                    self.listed(d, k, r);
                } else if r.is_err() {
                    // Walking up keeps the reason, as a locked archive's password asks by it.
                    let p = &mut self.panels[side];
                    p.load(h);
                    if let Some(n) = keep {
                        p.select_name(&n);
                    }
                    // Whole on the status line, as a listing from the thread says it: the
                    // panel's info line cuts it to the panel's width.
                    self.status = p.error.clone();
                    return;
                } else {
                    return self.listed(d, k, r);
                }
            }
        }
        // Meanwhile `..` leads out of a folder entered.
        let p = &mut self.panels[side];
        if !virtual_dir
            && p.entries.is_empty()
            && let Some(up) = dir.parent()
        {
            p.entries.push(Entry { name: "..".into(), path: up.to_path_buf(), is_dir: true, ..Entry::default() });
        }
        self.status = Some(t!("status.busy", "what" => Self::describe(std::slice::from_ref(&dir))));
    }

    /// A listing from the thread, for the panels still showing its folder. One that failed
    /// walks up, as `Panel::load` does, and the status line says why.
    fn listed(&mut self, dir: PathBuf, keep: Option<String>, mut r: std::io::Result<Vec<Entry>>) {
        self.hint_stale = true;
        // The last panel showing the folder takes the listing itself: a copy of a folder of
        // millions is as large again.
        let last = (0..2).rev().find(|&s| self.panels[s].dir == dir);
        let busy = t!("status.busy", "what" => Self::describe(std::slice::from_ref(&dir)));
        if self.status.as_deref() == Some(busy.as_str()) {
            self.status = None;
        }
        for side in 0..2 {
            let p = &mut self.panels[side];
            if p.dir != dir {
                continue;
            }
            match &mut r {
                Ok(v) => {
                    let v = if Some(side) == last { std::mem::take(v) } else { v.clone() };
                    (p.entries, p.link) = (v, None);
                    p.error = None;
                    p.fill(keep.clone());
                    // Listed after its key had gone on: its folders measured now.
                    if !virtual_list(&dir) && self.panels[side].sizes.is_empty() {
                        self.measure(side);
                    }
                }
                Err(e) => {
                    p.error = Some(e.to_string());
                    let why = p.error.clone();
                    if let Some(up) = p.dir.parent() {
                        p.dir = up.to_path_buf();
                        self.load(side, None);
                    }
                    // After the walk up: a parent read on a thread too says it is busy, and
                    // its listing clears only that.
                    self.status = why;
                }
            }
        }
    }

    fn input(&mut self, title: &str, label: String, value: String, prompt: Prompt) {
        self.status = None;
        self.input_note = None;
        self.dialog = Some(Dialog::Input { title: title.into(), label, value, prompt });
    }

    fn describe(paths: &[PathBuf]) -> String {
        match paths {
            [one] => format!("\"{}\"", ui::printable(&one.file_name().unwrap_or_default().to_string_lossy())),
            _ => tn!("items", paths.len()),
        }
    }

    // ------------------------------------------------------------ input

    fn on_key(&mut self, ev: KeyEvent) {
        if let Some(key) = to_key(ev) {
            self.key_typed(key);
        }
    }

    fn key_typed(&mut self, key: Key) {
        self.hint_stale = true;
        self.status = None;
        if self.dialog.is_some() {
            return self.dialog_key(key);
        }
        let plain_char = typed(&key);
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
        // Esc on the progress line: the file operation under way stops (`undo::run`).
        if key.code == KeyCode::Esc
            && !(key.ctrl || key.alt || key.shift)
            && let Some(stop) = self.job.as_ref().and_then(|j| j.stop.clone())
        {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = self.job.as_ref().map(Job::shown);
        }
        match (action, plain_char) {
            (Some(a), _) => self.act(a),
            (None, Some(c)) => self.cmdline.push(c),
            _ => {}
        }
    }

    fn quick_jump(&mut self) {
        let q = self.quick.clone().unwrap_or_default();
        let p = self.panel_mut();
        if let Some(i) = coxswain_core::fs::quick_match(&p.entries, &q) {
            p.cursor = i;
        }
    }

    fn act(&mut self, a: Action) {
        // Inside a disk image (a CHM, a CBR) nothing changes: said before any question is asked.
        if matches!(a, Action::Move | Action::Rename | Action::Delete | Action::DeleteForever | Action::NewFolder)
            && coxswain_core::archive::split(&self.panel().dir).is_some_and(|(i, _)| coxswain_core::archive::read_only(&i))
        {
            return self.status = Some(t!("disk_image.read_only"));
        }
        if matches!(a, Action::Move | Action::Rename | Action::Delete | Action::DeleteForever | Action::NewFolder) && coxswain_core::archive::nested_dir(&self.panel().dir) {
            return self.status = Some(t!("archive.nested_read_only"));
        }
        match a {
            // A second Quit while a job runs leaves anyway: a dead network mount must not
            // hold the app. The job's thread ends with the process, its file half done.
            Action::Quit if self.quit && (self.job.is_some() || self.searchable.is_some() || self.media.is_some() || self.ebook.is_some() || self.work.is_some() || self.export.is_some() || self.viruses.is_some()) => {
                (self.job, self.searchable, self.media, self.ebook, self.work, self.export, self.viruses) = (None, None, None, None, None, None, None)
            }
            Action::Quit => {
                self.quit = true;
                // Make PDF searchable is stopped, and waited for: its container is killed,
                // its scratch folder removed, and no copy is left half made.
                if let Some(s) = &self.searchable {
                    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // So is a conversion: its container killed, nothing half made left beside the file.
                if let Some((s, _)) = &self.media {
                    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // So is calibre: its container killed, nothing half made left beside the book.
                if let Some((s, ..)) = &self.ebook {
                    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // Checksums, a split or a join: stopped, and waited for, so a split's parts and a
                // join's file are not left half made.
                if let Some(w) = &self.work {
                    w.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // And a file operation, between files: none is left half copied.
                if let Some(s) = self.job.as_ref().and_then(|j| j.stop.as_ref()) {
                    s.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // So is an export.
                if let Some(x) = &self.export {
                    x.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // So is a virus scan: its container is killed.
                if let Some(x) = &self.viruses {
                    x.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if self.job.is_some() || self.searchable.is_some() || self.media.is_some() || self.ebook.is_some() || self.work.is_some() || self.export.is_some() || self.viruses.is_some() {
                    self.status = Some(t!("status.quit_job", "key" => self.key_label(Action::Quit)));
                }
            }
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
            Action::History => self.history(history::Scope::Branch),
            Action::HistoryAll => self.history(history::Scope::All),
            Action::HistoryUpstream => self.history(history::Scope::Upstream),
            Action::Snapshots => self.snapshots(),
            Action::Package => self.package(),
            Action::Flags => self.flags(),
            Action::Properties => self.properties(),
            Action::Branches => self.git_view(history::View::Branches),
            Action::Worktrees => self.git_view(history::View::Worktrees),
            Action::SwitchBranch => self.switch_branch(),
            Action::NewBranch => self.new_branch(),
            Action::GitTool => self.git_tool(),
            Action::GitConfig => self.git_config(),
            // A file in the viewer with who changed each line; a function's history is asked there.
            Action::Blame | Action::LineHistory => match self.panel().current().filter(|e| !e.is_dir && !e.is_parent()).map(|e| e.path.clone()) {
                Some(file) if a == Action::Blame => {
                    self.view_in(&file, None);
                    if let Some(Dialog::View { v, .. }) = &mut self.dialog {
                        v.blame();
                    }
                }
                _ => self.status = Some(t!("blame.history_where", "view" => self.key_label(Action::View), "edit" => self.key_label(Action::EditHere), "key" => self.key_label(Action::LineHistory))),
            },
            Action::Replace => self.replace(None),
            Action::Attrs => self.attrs(),
            Action::SearchablePdf => self.searchable_pdf(),
            Action::ConvertMedia => self.media_menu(coxswain_core::media::Program::Ffmpeg),
            Action::EditPictures => self.media_menu(coxswain_core::media::Program::Magick),
            Action::ConvertGeodata => self.media_menu(coxswain_core::media::Program::Gdal),
            Action::ExportModel => self.media_menu(coxswain_core::media::Program::Openscad),
            Action::ConvertEbook => self.ebook_menu(),
            Action::ExportKicad => self.media_menu(coxswain_core::media::Program::Kicad),
            Action::MakeChecksums => self.sums_menu(),
            Action::SplitFile => self.split_menu(),
            Action::JoinParts => match self.panel().current().filter(|e| !e.is_dir && coxswain_core::split::joins_into(&e.path).is_some()) {
                Some(e) => self.join_menu(e.path.clone()),
                None => self.status = Some(t!("split.not_first")),
            },
            Action::VerifyChecksums => match self.panel().current().filter(|e| !e.is_dir && coxswain_core::checksum::is_list(&e.path)) {
                Some(e) => self.sums_verify(e.path.clone()),
                None => self.status = Some(t!("sums.no_list")),
            },
            Action::Transcribe => self.transcribe(),
            Action::AdocPdf => self.export_menu(true),
            Action::DrawioExport => self.export_menu(false),
            Action::ScanViruses => self.scan_viruses(),
            Action::PreviewWith => self.preview_with(),
            Action::FollowLog => {
                if let Some(file) = self.panel().current().filter(|e| !e.is_dir).map(|e| e.path.clone()) {
                    self.view_in(&file, None);
                    if let Some(Dialog::View { v, back }) = self.dialog.take() {
                        self.dialog = Some(Dialog::View { v: Box::new(v.following()), back });
                    }
                }
            }
            Action::LongFiles => self.long_files(),
            Action::Chess => {
                if let Some(e) = self.panel().current().filter(|e| !e.is_dir && coxswain_core::chess::sniff(&e.path).is_some()).map(|e| e.path.clone()) {
                    self.chess(&e, None);
                } else {
                    self.status = Some(t!("chess.not_chess"));
                }
            }
            Action::SpaceView => self.space_view(),
            Action::CompareFiles => self.compare_files(),
            Action::CompareFolders => self.compare_folders(),
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
                self.measuring.swap(0, 1);
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
                // The cloud accounts, on FreeBSD the boot environments and jails, in Termux the
                // phone's folders: those to pick, as NC's drive list, under the line that types a
                // path.
                let places = coxswain_core::bsd::places();
                let phone = coxswain_core::termux::storage();
                // A hung cloud mount must not hang the app: the list comes without them.
                let clouds = coxswain_core::clouds::places_within(Duration::from_millis(300)).unwrap_or_else(|| {
                    self.status = Some(t!("clouds.slow"));
                    vec![]
                });
                if places.is_empty() && phone.is_empty() && clouds.is_empty() {
                    return self.goto_prompt(side);
                }
                let mut items = vec![MenuItem { key: String::new(), label: t!("bsd.type_path"), run: MenuRun::Goto(side, None), group: None }];
                items.extend(phone.into_iter().map(|(name, path)| MenuItem { key: String::new(), label: t!("termux.storage", "name" => name), run: MenuRun::Goto(side, Some(path)), group: None }));
                items.extend(clouds.into_iter().map(|c| MenuItem { key: String::new(), label: t!("clouds.goto", "name" => c.name), run: MenuRun::Goto(side, c.path), group: None }));
                items.extend(places.into_iter().map(|p| {
                    let what = t!(if p.kind == "boot" { "bsd.boot_env" } else { "bsd.jail" });
                    let label = if p.note.is_empty() { format!("{what}: {}", p.name) } else { format!("{what}: {} ({})", p.name, p.note) };
                    let run = match p.path {
                        Some(path) => MenuRun::Goto(side, Some(path)),
                        None => MenuRun::Say(t!(if p.kind == "boot" { "bsd.not_mounted" } else { "bsd.not_readable" }, "name" => p.name)),
                    };
                    MenuItem { key: String::new(), label, run, group: None }
                }));
                self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: false, feature: "moving" });
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
                    // For the shell that runs the line: fish quotes otherwise.
                    let q = config::quote_for(&std::env::var("SHELL").unwrap_or_default(), &e.name);
                    if !self.cmdline.is_empty() && !self.cmdline.ends_with(' ') {
                        self.cmdline.push(' ');
                    }
                    self.cmdline += &q;
                    self.cmdline.push(' ');
                }
            }
            Action::View | Action::Edit | Action::EditHere => {
                if let Some(e) = self.panel().current().filter(|e| !e.is_dir).map(|e| e.path.clone()) {
                    // Inside an archive the file is not on disk: F3 views a copy of it, and
                    // editing waits until it is copied out.
                    if let Some((archive, _)) = coxswain_core::archive::split(&self.panel().dir) {
                        if a == Action::View {
                            return self.peek(e);
                        }
                        return self.status = Some(t!("archive.copy_out_hint", "archive" => archive.file_name().unwrap_or_default().to_string_lossy()));
                    }
                    // A 7z split into parts with one missing: which one, on the status line,
                    // rather than its first part's bytes.
                    if a == Action::View
                        && let Some(why) = coxswain_core::archive::missing_part(&e)
                    {
                        return self.status = Some(why);
                    }
                    // In a snapshot: viewed as it is there; never edited.
                    if a != Action::View && coxswain_core::zfs::in_snapshot(&e).is_some() {
                        return self.status = Some(t!("zfs.read_only"));
                    }
                    // In a history: viewed as it was then, from a copy; never edited.
                    if history::is_history(&e) {
                        if a != Action::View {
                            return self.status = Some(t!("history.read_only"));
                        }
                        return match history::peek(&e) {
                            Ok(copy) => self.view_or_edit(a, &copy),
                            Err(err) => self.status = Some(err.to_string()),
                        };
                    }
                    // Coxswain's own name index or search store, here or a copy: what it holds.
                    if a == Action::View
                        && let Some(r) = coxswain_core::disk::describe(&e)
                    {
                        match coxswain_core::disk::text_copy(&e, &r) {
                            Ok(text) => return self.view_copy(&text, &e, r.title.clone()),
                            Err(err) => self.status = Some(err),
                        }
                    }
                    // An e-book calibre reads, when e-books are on: its title, authors and text.
                    if a == Action::View && coxswain_core::ebook::is_ebook(&e) {
                        if self.cfg.preview.ebooks {
                            return self.ebook_view(e);
                        }
                        // Off: shown as it is, and where to turn its text on.
                        self.status = Some(format!("{} ({})", t!("ebook.off"), t!("searchable.settings_key", "key" => self.key_label(Action::Settings))));
                    }
                    // An old picture, a tracker module, an Outlook message: what it holds, as text.
                    if a == Action::View
                        && let Some(r) = coxswain_core::facts::text_copy(&e)
                    {
                        match r {
                            Ok((text, format)) => return self.view_copy(&text, &e, format),
                            Err(err) => self.status = Some(err),
                        }
                    }
                    // A database, a dBase table or a Parquet file: its tables and first rows, as text.
                    if a == Action::View && coxswain_core::tables::is_tables(&e) {
                        match coxswain_core::tables::text_copy(&e) {
                            Ok(text) => return self.view_copy(&text, &e, coxswain_core::tables::format(&e).into()),
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
                    // A recording with its transcript: the transcript, as text.
                    if a == Action::View
                        && coxswain_core::transcribe::is_speech(&e)
                        && let Some(tr) = self.index.transcript(&e).filter(|t| !t.text.is_empty())
                    {
                        match coxswain_core::transcribe::text_copy(&e, &tr) {
                            Ok(copy) => return self.view_copy(&copy, &e, t!("whisper.transcript")),
                            Err(err) => self.status = Some(err),
                        }
                    }
                    if a == Action::View && coxswain_core::chess::sniff(&e).is_some() {
                        return self.chess(&e, None);
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
            // Nor in a git history: its commits and their files are as they were.
            Action::Move | Action::Rename | Action::NewFolder | Action::Delete | Action::DeleteForever if history::is_history(&self.panel().dir) => {
                self.status = Some(t!("history.read_only"));
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
                // Into an archive it is written anew beside itself: the room that takes.
                let note = coxswain_core::archive::split(&self.panels[self.active ^ 1].dir).and_then(|(a, _)| coxswain_core::archive::short_of_room(&a, &src));
                let prompt = if a == Action::Copy { Prompt::Copy(src) } else { Prompt::Move(src) };
                self.input(&title, t!("dialog.to"), dst, prompt);
                self.input_note = note;
            }
            Action::Extract => {
                let src: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| coxswain_core::archive::opens(p)).collect();
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
                let note = coxswain_core::archive::short_of_room(Path::new(&dst), &src);
                self.input(&title, t!("tui.pack_into"), dst, Prompt::Pack(src));
                self.input_note = note;
            }
            Action::Rename => {
                let Some(e) = self.panel().current().filter(|e| !e.is_parent()).cloned() else { return };
                let title = t!("dialog.rename", "what" => Self::describe(std::slice::from_ref(&e.path)));
                // A newline or an escape in the name shows as its picture, and comes back as it was.
                self.input(&title, t!("app.name_label"), coxswain_core::rename::shown(&e.name), Prompt::Rename(e.path));
            }
            Action::ActionMenu => self.action_menu(),
            Action::NewFolder => self.input(&t!("dialog.new_folder"), t!("tui.mkdir_label"), String::new(), Prompt::Mkdir),
            Action::Undo => self.undo_last(),
            Action::Delete | Action::DeleteForever => {
                let paths = self.panel().targets();
                self.ask_delete(paths, a == Action::DeleteForever);
            }
            Action::Search | Action::SearchText | Action::Ask => {
                let chip = match a {
                    Action::SearchText => Kind::InFiles,
                    Action::Ask => Kind::Ask,
                    _ => Kind::All,
                };
                self.find_dismissed = coxswain_core::state::AppState::load().notices_dismissed;
                self.find_hint = coxswain_core::menu::once("find_scope", &self.cfg);
                let show = if chip == Kind::Ask { Show::Answer } else { Show::List };
                self.dialog = Some(Dialog::Search { query: String::new(), chip, here: false, found: Found::default(), cursor: 0, offset: 0, show });
            }
            Action::UserMenu => {
                let p = self.panel();
                let file = p.current().filter(|e| !e.is_parent()).map(|e| e.path.as_path());
                let mut items: Vec<MenuItem> = coxswain_core::user_menu::entries(&self.cfg, &p.dir, file)
                    .into_iter()
                    .map(|u| MenuItem { key: u.key.clone(), label: u.label.clone(), run: MenuRun::User(u), group: None })
                    .collect();
                items.extend(coxswain_core::user_menu::scripts().into_iter().map(|p| MenuItem { key: String::new(), label: coxswain_core::user_menu::script_label(&p), run: MenuRun::Script(p), group: None }));
                items.push(MenuItem { key: String::new(), label: t!("usermenu.scripts_folder"), run: MenuRun::ScriptsFolder, group: None });
                let key = if items.iter().any(|i| i.key == "+") { "" } else { "+" };
                items.push(MenuItem { key: key.into(), label: t!("usermenu.add"), run: MenuRun::AddUser, group: None });
                self.dialog = Some(Dialog::Menu { title: t!("tui.user_menu"), filter: String::new(), items, cursor: 0, direct: true, feature: "user_menu" });
            }
            Action::Menu => {
                let items = Action::ALL
                    .iter()
                    .filter(|&&x| !x.gui_only() && !matches!(x, Action::Menu | Action::Up | Action::Down))
                    .map(|&x| MenuItem { key: self.key_label(x).to_string(), label: self.action_label(x), run: MenuRun::Action(x), group: None })
                    .collect();
                self.dialog = Some(Dialog::Menu { title: t!("menu.commands"), filter: String::new(), items, cursor: 0, direct: false, feature: "command_list" });
            }
            Action::Help => self.dialog = Some(Dialog::Help { scroll: 0 }),
            Action::Features => self.features(String::new(), None),
            Action::Settings => self.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(self, "")))),
            Action::DiskUse => self.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(self, "disk")))),
            Action::SuggestTranslation => self.status = Some(Self::suggest_translation()),
            Action::DiskImages => {
                let Some(path) = Config::path() else { return self.status = Some(t!("err.no_config_folder")) };
                let on = serde_json::json!({ "disk_images": true });
                self.status = Some(match self.save_options(&path, on.as_object().expect("an object")) {
                    Ok(()) => coxswain_core::disk_image::next_line(self.key_label(Action::Open), self.key_label(Action::Settings), self.key_label(Action::ActionMenu)),
                    Err(e) => e,
                });
            }
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
        let s = coxswain_core::menu::Subject { whisper: self.cfg.search.whisper, ..self.panel().subject() };
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
        self.dialog = Some(Dialog::Menu { title: t!("menu.title", "what" => what), filter: String::new(), items, cursor: 0, direct: false, feature: "action_menu" });
    }

    /// F1 → Features, filtered by `filter`; Esc goes back to `back`.
    fn features(&mut self, filter: String, back: Option<Box<Dialog>>) {
        let list = coxswain_core::features::list(&self.cfg, &filter, false);
        self.dialog = Some(Dialog::Features { filter, list, cursor: 0, said: None, back });
    }

    /// Show me: the feature's page on GitHub in the browser, and the address on the terminal's
    /// clipboard for when no browser opens (over ssh); what the dialog says about it.
    fn show_me(url: &str) -> String {
        guide::copy(url);
        match coxswain_core::fs::open_default(Path::new(url)) {
            Ok(()) => t!("features.opened", "url" => url),
            Err(_) => t!("features.copied", "url" => url),
        }
    }

    /// *Suggest a better translation*: a new GitHub issue for the language in use, in the
    /// browser (and on the clipboard); what to say about it.
    pub(crate) fn suggest_translation() -> String {
        Self::show_me(&coxswain_core::i18n::suggest_url(coxswain_core::i18n::language()))
    }

    /// A password that did not open the archive, asked for again: the box says so.
    fn asked_again(&mut self) {
        if let Some(Dialog::Input { label, prompt: Prompt::Peek(_) | Prompt::Unlock(..), .. }) = &mut self.dialog {
            *label = t!("archive.locked_again");
        }
    }

    /// F3 on a file inside an archive: a copy of it in the viewer, or its password asked for.
    fn peek(&mut self, path: PathBuf) {
        match coxswain_core::archive::peek(&path) {
            // Games (a PGN file, a ChessBase or SCID database with the files beside it, a DhtmlXQ
            // xiangqi game): on their board, as outside (`enter::view`).
            Ok(copy) if coxswain_core::enter::view(&copy, coxswain_core::enter::Place::Disk) == coxswain_core::enter::View::Games => self.chess(&copy, None),
            Ok(copy) => self.view_or_edit(Action::View, &copy),
            Err(e) if e.to_string().contains(coxswain_core::archive::LOCKED) => {
                self.input(&t!("archive.locked_title"), t!("archive.locked_label"), String::new(), Prompt::Peek(path));
            }
            Err(e) => self.status = Some(e.to_string()),
        }
    }

    fn open(&mut self) {
        let Some(e) = self.panel().current().cloned() else { return };
        // Enter goes into anything that holds things, and opens games on their board (`enter`).
        use coxswain_core::enter::{self, Enter};
        let inside = coxswain_core::archive::split(&self.panel().dir);
        let place = if inside.is_some() { enter::Place::Archive } else { enter::Place::Disk };
        let what = enter::enter(&e.path, e.is_dir, place);
        match what {
            Enter::Folder | Enter::Archive | Enter::DiskImage | Enter::Image | Enter::Mailbox => return self.cd(self.active, e.path),
            // From a copy, as F3 (a locked archive's password asked for).
            Enter::Games | Enter::Positions | Enter::Book if inside.is_some() => return self.peek(e.path),
            _ => {}
        }
        if let Some((archive, _)) = inside {
            // A `.bin` that may be an opening book: told by a copy's bytes.
            if enter::may_be_games(&e.path)
                && let Ok(copy) = coxswain_core::archive::peek(&e.path)
                && enter::view(&copy, enter::Place::Disk) == enter::View::Games
            {
                return self.chess(&copy, None);
            }
            let key = if coxswain_core::archive::read_only(&archive) { "disk_image.copy_out_hint" } else { "archive.copy_out_hint" };
            return self.status = Some(t!(key, "archive" => archive.file_name().unwrap_or_default().to_string_lossy()));
        }
        if history::is_history(&self.panel().dir) {
            return self.status = Some(t!("history.file_hint"));
        }
        // A file of a package: to its folder, the cursor on it.
        if coxswain_core::bsd::split(&self.panel().dir).is_some() {
            return self.reveal(&e.path.clone());
        }
        // Games on their board, positions, an opening book; in a snapshot too.
        if matches!(what, Enter::Games | Enter::Positions | Enter::Book) {
            return self.chess(&e.path, None);
        }
        // The first part of a split file: Join is offered.
        if coxswain_core::split::joins_into(&e.path).is_some() && coxswain_core::zfs::in_snapshot(&e.path).is_none() {
            return self.join_menu(e.path.clone());
        }
        // A list of checksums is checked (in a snapshot too).
        if coxswain_core::checksum::is_list(&e.path) {
            return self.sums_verify(e.path.clone());
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
            let mut line = Line::default();
            line.cmd = if cfg!(windows) { line.word(&e.name) } else { format!("./{}", line.word(&e.name)) };
            self.run = Some(Run::Shell { line, typed: false, dir, wait: true });
            return;
        }
        self.status = Some(match bfs::open_default(&e.path) {
            Ok(()) => t!("status.opened", "name" => e.name),
            Err(err) => t!("status.open_failed", "error" => err),
        });
    }

    /// Into the history of the entry under the cursor (of this folder on `..`): its commits on
    /// this branch, every branch's or those against the upstream (`scope`). In a history, every
    /// branch's and against the upstream switch to that list, and back.
    fn history(&mut self, scope: history::Scope) {
        let p = self.panel();
        let target = match p.current() {
            Some(e) if !e.is_parent() => e.path.clone(),
            _ => p.dir.clone(),
        };
        if scope != history::Scope::Branch && history::is_history(&p.dir) {
            return match history::switch_scope(&p.dir, &target, scope) {
                Some(to) => self.cd(self.active, to),
                None => self.status = Some(t!("history.all_not_here")),
            };
        }
        if history::is_history(&target) || coxswain_core::archive::split(&target).is_some() || !target.exists() {
            return self.status = Some(t!("history.not_here"));
        }
        if p.git.is_none() {
            return self.status = Some(t!("history.no_repo"));
        }
        let to = history::path_in(&target, scope, None);
        // A Subversion or CVS history is on its server: asked once per server, before anything is sent.
        if let Some(ask) = coxswain_core::wc::ask_first(&target) {
            return self.dialog = Some(Dialog::Server { title: t!("history.ask_server_title"), text: ask.text(), feature: if ask.program == "svn" { "subversion" } else { "cvs" }, server: ask.server, to });
        }
        self.cd(self.active, to);
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
            // A part of an archive split into parts, inside an archive: which part, by its name.
            if let Some((k, v)) = coxswain_core::facts::part(&path.file_name().unwrap_or_default().to_string_lossy()) {
                return self.status = Some(format!("{k}: {v}"));
            }
            return self.status = Some(t!("tui.props_not_here"));
        }
        let (tx, rx) = mpsc::channel();
        self.status = Some(t!("app.reading_properties"));
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            // A video's length, size and tracks too, by ffprobe (`media`).
            let r = bfs::properties(&path).map(|p| coxswain_core::media::with_facts(p, &cfg, &path)).map_err(|e| e.to_string());
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
        let branch = coxswain_core::branches::name_of(&e.name);
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
            Some(e) => t!("branches.new_from", "branch" => coxswain_core::branches::name_of(e)),
            None => t!("branches.new_here"),
        };
        self.input(&t!("action.new_branch"), label, String::new(), Prompt::NewBranch(p.dir.clone(), from));
    }

    /// Commit, push, pull and the rest: the repository in the user's own git tool. One that
    /// runs in a terminal runs in this one, as F4's editor does; a window of its own is started
    /// beside. The panels are read again after.
    fn git_tool(&mut self) {
        use coxswain_core::handoff;
        let p = self.panel();
        // From the branches and the history too: their repository.
        let dir = history::split(&p.dir).map_or_else(|| p.dir.clone(), |at| at.base);
        let h = match handoff::handoff(&self.cfg.git.tool, &dir, true) {
            Ok(h) => h,
            Err(e) => return self.status = Some(e),
        };
        self.status = Some(handoff::opened(&h));
        match &h.run {
            handoff::Run::Here(cmd) => self.run = Some(Run::Shell { line: Line::from(cmd.clone()), typed: false, dir: h.root.clone(), wait: false }),
            handoff::Run::Spawn(..) | handoff::Run::Line(_) => {
                if let Err(e) = handoff::spawn(&h) {
                    self.status = Some(e.to_string());
                }
                self.reload();
            }
        }
    }

    /// A chess file full screen: a PGN file's games (at the game of a Find hit's snippet), an EPD or
    /// FEN file's positions.
    fn chess(&mut self, path: &Path, hit: Option<String>) {
        // Inside an archive: from a copy, as F3 views any file there.
        let copy;
        let path = if !path.exists() && coxswain_core::archive::split(path).is_some() {
            match coxswain_core::archive::peek(path) {
                Ok(c) => {
                    copy = c;
                    &copy
                }
                Err(e) => return self.status = Some(e.to_string()),
            }
        } else {
            path
        };
        if coxswain_core::chess::format(path) == Some(coxswain_core::chess::Format::Sgf) {
            return match go::Viewer::open(path, &self.cfg, &self.theme) {
                Ok(mut v) => {
                    // Looked for on a thread: it may read up to 1 GiB of the file.
                    if let Some(s) = hit {
                        v.at_hit(s);
                    }
                    self.dialog = Some(Dialog::Go(Box::new(v)));
                }
                Err(e) => self.status = Some(e.to_string()),
            };
        }
        match chess::Viewer::open(path, &self.cfg, &self.theme).map(|v| v.with_index(self.index.clone())) {
            Ok(mut v) => {
                if let Some(snippet) = hit {
                    v.at_hit(snippet);
                }
                self.dialog = Some(Dialog::Chess(Box::new(v)));
            }
            Err(e) => self.status = Some(e.to_string()),
        }
    }

    /// The git config as git uses it in the panel's folder, read now, full screen. Outside a
    /// repository: the system and global config.
    /// Permissions and dates (`attrs`) of the marked entries, or the one under the cursor.
    fn attrs(&mut self) {
        if self.panel().subject().place != coxswain_core::menu::Place::Disk {
            return self.status = Some(t!("attrs.not_here"));
        }
        let roots = self.panel().targets();
        if roots.is_empty() {
            return self.status = Some(t!("attrs.nothing_marked"));
        }
        let base = self.panel().dir.clone();
        self.dialog = Some(Dialog::Attrs(Box::new(attrs::Viewer::new(roots, base))));
    }

    /// Permissions and dates have been changed: said on the status line, kept for undo.
    fn attrs_done(&mut self, done: Result<coxswain_core::attrs::Applied, String>) {
        let done = match done {
            Ok(d) => d,
            Err(e) => return self.dialog = Some(failure(t!("attrs.failed"), &e)),
        };
        let mut ok = done.summary();
        if !done.record.is_empty() {
            ok += &format!(" · {}", t!("undo.hint", "key" => self.key_label(Action::Undo)));
            self.undo.push(done.record);
        }
        self.after_op(ok, t!("attrs.failed"), done.failed);
    }

    /// One side has been made like the other: said on the status line, kept for undo; the
    /// compare starts again, so the marks show what is left.
    fn sync_done(&mut self, done: Result<coxswain_core::compare::sync::Applied, String>) {
        let done = match done {
            Ok(d) => d,
            Err(e) => return self.dialog = Some(failure(t!("sync.failed"), &e)),
        };
        let mut ok = done.summary();
        if let Some(k) = done.kept_text() {
            ok += &format!(" · {k}");
        }
        if !done.record.is_empty() {
            ok += &format!(" · {}", t!("undo.hint", "key" => self.key_label(Action::Undo)));
            self.undo.push(done.record);
        }
        if let Some(c) = self.panels[0].cmp.clone() {
            self.folders_start(c.f.mode);
            self.folders_lead = Some(ok.clone());
        }
        self.after_op(ok, t!("sync.failed"), done.failed);
    }

    /// Find and replace (`replace`) in `files` (Find's results), else the marked entries, the
    /// one under the cursor or, on `..`, the panel's folder.
    fn replace(&mut self, from_find: Option<(Vec<PathBuf>, String)>) {
        let dir = self.panel().dir.clone();
        if self.panel().subject().place != coxswain_core::menu::Place::Disk && from_find.is_none() {
            return self.status = Some(t!("replace.not_here"));
        }
        let (roots, find, base) = match from_find {
            Some((files, find)) => {
                let base = common_dir(&files).unwrap_or_default();
                (files, find, base)
            }
            None => {
                let t = self.panel().targets();
                (if t.is_empty() { vec![dir.clone()] } else { t }, String::new(), dir)
            }
        };
        if roots.is_empty() {
            return self.status = Some(t!("replace.no_files"));
        }
        self.dialog = Some(Dialog::Replace(Box::new(replace::Viewer::new(roots, base, find))));
    }

    /// A replace has been written: what came of it on the status line, kept for undo.
    fn replaced(&mut self, done: Result<coxswain_core::replace::Applied, String>) {
        let done = match done {
            Ok(d) => d,
            Err(e) => return self.dialog = Some(failure(t!("replace.failed"), &e)),
        };
        let mut ok = done.summary();
        if !done.record.is_empty() {
            ok += &format!(" · {}", t!("undo.hint", "key" => self.key_label(Action::Undo)));
            self.undo.push(done.record);
        }
        self.after_op(ok, t!("replace.failed"), done.failed);
    }

    fn git_config(&mut self) {
        let p = self.panel();
        let dir = history::split(&p.dir).map_or_else(|| p.dir.clone(), |at| at.base);
        let entries = match coxswain_core::git_config::read(&dir) {
            Ok(v) => v,
            Err(e) => return self.dialog = Some(failure(t!("action.git_config"), &e)),
        };
        let repo = coxswain_core::git_config::repo_name(&dir);
        let labels = [self.key_label(Action::Edit), self.key_label(Action::Copy), self.key_label(Action::GitTool)];
        // How a hidden secret is shown: said the first few times one is there.
        let hint = entries.iter().any(|e| e.hidden.is_some()) && coxswain_core::menu::once("git_config_show", &self.cfg);
        let v = git_config::Viewer::new(entries, repo, labels, hint);
        self.dialog = Some(Dialog::GitConfig(Box::new(v)));
    }

    /// *Make PDF searchable* on the PDFs among the marked entries (or the one under the cursor):
    /// asked first, with what it writes, which program runs and what a first run pulls. While
    /// one runs, its key stops it.
    fn searchable_pdf(&mut self) {
        use coxswain_core::searchable;
        if let Some(s) = &self.searchable {
            s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = Some(t!("searchable.stopping"));
        }
        // A PDF only in the cloud is left out: it is not downloaded to be read.
        let pdfs: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| searchable::is_pdf(p) && !coxswain_core::cloud::unasked(p) && p.is_file()).collect();
        if pdfs.is_empty() {
            return self.status = Some(t!("searchable.none"));
        }
        // Asking podman for the image can take a moment: on a thread, so keys keep working.
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            let plan = searchable::how(&cfg).map(|how| {
                let what = match pdfs.as_slice() {
                    [one] => {
                        let name = one.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let target = searchable::target(one).file_name().unwrap_or_default().to_string_lossy().into_owned();
                        t!("searchable.confirm_one", "name" => name, "target" => target)
                    }
                    many => t!("searchable.confirm_many", "n" => many.len()),
                };
                format!("{what} {}", searchable::about(&how).0)
            });
            let _ = tx.send((pdfs, plan));
        });
        self.searchable_plan = Some(rx);
    }

    /// *Transcribe speech* on the marked entries (or the one under the cursor): with
    /// transcribing off, how to turn it on; else the question, once the lengths are read.
    fn transcribe(&mut self) {
        let title = t!("action.transcribe");
        if !self.cfg.search.whisper || !self.cfg.search.text {
            let text = t!("whisper.off_tui", "key" => self.key_label(Action::Settings));
            return self.dialog = Some(Dialog::Message { title, text });
        }
        let paths: Vec<PathBuf> = self.panel().targets();
        let (tx, rx) = mpsc::channel();
        let (cfg, rate) = (self.cfg.clone(), self.index.status().transcripts.and_then(|t| t.rate));
        std::thread::spawn(move || {
            let survey = coxswain_core::transcribe::survey(&paths);
            let text = coxswain_core::transcribe::plan_text(&cfg, &survey, rate);
            let _ = tx.send((survey.files, text));
        });
        self.status = Some(t!("common.loading"));
        self.transcribe_plan = Some(rx);
    }

    fn poll_transcribe_plan(&mut self) {
        let Some(rx) = &self.transcribe_plan else { return };
        let Ok((files, text)) = rx.try_recv() else { return };
        self.transcribe_plan = None;
        self.status = None;
        let title = t!("action.transcribe");
        self.dialog = Some(if files.is_empty() { Dialog::Message { title, text: t!("whisper.none") } } else { Dialog::Transcribe { title, text, files } });
    }

    /// The question worked out: the dialog, or what is missing with the key of Settings.
    fn poll_searchable_plan(&mut self) {
        let Some(rx) = &self.searchable_plan else { return };
        let Ok((pdfs, plan)) = rx.try_recv() else { return };
        self.searchable_plan = None;
        let title = t!("action.searchable_pdf");
        self.dialog = Some(match plan {
            Ok(text) => Dialog::Searchable { title, text, pdfs },
            Err(e) => Dialog::Message { title, text: format!("{e}\n\n{}", t!("searchable.settings_key", "key" => self.key_label(Action::Settings))) },
        });
    }

    fn searchable_start(&mut self, pdfs: Vec<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (cfg, flag, list) = (self.cfg.preview.clone(), stop.clone(), pdfs.clone());
        std::thread::spawn(move || {
            let said = tx.clone();
            let at = tx.clone();
            let done = coxswain_core::searchable::run_all(&cfg, &list, &flag, |i| drop(at.send(SearchableMsg::At(i))), |s| {
                let line = match s {
                    coxswain_core::searchable::Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(coxswain_core::searchable::TOOL).cloned().unwrap_or_default()))),
                    coxswain_core::searchable::Said::Line(l) => l.to_string(),
                };
                let _ = said.send(SearchableMsg::Said(line));
            });
            let _ = tx.send(SearchableMsg::Done(done));
        });
        let names = pdfs.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        self.searchable = Some(Searchable { names, at: 0, line: String::new(), shown: String::new(), started: Instant::now(), stop, rx });
        self.poll_searchable();
    }

    /// How far *Make PDF searchable* is, on the status line; when done, what came of it.
    fn poll_searchable(&mut self) {
        let key = self.key_label(Action::SearchablePdf).to_string();
        let Some(s) = &mut self.searchable else { return };
        let mut done = None;
        while let Ok(msg) = s.rx.try_recv() {
            match msg {
                SearchableMsg::At(i) => (s.at, s.line) = (i, String::new()),
                SearchableMsg::Said(l) => s.line = l,
                SearchableMsg::Done(d) => done = Some(d),
            }
        }
        let Some(done) = done else {
            let (name, n, all, seconds) = (s.names.get(s.at).cloned().unwrap_or_default(), s.at + 1, s.names.len(), s.started.elapsed().as_secs());
            let busy = if all > 1 {
                t!("searchable.busy_of", "name" => name, "n" => n, "all" => all, "seconds" => seconds)
            } else {
                t!("searchable.busy", "name" => name, "seconds" => seconds)
            };
            let stopping = s.stop.load(std::sync::atomic::Ordering::Relaxed);
            let tail = if stopping { t!("searchable.stopping") } else { t!("searchable.stop_key", "key" => key) };
            let line = if s.line.is_empty() { String::new() } else { format!(" · {}", s.line) };
            let new = format!("{busy}{line} · {tail}");
            if self.status.is_none() || self.status.as_ref() == Some(&s.shown) {
                self.status = Some(new.clone());
            }
            s.shown = new;
            return;
        };
        let s = self.searchable.take().expect("a run");
        // The image was used: what *Remove images not used for 90 days* goes by, which the
        // desktop app does; the installed program uses none.
        if let Ok(coxswain_core::searchable::How::Container { image, .. }) = coxswain_core::searchable::how(&self.cfg.preview) {
            let mut st = coxswain_core::state::AppState::load();
            coxswain_core::images::used(&mut st, &image, coxswain_core::fs::now());
            let _ = st.save();
        }
        let stopped = s.stop.load(std::sync::atomic::Ordering::Relaxed);
        let (made, failed): (Vec<_>, Vec<_>) = done.into_iter().zip(s.names.iter()).partition(|(r, _)| r.is_ok());
        let made: Vec<PathBuf> = made.into_iter().filter_map(|(r, _)| r.ok()).collect();
        self.reload();
        if stopped {
            return self.status = Some(t!("searchable.stopped"));
        }
        if !failed.is_empty() {
            let why = failed.into_iter().map(|(r, n)| format!("{n}: {}", r.err().unwrap_or_default())).collect::<Vec<_>>().join("\n");
            self.status = None;
            return self.dialog = Some(failure(t!("searchable.failed"), &why));
        }
        self.status = Some(match made.as_slice() {
            [one] => t!("searchable.done_one", "name" => one.file_name().unwrap_or_default().to_string_lossy()),
            many => t!("searchable.done_many", "n" => many.len()),
        });
    }

    /// *Convert video or audio* (ffmpeg), *Convert or edit pictures* (ImageMagick), *Convert
    /// geodata* (GDAL), *Export 3D model* (OpenSCAD) or *Export from KiCad* (kicad-cli) on the files among the marked entries (or the one under the cursor) it takes: what to make of them,
    /// from a menu. While one runs, the action stops it.
    fn media_menu(&mut self, program: coxswain_core::media::Program) {
        use coxswain_core::media::Op;
        if let Some((s, _)) = &self.media {
            s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = Some(t!("media.stopping"));
        }
        let takes = |p: &PathBuf| Op::menu(program).iter().any(|o| o.takes(p));
        // A file only in the cloud is left out: it is not downloaded to be read.
        let files: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| takes(p) && !coxswain_core::cloud::unasked(p) && p.is_file()).collect();
        if files.is_empty() {
            return self.status = Some(program.none());
        }
        let items = Op::menu(program)
            .into_iter()
            .enumerate()
            .map(|(i, op)| MenuItem { key: (i + 1).to_string(), label: op.label(), run: MenuRun::Media(op, files.clone()), group: None })
            .collect();
        let action = media_action(program);
        self.dialog = Some(Dialog::Menu { title: self.action_label(action), filter: String::new(), items, cursor: 0, direct: true, feature: media_feature(program) });
    }

    /// *Preview with…* on the file under the cursor: the choices for its kind of file
    /// (`preview_tools`), what each would use, the line that installs its program, and Settings
    /// at Preview tools (where its image is pulled).
    fn preview_with(&mut self) {
        use coxswain_core::preview_tools as pt;
        let Some(kind) = self.panel().current().filter(|e| !e.is_dir).and_then(|e| pt::for_path(&e.path)) else {
            return self.status = Some(t!("ptools.none_here"));
        };
        let Some(r) = pt::row_of(&self.cfg.preview, kind.id) else { return };
        let installed: Vec<String> = r.programs.iter().filter(|p| p.path.is_some()).map(|p| p.version.as_ref().map_or_else(|| p.name.to_string(), |v| format!("{} {v}", p.name))).collect();
        let image = r.images.iter().find(|i| !i.image.is_empty());
        let now = |c: &str| if r.choice == c { format!(" · {}", t!("ptools.current")) } else { String::new() };
        let default = t!("ptools.choice.default", "value" => settings::choice_label("preview_prefer", &self.cfg.preview.prefer));
        let local = format!("{}: {}", t!("ptools.choice.local"), if installed.is_empty() { t!("convert.not_installed", "program" => r.programs.first().map_or("", |p| p.name)) } else { installed.join(", ") });
        let container = match image {
            Some(i) => format!("{}: {} ({})", t!("ptools.choice.container"), coxswain_core::images::short(&i.image).rsplit('/').next().unwrap_or_default(), match (i.pulled, &i.download) {
                (Some(b), _) => t!("settings.image_pulled", "size" => coxswain_core::settings::human(b)),
                (None, Some(d)) => format!("{} ({d})", t!("settings.image_not_pulled")),
                (None, None) => t!("settings.image_not_pulled"),
            }),
            // No image: Container cannot run it, so it is no choice here.
            None => String::new(),
        };
        let mut items: Vec<MenuItem> = [("", default), ("local", local), ("container", container), ("off", t!("ptools.choice.off"))]
            .into_iter()
            .filter(|(_, label)| !label.is_empty())
            .enumerate()
            .map(|(i, (c, label))| MenuItem { key: (i + 1).to_string(), label: format!("{label}{}", now(c)), run: MenuRun::PreviewTool(kind.id, c), group: None })
            .collect();
        if let (true, Some(line)) = (installed.is_empty(), &r.install) {
            items.push(MenuItem { key: "c".into(), label: t!("ptools.menu_copy", "line" => line.as_str()), run: MenuRun::CopyLine(line.clone()), group: None });
        }
        items.push(MenuItem { key: "s".into(), label: t!("ptools.menu_settings"), run: MenuRun::SettingsAt("preview_tools"), group: None });
        let title = format!("{} {}", self.action_label(Action::PreviewWith), r.title);
        self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: true, feature: "preview_tools" });
    }

    /// *Long files: sample or everything…* on the folder under the cursor: *Everything here*,
    /// *Sample here* or *Like the folders above*, each with what it would cost
    /// (`long_files::choices`); the one taken is written into config.toml.
    fn long_files(&mut self) {
        let Some(dir) = self.panel().current().filter(|e| e.is_dir && e.name != "..").map(|e| e.path.clone()) else {
            return self.status = Some(t!("long.on_folder"));
        };
        let measured = self.index.status().meaning_ms_per_passage;
        let index = self.index.clone();
        let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
        let items = coxswain_core::long_files::choices(&self.cfg.search, &dir, measured, |r| index.long_files_cost(r))
            .into_iter()
            .enumerate()
            .map(|(i, c)| MenuItem { key: (i + 1).to_string(), label: c.cost.map_or_else(|| c.label.clone(), |w| format!("{}: {w}", c.label)), run: MenuRun::Save(c.changes, name.clone()), group: None })
            .collect();
        let title = format!("{} {name}", self.action_label(Action::LongFiles));
        self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: true, feature: "long_files" });
    }

    /// A choice of *Preview with…* written into config.toml and used at once.
    fn preview_tool(&mut self, kind: &str, choice: &str) {
        let Some(path) = Config::path() else { return self.status = Some(t!("err.no_config_folder")) };
        self.status = Some(match coxswain_core::preview_tools::set_choice(&path, kind, choice) {
            Ok(cfg) => {
                self.cfg = cfg;
                t!("settings.saved", "path" => path.display())
            }
            Err(e) => e,
        });
    }

    /// What to make was chosen: a clip and a size are asked for first, then the question.
    fn media_chose(&mut self, op: coxswain_core::media::Op, files: Vec<PathBuf>) {
        use coxswain_core::media::Op;
        let title = op.label();
        self.dialog = Some(match op {
            Op::Clip { .. } => Dialog::Input { title, label: t!("media.clip_label"), value: "0:00 0:30".into(), prompt: Prompt::Clip(files) },
            Op::Resize(n) => Dialog::Input { title, label: t!("media.resize_label"), value: n.to_string(), prompt: Prompt::Resize(files) },
            _ => return self.media_ask(op, files),
        });
    }

    /// The question, worked out on a thread (asking podman for the image takes a moment).
    fn media_ask(&mut self, op: coxswain_core::media::Op, files: Vec<PathBuf>) {
        use coxswain_core::media;
        // Only the files this choice takes (a bill of materials of a schematic, not a board).
        let files: Vec<PathBuf> = files.into_iter().filter(|p| op.takes(p)).collect();
        if files.is_empty() {
            return self.status = Some(op.program().none());
        }
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            let plan = media::how(&cfg, op.program()).map(|how| format!("{} {}", media::confirm(&op, &files), media::about(&how).0));
            let _ = tx.send((op, files, plan));
        });
        self.media_plan = Some(rx);
    }

    fn poll_media_plan(&mut self) {
        let Some(rx) = &self.media_plan else { return };
        let Ok((op, files, plan)) = rx.try_recv() else { return };
        self.media_plan = None;
        let title = op.label();
        self.dialog = Some(match plan {
            Ok(text) => Dialog::Media { title, text, op, files },
            Err(e) => Dialog::Message { title, text: format!("{e}

{}", t!("searchable.settings_key", "key" => self.key_label(Action::Settings))) },
        });
    }

    fn media_start(&mut self, op: coxswain_core::media::Op, files: Vec<PathBuf>) {
        use coxswain_core::media::{self, Said};
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let action = media_action(op.program());
        let (cfg, flag, list) = (self.cfg.preview.clone(), stop.clone(), files.clone());
        std::thread::spawn(move || {
            let (said, at) = (tx.clone(), tx.clone());
            let image = op.program().tool();
            let done = media::run_all(&cfg, &op, &list, &flag, |i| drop(at.send(SearchableMsg::At(i))), |s| {
                let line = match s {
                    Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(image).cloned().unwrap_or_default()))),
                    Said::Percent(p) => format!("{p} %"),
                    Said::Line(l) => l.to_string(),
                };
                let _ = said.send(SearchableMsg::Said(line));
            });
            let _ = tx.send(SearchableMsg::Done(done));
        });
        let names = files.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        self.media = Some((Searchable { names, at: 0, line: String::new(), shown: String::new(), started: Instant::now(), stop, rx }, action));
        self.poll_media();
    }

    /// How far a conversion is, on the status line; when done, what came of it.
    fn poll_media(&mut self) {
        let Some((_, action)) = &self.media else { return };
        let action = *action;
        // No key of its own by default: F9 lists it, and stops it from there.
        let key = match self.key_label(action) {
            "" => format!("{} → {}", self.key_label(Action::Menu), self.action_label(action)),
            k => k.to_string(),
        };
        let Some((s, _)) = &mut self.media else { return };
        let mut done = None;
        while let Ok(msg) = s.rx.try_recv() {
            match msg {
                SearchableMsg::At(i) => (s.at, s.line) = (i, String::new()),
                SearchableMsg::Said(l) => s.line = l,
                SearchableMsg::Done(d) => done = Some(d),
            }
        }
        let Some(done) = done else {
            let (name, n, all, seconds) = (s.names.get(s.at).cloned().unwrap_or_default(), s.at + 1, s.names.len(), s.started.elapsed().as_secs());
            let busy = if all > 1 {
                t!("media.busy_of", "name" => name, "n" => n, "all" => all, "seconds" => seconds)
            } else {
                t!("media.busy", "name" => name, "seconds" => seconds)
            };
            let stopping = s.stop.load(std::sync::atomic::Ordering::Relaxed);
            let tail = if stopping { t!("media.stopping") } else { t!("searchable.stop_key", "key" => key) };
            let line = if s.line.is_empty() { String::new() } else { format!(" · {}", s.line) };
            let new = format!("{busy}{line} · {tail}");
            if self.status.is_none() || self.status.as_ref() == Some(&s.shown) {
                self.status = Some(new.clone());
            }
            s.shown = new;
            return;
        };
        let (s, _) = self.media.take().expect("a run");
        let stopped = s.stop.load(std::sync::atomic::Ordering::Relaxed);
        let (made, failed): (Vec<_>, Vec<_>) = done.into_iter().zip(s.names.iter()).partition(|(r, _)| r.is_ok());
        let made: Vec<PathBuf> = made.into_iter().filter_map(|(r, _)| r.ok()).collect();
        self.reload();
        if stopped {
            return self.status = Some(t!("media.stopped"));
        }
        if !failed.is_empty() {
            let why = failed.into_iter().map(|(r, n)| format!("{n}: {}", r.err().unwrap_or_default())).collect::<Vec<_>>().join("\n");
            self.status = None;
            return self.dialog = Some(failure(t!("media.failed"), &why));
        }
        self.status = Some(match made.as_slice() {
            [one] => t!("media.done_one", "name" => one.file_name().unwrap_or_default().to_string_lossy()),
            many => t!("media.done_many", "n" => many.len()),
        });
    }

    /// Make checksums: the algorithm from a menu, for the marked entries (or the one under the
    /// cursor). Its key again while checksums are made or checked stops them.
    fn sums_menu(&mut self) {
        use coxswain_core::checksum::Algo;
        if let Some(w) = &self.work {
            w.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = Some(t!("media.stopping"));
        }
        if self.panel().subject().place != coxswain_core::menu::Place::Disk {
            return self.status = Some(t!("sums.not_here"));
        }
        let files = self.panel().targets();
        if files.is_empty() {
            return self.status = Some(t!("sums.nothing"));
        }
        let dir = self.panel().dir.clone();
        let items = Algo::ALL
            .into_iter()
            .enumerate()
            .map(|(i, a)| MenuItem { key: (i + 1).to_string(), label: t!("sums.algo", "algo" => a.name(), "file" => coxswain_core::checksum::target(&dir, a).file_name().unwrap_or_default().to_string_lossy()), run: MenuRun::Sums(a, files.clone()), group: None })
            .collect();
        self.dialog = Some(Dialog::Menu { title: self.action_label(Action::MakeChecksums), filter: String::new(), items, cursor: 0, direct: true, feature: "checksums" });
    }

    fn work_start(&mut self, action: Action, run: impl FnOnce(&std::sync::atomic::AtomicBool, &dyn Fn(&coxswain_core::checksum::Progress)) -> WorkMsg + Send + 'static) {
        if self.work.is_some() {
            return self.status = Some(t!("sums.busy_already"));
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let at = tx.clone();
            // A message every megabyte would flood the loop: one a tenth of a second.
            let last = std::sync::Mutex::new(Instant::now() - Duration::from_secs(1));
            let msg = run(&flag, &|p| {
                let mut l = last.lock().unwrap();
                if l.elapsed() >= Duration::from_millis(100) {
                    *l = Instant::now();
                    let _ = at.send(WorkMsg::At(p.clone()));
                }
            });
            let _ = tx.send(msg);
        });
        self.work = Some(Work { action, progress: Default::default(), shown: String::new(), stop, rx });
        self.poll_work();
    }

    /// Split the file under the cursor: the size of the parts from a menu, or typed.
    fn split_menu(&mut self) {
        if let Some(w) = &self.work {
            w.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = Some(t!("media.stopping"));
        }
        let Some(e) = self.panel().current().filter(|e| !e.is_dir && !e.is_parent()).cloned() else {
            return self.status = Some(t!("split.no_file"));
        };
        if self.panel().subject().place != coxswain_core::menu::Place::Disk {
            return self.status = Some(t!("sums.not_here"));
        }
        let mut items: Vec<MenuItem> = coxswain_core::split::PRESETS
            .iter()
            .enumerate()
            .map(|(i, (size, id))| MenuItem { key: (i + 1).to_string(), label: t!(&format!("split.size.{id}")), run: MenuRun::Split(e.path.clone(), Some(*size)), group: None })
            .collect();
        items.push(MenuItem { key: (items.len() + 1).to_string(), label: t!("split.size.other"), run: MenuRun::Split(e.path.clone(), None), group: None });
        let title = format!("{} {}", self.action_label(Action::SplitFile), e.name);
        self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: true, feature: "split_and_join" });
    }

    fn split_start(&mut self, file: PathBuf, size: u64) {
        self.work_start(Action::SplitFile, move |stop, at| WorkMsg::Split(coxswain_core::split::split(&file, size, stop, at)));
    }

    /// Join the parts from `first` on: asked first, and again before a file of that name is
    /// replaced.
    fn join_menu(&mut self, first: PathBuf) {
        let Some(target) = coxswain_core::split::joins_into(&first) else { return };
        let n = coxswain_core::split::parts_of(&first).len();
        let name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let there = std::fs::symlink_metadata(&target).is_ok();
        let label = if there { t!("split.replace", "name" => &name) } else { tn!("split.join_into", n, "name" => &name) };
        let items = vec![MenuItem { key: "1".into(), label, run: MenuRun::Join(first, there), group: None }];
        self.dialog = Some(Dialog::Menu { title: self.action_label(Action::JoinParts), filter: String::new(), items, cursor: 0, direct: true, feature: "split_and_join" });
    }

    fn join_start(&mut self, first: PathBuf, replace: bool) {
        self.work_start(Action::JoinParts, move |stop, at| WorkMsg::Joined(coxswain_core::split::join(&first, replace, stop, at)));
    }

    fn sums_make(&mut self, algo: coxswain_core::checksum::Algo, files: Vec<PathBuf>) {
        let dir = self.panel().dir.clone();
        self.work_start(Action::MakeChecksums, move |stop, at| WorkMsg::Made(coxswain_core::checksum::make(&dir, &files, algo, stop, at)));
    }

    fn sums_verify(&mut self, list: PathBuf) {
        self.work_start(Action::VerifyChecksums, move |stop, at| {
            let r = coxswain_core::checksum::verify(&list, stop, at);
            WorkMsg::Checked(list, r)
        });
    }

    /// How far checksums are, on the status line; when done, what came of them.
    fn poll_work(&mut self) {
        use coxswain_core::checksum::{self, State};
        let Some(w) = &self.work else { return };
        let key = match self.key_label(w.action) {
            "" => format!("{} → {}", self.key_label(Action::Menu), self.action_label(w.action)),
            k => k.to_string(),
        };
        let what = self.action_label(w.action);
        let Some(w) = &mut self.work else { return };
        let mut done = None;
        while let Ok(msg) = w.rx.try_recv() {
            match msg {
                WorkMsg::At(p) => w.progress = p,
                m => done = Some(m),
            }
        }
        let Some(done) = done else {
            let stopping = w.stop.load(std::sync::atomic::Ordering::Relaxed);
            let tail = if stopping { t!("media.stopping") } else { t!("searchable.stop_key", "key" => key) };
            let new = format!("{} · {tail}", w.progress.line(&what));
            if self.status.is_none() || self.status.as_ref() == Some(&w.shown) {
                self.status = Some(new.clone());
            }
            w.shown = new;
            return;
        };
        self.work = None;
        match done {
            WorkMsg::Made(Ok(m)) => {
                self.reload();
                self.status = Some(tn!("sums.made", m.files, "name" => m.path.file_name().unwrap_or_default().to_string_lossy()));
            }
            WorkMsg::Made(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => self.status = Some(t!("sums.stopped")),
            WorkMsg::Split(Ok(s)) => {
                self.reload();
                let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.status = Some(tn!("split.done", s.parts.len(), "sums" => name(&s.sums)));
            }
            WorkMsg::Joined(Ok(j)) => {
                self.reload();
                let name = j.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                match j.matches {
                    Some(false) => {
                        self.status = None;
                        self.dialog = Some(Dialog::Message { title: self.action_label(Action::JoinParts), text: t!("split.mismatch", "name" => name) });
                    }
                    Some(true) => self.status = Some(t!("split.joined_ok", "name" => name)),
                    None => self.status = Some(t!("split.joined", "name" => name)),
                }
            }
            WorkMsg::Split(Err(e)) | WorkMsg::Joined(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => self.status = Some(t!("split.stopped")),
            WorkMsg::Split(Err(e)) => {
                self.status = None;
                self.dialog = Some(failure(t!("split.split_failed"), &e.to_string()));
            }
            WorkMsg::Joined(Err(e)) => {
                self.status = None;
                self.dialog = Some(failure(t!("split.join_failed"), &e.to_string()));
            }
            WorkMsg::Made(Err(e)) => {
                self.status = None;
                self.dialog = Some(failure(t!("sums.make_failed"), &e.to_string()));
            }
            WorkMsg::Checked(_, Err(e)) => {
                self.status = None;
                self.dialog = Some(failure(t!("sums.verify_failed"), &e.to_string()));
            }
            WorkMsg::Checked(list, Ok((all, stopped))) => {
                // The entries of the list's folder that hold a changed or unreadable file are
                // marked, in each panel that shows that folder.
                let dir = list.parent().unwrap_or(Path::new("")).to_path_buf();
                let bad: HashSet<PathBuf> = all
                    .iter()
                    .filter(|c| matches!(c.state, State::Changed | State::Unreadable(_)))
                    .filter_map(|c| c.path.strip_prefix(&dir).ok()?.components().next().map(|f| dir.join(f)))
                    .collect();
                for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                    p.clear_marks();
                    for i in 0..p.entries.len() {
                        if bad.contains(&p.entries[i].path) {
                            p.toggle_mark(i);
                        }
                    }
                }
                const SHOWN: usize = 30;
                let problems: Vec<String> = all.iter().filter(|c| c.state != State::Ok).map(|c| format!("{}: {}", c.name, checksum::word(&c.state))).collect();
                let mut text = checksum::summary(&all, stopped);
                if !bad.is_empty() {
                    text = format!("{text}\n{}", t!("sums.marked"));
                }
                if !problems.is_empty() {
                    text.push_str("\n\n");
                    text.push_str(&problems.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("\n"));
                    if problems.len() > SHOWN {
                        text = format!("{text}\n{}", tn!("sums.more", problems.len() - SHOWN));
                    }
                }
                self.status = None;
                let name = list.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.dialog = Some(Dialog::Message { title: t!("sums.verify_title", "name" => name), text });
            }
            WorkMsg::At(_) => {}
        }
    }

    /// *Build PDF of AsciiDoc* (`adoc`) or *Export draw.io diagram* on the files among the
    /// marked entries (or the one under the cursor) it takes; a diagram's format is picked from
    /// a menu first. While one runs, either action stops it.
    fn export_menu(&mut self, adoc: bool) {
        use coxswain_core::export::{self, Op};
        if let Some(x) = &self.export {
            x.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            return self.status = Some(t!("export.stopping"));
        }
        let takes = |p: &PathBuf| if adoc { export::is_adoc(p) } else { export::is_drawio(p) };
        // A file only in the cloud is left out: it is not downloaded to be read.
        let files: Vec<PathBuf> = self.panel().targets().into_iter().filter(|p| takes(p) && !coxswain_core::cloud::unasked(p) && p.is_file()).collect();
        if files.is_empty() {
            return self.status = Some(t!(if adoc { "export.none_adoc" } else { "export.none_drawio" }));
        }
        if adoc {
            return self.export_ask(Op::AdocPdf, files);
        }
        let items = Op::DRAWIO
            .into_iter()
            .enumerate()
            .map(|(i, op)| MenuItem { key: (i + 1).to_string(), label: op.label(), run: MenuRun::Export(op, files.clone()), group: None })
            .collect();
        self.dialog = Some(Dialog::Menu { title: self.action_label(Action::DrawioExport), filter: String::new(), items, cursor: 0, direct: true, feature: "drawio_export" });
    }

    /// The question, worked out on a thread (asking podman for the image takes a moment).
    fn export_ask(&mut self, op: coxswain_core::export::Op, files: Vec<PathBuf>) {
        use coxswain_core::export;
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.preview.clone();
        std::thread::spawn(move || {
            let plan = export::how(&cfg, op.tool()).map(|how| format!("{} {}", export::confirm(op, &files), export::about(&how).0));
            let _ = tx.send((op, files, plan));
        });
        self.export_plan = Some(rx);
    }

    fn poll_export_plan(&mut self) {
        let Some(rx) = &self.export_plan else { return };
        let Ok((op, files, plan)) = rx.try_recv() else { return };
        self.export_plan = None;
        let title = self.action_label(export_action(op));
        self.dialog = Some(match plan {
            Ok(text) => Dialog::Export { title, text, op, files },
            Err(e) => Dialog::Message { title, text: format!("{e}\n\n{}", t!("searchable.settings_key", "key" => self.key_label(Action::Settings))) },
        });
    }

    fn export_start(&mut self, op: coxswain_core::export::Op, files: Vec<PathBuf>) {
        use coxswain_core::export::{self, Said};
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (cfg, flag, list) = (self.cfg.preview.clone(), stop.clone(), files.clone());
        std::thread::spawn(move || {
            let (said, at) = (tx.clone(), tx.clone());
            let done = export::run_all(&cfg, op, &list, &flag, |i| drop(at.send(ExportMsg::At(i))), |s| {
                let line = match s {
                    Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(op.tool()).cloned().unwrap_or_default()))),
                    Said::Line(l) => l.to_string(),
                };
                let _ = said.send(ExportMsg::Said(line));
            });
            let _ = tx.send(ExportMsg::Done(done));
        });
        let names = files.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        self.export = Some(Exporting { op, names, at: 0, line: String::new(), shown: String::new(), started: Instant::now(), stop, rx });
        self.poll_export();
    }

    /// How far an export is, on the status line; when done, what came of it.
    fn poll_export(&mut self) {
        let Some(x) = &self.export else { return };
        let action = export_action(x.op);
        // No key of its own by default: the action menu and F9 list it, and stop it from there.
        let key = match self.key_label(action) {
            "" => format!("{} → {}", self.key_label(Action::Menu), self.action_label(action)),
            k => k.to_string(),
        };
        let Some(x) = &mut self.export else { return };
        let mut done = None;
        while let Ok(msg) = x.rx.try_recv() {
            match msg {
                ExportMsg::At(i) => (x.at, x.line) = (i, String::new()),
                ExportMsg::Said(l) => x.line = l,
                ExportMsg::Done(d) => done = Some(d),
            }
        }
        let Some(done) = done else {
            let (name, n, all, seconds) = (x.names.get(x.at).cloned().unwrap_or_default(), x.at + 1, x.names.len(), x.started.elapsed().as_secs());
            let busy = if all > 1 {
                t!("export.busy_of", "name" => name, "n" => n, "all" => all, "seconds" => seconds)
            } else {
                t!("export.busy", "name" => name, "seconds" => seconds)
            };
            let stopping = x.stop.load(std::sync::atomic::Ordering::Relaxed);
            let tail = if stopping { t!("export.stopping") } else { t!("searchable.stop_key", "key" => key) };
            let line = if x.line.is_empty() { String::new() } else { format!(" · {}", x.line) };
            let new = format!("{busy}{line} · {tail}");
            if self.status.is_none() || self.status.as_ref() == Some(&x.shown) {
                self.status = Some(new.clone());
            }
            x.shown = new;
            return;
        };
        let x = self.export.take().expect("a run");
        // The image was used: what *Remove images not used for 90 days* goes by.
        if let Ok(coxswain_core::export::How::Container { image, .. }) = coxswain_core::export::how(&self.cfg.preview, x.op.tool()) {
            let mut st = coxswain_core::state::AppState::load();
            coxswain_core::images::used(&mut st, &image, coxswain_core::fs::now());
            let _ = st.save();
        }
        let stopped = x.stop.load(std::sync::atomic::Ordering::Relaxed);
        let (made, failed): (Vec<_>, Vec<_>) = done.into_iter().zip(x.names.iter()).partition(|(r, _)| r.is_ok());
        let made: Vec<PathBuf> = made.into_iter().filter_map(|(r, _)| r.ok()).flatten().collect();
        self.reload();
        if stopped {
            return self.status = Some(t!("export.stopped"));
        }
        if !failed.is_empty() {
            let why = failed.into_iter().map(|(r, n)| format!("{n}: {}", r.err().unwrap_or_default())).collect::<Vec<_>>().join("\n");
            self.status = None;
            return self.dialog = Some(failure(t!("export.failed"), &why));
        }
        self.status = Some(match made.as_slice() {
            [one] => t!("export.done_one", "name" => one.file_name().unwrap_or_default().to_string_lossy()),
            many => t!("export.done_many", "n" => many.len()),
        });
    }

    /// Space on the folder under the cursor, else the panel's: walked now, full screen.
    fn space_view(&mut self) {
        let p = self.panel();
        if coxswain_core::archive::split(&p.dir).is_some() || history::split(&p.dir).is_some() || history::is_history(&p.dir) {
            return self.status = Some(t!("space.not_here"));
        }
        let dir = p.current().filter(|e| e.is_dir && !e.is_parent() && p.marked.is_empty()).map_or_else(|| p.dir.clone(), |e| e.path.clone());
        let labels = [Action::View, Action::Edit, Action::Delete, Action::Refresh].map(|a| self.key_label(a).to_string());
        self.dialog = Some(Dialog::Space(Box::new(space::Viewer::new(dir, labels))));
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

    /// `file` in the editor at `line`: config.toml at `[keys]` (Settings) or `[[user_menu]]` (F2).
    pub fn edit_at(&mut self, file: &Path, line: usize) {
        let Some(editor) = self.editor() else { return self.edit_here(file, Some(line), None) };
        if let Some(prog) = editor_missing(&editor) {
            return self.status = Some(t!("edit.not_found", "name" => prog));
        }
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        self.run = Some(Run::Shell { line: config::edit_command(&editor, file, line), typed: false, dir, wait: false });
    }

    /// View, edit (in the editor set, else the built-in one) or edit here (the built-in one).
    fn view_or_edit(&mut self, a: Action, file: &Path) {
        if a == Action::View && self.cfg.viewer.is_none() {
            return self.view_in(file, None);
        }
        let prog = match a {
            Action::View => self.cfg.viewer.clone().unwrap_or_else(pager),
            _ => match self.editor().filter(|_| a == Action::Edit) {
                Some(editor) => editor,
                None => return self.edit_here(file, None, None),
            },
        };
        // Said in the status line: the shell's own "not found" would flash by unread.
        if a == Action::Edit && let Some(prog) = editor_missing(&prog) {
            return self.status = Some(t!("edit.not_found", "name" => prog));
        }
        self.run_on(&prog, file);
    }

    /// Shift+F2: the two marked files compared, else the file under the cursor with the one under
    /// the other panel's; the left panel's on the left.
    fn compare_files(&mut self) {
        let p = self.panel();
        let marked: Vec<PathBuf> = p.entries.iter().filter(|e| !e.is_dir && p.marked.contains(&e.path)).map(|e| e.path.clone()).collect();
        let pair = if marked.len() == 2 {
            Some((marked[0].clone(), marked[1].clone()))
        } else {
            let file = |i: usize| self.panels[i].current().filter(|e| !e.is_dir && !e.is_parent()).map(|e| e.path.clone());
            file(0).zip(file(1))
        };
        let Some((a, b)) = pair else { return self.status = Some(t!("compare.pick_two")) };
        // Only files on the disk: a file inside an archive or a history is copied out first.
        if [&a, &b].iter().any(|f| f.parent().is_some_and(|d| coxswain_core::archive::split(d).is_some()) || history::is_history(f)) {
            return self.status = Some(t!("compare.not_here"));
        }
        self.dialog = Some(Dialog::Compare(Box::new(compare::Viewer::open(&a, &b))));
    }

    /// Ctrl+F2: the left panel's folder compared with the right one's; while a compare is on,
    /// its menu.
    fn compare_folders(&mut self) {
        use coxswain_core::compare::folders::Mode;
        let Some(c) = self.panels[0].cmp.clone() else { return self.folders_start(Mode::Quick) };
        let item = |key: &str, label: String, run| MenuItem { key: key.into(), label, run: MenuRun::Folders(run), group: None };
        let other = if c.f.mode == Mode::Quick { Mode::Content } else { Mode::Quick };
        let items = vec![
            item("1", t!("compare.mark_differences"), FoldersRun::Mark),
            item("2", t!(if c.diff_only { "compare.show_all" } else { "compare.differences_only" }), FoldersRun::DiffOnly),
            item("3", t!(if other == Mode::Content { "compare.by_content" } else { "compare.by_size_date" }), FoldersRun::Again(other)),
            item("4", t!("compare.stop_comparing"), FoldersRun::Stop),
            item("5", t!("compare.make_right"), FoldersRun::Make(1)),
            item("6", t!("compare.make_left"), FoldersRun::Make(0)),
        ];
        self.dialog = Some(Dialog::Menu { title: self.action_label(Action::CompareFolders), filter: String::new(), items, cursor: 0, direct: true, feature: "compare_folders" });
    }

    fn folders_start(&mut self, mode: coxswain_core::compare::folders::Mode) {
        let f = Arc::new(coxswain_core::compare::folders::Folders::open(&self.panels[0].dir, &self.panels[1].dir, mode));
        let diff_only = self.panels[0].cmp.as_ref().is_some_and(|c| c.diff_only);
        for side in 0..2 {
            self.panels[side].cmp = Some(Cmp { f: f.clone(), side, diff_only });
        }
        self.folders_said = false;
        self.folders_lead = None;
        self.status = Some(t!("compare.comparing", "n" => 0));
    }

    fn folders_run(&mut self, r: FoldersRun) {
        let Some(c) = self.panels[0].cmp.clone() else { return };
        match r {
            FoldersRun::Mark => {
                let mut n = 0;
                for side in 0..2 {
                    let p = &mut self.panels[side];
                    let ahead: Vec<usize> = (0..p.entries.len()).filter(|&i| !p.entries[i].is_parent() && c.f.state(side, &p.entries[i].path).is_some_and(|s| s.ahead(side))).collect();
                    for i in ahead {
                        if !p.marked.contains(&p.entries[i].path) {
                            p.toggle_mark(i);
                            n += 1;
                        }
                    }
                }
                self.status = Some(tn!("compare.marked", n));
            }
            FoldersRun::DiffOnly => {
                for p in &mut self.panels {
                    if let Some(c) = &mut p.cmp {
                        c.diff_only = !c.diff_only;
                    }
                }
                self.reload();
                // The summary stays on the status line: the key that chose this took it away.
                let found = c.f.found();
                if found.done {
                    self.status = Some(found.error.clone().unwrap_or_else(|| found.summary()));
                }
            }
            FoldersRun::Again(mode) => self.folders_start(mode),
            FoldersRun::Make(to) => {
                let (from, dest) = (c.f.roots[1 - to].clone(), c.f.roots[to].clone());
                if let Err(e) = coxswain_core::compare::sync::check(&from, &dest) {
                    return self.status = Some(e);
                }
                let undo = self.key_label(Action::Undo);
                self.dialog = Some(Dialog::Sync(Box::new(sync::Viewer::new(from, dest, c.f.mode, undo.to_string()))));
            }
            FoldersRun::Stop => {
                c.f.stop();
                let all = c.diff_only;
                for p in &mut self.panels {
                    p.cmp = None;
                }
                if all {
                    self.reload();
                }
                self.status = Some(t!("compare.stopped_comparing"));
            }
        }
    }

    /// How far Compare folders is.
    fn poll_folders(&mut self) {
        let Some(c) = self.panels[0].cmp.clone() else { return };
        if self.folders_said {
            return;
        }
        let found = c.f.found();
        let lead = |s: String| match &self.folders_lead {
            Some(l) => format!("{l} · {s}"),
            None => s,
        };
        if found.done {
            let line = lead(found.error.clone().unwrap_or_else(|| found.summary()));
            drop(found);
            self.folders_said = true;
            self.status = Some(line);
            if c.diff_only {
                self.reload();
            }
        } else {
            drop(found);
            self.status = Some(lead(t!("compare.comparing", "n" => coxswain_core::settings::count(c.f.seen.load(std::sync::atomic::Ordering::Relaxed) as usize))));
        }
    }

    /// `file` in the built-in editor, at `line` (from 1); `back` is shown again when it closes.
    fn edit_here(&mut self, file: &Path, line: Option<usize>, back: Option<Box<Dialog>>) {
        match editor::Editor::open(file) {
            Ok(e) => {
                let mut e = e.git_keys(self.key_label(Action::Blame), self.key_label(Action::LineHistory));
                if let Some(n) = line {
                    e.buf.go_to(n);
                }
                self.dialog = Some(Dialog::Edit { e: Box::new(e), back });
            }
            Err(err) => {
                self.status = Some(format!("{}: {err}", file.display()));
                self.dialog = back.map(|b| *b);
            }
        }
    }

    /// F3 on `of`, shown as `copy`, the text made of it: in the built-in viewer titled with
    /// `of`'s name and `kind`, its format (never "plain text").
    fn view_copy(&mut self, copy: &Path, of: &Path, kind: String) {
        if self.cfg.viewer.is_some() {
            return self.view_or_edit(Action::View, copy);
        }
        let labels = [self.key_label(Action::Edit), self.key_label(Action::View)];
        match viewer::Viewer::open(copy, labels) {
            Ok(v) => self.dialog = Some(Dialog::View { v: Box::new(v.made_of(of, kind)), back: None }),
            Err(e) => self.status = Some(format!("{}: {e}", copy.display())),
        }
    }

    /// `file` in the built-in viewer; `back` is shown again when it closes.
    fn view_in(&mut self, file: &Path, back: Option<Box<Dialog>>) {
        let labels = [self.key_label(Action::Edit), self.key_label(Action::View)];
        match viewer::Viewer::open(file, labels) {
            Ok(v) => {
                // What the log keys do is not guessed: said the first few times a log is viewed.
                let v = if v.is_log() && coxswain_core::menu::once("viewer_log", &self.cfg) { v.saying(t!("viewer.log_hint")) } else { v };
                let v = v.git_keys(self.key_label(Action::Blame), self.key_label(Action::LineHistory));
                self.dialog = Some(Dialog::View { v: Box::new(v), back });
            }
            Err(e) => {
                self.status = Some(format!("{}: {e}", file.display()));
                self.dialog = back.map(|b| *b);
            }
        }
    }

    /// A viewer's source (F3 in the BOM or provenance viewer): in the viewer, `back` shown
    /// again after it.
    fn source(&mut self, file: &Path, back: Dialog) {
        match self.cfg.viewer.clone() {
            Some(prog) => {
                self.run_on(&prog, file);
                self.dialog = Some(back);
            }
            None => self.view_in(file, Some(Box::new(back))),
        }
    }

    /// `prog` run on `file`, in the file's folder, with the panels hidden meanwhile.
    fn run_on(&mut self, prog: &str, file: &Path) {
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut line = Line::default();
        line.cmd = format!("{prog} {}", line.word(&file.to_string_lossy()));
        self.run = Some(Run::Shell { line, typed: false, dir, wait: false });
    }

    /// The editor F4 starts: `editor`, else `$VISUAL`, else `$EDITOR`; `None` for the built-in one.
    fn editor(&self) -> Option<String> {
        let env = |v: &str| std::env::var(v).ok().filter(|s| !s.is_empty());
        self.cfg.editor.clone().or_else(|| env("VISUAL")).or_else(|| env("EDITOR"))
    }

    /// F2's last entry: config.toml in the editor at `[[user_menu]]`, an example there first.
    fn add_user(&mut self) {
        match coxswain_core::user_menu::prepare() {
            Ok((path, line)) => self.edit_at(&path, line),
            Err(e) => self.status = Some(e),
        }
    }

    fn run_cmdline(&mut self) {
        let cmd = std::mem::take(&mut self.cmdline);
        let t = cmd.trim();
        // `cd` has to change our own directory, so it is built in.
        if t == "cd" || t.starts_with("cd ") {
            let arg = t[2..].trim().trim_matches(['"', '\'']);
            let target = if arg.is_empty() { coxswain_core::dirs::home_dir().unwrap_or_default() } else { resolve(&self.panel().dir, arg) };
            if target.is_dir() {
                self.cd(self.active, coxswain_core::fs::canonical(&target).unwrap_or(target));
            } else {
                self.status = Some(t!("status.no_dir", "dir" => arg));
            }
            return;
        }
        self.run = Some(Run::Shell { line: Line::from(cmd), typed: true, dir: self.panel().dir.clone(), wait: false });
    }

    /// Delete `paths` (in the panel's folder, or Space's file), asking first as the settings say.
    fn ask_delete(&mut self, paths: Vec<PathBuf>, forever: bool) {
        if paths.is_empty() {
            return;
        }
        let inside = coxswain_core::archive::split(&self.panel().dir);
        let plan = bfs::delete_plan(&paths, forever, inside.is_some());
        if self.cfg.confirm_delete || plan.must_confirm {
            let archive = inside.map(|(a, _)| a.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_default();
            let text = t!(plan.question, "what" => Self::describe(&paths), "archive" => archive);
            self.dialog = Some(Dialog::Confirm { title: t!("dialog.delete"), text, paths, forever: plan.forever });
        } else {
            self.delete(paths, plan.forever);
        }
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
            return self.status = Some(j.shown());
        }
        // Measured in `finish`: while it runs, a folder's size would be halfway.
        self.forget(&job_op(op).touches(&src, &dst));
        let (tx, rx) = mpsc::channel();
        let (items, to, pw) = (src.clone(), dst.clone(), password.clone());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let r = coxswain_core::undo::run(job_op(op), &items, &to, pw.as_deref(), &flag, |i| drop(tx.send(JobMsg::At(i))));
            let _ = tx.send(JobMsg::Done(r.failed, r.rec, r.stopped));
        });
        let line = t!("status.busy", "what" => Self::describe(&src));
        let select = select.map(|n| (self.active, n));
        let job = Job { op, src, dst, password, select, line, rx, stop: Some(stop) };
        self.status = Some(job.shown());
        self.job = Some(job);
    }

    /// How far the job is; when it is done, what came of it.
    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let (mut done, mut undone, mut gone) = (None, None, false);
        loop {
            let msg = match job.rx.try_recv() {
                Ok(msg) => msg,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            };
            match msg {
                JobMsg::At(i) if job.src.len() > 1 => {
                    let what = Self::describe(std::slice::from_ref(&job.src[i]));
                    let was = job.shown();
                    job.line = t!("status.busy_of", "what" => what, "n" => i + 1, "all" => job.src.len());
                    if self.status.as_ref() == Some(&was) || self.status.is_none() {
                        self.status = Some(job.shown());
                    }
                }
                JobMsg::At(_) => {}
                JobMsg::Done(failed, rec, stopped) => done = Some((failed, Some(rec), stopped)),
                JobMsg::Undone(u) => undone = Some(u),
            }
        }
        // Its thread ended without saying it was done (it panicked): the job goes, so the app
        // can quit, and the user is told. An undo's record is lost then.
        if gone && done.is_none() && undone.is_none() {
            let what = if job.src.is_empty() { job.line.clone() } else { Self::describe(&job.src) };
            self.job = None;
            self.status = None;
            return self.after_op(String::new(), t!("error.job_stopped", "what" => what), vec![t!("error.job_stopped_detail")]);
        }
        if let Some(u) = undone {
            self.job = None;
            self.status = None;
            // Measured while it ran, a folder's size would be halfway.
            self.changed(&u.touched);
            self.after_op(t!("undo.done", "what" => &u.label), t!("undo.partly", "what" => &u.label), u.refused);
            // The cursor goes on what came back, where its folder is shown.
            for side in 0..2 {
                if let Some(name) = u.back.iter().find(|p| p.parent() == Some(self.panels[side].dir.as_path())).and_then(|p| p.file_name()) {
                    self.panels[side].select_name(&name.to_string_lossy());
                }
            }
            return;
        }
        // A key clears the status line; while the job runs, it comes back.
        if self.status.is_none() {
            self.status = Some(job.shown());
        }
        let Some((failed, rec, stopped)) = done else { return };
        let job = self.job.take().expect("a job");
        self.status = None;
        self.finish(job, failed, rec, stopped);
    }

    /// When only locked archives were in the way, ask for the password and run again with it,
    /// for just what was locked.
    /// `stopped`: Esc stopped it, and the line that says how far it got.
    fn finish(&mut self, job: Job, failed: Vec<(PathBuf, std::io::Error)>, rec: Option<Record>, stopped: Option<String>) {
        let Job { op, src, dst, password, select, .. } = job;
        // Again now it has ended: a folder measured while it ran would keep a size halfway.
        self.changed(&job_op(op).touches(&src, &dst));
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
            let fail = coxswain_core::undo::failure_title(job_op(None), &what, src.len(), failed.len());
            let ok = stopped.unwrap_or_else(|| t!("archive.packed", "what" => what));
            return self.after_op(ok + &hint, fail, errors);
        };
        if stopped.is_none() && !failed.is_empty() && failed.iter().all(|(_, e)| e.to_string().contains(coxswain_core::archive::LOCKED)) {
            let label = t!(if password.is_none() { "archive.locked_label" } else { "archive.locked_again" });
            let again = failed.into_iter().map(|(p, _)| p).collect();
            // Nothing is being worked on while the password is asked for (and after Esc).
            self.status = None;
            return self.input(&t!("archive.locked_title"), label, String::new(), Prompt::Password(op, again, dst));
        }
        let errors = failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect();
        let ok = match op {
            _ if stopped.is_some() => stopped.unwrap_or_default(),
            Transfer::Copy => t!("status.copied", "what" => what),
            Transfer::Move => t!("status.moved", "what" => what),
            Transfer::Rename => t!("status.renamed", "what" => what),
            Transfer::Extract => t!("app.extracted", "what" => what),
            Transfer::Delete(true) => t!("status.deleted", "what" => what),
            Transfer::Delete(false) => t!("status.trashed", "what" => what),
            Transfer::Mkdir => t!("status.created", "what" => src.first().map(|p| p.display().to_string()).unwrap_or_default()),
        };
        // How many of them went through, and how many did not.
        let fail = match op {
            Transfer::Rename => t!("error.rename", "what" => what),
            Transfer::Mkdir => t!("error.create", "what" => src.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            _ => coxswain_core::undo::failure_title(job_op(Some(op)), &what, src.len(), failed.len()),
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
            return self.status = Some(j.shown());
        }
        let Some(rec) = self.undo.pop() else { return self.status = Some(t!("undo.nothing")) };
        self.forget(&rec.touched());
        let line = t!("status.busy", "what" => rec.label());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let u = rec.undo(|i| drop(tx.send(JobMsg::At(i))));
            let _ = tx.send(JobMsg::Undone(u));
        });
        self.status = Some(line.clone());
        self.job = Some(Job { op: None, src: vec![], dst: PathBuf::new(), password: None, select: None, line, rx, stop: None });
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
                self.asked_again();
            }
            Prompt::Unlock(side, dir) => {
                if let Some((archive, _)) = coxswain_core::archive::split(&dir) {
                    coxswain_core::archive::remember(&archive, &value);
                }
                self.cd(side, dir);
                self.asked_again();
            }
            Prompt::Rename(_) if value.trim().is_empty() => {}
            Prompt::Rename(src) => {
                // The text as it was shown: nothing to rename.
                let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let Some(value) = coxswain_core::rename::typed(&name, &value) else { return };
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
                let mut st = coxswain_core::state::AppState::load();
                if coxswain_core::archive::remember_format(&mut st, &value) {
                    let _ = st.save();
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
                let d = coxswain_core::fs::canonical(&d).unwrap_or(d);
                if d.is_dir() {
                    self.cd(side, d);
                } else {
                    self.status = Some(t!("status.not_dir", "dir" => d.display()));
                }
            }
            Prompt::Clip(files) => match coxswain_core::media::clip(&value) {
                Some(op) => self.media_ask(op, files),
                None => self.status = Some(t!("media.clip_bad")),
            },
            Prompt::SplitSize(file) => match coxswain_core::split::parse_size(&value) {
                Some(size) => self.split_start(file, size),
                None => self.status = Some(t!("split.size_bad")),
            },
            Prompt::Resize(files) => match coxswain_core::media::Op::parse(&format!("resize:{}", value.trim())) {
                Some(op) => self.media_ask(op, files),
                None => self.status = Some(t!("media.resize_bad")),
            },
            Prompt::Flags(path) => {
                let on: Vec<&str> = value.split([' ', ',']).filter(|s| !s.is_empty()).collect();
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.status = Some(match coxswain_core::flags::set_user(&path, &on) {
                    Ok(()) => t!("flags.set", "name" => name, "flags" => if on.is_empty() { t!("props.flags_none") } else { on.join(", ") }),
                    Err(e) => t!("flags.failed", "name" => name, "error" => e),
                });
            }
            Prompt::Mark(sel) => {
                // By letters, not bytes: `?` is one "æ" or "日", as in the desktop app.
                let pats: Vec<Vec<char>> = value.split([' ', ';', ',']).filter(|s| !s.is_empty()).map(|s| s.to_lowercase().chars().collect()).collect();
                let p = self.panel_mut();
                for e in p.entries.iter().filter(|e| !e.is_dir) {
                    let name: Vec<char> = e.name.to_lowercase().chars().collect();
                    if pats.iter().any(|g| index::glob(g, &name)) {
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
        find::rows(query, chip, found, self.ask_state(), self.slow.1.as_ref(), |id| self.find_dismissed.iter().any(|d| d == id))
    }

    /// A hit or a source of Find: its folder, with the cursor on it; a commit's folder as it was.
    fn go_to_hit(&mut self, path: &Path) {
        self.forget_chat();
        if history::is_history(path) {
            return self.cd(self.active, path.to_path_buf());
        }
        self.reveal(path);
    }

    /// To a file's folder, the cursor on it.
    fn reveal(&mut self, path: &Path) {
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
                    self.slow.0 = None;
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

    /// The better model of a poor choice (`setup::poor`), saved; without one, Settings opens
    /// at the option, where its line says what to do.
    fn take_better(&mut self, poor: &coxswain_core::setup::Poor) {
        if poor.button.is_none() {
            let at = if poor.ask { "ask_model" } else { "meaning_engine" };
            return self.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(self, at))));
        }
        let Some(path) = Config::path() else { return self.status = Some(t!("err.no_config_folder")) };
        self.status = Some(match self.save_options(&path, &poor.changes) {
            Ok(()) => t!("settings.saved", "path" => path.display()),
            Err(e) => e,
        });
        self.search_now();
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
        let (index, cfg, here) = (self.index.clone(), self.cfg.search.clone(), self.panel().dir.clone());
        std::thread::spawn(move || {
            // The chat model loads while the sources are looked up.
            coxswain_core::meaning::warm(&cfg);
            let done = match coxswain_core::ask::sources(|q, s, b| index.passages(q, s, b), &cfg, &earlier, &question, scope.as_deref(), &here) {
                Err(e) => Err(e),
                // Stopped while searching: the model is not asked.
                Ok(_) if stop.load(Ordering::SeqCst) => Ok(()),
                Ok((sources, similar)) => {
                    let _ = tx.send(AskMsg::Sources(sources.iter().map(|(p, _)| p.clone()).collect(), similar, coxswain_core::ask::read(&sources)));
                    index.answer(&cfg, &earlier, &question, &sources, |text| !stop.load(Ordering::SeqCst) && (text.is_empty() || tx.send(AskMsg::Piece(text.to_string())).is_ok()))
                }
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
        let ch = typed(&key);
        let esc = key.code == KeyCode::Esc || action == Some(Action::Quit);
        let Some(dialog) = self.dialog.take() else { return };
        // F1 in a dialog: Features, at what it is about; Esc comes back.
        if action == Some(Action::Help) && !matches!(dialog, Dialog::Help { .. } | Dialog::Features { .. } | Dialog::Search { .. }) {
            let filter = feature_of(&dialog).map(|id| t!(&format!("feature.{id}.title"))).unwrap_or_default();
            return self.features(filter, Some(Box::new(dialog)));
        }
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
            Dialog::Media { title, text, op, files } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.media_start(op, files),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Media { title, text, op, files }),
            },
            Dialog::Ebook { title, text, to, books } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.ebook_start(to, books),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Ebook { title, text, to, books }),
            },
            Dialog::Transcribe { title, text, files } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => {
                    self.status = Some(match self.index.transcribe(&files) {
                        Some(n) => t!("whisper.taken", "n" => n, "key" => self.key_label(Action::Settings)),
                        None => t!("whisper.no_helper"),
                    });
                }
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Transcribe { title, text, files }),
            },
            Dialog::Export { title, text, op, files } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.export_start(op, files),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Export { title, text, op, files }),
            },
            Dialog::Searchable { title, text, pdfs } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.searchable_start(pdfs),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Searchable { title, text, pdfs }),
            },
            Dialog::VirusScan { title, text, targets } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => self.viruses_start(targets),
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::VirusScan { title, text, targets }),
            },
            Dialog::Viruses(v) => self.viruses_key(v, key.code, ch, esc),
            Dialog::Switch { title, text, dir, entry } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => {
                    let r = coxswain_core::branches::switch(&dir, &entry);
                    let branch = coxswain_core::branches::name_of(&entry);
                    self.git_said(t!("error.switch", "branch" => branch), r);
                }
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Switch { title, text, dir, entry }),
            },
            Dialog::Server { title, text, server, to, feature } => match (key.code, ch) {
                (KeyCode::Enter, _) | (_, Some('y' | 'Y')) => {
                    let mut st = coxswain_core::state::AppState::load();
                    if !st.history_servers.contains(&server) {
                        st.history_servers.push(server);
                    }
                    coxswain_core::wc::allow(&st.history_servers);
                    if let Err(e) = st.save() {
                        self.status = Some(e.to_string());
                    }
                    self.cd(self.active, to);
                }
                _ if esc || matches!(ch, Some('n' | 'N')) => {}
                _ => self.dialog = Some(Dialog::Server { title, text, server, to, feature }),
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
                    // F1: the syntax; again, the questions about Find.
                    (KeyCode::F(1), _) if show == Show::Syntax => {
                        let back = Dialog::Search { query, chip, here, found, cursor, offset, show };
                        return self.features(t!("feature.find_file.title"), Some(Box::new(back)));
                    }
                    (KeyCode::F(1), _) => show = Show::Syntax,
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
                    // Find and replace in the files listed, what was searched for in them typed in.
                    _ if action == Some(Action::Replace) && !answer => {
                        let mut files: Vec<PathBuf> = vec![];
                        for h in found.names.hits.iter().chain(&found.in_files.hits) {
                            if !h.is_dir && !history::is_history(&h.path) && !files.contains(&h.path) {
                                files.push(h.path.clone());
                            }
                        }
                        let text = chip == Kind::InFiles || prefix == Some(Kind::InFiles);
                        return self.replace(Some((files, if text { question } else { String::new() })));
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
                        Some(Row::Hit { hit, .. }) => {
                            // A game in a PGN file: that game, on the board.
                            if matches!(coxswain_core::chess::format(&hit.path), Some(coxswain_core::chess::Format::Pgn | coxswain_core::chess::Format::Sgf | coxswain_core::chess::Format::Pdn | coxswain_core::chess::Format::Kif | coxswain_core::chess::Format::Csa | coxswain_core::chess::Format::Pbn))
                                && let Some(snippet) = hit.snippet.clone()
                            {
                                let path = hit.path.clone();
                                self.go_to_hit(&path);
                                return self.chess(&path, Some(snippet));
                            }
                            return self.go_to_hit(&hit.path.clone());
                        }
                        // The better chat model, taken; or Settings at Ask, where the line says what to do.
                        Some(Row::Slow { poor }) => {
                            let poor = poor.clone();
                            self.dialog = Some(Dialog::Search { query, chip, here, found, cursor, offset, show });
                            return self.take_better(&poor);
                        }
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
                    _ if matches!(action, Some(Action::View | Action::Edit | Action::EditHere)) => {
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
                            // The built-in viewer or editor over the search, which comes back
                            // as it was when it closes.
                            let search = Box::new(Dialog::Search { query, chip, here, found, cursor, offset, show });
                            match &mut self.dialog {
                                Some(Dialog::View { back, .. } | Dialog::Edit { back, .. }) => *back = Some(search),
                                _ => self.dialog = Some(*search),
                            }
                            return;
                        }
                    }
                    (KeyCode::Up, _) if answer => cursor = cursor.saturating_sub(1),
                    (KeyCode::Down, _) if answer => cursor = (cursor + 1).min(sources.len().saturating_sub(1)),
                    (KeyCode::Up, _) => cursor = find::step(&rows, cursor, -1),
                    (KeyCode::Down, _) => cursor = find::step(&rows, cursor, 1),
                    (KeyCode::PageUp, _) => cursor = find::step(&rows, cursor, -10),
                    (KeyCode::PageDown, _) => cursor = find::step(&rows, cursor, 10),
                    (KeyCode::Backspace, _) => requery = query.pop().is_some() && !answer,
                    // Ctrl+U clears the field, as in every prompt.
                    (KeyCode::Char('u'), None) if key.ctrl => {
                        requery = !query.is_empty() && !answer;
                        query.clear();
                    }
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
            Dialog::Menu { title, mut filter, items, mut cursor, direct, feature } => {
                let visible = menu_visible(&items, &filter);
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
                    Some(MenuRun::User(u)) => self.run_user(u),
                    Some(MenuRun::Script(path)) => self.run_script(&path.clone()),
                    Some(MenuRun::ScriptsFolder) => match coxswain_core::user_menu::scripts_folder() {
                        Ok(dir) => self.cd(self.active, dir),
                        Err(e) => self.status = Some(e),
                    },
                    Some(MenuRun::AddUser) => self.add_user(),
                    Some(MenuRun::Goto(side, None)) => self.goto_prompt(*side),
                    Some(MenuRun::Goto(side, Some(dir))) => self.cd(*side, dir.clone()),
                    Some(MenuRun::Say(why)) => self.status = Some(why.clone()),
                    Some(MenuRun::Export(op, files)) => self.export_ask(*op, files.clone()),
                    Some(MenuRun::Media(op, files)) => self.media_chose(op.clone(), files.clone()),
                    Some(MenuRun::Ebook(to, books)) => self.ebook_ask(Some(to), books.clone()),
                    Some(MenuRun::Sums(algo, files)) => self.sums_make(*algo, files.clone()),
                    Some(MenuRun::Drop(src, mv)) => {
                        let dst = self.panel().dir.clone();
                        self.transfer(if *mv { Transfer::Move } else { Transfer::Copy }, src.clone(), dst, None, None);
                    }
                    Some(MenuRun::Split(file, Some(size))) => self.split_start(file.clone(), *size),
                    Some(MenuRun::Split(file, None)) => self.input(&self.action_label(Action::SplitFile), t!("split.size_label"), "250M".into(), Prompt::SplitSize(file.clone())),
                    Some(MenuRun::Join(first, replace)) => self.join_start(first.clone(), *replace),
                    Some(MenuRun::PreviewTool(kind, choice)) => self.preview_tool(kind, choice),
                    Some(MenuRun::Save(changes, name)) => {
                        let (changes, name) = (changes.clone(), name.clone());
                        self.status = Some(match Config::path() {
                            Some(path) => match self.save_options(&path, &changes) {
                                Ok(()) => t!("long.saved", "name" => name),
                                Err(e) => e,
                            },
                            None => t!("err.no_config_folder"),
                        });
                    }
                    Some(MenuRun::CopyLine(line)) => {
                        crate::guide::copy(line);
                        self.status = Some(t!("guide.copied", "command" => line));
                    }
                    Some(MenuRun::SettingsAt(at)) => self.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(self, at)))),
                    Some(MenuRun::Folders(r)) => self.folders_run(*r),
                    None => self.dialog = Some(Dialog::Menu { title, filter, items, cursor, direct, feature }),
                }
            }
            Dialog::Help { .. } if matches!(ch, Some('g' | 'G')) => self.dialog = Some(Dialog::Guide(Box::default())),
            Dialog::Help { .. } if key.code == KeyCode::Tab => self.features(String::new(), None),
            Dialog::Features { mut filter, mut list, mut cursor, mut said, back } => {
                let last = list.len().saturating_sub(1);
                let mut refilter = false;
                match (key.code, ch) {
                    _ if esc || action == Some(Action::Help) => return self.dialog = back.map(|b| *b),
                    (KeyCode::Tab, _) => return self.dialog = Some(Dialog::Help { scroll: 0 }),
                    (KeyCode::Enter, _) => said = list.get(cursor).map(|e| Self::show_me(&e.url)),
                    (KeyCode::Up, _) => cursor = cursor.saturating_sub(1),
                    (KeyCode::Down, _) => cursor = (cursor + 1).min(last),
                    (KeyCode::PageUp, _) => cursor = cursor.saturating_sub(5),
                    (KeyCode::PageDown, _) => cursor = (cursor + 5).min(last),
                    (KeyCode::Backspace, _) => refilter = filter.pop().is_some(),
                    (_, Some(c)) => {
                        filter.push(c);
                        refilter = true;
                    }
                    _ => {}
                }
                if refilter {
                    list = coxswain_core::features::list(&self.cfg, &filter, false);
                    (cursor, said) = (0, None);
                }
                self.dialog = Some(Dialog::Features { filter, list, cursor, said, back });
            }
            Dialog::Help { scroll } => match key.code {
                KeyCode::Up => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(1) }),
                KeyCode::Down => self.dialog = Some(Dialog::Help { scroll: scroll + 1 }),
                KeyCode::PageUp => self.dialog = Some(Dialog::Help { scroll: scroll.saturating_sub(10) }),
                KeyCode::PageDown => self.dialog = Some(Dialog::Help { scroll: scroll + 10 }),
                _ => {}
            },
            Dialog::Message { .. } => {}
            Dialog::Refused { line, .. } if key.code == KeyCode::Enter => match Config::path() {
                Some(path) => self.edit_at(&path, line),
                None => self.status = Some(t!("err.no_config_folder")),
            },
            Dialog::Refused { .. } => {}
            Dialog::Settings(s) => self.settings_key(s, key, action),
            Dialog::Guide(g) => self.guide_key(g, key, esc),
            Dialog::Bom(mut v) => match v.key(key, action == Some(Action::Quit)) {
                bom::Outcome::Stay => self.dialog = Some(Dialog::Bom(v)),
                bom::Outcome::Close => {}
                bom::Outcome::Source => {
                    let path = v.path.clone();
                    self.source(&path, Dialog::Bom(v));
                }
                bom::Outcome::Reveal(file) => {
                    self.reveal(&file);
                }
                bom::Outcome::Compare => {
                    match self.panels[self.active ^ 1].current().filter(|e| !e.is_dir && e.path != v.path) {
                        Some(e) => v.compare(&e.path.clone()),
                        None => v.cannot_compare(t!("tui.bom.no_other")),
                    }
                    self.dialog = Some(Dialog::Bom(v));
                }
            },
            Dialog::View { mut v, back } => match v.key(key, action) {
                viewer::Outcome::Stay => self.dialog = Some(Dialog::View { v, back }),
                viewer::Outcome::Close => self.dialog = back.map(|b| *b),
                // Back to the BOM or provenance it was opened from, once the editor ends.
                viewer::Outcome::Edit(file, a) => {
                    self.view_or_edit(a, &file);
                    match &mut self.dialog {
                        Some(Dialog::Edit { back: b, .. }) => *b = back,
                        _ => self.dialog = back.map(|b| *b),
                    }
                }
                viewer::Outcome::Pager(file) => self.run_on(&pager(), &file),
                // A facts card's file as it is (hex for a model); Esc comes back to the card.
                viewer::Outcome::Original(file) => self.view_in(&file, Some(Box::new(Dialog::View { v, back }))),
            },
            Dialog::Edit { mut e, back } => match e.key(key, action) {
                editor::Outcome::Stay => self.dialog = Some(Dialog::Edit { e, back }),
                editor::Outcome::Close => {
                    self.dialog = back.map(|b| *b);
                    self.reload();
                }
                editor::Outcome::Copy(text) => {
                    guide::copy(&text);
                    self.dialog = Some(Dialog::Edit { e, back });
                }
            },
            Dialog::Compare(mut v) => match v.key(key, action) {
                compare::Outcome::Stay => self.dialog = Some(Dialog::Compare(v)),
                compare::Outcome::Close => self.reload(),
            },
            Dialog::Chess(mut v) => match v.key(key, action) {
                chess::Outcome::Stay => self.dialog = Some(Dialog::Chess(v)),
                chess::Outcome::Close => {}
                // Find this position: another file's game.
                chess::Outcome::Open(path, game) => match chess::Viewer::open(&path, &self.cfg, &self.theme) {
                    Ok(next) => {
                        let mut next = next.with_index(self.index.clone());
                        next.at_game(game);
                        self.dialog = Some(Dialog::Chess(Box::new(next)));
                    }
                    Err(e) => {
                        self.status = Some(e.to_string());
                        self.dialog = Some(Dialog::Chess(v));
                    }
                },
            },
            Dialog::Go(mut v) => match v.key(key, action) {
                chess::Outcome::Stay => self.dialog = Some(Dialog::Go(v)),
                chess::Outcome::Close | chess::Outcome::Open(..) => {}
            },
            Dialog::Replace(mut v) => match v.key(key, action) {
                replace::Outcome::Stay => self.dialog = Some(Dialog::Replace(v)),
                replace::Outcome::Close => {}
            },
            Dialog::Attrs(mut v) => match v.key(key, action) {
                attrs::Outcome::Stay => self.dialog = Some(Dialog::Attrs(v)),
                attrs::Outcome::Close => {}
            },
            Dialog::Sync(mut v) => match v.key(key, action) {
                sync::Outcome::Stay => self.dialog = Some(Dialog::Sync(v)),
                sync::Outcome::Close => {}
            },
            Dialog::GitConfig(mut v) => match v.key(key, action) {
                git_config::Outcome::Stay => self.dialog = Some(Dialog::GitConfig(v)),
                git_config::Outcome::Close => {}
                git_config::Outcome::Edit(file) => self.view_or_edit(Action::Edit, &file),
                git_config::Outcome::Copy(line) => {
                    guide::copy(&line);
                    self.status = Some(t!("gitconfig.copied", "line" => line));
                    self.dialog = Some(Dialog::GitConfig(v));
                }
                git_config::Outcome::Tool => self.git_tool(),
                git_config::Outcome::Say(said) => {
                    self.status = Some(said);
                    self.dialog = Some(Dialog::GitConfig(v));
                }
            },
            Dialog::Space(mut v) => match v.key(key, action) {
                space::Outcome::Stay => self.dialog = Some(Dialog::Space(v)),
                space::Outcome::Close => {}
                space::Outcome::Go(path) => self.reveal(&path),
                space::Outcome::Hand(a, path) => {
                    if a == Action::Delete {
                        // That file, by its path: not whatever the panel's cursor is on (a hidden
                        // file is not in the list).
                        self.space_back = Some((v, path.clone()));
                        self.ask_delete(vec![path], false);
                    } else if a == Action::View {
                        // In the built-in viewer (or the user's own), Space shown again after it.
                        self.source(&path, Dialog::Space(v));
                    } else {
                        self.space_back = Some((v, path.clone()));
                        self.view_or_edit(a, &path);
                    }
                }
            },
            Dialog::Provenance(mut v) => match v.key(key, action == Some(Action::Quit)) {
                provenance::Outcome::Stay => self.dialog = Some(Dialog::Provenance(v)),
                provenance::Outcome::Close => {}
                provenance::Outcome::Source => {
                    let path = v.path.clone();
                    self.source(&path, Dialog::Provenance(v));
                }
                provenance::Outcome::Reveal(file) => {
                    self.reveal(&file);
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

    fn run_user(&mut self, u: &config::UserCommand) {
        let p = self.panel();
        let file = p.current().filter(|e| !e.is_parent()).map(|e| e.path.as_path());
        let marked: Vec<PathBuf> = p.entries.iter().filter(|e| p.marked.contains(&e.path)).map(|e| e.path.clone()).collect();
        let line = u.expand(&p.dir, file, &marked);
        self.run = Some(Run::Shell { line, typed: false, dir: p.dir.clone(), wait: u.wait });
    }

    /// A script from F2's scripts folder, on the marked files or else the file under the cursor,
    /// as full paths; the output stays until a key is pressed.
    fn run_script(&mut self, script: &Path) {
        let p = self.panel();
        let file = p.current().filter(|e| !e.is_parent()).map(|e| e.path.as_path());
        let marked: Vec<PathBuf> = p.entries.iter().filter(|e| p.marked.contains(&e.path)).map(|e| e.path.clone()).collect();
        match coxswain_core::user_menu::script_line(script, file, &marked) {
            Ok(line) => self.run = Some(Run::Shell { line, typed: false, dir: p.dir.clone(), wait: true }),
            Err(e) => self.status = Some(e),
        }
    }

    /// Text the terminal pasted (bracketed paste): into the editor as it is; anywhere else
    /// as the keys it was typed with before, a line break as Enter.
    fn on_paste(&mut self, text: &str) {
        if let Some(Dialog::Edit { e, .. }) = &mut self.dialog {
            return e.paste(text);
        }
        // Files dropped on the terminal come as their paths: copy or move them into the panel.
        if self.dialog.is_none()
            && self.cmdline.is_empty()
            && let Some(src) = coxswain_core::drag::dropped(text)
        {
            return self.dropped(src);
        }
        for c in text.replace("\r\n", "\n").chars() {
            let code = match c {
                '\n' | '\r' => KeyCode::Enter,
                '\t' => KeyCode::Tab,
                c => KeyCode::Char(c),
            };
            self.key_typed(Key::new(code, false, false, false));
        }
    }

    /// The menu for files dropped on the terminal: Copy here, Move here.
    fn dropped(&mut self, src: Vec<PathBuf>) {
        let dir = self.panel().dir.clone();
        if let Some(key) = coxswain_core::drag::read_only(&dir) {
            return self.status = Some(t!(key));
        }
        let folder = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
        let title = t!("app.drop_title", "what" => Self::describe(&src), "folder" => ui::printable(&folder));
        let item = |key: &str, label, mv| MenuItem { key: key.into(), label, run: MenuRun::Drop(src.clone(), mv), group: None };
        let items = vec![item("c", t!("app.copy_here"), false), item("m", t!("app.move_here"), true)];
        self.dialog = Some(Dialog::Menu { title, filter: String::new(), items, cursor: 0, direct: true, feature: "drag_and_drop" });
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        self.hint_stale = true;
        if let Some(Dialog::View { v, .. }) = &mut self.dialog {
            match m.kind {
                MouseEventKind::ScrollUp => v.scroll(-3),
                MouseEventKind::ScrollDown => v.scroll(3),
                _ => {}
            }
            return;
        }
        if let Some(Dialog::Compare(v)) = &mut self.dialog {
            match m.kind {
                MouseEventKind::ScrollUp => v.scroll(-3),
                MouseEventKind::ScrollDown => v.scroll(3),
                _ => {}
            }
            return;
        }
        if let Some(Dialog::Edit { e, .. }) = &mut self.dialog {
            match m.kind {
                MouseEventKind::ScrollUp => e.scroll(-3),
                MouseEventKind::ScrollDown => e.scroll(3),
                _ => {}
            }
            return;
        }
        let Some(side) = (0..2).find(|&i| self.areas[i].contains((m.column, m.row).into())) else { return };
        if self.dialog.is_some() {
            return;
        }
        let area = self.areas[side];
        // The entry clicked: rows start below the border and the column header, and end
        // above the separator and the info line.
        let p = &self.panels[side];
        let i = m.row.checked_sub(area.y + 2).map(usize::from).filter(|&r| r < p.page).map(|r| p.offset + r).filter(|&i| i < p.entries.len());
        match m.kind {
            MouseEventKind::ScrollUp => self.panels[side].move_cursor(-3),
            MouseEventKind::ScrollDown => self.panels[side].move_cursor(3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.active = side;
                if let Some(i) = i {
                    self.panels[side].cursor = i;
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
                let Some(i) = i else { return };
                if self.cfg.right_click != "menu" {
                    return self.panels[side].toggle_mark(i);
                }
                self.panels[side].cursor = i;
                self.action_menu();
            }
            _ => {}
        }
    }

    /// What changed on disk, once it has settled: a panel whose folder changed is read again
    /// (the cursor stays on its name), and git's line when anything did, its `.git` too.
    fn follow_disk(&mut self) {
        let mut want: Vec<PathBuf> = self.panels.iter().map(|p| p.dir.clone()).collect();
        // Jujutsu's operation heads change with every jj command.
        want.extend(self.panels.iter().filter_map(|p| p.git.as_ref()).flat_map(|g| [g.root.join(".git"), g.root.join(".hg/store"), g.root.join(".jj/repo/op_heads/heads")]).filter(|g| g.is_dir()));
        want.dedup();
        self.watch.set(want);
        let paths = self.watch.changed();
        if paths.is_empty() {
            return;
        }
        let stale = self.forget(&paths);
        let sides = git_sides(&self.panels, &paths);
        for (side, stale) in stale.into_iter().enumerate() {
            // macOS names what changed by its real path (`/private/var` for `/var`).
            let dir = &self.panels[side].dir;
            let real = std::fs::canonicalize(dir).ok();
            let here = |p: &Path| p == dir || real.as_deref() == Some(p);
            let reread = paths.iter().any(|p| here(p) || p.parent().is_some_and(here));
            if reread {
                self.load(side, None);
            }
            if reread || stale {
                self.measure(side);
            }
        }
        self.refresh_git(&sides);
    }

    /// Git results and index progress, polled between events.
    /// The hint for what is under the cursor, worked out again when that changes. Not while
    /// typing, in quick search, a dialog, or with something on the status line: it would be
    /// counted as shown without being seen.
    fn follow_hint(&mut self) {
        if !self.cfg.hints {
            return self.hint = None;
        }
        if self.dialog.is_some() || self.status.is_some() || self.quick.is_some() || !self.cmdline.is_empty() || !self.hint_stale {
            return;
        }
        self.hint_stale = false;
        let s = coxswain_core::menu::Subject { whisper: self.cfg.search.whisper, ..self.panel().subject() };
        if self.hint_for.as_ref() == Some(&s) {
            return;
        }
        let last = self.hint_for.as_ref().zip(self.hint.as_ref().map(|h| h.0));
        self.hint = coxswain_core::menu::hint(&s, &self.cfg, last);
        self.hint_for = Some(s);
    }

    /// What Enter does on the entry under the cursor, worked out again when it is another one.
    fn follow_enter(&mut self) {
        let Some(e) = self.panel().current().filter(|e| !e.is_parent()) else { return self.enter_on = None };
        if self.enter_on.as_ref().is_some_and(|(p, _)| *p == e.path) {
            return;
        }
        use coxswain_core::enter;
        let place = if coxswain_core::archive::split(&self.panel().dir).is_some() { enter::Place::Archive } else { enter::Place::Disk };
        let label = enter::label(enter::enter(&e.path, e.is_dir, place));
        self.enter_on = Some((e.path.clone(), label));
    }

    /// The git facts of the repository under each panel's cursor (`Panel::facts_dir`), read on
    /// a thread when the cursor comes to it: never for a whole listing.
    fn follow_facts(&mut self) {
        for p in &mut self.panels {
            let Some(at) = p.facts_dir() else { continue };
            if p.facts.as_ref().is_some_and(|(d, _)| *d == at) {
                continue;
            }
            if p.facts_seen.as_ref() != Some(&at) {
                p.facts_seen = Some(at);
                continue;
            }
            p.facts = Some((at.clone(), None));
            // The panel's own repository has its status already.
            let st = p.current().filter(|e| e.is_parent()).and(p.git.clone());
            let (tx, g, dir) = (self.git_tx.clone(), self.glyphs.clone(), p.dir.clone());
            std::thread::spawn(move || {
                // On `..` the git line on the border shows the first line already.
                let (f, skip) = match st {
                    Some(st) => (Some(st.facts(&g)), 1),
                    None => (git::facts(&at, &g), 0),
                };
                let line = f.map(|f| f.lines()[skip..].join(" · "));
                let _ = tx.send((dir, Git::Facts(at, line)));
            });
        }
    }

    fn tick(&mut self, last_state: &mut State) {
        self.poll_folders();
        if let Some(Dialog::Replace(v)) = &mut self.dialog
            && let Some(done) = v.poll()
        {
            self.dialog = None;
            self.replaced(done);
        }
        if let Some(Dialog::Attrs(v)) = &mut self.dialog
            && let Some(done) = v.poll()
        {
            self.dialog = None;
            self.attrs_done(done);
        }
        if let Some(Dialog::Sync(v)) = &mut self.dialog
            && let Some(done) = v.poll()
        {
            self.dialog = None;
            self.sync_done(done);
        }
        if matches!(self.dialog, Some(Dialog::Search { .. })) && self.slow.0.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
            let poor = coxswain_core::setup::poor(&self.cfg.search, false).into_iter().find(|p| p.ask);
            self.slow = (Some(Instant::now()), poor);
        }
        self.follow_hint();
        self.follow_enter();
        self.follow_facts();
        if let Ok(v) = self.update_rx.try_recv() {
            use coxswain_core::update::{upgrade_hint, Upgrade, RELEASES_URL};
            self.status = Some(match upgrade_hint() {
                Some(Upgrade { cmd, lags: Some(channel) }) => t!("status.update_lags", "version" => v, "how" => cmd, "channel" => channel),
                up => t!("status.update", "version" => v, "how" => up.map_or(RELEASES_URL, |u| u.cmd)),
            });
        }
        while let Ok((dir, folder, bytes)) = self.sizes_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                p.sized(folder.clone(), bytes);
            }
        }
        // At most four sorts a second while a folder of thousands is measured.
        for p in &mut self.panels {
            if p.resort_due.is_some_and(|at| at.elapsed() >= Duration::from_millis(250)) {
                p.resort();
            }
        }
        while let Ok((generation, now)) = self.search_rx.try_recv() {
            self.search_got = generation;
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
                AskMsg::Sources(paths, similar, read) => (turn.sources, turn.similar, turn.read) = (paths, similar, read),
                AskMsg::Piece(text) => turn.answer.push_str(&text),
                AskMsg::Done(done) => {
                    turn.error = done.err();
                    self.ask_rx = None;
                }
            }
        }
        if let Some(Dialog::Settings(s)) = &mut self.dialog
            && let Some(changes) = s.poll()
            && let Some(Dialog::Settings(s)) = self.dialog.take()
        {
            self.settings_save(s, changes);
        }
        if let Some(Dialog::Provenance(v)) = &mut self.dialog {
            v.poll();
        }
        if let Some(Dialog::Chess(v)) = &mut self.dialog {
            v.poll();
        }
        if let Some(Dialog::Go(v)) = &mut self.dialog {
            v.poll();
        }
        if let Some(Dialog::View { v, .. }) = &mut self.dialog {
            v.poll();
        }
        if let Some(Dialog::Edit { e, .. }) = &mut self.dialog {
            e.poll();
        }
        // The editor is put aside while the jobs below report: a box one opens waits for it.
        let editing = if matches!(self.dialog, Some(Dialog::Edit { .. })) { self.dialog.take() } else { None };
        if let Some(r) = self.props_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.props_rx = None;
            if self.status.as_deref() == Some(t!("app.reading_properties").as_str()) {
                self.status = None;
            }
            match r {
                Ok(p) => {
                    let title = p.path.file_name().map_or_else(|| p.path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    let next = coxswain_core::disk_image::next_line(self.key_label(Action::Open), self.key_label(Action::Settings), self.key_label(Action::ActionMenu));
                    self.dialog = Some(Dialog::Message { title, text: ui::properties(&p, self.key_label(Action::Snapshots), &next) });
                }
                Err(e) => self.dialog = Some(failure(t!("action.properties"), &e)),
            }
        }
        while let Ok((dir, keep, r)) = self.list_rx.try_recv() {
            self.listed(dir, keep, r);
        }
        self.poll_job();
        self.poll_searchable();
        self.poll_searchable_plan();
        self.poll_export();
        self.poll_export_plan();
        self.poll_media();
        self.poll_media_plan();
        self.poll_ebook();
        self.poll_ebook_plan();
        self.poll_work();
        self.poll_transcribe_plan();
        self.poll_viruses();
        self.poll_viruses_plan();
        if let Some(e) = editing
            && let Some(d) = self.dialog.replace(e)
        {
            self.waiting.push(d);
        }
        if self.dialog.is_none() && !self.waiting.is_empty() {
            self.dialog = Some(self.waiting.remove(0));
        }
        self.follow_disk();
        while let Ok((dir, news)) = self.git_rx.try_recv() {
            for p in self.panels.iter_mut().filter(|p| p.dir == dir) {
                match &news {
                    Git::Status(st) => {
                        // Read again: the facts too, when they are this repository's (on `..`).
                        if p.facts.as_ref().is_some_and(|(d, _)| p.git.iter().chain(st.as_deref()).any(|g| g.root == *d)) {
                            p.facts = None;
                        }
                        (p.git, self.hint_stale) = (st.as_deref().cloned(), true);
                    }
                    Git::Last(last) => p.last = last.clone(),
                    Git::Zfs(f) => (p.zfs, self.hint_stale) = (f.as_deref().cloned(), true),
                    Git::Package(name) => p.package = name.clone(),
                    Git::Facts(at, line) => {
                        if p.facts.as_ref().is_some_and(|(d, _)| d == at) {
                            p.facts = Some((at.clone(), line.clone()));
                        }
                    }
                }
            }
        }
        // Find open: asked again when the index is ready. Only then, as each asking is a
        // round trip to the helper.
        if matches!(self.dialog, Some(Dialog::Search { .. })) {
            let state = self.index.state();
            if state != *last_state {
                *last_state = state;
                self.search_now();
            }
        }
        // Space comes back once View, Edit or Delete is done with its file.
        if self.dialog.is_none() && self.job.is_none() && self.run.is_none()
            && let Some((mut v, path)) = self.space_back.take()
        {
            if std::fs::symlink_metadata(&path).is_err() {
                v.gone(&path);
            }
            self.dialog = Some(Dialog::Space(v));
        }
        // Last, so a notice it shows is not written over in the same round.
        self.tell();
    }
}

/// The letter a key types, if it types one. Alt Gr letters (@ { \ € on German and Nordic
/// keyboards, ą ł on Polish) come from Windows terminals as Ctrl+Alt with the letter itself,
/// which is never one of the plain letters or digits a Ctrl+Alt shortcut uses.
fn typed(key: &Key) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.ctrl && !key.alt => Some(if key.shift { c.to_ascii_uppercase() } else { c }),
        KeyCode::Char(c) if key.ctrl && key.alt && !c.is_ascii_alphanumeric() && !c.is_control() => Some(c),
        _ => None,
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
    use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
    let mut out = std::io::stdout();
    // Bracketed paste where the terminal has it (not the old Windows console).
    let _ = execute!(out, DisableBracketedPaste);
    execute!(out, DisableMouseCapture, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;
    f();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, EnableMouseCapture)?;
    let _ = execute!(out, EnableBracketedPaste);
    term.clear()
}

fn run_shell(line: &Line, typed: bool, dir: &Path, wait: bool) {
    println!("{}> {}", dir.display(), line.shown());
    let c = if typed { coxswain_core::tools::typed_line(&line.cmd) } else { line.command() };
    let sh = c.get_program().to_string_lossy().into_owned();
    if let Err(e) = run_in(c, dir, wait) {
        println!("coxswain: {sh}: {e}");
    }
}

/// `c` (a shell and its line), on the plain terminal. Ctrl+C there interrupts the whole
/// foreground process group, Coxswain included: it is ignored here meanwhile, and the child
/// gets it as usual.
fn run_in(mut c: std::process::Command, dir: &Path, wait: bool) -> std::io::Result<()> {
    c.current_dir(dir);
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
    // A command has run: Ctrl+O shows its output, or says there is none yet.
    let mut ran = false;
    // A file operation still running when asked to quit finishes first: cut short, it
    // would leave half a file behind. A second Quit lets it go (`Action::Quit`).
    while !app.quit || app.job.is_some() || app.searchable.is_some() || app.media.is_some() || app.ebook.is_some() || app.work.is_some() || app.export.is_some() || app.viruses.is_some() {
        // An error on the status line in plain words, its first line only; the whole of it
        // goes to helper.log.
        if let Some(s) = &app.status {
            let p = coxswain_core::fs::plain(s);
            if &p != s {
                app.status = Some(p);
            }
        }
        term.draw(|f| ui::draw(f, app))?;
        match app.run.take() {
            Some(Run::Shell { line, typed, dir, wait }) => {
                suspended(term, || run_shell(&line, typed, &dir, wait))?;
                ran = true;
                // The setup guide, run from Find, changes the search settings.
                // An entry added to the user menu shows at once.
                if let Ok(cfg) = Config::load() {
                    app.cfg.search = cfg.search;
                    app.slow.0 = None;
                    app.cfg.user_menu = cfg.user_menu;
                }
                // A model downloaded from Settings shows there.
                if let Some(Dialog::Settings(s)) = &mut app.dialog {
                    s.models = None;
                }
                app.reload();
            }
            Some(Run::ShowOutput) => suspended(term, || {
                // A line under the output says what happened; any key returns to the panels
                // and takes the line away again.
                use ratatui::crossterm::style::{Attribute, Print, SetAttribute};
                let _ = terminal::enable_raw_mode();
                let mut out = std::io::stdout();
                if cursor::position().is_ok_and(|(x, _)| x > 0) {
                    let _ = execute!(out, Print("\r\n"));
                }
                let say = t!(if ran { "tui.panels_hidden" } else { "tui.panels_hidden_none" });
                let _ = execute!(out, SetAttribute(Attribute::Reverse), Print(say), SetAttribute(Attribute::Reset));
                // A read that fails (the terminal gone) ends the wait too, rather than spin.
                while event::read().is_ok_and(|e| !matches!(e, Event::Key(k) if k.kind == KeyEventKind::Press)) {}
                let _ = execute!(out, Print("\r"), terminal::Clear(terminal::ClearType::CurrentLine));
                let _ = terminal::disable_raw_mode();
            })?,
            None => {}
        }
        // Quicker while a search or an answer is out, so it shows as soon as it comes.
        let waiting = app.search_got != app.search_gen
            || app.ask_rx.is_some()
            || app.ask_check_rx.is_some()
            || matches!(&app.dialog, Some(Dialog::View { v, .. }) if v.busy())
            || matches!(&app.dialog, Some(Dialog::Edit { e, .. }) if e.busy())
            || matches!(&app.dialog, Some(Dialog::Compare(v)) if v.busy());
        if event::poll(Duration::from_millis(if waiting { 30 } else { 200 }))? {
            let first = event::read()?;
            // An Esc read in one go with the key after it took that key's Esc too (crossterm):
            // the key is put back together rather than typed as its letters.
            let rest = match &first {
                Event::Key(k) if k.code == CK::Esc && k.kind == KeyEventKind::Press && k.modifiers.is_empty() => {
                    split_escape::after_esc(|| event::poll(Duration::ZERO).unwrap_or(false).then(event::read).and_then(Result::ok))
                }
                _ => Vec::new(),
            };
            for e in std::iter::once(first).chain(rest) {
                match e {
                    Event::Key(k) => app.on_key(k),
                    Event::Mouse(m) => app.on_mouse(m),
                    Event::Paste(text) => app.on_paste(&text),
                    _ => {}
                }
            }
        }
        app.tick(&mut state);
    }
    Ok(())
}

const USAGE: &str = "coxswain [LEFT] [RIGHT]      a folder, or a file to open its folder with the cursor on it
  --settings[=AREA|OPTION]  start with Settings open (in the app: F12): search, previews,
                  games, looks, behaviour, keys, privacy, or an option such as show_hidden
  --dump-config   print the full default config (redirect it to the config file to customise)
  --config-path   print where the config file is read from
  --paths         print where everything is kept: config, state, index, search store, models
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
  --meaning bench [URL MODEL]  time the model on 64 passages and say what the files to go take;
                           with a server and model named, that server is tried instead
  --meaning ask MODEL|off  Ask in Find: the chat model on that server (Ollama here with the
                           built-in model) that answers questions from your files, or one
                           built in: builtin:qwen3-1.7b, builtin:qwen3-4b or
                           builtin:qwen3-14b (downloaded once);
                           delete: the built-in ones deleted
  --models                 the built-in models on the disk: what each is for, its size and
                           folder, whether it is in use and loaded, when it was last used
  --models delete NAME|unused  delete one (by the name --models shows), or all not in use
  --disk                   every place Coxswain keeps things: its size, where it is, what
                           clearing it costs; what it caused outside its folder too
  --disk clear NAME|rebuildable  clear one (by the name --disk shows: store, index, previews,
                           a model, tectonic, images), or everything that is built again
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
        let new = if l.new() { format!("  {}", t!("news.new")) } else { String::new() };
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
        println!("{}  {}\n  {}\n", c.version, c.date, notices::text(&c));
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
    Client::start(&Config::load_search()).restart();
}

/// A command line option that could not be done: why, and the exit code.
fn fail(e: impl std::fmt::Display, code: i32) -> ! {
    eprintln!("coxswain: {e}");
    std::process::exit(code)
}

/// `--meaning on|off|delete`: search by meaning, as Settings in the desktop app does it.
fn meaning(what: Option<&str>, rest: &[String]) {
    use coxswain_core::meaning;
    let fail = |e: String| -> ! { fail(e, 1) };
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
        let old = Config::load_search();
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
            let key = meaning::key_of(&Config::load_search());
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
        // The model's speed on a few passages, and what the files to go take at it; another
        // server is tried without a change: `--meaning bench http://my-server:8000/api/v1 <model>`.
        Some("bench") => {
            let saved = Config::load_search();
            let mut cfg = saved.clone();
            if let (Some(url), Some(model)) = (rest.first(), rest.get(1)) {
                (cfg.meaning_engine, cfg.meaning_url, cfg.meaning_model) = ("openai".into(), url.clone(), model.clone());
            }
            let status = Client::start(&saved).status();
            println!("{}", meaning::bench_report(&cfg, Some(&status)).unwrap_or_else(|e| fail(e)));
        }
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
                let helper = Client::start(&Config::load_search());
                for m in coxswain_core::chat::MODELS {
                    m.remove(&helper).unwrap_or_else(|e| fail(e.to_string()));
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
            let search = Config::load_search();
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
            let search = Config::load_search();
            let on = search.meaning && (search.meaning_engine != "builtin" || meaning::installed());
            println!("{}", if on { "on" } else { "off" });
            // The built-in model: on the GPU (Metal) or on the CPU, and why.
            if let Some(runs) = Client::start(&search).status().meaning_runs.filter(|_| on) {
                println!("{}", t!("settings.meaning_runs", "where" => runs.text()));
            }
            return;
        }
    }
    Client::start(&Config::load_search()).restart();
}

/// The options, and the values each takes after it (none: it takes none, or any words).
const OPTIONS: &[(&str, &[&str])] = &[
    ("--help", &[]),
    ("-h", &[]),
    ("--version", &[]),
    ("-V", &[]),
    ("--dump-config", &[]),
    ("--config-path", &[]),
    ("--paths", &[]),
    ("--languages", &[]),
    ("--hints", &["reset"]),
    ("--whats-new", &["all"]),
    ("--index-service", &["on", "off"]),
    ("--setup-search", &[]),
    ("--meaning", &["on", "off", "delete", "ollama", "server", "builtin", "cpu", "auto", "ask", "bench"]),
    ("--models", &["delete"]),
    ("--disk", &["clear"]),
    ("--settings", &[]),
];

/// The edits that turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// Why the command line cannot be run: an option or a value not known (with the closest known
/// option), or a folder that is not there. `args` without `--settings`; `cwd` for relative paths.
fn check_args(args: &[String], cwd: &Path) -> Result<(), String> {
    let close = |given: &str, known: &[&str]| known.iter().map(|k| (distance(given, k), *k)).filter(|(d, k)| *d <= 3.max(given.len() / 3) || k.starts_with(given)).min().map(|(_, k)| k.to_string());
    let Some(first) = args.first().filter(|a| *a != helper::ARG) else { return Ok(()) };
    if first.starts_with('-') {
        let name = first.split('=').next().unwrap_or(first);
        let Some((_, values)) = OPTIONS.iter().find(|(o, _)| *o == name && name == first) else {
            let mut said = format!("coxswain: {}", t!("cli.unknown_option", "option" => first));
            if let Some(near) = close(name, &OPTIONS.iter().map(|(o, _)| *o).collect::<Vec<_>>()) {
                said += &format!("\n{}", t!("cli.did_you_mean", "option" => near));
            }
            return Err(format!("{said}\n{}", t!("cli.help_lists")));
        };
        let value = args.get(1).map(String::as_str);
        // `--hints` takes its value; the others may stand alone.
        if first == "--hints" && value.is_none() {
            return Err(format!("coxswain: {}", t!("cli.needs_value", "option" => first, "values" => values.join(", "))));
        }
        if value.is_some_and(|v| !values.is_empty() && !values.contains(&v)) {
            let mut said = format!("coxswain: {}", t!("cli.unknown_value", "value" => value.unwrap_or(""), "option" => first, "values" => values.join(", ")));
            if let Some(near) = value.and_then(|v| close(v, values)) {
                said += &format!("\n{}", t!("cli.did_you_mean", "option" => format!("{first} {near}")));
            }
            return Err(said);
        }
        return Ok(());
    }
    for a in args.iter().take(2) {
        // An option is read first only: after a folder, it is said where it goes.
        if a.starts_with('-') {
            check_args(std::slice::from_ref(a), cwd)?;
            return Err(format!("coxswain: {}\n{}", t!("cli.unknown_option", "option" => a), t!("cli.did_you_mean", "option" => format!("coxswain {a}"))));
        }
        if !resolve(cwd, a).exists() {
            return Err(format!("coxswain: {}", t!("cli.no_folder", "path" => a)));
        }
    }
    Ok(())
}

/// `--models [delete NAME|unused]`: the built-in models on the disk, or one deleted (asked
/// first when it is in use: what comes instead is said), or all not in use.
fn models(rest: &[String]) {
    use coxswain_core::models::{self, For};
    let cfg = Config::load().unwrap_or_else(|e| fail(e, 2));
    let client = Client::start(&cfg.search);
    let entries = models::list(&cfg.search, &client.loaded());
    let human = coxswain_core::settings::human;
    match (rest.first().map(String::as_str), rest.get(1)) {
        (None, _) => {
            if entries.is_empty() {
                println!("{}", t!("models.none"));
            }
            for e in &entries {
                let name = if e.what == For::Leftover { format!("{} ({})", e.name, e.id) } else { e.name.clone() };
                println!("{:<24} {name}\n  {}\n  {}", e.id, e.line(), e.folder.display());
            }
            let unused = models::unused_bytes(&entries);
            if unused > 0 {
                println!("\n{}: coxswain --models delete unused", t!("models.delete_unused", "size" => human(unused)));
            }
        }
        (Some("delete"), Some(name)) if name == "unused" => {
            let gone: Vec<_> = entries.iter().filter(|e| !e.in_use).collect();
            for e in &gone {
                models::delete(e, &client).unwrap_or_else(|err| fail(format!("{}: {err}", e.folder.display()), 1));
            }
            println!("{}", t!("models.deleted_all", "n" => gone.len(), "size" => human(gone.iter().map(|e| e.bytes).sum())));
        }
        (Some("delete"), Some(name)) => {
            let Some(e) = models::find(&entries, name) else { fail(t!("models.no_such", "name" => name), 1) };
            let instead = models::instead(&cfg.search, e, coxswain_core::setup::look(true).as_deref());
            if instead.is_some() {
                print!("{} [y/N] ", t!("models.confirm", "model" => e.name.as_str()));
                let _ = std::io::stdout().flush();
                let mut answer = String::new();
                let _ = std::io::stdin().read_line(&mut answer);
                if !matches!(answer.trim(), "y" | "Y" | "j" | "J") {
                    return;
                }
            }
            if let Some((changes, _)) = &instead {
                coxswain_core::settings::save(changes).unwrap_or_else(|err| fail(err, 1));
            }
            models::delete(e, &client).unwrap_or_else(|err| fail(format!("{}: {err}", e.folder.display()), 1));
            println!("{}", t!("models.deleted", "model" => e.name.as_str(), "size" => human(e.bytes)));
            if let Some((_, said)) = instead {
                println!("{said}");
                client.restart();
            }
        }
        _ => fail(t!("cli.unknown_value", "value" => rest.join(" "), "option" => "--models", "values" => "delete NAME, delete unused"), 2),
    }
}

/// `--disk [clear NAME|rebuildable]`: every place Coxswain keeps things, or one cleared (what is
/// outside its folder is listed and asked about first; a model goes as `--models delete` has
/// it), or everything that is built again by itself.
fn disk(rest: &[String]) {
    use coxswain_core::disk::{self, Kind};
    let cfg = Config::load().unwrap_or_else(|e| fail(e, 2));
    let client = Client::start(&cfg.search);
    let items = disk::items(&cfg, &client.loaded(), true);
    let name = match (rest.first().map(String::as_str), rest.get(1)) {
        (None, _) => return print!("{}", disk::table(&items)),
        (Some("clear"), Some(name)) if rest.len() == 2 => name,
        _ => fail(t!("cli.unknown_value", "value" => rest.join(" "), "option" => "--disk", "values" => "clear NAME, clear rebuildable"), 2),
    };
    if name == disk::REBUILDABLE {
        let (freed, failed) = disk::clear_all(&items, false, false, &cfg, &client);
        println!("{}", t!("disk.cleared_all", "size" => coxswain_core::settings::human(freed)));
        if !failed.is_empty() {
            fail(failed.join("\n"), 1);
        }
        return;
    }
    let found = disk::find(&items, name);
    if found.is_empty() {
        let names: Vec<&str> = items.iter().map(|i| i.id.trim_start_matches("image:")).chain([disk::ALL_IMAGES, disk::REBUILDABLE]).collect();
        fail(t!("cli.unknown_value", "value" => name.as_str(), "option" => "--disk clear", "values" => names.join(", ")), 2);
    }
    if found[0].kind == Kind::Model {
        return models(&["delete".to_string(), found[0].id.clone()]);
    }
    // Outside its folder, or the user's own files: the exact list first, and a yes.
    if found.iter().any(|i| matches!(i.kind, Kind::Outside | Kind::Yours) && i.clear) {
        println!("{}", t!("disk.confirm"));
        for i in &found {
            println!("  {}  {}", i.paths.first().map(|p| p.display().to_string()).unwrap_or_else(|| i.name.clone()), coxswain_core::settings::human(i.bytes));
        }
        print!("[y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "j" | "J") {
            return;
        }
    }
    for i in found {
        match disk::clear(i, &cfg, &client) {
            Ok(n) => println!("{}", disk::cleared(&i.name, n)),
            Err(e) => fail(format!("{}: {e}", i.name), 1),
        }
    }
}

/// The program, under both its names (`coxswain` and `cox`).
pub fn main() {
    coxswain_core::sizes::deep_stacks();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--settings[=area|option] [LEFT] [RIGHT]`: start with Settings open, there.
    let open_settings = args.first().and_then(|a| a.strip_prefix("--settings")).filter(|r| r.is_empty() || r.starts_with('=')).map(|r| r.trim_start_matches('=').to_string());
    if open_settings.is_some() {
        args.remove(0);
    }
    // An option or a folder that is not known is said, and nothing starts.
    if args.first().map(String::as_str) != Some(helper::ARG) {
        coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&Config::load().map(|c| c.language).unwrap_or_default()));
        if let Err(e) = check_args(&args, &std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))) {
            eprintln!("{e}");
            std::process::exit(2);
        }
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
        Some("--setup-search") => return setup::run(),
        Some("--meaning") => return meaning(args.get(1).map(String::as_str), &args[2.min(args.len())..]),
        Some("--models") => return models(&args[1..]),
        Some("--disk") => return disk(&args[1..]),
        _ => {}
    }
    coxswain_core::fs::lock_down();
    // What a write of an archive cut short by a crash left beside it.
    std::thread::spawn(coxswain_core::spill::at_start);
    // A config.toml of 1.x gets the names of 2.0, once.
    if let Err(e) = coxswain_core::migrate::on_start() {
        eprintln!("coxswain: {e}");
    }
    let cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("coxswain: {e}");
        std::process::exit(2)
    });
    coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&cfg.language));
    coxswain_core::disk_image::set_open(cfg.archive.disk_images);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let dir = |i: usize| {
        let d = args.get(i).map(|a| resolve(&cwd, a)).unwrap_or_else(|| cwd.clone());
        coxswain_core::fs::canonical(&d).unwrap_or(d)
    };
    let mut app = App::new(cfg, dir(0), dir(1)).unwrap_or_else(|e| {
        eprintln!("coxswain: {e}");
        std::process::exit(2)
    });
    // A mistake in config.toml: said first, with the way to its line.
    let refused = coxswain_core::notices::refused(&app.cfg);
    if let Some(line) = refused.first().and_then(|n| n.config_line) {
        let text = refused.into_iter().map(|n| n.text).collect::<Vec<_>>().join("\n");
        app.dialog = Some(Dialog::Refused { text, line });
    } else if let Some(section) = open_settings {
        app.dialog = Some(Dialog::Settings(Box::new(settings::Settings::open(&app, &section))));
    } else if coxswain_core::state::AppState::load().guide_due() {
        app.dialog = Some(Dialog::Guide(Box::default()));
    }
    let mut term = ratatui::init();
    // ratatui's panic hook restores the screen but not the mouse: turn it off first, or the
    // shell gets mouse escapes after a panic.
    let restore = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |i| {
        let _ = execute!(std::io::stdout(), ratatui::crossterm::event::DisableMouseCapture, ratatui::crossterm::event::DisableBracketedPaste);
        restore(i)
    }));
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::EnableMouseCapture);
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::EnableBracketedPaste);
    pictures::live();
    let res = main_loop(&mut term, &mut app);
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::DisableBracketedPaste);
    let _ = execute!(std::io::stdout(), ratatui::crossterm::event::DisableMouseCapture);
    ratatui::restore();
    coxswain_core::archive::clean_up();
    coxswain_core::spill::at_exit();
    if let Err(e) = res {
        eprintln!("coxswain: {e}");
    }
}

/// The panels whose git line a change to these paths can alter: one with a change inside
/// its repository (its folder, the other panel's folder in it, its `.git`), or, outside a
/// repository, in its own folder (a `.git` made there).
fn git_sides(panels: &[Panel; 2], paths: &[PathBuf]) -> Vec<usize> {
    (0..2)
        .filter(|&s| {
            let p = &panels[s];
            let dir = &p.dir;
            // macOS names what changed by its real path (`/private/var` for `/var`).
            let real = std::fs::canonicalize(dir).ok();
            let root = p.git.as_ref().map(|g| g.root.as_path());
            let real_root = root.and_then(|r| std::fs::canonicalize(r).ok());
            paths.iter().any(|c| {
                [Some(dir.as_path()), real.as_deref(), root, real_root.as_deref()].into_iter().flatten().any(|d| c.starts_with(d))
            })
        })
        .collect()
}

/// A folder of a test's own, empty: panels on the shared temp folder would measure, and an
/// index would walk, everything other programs keep there.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("coxswain-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The app's own index for a test (no helper): over an empty folder, not the whole disk, and
/// kept in no cache file.
#[cfg(test)]
pub(crate) fn test_index(cfg: &Config) -> Arc<Client> {
    let search = coxswain_core::config::SearchConfig { roots: vec![test_dir("index")], watch: false, ..cfg.search.clone() };
    Client::with(None, &search, |_| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's tests keep their config, state and caches in a home of their own in the
    /// temporary folder (`coxswain-core`'s `test-home`), never the developer's own.
    #[test]
    fn tests_never_reach_the_real_home() {
        let mut all = coxswain_core::config::Config::paths();
        all.push(("home", coxswain_core::dirs::home_dir()));
        for (what, path) in all {
            let path = path.unwrap_or_else(|| panic!("{what}: none"));
            assert!(path.starts_with(std::env::temp_dir()), "{what}: {}", path.display());
        }
    }

    /// Alt Gr letters arrive as Ctrl+Alt from Windows terminals and are typed; Ctrl+Alt with a
    /// plain letter stays a shortcut.
    #[test]
    fn alt_gr_letters_are_typed() {
        let k = |c, ctrl, alt| Key::new(KeyCode::Char(c), ctrl, alt, false);
        assert_eq!(typed(&k('@', true, true)), Some('@'));
        assert_eq!(typed(&k('ą', true, true)), Some('ą'));
        assert_eq!(typed(&k('a', true, true)), None);
        assert_eq!(typed(&k('@', true, false)), None);
        assert_eq!(typed(&k('x', false, false)), Some('x'));
    }

    /// An option or a value not known, or a folder not there, is said, never taken for a folder.
    #[test]
    fn unknown_options_and_folders_are_said() {
        let cwd = crate::test_dir("panels");
        let check = |a: &[&str]| check_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &cwd);
        let e = check(&["--model"]).unwrap_err();
        assert!(e.contains("--model") && e.contains("--models?") && e.contains("--help"), "{e}");
        assert!(check(&["--frobnicate"]).unwrap_err().contains("--frobnicate"));
        assert!(check(&["--meaning", "of"]).unwrap_err().contains("--meaning off"));
        assert!(check(&["--hints"]).is_err() && check(&["--hints", "reset"]).is_ok());
        assert!(check(&["--meaning", "server", "http://x:8000/v1", "m"]).is_ok() && check(&["--meaning"]).is_ok());
        assert!(check(&["--models"]).is_ok() && check(&["--models", "delete", "unused"]).is_ok() && check(&["--models", "x"]).is_err());
        assert!(check(&["--disk"]).is_ok() && check(&["--disk", "clear", "index"]).is_ok() && check(&["--disk", "clean"]).unwrap_err().contains("--disk clear"));
        assert!(check(&["no-such-folder-here"]).unwrap_err().contains("no-such-folder-here"));
        assert!(check(&[".", "--paths"]).is_err(), "an option after a folder is not taken for one");
        assert!(check(&[".", "."]).is_ok() && check(&[]).is_ok() && check(&[helper::ARG]).is_ok());
    }

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
    thread_local! {
        /// How long a test's app waits for a folder to be listed (`App::list_wait`), from the
        /// start: long, as tests list small folders and look at them at once, on machines
        /// that can be slow.
        pub(super) static LIST_WAIT: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::from_secs(10)) };
        /// What holds a test's listing threads from the start (`App::list_hold`).
        pub(super) static LIST_HOLD: std::cell::RefCell<Option<Arc<std::sync::RwLock<()>>>> = const { std::cell::RefCell::new(None) };
    }

    fn app(left: PathBuf, right: PathBuf) -> App {
        let cfg = Config { check_updates: false, ..Config::default() };
        let index = crate::test_index(&cfg);
        App::with_index(cfg, left, right, index).unwrap()
    }

    /// After a run that made one side like the other, the compare that starts again keeps the
    /// run's result (Copied …) on the status line, in front of its own summary.
    #[test]
    #[cfg(unix)]
    fn a_sync_run_keeps_its_result_on_the_status_line() {
        use coxswain_core::compare::{folders::Mode, sync};
        let d = std::env::temp_dir().join(format!("coxswain-test-sync-said-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (l, r) = (d.join("l"), d.join("r"));
        std::fs::create_dir_all(&l).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        std::fs::write(l.join("new.txt"), "x").unwrap();
        let mut app = app(l.clone(), r.clone());
        app.folders_start(Mode::Quick);
        let (stop, progress) = (std::sync::atomic::AtomicBool::new(false), sync::Progress::default());
        let plan = sync::plan(&l, &r, Mode::Quick, false, &stop, &progress).unwrap();
        let done = sync::apply(&plan, &stop, &progress).unwrap();
        let said = done.summary();
        app.sync_done(Ok(done));
        let c = app.panels[0].cmp.clone().unwrap();
        let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(20));
        while !c.f.found().done && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        app.poll_folders();
        let status = app.status.clone().unwrap_or_default();
        assert!(status.starts_with(&said) && status.ends_with(&c.f.found().summary()), "{status}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Differences only (Ctrl+F2, 2) keeps the compare's summary on the status line: the key
    /// that chose it took the line away, and a hint stood there instead.
    #[test]
    fn differences_only_keeps_the_summary() {
        let d = std::env::temp_dir().join(format!("coxswain-test-diff-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (l, r) = (d.join("l"), d.join("r"));
        std::fs::create_dir_all(&l).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        std::fs::write(l.join("only.txt"), "x").unwrap();
        std::fs::write(l.join("same.txt"), "s").unwrap();
        std::fs::write(r.join("same.txt"), "s").unwrap();
        let mut app = app(l, r);
        app.folders_start(coxswain_core::compare::folders::Mode::Quick);
        let c = app.panels[0].cmp.clone().unwrap();
        let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(20));
        while !c.f.found().done && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        let summary = c.f.found().summary();
        app.status = None;
        app.folders_run(FoldersRun::DiffOnly);
        assert_eq!(app.status.as_deref(), Some(summary.as_str()));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A Find hit in a large SGF file: its game is looked for on a thread, so opening the file
    /// returns at once and the UI keeps answering (it read the whole file first, up to 1 GiB);
    /// "Finding the game" shows meanwhile, the game opens when found, and Esc stops the looking.
    /// No clock: what proves the looking is off the UI thread is that the first turn of the UI
    /// loop after opening is still looking, and that the game shows only after more than one
    /// turn (opening or a turn that waited for it would show it at the first).
    #[test]
    fn a_hit_in_a_large_sgf_file_is_found_off_the_ui_thread() {
        use std::io::Write;
        let d = std::env::temp_dir().join(format!("coxswain-test-sgf-hit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("big.sgf");
        // As many games as are listed, each with a long comment: some 40 MB; the hit is the last.
        let n = coxswain_core::go::MAX_GAMES;
        let note = "a long comment on the opening ".repeat(12);
        let mut f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        for i in 0..n {
            writeln!(f, "(;SZ[19]PB[Black{i}]PW[White{i}]RE[B+R];B[pd]C[{note}];W[dp];B[pp];W[dd])").unwrap();
        }
        drop(f);
        let (o, c) = coxswain_core::store::MARK;
        let snippet = format!("{o}Black{}{c} – White{}", n - 1, n - 1);
        let mut app = app(d.clone(), d.clone());
        let mut state = app.index.state();
        let looking = |app: &App| {
            let Some(Dialog::Go(v)) = &app.dialog else { panic!("no Go view") };
            v.finding() && v.shown().is_none()
        };
        // Opened: still looking after a turn, then the looking stopped by Esc.
        app.chess(&p, Some(snippet.clone()));
        app.tick(&mut state);
        assert!(looking(&app), "the game was found before the UI turned");
        app.dialog_key(Key { code: KeyCode::Esc, ctrl: false, alt: false, shift: false });
        let Some(Dialog::Go(v)) = &app.dialog else { panic!("no Go view") };
        assert!(!v.finding(), "Esc stops the looking; the table stays");
        app.dialog = None;
        // Opened again and left to find it: the UI loop keeps turning meanwhile.
        app.chess(&p, Some(snippet));
        let mut turns = 0;
        let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(120));
        loop {
            app.tick(&mut state);
            turns += 1;
            let Some(Dialog::Go(v)) = &app.dialog else { panic!("no Go view") };
            if let Some(i) = v.shown() {
                assert_eq!(i, n - 1);
                break;
            }
            assert!(Instant::now() < end, "the game was never found");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(turns > 1, "found before the first turn: the file is too small to show anything");
        // Opening it reads nothing of the file on the UI thread (it once read all of it before
        // the looking started). No clock: the file is a pipe held open but never written, so
        // any read of it waits; opening returns all the same and is still looking. Should it
        // read (and wait), the pipe is closed after a long while, so it fails, not hangs.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let pipe = d.join("pipe.sgf");
            let name = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            // Open for reading and writing: opening the pipe never waits, reading it does.
            let held = std::fs::OpenOptions::new().read(true).write(true).open(&pipe).unwrap();
            let (tx, rx) = mpsc::channel::<()>();
            let wait = coxswain_core::test_limit(Duration::from_secs(10));
            let closer = std::thread::spawn(move || {
                let stuck = rx.recv_timeout(wait).is_err();
                drop(held);
                stuck
            });
            app.chess(&pipe, Some(format!("{o}Black{}{c} – White{}", n - 1, n - 1)));
            let _ = tx.send(());
            assert!(!closer.join().unwrap(), "opening read the file on the UI thread");
            assert!(looking(&app), "nothing can be read, yet the game was found");
            // Stopped; the threads reading the pipe see its end.
            app.dialog = None;
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A box a background job opens while the editor is open waits for the editor to close:
    /// the editor and its unsaved text stay.
    #[test]
    fn a_job_ending_never_puts_the_editor_away() {
        let d = std::env::temp_dir().join(format!("coxswain-test-edit-waits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("f.txt"), "x\n").unwrap();
        let mut app = app(d.clone(), d.clone());
        app.edit_here(&d.join("f.txt"), None, None);
        let Some(Dialog::Edit { e, .. }) = &mut app.dialog else { panic!("no editor") };
        e.buf.insert("typed ");
        let (tx, rx) = mpsc::channel();
        app.props_rx = Some(rx);
        tx.send(Err("failed".into())).unwrap();
        let mut state = app.index.state();
        app.tick(&mut state);
        assert!(matches!(&app.dialog, Some(Dialog::Edit { e, .. }) if e.buf.dirty()));
        app.dialog = None;
        app.tick(&mut state);
        assert!(app.dialog.is_some() && !matches!(app.dialog, Some(Dialog::Edit { .. })));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// F2 Rename shows a newline in a name as ␊ and keeps it; text left as shown renames
    /// nothing. A copy of which one item fails says how many went through.
    #[cfg(unix)]
    #[test]
    fn rename_keeps_control_characters_and_failures_are_counted() {
        let d = std::env::temp_dir().join(format!("coxswain-test-rename-controls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("to")).unwrap();
        let odd = d.join("a\nb.txt");
        std::fs::write(&odd, "x").unwrap();
        let mut app = app(d.clone(), d.clone());
        let wait = |app: &mut App| {
            let t = Instant::now();
            while app.job.is_some() && t.elapsed() < coxswain_core::test_limit(Duration::from_secs(10)) {
                std::thread::sleep(Duration::from_millis(5));
                app.poll_job();
            }
        };
        app.submit(Prompt::Rename(odd.clone()), "a␊b.txt".into());
        assert!(app.job.is_none() && odd.exists(), "unchanged: nothing renamed");
        app.submit(Prompt::Rename(odd.clone()), "c␊b.txt".into());
        wait(&mut app);
        assert!(d.join("c\nb.txt").exists() && !odd.exists());
        app.transfer(Transfer::Copy, vec![d.join("c\nb.txt"), d.join("gone.txt")], d.join("to"), None, None);
        wait(&mut app);
        let Some(Dialog::Message { title, .. }) = &app.dialog else { panic!("no failure shown: {:?}", app.status) };
        assert_eq!(title, &coxswain_core::undo::failure_title(coxswain_core::undo::Op::Copy, "", 2, 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The user-test round's terminal fixes: Find's F3 and Ctrl+U, a history refusing changes
    /// at once, `?` as one letter, a wrong password said, an editor that is not there.
    #[test]
    fn find_views_a_hit_a_history_refuses_and_a_missing_editor_is_said() {
        let d = std::env::temp_dir().join(format!("coxswain-test-usertest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["ø.txt", "ab.txt", "a.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let mut app = app(d.clone(), d.clone());
        let k = |c: KeyCode, ctrl: bool| Key::new(c, ctrl, false, false);
        // F3 on a hit: the built-in viewer, and Find again after it.
        let mut found = Found::default();
        found.names.hits = vec![coxswain_core::index::Hit { path: d.join("a.txt"), is_dir: false, snippet: None, similar: None }];
        found.names.total = 1;
        let cursor = app.find_rows("a", Kind::All, &found).iter().position(|r| matches!(r, Row::Hit { .. })).unwrap();
        app.dialog = Some(Dialog::Search { query: "a".into(), chip: Kind::All, here: false, found, cursor, offset: 0, show: Show::List });
        app.dialog_key(k(KeyCode::F(3), false));
        let Some(Dialog::View { back: Some(back), .. }) = &app.dialog else { panic!("not viewed") };
        assert!(matches!(**back, Dialog::Search { .. }));
        app.dialog_key(k(KeyCode::Esc, false));
        // Ctrl+U clears Find's field.
        assert!(matches!(&app.dialog, Some(Dialog::Search { query, .. }) if query == "a"), "back at Find");
        app.dialog_key(k(KeyCode::Char('u'), true));
        assert!(matches!(&app.dialog, Some(Dialog::Search { query, .. }) if query.is_empty()));
        app.dialog = None;
        // `?` is one letter, also outside ASCII.
        app.submit(Prompt::Mark(true), "?.txt".into());
        let marked: Vec<String> = app.panels[0].marked.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(marked.len(), 2, "{marked:?}");
        assert!(marked.contains(&"ø.txt".to_string()) && marked.contains(&"a.txt".to_string()));
        app.panels[0].clear_marks();
        // In a history, F8, F7, F6 say read-only at once: no question first.
        app.panels[0].dir = d.join("@history");
        for a in [Action::Delete, Action::DeleteForever, Action::NewFolder, Action::Move, Action::Rename] {
            app.act(a);
            assert!(app.dialog.is_none(), "{a:?} asked");
            assert_eq!(app.status.as_deref(), Some(t!("history.read_only").as_str()), "{a:?}");
        }
        app.panels[0].dir = d.clone();
        // A password that did not open it: the box says so.
        app.dialog = Some(Dialog::Input { title: String::new(), label: t!("archive.locked_label"), value: String::new(), prompt: Prompt::Peek(d.join("x.zip").join("a")) });
        app.asked_again();
        assert!(matches!(&app.dialog, Some(Dialog::Input { label, .. }) if *label == t!("archive.locked_again")));
        app.dialog = None;
        // An editor that is not installed: the status line names it, nothing runs.
        app.cfg.editor = Some("no-such-editor-coxswain --wait".into());
        let at = app.panels[0].entries.iter().position(|e| e.name == "a.txt").unwrap();
        app.panels[0].cursor = at;
        app.act(Action::Edit);
        assert!(app.run.is_none());
        assert!(app.status.as_deref().is_some_and(|s| s.contains("no-such-editor-coxswain")), "{:?}", app.status);
        // The viewer's reading thread may still hold a.txt open: Windows refuses to remove it then.
        let _ = std::fs::remove_dir_all(d);
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
        while app.job.is_some() && t.elapsed() < coxswain_core::test_limit(Duration::from_secs(10)) {
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
            while app.job.is_some() && t.elapsed() < coxswain_core::test_limit(Duration::from_secs(10)) {
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
        while app.job.is_some() && t.elapsed() < coxswain_core::test_limit(Duration::from_secs(10)) {
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
        let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(10));
        while !app.panels[0].entries.iter().any(|e| e.name == "b.txt") && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
            app.follow_disk();
        }
        assert!(app.panels[0].entries.iter().any(|e| e.name == "b.txt"), "read again");
        assert_eq!(app.panel().current().map(|e| e.name.as_str()), Some("a.txt"), "the cursor stays");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn git_is_read_again_only_for_the_panels_a_change_concerns() {
        let d = std::env::temp_dir().join(format!("coxswain-test-gitsides-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        let mut app = app(d.join("a"), d.join("b"));
        assert_eq!(git_sides(&app.panels, &[d.join("a/x.txt")]), [0]);
        assert_eq!(git_sides(&app.panels, &[d.join("b")]), [1]);
        assert_eq!(git_sides(&app.panels, &[d.join("c.txt")]), Vec::<usize>::new());
        // Both folders in one repository: a change in either, or in its `.git`, is both's.
        app.panels[0].git = Some(git::Status { root: d.clone(), ..Default::default() });
        app.panels[1].git = Some(git::Status { root: d.clone(), ..Default::default() });
        assert_eq!(git_sides(&app.panels, &[d.join(".git/index")]), [0, 1]);
        // A read under way is not started twice: it is marked to run once more. The reads
        // the app started itself must be over first, or one ends between the two lines.
        let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(10));
        while !app.git_busy.lock().unwrap().is_empty() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        app.git_busy.lock().unwrap().insert(d.join("a"), false);
        app.refresh_git(&[0]);
        assert_eq!(app.git_busy.lock().unwrap().get(&d.join("a")), Some(&true));
        let _ = std::fs::remove_dir_all(d);
    }

    /// A folder not listed within `list_wait` (a million files) does not hold the keys: the
    /// panel shows `..` and the status line says it is being read, until the listing comes.
    #[test]
    fn a_slow_folder_is_listed_on_a_thread() {
        let d = std::env::temp_dir().join(format!("coxswain-test-slow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("big/inner")).unwrap();
        std::fs::write(d.join("big/a.txt"), "a").unwrap();
        let mut app = app(d.clone(), d.clone());
        app.list_wait = Duration::ZERO;
        // The listing waits until the panel has been looked at meanwhile.
        let hold = Arc::new(std::sync::RwLock::new(()));
        let held = hold.write().unwrap();
        app.list_hold = Some(hold.clone());
        app.cd(0, d.join("big"));
        let names = |app: &App| app.panels[0].entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&app), [".."], "only the way out while it is read");
        assert!(app.status.is_some(), "the status line says it is being read");
        drop(held);
        let (dir, keep, r) = app.list_rx.recv_timeout(coxswain_core::test_limit(Duration::from_secs(10))).unwrap();
        app.listed(dir, keep, r);
        assert_eq!(names(&app), ["..", "inner", "a.txt"]);
        assert!(app.status.is_none(), "and no longer says so");
        let _ = std::fs::remove_dir_all(d);
    }

    /// The app starts drawing at once on a folder slow to list: both panels show `..` and the
    /// status line says it is being read, keys answer, and one listing fills both panels.
    #[test]
    fn the_first_panels_are_listed_on_a_thread() {
        let d = std::env::temp_dir().join(format!("coxswain-test-slowstart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("big/inner")).unwrap();
        std::fs::write(d.join("big/a.txt"), "a").unwrap();
        LIST_WAIT.set(Duration::ZERO);
        // The listing waits until the panels have been looked at meanwhile.
        let hold = Arc::new(std::sync::RwLock::new(()));
        let held = hold.write().unwrap();
        LIST_HOLD.set(Some(hold.clone()));
        let mut app = app(d.join("big"), d.join("big"));
        LIST_WAIT.set(Duration::from_secs(10));
        LIST_HOLD.set(None);
        let names = |app: &App, side: usize| app.panels[side].entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!((names(&app, 0), names(&app, 1)), (vec!["..".to_string()], vec!["..".to_string()]), "only the way out while it is read");
        assert!(app.status.is_some(), "the status line says it is being read");
        app.key_typed(Key::new(KeyCode::Tab, false, false, false));
        assert_eq!(app.active, 1, "a key answers meanwhile");
        drop(held);
        let (dir, keep, r) = app.list_rx.recv_timeout(coxswain_core::test_limit(Duration::from_secs(10))).unwrap();
        app.listed(dir, keep, r);
        assert_eq!(names(&app, 0), ["..", "inner", "a.txt"]);
        assert_eq!(names(&app, 1), ["..", "inner", "a.txt"], "one listing, both panels");
        assert!(app.status.is_none(), "and no longer says so");
        assert!(app.list_rx.recv_timeout(Duration::from_millis(300)).is_err(), "the folder was listed once");
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
            let (dir, keep, r) = app.list_rx.recv_timeout(coxswain_core::test_limit(Duration::from_secs(10))).unwrap();
            app.listed(dir, keep, r);
        }
        assert_eq!(app.panels[0].dir, d.join("sub"), "not a repository: back to the folder");
        assert!(app.status.is_some(), "and says why");
        // The app's threads (sizes, git) may still hold the folder open on Windows.
        let _ = std::fs::remove_dir_all(d);
    }

    /// Quit while a job runs waits for it and says so; a second Quit leaves anyway (a job
    /// hung on a dead network mount).
    #[test]
    fn a_second_quit_leaves_a_hung_job() {
        let d = crate::test_dir("panels");
        let mut app = app(d.clone(), d.clone());
        let (_tx, rx) = mpsc::channel();
        app.job = Some(Job { op: Some(Transfer::Copy), src: vec![d.join("a.txt")], dst: d, password: None, select: None, line: "busy".into(), rx, stop: None });
        app.act(Action::Quit);
        assert!(app.quit && app.job.is_some(), "the first waits");
        assert!(app.status.as_deref().is_some_and(|s| s.contains("F10")), "{:?}", app.status);
        app.act(Action::Quit);
        assert!(app.quit && app.job.is_none(), "the second leaves");
    }

    /// Quit while Make PDF searchable runs stops it and waits for it to end.
    #[test]
    fn quit_stops_make_pdf_searchable_and_waits() {
        let d = crate::test_dir("panels");
        let mut app = app(d.clone(), d);
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.searchable = Some(Searchable { names: vec!["scan.pdf".into()], at: 0, line: String::new(), shown: String::new(), started: Instant::now(), stop: stop.clone(), rx });
        app.act(Action::Quit);
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed), "stopped");
        assert!(app.quit && app.searchable.is_some(), "and waited for");
        tx.send(SearchableMsg::Done(vec![Err("stopped".into())])).unwrap();
        app.poll_searchable();
        assert!(app.searchable.is_none(), "then the app can leave");
    }

    /// A job whose thread ended without a word (a panic in it) is let go and said, so the app
    /// can still quit.
    #[test]
    fn a_job_whose_thread_died_is_let_go() {
        let d = crate::test_dir("panels");
        let mut app = app(d.clone(), d.clone());
        let (tx, rx) = mpsc::channel();
        tx.send(JobMsg::At(0)).unwrap();
        drop(tx);
        app.job = Some(Job { op: Some(Transfer::Copy), src: vec![d.join("a.txt")], dst: d, password: None, select: None, line: "busy".into(), rx, stop: None });
        app.poll_job();
        assert!(app.job.is_none());
        assert!(matches!(app.dialog, Some(Dialog::Message { .. })), "and says so");
    }

    /// Esc on the progress line stops the file operation: the line says it is stopping, and
    /// then how far it got.
    #[test]
    fn esc_on_the_progress_line_stops_the_job() {
        let d = crate::test_dir("panels");
        let mut app = app(d.clone(), d.clone());
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.job = Some(Job { op: Some(Transfer::Copy), src: vec![d.join("a.txt")], dst: d.clone(), password: None, select: None, line: "busy".into(), rx, stop: Some(stop.clone()) });
        assert!(app.job.as_ref().unwrap().shown().contains("Esc"), "the line says Esc stops it");
        app.key_typed(Key::new(KeyCode::Esc, false, false, false));
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed), "stopped");
        assert!(app.status.as_ref().is_some_and(|s| s.contains(&t!("searchable.stopping"))), "{:?}", app.status);
        let line = coxswain_core::undo::stopped_line(coxswain_core::undo::Op::Copy, 1, 3);
        tx.send(JobMsg::Done(vec![], coxswain_core::undo::Record::new(coxswain_core::undo::Kind::Copy), Some(line.clone()))).unwrap();
        app.poll_job();
        assert!(app.job.is_none());
        assert_eq!(app.status.as_deref(), Some(line.as_str()));
        assert!(line.contains('1') && line.contains('3'), "{line}");
    }

    /// A folder not in a commit: back to the commit, which is read on a thread too, and the
    /// reason stays on the status line.
    #[test]
    fn a_folder_not_in_a_commit_says_why_when_it_walks_up() {
        let d = std::env::temp_dir().join(format!("coxswain-test-commit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "a").unwrap();
        let git = |args: &[&str]| coxswain_core::tools::command("git").arg("-C").arg(&d).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output();
        let made = git(&["init", "-q"]).is_ok_and(|o| o.status.success()) && git(&["add", "."]).is_ok_and(|o| o.status.success()) && git(&["commit", "-q", "-m", "one"]).is_ok_and(|o| o.status.success());
        if !made {
            return println!("no git");
        }
        let hash = String::from_utf8(git(&["rev-parse", "HEAD"]).unwrap().stdout).unwrap();
        let commit = history::path(&d, Some(hash.trim()));
        let mut app = app(d.clone(), d.clone());
        let gone = commit.join("no-such-folder");
        app.cd(0, gone.clone());
        while app.panels[0].dir == gone {
            let (dir, keep, r) = app.list_rx.recv_timeout(coxswain_core::test_limit(Duration::from_secs(10))).unwrap();
            app.listed(dir, keep, r);
        }
        let why = app.panels[0].error.clone();
        assert!(why.is_some());
        assert_eq!(app.panels[0].dir, commit, "back to the commit");
        // The commit, listed on its thread.
        let (dir, keep, r) = app.list_rx.recv_timeout(coxswain_core::test_limit(Duration::from_secs(10))).unwrap();
        app.listed(dir, keep, r);
        assert!(!app.panels[0].entries.is_empty(), "the commit is listed");
        assert_eq!(app.status, why, "the reason stays on the status line");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn find_keys_switch_kind_scope_and_view() {
        let d = crate::test_dir("panels");
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
        let d = crate::test_dir("panels");
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

    /// Games inside a zip (and a zip in it) open on their board with F3 and Enter, as outside:
    /// a ChessBase archive, a ChessBase and a SCID database with the files beside them, PGN, SGF.
    #[test]
    fn games_in_an_archive_open_as_games() {
        use coxswain_core::archive;
        let d = std::env::temp_dir().join(format!("coxswain-test-archive-games-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let files = d.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let chess = Path::new(env!("CARGO_MANIFEST_DIR")).join("../coxswain-core/src/chess");
        let mut sources = vec![];
        for name in ["Games.cbv", "Games.cbh", "Games.cbg", "Games.cba", "Games.cbp", "Games.cbt", "test.si4", "test.sg4", "test.sn4", "hand.pgn"] {
            let dir = if name.starts_with("Games") { "cbh-test" } else { "scid-test" };
            std::fs::copy(chess.join(dir).join(name), files.join(name)).unwrap();
            sources.push(files.join(name));
        }
        std::fs::write(files.join("go.sgf"), "(;GM[1]SZ[9];B[ee];W[cc])").unwrap();
        sources.push(files.join("go.sgf"));
        archive::create(&files.join("inner.zip"), &[files.join("Games.cbv")]).unwrap();
        sources.push(files.join("inner.zip"));
        let zip = d.join("games.zip");
        archive::create(&zip, &sources).unwrap();
        let mut app = app(d.clone(), d.clone());
        let opens = |app: &mut App, dir: PathBuf, name: &str, a: Action| -> PathBuf {
            app.cd(0, dir);
            let at = app.panels[0].entries.iter().position(|e| e.name == name).unwrap_or_else(|| panic!("{name} listed"));
            app.panels[0].cursor = at;
            app.dialog = None;
            app.act(a);
            match app.dialog.take() {
                Some(Dialog::Chess(v)) => v.path.clone(),
                Some(Dialog::Go(v)) => v.path.clone(),
                _ => panic!("{name} ({a:?}): not on its board; said {:?}", app.status),
            }
        };
        let first = |p: &Path| {
            let lib = coxswain_core::chess::Library::open(p).unwrap();
            let end = Instant::now() + coxswain_core::test_limit(Duration::from_secs(20));
            while !lib.progress().done && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            lib.row(0).map(|r| r.white).unwrap_or_default()
        };
        for a in [Action::View, Action::Open] {
            assert_eq!(first(&opens(&mut app, zip.clone(), "Games.cbv", a)), "Morphy, Paul");
            assert_eq!(first(&opens(&mut app, zip.clone(), "Games.cbh", a)), "Morphy, Paul", "its other files came along");
            assert!(!first(&opens(&mut app, zip.clone(), "test.si4", a)).is_empty(), "its other files came along");
            assert!(!first(&opens(&mut app, zip.clone(), "hand.pgn", a)).is_empty());
            opens(&mut app, zip.clone(), "go.sgf", a);
            assert_eq!(first(&opens(&mut app, zip.join("inner.zip"), "Games.cbv", a)), "Morphy, Paul");
        }
        let _ = std::fs::remove_dir_all(d);
    }

    /// A 7z split into parts with one missing: Enter and F3 say which on the status line,
    /// whole over its lines at 80 columns, where the panel's info line cuts it.
    #[test]
    fn a_missing_part_is_said_whole() {
        use ratatui::backend::TestBackend;
        let d = std::env::temp_dir().join(format!("coxswain-test-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Its start says its list of contents lies past the part there: the second is missing.
        let mut head = b"7z\xBC\xAF\x27\x1C\x00\x04\0\0\0\0".to_vec();
        head.extend(5000u64.to_le_bytes());
        head.extend(100u64.to_le_bytes());
        head.extend([0u8; 4]);
        head.resize(1000, 0);
        std::fs::write(d.join("6d.7z.001"), &head).unwrap();
        std::fs::write(d.join("6d.7z.003"), b"rest").unwrap();
        let mut app = app(d.clone(), d.clone());
        let at = app.panels[0].entries.iter().position(|e| e.name == "6d.7z.001").unwrap();
        app.panels[0].cursor = at;
        app.act(Action::View);
        assert!(app.dialog.is_none(), "not its bytes");
        let said = app.status.clone().unwrap_or_default();
        assert_eq!(said, t!("archive.part_missing", "name" => "6d.7z.002"));
        app.status = None;
        app.act(Action::Open);
        assert_eq!(app.panels[0].dir, d, "back in its folder");
        assert_eq!(app.status.as_deref(), Some(said.as_str()));
        let mut term = ratatui::Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rows: Vec<String> = (0..24).map(|y| (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect();
        assert_eq!(rows[21..23].join(" "), said, "{rows:#?}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    #[cfg(unix)]
    fn an_interrupt_meant_for_the_command_leaves_the_app_running() {
        // The child interrupts its parent (this process); ignored here, the test goes on.
        run_in(Line::from("kill -INT $PPID".to_string()).command(), Path::new("/"), false).unwrap();
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
    fn a_file_dropped_on_the_terminal_asks_copy_or_move_and_text_is_typed() {
        let d = std::env::temp_dir().join(format!("coxswain-test-paste-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("here")).unwrap();
        let f = d.join("a b.txt");
        std::fs::write(&f, "a").unwrap();
        let mut app = App::new(Config::default(), d.join("here"), d.clone()).unwrap();
        app.on_paste("ls");
        assert_eq!(app.cmdline, "ls");
        app.cmdline.clear();
        app.on_paste(&format!("'{}' ", f.display()));
        assert!(matches!(&app.dialog, Some(Dialog::Menu { feature: "drag_and_drop", items, .. }) if matches!(&items[0].run, MenuRun::Drop(p, false) if p == &[f.clone()])));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Find's rows are rebuilt every frame: a cursor past the last one (rows gone under it)
    /// goes back onto one, it never stops the app.
    #[test]
    fn finds_cursor_past_the_rows_is_brought_back() {
        use ratatui::backend::TestBackend;
        let d = crate::test_dir("panels");
        let cfg = Config { check_updates: false, ..Config::default() };
        let index = crate::test_index(&cfg);
        let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
        let hit = |n: &str| coxswain_core::index::Hit { path: d.join(n), is_dir: false, snippet: None, similar: None };
        let mut found = Found::default();
        found.names.hits = vec![hit("a.txt"), hit("b.txt")];
        found.names.total = 2;
        app.dialog = Some(Dialog::Search { query: "txt".into(), chip: Kind::All, here: false, found, cursor: 99, offset: 0, show: Show::List });
        let mut term = ratatui::Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let Some(Dialog::Search { query, chip, found, cursor, .. }) = &app.dialog else { panic!() };
        let rows = app.find_rows(query, *chip, found);
        assert!(*cursor < rows.len() && rows[*cursor].selectable(), "{cursor} of {}", rows.len());
    }

    /// A menu on a terminal of a few rows draws what fits, it never stops the app.
    #[test]
    fn a_menu_on_a_tiny_terminal_draws() {
        use ratatui::backend::TestBackend;
        let d = crate::test_dir("panels");
        let cfg = Config { check_updates: false, ..Config::default() };
        let index = crate::test_index(&cfg);
        let mut app = App::with_index(cfg, d.clone(), d, index).unwrap();
        let items = vec![MenuItem { key: "1".into(), label: "One".into(), run: MenuRun::Say(String::new()), group: None }];
        app.dialog = Some(Dialog::Menu { title: "M".into(), filter: String::new(), items, cursor: 0, direct: false, feature: "" });
        for h in 1..=5 {
            let mut term = ratatui::Terminal::new(TestBackend::new(40, h)).unwrap();
            term.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
        assert_eq!(menu_visible(&[MenuItem { key: "1".into(), label: "Copy".into(), run: MenuRun::Say(String::new()), group: None }], "COP"), [0]);
    }

    #[test]
    fn a_measured_folder_sorts_by_size_and_the_cursor_stays() {
        let d = std::env::temp_dir().join(format!("coxswain-test-size-sort-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["big", "mid", "small"] {
            std::fs::create_dir_all(d.join(f)).unwrap();
        }
        std::fs::write(d.join("f.txt"), "x").unwrap();
        let cfg = Config { check_updates: false, folder_sizes: false, ..Config::default() };
        let index = crate::test_index(&cfg);
        let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
        let names = |app: &App| app.panels[0].entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        let p = &mut app.panels[0];
        p.sort = SortKey::Size;
        p.resort();
        assert_eq!(names(&app), ["..", "big", "mid", "small", "f.txt"], "nothing measured: by name");
        app.panels[0].cursor = 1;
        app.panels[0].toggle_mark(1);
        for (f, n) in [("big", 5u64 << 30), ("mid", 50 << 20), ("small", 1 << 20)] {
            app.sizes_tx.send((d.clone(), d.join(f), n)).unwrap();
        }
        let mut state = app.index.state();
        app.tick(&mut state);
        assert_eq!(app.panels[0].marked_bytes, 5 << 30, "a marked folder counts its measured size");
        app.panels[0].resort_due = Some(Instant::now() - Duration::from_secs(1));
        app.tick(&mut state);
        assert_eq!(names(&app), ["..", "big", "mid", "small", "f.txt"]);
        app.sizes_tx.send((d.clone(), d.join("small"), 9 << 30)).unwrap();
        app.tick(&mut state);
        assert!(app.panels[0].resort_due.is_some(), "not at every arrival");
        app.panels[0].resort_due = Some(Instant::now() - Duration::from_secs(1));
        app.tick(&mut state);
        assert_eq!(names(&app), ["..", "small", "big", "mid", "f.txt"]);
        assert_eq!(app.panels[0].current().map(|e| e.name.as_str()), Some("big"), "the cursor stays on its folder");
        assert_eq!(app.panels[0].marked_bytes, 5 << 30);
        // Windows keeps the folder while the app watches it: let it go first, and leave a
        // folder still held (the temporary folder's own) rather than fail.
        drop(app);
        let _ = std::fs::remove_dir_all(&d);
    }

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

/// Chinese, Japanese and Korean letters take two columns: panels, the key bar, Find and a dialog must
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

    /// A name with a newline, a tab or an escape in it is drawn with `?` there: the bars stay
    /// straight, and no control character reaches the terminal.
    #[cfg(unix)]
    #[test]
    fn control_characters_in_names_never_reach_the_terminal() {
        let d = std::env::temp_dir().join(format!("coxswain-test-ctrl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["new\nline.txt", "tab\there.txt", "esc\u{1b}]2;x\u{7}.txt", "plain.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let cfg = Config { check_updates: false, ..Config::default() };
        let index = Client::with(None, &cfg.search, |_| {});
        let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
        let s = draw(&mut app);
        same_columns(&s, "│", 2..18);
        assert!(s.iter().flatten().all(|c| !c.chars().any(char::is_control)));
        assert!(s.iter().any(|r| r.concat().contains("new?line.txt")), "shown as ls shows it");
        // The delete question names it the same way, on one line.
        let at = app.panels[0].entries.iter().position(|e| e.name.starts_with("new")).unwrap();
        app.panels[0].cursor = at;
        app.act(Action::Delete);
        let s = draw(&mut app);
        assert!(s.iter().any(|r| r.concat().contains("\"new?line.txt\"")));
        assert!(s.iter().flatten().all(|c| !c.chars().any(char::is_control)));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn chinese_japanese_and_korean_line_up() {
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
        for lang in ["ja", "ko", "zh-TW", "zh-SG"] {
            coxswain_core::i18n::set_language(lang);
            let cfg = Config { check_updates: false, ..Config::default() };
            let index = crate::test_index(&cfg);
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
            // The key bar: each key number in its own slot, F12 (Settings) in the eleventh.
            let bar = &s[23];
            for n in 1..=9 {
                assert_eq!(bar[(n - 1) * 7], n.to_string(), "{lang}: {}", bar.concat());
            }
            assert_eq!(bar[70..72].concat(), "12", "{lang}: {}", bar.concat());
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
                // The bar runs down to the rule over the explanation, which takes the lines it needs.
                let rule = (1..23).find(|&y| s[y].iter().any(|c| c == "┴")).unwrap();
                same_columns(&s, "│", 1..rule);
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

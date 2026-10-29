//! Settings in the terminal app: the areas of `coxswain_core::settings` on the left, an area's
//! options on the right, the selected one explained at the bottom with its costs. Every change is
//! written to config.toml at once, its comments kept, and used straight away.

use crate::ui::{dstyle, fit, fit_left, frame, sty};
use crate::{App, Dialog, Run};
use coxswain_core::config::{Action, Config, Group, Key, KeyCode, Theme};
use coxswain_core::notices::Notice;
use coxswain_core::settings::{self as cs, Area, Level, Opt, Step};
use coxswain_core::t;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use crate::ui::Cols;
use coxswain_core::chess::play;

/// The areas, in the order of the list on the left.
pub const AREAS: [Area; 7] = Area::ALL;

/// Finding files' details, by group: its heading, the name `--settings=<name>` opens it at, and
/// its options. A test checks that every option of the area is in one.
const GROUPS: &[(&str, &str, &[&str])] = &[
    ("settings.group.reads", "reads", &["search_text", "search_archives", "search_archives_everywhere", "search_history", "search_commit_changes", "search_commit_changes_in", "search_cloud", "cloud_read", "search_tika", "search_whisper", "whisper_model", "text_max_size", "max_results"]),
    ("settings.group.folders", "folders", &["text_roots", "names_only", "text_exclude", "name_roots", "name_exclude", "watch"]),
    ("settings.group.meaning", "meaning", &["search_meaning", "meaning_engine", "meaning_url", "meaning_model", "meaning_key_env", "meaning_device", "long_files", "long_files_everything", "long_files_sample"]),
    ("settings.group.ask", "ask", &["ask_model", "ask_code_model", "ask_think", "ask_context"]),
];

/// Looks, by group: its heading (none for what both apps use), its name, its options. A test
/// checks that every option of the area is in one.
const LOOKS: [(Option<&str>, &str, &[&str]); 3] = [
    (None, "looks_both", &["language", "glyphs"]),
    (Some("settings.theme_tui"), "looks_tui", &["tui_theme"]),
    (Some("settings.theme_gui"), "looks_gui", &["theme", "font", "mono_font", "icon_font", "font_size", "line_height"]),
];

/// Text options that are a switch: (name, value when on, value when off).
const SWITCHES: &[(&str, &str, &str)] = &[("search_cloud", "all", "local-only"), ("meaning_device", "cpu", "auto")];

/// Number options: (name, least, most, scale): `text_max_size` is kept in bytes, typed in MB.
const NUMBERS: &[(&str, f64, f64, f64)] = &[("text_max_size", 1.0, 4096.0, 1_048_576.0), ("max_results", 10.0, 1_000_000.0, 1.0), ("ask_context", 2048.0, 131_072.0, 1.0), ("preview_timeout", 10.0, 3600.0, 1.0), ("chess_engine_time", 100.0, 60_000.0, 1.0), ("chess_engine_lines", 1.0, 5.0, 1.0), ("chess_review_time", 100.0, 10_000.0, 1.0), ("chess_engine_depth", 1.0, 99.0, 1.0), ("chess_review_depth", 1.0, 99.0, 1.0), ("chess_engine_nodes", 1000.0, 10_000_000_000.0, 1.0), ("chess_review_nodes", 1000.0, 10_000_000_000.0, 1.0), ("font_size", 9.0, 28.0, 1.0), ("line_height", 1.2, 3.0, 1.0)];

/// Lists of names and patterns, not of folders.
const WORDS: &[&str] = &["text_exclude", "name_exclude"];

/// One row on the right.
pub enum Row {
    /// A heading, and the name it opens at; never selected.
    Head(String, &'static str),
    /// A line of the status block with its next step.
    Status(cs::Line),
    Level(Level),
    /// The search setup guide.
    SetUp,
    Opt(&'static Opt),
    /// Background reading with the session.
    Service,
    /// Read only: a label, a value and what it means.
    Info(String, String, String),
    /// Overview: an area and how it stands.
    Go(Area, String),
    /// What's new: a version and what it brought. Enter shows it whole in the viewer.
    News(coxswain_core::notices::Change),
    Notice(Notice),
    /// The first-run guide.
    Guide,
    /// config.toml in the editor at `[keys]`.
    EditKeys,
    /// *Suggest a better translation*: a new GitHub issue in the browser.
    Translate,
    /// A program that reads more (tesseract …), whether it is there, and the line that installs it.
    Tool(String, bool, Option<String>),
    /// A model chosen that is a poor choice here, and the better one (Enter takes it).
    Poor(coxswain_core::setup::Poor),
    /// A built-in model on the disk: Enter shows its folder, Delete deletes it, U unloads it.
    Model(coxswain_core::models::Entry),
    /// Delete every built-in model not in use, and the bytes that frees.
    DeleteUnused(u64),
    /// Disk use: a place Coxswain keeps things. Enter shows it in the panel, Delete clears it.
    Disk(coxswain_core::disk::Item),
    /// Also clear the built-in models not in use (`false`), or what Coxswain caused outside its
    /// folder (`true`), with *Clear everything that can be built again*; the bytes they hold.
    Tick(bool, u64),
    /// Clear everything that can be built again, and the bytes that frees.
    ClearAll(u64),
    /// A Subversion or CVS server said yes to: Delete forgets it.
    Server(String),
    /// Preview tools: a kind of file, what is installed, its image, the choice. Enter takes
    /// the next choice, C copies the install line, P pulls the image, Delete removes it.
    PreviewTool(coxswain_core::preview_tools::Row),
    /// Chess: a known engine, found or not (only looked for, never run). Enter chooses it, C
    /// copies the line that installs it.
    Engine(play::Listed),
    /// Chess: *Test engine*: runs the chosen engine and reads its name and options.
    EngineTest,
    /// Chess: one of the engine's own options. Enter changes it, Delete gives it back the
    /// engine's default.
    EngineOpt(play::Shown),
    /// Chess: every option of the engine back to its own default.
    EngineReset,
}

impl Row {
    /// What the row is, kept while rows come and go around it.
    fn key(&self) -> String {
        match self {
            Row::Head(_, id) => format!("head:{id}"),
            Row::Status(l) => format!("status:{:?}", l.part),
            Row::Level(l) => format!("level:{}", l.id()),
            Row::SetUp => "setup".into(),
            Row::Opt(o) => o.name.into(),
            Row::Service => "service".into(),
            Row::Info(label, ..) => format!("info:{label}"),
            Row::Go(area, _) => format!("go:{}", area.id()),
            Row::News(c) => format!("news:{}", c.version),
            Row::Notice(n) => format!("notice:{}", n.id),
            Row::Guide => "guide".into(),
            Row::EditKeys => "keys".into(),
            Row::Translate => "translate".into(),
            Row::Tool(name, ..) => format!("tool:{name}"),
            Row::Poor(p) => format!("poor:{}", p.ask),
            Row::Model(e) => format!("model:{}", e.id),
            Row::DeleteUnused(_) => "unused".into(),
            Row::Disk(i) => format!("disk:{}", i.id),
            Row::Server(server) => format!("server:{server}"),
            Row::Tick(outside, _) => format!("tick:{outside}"),
            Row::ClearAll(_) => "clear_all".into(),
            Row::PreviewTool(r) => format!("ptool:{}", r.kind),
            Row::Engine(e) => format!("engine:{}", e.program),
            Row::EngineTest => "engine_test".into(),
            Row::EngineOpt(o) => format!("eopt:{}", o.option.name),
            Row::EngineReset => "engine_reset".into(),
        }
    }

    fn selectable(&self) -> bool {
        !matches!(self, Row::Head(..))
    }

    /// The name `open_at` gives for it.
    fn id(&self) -> Option<&'static str> {
        match self {
            Row::Head(_, id) => Some(id),
            Row::Opt(o) => Some(o.name),
            _ => None,
        }
    }
}

/// What the keys do now.
pub enum Mode {
    Browse,
    /// A text or a number typed in place.
    Edit(String),
    /// A list's items, the cursor on one of them or on the field that adds one (`at == len`).
    List { at: usize, new: String },
    /// A choice's values, the cursor on one.
    Pick(usize),
    /// One of the chess engine's options typed in place (a number or a text).
    EngineEdit(String),
    /// Find a setting: the words, the cursor on a result.
    Find { query: String, at: usize },
    /// Another model reads every file's meaning again: why, and the change that waits for a yes.
    Confirm(String, Map<String, Value>),
    /// A built-in model in use is to be deleted: its folder's name, waiting for a yes.
    ConfirmDelete(String),
    /// A row of Disk use is to be cleared (its id; `CLEAR_ALL` for everything that can be built
    /// again), waiting for a yes.
    ConfirmClear(String),
}

/// `Mode::ConfirmClear` of *Clear everything that can be built again*.
const CLEAR_ALL: &str = "\u{1}all";

pub struct Settings {
    pub area: Area,
    pub cursor: usize,
    offset: usize,
    pub mode: Mode,
    /// The value in `Mode::Edit` or `Mode::EngineEdit` is still the one it opened with, shown
    /// selected: the first key typed replaces it, Backspace clears it.
    pub selected: bool,
    /// The config.toml written to (a test's own).
    pub path: Option<PathBuf>,
    /// What the last change did: saved, or why not.
    pub said: Option<(String, bool)>,
    /// Ask's test question: the answer on its way, then what it said.
    trial_rx: Option<mpsc::Receiver<Result<Duration, String>>>,
    trial: Option<(String, bool)>,
    /// The speed test of search by meaning: its report on its way.
    bench_rx: Option<mpsc::Receiver<Result<String, String>>>,
    /// Read when Settings opens: they ask the system.
    service: bool,
    notices: Vec<Notice>,
    /// What's new: the versions not read yet (else this one), with what they brought.
    news: Vec<coxswain_core::notices::Change>,
    /// The cursor's row when last drawn, by index and by `Row::key`: rows that come later (a
    /// poor choice found in the background) do not move the cursor off its row.
    anchor: (usize, String),
    /// Disk use, read when Privacy opens and after each clear; the container images come a
    /// moment later, from a thread.
    disk: Vec<coxswain_core::disk::Item>,
    disk_rx: Option<mpsc::Receiver<Vec<coxswain_core::disk::Item>>>,
    /// Also the models not in use, also what is outside: ticked for *Clear everything*.
    ticks: (bool, bool),
    /// The built-in models on the disk, with what was loaded when they were read: their sizes
    /// take a walk of each folder, so a drawing reads them only when that changed, and a key
    /// that deletes one, or saves a setting (which may change the one in use), forgets them.
    pub(crate) models: Option<(coxswain_core::models::Loaded, Vec<coxswain_core::models::Entry>)>,
    /// Preview tools, read when Previews opens, after a change and after a pull, by a thread:
    /// it asks every program and the container runtime.
    tools: Option<Vec<coxswain_core::preview_tools::Row>>,
    tools_rx: Option<mpsc::Receiver<(Vec<coxswain_core::preview_tools::Row>, Option<String>)>>,
    /// The servers a Subversion or CVS history may be read from (`AppState::history_servers`).
    servers: Vec<String>,
    /// Chess: the known engines, looked for when Previews opens; what *Test engine* read from
    /// the chosen one (on a thread), or why it could not.
    engines: Vec<play::Listed>,
    engine: Option<Result<play::EngineInfo, String>>,
    engine_rx: Option<mpsc::Receiver<Result<play::EngineInfo, String>>>,
}

/// How to install a search tool that is missing, asked of the system once per tool: it looks
/// for the package manager on the PATH.
pub(crate) fn install_line(name: &str) -> Option<String> {
    static LINES: std::sync::Mutex<Vec<(String, Option<String>)>> = std::sync::Mutex::new(Vec::new());
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, line)) = lines.iter().find(|(n, _)| n == name) {
        return line.clone();
    }
    let line = coxswain_core::tools::install(name);
    lines.push((name.to_string(), line.clone()));
    line
}

impl Settings {
    /// Settings, open at `section` (an area, an option, or a name of before the areas).
    pub fn open(app: &App, section: &str) -> Settings {
        let st = coxswain_core::state::AppState::load();
        let mut s = Settings {
            area: Area::Overview,
            cursor: 0,
            offset: 0,
            mode: Mode::Browse,
            selected: false,
            path: Config::path(),
            said: None,
            trial_rx: None,
            trial: None,
            bench_rx: None,
            service: coxswain_core::service::installed(),
            notices: coxswain_core::notices::all(&app.cfg, &app.index.status(), &st, true),
            news: news(&st),
            anchor: (0, String::new()),
            disk: vec![],
            disk_rx: None,
            ticks: (false, false),
            models: None,
            tools: None,
            tools_rx: None,
            servers: st.history_servers.clone(),
            engines: vec![],
            engine: None,
            engine_rx: None,
        };
        s.go(app, section);
        s
    }

    /// To `section`'s area, the cursor on its option or group.
    fn go(&mut self, app: &App, section: &str) {
        let (area, at) = cs::open_at(section);
        self.area = area;
        self.offset = 0;
        if area == Area::Privacy {
            self.load_disk(app);
        }
        if area == Area::Previews {
            self.load_tools(app, None);
            self.engines = play::listed(&app.cfg);
        }
        let rows = rows(app, self);
        let start = at.and_then(|at| rows.iter().position(|r| r.id() == Some(at))).unwrap_or(0);
        self.cursor = (start..rows.len()).find(|&i| rows[i].selectable()).unwrap_or(0);
    }

    /// Disk use now: at once without the container images, which a thread asks for.
    fn load_disk(&mut self, app: &App) {
        let (cfg, loaded) = (app.cfg.clone(), app.index.loaded());
        self.disk = coxswain_core::disk::items(&cfg, &loaded, false);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || tx.send(coxswain_core::disk::items(&cfg, &loaded, true)));
        self.disk_rx = Some(rx);
    }

    /// Preview tools read again by a thread; `pull` first pulls these images, and what that
    /// gave is said when the rows come.
    fn load_tools(&mut self, app: &App, pull: Option<Vec<String>>) {
        let cfg = app.cfg.preview.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let said = pull.map(|images| {
                let rt = coxswain_core::tools::container_runtime(&cfg).map(|r| r.1);
                let failed: Vec<String> = images.iter().filter_map(|i| rt.as_ref().ok_or_else(coxswain_core::tools::no_runtime).and_then(|rt| coxswain_core::images::pull(rt, i)).err()).collect();
                if failed.is_empty() { t!("ptools.pulled", "images" => images.iter().map(|i| coxswain_core::images::short(i)).collect::<Vec<_>>().join(", ")) } else { failed.join(" ") }
            });
            tx.send((coxswain_core::preview_tools::rows(&cfg), said))
        });
        self.tools_rx = Some(rx);
    }

    /// The answer to Ask's test question, when it has come.
    pub fn poll(&mut self) {
        if let Some(r) = self.engine_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.engine_rx = None;
            self.said = Some(match &r {
                Ok(i) => (t!("chess.engine_tested", "name" => &i.name), false),
                Err(e) => (e.clone(), true),
            });
            self.engine = Some(r);
        }
        if let Some((rows, said)) = self.tools_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.tools_rx = None;
            self.tools = Some(rows);
            if let Some(said) = said {
                self.said = Some((said, false));
            }
        }
        if let Some(items) = self.disk_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.disk_rx = None;
            self.disk = items;
        }
        if let Some(r) = self.trial_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.trial_rx = None;
            self.trial = Some(match r {
                Ok(d) => (t!("setup.try_done", "seconds" => format!("{:.1}", d.as_secs_f64())), false),
                Err(e) => (e, true),
            });
        }
        if let Some(r) = self.bench_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.bench_rx = None;
            self.said = Some(match r {
                Ok(text) => (text, false),
                Err(e) => (e, true),
            });
        }
    }
}

// ---------------------------------------------------------------- what an area holds

/// The versions not read yet, else this one.
fn news(st: &coxswain_core::state::AppState) -> Vec<coxswain_core::notices::Change> {
    let unread = coxswain_core::notices::unread(st);
    let changes = if unread.is_empty() { coxswain_core::notices::changes().into_iter().take(1).collect() } else { unread };
    changes.into_iter().take(5).collect()
}

/// What a version brought, without its links.
fn plain(c: &coxswain_core::notices::Change) -> String {
    c.parts.iter().map(|(t, _)| t.as_str()).collect()
}

/// The start of `text` that fits in `h` lines of `w`, cut at a word and ended with "…", and
/// `more` (how to see it whole) on a line of its own below.
fn cut(text: &str, more: &str, w: u16, h: u16, dim: Style) -> Paragraph<'static> {
    let make = |n: usize| {
        let head: String = text.chars().take(n).collect();
        let head = head.rsplit_once(' ').map_or(head.as_str(), |(a, _)| a).trim_end().to_string();
        Paragraph::new(vec![Line::from(format!("{head}…")), Line::from(Span::styled(more.to_string(), dim))]).wrap(Wrap { trim: true })
    };
    let (mut lo, mut hi) = (0, text.chars().count());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if make(mid).line_count(w) <= h as usize {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    make(lo)
}

/// The rows of the area shown.
pub fn rows(app: &App, s: &Settings) -> Vec<Row> {
    let cfg = &app.cfg;
    let opts = |area: Area| cs::OPTIONS.iter().filter(move |o| o.area == area).map(Row::Opt);
    let level_name = || cs::level(&cfg.search).map_or_else(|| t!("settings.level.custom"), |l| t!(&format!("settings.level.{}", l.id())));
    let mut v = vec![];
    match s.area {
        Area::Overview => {
            let lines = cs::status(&cfg.search, &app.index.status(), app.index.shared(), &[]);
            let search = std::iter::once(level_name()).chain(lines.iter().skip(1).map(|l| format!("{}: {}", l.label, l.text))).collect::<Vec<_>>().join(" · ");
            v.push(Row::Go(Area::Search, search));
            v.push(Row::Go(Area::Previews, choice_label("preview_prefer", &cfg.preview.prefer)));
            let lang = choice_label("language", &cfg.language);
            v.push(Row::Go(Area::Looks, format!("{} · {lang}", choice_label("tui_theme", &cfg.theme))));
            let out: Vec<String> = cs::outbound(cfg).into_iter().filter(|o| !o.local).map(|o| o.to).collect();
            v.push(Row::Go(Area::Privacy, if out.is_empty() { t!("settings.privacy_nothing") } else { out.join(", ") }));
            v.push(Row::SetUp);
            v.push(Row::Guide);
            v.push(Row::Head(t!("news.title"), "news"));
            v.extend(s.notices.iter().cloned().map(Row::Notice));
            v.extend(s.news.iter().cloned().map(Row::News));
        }
        Area::Search => {
            let poor = coxswain_core::setup::poor(&cfg.search, false);
            v.extend(cs::status(&cfg.search, &app.index.status(), app.index.shared(), &poor).into_iter().map(Row::Status));
            v.push(Row::Head(t!("settings.level"), "level"));
            v.extend(Level::ALL.into_iter().map(Row::Level));
            v.push(Row::SetUp);
            for (head, id, names) in GROUPS {
                v.push(Row::Head(t!(head), id));
                v.extend(names.iter().filter_map(|n| cs::find(n)).map(Row::Opt));
                // A poor choice of model is said in its group for as long as it stands.
                if matches!(*id, "ask" | "meaning") {
                    v.extend(poor.iter().filter(|p| p.ask == (*id == "ask")).cloned().map(Row::Poor));
                }
            }
            // The built-in models on the disk: what each is, its folder, whether it is loaded.
            v.push(Row::Head(t!("models.title"), "models"));
            let models = match &s.models {
                Some((_, models)) => models.clone(),
                None => coxswain_core::models::list(&cfg.search, &app.index.loaded()),
            };
            if models.is_empty() {
                v.push(Row::Info(t!("models.none"), String::new(), t!("models.hint")));
            }
            let unused = coxswain_core::models::unused_bytes(&models);
            v.extend(models.into_iter().map(Row::Model));
            if unused > 0 {
                v.push(Row::DeleteUnused(unused));
            }
            let tools = app.index.status().tools;
            if !tools.is_empty() {
                v.push(Row::Head(t!("settings.search_tools"), "tools"));
                v.extend(tools.into_iter().map(|(name, there)| {
                    let line = (!there).then(|| install_line(&name)).flatten();
                    Row::Tool(name, there, line)
                }));
            }
            v.push(Row::Head(t!("settings.group.background"), "background"));
            v.push(Row::Service);
        }
        Area::Keys => {
            v.push(Row::EditKeys);
            for g in Group::ALL {
                let acts: Vec<Action> = g.actions().filter(|a| !a.gui_only()).collect();
                if acts.is_empty() {
                    continue;
                }
                v.push(Row::Head(g.label(), "keys"));
                for a in acts {
                    let keys = cfg.keys.get(&a).map(|k| k.join(" · ")).filter(|k| !k.is_empty()).unwrap_or_else(|| "—".into());
                    v.push(Row::Info(a.label(), keys, t!("settings.keys_hint")));
                }
            }
        }
        Area::Privacy => {
            v.extend(opts(Area::Privacy));
            v.push(Row::Head(t!("settings.privacy_out"), "out"));
            let out = cs::outbound(cfg);
            if out.is_empty() {
                v.push(Row::Info(t!("settings.privacy_nothing"), String::new(), String::new()));
            }
            for o in out {
                let to = if o.local { format!("{} ({})", o.to, t!("settings.privacy_local")) } else { o.to };
                v.push(Row::Info(o.what.clone(), to, o.what));
            }
            v.push(Row::Head(t!("settings.history_servers"), "history_servers"));
            if s.servers.is_empty() {
                v.push(Row::Info(t!("settings.history_servers_none"), String::new(), t!("settings.history_servers_hint")));
            }
            v.extend(s.servers.iter().cloned().map(Row::Server));
            // Disk use: Coxswain's own folder, the built-in models, then what it caused outside.
            use coxswain_core::disk::{self, Kind};
            v.push(Row::Head(t!("disk.title"), "disk"));
            v.extend(s.disk.iter().filter(|i| matches!(i.kind, Kind::Own | Kind::Model)).cloned().map(Row::Disk));
            let models = disk::clearable_bytes(&s.disk, true, false) - disk::clearable_bytes(&s.disk, false, false);
            let out = disk::clearable_bytes(&s.disk, false, true) - disk::clearable_bytes(&s.disk, false, false);
            v.push(Row::ClearAll(disk::clearable_bytes(&s.disk, s.ticks.0, s.ticks.1)));
            if models > 0 {
                v.push(Row::Tick(false, models));
            }
            if out > 0 {
                v.push(Row::Tick(true, out));
            }
            let outside: Vec<&disk::Item> = s.disk.iter().filter(|i| i.kind == Kind::Outside).collect();
            if !outside.is_empty() {
                v.push(Row::Head(t!("disk.head.outside"), "disk_outside"));
                v.extend(outside.into_iter().cloned().map(Row::Disk));
            }
            // The user's own files Coxswain keeps aside: each removed on its own row only.
            let yours: Vec<&disk::Item> = s.disk.iter().filter(|i| i.kind == Kind::Yours).collect();
            if !yours.is_empty() {
                v.push(Row::Head(t!("disk.head.yours"), "disk_yours"));
                v.extend(yours.into_iter().cloned().map(Row::Disk));
            }
            v.push(Row::Head(t!("settings.paths"), "paths"));
            for (what, p) in Config::paths() {
                if let Some(p) = p {
                    let label = t!(&format!("settings.path.{}", what.replace(' ', "_")));
                    v.push(Row::Info(label, p.display().to_string(), p.display().to_string()));
                }
            }
            let version = t!("settings.version", "version" => coxswain_core::update::VERSION);
            v.push(Row::Info(version.clone(), String::new(), t!("settings.stored_in", "path" => s.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default())));
        }
        Area::Previews => {
            v.extend(opts(Area::Previews));
            v.push(Row::Head(t!("chess.engines_title"), "chess_engines"));
            v.extend(s.engines.iter().cloned().map(Row::Engine));
            v.push(Row::EngineTest);
            match &s.engine {
                Some(Ok(info)) => {
                    v.push(Row::Info(t!("chess.engine_tested", "name" => &info.name), String::new(), t!("chess.engine_options_hint")));
                    if info.options.is_empty() {
                        v.push(Row::Info(t!("chess.engine_options_none"), String::new(), t!("chess.engine_options_hint")));
                    }
                    v.extend(info.options.iter().cloned().map(Row::EngineOpt));
                    v.extend(info.ignored.iter().map(|(n, why)| Row::Info(format!("✗ {n}"), why.clone(), t!("chess.option_ignored_hint"))));
                    v.push(Row::EngineReset);
                }
                Some(Err(e)) => v.push(Row::Info(t!("chess.engine_test_failed"), e.clone(), e.clone())),
                None => {}
            }
            v.push(Row::Head(t!("ptools.title"), "preview_tools"));
            match &s.tools {
                Some(tools) => v.extend(tools.iter().cloned().map(Row::PreviewTool)),
                None => v.push(Row::Info(t!("common.loading"), String::new(), t!("ptools.hint"))),
            }
        }
        // What both apps share first; then each app's own under its name, so a desktop app's
        // setting never reads as part of the row above it.
        Area::Looks => {
            for (head, id, names) in LOOKS {
                if let Some(head) = head {
                    v.push(Row::Head(t!(head), id));
                }
                v.extend(names.iter().filter_map(|n| cs::find(n)).map(Row::Opt));
                // Right under the language: where to say a word reads wrong.
                if id == "looks_both" {
                    v.insert(v.iter().position(|r| matches!(r, Row::Opt(o) if o.name == "language")).map_or(v.len(), |i| i + 1), Row::Translate);
                }
            }
        }
        area => v.extend(opts(area)),
    }
    v
}

// ---------------------------------------------------------------- values

/// How an option is changed.
#[derive(Debug, PartialEq)]
enum Kind {
    Switch,
    /// A text that is a switch: its values when on and off.
    TextSwitch(&'static str, &'static str),
    Choice(Vec<(String, String)>),
    Number(f64, f64, f64),
    List,
    Text,
}

fn kind(o: &Opt, cfg: &Config, value: &Value) -> Kind {
    if let Some((_, on, off)) = SWITCHES.iter().find(|s| s.0 == o.name) {
        return Kind::TextSwitch(on, off);
    }
    if let Some(&(_, lo, hi, scale)) = NUMBERS.iter().find(|n| n.0 == o.name) {
        return Kind::Number(lo, hi, scale);
    }
    match value {
        Value::Bool(_) => Kind::Switch,
        Value::Array(_) => Kind::List,
        _ => match choices(o.name, cfg) {
            Some(c) => Kind::Choice(c),
            None => Kind::Text,
        },
    }
}

/// The values a choice offers, with their names.
fn choices(name: &str, cfg: &Config) -> Option<Vec<(String, String)>> {
    let pairs = |p: &[(&str, &str)]| p.iter().map(|(v, k)| (v.to_string(), if k.contains('.') { t!(k) } else { k.to_string() })).collect();
    Some(match name {
        "preview_prefer" => pairs(&[("auto", "settings.prefer_auto"), ("local", "settings.prefer_local"), ("container", "settings.prefer_container")]),
        "preview_container" => pairs(&[("auto", "settings.container_auto"), ("podman", "podman"), ("docker", "docker"), ("off", "settings.container_off")]),
        "image_updates" => pairs(&[("off", "settings.image_updates_off"), ("weekly", "settings.image_updates_weekly"), ("monthly", "settings.image_updates_monthly")]),
        "glyphs" => pairs(&[("nerd", "settings.glyphs_nerd"), ("ascii", "settings.glyphs_ascii")]),
        "right_click" => pairs(&[("mark", "settings.right_click_mark"), ("menu", "settings.right_click_menu")]),
        "long_files" => pairs(&[("sample", "settings.long_files.sample"), ("everything", "settings.long_files.everything")]),
        "chess_notation" => pairs(&[("figurines", "settings.chess_figurines"), ("letters", "settings.chess_letters")]),
        "chess_pieces" => pairs(&[
            ("auto", "settings.chess_pieces_auto"),
            ("pictures", "settings.chess_pieces_pictures"),
            ("octants", "settings.chess_pieces_octants"),
            ("sextants", "settings.chess_pieces_sextants"),
            ("quadrants", "settings.chess_pieces_quadrants"),
            ("halves", "settings.chess_pieces_halves"),
            ("figurines", "settings.chess_pieces_figurines"),
        ]),
        "chess_engine_limit" | "chess_review_limit" => pairs(&[("time", "settings.chess_limit_time"), ("depth", "settings.chess_limit_depth"), ("nodes", "settings.chess_limit_nodes")]),
        "chess_board" => pairs(&[("theme", "settings.chess_board_theme"), ("green", "settings.chess_board_green"), ("brown", "settings.chess_board_brown")]),
        "whisper_model" => coxswain_core::transcribe::choices(cfg),
        "meaning_engine" => vec![
            ("builtin".into(), t!("settings.meaning_builtin", "size" => cs::human(coxswain_core::meaning::size()))),
            ("ollama".into(), "Ollama".into()),
            ("openai".into(), t!("settings.meaning_openai")),
        ],
        // Every chat model Ask can take, in one list: the server's and the built-in ones, under
        // headings, then another one typed, and off.
        "ask_model" => {
            let look = coxswain_core::setup::look(false);
            let mut v = chat_list(coxswain_core::setup::ask_choices(&cfg.search, look.as_deref()), look.is_some());
            v.push((OTHER.to_string(), t!("settings.ask_other")));
            v.push((String::new(), t!("settings.ask_off")));
            v
        }
        // The model for code questions: why, Same as Ask, then the same list as Ask's.
        "ask_code_model" => {
            let look = coxswain_core::setup::look(false);
            let mut v = vec![(HEAD.to_string(), coxswain_core::setup::code_advice(&cfg.search, look.as_deref()).why), (String::new(), t!("settings.code_same"))];
            v.extend(chat_list(coxswain_core::setup::code_choices(&cfg.search, look.as_deref()), look.is_some()));
            v.push((OTHER.to_string(), t!("settings.ask_other")));
            v
        }
        // The git tools installed here, the first of them chosen when none is named; then a
        // command of one's own.
        "git_tool" => {
            let found = coxswain_core::handoff::installed();
            let auto = found.first().map_or_else(|| t!("settings.git_tool_terminal"), |n| n.to_string());
            let mut v = vec![(String::new(), t!("settings.git_tool_auto", "name" => auto))];
            v.extend(found.iter().map(|n| (n.to_string(), n.to_string())));
            v.push((OTHER.to_string(), t!("settings.git_tool_other")));
            v
        }
        "language" => std::iter::once(("auto".to_string(), t!("settings.language_auto"))).chain(coxswain_core::i18n::LANGUAGES.iter().map(|l| (l.code.to_string(), l.name.to_string()))).collect(),
        "theme" | "tui_theme" => {
            // Under the headings of their groups, as the desktop app lists them; your own last.
            let builtin = Theme::BUILTIN.iter().map(|(n, ..)| (n.to_string(), t!(&format!("theme.{n}"))));
            let own = cfg.themes.keys().filter(|n| Theme::BUILTIN.iter().all(|(b, ..)| b != n)).map(|n| (n.clone(), n.clone()));
            let (mut v, mut group) = (vec![], "");
            for (name, label) in builtin.chain(own) {
                let g = Theme::group(&name);
                if g != group {
                    group = g;
                    v.push((HEAD.to_string(), t!(group)));
                }
                v.push((name, label));
            }
            v
        }
        _ => return None,
    })
}

/// The values of Ask's list that are no model: a heading, the setup guide, a name to type.
const HEAD: &str = "\u{1}head";
const SET_UP: &str = "\u{1}setup";
const OTHER: &str = "\u{1}other";

/// Where a list of choices opens: on the value set; with Ask off, on the recommended model;
/// a model typed by name, on *Other*. Never on a heading.
fn pick_start(o: &Opt, cfg: &Config, value: &Value, c: &[(String, String)]) -> usize {
    let now = value.as_str().unwrap_or_default();
    let at = if o.name == "ask_model" && now.is_empty() {
        let best = coxswain_core::setup::recommend_ask(&cfg.search, coxswain_core::setup::look(false).as_deref());
        c.iter().position(|(v, _)| *v == best)
    } else {
        c.iter().position(|(v, _)| v == now).or_else(|| c.iter().position(|(v, _)| !now.is_empty() && v == OTHER))
    };
    pick_step(c, at.unwrap_or(0), 1)
}

/// `at` in a list of choices, moved on past headings in the direction `by` (back, when that
/// runs off the end).
fn pick_step(c: &[(String, String)], at: usize, by: isize) -> usize {
    let n = c.len() as isize;
    if n == 0 {
        return 0;
    }
    let at = (at as isize).min(n - 1);
    let dir = if by < 0 { -1 } else { 1 };
    let head = |i: isize| c[i as usize].0 == HEAD;
    for d in [dir, -dir] {
        let mut i = at;
        while (0..n).contains(&i) && head(i) {
            i += d;
        }
        if (0..n).contains(&i) {
            return i as usize;
        }
    }
    at as usize
}

/// A choice's value by its name: a value not in the list (a theme of the user's own) by itself.
pub(crate) fn choice_label(name: &str, value: &str) -> String {
    let cfg = Config::default();
    choices(name, &cfg).and_then(|c| c.into_iter().find(|(v, _)| v == value)).map_or_else(|| value.to_string(), |(_, l)| l)
}

/// A number as typed: a whole one without decimals.
/// What a chess engine's option takes, and its default.
pub(crate) fn option_about(o: &play::UciOption) -> String {
    let what = match o.kind.as_str() {
        "spin" => t!("chess.option_spin", "min" => o.min.unwrap_or(0), "max" => o.max.unwrap_or(0)),
        "check" => t!("chess.option_check"),
        "combo" => t!("chess.option_combo", "values" => o.vars.join(", ")),
        _ => t!("chess.option_string"),
    };
    format!("{what} · {}", t!("chess.engine_default", "value" => if o.default.is_empty() { "—" } else { o.default.as_str() }))
}

fn number_text(n: f64) -> String {
    // Kept as f32 in config.toml (1.9 reads back as 1.899999976…): two decimals at most.
    let n = (n * 100.0).round() / 100.0;
    if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n}") }
}

/// The option's value as the row shows it.
fn shown(o: &Opt, cfg: &Config, value: &Value) -> String {
    match (kind(o, cfg, value), value) {
        (Kind::Switch | Kind::TextSwitch(..), _) => String::new(),
        (Kind::Choice(c), Value::String(v)) => c.into_iter().find(|(x, _)| x == v).map_or_else(|| v.clone(), |(_, l)| l),
        (Kind::Number(_, _, scale), Value::Number(n)) => {
            // The label names the unit: "Largest file read (MB)".
            number_text(n.as_f64().unwrap_or(0.0) / scale)
        }
        (Kind::List, Value::Array(a)) if a.is_empty() => t!(if o.name == "text_roots" { "settings.search_roots_home" } else { "settings.search_names_only_none" }),
        (Kind::List, Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        (_, Value::String(v)) if v.is_empty() => match o.name {
            "editor" => "$EDITOR".into(),
            "viewer" => t!("setting.viewer.empty"),
            _ => String::new(),
        },
        (_, Value::String(v)) => v.clone(),
        (_, v) => v.to_string(),
    }
}

/// Whether a switch is on. As `kind` tells, without making a choice's list (which asks the
/// system: the git tools on the PATH).
fn on(o: &Opt, value: &Value) -> Option<bool> {
    if let Some((_, yes, _)) = SWITCHES.iter().find(|s| s.0 == o.name) {
        return Some(value.as_str() == Some(yes));
    }
    if NUMBERS.iter().any(|n| n.0 == o.name) {
        return None;
    }
    value.as_bool()
}

// ---------------------------------------------------------------- keys

impl App {
    /// A key in Settings. Saving happens here, at once.
    pub fn settings_key(&mut self, mut s: Box<Settings>, key: Key, action: Option<Action>) {
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
            _ => None,
        };
        let esc = key.code == KeyCode::Esc;
        s.said = None;
        let rows = rows(self, &s);
        let values = cs::values(&self.cfg);
        let opt = match rows.get(s.cursor) {
            Some(Row::Opt(o)) => Some(*o),
            _ => None,
        };
        let value = opt.map(|o| values[o.name].clone()).unwrap_or(Value::Null);
        // Editing needs the option under the cursor; the expects below hold by this.
        if opt.is_none() && matches!(s.mode, Mode::Edit(_) | Mode::List { .. } | Mode::Pick(_)) {
            s.mode = Mode::Browse;
        }
        match std::mem::replace(&mut s.mode, Mode::Browse) {
            Mode::Browse => match (key.code, ch) {
                _ if esc || action == Some(Action::Quit) => return,
                (KeyCode::Up, _) => s.cursor = step(&rows, s.cursor, -1),
                (KeyCode::Down, _) => s.cursor = step(&rows, s.cursor, 1),
                (KeyCode::PageUp, _) => s.cursor = step(&rows, s.cursor, -10),
                (KeyCode::PageDown, _) => s.cursor = step(&rows, s.cursor, 10),
                (KeyCode::Home, _) => s.cursor = step(&rows, 0, 0),
                (KeyCode::End, _) => s.cursor = step(&rows, rows.len().saturating_sub(1), 0),
                (KeyCode::Left | KeyCode::Right | KeyCode::Tab, _) => {
                    let i = AREAS.iter().position(|&a| a == s.area).unwrap_or(0);
                    let back = key.code == KeyCode::Left || key.shift;
                    let next = AREAS[(i + if back { AREAS.len() - 1 } else { 1 }) % AREAS.len()];
                    s.go(self, next.id());
                }
                (_, Some('/')) => s.mode = Mode::Find { query: String::new(), at: 0 },
                (KeyCode::Delete, _) => match rows.get(s.cursor) {
                    Some(Row::PreviewTool(r)) => self.tool_remove(&mut s, r),
                    Some(Row::EngineOpt(o)) => return self.engine_option(s, &o.option.name.clone(), None),
                    Some(Row::Model(e)) => return self.model_delete(s, &e.id.clone()),
                    Some(Row::Disk(i)) if i.bytes > 0 && i.clear => s.mode = Mode::ConfirmClear(i.id.clone()),
                    Some(Row::Status(l)) if l.also == Some(Step::StopTranscribing) => self.index.transcribe_pause(false, true),
                    Some(Row::Server(server)) => {
                        let mut st = coxswain_core::state::AppState::load();
                        st.history_servers.retain(|x| x != server);
                        coxswain_core::wc::allow(&st.history_servers);
                        s.said = Some(match st.save() {
                            Ok(()) => (t!("settings.history_server_forgotten", "server" => server), false),
                            Err(e) => (e.to_string(), true),
                        });
                        s.servers = st.history_servers;
                    }
                    _ => {}
                },
                (_, Some('c' | 'C')) if matches!(rows.get(s.cursor), Some(Row::Engine(_))) => {
                    if let Some(Row::Engine(e)) = rows.get(s.cursor) {
                        let line = e.install.clone().or_else(|| e.site.map(String::from)).unwrap_or_else(|| e.program.clone());
                        crate::guide::copy(&line);
                        s.said = Some((t!("guide.copied", "command" => line), false));
                    }
                }
                (_, Some('o' | 'O')) if matches!(rows.get(s.cursor), Some(Row::EngineOpt(_) | Row::EngineTest | Row::EngineReset)) => {
                    let key = match &s.engine {
                        Some(Ok(i)) => i.key.clone(),
                        _ => "Stockfish".into(),
                    };
                    match Config::prepare_chess_engine(&key) {
                        Ok((path, line)) => self.edit_at(&path, line),
                        Err(e) => s.said = Some((e, true)),
                    }
                }
                (KeyCode::Enter, _) | (_, Some(' ')) if matches!(rows.get(s.cursor), Some(Row::Engine(_) | Row::EngineTest | Row::EngineOpt(_) | Row::EngineReset)) => return self.engine_enter(s, &rows),
                (_, Some('c' | 'C')) if matches!(rows.get(s.cursor), Some(Row::PreviewTool(_))) => {
                    if let Some(Row::PreviewTool(r)) = rows.get(s.cursor) {
                        match &r.install {
                            Some(line) => {
                                crate::guide::copy(line);
                                s.said = Some((t!("guide.copied", "command" => line), false));
                            }
                            None => s.said = Some((t!("install.get", "program" => r.programs.first().map_or("", |p| p.name)), false)),
                        }
                    }
                }
                // No image: config.toml at `[preview.images]`, to name one.
                (_, Some('o' | 'O')) if matches!(rows.get(s.cursor), Some(Row::PreviewTool(r)) if !r.container) => {
                    if let Some(Row::PreviewTool(r)) = rows.get(s.cursor) {
                        match coxswain_core::config::Config::prepare_images(r.images.first().map_or(r.kind, |i| i.key)) {
                            Ok((path, line)) => self.edit_at(&path, line),
                            Err(e) => s.said = Some((e, true)),
                        }
                    }
                }
                (_, Some('p' | 'P')) if matches!(rows.get(s.cursor), Some(Row::PreviewTool(_))) => {
                    if let Some(Row::PreviewTool(r)) = rows.get(s.cursor) {
                        let images: Vec<String> = r.images.iter().filter(|i| !i.image.is_empty()).map(|i| i.image.clone()).collect();
                        if images.is_empty() {
                            s.said = Some((t!("ptools.container_none", "key" => r.images.first().map_or(r.kind, |i| i.key)), false));
                        } else {
                            s.said = Some((t!("convert.pulling", "image" => images.iter().map(|i| coxswain_core::images::short(i)).collect::<Vec<_>>().join(", ")), false));
                            s.load_tools(self, Some(images));
                        }
                    }
                }
                (_, Some('u' | 'U')) => {
                    if let Some(Row::Model(e)) = rows.get(s.cursor) {
                        s.said = Some(match (&e.loaded, e.what) {
                            (Some(_), coxswain_core::models::For::Meaning) => (t!("models.stays_loaded"), false),
                            (Some(_), _) => {
                                self.index.unload();
                                (t!("models.unloaded", "model" => e.name.as_str()), false)
                            }
                            (None, _) => (t!("models.not_loaded"), false),
                        });
                    }
                }
                (KeyCode::Enter, _) if opt.is_some() => {
                    let o = opt.unwrap();
                    s.selected = true;
                    s.mode = match kind(o, &self.cfg, &value) {
                        Kind::Text => Mode::Edit(value.as_str().unwrap_or_default().to_string()),
                        Kind::Number(_, _, scale) => Mode::Edit(number_text(value.as_f64().unwrap_or(0.0) / scale)),
                        Kind::List => Mode::List { at: 0, new: String::new() },
                        Kind::Choice(c) => Mode::Pick(pick_start(o, &self.cfg, &value, &c)),
                        Kind::Switch | Kind::TextSwitch(..) => return self.settings_space(s, &rows, &value),
                    };
                }
                (KeyCode::Enter, _) if matches!(rows.get(s.cursor), Some(Row::News(_))) => {
                    if let Some(Row::News(c)) = rows.get(s.cursor) {
                        return self.news_whole(s, c);
                    }
                }
                (KeyCode::Enter, _) | (_, Some(' ')) => return self.settings_space(s, &rows, &value),
                _ => {}
            },
            Mode::Edit(mut text) => match (key.code, ch) {
                _ if std::mem::take(&mut s.selected) && (ch.is_some() || key.code == KeyCode::Backspace) && !esc => {
                    text.clear();
                    text.extend(ch);
                    s.mode = Mode::Edit(text);
                }
                _ if esc => {}
                (KeyCode::Enter, _) => {
                    let o = opt.expect("editing an option");
                    let new = match kind(o, &self.cfg, &value) {
                        Kind::Number(lo, hi, scale) => match text.trim().replace(',', ".").parse::<f64>() {
                            Ok(n) if (lo..=hi).contains(&n) => {
                                // Only the font's sizes are kept with decimals (f32); the rest
                                // are whole numbers (bytes, counts, seconds).
                                let n = n * scale;
                                if matches!(o.name, "font_size" | "line_height") && n.fract() != 0.0 { Value::from((n * 100.0).round() / 100.0) } else { Value::from(n.round() as i64) }
                            }
                            _ => {
                                s.said = Some((t!("tui.settings.range", "name" => o.label(), "min" => number_text(lo), "max" => number_text(hi)), true));
                                s.mode = Mode::Edit(text);
                                return self.dialog = Some(Dialog::Settings(s));
                            }
                        },
                        _ => Value::from(text.trim()),
                    };
                    if new != value {
                        return self.settings_change(s, Map::from_iter([(o.name.to_string(), new)]));
                    }
                }
                (KeyCode::Backspace, _) => {
                    text.pop();
                    s.mode = Mode::Edit(text);
                }
                (KeyCode::Char('u'), None) if key.ctrl => s.mode = Mode::Edit(String::new()),
                (_, Some(c)) => {
                    text.push(c);
                    s.mode = Mode::Edit(text);
                }
                _ => s.mode = Mode::Edit(text),
            },
            Mode::EngineEdit(mut text) => {
                let Some(Row::EngineOpt(o)) = rows.get(s.cursor) else { return self.dialog = Some(Dialog::Settings(s)) };
                match (key.code, ch) {
                    _ if esc => {}
                    _ if std::mem::take(&mut s.selected) && (ch.is_some() || key.code == KeyCode::Backspace) => {
                        text.clear();
                        text.extend(ch);
                        s.mode = Mode::EngineEdit(text);
                    }
                    (KeyCode::Enter, _) => return self.engine_option(s, &o.option.name.clone(), Some(Value::String(text))),
                    (KeyCode::Backspace, _) => {
                        text.pop();
                        s.mode = Mode::EngineEdit(text);
                    }
                    (KeyCode::Char('u'), None) if key.ctrl => s.mode = Mode::EngineEdit(String::new()),
                    (_, Some(c)) => {
                        text.push(c);
                        s.mode = Mode::EngineEdit(text);
                    }
                    _ => s.mode = Mode::EngineEdit(text),
                }
            }
            Mode::List { mut at, mut new } => {
                let o = opt.expect("a list");
                let mut items: Vec<String> = value.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                let mut changed = false;
                match (key.code, ch) {
                    _ if esc => return self.dialog = Some(Dialog::Settings(s)),
                    (KeyCode::Up, _) => at = at.saturating_sub(1),
                    (KeyCode::Down, _) => at = (at + 1).min(items.len()),
                    (KeyCode::Delete, _) if at < items.len() => {
                        items.remove(at);
                        changed = true;
                    }
                    (KeyCode::Enter, _) if at == items.len() => {
                        let item = if WORDS.contains(&o.name) {
                            new.trim().to_string()
                        } else {
                            let base = self.panel().dir.clone();
                            let dir = coxswain_core::fs::resolve(&base, new.trim());
                            coxswain_core::fs::canonical(&dir).unwrap_or(dir).display().to_string()
                        };
                        if !item.is_empty() && !items.contains(&item) {
                            items.push(item);
                            changed = true;
                        }
                        new.clear();
                        at = items.len();
                    }
                    (KeyCode::Backspace, _) => {
                        new.pop();
                        at = items.len();
                    }
                    (_, Some(c)) => {
                        new.push(c);
                        at = items.len();
                    }
                    _ => {}
                }
                s.mode = Mode::List { at: at.min(items.len()), new };
                if changed {
                    return self.settings_change(s, Map::from_iter([(o.name.to_string(), Value::from(items))]));
                }
            }
            Mode::Pick(mut at) => {
                let o = opt.expect("a choice");
                let Kind::Choice(c) = kind(o, &self.cfg, &value) else { return self.dialog = Some(Dialog::Settings(s)) };
                match key.code {
                    _ if esc => {}
                    KeyCode::Up => s.mode = Mode::Pick(pick_step(&c, at.saturating_sub(1), -1)),
                    KeyCode::Down => s.mode = Mode::Pick(pick_step(&c, at + 1, 1)),
                    KeyCode::PageUp => s.mode = Mode::Pick(pick_step(&c, at.saturating_sub(10), -1)),
                    KeyCode::PageDown => s.mode = Mode::Pick(pick_step(&c, at + 10, 1)),
                    KeyCode::Enter if matches!(o.name, "ask_model" | "ask_code_model" | "git_tool") && c[at.min(c.len() - 1)].0.starts_with('\u{1}') => match c[at.min(c.len() - 1)].0.as_str() {
                        OTHER => {
                            s.selected = true;
                            s.mode = Mode::Edit(value.as_str().unwrap_or_default().to_string());
                        }
                        SET_UP => self.setup_guide(),
                        _ => s.mode = Mode::Pick(at),
                    },
                    // A built-in model not downloaded yet: downloaded on the plain terminal, then set.
                    KeyCode::Enter if o.name == "ask_model" && coxswain_core::chat::of(&c[at.min(c.len() - 1)].0).is_some_and(|m| !m.installed()) => {
                        self.run_self(&["--meaning", "ask", &c[at.min(c.len() - 1)].0]);
                    }
                    KeyCode::Enter => {
                        at = at.min(c.len() - 1);
                        if value.as_str() != Some(c[at].0.as_str()) {
                            return self.settings_change(s, Map::from_iter([(o.name.to_string(), Value::from(c[at].0.clone()))]));
                        }
                    }
                    _ => s.mode = Mode::Pick(at),
                }
            }
            Mode::Find { mut query, mut at } => {
                let found = find(&query);
                match (key.code, ch) {
                    _ if esc => {}
                    (KeyCode::Enter, _) => {
                        if let Some(o) = found.get(at) {
                            s.go(self, o.name);
                        }
                    }
                    (KeyCode::Up, _) => s.mode = Mode::Find { at: at.saturating_sub(1), query },
                    (KeyCode::Down, _) => s.mode = Mode::Find { at: (at + 1).min(found.len().saturating_sub(1)), query },
                    (KeyCode::Backspace, _) => {
                        query.pop();
                        s.mode = Mode::Find { query, at: 0 };
                    }
                    (_, Some(c)) => {
                        query.push(c);
                        at = 0;
                        s.mode = Mode::Find { query, at };
                    }
                    _ => s.mode = Mode::Find { query, at },
                }
            }
            Mode::Confirm(why, changes) => match key.code {
                KeyCode::Enter => return self.settings_save(s, changes),
                _ if esc => {}
                _ => s.mode = Mode::Confirm(why, changes),
            },
            Mode::ConfirmDelete(id) => match key.code {
                KeyCode::Enter => return self.model_delete(s, &id),
                _ if esc => {}
                _ => s.mode = Mode::ConfirmDelete(id),
            },
            Mode::ConfirmClear(id) => match key.code {
                KeyCode::Enter => return self.disk_clear(s, &id),
                _ if esc => {}
                _ => s.mode = Mode::ConfirmClear(id),
            },
        }
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Enter on a version under What's new: its whole text in the built-in viewer, Settings
    /// again when it closes.
    fn news_whole(&mut self, mut s: Box<Settings>, c: &coxswain_core::notices::Change) {
        let labels = [self.key_label(Action::Edit), self.key_label(Action::View)];
        match coxswain_core::notices::text_file(c).and_then(|f| crate::viewer::Viewer::open(&f, labels)) {
            Ok(v) => self.dialog = Some(Dialog::View { v: Box::new(v.wrapped()), back: Some(Box::new(Dialog::Settings(s))) }),
            Err(e) => {
                s.said = Some((e.to_string(), true));
                self.dialog = Some(Dialog::Settings(s));
            }
        }
    }

    /// Space (and Enter where there is nothing to type): a switch flips, a choice takes its next
    /// value, a step or a level is taken.
    fn settings_space(&mut self, mut s: Box<Settings>, rows: &[Row], value: &Value) {
        let set = |name: &str, v: Value| Map::from_iter([(name.to_string(), v)]);
        match rows.get(s.cursor) {
            // Tika's: `[search] tika` turns it on, it has no choice here.
            Some(Row::PreviewTool(r)) if !r.can_choose => {}
            Some(Row::PreviewTool(r)) => {
                // The next choice: Default, Installed program, Container, Off.
                // Container only when an image is set: without one it cannot run.
                let order: Vec<&str> = ["", "local", "container", "off"].into_iter().filter(|c| r.container || *c != "container").collect();
                let next = order[(order.iter().position(|c| *c == r.choice).unwrap_or(0) + 1) % order.len()];
                let Some(path) = s.path.clone() else {
                    s.said = Some((t!("err.no_config_folder"), true));
                    return self.dialog = Some(Dialog::Settings(s));
                };
                match coxswain_core::preview_tools::set_choice(&path, r.kind, next) {
                    Ok(cfg) => {
                        self.cfg = cfg;
                        s.said = Some((t!("settings.saved", "path" => path.display()), false));
                        s.load_tools(self, None);
                    }
                    Err(e) => s.said = Some((e, true)),
                }
                return self.dialog = Some(Dialog::Settings(s));
            }
            Some(Row::Opt(o)) if matches!(o.name, "ask_model" | "ask_code_model") => {
                if let Kind::Choice(c) = kind(o, &self.cfg, value) {
                    s.mode = Mode::Pick(pick_start(o, &self.cfg, value, &c));
                }
            }
            Some(Row::Opt(o)) => {
                let changes = match kind(o, &self.cfg, value) {
                    Kind::Switch => set(o.name, Value::Bool(!value.as_bool().unwrap_or(false))),
                    Kind::TextSwitch(yes, no) => set(o.name, Value::from(if value.as_str() == Some(yes) { no } else { yes })),
                    Kind::Choice(c) => {
                        let next = c.iter().position(|(v, _)| value.as_str() == Some(v)).map_or(0, |i| (i + 1) % c.len());
                        let i = pick_step(&c, next, 1);
                        set(o.name, Value::from(c[i].0.clone()))
                    }
                    _ => return self.dialog = Some(Dialog::Settings(s)),
                };
                return self.settings_change(s, changes);
            }
            Some(Row::Level(l)) => return self.settings_level(s, *l),
            Some(Row::SetUp) => self.setup_guide(),
            Some(Row::Guide) => return self.dialog = Some(Dialog::Guide(Box::default())),
            Some(Row::Translate) => s.said = Some((Self::suggest_translation(), false)),
            Some(Row::EditKeys) => match coxswain_core::config::Config::prepare_keys() {
                Ok((path, line)) => self.edit_at(&path, line),
                Err(e) => s.said = Some((e, true)),
            },
            Some(Row::Tool(_, false, Some(line))) => {
                crate::guide::copy(line);
                s.said = Some((t!("guide.copied", "command" => line), false));
            }
            // A poor choice on its line: Enter takes the better one.
            Some(Row::Status(line)) if line.poor.as_ref().is_some_and(|p| p.button.is_some()) => return self.settings_change(s, line.poor.clone().map(|p| p.changes).unwrap_or_default()),
            Some(Row::Status(line)) => match line.step {
                Some(Step::TurnOn) => return self.settings_level(s, Level::Text),
                Some(Step::Start) => self.index.restart(),
                Some(Step::ReadNow) => self.index.index_now(),
                Some(Step::Pause) => self.index.transcribe_pause(true, false),
                Some(Step::Resume) => self.index.transcribe_pause(false, false),
                Some(Step::StopTranscribing) => self.index.transcribe_pause(false, true),
                Some(Step::SetUp) => self.setup_guide(),
                Some(Step::TryIt) if s.trial_rx.is_none() => {
                    let (tx, rx) = mpsc::channel();
                    let cfg = self.cfg.search.clone();
                    std::thread::spawn(move || tx.send(coxswain_core::setup::try_ask(&cfg)));
                    s.trial_rx = Some(rx);
                    s.trial = Some((t!("common.loading"), false));
                }
                Some(Step::Bench) if s.bench_rx.is_none() => {
                    let (tx, rx) = mpsc::channel();
                    let (cfg, status) = (self.cfg.search.clone(), self.index.status());
                    std::thread::spawn(move || tx.send(coxswain_core::meaning::bench_report(&cfg, Some(&status))));
                    s.bench_rx = Some(rx);
                    s.said = Some((t!("common.loading"), false));
                }
                _ => {}
            },
            Some(Row::Service) => {
                let done = if s.service { coxswain_core::service::uninstall() } else { coxswain_core::tools::this_app().and_then(|exe| coxswain_core::service::install(&exe)) };
                match done {
                    Ok(()) => {
                        s.service = !s.service;
                        self.index.restart();
                    }
                    Err(e) => s.said = Some((e.to_string(), true)),
                }
            }
            Some(Row::Go(area, _)) => s.go(self, area.id()),
            Some(Row::Poor(p)) if p.button.is_some() => return self.settings_change(s, p.changes.clone()),
            Some(Row::Poor(_)) => self.setup_guide(),
            // Its folder in the active panel; Settings closes.
            Some(Row::Model(e)) => {
                let dir = e.folder.clone();
                return self.cd(self.active, dir);
            }
            // The file or folder in the active panel, the cursor on it; Settings closes.
            Some(Row::Disk(i)) => {
                let Some(p) = i.paths.iter().find(|p| p.exists()).or(i.paths.first()).cloned() else { return self.dialog = Some(Dialog::Settings(s)) };
                let dir = if p.is_dir() { p.clone() } else { p.parent().map(PathBuf::from).unwrap_or_default() };
                self.cd(self.active, dir);
                if !p.is_dir()
                    && let Some(n) = p.file_name()
                {
                    self.panel_mut().select_name(&n.to_string_lossy());
                }
                return;
            }
            Some(Row::Tick(outside, _)) => {
                let t = if *outside { &mut s.ticks.1 } else { &mut s.ticks.0 };
                *t = !*t;
            }
            Some(Row::ClearAll(n)) if *n > 0 => s.mode = Mode::ConfirmClear(CLEAR_ALL.into()),
            Some(Row::DeleteUnused(_)) => {
                let models = coxswain_core::models::list(&self.cfg.search, &self.index.loaded());
                let gone: Vec<_> = models.iter().filter(|e| !e.in_use).collect();
                let mut freed = 0;
                for e in &gone {
                    match coxswain_core::models::delete(e, &self.index) {
                        Ok(()) => freed += e.bytes,
                        Err(err) => s.said = Some((format!("{}: {err}", e.folder.display()), true)),
                    }
                }
                s.models = None;
                if s.said.is_none() {
                    s.said = Some((t!("models.deleted_all", "n" => gone.len(), "size" => cs::human(freed)), false));
                }
            }
            Some(Row::Notice(n)) => {
                let mut st = coxswain_core::state::AppState::load();
                coxswain_core::notices::dismiss(&mut st, &n.id);
                let _ = st.save();
                let at = n.settings.unwrap_or("overview");
                s.notices.retain(|x| x.id != n.id);
                s.go(self, at);
            }
            _ => {}
        }
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Delete a built-in model; the one in use first gives way to the recommended choice, saved,
    /// and the line says which.
    fn model_delete(&mut self, mut s: Box<Settings>, id: &str) {
        use coxswain_core::models;
        let all = models::list(&self.cfg.search, &self.index.loaded());
        let Some(e) = all.iter().find(|e| e.id == id) else { return self.dialog = Some(Dialog::Settings(s)) };
        let instead = models::instead(&self.cfg.search, e, coxswain_core::setup::look(false).as_deref());
        if let (Some((changes, _)), Some(path)) = (&instead, s.path.clone())
            && let Err(err) = self.save_options(&path, changes)
        {
            s.said = Some((err, true));
            return self.dialog = Some(Dialog::Settings(s));
        }
        s.said = Some(match models::delete(e, &self.index) {
            Ok(()) => {
                let mut said = t!("models.deleted", "model" => e.name.as_str(), "size" => cs::human(e.bytes));
                if let Some((_, then)) = instead {
                    said = format!("{said} {then}");
                }
                (said, false)
            }
            Err(err) => (format!("{}: {err}", e.folder.display()), true),
        });
        s.models = None;
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Clear a row of Disk use (a model the settings use gives way first, as in Built-in
    /// models), or everything that can be built again with what is ticked; the line says what
    /// it freed.
    fn disk_clear(&mut self, mut s: Box<Settings>, id: &str) {
        use coxswain_core::disk;
        if id == CLEAR_ALL {
            let (freed, failed) = disk::clear_all(&s.disk, s.ticks.0, s.ticks.1, &self.cfg, &self.index);
            s.said = Some(match failed.is_empty() {
                true => (t!("disk.cleared_all", "size" => cs::human(freed)), false),
                false => (failed.join(" · "), true),
            });
        } else if let Some(i) = s.disk.iter().find(|i| i.id == id).cloned() {
            if i.kind == disk::Kind::Model {
                self.model_delete(s, id);
                let Some(Dialog::Settings(mut s)) = self.dialog.take() else { return };
                s.load_disk(self);
                return self.dialog = Some(Dialog::Settings(s));
            }
            s.said = Some(match disk::clear(&i, &self.cfg, &self.index) {
                Ok(n) => (disk::cleared(&i.name, n), false),
                Err(e) => (e, true),
            });
        }
        s.load_disk(self);
        s.models = None;
        self.dialog = Some(Dialog::Settings(s));
    }

    /// A search level: what it needs turned on, or the setup guide when a model is to be chosen.
    fn settings_level(&mut self, s: Box<Settings>, l: Level) {
        match cs::level_changes(l, &self.cfg.search, coxswain_core::meaning::installed()) {
            Some(changes) => self.settings_change(s, changes),
            None => {
                self.setup_guide();
                self.dialog = Some(Dialog::Settings(s));
            }
        }
    }

    /// A change: asked about first when another model would read every file again; the guide
    /// instead when meaning is turned on without its model.
    fn settings_change(&mut self, mut s: Box<Settings>, changes: Map<String, Value>) {
        let text = s.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let new = match cs::apply(&text, &changes).and_then(|t| Config::parse_saved(&text, &t)) {
            Ok(c) => c,
            Err(e) => {
                s.said = Some((e, true));
                return self.dialog = Some(Dialog::Settings(s));
            }
        };
        if new.search.meaning && !self.cfg.search.meaning && new.search.meaning_engine == "builtin" && !coxswain_core::meaning::installed() {
            self.setup_guide();
            return self.dialog = Some(Dialog::Settings(s));
        }
        let st = self.index.status();
        let index = &self.index;
        let why = coxswain_core::meaning::change_notice(&self.cfg.search, &new.search, st.meaning_done, st.meaning_passages)
            // What is kept of long files: what the change reads, adds or frees, first.
            .or_else(|| coxswain_core::long_files::change_notice(&self.cfg.search, &new.search, st.meaning_ms_per_passage, |r| index.long_files_cost(r)))
            // The changes of commits: their room and time said first, or what turning them off frees.
            .or_else(|| coxswain_core::store::changes_notice(&self.cfg.search, &new.search));
        if let Some(why) = why {
            s.mode = Mode::Confirm(why, changes);
            return self.dialog = Some(Dialog::Settings(s));
        }
        self.settings_save(s, changes)
    }

    /// Written to config.toml and used at once; the helper restarts when it reads one of them.
    /// Enter on a chess engine's row: a known engine chosen, *Test engine* run, an option
    /// changed (a switch flipped, a combo's next value, else typed), every option reset.
    fn engine_enter(&mut self, mut s: Box<Settings>, rows: &[Row]) {
        match rows.get(s.cursor) {
            Some(Row::Engine(e)) => {
                let program = e.program.clone();
                for l in &mut s.engines {
                    l.chosen = l.program == program;
                }
                s.engine = None;
                return self.settings_change(s, Map::from_iter([("chess_engine".to_string(), Value::from(play::choose(&program)))]));
            }
            Some(Row::EngineTest) if s.engine_rx.is_none() => {
                let cfg = self.cfg.clone();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || tx.send(play::info(&cfg)));
                s.engine_rx = Some(rx);
                s.said = Some((t!("chess.engine_testing"), false));
            }
            Some(Row::EngineOpt(o)) => {
                let now = o.value.as_ref().map_or_else(|| o.option.default.clone(), |v| v.as_str().map_or_else(|| v.to_string(), String::from));
                let name = o.option.name.clone();
                match o.option.kind.as_str() {
                    "check" => return self.engine_option(s, &name, Some(Value::Bool(now != "true"))),
                    "combo" if !o.option.vars.is_empty() => {
                        let vars = &o.option.vars;
                        let next = vars.iter().position(|v| v.eq_ignore_ascii_case(&now)).map_or(0, |i| (i + 1) % vars.len());
                        return self.engine_option(s, &name, Some(Value::String(vars[next].clone())));
                    }
                    _ => {
                        s.selected = true;
                        s.mode = Mode::EngineEdit(now);
                    }
                }
            }
            Some(Row::EngineReset) => {
                if let (Some(Ok(info)), Some(path)) = (&s.engine, s.path.clone()) {
                    match play::reset_options(&path, &info.key) {
                        Ok(cfg) => {
                            self.cfg = cfg;
                            s.said = Some((t!("chess.engine_reset_done", "engine" => &info.key), false));
                        }
                        Err(e) => s.said = Some((e, true)),
                    }
                }
                if let Some(Ok(i)) = &mut s.engine {
                    i.refresh(&self.cfg.chess);
                }
            }
            _ => {}
        }
        self.dialog = Some(Dialog::Settings(s));
    }

    /// The chess engine's option `name` set to `value` (None: the engine's default) in
    /// config.toml, after `play::set_option` checked it; a value refused is typed again.
    fn engine_option(&mut self, mut s: Box<Settings>, name: &str, value: Option<Value>) {
        let (Some(Ok(info)), Some(path)) = (&s.engine, s.path.clone()) else { return self.dialog = Some(Dialog::Settings(s)) };
        match play::set_option(&path, info, name, value.as_ref()) {
            Ok(cfg) => {
                self.cfg = cfg;
                s.said = Some((t!("settings.saved", "path" => path.display()), false));
                if let Some(Ok(i)) = &mut s.engine {
                    i.refresh(&self.cfg.chess);
                }
            }
            Err(e) => {
                s.said = Some((e, true));
                if let Some(Value::String(text)) = value {
                    s.mode = Mode::EngineEdit(text);
                }
            }
        }
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Delete on a preview tool: its pulled images removed.
    fn tool_remove(&mut self, s: &mut Settings, r: &coxswain_core::preview_tools::Row) {
        let Some((_, rt)) = coxswain_core::tools::container_runtime(&self.cfg.preview) else {
            return s.said = Some((coxswain_core::tools::no_runtime(), true));
        };
        let gone: Vec<Result<String, String>> = r.images.iter().filter(|i| i.pulled.is_some()).map(|i| coxswain_core::images::remove(&rt, &i.image).map(|_| coxswain_core::images::short(&i.image).to_string())).collect();
        s.said = Some(match gone.iter().find_map(|g| g.as_ref().err()) {
            Some(e) => (e.clone(), true),
            None if gone.is_empty() => (t!("settings.image_not_pulled"), false),
            None => (t!("ptools.removed", "images" => gone.into_iter().flatten().collect::<Vec<_>>().join(", ")), false),
        });
        s.load_tools(self, None);
    }

    fn settings_save(&mut self, mut s: Box<Settings>, changes: Map<String, Value>) {
        let Some(path) = s.path.clone() else {
            s.said = Some((t!("err.no_config_folder"), true));
            return self.dialog = Some(Dialog::Settings(s));
        };
        s.said = Some(match self.save_options(&path, &changes) {
            Ok(()) => (t!("settings.saved", "path" => path.display()), false),
            Err(e) => (e, true),
        });
        s.models = None;
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Options written to the config.toml at `path` (comments kept) and used at once; the search
    /// helper restarts when it reads one of them.
    pub fn save_options(&mut self, path: &std::path::Path, changes: &Map<String, Value>) -> Result<(), String> {
        let cfg = cs::save_to(path, changes)?;
        self.theme = cfg.theme();
        self.glyphs = cfg.glyphs();
        let hidden = cfg.show_hidden != self.cfg.show_hidden;
        self.cfg = cfg;
        self.slow.0 = None;
        // The panels list by the app's own flag (Hidden files flips it for the run): a new setting
        // replaces it and shows at once.
        if hidden {
            self.show_hidden = self.cfg.show_hidden;
            self.reload();
        }
        if changes.keys().any(|k| cs::find(k).is_some_and(|o| o.restarts_helper())) {
            self.index.restart();
        }
        Ok(())
    }

    /// The search setup guide, on the plain terminal.
    pub fn setup_guide(&mut self) {
        self.run_self(&["--setup-search"]);
    }

    /// This program with `args`, on the plain terminal, waiting for a key when it ends.
    fn run_self(&mut self, args: &[&str]) {
        let exe = coxswain_core::tools::this_app().map(|p| p.display().to_string()).unwrap_or_else(|_| "coxswain".into());
        let mut line = coxswain_core::tools::Line::default();
        line.cmd = std::iter::once(exe.as_str()).chain(args.iter().copied()).map(|w| line.word(w)).collect::<Vec<_>>().join(" ");
        self.run = Some(Run::Shell { line, typed: false, dir: self.panel().dir.clone(), wait: true });
    }
}

/// The selectable row `delta` rows on from `at` (at the ends: the first or last one).
fn step(rows: &[Row], at: usize, delta: isize) -> usize {
    let ok: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].selectable()).collect();
    if ok.is_empty() {
        return 0;
    }
    let now = ok.iter().position(|&i| i >= at).unwrap_or(ok.len() - 1);
    // A cursor on a heading counts as on the row after it.
    let now = if ok[now] > at && delta > 0 { now as isize - 1 } else { now as isize };
    ok[(now + delta).clamp(0, ok.len() as isize - 1) as usize]
}

/// The options whose label, explanation or config key hold `query`.
fn find(query: &str) -> Vec<&'static Opt> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    cs::OPTIONS.iter().filter(|o| [o.label(), o.hint(), o.name.to_string(), o.path.join(".")].iter().any(|x| x.to_lowercase().contains(&q))).collect()
}

// ---------------------------------------------------------------- drawing

/// What the row under the cursor is (or what a question waits for), as the bottom box shows it.
fn explain(app: &App, s: &Settings, rows: &[Row]) -> Vec<Span<'static>> {
    let cfg = &app.cfg;
    let level = cs::level(&cfg.search);
    let base = dstyle(&app.theme);
    let dim = base.add_modifier(Modifier::DIM);
    let red = base.fg(Color::Red);
    let mut text: Vec<Span> = vec![];
    match &s.mode {
        // A change of what is kept of long files, or of the vectors' model.
        Mode::Confirm(why, changes) if changes.keys().any(|k| k.starts_with("long_files") || k.starts_with("search_commit_changes")) => text.push(Span::styled(format!("{why} Enter: {} · Esc: {}", t!("common.apply"), t!("common.cancel")), red)),
        Mode::Confirm(why, _) => text.push(Span::styled(format!("{why} Enter: {} · Esc: {}", t!("settings.meaning_change_go"), t!("settings.meaning_change_keep")), red)),
        Mode::ConfirmDelete(id) => {
            let name = rows.iter().find_map(|r| match r {
                Row::Model(e) if e.id == *id => Some(e.name.clone()),
                _ => None,
            });
            text.push(Span::styled(format!("{} {}", t!("models.confirm", "model" => name.unwrap_or_default()), t!("models.confirm_tui")), red));
        }
        // What goes, exactly, before it goes.
        Mode::ConfirmClear(id) => {
            let goes: Vec<String> = if id == CLEAR_ALL {
                s.disk.iter().filter(|i| coxswain_core::disk::clearable_bytes(std::slice::from_ref(*i), s.ticks.0, s.ticks.1) > 0).map(|i| i.name.clone()).collect()
            } else {
                s.disk.iter().filter(|i| i.id == *id).map(|i| format!("{} ({}) · {}", i.paths.first().map(|p| p.display().to_string()).unwrap_or_else(|| i.name.clone()), cs::human(i.bytes), i.cost)).collect()
            };
            text.push(Span::styled(format!("{} {}. {}", t!("disk.confirm"), goes.join(", "), t!("disk.confirm_tui")), red));
        }
        Mode::Find { query, at } => {
            if let Some(o) = find(query).get(*at) {
                text.push(Span::raw(o.hint()));
            }
        }
        _ => match rows.get(s.cursor) {
            Some(Row::Opt(o)) => {
                text.push(Span::raw(o.hint()));
                if o.name == "ask_context" {
                    let a = coxswain_core::setup::context_advice(&cfg.search, coxswain_core::setup::look(false).as_deref());
                    if let Some(n) = a.suggest {
                        text.push(Span::styled(format!(" {} {}", a.why, t!("setup.context_use", "n" => n)), red));
                    }
                }
                for c in o.costs {
                    text.push(Span::styled(format!(" [{}]", c.label()), if *c == cs::Cost::Leaves { red } else { dim }));
                }
                let key = if o.path.len() > 1 { format!("[{}] {}", o.path[..o.path.len() - 1].join("."), o.path[o.path.len() - 1]) } else { o.path[0].to_string() };
                text.push(Span::styled(format!("  {key}"), dim));
            }
            Some(Row::Level(l)) => {
                text.push(Span::raw(t!(&format!("settings.level.{}.hint", l.id()))));
                for c in l.costs() {
                    text.push(Span::styled(format!(" [{}]", c.label()), if *c == cs::Cost::Leaves { red } else { dim }));
                }
                if level.is_none() {
                    text.push(Span::styled(format!(" {}", t!("settings.level.custom")), red));
                }
            }
            Some(Row::Status(l)) if l.poor.is_some() => text.push(Span::styled(l.poor.as_ref().map(|p| p.text.clone()).unwrap_or_default(), red)),
            Some(Row::Status(l)) => {
                let note = match (l.part, &s.trial) {
                    (cs::Part::Ask, Some((said, bad))) => Some((said.clone(), *bad)),
                    _ => l.note.clone().map(|n| (n, l.bad)),
                };
                match note {
                    Some((n, bad)) => text.push(Span::styled(n, if bad { red } else { base })),
                    None => text.push(Span::raw(l.text.clone())),
                }
                if let Some(also) = l.also {
                    text.push(Span::styled(format!("  {}", t!("settings.step.also_tui", "step" => also.label())), dim));
                }
            }
            Some(Row::SetUp) => text.push(Span::raw(t!("setup.open_hint"))),
            Some(Row::Guide) => text.push(Span::raw(t!("guide.show_again_hint"))),
            Some(Row::EditKeys) => text.push(Span::raw(t!("settings.keys_hint"))),
            Some(Row::Translate) => text.push(Span::raw(t!("settings.suggest_translation_hint"))),
            Some(Row::Tool(_, false, Some(line))) => text.push(Span::raw(t!("install.copy_tui", "command" => line))),
            Some(Row::Tool(name, false, None)) => text.push(Span::raw(t!("install.get", "program" => name))),
            Some(Row::Tool(name, true, _)) => text.push(Span::raw(t!(&format!("settings.search_tool_{name}")))),
            Some(Row::Service) => text.push(Span::raw(t!("setup.service_hint"))),
            Some(Row::Info(_, _, hint)) => text.push(Span::raw(hint.clone())),
            Some(Row::News(c)) => text.push(Span::raw(plain(c))),
            Some(Row::Go(_, value)) => text.push(Span::raw(value.clone())),
            Some(Row::Notice(n)) => text.push(Span::raw(n.text.clone())),
            Some(Row::Poor(p)) => text.push(Span::raw(p.text.clone())),
            Some(Row::Model(e)) => {
                text.push(Span::raw(e.folder.display().to_string()));
                text.push(Span::styled(format!("  {}", t!("models.keys_tui")), dim));
            }
            Some(Row::DeleteUnused(_)) => text.push(Span::raw(t!("models.hint"))),
            Some(Row::Server(_)) => text.push(Span::raw(t!("settings.history_servers_hint"))),
            Some(Row::Disk(i)) => {
                let at = i.paths.first().map(|p| format!(" · {}", p.display())).unwrap_or_default();
                text.push(Span::raw(format!("{} {}: {}", i.what, t!("disk.cost_label"), i.cost)));
                text.push(Span::styled(at, dim));
            }
            Some(Row::Tick(true, _)) => text.push(Span::raw(t!("disk.outside_hint"))),
            Some(Row::Tick(false, _)) => text.push(Span::raw(t!("models.hint"))),
            Some(Row::ClearAll(_)) => text.push(Span::raw(t!("disk.clear_all_hint"))),
            Some(Row::Engine(e)) => {
                let mut parts: Vec<String> = e.note.iter().cloned().collect();
                if let Some(line) = &e.install {
                    parts.push(format!("{} {line}", t!("install.with")));
                }
                parts.extend(e.site.map(String::from));
                parts.retain(|p| !p.is_empty());
                text.push(Span::raw(parts.join(" · ")));
            }
            Some(Row::EngineTest) => text.push(Span::raw(t!("chess.engine_test_hint"))),
            Some(Row::EngineReset) => text.push(Span::raw(t!("chess.engine_reset_hint"))),
            Some(Row::EngineOpt(o)) => text.push(Span::raw(option_about(&o.option))),
            Some(Row::PreviewTool(r)) => {
                let installed: Vec<String> = r.programs.iter().filter(|p| p.path.is_some()).map(|p| p.version.as_ref().map_or_else(|| p.name.to_string(), |v| format!("{} {v}", p.name))).collect();
                let mut parts = vec![if installed.is_empty() { t!("ptools.installed_none") } else { t!("ptools.installed", "programs" => installed.join(", ")) }];
                if let (true, Some(line)) = (installed.is_empty(), &r.install) {
                    parts.push(format!("{} {line}", t!("install.with")));
                }
                for i in &r.images {
                    let state = match (i.pulled, &i.download) {
                        (Some(b), _) => t!("settings.image_pulled", "size" => cs::human(b)),
                        (None, Some(d)) => format!("{} ({d})", t!("settings.image_not_pulled")),
                        (None, None) => t!("settings.image_not_pulled"),
                    };
                    parts.push(if i.image.is_empty() { t!("ptools.container_none", "key" => i.key) } else { t!("ptools.container", "image" => coxswain_core::images::short(&i.image), "state" => state) });
                }
                // ClamAV reads any file: no endings, and no separator left dangling.
                parts.push(r.exts.clone());
                parts.retain(|p| !p.is_empty());
                text.push(Span::raw(parts.join(" · ")));
            }
            _ => {}
        },
    }
    text
}

/// A row's label and its value, the value in its column from `lw` of `w`. A label longer than
/// its column goes on over the lines below, under its first word, so none is cut; without a
/// value (a switch, a heading) the label has the whole width.
fn label_lines(label: &str, value: &str, st: Style, w: usize, lw: usize) -> Vec<Line<'static>> {
    let lw = if value.is_empty() { w } else { lw };
    // The mark before the words: " [x] ", "     ", " ✓ ".
    let spaces = label.trim_start_matches(' ');
    let words = [" [x] ", " [ ] "]
        .iter()
        .find_map(|m| label.strip_prefix(m))
        .or_else(|| spaces.split_once(' ').filter(|(mark, _)| ["✓", "✗", "·", "!"].contains(mark)).map(|(_, rest)| rest))
        .unwrap_or(spaces);
    let lead = label.width() - words.width();
    let room = (lw - 1).saturating_sub(lead);
    if label.width() < lw || room < 8 {
        return vec![Line::from(vec![Span::styled(format!("{} ", fit(label, lw - 1)), st), Span::styled(fit(value, w - lw), st)])];
    }
    crate::ui::wrap(words, room)
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let head = if i == 0 { label[..label.len() - words.len()].to_string() } else { " ".repeat(lead) };
            let value = if i == 0 { value } else { "" };
            Line::from(vec![Span::styled(format!("{} ", fit(&format!("{head}{part}"), lw - 1)), st), Span::styled(fit(value, w - lw), st)])
        })
        .collect()
}

/// Full screen: areas left, rows right, the selected row explained at the bottom.
pub fn draw(f: &mut Frame, app: &mut App) {
    // The models on the disk, read again only when what is loaded changed.
    let Some(Dialog::Settings(s)) = &app.dialog else { return };
    if s.area == Area::Search {
        let loaded = app.index.loaded();
        if s.models.as_ref().is_none_or(|(l, _)| *l != loaded) {
            let list = coxswain_core::models::list(&app.cfg.search, &loaded);
            if let Some(Dialog::Settings(s)) = &mut app.dialog {
                s.models = Some((loaded, list));
            }
        }
    }
    // The rows, once a drawing. The cursor stays on its row when rows came or went since the
    // last drawing.
    let Some(Dialog::Settings(s)) = &app.dialog else { return };
    let rows = rows(app, s);
    let (at, key) = &s.anchor;
    let now = match rows.get(s.cursor).map(Row::key) {
        Some(k) if k != *key && s.cursor == *at => rows.iter().position(|r| r.key() == *key).unwrap_or(s.cursor),
        _ => s.cursor,
    };
    let anchor = (now, rows.get(now).map(Row::key).unwrap_or_default());
    if let Some(Dialog::Settings(s)) = &mut app.dialog {
        (s.cursor, s.anchor) = (now, anchor);
    }
    let full = f.area();
    let t = app.theme.clone();
    let base = dstyle(&t);
    let inner = frame(f, app, full, &t!("settings.title"));
    if inner.height < 6 || inner.width < 30 {
        return;
    }
    let Some(Dialog::Settings(s)) = &app.dialog else { return };
    let selected = sty(&t.dialog_input);
    let dim = base.add_modifier(Modifier::DIM);
    let head = base.fg(sty(&t.header).fg.unwrap_or(Color::Reset)).add_modifier(Modifier::BOLD);
    let red = base.fg(Color::Red);
    let border = sty(&t.dialog_border).bg(base.bg.unwrap_or(Color::Reset));

    // Areas on the left, a third of the width at most.
    let aw = AREAS.iter().map(|a| a.label().width()).max().unwrap_or(10).min(inner.width as usize / 3) + 2;
    // The explanation whole: its box grows to the lines it needs, the rows above giving way
    // (they scroll); three rows are kept for them. A version's changes can be longer than any
    // screen: as much as fits, and Enter shows them whole.
    let explain_w = inner.width - 3;
    let most = inner.height.saturating_sub(5).max(2);
    let mut text = Paragraph::new(Line::from(explain(app, s, &rows))).wrap(Wrap { trim: true });
    if let (Some(Row::News(c)), Mode::Browse) = (rows.get(s.cursor), &s.mode)
        && text.line_count(explain_w) > most as usize
    {
        text = cut(&plain(c), &t!("news.whole_tui"), explain_w, most, dim);
    }
    let explain_h = (text.line_count(explain_w) as u16).clamp(2, most);
    let body_h = inner.height - 2 - explain_h;
    for (i, a) in AREAS.iter().enumerate().take(body_h as usize) {
        let st = if *a == s.area { selected } else { base };
        f.render_widget(Paragraph::new(fit(&format!(" {}", a.label()), aw)).style(st), Rect { x: inner.x, y: inner.y + i as u16, width: aw as u16, height: 1 });
    }
    for y in 0..body_h {
        f.render_widget(Paragraph::new("│").style(border), Rect { x: inner.x + aw as u16, y: inner.y + y, width: 1, height: 1 });
    }
    let right = Rect { x: inner.x + aw as u16 + 2, y: inner.y, width: inner.width.saturating_sub(aw as u16 + 2), height: body_h };
    let w = right.width as usize;
    let cfg = &app.cfg;
    let values = cs::values(cfg);
    let level = cs::level(&cfg.search);
    let mut cursor_at: Option<Position> = None;

    // Each row as lines; the one under the cursor may open below it (a list, a choice).
    let mut lines: Vec<Line> = vec![];
    let mut cursor_line = 0;
    let lw = (w * 11 / 20).max(12).min(w);
    if let Mode::Find { query, at } = &s.mode {
        let prompt = format!("/ {query}");
        lines.push(Line::from(Span::styled(fit(&prompt, w), selected)));
        cursor_at = Some(Position::new(right.x + prompt.width().min(w) as u16, right.y));
        let found = find(query);
        if found.is_empty() && !query.trim().is_empty() {
            lines.push(Line::from(Span::styled(t!("settings.find_none"), dim)));
        }
        for (i, o) in found.iter().enumerate() {
            let text = format!(" {} · {}", o.label(), o.area.label());
            if i == *at {
                cursor_line = lines.len();
            }
            lines.push(Line::from(Span::styled(fit(&text, w), if i == *at { selected } else { base })));
        }
    } else {
        for (i, row) in rows.iter().enumerate() {
            let here = i == s.cursor;
            let st = if here && matches!(s.mode, Mode::Browse | Mode::Confirm(..)) { selected } else { base };
            if here {
                cursor_line = lines.len();
            }
            let pair = |label: &str, value: &str, st: Style| label_lines(label, value, st, w, lw);
            match row {
                Row::Head(text, _) => {
                    if !lines.is_empty() {
                        lines.push(Line::from(""));
                    }
                    lines.push(Line::from(Span::styled(fit(text, w), head)));
                }
                Row::Status(l) => {
                    let step = match l.poor.as_ref().and_then(|p| p.button.clone()) {
                        Some(b) => format!(" ! [{b}]"),
                        None => l.step.into_iter().chain(l.also).map(|x| format!(" [{}]", x.label())).collect(),
                    };
                    let label = format!(" {}", fit(&l.label, 9));
                    let bar = l.bar.map(|(done, total)| format!(" {} {} %", cs::bar(done, total, 10), cs::percent(done, total))).unwrap_or_default();
                    let text = fit(&l.text, w.saturating_sub(label.width() + bar.width() + step.width() + 1));
                    lines.push(Line::from(vec![Span::styled(format!("{label} "), if here { selected } else { head }), Span::styled(text, st), Span::styled(bar, st), Span::styled(step, st)]));
                }
                Row::Level(l) => {
                    let mark = if level == Some(*l) { "(•)" } else { "( )" };
                    lines.push(Line::from(Span::styled(fit(&format!(" {mark} {}", t!(&format!("settings.level.{}", l.id()))), w), st)));
                }
                Row::SetUp => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("setup.open")), w), st))),
                Row::Guide => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("guide.show_again")), w), st))),
                Row::EditKeys => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("settings.keys_change")), w), st))),
                Row::Translate => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", Action::SuggestTranslation.label()), w), st))),
                Row::Tool(name, there, line) => {
                    let value = if *there { String::new() } else { line.clone().unwrap_or_else(|| t!("settings.search_tool_missing")) };
                    lines.extend(pair(&format!(" {} {}", if *there { "✓" } else { "✗" }, t!(&format!("settings.search_tool_{name}"))), &value, st));
                }
                Row::Service => {
                    let mark = if s.service { "[x]" } else { "[ ]" };
                    lines.push(Line::from(Span::styled(fit(&format!(" {mark} {}", t!("setup.service_on")), w), st)));
                }
                // A path keeps its tail, the informative part; a sentence (what a version brought)
                // keeps its start.
                Row::Info(label, value, _) if Path::new(value).is_absolute() || value.starts_with('~') => lines.extend(pair(&format!(" {label}"), &fit_left(value, w.saturating_sub(lw)), st)),
                Row::Info(label, value, _) => lines.extend(pair(&format!(" {label}"), value, st)),
                Row::News(c) => lines.extend(pair(&format!(" {}", c.version), &plain(c), st)),
                Row::Go(area, value) => lines.push(Line::from(vec![Span::styled(fit(&format!(" {}", area.label()), aw.max(14)), if here { selected } else { head }), Span::styled(fit(value, w.saturating_sub(aw.max(14))), st)])),
                Row::Notice(n) => lines.push(Line::from(Span::styled(fit(&format!(" • {}", n.text), w), st))),
                Row::Poor(p) => {
                    let button = p.button.as_ref().map(|b| format!(" [{b}]")).unwrap_or_else(|| format!(" [{}]", t!("setup.open")));
                    // A tip (a Mac's GPU) is said plain; a poor choice in red.
                    let (mark, colour) = if p.tip { ("·", st) } else { ("!", if here { st } else { red }) };
                    lines.push(Line::from(vec![Span::styled(fit(&format!(" {mark} {}", p.text), w.saturating_sub(button.width())), colour), Span::styled(button, st)]));
                }
                Row::Model(e) => lines.extend(pair(&format!(" {}", e.name), &e.line(), st)),
                Row::Server(server) => lines.extend(pair(&format!(" {server}"), &format!("[{}]", t!("settings.history_server_forget")), st)),
                Row::DeleteUnused(n) => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("models.delete_unused", "size" => cs::human(*n))), w), st))),
                // The name, its size, what clearing costs: the cost is what tells the rows apart.
                Row::Disk(i) => {
                    let use_ = if i.in_use { format!(" · {}", t!("models.in_use")) } else { String::new() };
                    let head = format!(" {} {:>9}  ", fit(&i.name, 28.min(w / 3)), cs::human(i.bytes));
                    lines.push(Line::from(Span::styled(fit(&format!("{head}{}{use_}", i.cost), w), st)));
                }
                Row::Tick(outside, n) => {
                    let on = if *outside { s.ticks.1 } else { s.ticks.0 };
                    let label = t!(if *outside { "disk.also_outside" } else { "disk.also_models" }, "size" => cs::human(*n));
                    lines.push(Line::from(Span::styled(fit(&format!(" {} {label}", if on { "[x]" } else { "[ ]" }), w), st)));
                }
                Row::ClearAll(n) => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("disk.clear_all", "size" => cs::human(*n))), w), st))),
                Row::Engine(e) => {
                    let mark = if e.chosen { "(•)" } else { "( )" };
                    let value = match (&e.path, &e.install) {
                        (Some(p), _) => p.display().to_string(),
                        (None, Some(line)) => format!("{} {line}", t!("install.with")),
                        (None, None) => t!("chess.engine_not_found"),
                    };
                    lines.extend(pair(&format!(" {mark} {}", e.name), &value, st));
                }
                Row::EngineTest => {
                    let label = if s.engine_rx.is_some() { t!("chess.engine_testing") } else { t!("chess.engine_test") };
                    lines.push(Line::from(Span::styled(fit(&format!(" [ {label} ]"), w), st)));
                }
                Row::EngineReset => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("chess.engine_reset")), w), st))),
                Row::EngineOpt(o) => {
                    let label = format!("     {}", o.option.name);
                    match &s.mode {
                        Mode::EngineEdit(text) if here => {
                            let vw = w.saturating_sub(lw);
                            let field = fit_left(text, vw.saturating_sub(1));
                            cursor_at = Some(Position::new(right.x + (lw + field.width()) as u16, 0));
                            lines.push(Line::from(vec![Span::styled(format!("{} ", fit(&label, lw - 1)), base), Span::styled(fit(&field, vw), if s.selected { selected.add_modifier(Modifier::REVERSED) } else { selected })]));
                        }
                        _ => {
                            let value = match &o.value {
                                Some(v) => v.as_str().map_or_else(|| v.to_string(), String::from),
                                None => t!("chess.engine_default", "value" => if o.option.default.is_empty() { "—" } else { o.option.default.as_str() }),
                            };
                            lines.extend(pair(&label, &value, st));
                        }
                    }
                }
                // The kind, then what is used and the choice.
                Row::PreviewTool(r) => {
                    let choice = if !r.can_choose { String::new() } else if r.choice.is_empty() { t!("ptools.choice.default", "value" => choice_label("preview_prefer", &cfg.preview.prefer)) } else if r.choice == "container" && !r.container { t!("ptools.choice.container_no_image") } else { t!(&format!("ptools.choice.{}", r.choice)) };
                    // What is used first: the choice is long when it is the default.
                    let pulling = coxswain_core::images::pulling();
                    let uses = match r.images.iter().find_map(|i| pulling.get(&i.image)) {
                        Some(line) => format!("{} {line}", t!("common.loading")),
                        None => r.uses.clone(),
                    };
                    lines.extend(pair(&format!(" {} {}", if r.missing { "✗" } else { "✓" }, r.title), &if choice.is_empty() { uses } else { format!("{uses} · {choice}") }, if r.missing && !here { red } else { st }));
                }
                Row::Opt(o) => {
                    let value = &values[o.name];
                    let label = match on(o, value) {
                        Some(true) => format!(" [x] {}", o.label()),
                        Some(false) => format!(" [ ] {}", o.label()),
                        None => format!("     {}", o.label()),
                    };
                    match &s.mode {
                        Mode::Edit(text) if here => {
                            let vw = w.saturating_sub(lw);
                            let field = fit_left(text, vw.saturating_sub(1));
                            cursor_at = Some(Position::new(right.x + (lw + field.width()) as u16, 0));
                            lines.push(Line::from(vec![Span::styled(format!("{} ", fit(&label, lw - 1)), base), Span::styled(fit(&field, vw), if s.selected { selected.add_modifier(Modifier::REVERSED) } else { selected })]));
                        }
                        Mode::List { at, new } if here => {
                            lines.extend(pair(&label, "", head));
                            let items: Vec<&str> = value.as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                            for (j, item) in items.iter().enumerate() {
                                if j == *at {
                                    cursor_line = lines.len();
                                }
                                lines.push(Line::from(Span::styled(fit(&format!("     − {}", fit_left(item, w.saturating_sub(7))), w), if j == *at { selected } else { base })));
                            }
                            let prompt = format!("     + {}: ", t!("settings.search_add"));
                            let field = fit_left(new, w.saturating_sub(prompt.width() + 1));
                            if *at >= items.len() {
                                cursor_line = lines.len();
                                cursor_at = Some(Position::new(right.x + (prompt.width() + field.width()) as u16, 0));
                            }
                            lines.push(Line::from(vec![Span::styled(prompt.clone(), base), Span::styled(fit(&field, w.saturating_sub(prompt.width())), selected)]));
                        }
                        Mode::Pick(at) if here => {
                            lines.extend(pair(&label, "", head));
                            if let Kind::Choice(c) = kind(o, cfg, value) {
                                for (j, (v, name)) in c.iter().enumerate() {
                                    if v == HEAD {
                                        lines.push(Line::from(Span::styled(fit(&format!("   {name}"), w), if j == *at { selected } else { head })));
                                        continue;
                                    }
                                    let mark = if value.as_str() == Some(v.as_str()) { "(•)" } else if v.starts_with('\u{1}') { "   " } else { "( )" };
                                    if j == *at {
                                        cursor_line = lines.len();
                                    }
                                    lines.push(Line::from(Span::styled(fit(&format!("     {mark} {name}"), w), if j == *at { selected } else { base })));
                                }
                            }
                        }
                        _ => lines.extend(pair(&label, &shown(o, cfg, value), st)),
                    }
                }
            }
        }
    }

    // The cursor's line in sight.
    let height = right.height as usize;
    let Some(Dialog::Settings(s)) = &mut app.dialog else { return };
    if cursor_line < s.offset {
        s.offset = cursor_line;
    } else if cursor_line >= s.offset + height {
        s.offset = cursor_line + 1 - height;
    }
    let offset = s.offset.min(lines.len().saturating_sub(1));
    if let Some(p) = cursor_at.as_mut() {
        // An edit field's row: where its line is shown.
        if p.y == 0 {
            p.y = right.y + cursor_line.saturating_sub(offset) as u16;
        }
    }
    f.render_widget(Paragraph::new(lines.into_iter().skip(offset).take(height).collect::<Vec<_>>()), right);

    // The bottom: a rule, what the row is (as many lines as it needs), the keys or what was done.
    let y = inner.y + body_h;
    let rule = format!("{}┴{}", "─".repeat(aw), "─".repeat((inner.width as usize).saturating_sub(aw + 1)));
    f.render_widget(Paragraph::new(rule).style(border), Rect { y, height: 1, ..inner });
    let Some(Dialog::Settings(s)) = &app.dialog else { return };
    // One column short: ratatui lets a wide letter that wraps at the edge spill over the frame.
    let explain = Rect { x: inner.x + 1, y: y + 1, height: explain_h, width: explain_w };
    f.render_widget(text, explain);
    let keys = Rect { x: inner.x + 1, y: y + 1 + explain_h, height: 1, width: inner.width - 2 };
    let (line, st) = match (&s.said, &s.mode) {
        (Some((said, bad)), _) => (said.clone(), if *bad { red } else { dim }),
        (None, Mode::Edit(_) | Mode::EngineEdit(_)) => (crate::ui::keys(&t!("verb.save")), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::Engine(_))) => (t!("chess.engines_keys_tui"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::EngineOpt(_))) => (t!("chess.option_keys_tui"), dim),
        (None, Mode::List { .. }) => (t!("tui.settings.list_keys"), dim),
        (None, Mode::Pick(_)) => (t!("tui.settings.pick_keys"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::Disk(i)) if !i.clear) => (t!("disk.keys_tui_show"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::Disk(i)) if i.kind == coxswain_core::disk::Kind::Yours) => (t!("disk.keys_tui_remove"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::Disk(_))) => (t!("disk.keys_tui"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::Server(_))) => (t!("settings.history_server_keys_tui"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::PreviewTool(r)) if !r.container) => (t!("ptools.keys_tui_no_image"), dim),
        (None, _) if matches!(rows.get(s.cursor), Some(Row::PreviewTool(_))) => (t!("ptools.keys_tui"), dim),
        (None, _) => (t!("tui.settings.keys"), dim),
    };
    f.render_widget(Paragraph::new(fit(&line, keys.width as usize)).style(st), keys);
    if let Some(p) = cursor_at.filter(|p| p.y >= right.y && p.y < right.bottom()) {
        f.set_cursor_position(p);
    }
}

/// Chat models under the headings of where they run, as Settings lists them for Ask and for
/// code questions: a server that is missing with the way to set it up, and the recommended
/// one and those slow here marked.
fn chat_list(groups: Vec<coxswain_core::setup::AskGroup>, looked: bool) -> Vec<(String, String)> {
    let mut v = vec![];
    for g in groups {
        let missing = g.missing.clone();
        v.push((HEAD.to_string(), match (&missing, looked) {
            (Some(m), false) => format!("{} ({m})", g.label),
            _ => g.label,
        }));
        if let (Some(m), true) = (missing, looked) {
            v.push((SET_UP.to_string(), format!("{m}: {}", t!("settings.ask_set_up_server"))));
        }
        for c in g.models {
            let mut label = c.label;
            if c.recommended {
                label += &format!(" [{}]", t!("setup.recommended"));
            }
            if c.slow {
                label += &format!(" [{}]", t!("settings.ask_slow_here"));
            }
            v.push((c.value, label));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app(dir: &std::path::Path) -> App {
        let cfg = Config { check_updates: false, language: "en-GB".into(), ..Config::default() };
        let index = crate::test_index(&cfg);
        App::with_index(cfg, dir.to_path_buf(), dir.to_path_buf(), index).unwrap()
    }

    fn screen(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect()
    }

    fn key(app: &mut App, code: KeyCode) {
        app.dialog_key(Key::new(code, false, false, false));
    }

    fn typed(app: &mut App, text: &str) {
        text.chars().for_each(|c| key(app, KeyCode::Char(c)));
    }

    fn settings(app: &App) -> &Settings {
        match &app.dialog {
            Some(Dialog::Settings(s)) => s,
            _ => panic!("Settings closed"),
        }
    }

    fn selected(app: &App) -> Option<&'static str> {
        let s = settings(app);
        rows(app, s).get(s.cursor).and_then(Row::id)
    }

    /// Preview tools: a row per kind once read; Enter takes the next choice and writes it.
    #[test]
    fn preview_tools_rows_take_the_next_choice() {
        let d = crate::test_dir("panels");
        let mut app = app(&d);
        app.cfg.preview.container = "off".into();
        let mut s = Settings::open(&app, "preview_tools");
        s.path = Some(d.join("config.toml"));
        while s.tools.is_none() {
            s.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let rows_now = rows(&app, &s);
        let at = rows_now.iter().position(|r| matches!(r, Row::PreviewTool(t) if t.kind == "latex")).unwrap();
        assert_eq!(rows_now.iter().filter(|r| matches!(r, Row::PreviewTool(_))).count(), coxswain_core::preview_tools::KINDS.len());
        s.cursor = at;
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cfg.preview.tools.get("latex").map(String::as_str), Some("local"));
        assert!(std::fs::read_to_string(d.join("config.toml")).unwrap().contains("latex = \"local\""));
    }

    /// Chess engines: the known ones listed, Test engine reads a fake engine's options, a
    /// switch flips and a number is typed, each written under its table in config.toml; a
    /// number out of range is refused; Reset takes them all out.
    #[cfg(unix)]
    #[test]
    fn chess_engine_options_set_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::test_dir("panels");
        let engine = d.join("fake-engine");
        std::fs::write(&engine, "#!/bin/sh\nwhile read l; do case \"$l\" in uci) echo 'id name Fake 2.0'; echo 'option name Threads type spin default 1 min 1 max 512'; echo 'option name Ponder type check default false'; echo 'option name UCI_LimitStrength type check default false'; echo uciok;; quit) exit 0;; esac; done\n").unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut app = app(&d);
        app.cfg.chess.engine = engine.display().to_string();
        let mut s = Settings::open(&app, "chess_engines");
        let config = d.join("config.toml");
        s.path = Some(config.clone());
        let listed = rows(&app, &s).iter().filter(|r| matches!(r, Row::Engine(_))).count();
        assert_eq!(listed, coxswain_core::chess::play::KNOWN.len());
        s.cursor = rows(&app, &s).iter().position(|r| matches!(r, Row::EngineTest)).unwrap();
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        let t = std::time::Instant::now();
        loop {
            let Some(Dialog::Settings(s)) = app.dialog.as_mut() else { panic!("settings closed") };
            s.poll();
            if s.engine.is_some() || t.elapsed() > coxswain_core::test_limit(std::time::Duration::from_secs(20)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let at = |app: &App, name: &str| rows(app, settings(app)).iter().position(|r| matches!(r, Row::EngineOpt(o) if o.option.name == name));
        // Ponder is Coxswain's own: not shown.
        assert!(at(&app, "Ponder").is_none());
        let strength = at(&app, "UCI_LimitStrength").expect("options read");
        if let Some(Dialog::Settings(s)) = app.dialog.as_mut() {
            s.cursor = strength;
        }
        key(&mut app, KeyCode::Enter);
        let threads = at(&app, "Threads").unwrap();
        if let Some(Dialog::Settings(s)) = app.dialog.as_mut() {
            s.cursor = threads;
        }
        // Not set yet: the default opens selected, and what is typed replaces it.
        key(&mut app, KeyCode::Enter);
        assert!(settings(&app).selected);
        typed(&mut app, "100000");
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&settings(&app).mode, Mode::EngineEdit(t) if t == "100000"), "a number out of range is typed again");
        for _ in 0..6 {
            key(&mut app, KeyCode::Backspace);
        }
        typed(&mut app, "1");
        key(&mut app, KeyCode::Enter);
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("[chess.engines.Fake]") && text.contains("UCI_LimitStrength = true") && text.contains("Threads = 1"), "{text}");
        assert_eq!(app.cfg.chess.engines["Fake"]["Threads"].as_integer(), Some(1));
        // Delete on each option set: the engine's table goes with its last one.
        for name in ["Threads", "UCI_LimitStrength"] {
            let at = at(&app, name).unwrap();
            if let Some(Dialog::Settings(s)) = app.dialog.as_mut() {
                s.cursor = at;
            }
            key(&mut app, KeyCode::Delete);
        }
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(!text.contains("[chess.engines"), "{text}");
        let threads = at(&app, "Threads").unwrap();
        if let Some(Dialog::Settings(s)) = app.dialog.as_mut() {
            s.cursor = threads;
        }
        key(&mut app, KeyCode::Enter);
        typed(&mut app, "1");
        key(&mut app, KeyCode::Enter);
        let reset = rows(&app, settings(&app)).iter().position(|r| matches!(r, Row::EngineReset)).unwrap();
        if let Some(Dialog::Settings(s)) = app.dialog.as_mut() {
            s.cursor = reset;
        }
        key(&mut app, KeyCode::Enter);
        assert!(!std::fs::read_to_string(&config).unwrap().contains("Threads"));
        assert!(app.cfg.chess.engines.get("Fake").is_none_or(|t| t.is_empty()));
    }

    /// At 80 columns no label is cut, in any language: a long one goes on over the lines below,
    /// its value on its first line.
    #[test]
    fn labels_are_whole_at_80_columns() {
        use coxswain_core::i18n::{tr_in, LANGUAGES};
        let words = |s: &str| s.split_whitespace().collect::<String>();
        for lang in LANGUAGES {
            // The widths `draw` gives the rows on a terminal 80 wide: its frame takes two.
            let aw = AREAS.iter().map(|a| tr_in(lang.code, &format!("settings.area.{}", a.id())).width()).max().unwrap().min(78 / 3) + 2;
            let w = 78 - aw - 2;
            let lw = (w * 11 / 20).max(12).min(w);
            for o in cs::OPTIONS {
                let label = tr_in(lang.code, &format!("setting.{}", o.name));
                for mark in [" [ ] ", "     "] {
                    let lines = label_lines(&format!("{mark}{label}"), "Classic blue (NC)", Style::default(), w, lw);
                    let text: Vec<String> = lines.iter().map(|l| l.spans[0].content.to_string()).collect();
                    assert!(text.iter().all(|t| !t.contains('…') && t.width() == lw), "{}: {}: {text:?}", lang.code, o.name);
                    assert_eq!(words(&text.concat()), words(&format!("{}{label}", mark.trim())), "{}: {}", lang.code, o.name);
                    assert!(lines[0].spans[1].content.starts_with("Classic blue"));
                }
            }
        }
        // Short ones stay on one line.
        assert_eq!(label_lines(" [x] Icons", "On", Style::default(), 55, 30).len(), 1);
    }

    /// Under its heading at 80 × 24, the terminal app's theme is named whole.
    #[test]
    fn the_terminal_theme_is_named_whole() {
        let d = crate::test_dir("panels");
        let mut app = app(&d);
        app.dialog = Some(Dialog::Settings(Box::new(Settings::open(&app, "tui_theme"))));
        let text = screen(&mut app, 80, 24).iter().map(|l| l.split('│').nth(1).unwrap_or_default().trim().to_string()).collect::<Vec<_>>().join(" ");
        assert!(text.contains(&t!("settings.theme_tui")), "{text}");
        // Its value beside its first line, the rest of it on the next.
        let label = text.replace(&choice_label("tui_theme", "nc"), "").replace('║', "");
        assert!(words_in(&label, &t!("setting.tui_theme")), "{text}");
    }

    fn words_in(text: &str, label: &str) -> bool {
        let text: Vec<&str> = text.split_whitespace().collect();
        let label: Vec<&str> = label.split_whitespace().collect();
        text.windows(label.len()).any(|w| w == label.as_slice())
    }

    /// A preview tool's explanation leaves no separator dangling where it has no file endings
    /// to list (ClamAV reads any file).
    #[test]
    fn no_dangling_separator_without_endings() {
        let d = crate::test_dir("panels");
        let app = app(&d);
        let mut s = Settings::open(&app, "preview_tools");
        while s.tools.is_none() {
            s.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let rows_now = rows(&app, &s);
        for kind in ["viruses", "latex"] {
            s.cursor = rows_now.iter().position(|r| matches!(r, Row::PreviewTool(t) if t.kind == kind)).unwrap();
            let text: String = explain(&app, &s, &rows_now).iter().map(|sp| sp.content.to_string()).collect();
            assert!(!text.trim_end().ends_with('·') && !text.contains("·  ·") && !text.contains("· ·"), "{kind}: {text}");
        }
    }

    /// The themes stand under their groups' headings, your own last; the next one skips a heading.
    #[test]
    fn themes_are_listed_by_group() {
        let d = crate::test_dir("panels");
        let mut app = app(&d);
        app.cfg.themes.insert("mine".into(), Theme::nc());
        let o = cs::find("tui_theme").unwrap();
        let Kind::Choice(c) = kind(o, &app.cfg, &Value::from("nc")) else { panic!() };
        let heads: Vec<&str> = c.iter().filter(|(v, _)| v == HEAD).map(|(_, l)| l.as_str()).collect();
        assert_eq!(heads.len(), 7);
        assert_eq!((c[0].0.as_str(), c[1].0.as_str(), c.last().unwrap().0.as_str()), (HEAD, "cyber", "mine"));
        let paper = c.iter().position(|(v, _)| v == "paper").unwrap();
        assert_eq!(c[pick_step(&c, paper + 1, 1)].0, "nc");
    }

    /// Ask's list opens on the same row by Enter and by Space, and no move stops on a heading.
    #[test]
    fn asks_list_opens_and_moves_on_models() {
        let d = crate::test_dir("panels");
        let mut app = app(&d);
        app.cfg.search.ask_model = String::new();
        // The machine looked at first: a probe finishing between Enter and Space adds a row
        // (a server's set-up line) above the model, and the two open one row apart. That took
        // longer than the keys on a slow OpenBSD machine.
        coxswain_core::setup::look(true);
        app.dialog = Some(Dialog::Settings(Box::new(Settings::open(&app, "ask_model"))));
        let pick = |app: &App| match settings(app).mode {
            Mode::Pick(at) => at,
            _ => panic!("no list open"),
        };
        key(&mut app, KeyCode::Enter);
        let by_enter = pick(&app);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char(' '));
        assert_eq!(pick(&app), by_enter);
        // The list is read again at every key, as the app does: the machine probe can finish
        // in between and change which models it offers.
        let o = cs::find("ask_model").unwrap();
        let list = |app: &App| match kind(o, &app.cfg, &Value::from("")) {
            Kind::Choice(c) => c,
            _ => panic!("ask_model is no choice"),
        };
        assert_ne!(list(&app)[by_enter].0, HEAD);
        for code in [KeyCode::Up, KeyCode::PageUp, KeyCode::Down, KeyCode::PageDown, KeyCode::Up] {
            key(&mut app, code);
            assert_ne!(list(&app)[pick(&app)].0, HEAD, "{code:?}");
        }
        let c = [(HEAD.to_string(), String::new()), ("a".to_string(), String::new()), (HEAD.to_string(), String::new()), ("b".to_string(), String::new())];
        assert_eq!((pick_step(&c, 0, -1), pick_step(&c, 2, 1), pick_step(&c, 2, -1), pick_step(&c, 9, 1)), (1, 3, 1, 3));
    }

    /// The built-in models are walked once for a drawing, not again at each key: only a key
    /// that saves a setting or deletes one forgets them.
    #[test]
    fn a_key_keeps_the_models_read() {
        let d = crate::test_dir("panels");
        let mut app = app(&d);
        app.dialog = Some(Dialog::Settings(Box::new(Settings::open(&app, "search"))));
        screen(&mut app, 100, 40);
        assert!(settings(&app).models.is_some(), "read for the drawing");
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Up);
        assert!(settings(&app).models.is_some(), "kept by a key that changes nothing");
    }

    #[test]
    fn every_option_of_finding_files_is_in_a_group() {
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|g| g.2.iter().copied()).collect();
        for o in cs::OPTIONS.iter().filter(|o| o.area == Area::Search) {
            assert_eq!(grouped.iter().filter(|n| **n == o.name).count(), 1, "{}", o.name);
        }
        assert!(grouped.iter().all(|n| cs::find(n).is_some()));
    }

    #[test]
    fn settings_move_edit_and_save_at_once() {
        let d = std::env::temp_dir().join(format!("coxswain-test-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("mail")).unwrap();
        std::fs::write(d.join(".secret"), "").unwrap();
        let path = d.join("config.toml");
        std::fs::write(&path, "# mine\nlanguage = \"en-GB\"\nshow_hidden = false # keep\n").unwrap();
        let mut app = app(&d);
        let hidden = |app: &App| app.panels.iter().all(|p| p.entries.iter().any(|e| e.name == ".secret"));
        // As at start: the panels list by the config read.
        app.show_hidden = false;
        app.reload();
        assert!(!hidden(&app));
        app.cfg = Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut s = Settings::open(&app, "show_hidden");
        s.path = Some(path.clone());
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        assert_eq!((settings(&app).area, selected(&app)), (Area::Behaviour, Some("show_hidden")));

        // Every area draws at 80×24, its name marked on the left.
        for _ in 0..AREAS.len() {
            let area = settings(&app).area;
            let scr = screen(&mut app, 80, 24);
            assert!(scr.iter().any(|l| l.contains(&area.label())), "{area:?}");
            assert!(scr[22].contains("Esc"), "{area:?}: {}", scr[22]);
            key(&mut app, KeyCode::Right);
        }
        assert_eq!(settings(&app).area, Area::Behaviour, "Right goes round the areas");

        // Space flips a switch: written at once, the comments kept.
        key(&mut app, KeyCode::Char(' '));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine") && text.contains("show_hidden = true # keep"), "{text}");
        assert!(app.cfg.show_hidden);
        assert!(app.show_hidden && hidden(&app), "the panels show hidden files at once");
        assert!(settings(&app).said.as_ref().is_some_and(|(_, bad)| !bad));

        // Enter edits a number in place; out of its range it is refused and not written.
        let mut s = Settings::open(&app, "max_results");
        s.path = Some(path.clone());
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        assert!(matches!(settings(&app).mode, Mode::Edit(_)));
        for _ in 0..10 {
            key(&mut app, KeyCode::Backspace);
        }
        typed(&mut app, "5");
        key(&mut app, KeyCode::Enter);
        assert!(settings(&app).said.as_ref().is_some_and(|(_, bad)| *bad), "5 is below 10");
        typed(&mut app, "00");
        key(&mut app, KeyCode::Enter);
        assert_eq!(Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap().search.max_results, 500);
        // Enter again: the value opens selected, shown reversed, and the first key replaces it.
        key(&mut app, KeyCode::Enter);
        let scr = screen(&mut app, 80, 24);
        assert!(scr.iter().any(|l| l.contains("500")), "{scr:#?}");
        typed(&mut app, "250");
        key(&mut app, KeyCode::Enter);
        assert_eq!(Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap().search.max_results, 250);

        // A list: a folder added (resolved from the panel's folder), then removed.
        let mut s = Settings::open(&app, "names_only");
        s.path = Some(path.clone());
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        typed(&mut app, "mail");
        key(&mut app, KeyCode::Enter);
        let mail = coxswain_core::fs::canonical(&d.join("mail")).unwrap();
        assert_eq!(app.cfg.search.names_only, [mail]);
        let scr = screen(&mut app, 80, 24);
        assert!(scr.iter().any(|l| l.contains("− ") && l.contains("mail")), "{scr:#?}");
        key(&mut app, KeyCode::Up);
        key(&mut app, KeyCode::Delete);
        assert!(app.cfg.search.names_only.is_empty());
        key(&mut app, KeyCode::Esc);
        assert!(matches!(settings(&app).mode, Mode::Browse));

        // A choice: Enter opens its values, Down and Enter take one.
        let mut s = Settings::open(&app, "preview_prefer");
        s.path = Some(path.clone());
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cfg.preview.prefer, "local");

        // / finds a setting and goes to it.
        typed(&mut app, "/");
        typed(&mut app, "check_updates");
        key(&mut app, KeyCode::Enter);
        assert_eq!((settings(&app).area, selected(&app)), (Area::Privacy, Some("check_updates")));

        // Narrow, the rows still fit: no line wider than the screen, the bar in one column.
        let scr = screen(&mut app, 80, 24);
        let bar: Vec<usize> = (1..19).filter_map(|y| scr[y].chars().position(|c| c == '│')).collect();
        assert!(bar.windows(2).all(|w| w[0] == w[1]), "{bar:?}");

        // Esc closes.
        key(&mut app, KeyCode::Esc);
        assert!(app.dialog.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_levels_and_steps_are_rows_of_finding_files() {
        let d = crate::test_dir("panels");
        let app = app(&d);
        let s = Settings::open(&app, "search");
        let rows = rows(&app, &s);
        assert_eq!(rows.iter().filter(|r| matches!(r, Row::Status(_))).count(), 4);
        assert_eq!(rows.iter().filter(|r| matches!(r, Row::Level(_))).count(), 4);
        assert!(matches!(rows[s.cursor], Row::Status(_)), "the cursor starts on the status block");
        let s = Settings::open(&app, "ask_model");
        assert_eq!(rows.get(s.cursor).and_then(Row::id), Some("ask_model"), "--settings=ask_model: at it");
        let s = Settings::open(&app, "ask");
        assert_eq!(s.area, Area::Overview, "the section names of 1.x are gone");
        assert_eq!(step(&rows, 0, -1), 0);
    }

    /// Looks names the app each theme and font is for: every option of the area once, the
    /// desktop app's under its heading, never right under Language.
    #[test]
    fn looks_rows_stand_under_their_app() {
        let d = crate::test_dir("panels");
        let app = app(&d);
        let s = Settings::open(&app, "looks");
        let rows = rows(&app, &s);
        let mut names: Vec<&str> = rows.iter().filter_map(|r| if let Row::Opt(o) = r { Some(o.name) } else { None }).collect();
        assert_eq!(names[..2], ["language", "glyphs"]);
        let gui = rows.iter().position(|r| r.id() == Some("looks_gui")).unwrap();
        assert!(rows.iter().position(|r| r.id() == Some("theme")).unwrap() > gui);
        names.sort();
        let mut all: Vec<&str> = cs::OPTIONS.iter().filter(|o| o.area == Area::Looks).map(|o| o.name).collect();
        all.sort();
        assert_eq!(names, all);
    }

    /// At 80×24, in English and in the language whose texts are widest, every row's
    /// explanation shows whole in the box at the bottom. The language is the whole process's,
    /// so this runs in a process of its own.
    #[test]
    fn every_explanation_shows_whole_at_80_by_24() {
        if std::env::var_os("COXSWAIN_EXPLAIN_TEST").is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["settings::tests::every_explanation_shows_whole_at_80_by_24", "--exact", "--test-threads=1"])
                .env("COXSWAIN_EXPLAIN_TEST", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            return;
        }
        let width = |code: &str| coxswain_core::i18n::catalogue(code).values().filter_map(Value::as_str).map(|v| v.width()).sum::<usize>();
        let widest = coxswain_core::i18n::LANGUAGES.iter().map(|l| l.code).max_by_key(|c| width(c)).unwrap();
        let d = crate::test_dir("panels");
        let squeeze = |t: &str| t.chars().filter(|c| !c.is_whitespace() && *c != '║').collect::<String>();
        for lang in ["en-GB", widest] {
            coxswain_core::i18n::set_language(lang);
            for area in AREAS {
                let mut app = app(&d);
                app.dialog = Some(Dialog::Settings(Box::new(Settings::open(&app, area.id()))));
                let mut last = None;
                loop {
                    // The file count goes on in the background: the screen and the text it is
                    // held to are made from the same count, made again until it stood still.
                    let count = |app: &App| serde_json::to_string(&app.index.status()).unwrap();
                    let (lines, want) = loop {
                        let before = count(&app);
                        let lines = screen(&mut app, 80, 24);
                        let s = settings(&app);
                        let want: String = explain(&app, s, &rows(&app, s)).iter().map(|x| x.content.as_ref()).collect();
                        if count(&app) == before {
                            break (lines, want);
                        }
                    };
                    let s = settings(&app);
                    let rows = rows(&app, s);
                    let rule = lines.iter().position(|l| l.contains('┴')).unwrap();
                    let shown = lines[rule + 1..22].concat();
                    // A version longer than the box: its start, "…" and how to see it whole.
                    if matches!(rows.get(s.cursor), Some(Row::News(_))) && squeeze(&shown) != squeeze(&want) {
                        assert!(cut_shown(&squeeze(&shown), &squeeze(&want), &squeeze(&t!("news.whole_tui"))), "{lang}, {}, row {}: {shown}", area.id(), s.cursor);
                    } else {
                        assert_eq!(squeeze(&shown), squeeze(&want), "{lang}, {}, row {}", area.id(), s.cursor);
                    }
                    if last == Some(s.cursor) {
                        break;
                    }
                    last = Some(s.cursor);
                    key(&mut app, KeyCode::Down);
                }
            }
        }
        coxswain_core::i18n::set_language("en-GB");
    }

    /// A text cut to fit: a start of `whole` that is not all of it, then "…", then `more`.
    fn cut_shown(shown: &str, whole: &str, more: &str) -> bool {
        shown.strip_suffix(more).and_then(|s| s.strip_suffix('…')).is_some_and(|head| !head.is_empty() && head.len() < whole.len() && whole.starts_with(head))
    }

    /// A version whose changes are longer than any screen (2.33.0's were 66,000 letters): at
    /// 80×24 the box shows their start, "…" and that Enter shows them whole, and Enter does,
    /// in the viewer, back in Settings after it.
    #[test]
    fn a_long_version_shows_its_start_and_enter_shows_it_whole() {
        let d = crate::test_dir("long-news");
        let mut app = app(&d);
        let long: String = (0..2000).map(|i| format!("word{:05} ", i)).collect::<String>() + "THE END";
        assert!(long.chars().count() > 20_000);
        let mut s = Settings::open(&app, "overview");
        s.news = vec![coxswain_core::notices::Change { version: "9.9.9".into(), date: "2026-01-01".into(), parts: vec![(long.clone(), None)] }];
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        while !matches!(rows(&app, settings(&app)).get(settings(&app).cursor), Some(Row::News(_))) {
            key(&mut app, KeyCode::Down);
            screen(&mut app, 80, 24);
        }
        let lines = screen(&mut app, 80, 24);
        let squeeze = |t: &str| t.chars().filter(|c| !c.is_whitespace() && *c != '║').collect::<String>();
        let rule = lines.iter().position(|l| l.contains('┴')).unwrap();
        let shown = lines[rule + 1..22].concat();
        assert!(cut_shown(&squeeze(&shown), &squeeze(&long), &squeeze(&t!("news.whole_tui"))), "{shown}");
        key(&mut app, KeyCode::Enter);
        let Some(Dialog::View { v, back }) = &app.dialog else { panic!("no viewer") };
        assert!(matches!(back.as_deref(), Some(Dialog::Settings(_))), "Settings again after it");
        assert!(std::fs::read_to_string(&v.path).unwrap().contains(&long), "the whole text");
        let _ = std::fs::remove_file(&v.path);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every section can be walked to its last row by Down, drawn between keys as the app does,
    /// at every common terminal size: no row (a choice, a list) keeps the cursor.
    #[test]
    fn every_row_of_every_section_is_reached() {
        let d = crate::test_dir("panels");
        for (w, h) in [(80, 24), (100, 30), (140, 40), (200, 60)] {
            for area in AREAS {
                let mut app = app(&d);
                app.dialog = Some(Dialog::Settings(Box::new(Settings::open(&app, area.id()))));
                screen(&mut app, w, h);
                let want: Vec<String> = rows(&app, settings(&app)).iter().filter(|r| r.selectable()).map(Row::key).collect();
                let mut seen = vec![];
                for _ in 0..want.len() + 2 {
                    let s = settings(&app);
                    let k = rows(&app, s).get(s.cursor).map(Row::key).unwrap_or_default();
                    if seen.last() != Some(&k) {
                        seen.push(k);
                    }
                    key(&mut app, KeyCode::Down);
                    screen(&mut app, w, h);
                }
                assert_eq!(seen, want, "{} at {w}×{h}", area.id());
            }
        }
    }
}

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
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// The areas, in the order of the list on the left.
pub const AREAS: [Area; 7] = Area::ALL;

/// Finding files' details, by group: its heading, the name `--settings=<name>` opens it at, and
/// its options. A test checks that every option of the area is in one.
const GROUPS: &[(&str, &str, &[&str])] = &[
    ("settings.group.reads", "reads", &["search_text", "search_archives", "search_archives_everywhere", "search_history", "search_cloud", "cloud_read", "text_max_size", "max_results"]),
    ("settings.group.folders", "folders", &["text_roots", "names_only", "text_exclude", "name_roots", "name_exclude", "watch"]),
    ("settings.group.meaning", "meaning", &["search_meaning", "meaning_engine", "meaning_url", "meaning_model", "meaning_key_env", "meaning_device"]),
    ("settings.group.ask", "ask", &["ask_model", "ask_think"]),
];

/// Text options that are a switch: (name, value when on, value when off).
const SWITCHES: &[(&str, &str, &str)] = &[("search_cloud", "all", "local-only"), ("meaning_device", "cpu", "auto")];

/// Number options: (name, least, most, scale): `text_max_size` is kept in bytes, typed in MB.
const NUMBERS: &[(&str, f64, f64, f64)] = &[("text_max_size", 1.0, 4096.0, 1_048_576.0), ("max_results", 10.0, 1_000_000.0, 1.0), ("preview_timeout", 10.0, 3600.0, 1.0), ("font_size", 9.0, 28.0, 1.0), ("line_height", 1.2, 3.0, 1.0)];

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
    Notice(Notice),
    /// The first-run guide.
    Guide,
    /// A program that reads more (tesseract …), whether it is there, and the line that installs it.
    Tool(String, bool, Option<String>),
}

impl Row {
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
    /// Find a setting: the words, the cursor on a result.
    Find { query: String, at: usize },
    /// Another model reads every file's meaning again: why, and the change that waits for a yes.
    Confirm(String, Map<String, Value>),
}

pub struct Settings {
    pub area: Area,
    pub cursor: usize,
    offset: usize,
    pub mode: Mode,
    /// The config.toml written to (a test's own).
    pub path: Option<PathBuf>,
    /// What the last change did: saved, or why not.
    pub said: Option<(String, bool)>,
    /// Ask's test question: the answer on its way, then what it said.
    trial_rx: Option<mpsc::Receiver<Result<Duration, String>>>,
    trial: Option<(String, bool)>,
    /// Read when Settings opens: they ask the system.
    service: bool,
    notices: Vec<Notice>,
    /// What's new: the versions not read yet (else this one), with what they brought.
    news: Vec<(String, String)>,
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
            path: Config::path(),
            said: None,
            trial_rx: None,
            trial: None,
            service: coxswain_core::service::installed(),
            notices: coxswain_core::notices::all(&app.cfg, &app.index.status(), &st, true),
            news: news(&st),
        };
        s.go(app, section);
        s
    }

    /// To `section`'s area, the cursor on its option or group.
    fn go(&mut self, app: &App, section: &str) {
        let (area, at) = cs::open_at(section);
        self.area = area;
        self.offset = 0;
        let rows = rows(app, self);
        let start = at.and_then(|at| rows.iter().position(|r| r.id() == Some(at))).unwrap_or(0);
        self.cursor = (start..rows.len()).find(|&i| rows[i].selectable()).unwrap_or(0);
    }

    /// The answer to Ask's test question, when it has come.
    pub fn poll(&mut self) {
        if let Some(r) = self.trial_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.trial_rx = None;
            self.trial = Some(match r {
                Ok(d) => (t!("setup.try_done", "seconds" => format!("{:.1}", d.as_secs_f64())), false),
                Err(e) => (e, true),
            });
        }
    }
}

// ---------------------------------------------------------------- what an area holds

/// The versions not read yet, else this one: (version, what it brought).
fn news(st: &coxswain_core::state::AppState) -> Vec<(String, String)> {
    let unread = coxswain_core::notices::unread(st);
    let changes = if unread.is_empty() { coxswain_core::notices::changes().into_iter().take(1).collect() } else { unread };
    changes.into_iter().take(5).map(|c| (c.version, c.parts.iter().map(|(t, _)| t.as_str()).collect())).collect()
}

/// The rows of the area shown.
pub fn rows(app: &App, s: &Settings) -> Vec<Row> {
    let cfg = &app.cfg;
    let opts = |area: Area| cs::OPTIONS.iter().filter(move |o| o.area == area).map(Row::Opt);
    let level_name = || cs::level(&cfg.search).map_or_else(|| t!("settings.level.custom"), |l| t!(&format!("settings.level.{}", l.id())));
    let mut v = vec![];
    match s.area {
        Area::Overview => {
            let lines = cs::status(&cfg.search, &app.index.status(), app.index.shared());
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
            v.extend(s.news.iter().map(|(version, text)| Row::Info(version.clone(), text.clone(), text.clone())));
        }
        Area::Search => {
            v.extend(cs::status(&cfg.search, &app.index.status(), app.index.shared()).into_iter().map(Row::Status));
            v.push(Row::Head(t!("settings.level"), "level"));
            v.extend(Level::ALL.into_iter().map(Row::Level));
            v.push(Row::SetUp);
            for (head, id, names) in GROUPS {
                v.push(Row::Head(t!(head), id));
                v.extend(names.iter().filter_map(|n| cs::find(n)).map(Row::Opt));
            }
            let tools = app.index.status().tools;
            if !tools.is_empty() {
                v.push(Row::Head(t!("settings.search_tools"), "tools"));
                v.extend(tools.into_iter().map(|(name, there)| {
                    let line = (!there).then(|| coxswain_core::tools::install(&name)).flatten();
                    Row::Tool(name, there, line)
                }));
            }
            v.push(Row::Head(t!("settings.group.background"), "background"));
            v.push(Row::Service);
        }
        Area::Keys => {
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
        "glyphs" => pairs(&[("nerd", "settings.glyphs_nerd"), ("ascii", "settings.glyphs_ascii")]),
        "meaning_engine" => vec![
            ("builtin".into(), t!("settings.meaning_builtin", "size" => cs::human(coxswain_core::meaning::size()))),
            ("ollama".into(), "Ollama".into()),
            ("openai".into(), t!("settings.meaning_openai")),
        ],
        "language" => std::iter::once(("auto".to_string(), t!("settings.language_auto"))).chain(coxswain_core::i18n::LANGUAGES.iter().map(|l| (l.code.to_string(), l.name.to_string()))).collect(),
        "theme" | "tui_theme" => {
            let builtin: Vec<&str> = Theme::builtin().into_iter().map(|(n, _)| n).collect();
            let own = cfg.themes.keys().filter(|n| !builtin.contains(&n.as_str())).cloned();
            builtin.iter().map(|n| (n.to_string(), t!(&format!("theme.{n}")))).chain(own.map(|n| (n.clone(), n))).collect()
        }
        _ => return None,
    })
}

/// A choice's value by its name.
fn choice_label(name: &str, value: &str) -> String {
    let cfg = Config::default();
    choices(name, &cfg).and_then(|c| c.into_iter().find(|(v, _)| v == value)).map_or_else(|| value.to_string(), |(_, l)| l)
}

/// A number as typed: a whole one without decimals.
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
            "viewer" => "$PAGER".into(),
            _ => String::new(),
        },
        (_, Value::String(v)) => v.clone(),
        (_, v) => v.to_string(),
    }
}

/// Whether a switch is on.
fn on(o: &Opt, cfg: &Config, value: &Value) -> Option<bool> {
    match kind(o, cfg, value) {
        Kind::Switch => value.as_bool(),
        Kind::TextSwitch(yes, _) => Some(value.as_str() == Some(yes)),
        _ => None,
    }
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
                (KeyCode::Enter, _) if opt.is_some() => {
                    let o = opt.unwrap();
                    s.mode = match kind(o, &self.cfg, &value) {
                        Kind::Text => Mode::Edit(value.as_str().unwrap_or_default().to_string()),
                        Kind::Number(_, _, scale) => Mode::Edit(number_text(value.as_f64().unwrap_or(0.0) / scale)),
                        Kind::List => Mode::List { at: 0, new: String::new() },
                        Kind::Choice(c) => Mode::Pick(c.iter().position(|(v, _)| value.as_str() == Some(v)).unwrap_or(0)),
                        Kind::Switch | Kind::TextSwitch(..) => return self.settings_space(s, &rows, &value),
                    };
                }
                (KeyCode::Enter, _) | (_, Some(' ')) => return self.settings_space(s, &rows, &value),
                _ => {}
            },
            Mode::Edit(mut text) => match (key.code, ch) {
                _ if esc => {}
                (KeyCode::Enter, _) => {
                    let o = opt.expect("editing an option");
                    let new = match kind(o, &self.cfg, &value) {
                        Kind::Number(lo, hi, scale) => match text.trim().replace(',', ".").parse::<f64>() {
                            Ok(n) if (lo..=hi).contains(&n) => {
                                let n = n * scale;
                                if n.fract() == 0.0 { Value::from(n as i64) } else { Value::from((n * 100.0).round() / 100.0) }
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
                            std::fs::canonicalize(&dir).unwrap_or(dir).display().to_string()
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
                    KeyCode::Up => s.mode = Mode::Pick(at.saturating_sub(1)),
                    KeyCode::Down => s.mode = Mode::Pick((at + 1).min(c.len() - 1)),
                    KeyCode::PageUp => s.mode = Mode::Pick(at.saturating_sub(10)),
                    KeyCode::PageDown => s.mode = Mode::Pick((at + 10).min(c.len() - 1)),
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
        }
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Space (and Enter where there is nothing to type): a switch flips, a choice takes its next
    /// value, a step or a level is taken.
    fn settings_space(&mut self, mut s: Box<Settings>, rows: &[Row], value: &Value) {
        let set = |name: &str, v: Value| Map::from_iter([(name.to_string(), v)]);
        match rows.get(s.cursor) {
            Some(Row::Opt(o)) => {
                let changes = match kind(o, &self.cfg, value) {
                    Kind::Switch => set(o.name, Value::Bool(!value.as_bool().unwrap_or(false))),
                    Kind::TextSwitch(yes, no) => set(o.name, Value::from(if value.as_str() == Some(yes) { no } else { yes })),
                    Kind::Choice(c) => {
                        let i = c.iter().position(|(v, _)| value.as_str() == Some(v)).map_or(0, |i| (i + 1) % c.len());
                        set(o.name, Value::from(c[i].0.clone()))
                    }
                    _ => return self.dialog = Some(Dialog::Settings(s)),
                };
                return self.settings_change(s, changes);
            }
            Some(Row::Level(l)) => return self.settings_level(s, *l),
            Some(Row::SetUp) => self.setup_guide(),
            Some(Row::Guide) => return self.dialog = Some(Dialog::Guide(Box::default())),
            Some(Row::Tool(_, false, Some(line))) => {
                crate::guide::copy(line);
                s.said = Some((t!("guide.copied", "command" => line), false));
            }
            Some(Row::Status(line)) => match line.step {
                Some(Step::TurnOn) => return self.settings_level(s, Level::Text),
                Some(Step::Start) => self.index.restart(),
                Some(Step::ReadNow) => self.index.index_now(),
                Some(Step::SetUp) => self.setup_guide(),
                Some(Step::TryIt) if s.trial_rx.is_none() => {
                    let (tx, rx) = mpsc::channel();
                    let cfg = self.cfg.search.clone();
                    std::thread::spawn(move || tx.send(coxswain_core::setup::try_ask(&cfg)));
                    s.trial_rx = Some(rx);
                    s.trial = Some((t!("common.loading"), false));
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
        let new = match cs::apply(&text, &changes).and_then(|t| Config::parse(&t)) {
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
        if let Some(why) = coxswain_core::meaning::change_notice(&self.cfg.search, &new.search, st.meaning_done, st.meaning_passages) {
            s.mode = Mode::Confirm(why, changes);
            return self.dialog = Some(Dialog::Settings(s));
        }
        self.settings_save(s, changes)
    }

    /// Written to config.toml and used at once; the helper restarts when it reads one of them.
    fn settings_save(&mut self, mut s: Box<Settings>, changes: Map<String, Value>) {
        let Some(path) = s.path.clone() else {
            s.said = Some((t!("err.no_config_folder"), true));
            return self.dialog = Some(Dialog::Settings(s));
        };
        s.said = Some(match self.save_options(&path, &changes) {
            Ok(()) => (t!("settings.saved", "path" => path.display()), false),
            Err(e) => (e, true),
        });
        self.dialog = Some(Dialog::Settings(s));
    }

    /// Options written to the config.toml at `path` (comments kept) and used at once; the search
    /// helper restarts when it reads one of them.
    pub fn save_options(&mut self, path: &std::path::Path, changes: &Map<String, Value>) -> Result<(), String> {
        let cfg = cs::save_to(path, changes)?;
        self.theme = cfg.theme();
        self.glyphs = cfg.glyphs();
        self.cfg = cfg;
        if changes.keys().any(|k| cs::find(k).is_some_and(|o| o.restarts_helper())) {
            self.index.restart();
        }
        Ok(())
    }

    /// The search setup guide, on the plain terminal.
    pub fn setup_guide(&mut self) {
        let exe = coxswain_core::tools::this_app().map(|p| p.display().to_string()).unwrap_or_else(|_| "coxswain".into());
        self.run = Some(Run::Shell { cmd: format!("{} --setup-search", coxswain_core::config::quote(&exe)), dir: self.panel().dir.clone(), wait: true });
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

/// Full screen: areas left, rows right, the selected row explained at the bottom.
pub fn draw(f: &mut Frame, app: &mut App) {
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
    let body_h = inner.height - 4;
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
    let rows = rows(app, s);
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
            let pair = |label: &str, value: &str, st: Style| {
                // Without a value (a switch, a heading) the label has the whole width.
                let lw = if value.is_empty() { w } else { lw };
                Line::from(vec![Span::styled(format!("{} ", fit(label, lw - 1)), st), Span::styled(fit(value, w - lw), st)])
            };
            match row {
                Row::Head(text, _) => {
                    if !lines.is_empty() {
                        lines.push(Line::from(""));
                    }
                    lines.push(Line::from(Span::styled(fit(text, w), head)));
                }
                Row::Status(l) => {
                    let step = l.step.map(|x| format!(" [{}]", x.label())).unwrap_or_default();
                    let label = format!(" {}", fit(&l.label, 9));
                    let text = fit(&l.text, w.saturating_sub(label.width() + step.width() + 1));
                    lines.push(Line::from(vec![Span::styled(format!("{label} "), if here { selected } else { head }), Span::styled(text, st), Span::styled(step, st)]));
                }
                Row::Level(l) => {
                    let mark = if level == Some(*l) { "(•)" } else { "( )" };
                    lines.push(Line::from(Span::styled(fit(&format!(" {mark} {}", t!(&format!("settings.level.{}", l.id()))), w), st)));
                }
                Row::SetUp => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("setup.open")), w), st))),
                Row::Guide => lines.push(Line::from(Span::styled(fit(&format!(" [ {} ]", t!("guide.show_again")), w), st))),
                Row::Tool(name, there, line) => {
                    let value = if *there { String::new() } else { line.clone().unwrap_or_else(|| t!("settings.search_tool_missing")) };
                    lines.push(pair(&format!(" {} {}", if *there { "✓" } else { "✗" }, t!(&format!("settings.search_tool_{name}"))), &value, st));
                }
                Row::Service => {
                    let mark = if s.service { "[x]" } else { "[ ]" };
                    lines.push(Line::from(Span::styled(fit(&format!(" {mark} {}", t!("setup.service_on")), w), st)));
                }
                // A path keeps its tail, the informative part.
                Row::Info(label, value, _) if value.contains(std::path::MAIN_SEPARATOR) => lines.push(pair(&format!(" {label}"), &fit_left(value, w.saturating_sub(lw)), st)),
                Row::Info(label, value, _) => lines.push(pair(&format!(" {label}"), value, st)),
                Row::Go(area, value) => lines.push(Line::from(vec![Span::styled(fit(&format!(" {}", area.label()), aw.max(14)), if here { selected } else { head }), Span::styled(fit(value, w.saturating_sub(aw.max(14))), st)])),
                Row::Notice(n) => lines.push(Line::from(Span::styled(fit(&format!(" • {}", n.text), w), st))),
                Row::Opt(o) => {
                    let value = &values[o.name];
                    let label = match on(o, cfg, value) {
                        Some(true) => format!(" [x] {}", o.label()),
                        Some(false) => format!(" [ ] {}", o.label()),
                        None => format!("     {}", o.label()),
                    };
                    match &s.mode {
                        Mode::Edit(text) if here => {
                            let vw = w.saturating_sub(lw);
                            let field = fit_left(text, vw.saturating_sub(1));
                            cursor_at = Some(Position::new(right.x + (lw + field.width()) as u16, 0));
                            lines.push(Line::from(vec![Span::styled(format!("{} ", fit(&label, lw - 1)), base), Span::styled(fit(&field, vw), selected)]));
                        }
                        Mode::List { at, new } if here => {
                            lines.push(pair(&label, "", head));
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
                            lines.push(pair(&label, "", head));
                            if let Kind::Choice(c) = kind(o, cfg, value) {
                                for (j, (v, name)) in c.iter().enumerate() {
                                    let mark = if value.as_str() == Some(v.as_str()) { "(•)" } else { "( )" };
                                    if j == *at {
                                        cursor_line = lines.len();
                                    }
                                    lines.push(Line::from(Span::styled(fit(&format!("     {mark} {name}"), w), if j == *at { selected } else { base })));
                                }
                            }
                        }
                        _ => lines.push(pair(&label, &shown(o, cfg, value), st)),
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

    // The bottom: a rule, what the row is (two lines), the keys or what was done.
    let y = inner.y + body_h;
    let rule = format!("{}┴{}", "─".repeat(aw), "─".repeat((inner.width as usize).saturating_sub(aw + 1)));
    f.render_widget(Paragraph::new(rule).style(border), Rect { y, height: 1, ..inner });
    let Some(Dialog::Settings(s)) = &app.dialog else { return };
    // One column short: ratatui lets a wide letter that wraps at the edge spill over the frame.
    let explain = Rect { x: inner.x + 1, y: y + 1, height: 2, width: inner.width - 3 };
    let mut text: Vec<Span> = vec![];
    match &s.mode {
        Mode::Confirm(why, _) => text.push(Span::styled(format!("{why} Enter: {} · Esc: {}", t!("settings.meaning_change_go"), t!("settings.meaning_change_keep")), red)),
        Mode::Find { query, at } => {
            if let Some(o) = find(query).get(*at) {
                text.push(Span::raw(o.hint()));
            }
        }
        _ => match rows.get(s.cursor) {
            Some(Row::Opt(o)) => {
                text.push(Span::raw(o.hint()));
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
            Some(Row::Status(l)) => {
                let note = match (l.part, &s.trial) {
                    (cs::Part::Ask, Some((said, bad))) => Some((said.clone(), *bad)),
                    _ => l.note.clone().map(|n| (n, l.bad)),
                };
                match note {
                    Some((n, bad)) => text.push(Span::styled(n, if bad { red } else { base })),
                    None => text.push(Span::raw(l.text.clone())),
                }
            }
            Some(Row::SetUp) => text.push(Span::raw(t!("setup.open_hint"))),
            Some(Row::Guide) => text.push(Span::raw(t!("guide.show_again_hint"))),
            Some(Row::Tool(_, false, Some(line))) => text.push(Span::raw(t!("install.copy_tui", "command" => line))),
            Some(Row::Tool(name, false, None)) => text.push(Span::raw(t!("install.get", "program" => name))),
            Some(Row::Tool(name, true, _)) => text.push(Span::raw(t!(&format!("settings.search_tool_{name}")))),
            Some(Row::Service) => text.push(Span::raw(t!("setup.service_hint"))),
            Some(Row::Info(_, _, hint)) => text.push(Span::raw(hint.clone())),
            Some(Row::Go(_, value)) => text.push(Span::raw(value.clone())),
            Some(Row::Notice(n)) => text.push(Span::raw(n.text.clone())),
            _ => {}
        },
    }
    f.render_widget(Paragraph::new(Line::from(text)).wrap(Wrap { trim: true }), explain);
    let keys = Rect { x: inner.x + 1, y: y + 3, height: 1, width: inner.width - 2 };
    let (line, st) = match (&s.said, &s.mode) {
        (Some((said, bad)), _) => (said.clone(), if *bad { red } else { dim }),
        (None, Mode::Edit(_)) => (crate::ui::keys(&t!("verb.save")), dim),
        (None, Mode::List { .. }) => (t!("tui.settings.list_keys"), dim),
        (None, Mode::Pick(_)) => (t!("tui.settings.pick_keys"), dim),
        (None, _) => (t!("tui.settings.keys"), dim),
    };
    f.render_widget(Paragraph::new(fit(&line, keys.width as usize)).style(st), keys);
    if let Some(p) = cursor_at.filter(|p| p.y >= right.y && p.y < right.bottom()) {
        f.set_cursor_position(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coxswain_core::helper::Client;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app(dir: &std::path::Path) -> App {
        let cfg = Config { check_updates: false, language: "en-GB".into(), ..Config::default() };
        let index = Client::with(None, &cfg.search, || {});
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
        let path = d.join("config.toml");
        std::fs::write(&path, "# mine\nlanguage = \"en-GB\"\nshow_hidden = false # keep\n").unwrap();
        let mut app = app(&d);
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

        // A list: a folder added (resolved from the panel's folder), then removed.
        let mut s = Settings::open(&app, "names_only");
        s.path = Some(path.clone());
        app.dialog = Some(Dialog::Settings(Box::new(s)));
        key(&mut app, KeyCode::Enter);
        typed(&mut app, "mail");
        key(&mut app, KeyCode::Enter);
        let mail = std::fs::canonicalize(d.join("mail")).unwrap();
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
        let d = std::env::temp_dir();
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
}

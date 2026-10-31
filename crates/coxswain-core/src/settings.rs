//! Settings: every option the apps let the user change, described once. Each has its place in
//! config.toml, the area of Settings it sits in, and what it costs (disk, processor, network, data
//! that leaves the machine); its label and one-line explanation are the `setting.<name>` and
//! `setting.<name>.hint` texts. The desktop app's Settings window is built from this, and so is
//! the terminal app's. Here too: reading and writing the values, where `--settings=<name>`
//! opens, the search level, the status lines of search, and what can leave the machine.

use serde::Serialize;
use serde_json::Value;

use crate::config::{Config, SearchConfig};
use crate::helper::Status;
use crate::t;

/// An area of Settings, by what users come for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// How everything stands, the next steps, and what's new.
    Overview,
    /// Finding files: names, words in files, meaning, Ask.
    Search,
    Previews,
    /// Chess and the other board games: the board, the engine, analysing a game, the book.
    Games,
    Looks,
    Behaviour,
    /// Every action and its keys.
    Keys,
    /// The update check, what can leave the machine, where things are kept.
    Privacy,
}

impl Area {
    pub const ALL: [Area; 8] = [
        Area::Overview,
        Area::Search,
        Area::Previews,
        Area::Games,
        Area::Looks,
        Area::Behaviour,
        Area::Keys,
        Area::Privacy,
    ];

    /// Its name in `--settings=<name>` and in the `settings.area.<name>` text.
    pub fn id(self) -> &'static str {
        match self {
            Area::Overview => "overview",
            Area::Search => "search",
            Area::Previews => "previews",
            Area::Games => "games",
            Area::Looks => "looks",
            Area::Behaviour => "behaviour",
            Area::Keys => "keys",
            Area::Privacy => "privacy",
        }
    }

    pub fn label(self) -> String {
        crate::i18n::tr(&format!("settings.area.{}", self.id()), &[])
    }
}

/// What an option costs, shown as a badge beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    /// Room on the disk.
    Disk,
    /// Processor time, mostly in the background.
    Cpu,
    /// It downloads.
    Network,
    /// Something of the user's (text, questions) is sent to a server, which may be elsewhere.
    Leaves,
}

impl Cost {
    pub fn label(self) -> String {
        t!(match self {
            Cost::Disk => "settings.cost.disk",
            Cost::Cpu => "settings.cost.cpu",
            Cost::Network => "settings.cost.network",
            Cost::Leaves => "settings.cost.leaves",
        })
    }
}

/// One option.
#[derive(Debug, Serialize)]
pub struct Opt {
    /// What the apps call it: `search_text`. Its texts are `setting.<name>` and `setting.<name>.hint`.
    pub name: &'static str,
    /// Where it lives in config.toml: `["search", "text"]`.
    pub path: &'static [&'static str],
    pub area: Area,
    pub costs: &'static [Cost],
}

impl Opt {
    pub fn label(&self) -> String {
        crate::i18n::tr(&format!("setting.{}", self.name), &[])
    }

    pub fn hint(&self) -> String {
        crate::i18n::tr(&format!("setting.{}.hint", self.name), &[])
    }

    /// The `[search]` options the helper reads when it starts, and `[preview] ebooks` (calibre
    /// reads e-books for the search): a change restarts it. Ask's are read by the app itself.
    pub fn restarts_helper(&self) -> bool {
        (self.path[0] == "search" && !self.name.starts_with("ask_")) || self.name == "ebooks"
    }

    /// Optional in config.toml: empty means the key is taken out (the default applies).
    fn optional(&self) -> bool {
        matches!(self.name, "editor" | "viewer" | "terminal" | "git_tool" | "chess_book" | "chess_engine")
    }
}

const fn opt(
    name: &'static str,
    path: &'static [&'static str],
    area: Area,
    costs: &'static [Cost],
) -> Opt {
    Opt {
        name,
        path,
        area,
        costs,
    }
}

use Area::*;
use Cost::*;

/// Every option, by area.
pub const OPTIONS: &[Opt] = &[
    opt("search_text", &["search", "text"], Search, &[Disk, Cpu]),
    opt(
        "search_meaning",
        &["search", "meaning"],
        Search,
        &[Disk, Cpu, Network],
    ),
    opt("search_archives", &["search", "archives"], Search, &[Cpu]),
    opt(
        "search_archives_everywhere",
        &["search", "archives_everywhere"],
        Search,
        &[Disk, Cpu],
    ),
    opt("search_history", &["search", "history"], Search, &[Disk]),
    opt("search_commit_changes", &["search", "commit_changes"], Search, &[Disk, Cpu]),
    opt("search_commit_changes_in", &["search", "commit_changes_in"], Search, &[Disk, Cpu]),
    opt("search_cloud", &["search", "cloud"], Search, &[Network]),
    opt("cloud_read", &["search", "cloud_read"], Search, &[Network]),
    opt("search_tika", &["search", "tika"], Search, &[Disk, Cpu, Network]),
    opt("search_whisper", &["search", "whisper"], Search, &[Disk, Cpu, Network]),
    opt("whisper_model", &["search", "whisper_model"], Search, &[Disk]),
    opt("text_max_size", &["search", "text_max_size"], Search, &[]),
    opt("max_results", &["search", "max_results"], Search, &[]),
    opt("text_roots", &["search", "text_roots"], Search, &[]),
    opt("names_only", &["search", "names_only"], Search, &[]),
    opt("text_exclude", &["search", "text_exclude"], Search, &[]),
    opt("name_roots", &["search", "name_roots"], Search, &[]),
    opt("name_exclude", &["search", "name_exclude"], Search, &[]),
    opt("watch", &["search", "watch"], Search, &[Cpu]),
    opt("meaning_engine", &["search", "meaning_engine"], Search, &[]),
    opt("meaning_url", &["search", "meaning_url"], Search, &[Leaves]),
    opt("meaning_model", &["search", "meaning_model"], Search, &[]),
    opt(
        "meaning_key_env",
        &["search", "meaning_key_env"],
        Search,
        &[],
    ),
    opt("meaning_device", &["search", "meaning_device"], Search, &[]),
    opt("long_files", &["search", "long_files"], Search, &[Disk, Cpu]),
    opt("long_files_everything", &["search", "long_files_everything"], Search, &[Disk, Cpu]),
    opt("long_files_sample", &["search", "long_files_sample"], Search, &[]),
    opt("ask_model", &["search", "ask_model"], Search, &[Leaves]),
    opt(
        "ask_code_model",
        &["search", "ask_code_model"],
        Search,
        &[Leaves],
    ),
    opt("ask_think", &["search", "ask_think"], Search, &[]),
    opt("ask_context", &["search", "ask_context"], Search, &[]),
    opt("preview_prefer", &["preview", "prefer"], Previews, &[]),
    opt(
        "preview_container",
        &["preview", "container"],
        Previews,
        &[],
    ),
    opt(
        "latex_image",
        &["preview", "images", "latex"],
        Previews,
        &[Disk, Network],
    ),
    opt("latex_auto", &["preview", "latex_auto"], Previews, &[Cpu]),
    opt("drawio", &["preview", "drawio"], Previews, &[Disk, Network, Cpu]),
    opt(
        "image_updates",
        &["preview", "image_updates"],
        Previews,
        &[Network, Disk],
    ),
    opt("image_prune", &["preview", "image_prune"], Previews, &[]),
    opt("ebooks", &["preview", "ebooks"], Previews, &[Disk, Cpu, Network]),
    opt("virus_scan", &["preview", "virus_scan"], Previews, &[Disk, Network]),
    opt("preview_timeout", &["preview", "timeout"], Previews, &[]),
    opt("chess_notation", &["chess", "notation"], Games, &[]),
    opt("chess_board", &["chess", "board"], Games, &[]),
    opt("chess_book", &["chess", "book"], Games, &[]),
    opt("chess_pieces", &["chess", "pieces"], Games, &[]),
    opt("chess_analysis", &["chess", "analysis"], Games, &[Cpu]),
    opt("chess_engine", &["chess", "engine"], Games, &[]),
    opt("chess_engine_limit", &["chess", "engine_limit"], Games, &[]),
    opt("chess_engine_time", &["chess", "engine_time"], Games, &[]),
    opt("chess_engine_depth", &["chess", "engine_depth"], Games, &[]),
    opt("chess_engine_nodes", &["chess", "engine_nodes"], Games, &[]),
    opt("chess_engine_lines", &["chess", "engine_lines"], Games, &[]),
    opt("chess_review_limit", &["chess", "review_limit"], Games, &[]),
    opt("chess_review_time", &["chess", "review_time"], Games, &[]),
    opt("chess_review_depth", &["chess", "review_depth"], Games, &[]),
    opt("chess_review_nodes", &["chess", "review_nodes"], Games, &[]),
    opt("language", &["language"], Looks, &[]),
    opt("theme", &["gui", "theme"], Looks, &[]),
    opt("tui_theme", &["theme"], Looks, &[]),
    opt("glyphs", &["glyphs"], Looks, &[]),
    opt("font", &["gui", "font"], Looks, &[]),
    opt("mono_font", &["gui", "mono_font"], Looks, &[]),
    opt("icon_font", &["gui", "icon_font"], Looks, &[]),
    opt("font_size", &["gui", "font_size"], Looks, &[]),
    opt("line_height", &["gui", "line_height"], Looks, &[]),
    opt("show_hidden", &["show_hidden"], Behaviour, &[]),
    opt("confirm_delete", &["confirm_delete"], Behaviour, &[]),
    opt("right_click", &["right_click"], Behaviour, &[]),
    opt("hints", &["hints"], Behaviour, &[]),
    opt("folder_sizes", &["folder_sizes"], Behaviour, &[Cpu]),
    opt("disk_images", &["archive", "disk_images"], Behaviour, &[]),
    opt("repos_order", &["gui", "repos_order"], Behaviour, &[]),
    opt(
        "git_last_commit",
        &["git", "last_commit"],
        Behaviour,
        &[Cpu],
    ),
    opt("git_tool", &["git", "tool"], Behaviour, &[]),
    opt("git_prefer_git", &["git", "prefer_git"], Behaviour, &[]),
    opt("editor", &["editor"], Behaviour, &[]),
    opt("viewer", &["viewer"], Behaviour, &[]),
    opt("terminal", &["terminal"], Behaviour, &[]),
    opt("bom_viewer", &["bom_viewer"], Behaviour, &[]),
    opt("provenance_viewer", &["provenance_viewer"], Behaviour, &[]),
    opt("check_updates", &["check_updates"], Privacy, &[Network]),
];

/// The option called `name`.
pub fn find(name: &str) -> Option<&'static Opt> {
    OPTIONS.iter().find(|o| o.name == name)
}

/// A group of an area's options under a heading, the same in both apps: its name (where
/// `Row::Head` and the desktop app's block are found), its heading's text key (none: the
/// area's first options, without a heading), its options in the order shown.
#[derive(Debug, Serialize)]
pub struct Group {
    pub area: Area,
    pub id: &'static str,
    pub head: Option<&'static str>,
    pub options: &'static [&'static str],
}

impl Group {
    pub fn label(&self) -> String {
        self.head.map(|h| crate::i18n::tr(h, &[])).unwrap_or_default()
    }
}

const fn group(area: Area, id: &'static str, head: Option<&'static str>, options: &'static [&'static str]) -> Group {
    Group { area, id, head, options }
}

/// Every option in exactly one group, by area (a test checks it). Go, draughts, shogi and
/// xiangqi have no settings of their own: *Board* is theirs too.
pub const GROUPS: &[Group] = &[
    group(Search, "reads", Some("settings.group.reads"), &["search_text", "search_archives", "search_archives_everywhere", "search_history", "search_commit_changes", "search_commit_changes_in", "search_cloud", "cloud_read", "search_tika", "search_whisper", "whisper_model", "text_max_size", "max_results"]),
    group(Search, "folders", Some("settings.group.folders"), &["text_roots", "names_only", "text_exclude", "name_roots", "name_exclude", "watch"]),
    group(Search, "meaning", Some("settings.group.meaning"), &["search_meaning", "meaning_engine", "meaning_url", "meaning_model", "meaning_key_env", "meaning_device", "long_files", "long_files_everything", "long_files_sample"]),
    group(Search, "ask", Some("settings.group.ask"), &["ask_model", "ask_code_model", "ask_think", "ask_context"]),
    group(Previews, "previews_general", Some("settings.group.general"), &["preview_prefer", "preview_timeout"]),
    group(Previews, "documents", Some("settings.group.documents"), &["latex_auto", "ebooks", "drawio"]),
    group(Previews, "images", Some("settings.group.images"), &["preview_container", "latex_image", "image_updates", "image_prune"]),
    group(Previews, "safety", Some("settings.group.safety"), &["virus_scan"]),
    group(Games, "board", Some("settings.group.board"), &["chess_board", "chess_pieces", "chess_notation"]),
    group(Games, "chess_engines", Some("chess.engines_title"), &["chess_analysis", "chess_engine", "chess_engine_limit", "chess_engine_time", "chess_engine_depth", "chess_engine_nodes", "chess_engine_lines"]),
    group(Games, "review", Some("settings.group.review"), &["chess_review_limit", "chess_review_time", "chess_review_depth", "chess_review_nodes"]),
    group(Games, "book", Some("settings.group.book"), &["chess_book"]),
    group(Looks, "looks_both", None, &["language", "glyphs"]),
    group(Looks, "looks_tui", Some("settings.theme_tui"), &["tui_theme"]),
    group(Looks, "looks_gui", Some("settings.theme_gui"), &["theme", "font", "mono_font", "icon_font", "font_size", "line_height"]),
    group(Behaviour, "panels", Some("settings.group.panels"), &["show_hidden", "confirm_delete", "right_click", "hints", "folder_sizes", "disk_images", "repos_order"]),
    group(Behaviour, "git", Some("settings.group.git"), &["git_last_commit", "git_tool", "git_prefer_git"]),
    group(Behaviour, "programs", Some("settings.group.programs"), &["editor", "viewer", "terminal", "bom_viewer", "provenance_viewer"]),
    group(Privacy, "updates", None, &["check_updates"]),
];

/// The groups of `area`, in order.
pub fn groups(area: Area) -> impl Iterator<Item = &'static Group> {
    GROUPS.iter().filter(move |g| g.area == area)
}

impl Opt {
    /// The group it is shown in.
    pub fn group(&self) -> Option<&'static Group> {
        GROUPS.iter().find(|g| g.options.contains(&self.name))
    }

    /// Its place in config.toml as written there: `[search] text`, or `language` at the top.
    pub fn key(&self) -> String {
        match self.path.split_last() {
            Some((last, [])) => last.to_string(),
            Some((last, table)) => format!("[{}] {last}", table.join(".")),
            None => String::new(),
        }
    }
}

/// *Find a setting*: the options whose label, explanation, group heading (in the current
/// language) or config.toml key hold every word of `query`, case and accents aside (`Größe` finds
/// `grosse`, `[chess] engine`, `chess.engine` and `chess_engine` all find the engine), in
/// the order Settings shows them.
pub fn search(query: &str) -> Vec<&'static Opt> {
    let words: Vec<String> = crate::features::fold(query).split_whitespace().map(String::from).collect();
    if words.is_empty() {
        return vec![];
    }
    GROUPS
        .iter()
        .flat_map(|g| g.options.iter().filter_map(|n| find(n)))
        .filter(|o| {
            let group = o.group().map(|g| g.label()).unwrap_or_default();
            let hay = crate::features::fold(&[o.label(), o.hint(), o.name.to_string(), o.path.join("."), o.key(), group].join("\n"));
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

/// The value of every option, by name, as config.toml holds it now (an unset optional one as
/// an empty text).
pub fn values(cfg: &Config) -> serde_json::Map<String, Value> {
    let all = serde_json::to_value(cfg).unwrap_or_default();
    OPTIONS
        .iter()
        .map(|o| {
            let v = o
                .path
                .iter()
                .try_fold(&all, |v, k| v.get(k))
                .cloned()
                .unwrap_or(Value::Null);
            (
                o.name.to_string(),
                if v.is_null() {
                    Value::String(String::new())
                } else {
                    v
                },
            )
        })
        .collect()
}

/// `text` (a config.toml) with the options in `changes` set, its comments and layout kept.
pub fn apply(text: &str, changes: &serde_json::Map<String, Value>) -> Result<String, String> {
    let mut text = text.to_string();
    for (name, v) in changes {
        let o = find(name).ok_or_else(|| t!("err.unknown_setting", "name" => name))?;
        if o.optional() && v.as_str() == Some("") {
            text = Config::unset(&text, o.path)?;
            continue;
        }
        let value: toml_edit::Value = match v {
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => match n.as_i64() {
                Some(i) => i.into(),
                None => n
                    .as_f64()
                    .ok_or_else(|| t!("err.unsupported_value", "name" => name))?
                    .into(),
            },
            Value::String(s) => s.as_str().into(),
            Value::Array(a) => {
                let texts: Option<Vec<&str>> = a.iter().map(|v| v.as_str()).collect();
                texts
                    .ok_or_else(|| t!("err.unsupported_value", "name" => name))?
                    .into_iter()
                    .collect::<toml_edit::Array>()
                    .into()
            }
            _ => return Err(t!("err.unsupported_value", "name" => name)),
        };
        text = Config::edit(&text, o.path, value)?;
    }
    Ok(text)
}

/// Write `changes` into the user's config.toml (comments and layout kept) and use the new
/// language at once. Only what parses is written, and through a file beside it, so a broken or
/// half-written config never replaces a working one. The caller restarts the helper when an
/// option says so (`Opt::restarts_helper`).
pub fn save(changes: &serde_json::Map<String, Value>) -> Result<Config, String> {
    save_to(
        &Config::path().ok_or_else(|| t!("err.no_config_folder"))?,
        changes,
    )
}

/// `save`, to the config.toml at `path`.
pub fn save_to(
    path: &std::path::Path,
    changes: &serde_json::Map<String, Value>,
) -> Result<Config, String> {
    let cfg = crate::config::update_config(path, |text| {
        let new_text = apply(text, changes)?;
        let cfg = Config::parse_saved(text, &new_text)?;
        Ok((Some(new_text), cfg))
    })?;
    // Only a save of the language changes it: the language is the whole process's, and a save
    // of another option must not swap it (a test saving one, on a Mac set to US English).
    if changes.contains_key("language") {
        crate::i18n::set_language(crate::i18n::resolve(&cfg.language));
    }
    crate::disk_image::set_open(cfg.archive.disk_images);
    Ok(cfg)
}

/// Where Settings opens for `--settings=<name>` and a notice's *Show me*: the area, and the
/// option to show in it. An area's name or an option's; anything else, the Overview.
pub fn open_at(section: &str) -> (Area, Option<&'static str>) {
    if let Some(a) = Area::ALL.into_iter().find(|a| a.id() == section) {
        return (a, None);
    }
    if let Some((g, area)) = PLACES.iter().find(|g| g.0 == section) {
        return (*area, Some(g));
    }
    match find(section) {
        Some(o) => (o.area, Some(o.name)),
        None => (Overview, None),
    }
}

/// Groups that Settings opens at by name, in both apps, with their area (1.x's names of
/// sections, `meaning` and `ask`, open Overview).
const PLACES: [(&str, Area); 4] = [("models", Search), ("disk", Privacy), ("preview_tools", Previews), ("chess_engines", Games)];

/// Every name `open_at` knows, with where it opens: for the desktop app, which opens Settings
/// at sections given by notices too.
pub fn sections() -> Vec<(&'static str, Area, Option<&'static str>)> {
    let names = Area::ALL
        .iter()
        .map(|a| a.id())
        .chain(PLACES.map(|g| g.0))
        .chain(OPTIONS.iter().map(|o| o.name));
    names.map(|n| (n, open_at(n).0, open_at(n).1)).collect()
}

// ---------------------------------------------------------------- the search level

/// How far Find looks: a preset of the three switches that matter most.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// File names, on the whole machine. Nothing is opened.
    Names,
    /// Also the words inside the files read.
    Text,
    /// Also files about the words, in any language.
    Meaning,
    /// Also questions answered by a chat model.
    Ask,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Names, Level::Text, Level::Meaning, Level::Ask];

    pub fn id(self) -> &'static str {
        match self {
            Level::Names => "names",
            Level::Text => "text",
            Level::Meaning => "meaning",
            Level::Ask => "ask",
        }
    }

    /// What it costs, beyond the level below it.
    pub fn costs(self) -> &'static [Cost] {
        match self {
            Level::Names => &[],
            Level::Text => &[Disk, Cpu],
            Level::Meaning => &[Disk, Cpu, Network],
            Level::Ask => &[Leaves],
        }
    }
}

/// The level the settings make, or none for a mix no level makes (meaning on, text off).
pub fn level(s: &SearchConfig) -> Option<Level> {
    match (s.text, s.meaning, s.ask_model.is_empty()) {
        (false, false, _) => Some(Level::Names),
        (true, false, _) => Some(Level::Text),
        (true, true, true) => Some(Level::Meaning),
        (true, true, false) => Some(Level::Ask),
        (false, true, _) => None,
    }
}

/// What choosing `level` changes, as options to save; `None` when it needs something chosen or
/// installed first (a model for meaning, a chat model for Ask): then the search setup guide
/// opens instead. `builtin_ready`: the built-in model is downloaded. Going down a level keeps
/// what was set up (the model stays; deleting it is a button of its own), except that leaving
/// Ask forgets its chat model, as that is what turns Ask on.
pub fn level_changes(
    level: Level,
    s: &SearchConfig,
    builtin_ready: bool,
) -> Option<serde_json::Map<String, Value>> {
    let meaning_ready = s.meaning_engine != "builtin" || builtin_ready;
    let mut m = serde_json::Map::new();
    let mut set = |k: &str, v: Value| {
        m.insert(k.to_string(), v);
    };
    match level {
        Level::Names => {
            set("search_text", false.into());
            set("search_meaning", false.into());
        }
        Level::Text => {
            set("search_text", true.into());
            set("search_meaning", false.into());
        }
        Level::Meaning | Level::Ask if !meaning_ready => return None,
        Level::Ask if s.ask_model.is_empty() => return None,
        Level::Meaning | Level::Ask => {
            set("search_text", true.into());
            set("search_meaning", true.into());
            if level == Level::Meaning && !s.ask_model.is_empty() {
                set("ask_model", "".into());
            }
        }
    }
    Some(m)
}

// ---------------------------------------------------------------- how search stands

/// A part of search, in the status lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Names,
    Text,
    Meaning,
    Ask,
    /// Apache Tika, for other formats: only while it is turned on.
    Tika,
    /// Transcripts with Whisper: only while it is turned on.
    Whisper,
}

/// The one step a status line offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Turn on the level that has this part.
    TurnOn,
    /// Start background reading.
    Start,
    /// Read what waits, at full speed.
    ReadNow,
    /// Open the search setup guide.
    SetUp,
    /// Ask the chat model a test question.
    TryIt,
    /// Time the model on a few passages.
    Bench,
    /// Transcribing: pause, go on, or let every file waiting go.
    Pause,
    Resume,
    StopTranscribing,
}

impl Step {
    pub fn label(self) -> String {
        t!(match self {
            Step::TurnOn => "settings.step.turn_on",
            Step::Start => "settings.step.start",
            Step::ReadNow => "settings.step.read_now",
            Step::SetUp => "settings.step.set_up",
            Step::TryIt => "settings.step.try_it",
            Step::Bench => "settings.step.bench",
            Step::Pause => "settings.step.pause",
            Step::Resume => "settings.step.resume",
            Step::StopTranscribing => "settings.step.stop_transcribing",
        })
    }
}

/// One line of the status block: what a part is doing, a second line when something needs
/// saying, and its next step.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub part: Part,
    pub label: String,
    pub text: String,
    pub note: Option<String>,
    /// The note is a problem, not news.
    pub bad: bool,
    pub step: Option<Step>,
    /// A second step beside it: "Let the rest go" while transcribing or paused.
    pub also: Option<Step>,
    /// The model chosen is a poor choice here: what to say, and the better one.
    pub poor: Option<crate::setup::Poor>,
    /// Work under way: files done of all, for a progress bar.
    pub bar: Option<(usize, usize)>,
}

/// A progress bar of `width` cells for the terminal: `done` of `total`, filled from the left.
pub fn bar(done: usize, total: usize, width: usize) -> String {
    let full = if total == 0 {
        width
    } else {
        (done.min(total) * width) / total
    };
    format!("{}{}", "█".repeat(full), "░".repeat(width - full))
}

/// Whole percent of `done` of `total`, never 100 while something is left.
pub fn percent(done: usize, total: usize) -> usize {
    if total == 0 {
        100
    } else {
        ((done.min(total) * 100) / total).min(if done < total { 99 } else { 100 })
    }
}

/// A count with its thousands set apart by a narrow space, which reads right in every language:
/// 912 330.
pub fn count(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    crate::i18n::digits(out)
}

/// Bytes as the desktop app writes them: exact below 10 KB, then one decimal.
pub fn human(n: u64) -> String {
    crate::i18n::digits(human_latin(n))
}

fn human_latin(n: u64) -> String {
    if n < 10_240 {
        return format!("{n} {}", t!("unit.B"));
    }
    let mut v = n as f64;
    for u in ["KB", "MB", "GB", "TB", "PB", "EB"] {
        v /= 1024.0;
        if v < 1024.0 || u == "EB" {
            return if v < 100.0 {
                format!("{v:.1} {}", crate::i18n::tr(&format!("unit.{u}"), &[]))
            } else {
                format!("{v:.0} {}", crate::i18n::tr(&format!("unit.{u}"), &[]))
            };
        }
    }
    unreachable!("the last unit takes any size")
}

/// The server's address without its scheme and path: `localhost:11434`.
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    rest.rsplit_once('@').map_or(rest, |(_, h)| h)
}

/// Whether the server at `url` is this machine.
pub fn is_local(url: &str) -> bool {
    let h = host(url);
    let name = if h.starts_with('[') {
        h.split(']').next().map(|n| &n[1..]).unwrap_or(h)
    } else {
        h.split(':').next().unwrap_or(h)
    };
    matches!(name, "localhost" | "::1") || name.starts_with("127.")
}

/// The server that makes the vectors (or Ask's, with the built-in model: Ollama here).
pub fn server_url(s: &SearchConfig) -> String {
    if s.meaning_url.is_empty() && s.meaning_engine != "openai" {
        crate::meaning::OLLAMA.to_string()
    } else {
        s.meaning_url.clone()
    }
}

/// A server error in words with the step it calls for, not as it came: a server that does not
/// answer is said to be so.
fn cause(why: &str, url: &str) -> String {
    let w = why.to_lowercase();
    if [
        "connection refused",
        "connect",
        "dns",
        "timed out",
        "unreachable",
    ]
    .iter()
    .any(|k| w.contains(k))
    {
        t!("settings.status.down", "host" => host(url))
    } else {
        t!("settings.status.failed", "why" => why)
    }
}

/// The status block of Finding files, from the settings and the helper's status. `helper`:
/// the helper answers (without it there is no reading). `poor`: the models chosen that are
/// poor choices here (`setup::poor`), each said on its line for as long as it stands.
pub fn status(
    s: &SearchConfig,
    st: &Status,
    helper: bool,
    poor: &[crate::setup::Poor],
) -> Vec<Line> {
    let line = |part, key: &str, text: String| Line {
        part,
        label: t!(key),
        text,
        note: None,
        bad: false,
        step: None,
        also: None,
        poor: None,
        bar: None,
    };
    let mut names = line(
        Part::Names,
        "settings.status.names",
        t!("settings.status.names_ready", "n" => count(st.len)),
    );
    if st.state != crate::index::State::Ready {
        names.text = t!("settings.status.names_building", "n" => count(st.len));
    }
    let mut text = line(
        Part::Text,
        "settings.status.text",
        t!("settings.status.off"),
    );
    if !s.text {
        text.note = Some(t!("settings.status.text_off"));
        text.step = Some(Step::TurnOn);
    } else if !helper {
        text.text = t!("settings.status.not_running");
        text.note = Some(t!("settings.status.text_no_helper"));
        text.bad = true;
        text.step = Some(Step::Start);
    } else {
        text.text = t!("settings.status.text_ready", "texts" => count(st.texts), "pending" => count(st.pending), "size" => human(st.bytes));
        if let Some(why) = &st.error {
            text.note = Some(t!("settings.status.text_error", "why" => why));
            text.bad = true;
        } else if st.paused {
            text.note = Some(t!("settings.status.paused"));
        }
        if st.pending > 0 {
            text.step = Some(Step::ReadNow);
        }
        // The changes of commits being read, or what taking them out freed (for ten minutes).
        if text.note.is_none() && st.changes_left > 0 {
            text.note = Some(t!("changes.reading", "n" => count(st.changes_left)));
        } else if let Some((freed, at)) = st.changes_freed.filter(|(_, at)| crate::store::now().saturating_sub(*at) < 600) {
            text.note = text.note.take().or_else(|| Some(t!("changes.freed", "size" => human(freed), "at" => crate::i18n::date_time(at))));
        }
    }
    let url = server_url(s);
    let mut meaning = line(
        Part::Meaning,
        "settings.status.meaning",
        t!("settings.status.off"),
    );
    if !s.meaning {
        meaning.note = Some(t!("settings.status.meaning_off"));
        meaning.step = Some(Step::SetUp);
    } else {
        let by = if s.meaning_engine == "builtin" {
            t!("settings.status.builtin", "where" => st.meaning_runs.as_ref().map(|r| r.text()).unwrap_or_else(|| t!("meaning.on_cpu")))
        } else {
            let model = if s.meaning_model.is_empty() {
                "bge-m3"
            } else {
                &s.meaning_model
            };
            t!("settings.status.server", "model" => model, "host" => host(&url))
        };
        let total = st.meaning_done + st.meaning_pending;
        meaning.text = t!("settings.status.meaning_ready", "done" => count(st.meaning_done), "total" => count(total), "by" => by);
        if let Some(why) = &st.meaning_error {
            meaning.note = Some(cause(why, &url));
            meaning.bad = true;
            meaning.step = Some(Step::SetUp);
        } else if st.meaning_pending > 0 {
            meaning.bar = Some((st.meaning_done, total));
            if st.meaning_ms_per_passage > 0 {
                meaning.note = Some(
                    t!("settings.status.meaning_left", "time" => crate::notices::time_left(st)),
                );
            }
        }
        if st.meaning_error.is_none() && st.meaning_changing > 0 {
            let changing = crate::tn!("settings.status.meaning_changing", st.meaning_changing);
            meaning.note = Some(match meaning.note.take() {
                Some(left) => format!("{left} {changing}"),
                None => changing,
            });
        }
        // The model's speed here, and what the files to go take at it: measured on request.
        if meaning.step.is_none() {
            meaning.step = Some(Step::Bench);
        }
    }
    let mut ask = line(Part::Ask, "settings.status.ask", t!("settings.status.off"));
    match crate::find::ask_ready(s) {
        Err(crate::find::Off::AskNeedsMeaning) => {
            ask.note = Some(t!("settings.status.ask_needs_meaning"));
            ask.step = Some(Step::SetUp);
        }
        Err(_) => {
            ask.note = Some(t!("settings.status.ask_no_model"));
            ask.step = Some(Step::SetUp);
        }
        Ok(()) => {
            ask.text = match crate::chat::of(&s.ask_model) {
                Some(_) => crate::chat::shown(&s.ask_model),
                None => t!("settings.status.server", "model" => s.ask_model, "host" => host(&url)),
            };
            ask.step = Some(Step::TryIt);
            ask.poor = poor.iter().find(|p| p.ask).cloned();
        }
    }
    if s.meaning && st.meaning_error.is_none() {
        meaning.poor = poor.iter().find(|p| !p.ask).cloned();
    }
    let mut lines = vec![names, text, meaning, ask];
    if s.tika && s.text {
        use crate::extract::tika::State;
        let image = crate::images::short(&crate::config::PreviewConfig::default().images["tika"]).to_string();
        let mut tika = line(Part::Tika, "settings.status.tika", t!("settings.status.not_running"));
        match st.tika.as_ref().filter(|_| helper) {
            None => {}
            Some(State::Ready) => tika.text = t!("settings.status.tika_ready"),
            Some(State::Pulling(progress)) => {
                tika.text = t!("settings.status.tika_pulling", "image" => image);
                tika.note = (!progress.is_empty()).then(|| progress.clone());
            }
            Some(State::NoRuntime) => {
                tika.text = t!("settings.status.off");
                tika.note = Some(t!("settings.status.tika_no_runtime"));
                tika.bad = true;
            }
            Some(State::Waiting { fault, at }) => {
                tika.text = t!("settings.status.tika_waiting");
                tika.note = Some(t!("settings.status.tika_again", "fault" => fault.text(), "time" => crate::i18n::time(*at)));
            }
            Some(State::Unpulled(fault)) => {
                tika.text = t!("settings.status.off");
                tika.note = Some(t!("settings.status.tika_unpulled", "fault" => fault.text()));
                tika.bad = true;
            }
            Some(State::Failed(why)) => {
                tika.text = t!("settings.status.off");
                tika.note = Some(t!("settings.status.failed", "why" => why));
                tika.bad = true;
            }
        }
        lines.push(tika);
    }
    if s.whisper && s.text {
        use crate::transcribe::State;
        let mut w = line(Part::Whisper, "settings.status.whisper", t!("settings.status.not_running"));
        if let Some(ts) = st.transcripts.as_ref().filter(|_| helper) {
            w.text = t!("settings.status.whisper_counts", "done" => count(ts.done), "waiting" => count(ts.waiting));
            match &ts.state {
                State::Idle => {}
                State::Working => {
                    if let Some((path, pct)) = &ts.current {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        w.note = Some(t!("settings.status.whisper_working", "name" => name, "percent" => pct));
                    }
                    if let Some(left) = ts.left.filter(|l| *l > 0) {
                        let left = t!("settings.status.whisper_left", "time" => crate::transcribe::span(left));
                        w.note = Some(w.note.take().map_or(left.clone(), |n| format!("{n} {left}")));
                    }
                    w.step = Some(Step::Pause);
                }
                State::Paused => {
                    w.note = Some(t!("settings.status.whisper_paused"));
                    w.step = Some(Step::Resume);
                }
                State::Battery => w.note = Some(t!("settings.status.paused")),
                State::Downloading(done, total) => {
                    w.note = Some(t!("settings.status.whisper_downloading", "done" => human(*done), "total" => human(*total)));
                    w.bar = Some((*done as usize, *total as usize));
                }
                State::Pulling(progress) => {
                    let image = crate::images::short(&crate::config::PreviewConfig::default().images["whisper"]).to_string();
                    w.note = Some(format!("{} {progress}", t!("settings.status.tika_pulling", "image" => image)).trim().to_string());
                }
                State::Off(why) => {
                    w.note = Some(t!("settings.status.failed", "why" => why));
                    w.bad = true;
                }
            }
            if ts.waiting > 0 {
                match w.step {
                    None => w.step = Some(Step::StopTranscribing),
                    Some(Step::Pause | Step::Resume) => w.also = Some(Step::StopTranscribing),
                    Some(_) => {}
                }
            }
        }
        lines.push(w);
    }
    lines
}

// ---------------------------------------------------------------- privacy

/// Something that can leave the machine, and where to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Out {
    pub what: String,
    pub to: String,
    /// The server is this machine: nothing leaves it after all.
    pub local: bool,
}

/// What the settings as they are can send or fetch, and where: the update check, the built-in
/// model's download, the model server, Ask, cloud files, container images.
pub fn outbound(cfg: &Config) -> Vec<Out> {
    let out = |key: &str, to: &str, local| Out {
        what: t!(key),
        to: to.to_string(),
        local,
    };
    let s = &cfg.search;
    let mut v = vec![];
    if cfg.check_updates {
        v.push(out("settings.out.update", &crate::update::host(), false));
    }
    let url = server_url(s);
    if s.meaning && s.meaning_engine == "builtin" && !crate::meaning::installed() {
        v.push(out("settings.out.model", "huggingface.co", false));
    }
    if s.meaning && s.meaning_engine != "builtin" {
        v.push(out("settings.out.meaning", host(&url), is_local(&url)));
    }
    match crate::chat::of(&s.ask_model) {
        Some(m) if !m.installed() => {
            v.push(out("settings.out.chat_model", "huggingface.co", false))
        }
        Some(_) => {}
        None if crate::find::ask_ready(s).is_ok() => {
            v.push(out("settings.out.ask", host(&url), is_local(&url)))
        }
        None => {}
    }
    if s.cloud == "all" || !s.cloud_read.is_empty() {
        v.push(out(
            "settings.out.cloud",
            &t!("settings.out.cloud_where"),
            false,
        ));
    }
    if s.whisper && s.text {
        v.push(out("settings.out.whisper", "huggingface.co", false));
        if let Ok(crate::transcribe::Engine::Container { image, .. }) = crate::transcribe::engine(cfg) {
            v.push(out("settings.out.whisper_image", registry(&image), false));
        }
    }
    if s.tika && s.text && cfg.preview.container != "off" {
        let image = cfg.preview.images.get("tika").map_or("", String::as_str);
        if !image.is_empty() {
            v.push(out("settings.out.tika", registry(image), false));
        }
    }
    if cfg.preview.prefer != "local" && cfg.preview.container != "off" {
        let mut hosts: Vec<&str> = cfg
            .preview
            .images
            .values()
            .filter(|i| !i.is_empty())
            .map(|i| registry(i))
            .collect();
        hosts.sort();
        hosts.dedup();
        if !hosts.is_empty() {
            let what = match crate::images::every(&cfg.preview.image_updates) {
                Some(7) => "settings.out.images_weekly",
                Some(_) => "settings.out.images_monthly",
                None => "settings.out.images",
            };
            v.push(out(what, &hosts.join(", "), false));
        }
    }
    if cfg.preview.virus_scan {
        v.push(out("settings.out.viruses", crate::viruses::SERVER, false));
    }
    v
}

/// The registry a container image comes from: `docker.io` unless its name starts with one.
pub(crate) fn registry(image: &str) -> &str {
    match image.split_once('/') {
        Some((first, _)) if first.contains(['.', ':']) || first == "localhost" => first,
        _ => "docker.io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_every_option_is_explained_in_every_language() {
        // Texts of their own: every language but the variants (English, German, French, …),
        // which take the rest from British English or their own language.
        for l in crate::i18n::LANGUAGES
            .iter()
            .filter(|l| !l.code.starts_with("en-") && crate::i18n::chain(l.code).len() == 2)
            .map(|l| l.code)
            .chain(["en-GB"])
        {
            let own = crate::i18n::own(l);
            let lacks: Vec<String> = OPTIONS
                .iter()
                .flat_map(|o| {
                    [
                        format!("setting.{}", o.name),
                        format!("setting.{}.hint", o.name),
                    ]
                })
                .chain(
                    Area::ALL
                        .iter()
                        .map(|a| format!("settings.area.{}", a.id())),
                )
                .chain(GROUPS.iter().filter_map(|g| g.head.map(String::from)))
                .chain(Level::ALL.iter().flat_map(|v| {
                    [
                        format!("settings.level.{}", v.id()),
                        format!("settings.level.{}.hint", v.id()),
                    ]
                }))
                .filter(|k| {
                    !own.get(k)
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.trim().is_empty())
                })
                .collect();
            assert!(lacks.is_empty(), "{l} lacks {lacks:?}");
        }
    }

    #[test]
    fn settings_values_follow_the_description() {
        let cfg = Config::default();
        let v = values(&cfg);
        assert_eq!(v.len(), OPTIONS.len());
        // Each one is found where the description says, unset optional ones as empty text.
        assert_eq!(v["search_text"], Value::Bool(cfg.search.text));
        assert_eq!(
            v["latex_image"],
            Value::String(cfg.preview.images["latex"].clone())
        );
        assert_eq!(v["tui_theme"], Value::String(cfg.theme.clone()));
        assert_eq!(v["editor"], Value::String(String::new()));
        assert!(
            v.iter().all(|(k, x)| !x.is_null() && find(k).is_some()),
            "{v:?}"
        );
        let mut names: Vec<&str> = OPTIONS.iter().map(|o| o.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), OPTIONS.len(), "an option is described twice");
    }

    #[test]
    fn settings_apply_keeps_comments_and_writes_every_kind() {
        let text = "# mine\nshow_hidden = false # keep\n\n[gui]\nfont_size = 13\n";
        let mut ch = serde_json::Map::new();
        ch.insert("show_hidden".into(), true.into());
        ch.insert("line_height".into(), serde_json::json!(1.5));
        ch.insert("text_roots".into(), serde_json::json!(["/a", "/b"]));
        ch.insert("editor".into(), "hx".into());
        let new = apply(text, &ch).unwrap();
        assert!(
            new.contains("# mine") && new.contains("show_hidden = true # keep"),
            "{new}"
        );
        let cfg = Config::parse(&new).unwrap();
        assert_eq!(
            (cfg.gui.line_height, cfg.editor.as_deref()),
            (1.5, Some("hx"))
        );
        assert_eq!(cfg.search.text_roots.len(), 2);
        // An empty editor takes the key out again: $EDITOR applies.
        let mut ch = serde_json::Map::new();
        ch.insert("editor".into(), "".into());
        let cfg = Config::parse(&apply(&new, &ch).unwrap()).unwrap();
        assert_eq!(cfg.editor, None);
        ch.insert("nonsense".into(), true.into());
        assert!(apply(&new, &ch).is_err());
    }

    #[test]
    fn settings_apply_keeps_other_keys_and_defaults() {
        let text = "# my config\ntheme = \"nc\"  # terminal\n\n[gui]\n# big text\nfont_size = 15\n\n[keys]\nquit = [\"F10\"]\n";
        let mut ch = serde_json::Map::new();
        ch.insert("font_size".into(), 17.into());
        ch.insert("language".into(), "da".into());
        ch.insert("latex_image".into(), "texlive:medium".into());
        ch.insert("show_hidden".into(), false.into());
        ch.insert("names_only".into(), serde_json::json!(["/home/me/Mail"]));
        ch.insert("search_archives".into(), false.into());
        ch.insert("cloud_read".into(), serde_json::json!(["/home/me/gdrive"]));
        let out = apply(text, &ch).unwrap();
        for kept in [
            "# my config",
            "theme = \"nc\"  # terminal",
            "# big text",
            "quit = [\"F10\"]",
        ] {
            assert!(out.contains(kept), "{kept} lost:\n{out}");
        }
        let cfg = Config::parse(&out).unwrap();
        assert_eq!(
            (cfg.gui.font_size, cfg.language.as_str(), cfg.show_hidden),
            (17.0, "da", false)
        );
        assert_eq!(cfg.preview.images["latex"], "texlive:medium");
        assert_eq!(
            cfg.search.names_only,
            [std::path::PathBuf::from("/home/me/Mail")]
        );
        assert_eq!(
            (cfg.search.cloud_read, cfg.search.cloud.as_str()),
            (
                vec![std::path::PathBuf::from("/home/me/gdrive")],
                "local-only"
            ),
            "online files stay there unless asked"
        );
        assert!(
            !cfg.search.archives && Config::default().search.archives,
            "on unless switched off"
        );
        assert_eq!(
            crate::images::short(&cfg.preview.images["plantuml"]), "docker.io/plantuml/plantuml:latest",
            "other defaults stay"
        );
        assert!(
            apply(
                text,
                &serde_json::Map::from_iter([("nope".into(), 1.into())])
            )
            .is_err()
        );
    }

    #[test]
    fn settings_never_write_over_a_config_that_cannot_be_read() {
        let dir = std::env::temp_dir().join(format!("coxswain-unreadable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // A comment in Latin-1: not UTF-8, so it cannot be read as text.
        let old = b"# caf\xe9\ntheme = \"nc\"\n";
        std::fs::write(&path, old).unwrap();
        let mut ch = serde_json::Map::new();
        ch.insert("show_hidden".into(), true.into());
        assert!(save_to(&path, &ch).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        // A missing one is written new.
        std::fs::remove_file(&path).unwrap();
        save_to(&path, &ch).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("show_hidden = true")
        );
        assert!(
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")),
            "nothing left beside it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_sizes_go_up_to_exabytes() {
        assert_eq!(human_latin(1 << 50), "1.0 PB");
        assert_eq!(human_latin(1 << 60), "1.0 EB");
    }

    #[test]
    fn settings_open_at_an_area_or_an_option() {
        assert_eq!(open_at(""), (Overview, None));
        assert_eq!(open_at("search"), (Search, None));
        assert_eq!(open_at("search_meaning"), (Search, Some("search_meaning")));
        assert_eq!(open_at("ask_model"), (Search, Some("ask_model")));
        assert_eq!(open_at("language"), (Looks, Some("language")));
        assert_eq!(open_at("previews"), (Previews, None));
        assert_eq!(open_at("check_updates"), (Privacy, Some("check_updates")));
        assert_eq!(open_at("disk"), (Privacy, Some("disk")));
        // Chess moved from Previews to Games: its old names open there.
        assert_eq!(open_at("games"), (Games, None));
        assert_eq!(open_at("chess_engines"), (Games, Some("chess_engines")));
        assert_eq!(open_at("chess_book"), (Games, Some("chess_book")));
        assert_eq!(open_at("chess_review_time"), (Games, Some("chess_review_time")));
        // The section names of 1.x are gone with 2.0.
        for old in ["meaning", "ask", "news", "cloud", "no-such"] {
            assert_eq!(open_at(old), (Overview, None), "{old}");
        }
        // Every section a notice opens is one of them.
        let known = sections();
        let status = Status {
            state: crate::index::State::Ready,
            len: 1,
            texts: 10,
            pending: 0,
            bytes: 0,
            paused: false,
            roots: vec![],
            tools: vec![("tesseract".into(), false)],
            meaning: true,
            meaning_pending: 0,
            meaning_done: 0,
            meaning_changing: 0,
            meaning_passages: 0,
            meaning_renewing: 0,
            meaning_ms_per_passage: 0,
            meaning_engine: String::new(),
            meaning_error: Some("down".into()),
            meaning_runs: None,
            chat_loaded: None,
            error: Some("stalled".into()),
            clouds: vec![("OneDrive".into(), "/c".into())],
            tika: None,
            transcripts: None,
            outdated: None,
            changes_left: 0,
            changes_freed: None,
        };
        let mut state = crate::state::AppState::default();
        state.migrated = vec!["[keys] mkdir → new_folder".into()];
        for n in crate::notices::all(&Config::default(), &status, &state, false) {
            if let Some(s) = n.settings {
                assert!(known.iter().any(|k| k.0 == s), "{}: {s}", n.id);
            }
        }
    }

    #[test]
    fn settings_every_option_is_in_one_group_of_its_area() {
        for o in OPTIONS {
            let gs: Vec<&Group> = GROUPS.iter().filter(|g| g.options.contains(&o.name)).collect();
            assert_eq!(gs.len(), 1, "{} is in {} groups", o.name, gs.len());
            assert_eq!(gs[0].area, o.area, "{}: its group {} is of another area", o.name, gs[0].id);
        }
        for g in GROUPS {
            assert!(g.options.iter().all(|n| find(n).is_some()), "{}: an unknown option", g.id);
            assert!(OPTIONS.iter().all(|o| o.name != g.id) && Area::ALL.iter().all(|a| a.id() != g.id), "{}: also an option's or an area's name", g.id);
        }
        let ids: std::collections::HashSet<_> = GROUPS.iter().map(|g| g.id).collect();
        assert_eq!(ids.len(), GROUPS.len(), "a group's name twice");
    }

    #[test]
    fn settings_every_option_is_found_by_its_config_key() {
        crate::i18n::set_language("en-GB");
        for o in OPTIONS {
            for q in [o.key(), o.path.join("."), o.name.to_string(), o.name.to_uppercase()] {
                assert!(search(&q).iter().any(|x| x.name == o.name), "{q} does not find {}", o.name);
            }
        }
        assert_eq!(find("search_text").unwrap().key(), "[search] text");
        assert_eq!(find("language").unwrap().key(), "language");
        assert!(search("").is_empty() && search("  ").is_empty());
        assert!(search("no such setting at all").is_empty());
        // Every word, in any order, case and accents aside.
        assert!(search("ENGINE time").iter().any(|o| o.name == "chess_engine_time"));
        assert!(search("bóok").iter().any(|o| o.name == "chess_book"));
    }

    #[test]
    fn settings_levels_turn_on_what_they_need() {
        let mut s = SearchConfig::default();
        s.text = true;
        s.meaning = false;
        assert_eq!(level(&s), Some(Level::Text));
        let ch = level_changes(Level::Names, &s, false).unwrap();
        assert_eq!(
            (ch["search_text"].as_bool(), ch["search_meaning"].as_bool()),
            (Some(false), Some(false))
        );
        // Meaning with the built-in model not downloaded, or Ask without a chat model: the guide.
        assert_eq!(level_changes(Level::Meaning, &s, false), None);
        assert!(level_changes(Level::Meaning, &s, true).is_some());
        assert_eq!(level_changes(Level::Ask, &s, true), None);
        s.meaning_engine = "ollama".into();
        s.ask_model = "qwen3:8b".into();
        let ch = level_changes(Level::Ask, &s, false).unwrap();
        assert_eq!(
            (ch["search_text"].as_bool(), ch["search_meaning"].as_bool()),
            (Some(true), Some(true))
        );
        s.meaning = true;
        assert_eq!(level(&s), Some(Level::Ask));
        // Down from Ask to meaning forgets the chat model; down to text keeps it.
        assert_eq!(
            level_changes(Level::Meaning, &s, false).unwrap()["ask_model"],
            ""
        );
        assert!(
            !level_changes(Level::Text, &s, false)
                .unwrap()
                .contains_key("ask_model")
        );
        s.text = false;
        assert_eq!(level(&s), None);
    }

    #[test]
    fn settings_status_says_the_next_step() {
        let mut s = SearchConfig::default();
        let st = Status {
            state: crate::index::State::Ready,
            len: 912_330,
            texts: 3875,
            pending: 438,
            bytes: 0,
            meaning: false,
            meaning_pending: 0,
            meaning_done: 0,
            meaning_changing: 0,
            meaning_passages: 0,
            meaning_renewing: 0,
            meaning_ms_per_passage: 0,
            meaning_engine: String::new(),
            meaning_error: None,
            meaning_runs: None,
            chat_loaded: None,
            error: None,
            paused: true,
            roots: vec![],
            tools: vec![],
            clouds: vec![],
            tika: None,
            transcripts: None,
            outdated: None,
            changes_left: 0,
            changes_freed: None,
        };
        s.text = true;
        s.meaning = false;
        let lines = status(&s, &st, true, &[]);
        assert_eq!(
            lines.iter().map(|l| l.step).collect::<Vec<_>>(),
            [
                None,
                Some(Step::ReadNow),
                Some(Step::SetUp),
                Some(Step::SetUp)
            ]
        );
        assert!(
            lines[0].text.contains("912\u{202f}330"),
            "{}",
            lines[0].text
        );
        assert_eq!(
            (count(7), count(1000), count(12_345_678)),
            (
                "7".into(),
                "1\u{202f}000".into(),
                "12\u{202f}345\u{202f}678".into()
            )
        );
        assert_eq!(status(&s, &st, false, &[])[1].step, Some(Step::Start));
        s.meaning = true;
        s.meaning_engine = "ollama".into();
        s.ask_model = "qwen3:8b".into();
        let st = Status {
            meaning_error: Some("http://localhost:11434: Connection refused".into()),
            ..st
        };
        let lines = status(&s, &st, true, &[]);
        assert!(
            lines[2].bad
                && lines[2]
                    .note
                    .as_deref()
                    .unwrap()
                    .contains("localhost:11434"),
            "{:?}",
            lines[2]
        );
        assert_eq!(lines[3].step, Some(Step::TryIt));
        let st = Status {
            meaning_error: None,
            meaning_done: 23_611,
            meaning_pending: 245_762,
            ..st
        };
        let lines = status(&s, &st, true, &[]);
        assert_eq!(lines[2].bar, Some((23_611, 269_373)));
        assert_eq!(bar(23_611, 269_373, 20), "█░░░░░░░░░░░░░░░░░░░");
        assert_eq!(
            (
                percent(23_611, 269_373),
                percent(269_372, 269_373),
                percent(5, 5),
                percent(0, 0)
            ),
            (8, 99, 100, 100)
        );
        // A file an app keeps saving: said apart, not in the files to go.
        let st = Status {
            meaning_pending: 0,
            meaning_changing: 1,
            ..st
        };
        let lines = status(&s, &st, true, &[]);
        assert_eq!(lines[2].bar, None);
        assert_eq!(
            lines[2].note.as_deref(),
            Some(crate::tn!("settings.status.meaning_changing", 1).as_str())
        );
        let st = Status { meaning: true, ..st };
        let footer = crate::find::footer(&st);
        assert!(
            footer.contains(&crate::tn!("find.meaning_changing", 1)),
            "{footer}"
        );
        // Transcribing or paused with files waiting: both steps, so the rest can be let go.
        s.whisper = true;
        let whisper = |state, waiting| {
            let ts = crate::transcribe::Status { waiting, state, ..Default::default() };
            let st = Status { transcripts: Some(ts), ..st.clone() };
            let l = status(&s, &st, true, &[]).into_iter().find(|l| l.part == Part::Whisper).unwrap();
            (l.step, l.also)
        };
        use crate::transcribe::State as T;
        assert_eq!(whisper(T::Working, 3), (Some(Step::Pause), Some(Step::StopTranscribing)));
        assert_eq!(whisper(T::Paused, 3), (Some(Step::Resume), Some(Step::StopTranscribing)));
        assert_eq!(whisper(T::Working, 0), (Some(Step::Pause), None));
        assert_eq!(whisper(T::Idle, 3), (Some(Step::StopTranscribing), None));
    }

    #[test]
    fn settings_say_what_leaves_and_where() {
        assert_eq!(
            host("http://user@my-server:13305/api/v1"),
            "my-server:13305"
        );
        assert!(
            is_local("http://localhost:11434")
                && is_local("http://127.0.0.1:8000/v1")
                && is_local("http://[::1]:1234")
        );
        assert!(!is_local("http://my-server:13305/api/v1"));
        assert_eq!(
            (
                registry("docker.io/pandoc/core"),
                registry("texlive/texlive"),
                registry("ghcr.io/x/y:1")
            ),
            ("docker.io", "docker.io", "ghcr.io")
        );
        let mut cfg = Config::default();
        cfg.check_updates = false;
        cfg.preview.container = "off".into();
        cfg.search.meaning = false;
        assert!(outbound(&cfg).is_empty());
        cfg.preview.container = "auto".into();
        cfg.preview.image_updates = "weekly".into();
        assert_eq!(
            outbound(&cfg),
            [Out {
                what: t!("settings.out.images_weekly"),
                // Typst's image is on ghcr.io.
                to: "docker.io, ghcr.io".into(),
                local: false
            }],
            "keeping images up to date names the registry"
        );
        cfg.preview.container = "off".into();
        cfg.check_updates = true;
        cfg.search.meaning = true;
        cfg.search.meaning_engine = "openai".into();
        cfg.search.meaning_url = "http://my-server:13305/api/v1".into();
        cfg.search.ask_model = "qwen3:8b".into();
        let out = outbound(&cfg);
        assert_eq!(
            out.iter()
                .map(|o| (o.to.as_str(), o.local))
                .collect::<Vec<_>>(),
            [
                ("api.github.com", false),
                ("my-server:13305", false),
                ("my-server:13305", false)
            ]
        );
    }
}

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
    Looks,
    Behaviour,
    /// Every action and its keys.
    Keys,
    /// The update check, what can leave the machine, where things are kept.
    Privacy,
}

impl Area {
    pub const ALL: [Area; 7] = [Area::Overview, Area::Search, Area::Previews, Area::Looks, Area::Behaviour, Area::Keys, Area::Privacy];

    /// Its name in `--settings=<name>` and in the `settings.area.<name>` text.
    pub fn id(self) -> &'static str {
        match self {
            Area::Overview => "overview",
            Area::Search => "search",
            Area::Previews => "previews",
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

    /// The `[search]` options the helper reads when it starts: a change restarts it. Ask's are
    /// read by the app itself.
    pub fn restarts_helper(&self) -> bool {
        self.path[0] == "search" && !self.name.starts_with("ask_")
    }

    /// Optional in config.toml: empty means the key is taken out (the default applies).
    fn optional(&self) -> bool {
        matches!(self.name, "editor" | "viewer")
    }
}

const fn opt(name: &'static str, path: &'static [&'static str], area: Area, costs: &'static [Cost]) -> Opt {
    Opt { name, path, area, costs }
}

use Area::*;
use Cost::*;

/// Every option, by area.
pub const OPTIONS: &[Opt] = &[
    opt("search_text", &["search", "text"], Search, &[Disk, Cpu]),
    opt("search_meaning", &["search", "meaning"], Search, &[Disk, Cpu, Network]),
    opt("search_archives", &["search", "archives"], Search, &[Cpu]),
    opt("search_archives_everywhere", &["search", "archives_everywhere"], Search, &[Disk, Cpu]),
    opt("search_history", &["search", "history"], Search, &[Disk]),
    opt("search_cloud", &["search", "cloud"], Search, &[Network]),
    opt("cloud_read", &["search", "cloud_read"], Search, &[Network]),
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
    opt("meaning_key_env", &["search", "meaning_key_env"], Search, &[]),
    opt("meaning_device", &["search", "meaning_device"], Search, &[]),
    opt("ask_model", &["search", "ask_model"], Search, &[Leaves]),
    opt("ask_think", &["search", "ask_think"], Search, &[]),
    opt("preview_prefer", &["preview", "prefer"], Previews, &[]),
    opt("preview_container", &["preview", "container"], Previews, &[]),
    opt("latex_image", &["preview", "images", "latex"], Previews, &[Disk, Network]),
    opt("latex_auto", &["preview", "latex_auto"], Previews, &[Cpu]),
    opt("preview_timeout", &["preview", "timeout"], Previews, &[]),
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
    opt("folder_sizes", &["folder_sizes"], Behaviour, &[Cpu]),
    opt("git_last_commit", &["git", "last_commit"], Behaviour, &[Cpu]),
    opt("editor", &["editor"], Behaviour, &[]),
    opt("viewer", &["viewer"], Behaviour, &[]),
    opt("bom_viewer", &["bom_viewer"], Behaviour, &[]),
    opt("provenance_viewer", &["provenance_viewer"], Behaviour, &[]),
    opt("check_updates", &["check_updates"], Privacy, &[Network]),
];

/// The option called `name`.
pub fn find(name: &str) -> Option<&'static Opt> {
    OPTIONS.iter().find(|o| o.name == name)
}

/// The value of every option, by name, as config.toml holds it now (an unset optional one as
/// an empty text).
pub fn values(cfg: &Config) -> serde_json::Map<String, Value> {
    let all = serde_json::to_value(cfg).unwrap_or_default();
    OPTIONS
        .iter()
        .map(|o| {
            let v = o.path.iter().try_fold(&all, |v, k| v.get(k)).cloned().unwrap_or(Value::Null);
            (o.name.to_string(), if v.is_null() { Value::String(String::new()) } else { v })
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
                None => n.as_f64().ok_or_else(|| t!("err.unsupported_value", "name" => name))?.into(),
            },
            Value::String(s) => s.as_str().into(),
            Value::Array(a) => {
                let texts: Option<Vec<&str>> = a.iter().map(|v| v.as_str()).collect();
                texts.ok_or_else(|| t!("err.unsupported_value", "name" => name))?.into_iter().collect::<toml_edit::Array>().into()
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
    save_to(&Config::path().ok_or_else(|| t!("err.no_config_folder"))?, changes)
}

/// `save`, to the config.toml at `path`.
pub fn save_to(path: &std::path::Path, changes: &serde_json::Map<String, Value>) -> Result<Config, String> {
    // One save at a time: two read-change-write rounds at once would lose one's change.
    static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    // Through a link (a config kept with dotfiles) to the file itself.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new_text = apply(&text, changes)?;
    let cfg = Config::parse(&new_text)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, new_text).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::i18n::set_language(crate::i18n::resolve(&cfg.language));
    Ok(cfg)
}

/// Where Settings opens for `--settings=<name>` and a notice's *Show me*: the area, and the
/// option to show in it. An area's name or an option's; anything else, the Overview.
pub fn open_at(section: &str) -> (Area, Option<&'static str>) {
    if let Some(a) = Area::ALL.into_iter().find(|a| a.id() == section) {
        return (a, None);
    }
    match find(section) {
        Some(o) => (o.area, Some(o.name)),
        None => (Overview, None),
    }
}

/// Every name `open_at` knows, with where it opens: for the desktop app, which opens Settings
/// at sections given by notices too.
pub fn sections() -> Vec<(&'static str, Area, Option<&'static str>)> {
    let names = Area::ALL.iter().map(|a| a.id()).chain(OPTIONS.iter().map(|o| o.name));
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
pub fn level_changes(level: Level, s: &SearchConfig, builtin_ready: bool) -> Option<serde_json::Map<String, Value>> {
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
}

impl Step {
    pub fn label(self) -> String {
        t!(match self {
            Step::TurnOn => "settings.step.turn_on",
            Step::Start => "settings.step.start",
            Step::ReadNow => "settings.step.read_now",
            Step::SetUp => "settings.step.set_up",
            Step::TryIt => "settings.step.try_it",
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
    for u in ["KB", "MB", "GB", "TB"] {
        v /= 1024.0;
        if v < 1024.0 {
            return if v < 100.0 { format!("{v:.1} {}", crate::i18n::tr(&format!("unit.{u}"), &[])) } else { format!("{v:.0} {}", crate::i18n::tr(&format!("unit.{u}"), &[])) };
        }
    }
    format!("{v:.0} {}", t!("unit.PB"))
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
    let name = if h.starts_with('[') { h.split(']').next().map(|n| &n[1..]).unwrap_or(h) } else { h.split(':').next().unwrap_or(h) };
    matches!(name, "localhost" | "::1") || name.starts_with("127.")
}

/// The server that makes the vectors (or Ask's, with the built-in model: Ollama here).
pub fn server_url(s: &SearchConfig) -> String {
    if s.meaning_url.is_empty() && s.meaning_engine != "openai" { crate::meaning::OLLAMA.to_string() } else { s.meaning_url.clone() }
}

/// A server error in words with the step it calls for, not as it came: a server that does not
/// answer is said to be so.
fn cause(why: &str, url: &str) -> String {
    let w = why.to_lowercase();
    if ["connection refused", "connect", "dns", "timed out", "unreachable"].iter().any(|k| w.contains(k)) {
        t!("settings.status.down", "host" => host(url))
    } else {
        t!("settings.status.failed", "why" => why)
    }
}

/// The status block of Finding files, from the settings and the helper's status. `helper`:
/// the helper answers (without it there is no reading).
pub fn status(s: &SearchConfig, st: &Status, helper: bool) -> Vec<Line> {
    let line = |part, key: &str, text: String| Line { part, label: t!(key), text, note: None, bad: false, step: None };
    let mut names = line(Part::Names, "settings.status.names", t!("settings.status.names_ready", "n" => count(st.len)));
    if st.state != crate::index::State::Ready {
        names.text = t!("settings.status.names_building", "n" => count(st.len));
    }
    let mut text = line(Part::Text, "settings.status.text", t!("settings.status.off"));
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
    }
    let url = server_url(s);
    let mut meaning = line(Part::Meaning, "settings.status.meaning", t!("settings.status.off"));
    if !s.meaning {
        meaning.note = Some(t!("settings.status.meaning_off"));
        meaning.step = Some(Step::SetUp);
    } else {
        let by = if s.meaning_engine == "builtin" {
            t!("settings.status.builtin", "where" => st.meaning_runs.as_ref().map(|r| r.text()).unwrap_or_else(|| t!("meaning.on_cpu")))
        } else {
            let model = if s.meaning_model.is_empty() { "bge-m3" } else { &s.meaning_model };
            t!("settings.status.server", "model" => model, "host" => host(&url))
        };
        let total = st.meaning_done + st.meaning_pending;
        meaning.text = t!("settings.status.meaning_ready", "done" => count(st.meaning_done), "total" => count(total), "by" => by);
        if let Some(why) = &st.meaning_error {
            meaning.note = Some(cause(why, &url));
            meaning.bad = true;
            meaning.step = Some(Step::SetUp);
        } else if st.meaning_pending > 0 && st.meaning_ms_per_file > 0 {
            meaning.note = Some(t!("settings.status.meaning_left", "time" => crate::notices::time_left(st)));
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
        }
    }
    vec![names, text, meaning, ask]
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
    let out = |key: &str, to: &str, local| Out { what: t!(key), to: to.to_string(), local };
    let s = &cfg.search;
    let mut v = vec![];
    if cfg.check_updates {
        v.push(out("settings.out.update", "api.github.com", false));
    }
    let url = server_url(s);
    if s.meaning && s.meaning_engine == "builtin" && !crate::meaning::installed() {
        v.push(out("settings.out.model", "huggingface.co", false));
    }
    if s.meaning && s.meaning_engine != "builtin" {
        v.push(out("settings.out.meaning", host(&url), is_local(&url)));
    }
    match crate::chat::of(&s.ask_model) {
        Some(m) if !m.installed() => v.push(out("settings.out.chat_model", "huggingface.co", false)),
        Some(_) => {}
        None if crate::find::ask_ready(s).is_ok() => v.push(out("settings.out.ask", host(&url), is_local(&url))),
        None => {}
    }
    if s.cloud == "all" || !s.cloud_read.is_empty() {
        v.push(out("settings.out.cloud", &t!("settings.out.cloud_where"), false));
    }
    if cfg.preview.prefer != "local" && cfg.preview.container != "off" {
        let mut hosts: Vec<&str> = cfg.preview.images.values().filter(|i| !i.is_empty()).map(|i| registry(i)).collect();
        hosts.sort();
        hosts.dedup();
        if !hosts.is_empty() {
            v.push(out("settings.out.images", &hosts.join(", "), false));
        }
    }
    v
}

/// The registry a container image comes from: `docker.io` unless its name starts with one.
fn registry(image: &str) -> &str {
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
        // Texts of their own: every language but the English and German variants, which take
        // the rest from British English and German.
        for l in crate::i18n::LANGUAGES.iter().filter(|l| !l.code.starts_with("en-") && !l.code.starts_with("de-")).map(|l| l.code).chain(["en-GB"]) {
            let own = crate::i18n::own(l);
            let lacks: Vec<String> = OPTIONS
                .iter()
                .flat_map(|o| [format!("setting.{}", o.name), format!("setting.{}.hint", o.name)])
                .chain(Area::ALL.iter().map(|a| format!("settings.area.{}", a.id())))
                .chain(Level::ALL.iter().flat_map(|v| [format!("settings.level.{}", v.id()), format!("settings.level.{}.hint", v.id())]))
                .filter(|k| !own.get(k).and_then(|v| v.as_str()).is_some_and(|s| !s.trim().is_empty()))
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
        assert_eq!(v["latex_image"], Value::String(cfg.preview.images["latex"].clone()));
        assert_eq!(v["tui_theme"], Value::String(cfg.theme.clone()));
        assert_eq!(v["editor"], Value::String(String::new()));
        assert!(v.iter().all(|(k, x)| !x.is_null() && find(k).is_some()), "{v:?}");
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
        assert!(new.contains("# mine") && new.contains("show_hidden = true # keep"), "{new}");
        let cfg = Config::parse(&new).unwrap();
        assert_eq!((cfg.gui.line_height, cfg.editor.as_deref()), (1.5, Some("hx")));
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
        for kept in ["# my config", "theme = \"nc\"  # terminal", "# big text", "quit = [\"F10\"]"] {
            assert!(out.contains(kept), "{kept} lost:\n{out}");
        }
        let cfg = Config::parse(&out).unwrap();
        assert_eq!((cfg.gui.font_size, cfg.language.as_str(), cfg.show_hidden), (17.0, "da", false));
        assert_eq!(cfg.preview.images["latex"], "texlive:medium");
        assert_eq!(cfg.search.names_only, [std::path::PathBuf::from("/home/me/Mail")]);
        assert_eq!((cfg.search.cloud_read, cfg.search.cloud.as_str()), (vec![std::path::PathBuf::from("/home/me/gdrive")], "local-only"), "online files stay there unless asked");
        assert!(!cfg.search.archives && Config::default().search.archives, "on unless switched off");
        assert_eq!(cfg.preview.images["plantuml"], "docker.io/plantuml/plantuml:latest", "other defaults stay");
        assert!(apply(text, &serde_json::Map::from_iter([("nope".into(), 1.into())])).is_err());
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
        // The section names of 1.x are gone with 2.0.
        for old in ["meaning", "ask", "news", "cloud", "no-such"] {
            assert_eq!(open_at(old), (Overview, None), "{old}");
        }
        // Every section a notice opens is one of them.
        let known = sections();
        let status = Status { state: crate::index::State::Ready, len: 1, texts: 10, pending: 0, bytes: 0, paused: false, roots: vec![], tools: vec![("tesseract".into(), false)], meaning: true, meaning_pending: 0, meaning_done: 0, meaning_passages: 0, meaning_renewing: 0, meaning_ms_per_file: 0, meaning_engine: String::new(), meaning_error: Some("down".into()), meaning_runs: None, error: Some("stalled".into()), clouds: vec![("OneDrive".into(), "/c".into())] };
        let mut state = crate::state::AppState::default();
        state.migrated = vec!["[keys] mkdir → new_folder".into()];
        for n in crate::notices::all(&Config::default(), &status, &state, false) {
            if let Some(s) = n.settings {
                assert!(known.iter().any(|k| k.0 == s), "{}: {s}", n.id);
            }
        }
    }

    #[test]
    fn settings_levels_turn_on_what_they_need() {
        let mut s = SearchConfig::default();
        s.text = true;
        s.meaning = false;
        assert_eq!(level(&s), Some(Level::Text));
        let ch = level_changes(Level::Names, &s, false).unwrap();
        assert_eq!((ch["search_text"].as_bool(), ch["search_meaning"].as_bool()), (Some(false), Some(false)));
        // Meaning with the built-in model not downloaded, or Ask without a chat model: the guide.
        assert_eq!(level_changes(Level::Meaning, &s, false), None);
        assert!(level_changes(Level::Meaning, &s, true).is_some());
        assert_eq!(level_changes(Level::Ask, &s, true), None);
        s.meaning_engine = "ollama".into();
        s.ask_model = "qwen3:8b".into();
        let ch = level_changes(Level::Ask, &s, false).unwrap();
        assert_eq!((ch["search_text"].as_bool(), ch["search_meaning"].as_bool()), (Some(true), Some(true)));
        s.meaning = true;
        assert_eq!(level(&s), Some(Level::Ask));
        // Down from Ask to meaning forgets the chat model; down to text keeps it.
        assert_eq!(level_changes(Level::Meaning, &s, false).unwrap()["ask_model"], "");
        assert!(!level_changes(Level::Text, &s, false).unwrap().contains_key("ask_model"));
        s.text = false;
        assert_eq!(level(&s), None);
    }

    #[test]
    fn settings_status_says_the_next_step() {
        let mut s = SearchConfig::default();
        let st = Status { state: crate::index::State::Ready, len: 912_330, texts: 3875, pending: 438, bytes: 0, meaning: false, meaning_pending: 0, meaning_done: 0, meaning_passages: 0, meaning_renewing: 0, meaning_ms_per_file: 0, meaning_engine: String::new(), meaning_error: None, meaning_runs: None, error: None, paused: true, roots: vec![], tools: vec![], clouds: vec![] };
        s.text = true;
        s.meaning = false;
        let lines = status(&s, &st, true);
        assert_eq!(lines.iter().map(|l| l.step).collect::<Vec<_>>(), [None, Some(Step::ReadNow), Some(Step::SetUp), Some(Step::SetUp)]);
        assert!(lines[0].text.contains("912\u{202f}330"), "{}", lines[0].text);
        assert_eq!((count(7), count(1000), count(12_345_678)), ("7".into(), "1\u{202f}000".into(), "12\u{202f}345\u{202f}678".into()));
        assert_eq!(status(&s, &st, false)[1].step, Some(Step::Start));
        s.meaning = true;
        s.meaning_engine = "ollama".into();
        s.ask_model = "qwen3:8b".into();
        let st = Status { meaning_error: Some("http://localhost:11434: Connection refused".into()), ..st };
        let lines = status(&s, &st, true);
        assert!(lines[2].bad && lines[2].note.as_deref().unwrap().contains("localhost:11434"), "{:?}", lines[2]);
        assert_eq!(lines[3].step, Some(Step::TryIt));
    }

    #[test]
    fn settings_say_what_leaves_and_where() {
        assert_eq!(host("http://user@my-server:13305/api/v1"), "my-server:13305");
        assert!(is_local("http://localhost:11434") && is_local("http://127.0.0.1:8000/v1") && is_local("http://[::1]:1234"));
        assert!(!is_local("http://my-server:13305/api/v1"));
        assert_eq!((registry("docker.io/pandoc/core"), registry("texlive/texlive"), registry("ghcr.io/x/y:1")), ("docker.io", "docker.io", "ghcr.io"));
        let mut cfg = Config::default();
        cfg.check_updates = false;
        cfg.preview.container = "off".into();
        cfg.search.meaning = false;
        assert!(outbound(&cfg).is_empty());
        cfg.check_updates = true;
        cfg.search.meaning = true;
        cfg.search.meaning_engine = "openai".into();
        cfg.search.meaning_url = "http://my-server:13305/api/v1".into();
        cfg.search.ask_model = "qwen3:8b".into();
        let out = outbound(&cfg);
        assert_eq!(out.iter().map(|o| (o.to.as_str(), o.local)).collect::<Vec<_>>(), [("api.github.com", false), ("my-server:13305", false), ("my-server:13305", false)]);
    }
}

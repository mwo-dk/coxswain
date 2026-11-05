//! Modules: the viewers grouped by what they show, each of which the user can turn off
//! (`[modules] off` in config.toml, Settings → Modules in both apps). A module names its kinds
//! (`viewers::rules`), the libraries and programs it uses, the container images it may
//! download, and its feature (`features::FEATURES`: its title and docs page). All are on by
//! default. A file whose module is off is shown as text or hex, as any file is, and the first
//! time that happens for a module the app says which one would show it (`notice`).
//!
//! What stays whatever is off: folders, archives, text and code, logs, data files (JSON, YAML,
//! TOML) and Markdown. Search reads what it reads either way; modules are about what the apps
//! show.

use serde::Serialize;
use std::sync::RwLock;

/// One module.
#[derive(Debug, Serialize)]
pub struct Module {
    /// Its name in `[modules] off`: `games`.
    pub id: &'static str,
    /// Its entry in F1 → Features (`feature.<id>.title` names the module; its page is the docs).
    pub feature: &'static str,
    /// The kinds of `viewers::rules` it shows.
    pub kinds: &'static [&'static str],
    /// The libraries in Coxswain it uses to read or draw them.
    pub libraries: &'static [&'static str],
    /// The programs it runs when they are installed.
    pub tools: &'static [&'static str],
    /// The `[preview.images]` it may download from their registry, when a program is not
    /// installed and containers are on. Nothing of the user's is sent anywhere by any module.
    pub images: &'static [&'static str],
    /// Names it shows that its rules know by a pattern Settings cannot list: `cdx.json`.
    pub also: &'static [&'static str],
}

const fn m(
    id: &'static str,
    feature: &'static str,
    kinds: &'static [&'static str],
    libraries: &'static [&'static str],
    tools: &'static [&'static str],
    images: &'static [&'static str],
) -> Module {
    Module { id, feature, kinds, libraries, tools, images, also: &[] }
}

const fn also(m: Module, also: &'static [&'static str]) -> Module {
    Module { also, ..m }
}

/// Every module, in the order Settings shows them.
pub const MODULES: &[Module] = &[
    m("documents", "preview_documents", &["pdf", "docx", "epub", "notebook", "xps", "rst", "asciidoc", "typst", "postscript", "mail", "calendar", "contacts"], &["mammoth", "Asciidoctor.js", "marked", "KaTeX", "mail-parser", "cfb"], &["pandoc", "typst", "gs", "asciidoctor-pdf"], &["pandoc", "typst", "ghostscript"]),
    m("office", "office", &["office"], &["pptx-to-html"], &["soffice"], &["libreoffice"]),
    m("ebooks", "ebooks", &["ebook", "comic"], &["jszip"], &["ebook-convert"], &["calibre"]),
    m("latex", "latex", &["latex"], &[], &["latexmk", "tectonic", "pdflatex"], &["latex"]),
    m("html", "html", &["html"], &["DOMPurify"], &[], &[]),
    m("diagrams", "preview_diagrams", &["graphviz", "mermaid", "plantuml", "d2", "ditaa", "svgbob", "pikchr", "nomnoml", "wavedrom", "vega", "vegalite", "dbml", "erd", "bytefield", "drawio"], &["Viz.js", "Mermaid"], &["dot", "plantuml", "d2", "drawio"], &["kroki", "plantuml", "drawio"]),
    m("openscad", "openscad", &["scad"], &[], &["openscad"], &["openscad"]),
    m("kicad", "kicad", &["kicad"], &[], &["kicad-cli"], &["kicad"]),
    m("data", "preview_data", &["sheet", "sqlite", "parquet", "duckdb", "plist", "cert"], &["SheetJS", "rusqlite", "parquet", "hyparquet", "plist", "x509-parser"], &["duckdb"], &["duckdb"]),
    m("media", "preview_media", &["image", "video", "audio", "video_other", "audio_other", "heif", "magick", "raw", "font"], &["lofty", "kamadak-exif"], &["ffmpeg", "magick"], &["ffmpeg", "imagemagick"]),
    m("old_formats", "old_formats", &["picture", "tracker"], &["image", "jxl-oxide"], &[], &[]),
    m("geodata", "geodata", &["geo", "gpx"], &[], &["gdalinfo"], &["gdal"]),
    m("neural_networks", "neural_networks", &["netron", "model"], &["Netron"], &[], &[]),
    also(m("games", "chess", &["chess", "go"], &["cozy-chess"], &["stockfish"], &[]), &["si4", "si5", "pgn.gz", "bin"]),
    also(m("bom", "cbom", &["bom"], &[], &[], &[]), &["cdx.json", "cdx.xml", "cbom.json", "bom.json", "bom.xml"]),
    also(m("provenance", "provenance", &["provenance"], &["x509-parser"], &[], &[]), &["intoto.jsonl", "sigstore.json", "dsse.json", "provenance.json", "build.slsa"]),
    m("disk_images", "disk_images", &["disk_image"], &[], &["7z"], &[]),
];

/// The module that shows `kind`; `None` for what is always shown (text, folders, archives …).
pub fn of(kind: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.kinds.contains(&kind))
}

/// The module called `id`.
pub fn find(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id == id)
}

static OFF: RwLock<Vec<String>> = RwLock::new(Vec::new());

#[cfg(test)]
thread_local! {
    /// A test's own list: the global one is set by other tests' configs meanwhile.
    pub(crate) static FORCE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Use `[modules] off` from now on (the apps call it with each config they load or save).
pub fn set_off(off: &[String]) {
    *OFF.write().unwrap_or_else(|e| e.into_inner()) = off.to_vec();
}

/// Whether the module called `id` is on.
pub fn on(id: &str) -> bool {
    #[cfg(test)]
    if let Some(off) = FORCE.with(|f| f.borrow().clone()) {
        return !off.iter().any(|o| o == id);
    }
    !OFF.read().unwrap_or_else(|e| e.into_inner()).iter().any(|o| o == id)
}

/// The module that would show `kind`, when it is off: the file is shown as text or hex then.
pub fn off_for(kind: &str) -> Option<&'static Module> {
    of(kind).filter(|m| !on(m.id))
}

/// `kind`, or `text` when its module is off.
pub fn gate(kind: &'static str) -> &'static str {
    if off_for(kind).is_some() { "text" } else { kind }
}

impl Module {
    /// Its name: its feature's title in the user's language.
    pub fn label(&self) -> String {
        crate::i18n::tr(&format!("feature.{}.title", self.feature), &[])
    }

    /// Its page under `docs/`.
    pub fn docs(&self) -> &'static str {
        crate::features::FEATURES.iter().find(|f| f.id == self.feature).map_or("", |f| f.docs)
    }

    /// The file endings it shows, from the rules that name them by ending (a few more are known
    /// by a pattern: `.pgn.gz`, `.drawio.svg`).
    pub fn endings(&self) -> Vec<&'static str> {
        use crate::viewers::Match;
        let mut v: Vec<&'static str> = vec![];
        for r in crate::viewers::rules().iter().filter(|r| self.kinds.contains(&r.kind)) {
            match &r.when {
                Match::Ending { endings } => v.extend(endings.iter().filter(|e| !e.is_empty())),
                Match::Suffix { suffixes } => v.extend(suffixes.iter().map(|s| s.trim_start_matches('.'))),
                Match::Name { re } => v.extend(pattern_endings(re)),
                Match::Dir { .. } => {}
            }
        }
        v.extend(self.also);
        let mut seen = std::collections::HashSet::new();
        v.retain(|e| seen.insert(*e));
        v
    }

    /// One line for Settings: what it shows, then what it uses.
    pub fn about(&self) -> String {
        let mut parts = vec![self.endings().iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")];
        let uses: Vec<&str> = self.libraries.iter().chain(self.tools).copied().collect();
        if !uses.is_empty() {
            parts.push(crate::t!("modules.uses", "what" => uses.join(", ")));
        }
        if !self.images.is_empty() {
            parts.push(crate::t!("modules.images"));
        }
        parts.join(" · ")
    }
}

/// The plain endings in a rule's pattern of the form `\.(a|b|c)$`: what Settings can list.
fn pattern_endings(re: &'static str) -> Vec<&'static str> {
    let word = |e: &'static str| {
        let n = e.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(e.len());
        // `pgn(\.gz|\.zst)?` gives `pgn`; `si[45]` nothing.
        (n > 0 && !e[n..].starts_with('[')).then(|| &e[..n])
    };
    if let Some(one) = re.strip_prefix(r"\.").and_then(|r| r.strip_suffix('$')).filter(|r| !r.starts_with('(')) {
        return word(one).into_iter().filter(|w| w.len() == one.len()).collect();
    }
    let Some(inner) = re.strip_prefix(r"\.(").and_then(|r| r.split_once(")$")).map(|(a, _)| a) else { return vec![] };
    inner.split('|').filter_map(word).collect()
}

/// What Settings → Modules lists: each module with its name, line and whether it is on.
#[derive(Debug, Serialize)]
pub struct Row {
    pub id: &'static str,
    /// Its F1 → Features entry (*Show me* opens its page).
    pub feature: &'static str,
    pub label: String,
    pub about: String,
    pub docs: &'static str,
    pub on: bool,
    /// It may download a container image (a Network badge).
    pub downloads: bool,
}

pub fn rows(off: &[String]) -> Vec<Row> {
    MODULES
        .iter()
        .map(|m| Row { id: m.id, feature: m.feature, label: m.label(), about: m.about(), docs: m.docs(), on: !off.iter().any(|o| o == m.id), downloads: !m.images.is_empty() })
        .collect()
}

/// `off` with the module `id` turned on or off, in the order of `MODULES`.
pub fn toggled(off: &[String], id: &str, on: bool) -> Vec<String> {
    MODULES
        .iter()
        .filter(|m| if m.id == id { !on } else { off.iter().any(|o| o == m.id) })
        .map(|m| m.id.to_string())
        .collect()
}

/// The first time a file of a module that is off is shown as text: which module would show
/// it, and where to turn it on; `None` after that (remembered with the dismissed notices).
pub fn notice(state: &mut crate::state::AppState, m: &Module, settings_key: &str) -> Option<String> {
    let id = format!("module-off:{}", m.id);
    if state.notices_dismissed.contains(&id) {
        return None;
    }
    state.notices_dismissed.push(id);
    Some(off_text(m, settings_key))
}

/// "Shown as text: the Chess games module is off (Settings, F9 → Modules)."
pub fn off_text(m: &Module, settings_key: &str) -> String {
    crate::t!("modules.off_notice", "module" => m.label(), "key" => settings_key)
}

/// `notice`, with state.json read and written: for the apps.
pub fn notice_once(m: &Module, settings_key: &str) -> Option<String> {
    let mut st = crate::state::AppState::load();
    let text = notice(&mut st, m, settings_key)?;
    let _ = st.save();
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind the table names is in one module at most, every module's kinds are kinds the
    /// table names, and only the base kinds are in none.
    #[test]
    fn every_viewer_belongs_to_one_module() {
        let kinds: std::collections::BTreeSet<&str> = crate::viewers::rules().iter().map(|r| r.kind).collect();
        let base = ["folder", "archive", "text", "markdown", "data", "log", "jsonl"];
        for k in &kinds {
            let n = MODULES.iter().filter(|m| m.kinds.contains(k)).count();
            if base.contains(k) {
                assert_eq!(n, 0, "{k} is always shown");
            } else {
                assert_eq!(n, 1, "{k} belongs to one module");
            }
        }
        for m in MODULES {
            for k in m.kinds {
                assert!(kinds.contains(k), "{}: {k} is no kind of the table", m.id);
            }
            assert!(!m.docs().is_empty(), "{}: its feature {} is in F1 → Features", m.id, m.feature);
            assert!(!m.endings().is_empty(), "{} lists its endings", m.id);
            assert!(m.id.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", m.id);
        }
    }

    #[test]
    fn a_kind_whose_module_is_off_is_text() {
        FORCE.with(|f| *f.borrow_mut() = Some(vec!["games".into()]));
        assert_eq!(gate("chess"), "text");
        assert_eq!(gate("pdf"), "pdf");
        assert_eq!(gate("text"), "text");
        assert_eq!(off_for("go").map(|m| m.id), Some("games"));
        FORCE.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn toggling_keeps_the_order_and_drops_unknown_names() {
        let off = vec!["games".to_string(), "nonsense".to_string()];
        assert_eq!(toggled(&off, "documents", false), ["documents", "games"]);
        assert_eq!(toggled(&off, "games", true), Vec::<String>::new());
    }

    #[test]
    fn the_notice_comes_once_per_module() {
        let mut st = crate::state::AppState::default();
        let games = find("games").unwrap();
        assert!(notice(&mut st, games, "F9").is_some());
        assert!(notice(&mut st, games, "F9").is_none());
        assert!(notice(&mut st, find("media").unwrap(), "F9").is_some());
    }

    #[test]
    fn pattern_endings_are_read_from_simple_patterns_only() {
        assert_eq!(pattern_endings(r"\.(onnx|tflite)$"), ["onnx", "tflite"]);
        assert!(pattern_endings(r"\.log(\.[0-9]+)?$").is_empty());
        assert_eq!(pattern_endings(r"\.(pgn(\.gz|\.zst)?|si[45]|cbh)$"), ["pgn", "cbh"]);
        assert_eq!(pattern_endings(r"\.sgf$"), ["sgf"]);
        assert!(pattern_endings(r"(\.cdx\.(json|xml)|^bom\.json)$").is_empty());
    }
}

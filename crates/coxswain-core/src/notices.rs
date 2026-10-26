//! Notices: what Coxswain can do that this user has not turned on yet, and what a new version
//! brought. The terminal app shows one at a time in its status line; the desktop app lists them
//! under *Settings → What's new*, with a count on its Settings button. One that was dismissed,
//! or acted on, is not shown again.

use serde::Serialize;

use crate::config::Config;
use crate::helper::Status;
use crate::state::AppState;
use crate::t;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Notice {
    /// Remembered once dismissed.
    pub id: String,
    pub text: String,
    /// The Settings section that does it (desktop app).
    pub settings: Option<&'static str>,
    /// A line to copy and run (an install command): shown with a Copy button.
    pub copy: Option<String>,
    /// The line of config.toml it is about: shown with a button that opens the file there.
    pub config_line: Option<usize>,
}

impl Notice {
    fn new(id: impl Into<String>, text: String, settings: Option<&'static str>) -> Notice {
        Notice {
            id: id.into(),
            text,
            settings,
            copy: None,
            config_line: None,
        }
    }
}

/// One notice for each setting config.toml gives that could not be used (`Config::refused`):
/// its line and what is wrong, while it stays wrong. The terminal app shows them in a dialog
/// at start, the desktop app first in its status line.
pub fn refused(cfg: &Config) -> Vec<Notice> {
    cfg.refused
        .iter()
        .map(|r| Notice {
            config_line: Some(r.line),
            ..Notice::new(
                format!("config:{}", r.problem),
                t!("notice.config_refused", "line" => r.line, "problem" => r.problem),
                None,
            )
        })
        .collect()
}

/// About how long the files still to get their vectors take here: the files to go, times
/// the passages a file has had so far (over every file done, not the last few: the newest
/// go first and are seldom typical), times what a passage takes as the helper measured it.
/// "40 minutes", "3 hours".
pub fn time_left(status: &Status) -> String {
    let per_file = if status.meaning_done > 0 {
        status.meaning_passages as f64 / status.meaning_done as f64
    } else {
        1.0
    };
    crate::meaning::about(
        status.meaning_pending as f64 * per_file * status.meaning_ms_per_passage as f64 / 1000.0,
    )
}

/// The notice to show now, if any. `terminal`: the notice names the command instead of Settings.
pub fn next(cfg: &Config, status: &Status, state: &AppState, terminal: bool) -> Option<Notice> {
    all(cfg, status, state, terminal).into_iter().next()
}

/// Every notice not dismissed yet, the most pressing first.
pub fn all(cfg: &Config, status: &Status, state: &AppState, terminal: bool) -> Vec<Notice> {
    let version = crate::update::VERSION;
    let seen = |id: &str| state.notices_dismissed.iter().any(|d| d == id);
    // A mistake in config.toml, before anything else: the terminal app has said it at start.
    let mut all = if terminal { vec![] } else { refused(cfg) };
    // Version 2.0 renamed keys in config.toml: which, once.
    if !state.migrated.is_empty() {
        all.push(Notice::new(
            "migrated-2",
            t!("notice.migrated", "changes" => state.migrated.join(", ")),
            Some("keys"),
        ));
    }
    // Actions in [keys] this version does not know (a newer one's, or a typo): left out, once.
    if !cfg.unknown_actions.is_empty() {
        let names = cfg.unknown_actions.join(", ");
        all.push(Notice::new(format!("unknown-actions:{names}"), t!("notice.unknown_actions", "names" => names), Some("keys")));
    }
    // A folder sync that ended left what it replaced or deleted: in the bin now, or still in
    // its folder where the bin could not take it (Disk use removes it). Each once.
    for l in crate::compare::aside::left() {
        let undo = cfg.key_for(crate::config::Action::Undo).unwrap_or("Ctrl+Z");
        let (n, size) = (crate::settings::count(l.files as usize), crate::settings::human(l.bytes));
        let text = if l.binned {
            t!("notice.sync_binned", "n" => n, "size" => size, "folder" => l.folder().display(), "undo" => undo)
        } else {
            t!("notice.sync_kept", "n" => n, "size" => size, "path" => l.path().display(), "undo" => undo)
        };
        all.push(Notice::new(l.id(), text, (!l.binned).then_some("disk")));
    }
    // Search has stalled: why, before anything else. Dismissed, it comes back with another why.
    if let Some(why) = &status.error {
        all.push(Notice::new(
            format!("error:{why}"),
            t!("notice.search_error", "why" => why),
            Some("search"),
        ));
    }
    if let Some(why) = status.meaning_error.as_ref().filter(|_| status.meaning) {
        all.push(Notice::new(
            format!("error:{why}"),
            t!("notice.meaning_error", "why" => why),
            Some("search_meaning"),
        ));
    }
    // Start with my session is on, but systemd or launchd did not start the helper: this app did.
    if crate::helper::UNSTARTED.load(std::sync::atomic::Ordering::Relaxed) {
        let text = if cfg!(target_os = "macos") {
            t!("notice.helper_unstarted_macos")
        } else {
            t!("notice.helper_unstarted")
        };
        all.push(Notice::new("helper-unstarted", text, Some("search")));
    }
    // A newer version's helper runs, which this app cannot talk to: a restart brings the new one.
    if let Some(v) = &status.outdated {
        all.push(Notice::new(
            format!("helper-outdated-{v}"),
            t!("notice.helper_outdated", "version" => v),
            None,
        ));
    }
    // On a Mac: why macOS asks about folders, and what Coxswain reads.
    if cfg!(target_os = "macos") {
        all.push(Notice::new(
            "macos-folders",
            t!("notice.macos_folders"),
            None,
        ));
    }
    // The vectors are made again to cover whole documents: once the time one takes is known.
    if status.meaning && status.meaning_renewing > 0 && status.meaning_ms_per_passage > 0 {
        let text =
            t!("notice.meaning_renew", "n" => status.meaning_renewing, "time" => time_left(status));
        all.push(Notice::new(
            format!("renew-passages-{}", crate::meaning::SCHEME),
            text,
            Some("search_meaning"),
        ));
    }
    // Everything chosen for long files, with words in files off: nothing is kept, whole or not.
    if crate::long_files::Rule::of(&cfg.search).any() && !cfg.search.text {
        all.push(Notice::new("long-files-text-off", t!("notice.long_files_text_off"), Some("search_text")));
    }
    // Clouds found: their files that are only online are found by name, never downloaded.
    if cfg.search.cloud != "all" && !status.clouds.is_empty() {
        // Each once, in the order first seen: two OneDrive roots name OneDrive once.
        let mut names: Vec<&str> = vec![];
        for (n, _) in &status.clouds {
            if !names.contains(&n.as_str()) {
                names.push(n);
            }
        }
        let names = names.join(", ");
        let text = if terminal {
            t!("notice.cloud_tui", "names" => names)
        } else {
            t!("notice.cloud", "names" => names)
        };
        all.push(Notice::new("cloud", text, Some("search")));
    }
    // After an update, in the terminal app: where to read what it brought. Not on a first
    // start. The desktop app counts the versions not read on its Settings button instead.
    if terminal && !state.seen_version.is_empty() && state.seen_version != version {
        all.push(Notice::new(
            format!("new-{version}"),
            t!("notice.updated_tui", "version" => version),
            None,
        ));
    }
    // A repository was opened: its history is a key away.
    if !state.recent_repos.is_empty() {
        let key = cfg.key_for(crate::config::Action::History).unwrap_or("F9");
        all.push(Notice::new(
            "history",
            t!("notice.history", "key" => key),
            None,
        ));
    }
    // "Open in your git tool" found none: what to install, or where to name one. Once.
    let missed = crate::handoff::MISSING.load(std::sync::atomic::Ordering::Relaxed);
    if missed != 0 {
        let key = cfg.key_for(crate::config::Action::GitTool).unwrap_or("F9");
        let copy = crate::handoff::install();
        // What opened instead, as it was.
        let text = t!(
            match missed {
                crate::handoff::OPENED_SHELL => "notice.git_tool_shell",
                crate::handoff::OPENED_NOTHING => "notice.git_tool_nothing",
                _ => "notice.git_tool",
            },
            "key" => key
        );
        // The terminal app has no Copy button: the line is in the text.
        let text = match &copy {
            Some(line) => format!("{text} {line}"),
            None => text,
        };
        all.push(Notice {
            copy,
            ..Notice::new("git-tool", text, Some("git_tool"))
        });
    }
    // A Jujutsu repository was opened and jj is not installed: git is asked where it can be.
    if crate::vcs::JJ_MISSING.load(std::sync::atomic::Ordering::Relaxed) {
        all.push(Notice::new("jj-missing", t!("notice.jj_missing"), None));
    }
    // The bin was looked at where Coxswain sees only part of it: a Flatpak's own, a Mac's
    // without a way back (or, without Full Disk Access, not at all).
    if crate::bin::LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        let text = if crate::tools::flatpak().is_some() { t!("notice.bin_flatpak") } else { t!("notice.bin_mac") };
        all.push(Notice::new("bin-limited", text, None));
    }
    // A Mercurial repository was opened and hg is not installed.
    if crate::vcs::HG_MISSING.load(std::sync::atomic::Ordering::Relaxed) {
        all.push(Notice::new("hg-missing", t!("notice.hg_missing"), None));
    }
    // A Subversion or CVS history was asked for, and its program is not installed.
    for (flag, id, system, program) in [
        (&crate::wc::SVN_MISSING, "svn-missing", "Subversion", "svn"),
        (&crate::wc::CVS_MISSING, "cvs-missing", "CVS", "cvs"),
    ] {
        if flag.load(std::sync::atomic::Ordering::Relaxed) {
            all.push(Notice::new(id, t!("notice.wc_tool_missing", "system" => system, "program" => program), None));
        }
    }
    // Home is on ZFS: its snapshots are a key away.
    if !seen("zfs") && crate::dirs::home_dir().is_some_and(|h| crate::zfs::dataset_of(&h).is_some()) {
        let key = cfg
            .key_for(crate::config::Action::Snapshots)
            .unwrap_or("F9");
        all.push(Notice::new("zfs", t!("notice.zfs", "key" => key), None));
    }
    if cfg.search.text && !cfg.search.meaning && status.texts > 0 {
        let text = if terminal {
            t!("notice.meaning_tui")
        } else {
            t!("notice.meaning")
        };
        all.push(Notice::new("meaning", text, Some("search_meaning")));
    }
    // The built-in model on a Mac's GPU, or on its CPU when the GPU could not be used: said once.
    let runs = status.meaning_runs.as_ref().filter(|_| status.meaning);
    if let Some(runs) = runs {
        if runs.metal {
            all.push(Notice::new(
                "meaning-metal",
                t!("notice.meaning_metal", "n" => format!("{:.0}", runs.faster.max(1.0))),
                Some("search_meaning"),
            ));
        } else if let Some(why) = runs
            .cpu_why
            .as_ref()
            .filter(|w| **w != crate::meaning::Fallback::Chosen)
        {
            all.push(Notice::new(
                "meaning-cpu",
                t!("notice.meaning_cpu", "why" => why.text()),
                Some("search_meaning"),
            ));
        }
    }
    // A built-in model on the processor while the machine has a graphics card and no model
    // server answers: a local Ollama would use the card, and how to install it. Said once.
    if crate::setup::uses_builtin(&cfg.search)
        && !seen("gpu-ollama")
        && let Some(l) = crate::setup::look(false)
        && let Some(f) =
            crate::setup::faster_with_ollama(&l.machine, &l.found, std::env::consts::OS)
    {
        all.push(Notice {
            copy: f.copy,
            ..Notice::new("gpu-ollama", f.text + &f.how, Some("search_meaning"))
        });
    }
    // A built-in model on the processor while a server runs its models on the graphics card,
    // or a chat model too large for the processor: said once here, and on its line in
    // Settings for as long as the choice stands.
    for p in crate::setup::poor(&cfg.search, false) {
        let (id, at) = if p.ask {
            (format!("poor-ask:{}", cfg.search.ask_model), "ask_model")
        } else {
            ("poor-meaning".to_string(), "meaning_engine")
        };
        all.push(Notice::new(id, p.text, Some(at)));
    }
    // The desktop app's preview cannot play video and sound here: what to install.
    if !terminal {
        if let Some(why) = crate::tools::media_missing() {
            all.push(Notice::new("media", why, None));
        }
        // Chinese, Japanese, Korean, Persian, Armenian or Georgian without a font for it: what to install.
        if crate::tools::script_font_missing() {
            let lang = crate::i18n::language();
            let text = match lang {
                "ja" | "ko" | "zh-TW" | "zh-SG" => match crate::tools::install("cjk-font") {
                    Some(cmd) => t!("notice.cjk_font_cmd", "cmd" => cmd),
                    None => t!("notice.cjk_font"),
                },
                _ => {
                    t!("notice.script_font", "language" => crate::i18n::find(lang).map_or(lang, |l| l.name), "fonts" => crate::tools::script_fonts(lang))
                }
            };
            all.push(Notice::new(
                if matches!(lang, "ja" | "ko" | "zh-TW" | "zh-SG") {
                    "cjk-font"
                } else {
                    "script-font"
                },
                text,
                None,
            ));
        }
    }
    // In Termux: the phone's folders wait for termux-setup-storage, the clipboard and the
    // battery for Termux:API.
    if crate::termux::storage_missing() {
        let key = cfg
            .key_for(crate::config::Action::GotoLeft)
            .unwrap_or("Alt+F1");
        all.push(Notice {
            copy: Some("termux-setup-storage".into()),
            ..Notice::new(
                "termux-storage",
                t!("notice.termux_storage", "key" => key),
                None,
            )
        });
    }
    if crate::termux::api_missing() {
        all.push(Notice {
            copy: Some("pkg install termux-api".into()),
            ..Notice::new("termux-api", t!("notice.termux_api"), None)
        });
    }
    // A new translation in use: where to suggest a better word.
    if let Some(l) = crate::i18n::find(crate::i18n::language()).filter(|l| l.new()) {
        let text = if terminal {
            t!("notice.language_new_tui", "language" => l.name)
        } else {
            t!("notice.language_new", "language" => l.name)
        };
        all.push(Notice::new(
            format!("language-{}", l.code),
            text,
            Some("language"),
        ));
    }
    // The search helper found no tesseract: pictures and scans have no words to search.
    if cfg.search.text
        && status
            .tools
            .iter()
            .any(|(name, there)| name == "tesseract" && !there)
    {
        let copy = crate::tools::install("tesseract");
        let text = match &copy {
            Some(line) => t!("notice.tesseract_install", "command" => line),
            None => t!("notice.tesseract"),
        };
        all.push(Notice {
            copy,
            ..Notice::new("tesseract", text, Some("search"))
        });
    }
    // Apache Tika is turned on, but there is no podman or docker to run it in. Once.
    if cfg.search.tika
        && cfg.search.text
        && status.tika == Some(crate::extract::tika::State::NoRuntime)
    {
        all.push(Notice::new("tika-runtime", t!("notice.tika_no_runtime"), Some("search_tika")));
    }
    // Transcribing is on, but cannot run here (no whisper.cpp, no podman or docker, no model).
    if cfg.search.whisper
        && cfg.search.text
        && let Some(crate::transcribe::State::Off(why)) = status.transcripts.as_ref().map(|t| &t.state)
    {
        all.push(Notice::new("whisper-off", t!("notice.whisper_off", "why" => why), Some("search_whisper")));
    }
    // Disk images open like folders, but no 7-Zip reads them: what to install. Once.
    if cfg.archive.disk_images && crate::disk_image::seven_zip().is_none() {
        let copy = crate::tools::install("7z");
        let text = match &copy {
            Some(line) => t!("disk_image.next_install", "command" => line),
            None => t!("disk_image.next_install_get"),
        };
        all.push(Notice {
            copy,
            ..Notice::new("7zip", text, Some("disk_images"))
        });
    }
    // E-books are turned on, but neither calibre nor podman or docker is there to read them. Once.
    if cfg.preview.ebooks
        && let Err(why) = crate::ebook::how(&cfg.preview)
    {
        all.push(Notice {
            copy: crate::tools::install("calibre"),
            ..Notice::new("ebooks", why, Some("ebooks"))
        });
    }
    // The chess view asks an engine about each position, but none is installed: what to install.
    if cfg.chess.analysis
        && let Err(text) = crate::chess::play::engine_for(cfg)
    {
        let copy = crate::chess::play::install();
        all.push(Notice {
            copy,
            ..Notice::new("chess-engine", text, Some("chess_analysis"))
        });
    }
    all.retain(|n| !seen(&n.id));
    all
}

/// A version's changes, from the changelog at the bottom of the README: plain text, and the
/// links in it as (label, address).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Change {
    pub version: String,
    pub date: String,
    pub parts: Vec<(String, Option<String>)>,
}

const DOCS_URL: &str = "https://github.com/mwo-dk/coxswain/blob/master/";

/// Every version's changes, newest first.
pub fn changes() -> Vec<Change> {
    changelog(
        include_str!(env!("COXSWAIN_CHANGELOG")),
        env!("COXSWAIN_CHANGELOG_BASE"),
    )
}

/// A version's whole text, its links written out after their labels: `coxswain --whats-new`
/// and the terminal app's viewer.
pub fn text(c: &Change) -> String {
    c.parts
        .iter()
        .map(|(t, url)| url.as_ref().map_or_else(|| t.clone(), |u| format!("{t} <{u}>")))
        .collect()
}

/// A version's whole text in a file of the cache, for the terminal app's viewer: Enter on a
/// version under *What's new*.
pub fn text_file(c: &Change) -> std::io::Result<std::path::PathBuf> {
    let name: String = c
        .version
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.')
        .collect();
    let to = crate::archive::run_folder("news", false)?.join(format!("coxswain-{name}.txt"));
    std::fs::write(&to, format!("Coxswain {}  {}\n\n{}\n", c.version, c.date, text(c)))?;
    Ok(to)
}

fn changelog(readme: &str, base: &str) -> Vec<Change> {
    // `## Changelog` in the README, `# Changelog` on its docs page.
    let Some((_, table)) = readme.split_once("# Changelog") else {
        return vec![];
    };
    table
        .lines()
        .filter_map(|line| {
            let mut cells = line.strip_prefix("| **")?.splitn(3, " | ");
            let version = cells.next()?.strip_suffix("**")?.to_string();
            let date = cells.next()?.to_string();
            let text = cells
                .next()?
                .trim_end()
                .strip_suffix('|')?
                .trim()
                .replace("\\|", "|");
            Some(Change {
                version,
                date,
                parts: parts(&text, base),
            })
        })
        .collect()
}

/// Markdown made plain: emphasis and code marks go, `[label](target)` becomes a link, a page
/// of the docs one on GitHub.
fn parts(text: &str, base: &str) -> Vec<(String, Option<String>)> {
    let plain = |s: &str| s.replace(['*', '`'], "");
    let mut out = vec![];
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some((label, after)) = rest[open + 1..].split_once("](") else {
            break;
        };
        let Some((target, tail)) = after.split_once(')') else {
            break;
        };
        out.push((plain(&rest[..open]), None));
        let url = if target.starts_with("http") {
            target.to_string()
        } else if let Some(up) = target.strip_prefix("../") {
            format!("{DOCS_URL}{up}")
        } else {
            format!("{DOCS_URL}{base}{target}")
        };
        out.push((plain(label), Some(url)));
        rest = tail;
    }
    out.push((plain(rest), None));
    out.retain(|(t, _)| !t.is_empty());
    out
}

/// The versions after the one whose changes were last read, newest first.
pub fn unread(state: &AppState) -> Vec<Change> {
    if state.seen_version.is_empty() {
        return vec![];
    }
    changes()
        .into_iter()
        .filter(|c| crate::update::is_newer(&state.seen_version, &c.version))
        .collect()
}

/// The changes have been read, up to this version.
pub fn read(state: &mut AppState) {
    state.seen_version = crate::update::VERSION.to_string();
}

/// Remember a notice as seen, and the version in use.
pub fn dismiss(state: &mut AppState, id: &str) {
    if !state.notices_dismissed.iter().any(|d| d == id) {
        state.notices_dismissed.push(id.to_string());
    }
    if id.starts_with("new-") {
        state.seen_version = crate::update::VERSION.to_string();
    }
}

/// The first start of this version: remembered, so the next update is told of.
pub fn started(state: &mut AppState) -> bool {
    if state.seen_version.is_empty() {
        state.seen_version = crate::update::VERSION.to_string();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_come_one_at_a_time_and_stay_away_once_dismissed() {
        let cfg = Config::default();
        let mut status = Status {
            state: crate::index::State::Ready,
            len: 1,
            texts: 10,
            pending: 0,
            bytes: 0,
            paused: false,
            roots: vec![],
            tools: vec![("tesseract".into(), false)],
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
            clouds: vec![],
            tika: None,
            transcripts: None,
            outdated: None,
            changes_left: 0,
            changes_freed: None,
        };
        let mut state = AppState::default();
        // Whether this machine's GStreamer can play video is not what is tested here.
        dismiss(&mut state, "media");
        // Nor whether the home folder is on ZFS (it is on illumos and many FreeBSD machines).
        dismiss(&mut state, "zfs");
        // On a Mac: the folder prompts explained, once.
        let first = next(&cfg, &status, &state, false).map(|n| n.id);
        assert_eq!(
            first.as_deref() == Some("macos-folders"),
            cfg!(target_os = "macos"),
            "{first:?}"
        );
        dismiss(&mut state, "macos-folders");
        assert!(started(&mut state), "a first start is told of nothing new");
        assert!(!started(&mut state));

        let ids =
            |state: &AppState, status: &Status| next(&cfg, status, state, false).map(|n| n.id);
        assert_eq!(ids(&state, &status).as_deref(), Some("meaning"));
        dismiss(&mut state, "meaning");
        let n = next(&cfg, &status, &state, false).unwrap();
        assert_eq!(n.id, "tesseract");
        // Where Coxswain knows the line that installs it, the notice says it and offers to copy it.
        if let Some(line) = crate::tools::install("tesseract") {
            assert!(
                n.text.contains(&line) && n.copy.as_deref() == Some(line.as_str()),
                "{n:?}"
            );
        }
        dismiss(&mut state, "tesseract");
        assert_eq!(ids(&state, &status), None);

        state.seen_version = "0.9.0".into();
        assert_eq!(
            ids(&state, &status),
            None,
            "the desktop app counts what is new on its Settings button"
        );
        assert!(!unread(&state).is_empty());
        let tui = next(&cfg, &status, &state, true).unwrap();
        assert_eq!(
            tui.id,
            format!("new-{}", crate::update::VERSION),
            "the terminal app tells of an update"
        );
        assert!(tui.text.contains("--whats-new"), "{}", tui.text);
        dismiss(&mut state, &tui.id);
        assert_eq!(
            (
                next(&cfg, &status, &state, true),
                state.seen_version.as_str()
            ),
            (None, crate::update::VERSION)
        );
        assert!(unread(&state).is_empty());

        // In a repository for the first time: told of the history, once.
        state.touch_repo(std::path::Path::new("/r"));
        assert_eq!(ids(&state, &status).as_deref(), Some("history"));
        assert!(
            next(&cfg, &status, &state, true)
                .unwrap()
                .text
                .contains("Ctrl+G")
        );
        dismiss(&mut state, "history");
        assert_eq!(ids(&state, &status), None);

        // Started on a config.toml of 1.x: what 2.0 renamed, first, once.
        state.migrated = vec!["[keys] mkdir → new_folder".into()];
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(
            n.id == "migrated-2" && n.text.contains("mkdir → new_folder"),
            "{n:?}"
        );
        dismiss(&mut state, &n.id);
        assert_eq!(ids(&state, &status), None);

        // Search stalled: said at once, and again when the reason changes.
        status.meaning_error = Some("http://localhost:11434: Connection refused".into());
        assert_eq!(ids(&state, &status), None, "not while meaning is off");
        status.meaning = true;
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(n.text.contains("Connection refused"), "{}", n.text);
        dismiss(&mut state, &n.id);
        status.meaning_error = None;
        status.error = Some("constraint failed".into());
        assert!(
            next(&cfg, &status, &state, true)
                .unwrap()
                .text
                .contains("constraint failed")
        );
        status.error = None;

        // Updated while this app ran, to a version whose helper it cannot talk to: once.
        status.outdated = Some("9.0.0".into());
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(
            n.id == "helper-outdated-9.0.0" && n.text.contains("9.0.0"),
            "{n:?}"
        );
        dismiss(&mut state, &n.id);
        assert_eq!(ids(&state, &status), None);
        status.outdated = None;

        // Clouds found: told once that their online files stay there.
        status.clouds = vec![
            ("OneDrive".into(), "/c/OneDrive".into()),
            ("Dropbox".into(), "/c/Dropbox".into()),
            ("OneDrive".into(), "/c/OneDrive - Work".into()),
        ];
        let n = next(&cfg, &status, &state, false).unwrap();
        assert_eq!(n.id, "cloud");
        assert!(n.text.contains("OneDrive, Dropbox"), "{}", n.text);
        assert_eq!(n.text.matches("OneDrive").count(), 1, "{}", n.text);
        dismiss(&mut state, "cloud");
        assert_eq!(ids(&state, &status), None);

        // Vectors made again to cover whole documents: told once, with the time it takes here.
        status.meaning_renewing = 3437;
        assert_eq!(
            ids(&state, &status),
            None,
            "not before the time a file takes is known"
        );
        (status.meaning_pending, status.meaning_ms_per_passage) = (3437, 1200);
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(
            n.text.contains("3437") && n.text.contains("69 minutes"),
            "{}",
            n.text
        );
        // Five passages a file so far: five times as long.
        (status.meaning_done, status.meaning_passages) = (100, 500);
        assert_eq!(time_left(&status), "6 hours");
        (status.meaning_done, status.meaning_passages) = (0, 0);
        dismiss(&mut state, &n.id);

        // The built-in model on a Mac: on its GPU, or on its CPU and why; once each.
        use crate::meaning::{Fallback, Runs};
        status.meaning_runs = Some(Runs {
            metal: true,
            faster: 6.4,
            cpu_why: None,
        });
        let n = next(&cfg, &status, &state, false).unwrap();
        assert!(
            n.id == "meaning-metal" && n.text.contains('6'),
            "{}",
            n.text
        );
        dismiss(&mut state, &n.id);
        status.meaning_runs = Some(Runs {
            metal: false,
            faster: 0.0,
            cpu_why: Some(Fallback::Chosen),
        });
        assert_eq!(
            ids(&state, &status),
            None,
            "the CPU the user chose is no news"
        );
        status.meaning_runs = Some(Runs {
            metal: false,
            faster: 0.0,
            cpu_why: Some(Fallback::Load("no memory".into())),
        });
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(
            n.id == "meaning-cpu" && n.text.contains("no memory"),
            "{}",
            n.text
        );
        dismiss(&mut state, &n.id);
        assert_eq!(ids(&state, &status), None);
    }

    #[test]
    fn changelog_is_read_with_its_links() {
        let all = changes();
        assert_eq!(
            all[0].version,
            crate::update::VERSION,
            "the changelog has a row for this version"
        );
        assert!(all.len() > 50 && all.iter().all(|c| c.date.len() == 10));
        let c = &changelog(
            "## Changelog\n| Version | Date | What's new |\n|---|---|---|\n| **1.2.3** | 2026-01-02 | *Ask* is `quicker` a \\| b. [Ask](docs/search/ask.md) · [site](https://x.dk) |\n",
            "",
        )[0];
        assert_eq!(c.version, "1.2.3");
        assert_eq!(c.parts[0], ("Ask is quicker a | b. ".into(), None));
        assert_eq!(
            c.parts[1],
            ("Ask".into(), Some(format!("{DOCS_URL}docs/search/ask.md")))
        );
        assert_eq!(c.parts[3], ("site".into(), Some("https://x.dk".into())));
    }
}

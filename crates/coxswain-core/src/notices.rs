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
}

/// The kinds of search on, for the apps' titles: "names · text · meaning".
pub fn search_level(cfg: &Config, status: &Status) -> String {
    let mut kinds = vec![t!("title.names")];
    if cfg.search.text {
        kinds.push(t!("title.text"));
    }
    if status.meaning {
        kinds.push(t!("title.meaning"));
    }
    kinds.join(" · ")
}

/// About how long the files still to get their vectors take here, as the helper measured
/// them: "40 minutes", "3 hours".
pub fn time_left(status: &Status) -> String {
    crate::meaning::about(status.meaning_pending as f64 * status.meaning_ms_per_file as f64 / 1000.0)
}

/// The notice to show now, if any. `terminal`: the notice names the command instead of Settings.
pub fn next(cfg: &Config, status: &Status, state: &AppState, terminal: bool) -> Option<Notice> {
    all(cfg, status, state, terminal).into_iter().next()
}

/// Every notice not dismissed yet, the most pressing first.
pub fn all(cfg: &Config, status: &Status, state: &AppState, terminal: bool) -> Vec<Notice> {
    let version = crate::update::VERSION;
    let seen = |id: &str| state.notices_dismissed.iter().any(|d| d == id);
    let mut all = vec![];
    // Search has stalled: why, before anything else. Dismissed, it comes back with another why.
    if let Some(why) = &status.error {
        all.push(Notice { id: format!("error:{why}"), text: t!("notice.search_error", "why" => why), settings: Some("search") });
    }
    if let Some(why) = status.meaning_error.as_ref().filter(|_| status.meaning) {
        all.push(Notice { id: format!("error:{why}"), text: t!("notice.meaning_error", "why" => why), settings: Some("meaning") });
    }
    // The vectors are made again to cover whole documents: once the time one takes is known.
    if status.meaning && status.meaning_renewing > 0 && status.meaning_ms_per_file > 0 {
        let text = t!("notice.meaning_renew", "n" => status.meaning_renewing, "time" => time_left(status));
        all.push(Notice { id: format!("renew-passages-{}", crate::meaning::SCHEME), text, settings: Some("meaning") });
    }
    // Clouds found: their files that are only online are found by name, never downloaded.
    if cfg.search.cloud != "all" && !status.clouds.is_empty() {
        let mut names: Vec<&str> = status.clouds.iter().map(|(n, _)| n.as_str()).collect();
        names.dedup();
        let names = names.join(", ");
        let text = if terminal { t!("notice.cloud_tui", "names" => names) } else { t!("notice.cloud", "names" => names) };
        all.push(Notice { id: "cloud".into(), text, settings: Some("search") });
    }
    // After an update, in the terminal app: where to read what it brought. Not on a first
    // start. The desktop app counts the versions not read on its Settings button instead.
    if terminal && !state.seen_version.is_empty() && state.seen_version != version {
        all.push(Notice { id: format!("new-{version}"), text: t!("notice.updated_tui", "version" => version), settings: None });
    }
    // A repository was opened: its history is a key away.
    if !state.recent_repos.is_empty() {
        let key = cfg.key_for(crate::config::Action::History).unwrap_or("F9");
        all.push(Notice { id: "history".into(), text: t!("notice.history", "key" => key), settings: None });
    }
    if cfg.search.text && !cfg.search.meaning && status.texts > 0 {
        let text = if terminal { t!("notice.meaning_tui") } else { t!("notice.meaning") };
        all.push(Notice { id: "meaning".into(), text, settings: Some("meaning") });
    }
    // Meaning by the built-in model on the CPU, while Ollama answers here: it could use the GPU.
    if status.meaning && status.meaning_engine.starts_with("builtin") && cfg.search.meaning_engine == "builtin" && !seen("ollama") && crate::meaning::ollama_here() {
        let text = if terminal { t!("notice.ollama_tui") } else { t!("notice.ollama") };
        all.push(Notice { id: "ollama".into(), text, settings: Some("meaning") });
    }
    // The desktop app's preview cannot play video and sound here: what to install.
    if !terminal {
        if let Some(why) = crate::tools::media_missing() {
            all.push(Notice { id: "media".into(), text: why, settings: None });
        }
    }
    // The helper found no tesseract: pictures and scans have no words to search.
    if cfg.search.text && status.tools.iter().any(|(name, there)| name == "tesseract" && !there) {
        all.push(Notice { id: "tesseract".into(), text: t!("notice.tesseract"), settings: Some("search") });
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
    changelog(include_str!(env!("COXSWAIN_README")))
}

fn changelog(readme: &str) -> Vec<Change> {
    let Some((_, table)) = readme.split_once("## Changelog") else { return vec![] };
    table
        .lines()
        .filter_map(|line| {
            let mut cells = line.strip_prefix("| **")?.splitn(3, " | ");
            let version = cells.next()?.strip_suffix("**")?.to_string();
            let date = cells.next()?.to_string();
            let text = cells.next()?.trim_end().strip_suffix('|')?.trim().replace("\\|", "|");
            Some(Change { version, date, parts: parts(&text) })
        })
        .collect()
}

/// Markdown made plain: emphasis and code marks go, `[label](target)` becomes a link, a page
/// of the docs one on GitHub.
fn parts(text: &str) -> Vec<(String, Option<String>)> {
    let plain = |s: &str| s.replace(['*', '`'], "");
    let mut out = vec![];
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some((label, after)) = rest[open + 1..].split_once("](") else { break };
        let Some((target, tail)) = after.split_once(')') else { break };
        out.push((plain(&rest[..open]), None));
        let url = if target.starts_with("http") { target.to_string() } else { format!("{DOCS_URL}{target}") };
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
    changes().into_iter().filter(|c| crate::update::is_newer(&state.seen_version, &c.version)).collect()
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
        let mut status = Status { state: crate::index::State::Ready, len: 1, texts: 10, pending: 0, bytes: 0, paused: false, roots: vec![], tools: vec![("tesseract".into(), false)], meaning: false, meaning_pending: 0, meaning_done: 0, meaning_renewing: 0, meaning_ms_per_file: 0, meaning_engine: String::new(), meaning_error: None, error: None, clouds: vec![] };
        let mut state = AppState::default();
        // Whether this machine's GStreamer can play video is not what is tested here.
        dismiss(&mut state, "media");
        assert!(started(&mut state), "a first start is told of nothing new");
        assert!(!started(&mut state));

        let ids = |state: &AppState, status: &Status| next(&cfg, status, state, false).map(|n| n.id);
        assert_eq!(ids(&state, &status).as_deref(), Some("meaning"));
        dismiss(&mut state, "meaning");
        assert_eq!(ids(&state, &status).as_deref(), Some("tesseract"));
        dismiss(&mut state, "tesseract");
        assert_eq!(ids(&state, &status), None);

        state.seen_version = "0.9.0".into();
        assert_eq!(ids(&state, &status), None, "the desktop app counts what is new on its Settings button");
        assert!(!unread(&state).is_empty());
        let tui = next(&cfg, &status, &state, true).unwrap();
        assert_eq!(tui.id, format!("new-{}", crate::update::VERSION), "the terminal app tells of an update");
        assert!(tui.text.contains("--whats-new"), "{}", tui.text);
        dismiss(&mut state, &tui.id);
        assert_eq!((next(&cfg, &status, &state, true), state.seen_version.as_str()), (None, crate::update::VERSION));
        assert!(unread(&state).is_empty());

        // In a repository for the first time: told of the history, once.
        state.touch_repo(std::path::Path::new("/r"));
        assert_eq!(ids(&state, &status).as_deref(), Some("history"));
        assert!(next(&cfg, &status, &state, true).unwrap().text.contains("Ctrl+G"));
        dismiss(&mut state, "history");
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
        assert!(next(&cfg, &status, &state, true).unwrap().text.contains("constraint failed"));
        status.error = None;
        assert!(search_level(&cfg, &status).split(" · ").count() == 3);

        // Clouds found: told once that their online files stay there.
        status.clouds = vec![("OneDrive".into(), "/c/OneDrive".into()), ("Dropbox".into(), "/c/Dropbox".into())];
        let n = next(&cfg, &status, &state, false).unwrap();
        assert_eq!(n.id, "cloud");
        assert!(n.text.contains("OneDrive, Dropbox"), "{}", n.text);
        dismiss(&mut state, "cloud");
        assert_eq!(ids(&state, &status), None);

        // Vectors made again to cover whole documents: told once, with the time it takes here.
        status.meaning_renewing = 3437;
        assert_eq!(ids(&state, &status), None, "not before the time a file takes is known");
        (status.meaning_pending, status.meaning_ms_per_file) = (3437, 1200);
        let n = next(&cfg, &status, &state, true).unwrap();
        assert!(n.text.contains("3437") && n.text.contains("69 minutes"), "{}", n.text);
        dismiss(&mut state, &n.id);
        assert_eq!(ids(&state, &status), None);
    }

    #[test]
    fn changelog_is_read_from_the_readme_with_its_links() {
        let all = changes();
        assert_eq!(all[0].version, crate::update::VERSION, "the changelog has a row for this version");
        assert!(all.len() > 50 && all.iter().all(|c| c.date.len() == 10));
        let c = &changelog("## Changelog\n| Version | Date | What's new |\n|---|---|---|\n| **1.2.3** | 2026-01-02 | *Ask* is `quicker` a \\| b. [Ask](docs/search/ask.md) · [site](https://x.dk) |\n")[0];
        assert_eq!(c.version, "1.2.3");
        assert_eq!(c.parts[0], ("Ask is quicker a | b. ".into(), None));
        assert_eq!(c.parts[1], ("Ask".into(), Some(format!("{DOCS_URL}docs/search/ask.md"))));
        assert_eq!(c.parts[3], ("site".into(), Some("https://x.dk".into())));
    }
}

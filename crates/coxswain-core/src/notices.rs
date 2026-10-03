//! Notices: what Coxswain can do that this user has not turned on yet, and what a new version
//! brought. Both apps show one at a time in their status line, the way they tell of an update;
//! one that was dismissed, or acted on, is not shown again.

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
    /// The Settings section that does it (desktop app), or the web page to read.
    pub settings: Option<&'static str>,
    pub url: Option<&'static str>,
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

/// The notice to show now, if any. `terminal`: the notice names the command instead of Settings.
pub fn next(cfg: &Config, status: &Status, state: &AppState, terminal: bool) -> Option<Notice> {
    let version = crate::update::VERSION;
    let seen = |id: &str| state.notices_dismissed.iter().any(|d| d == id);
    let mut all = vec![];
    // Search has stalled: why, before anything else. Dismissed, it comes back with another why.
    if let Some(why) = &status.error {
        all.push(Notice { id: format!("error:{why}"), text: t!("notice.search_error", "why" => why), settings: Some("search"), url: None });
    }
    if let Some(why) = status.meaning_error.as_ref().filter(|_| status.meaning) {
        all.push(Notice { id: format!("error:{why}"), text: t!("notice.meaning_error", "why" => why), settings: Some("meaning"), url: None });
    }
    // Clouds found: their files that are only online are found by name, never downloaded.
    if cfg.search.cloud != "all" && !status.clouds.is_empty() {
        let mut names: Vec<&str> = status.clouds.iter().map(|(n, _)| n.as_str()).collect();
        names.dedup();
        let names = names.join(", ");
        let text = if terminal { t!("notice.cloud_tui", "names" => names) } else { t!("notice.cloud", "names" => names) };
        all.push(Notice { id: "cloud".into(), text, settings: Some("search"), url: None });
    }
    // After an update: where to read what it brought. Not on a first start.
    if !state.seen_version.is_empty() && state.seen_version != version {
        all.push(Notice { id: format!("new-{version}"), text: t!("notice.updated", "version" => version), settings: None, url: Some(crate::update::RELEASES_URL) });
    }
    // A repository was opened: its history is a key away.
    if !state.recent_repos.is_empty() {
        let key = cfg.key_for(crate::config::Action::History).unwrap_or("F9");
        all.push(Notice { id: "history".into(), text: t!("notice.history", "key" => key), settings: None, url: None });
    }
    if cfg.search.text && !cfg.search.meaning && status.texts > 0 {
        let text = if terminal { t!("notice.meaning_tui") } else { t!("notice.meaning") };
        all.push(Notice { id: "meaning".into(), text, settings: Some("meaning"), url: None });
    }
    // Meaning by the built-in model on the CPU, while Ollama answers here: it could use the GPU.
    if status.meaning && status.meaning_engine.starts_with("builtin") && cfg.search.meaning_engine == "builtin" && !seen("ollama") && crate::meaning::ollama_here() {
        let text = if terminal { t!("notice.ollama_tui") } else { t!("notice.ollama") };
        all.push(Notice { id: "ollama".into(), text, settings: Some("meaning"), url: None });
    }
    // The helper found no tesseract: pictures and scans have no words to search.
    if cfg.search.text && status.tools.iter().any(|(name, there)| name == "tesseract" && !there) {
        all.push(Notice { id: "tesseract".into(), text: t!("notice.tesseract"), settings: Some("search"), url: None });
    }
    all.into_iter().find(|n| !seen(&n.id))
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
        let mut status = Status { state: crate::index::State::Ready, len: 1, texts: 10, pending: 0, bytes: 0, paused: false, roots: vec![], tools: vec![("tesseract".into(), false)], meaning: false, meaning_pending: 0, meaning_done: 0, meaning_engine: String::new(), meaning_error: None, error: None, clouds: vec![] };
        let mut state = AppState::default();
        assert!(started(&mut state), "a first start is told of nothing new");
        assert!(!started(&mut state));

        let ids = |state: &AppState, status: &Status| next(&cfg, status, state, false).map(|n| n.id);
        assert_eq!(ids(&state, &status).as_deref(), Some("meaning"));
        dismiss(&mut state, "meaning");
        assert_eq!(ids(&state, &status).as_deref(), Some("tesseract"));
        dismiss(&mut state, "tesseract");
        assert_eq!(ids(&state, &status), None);

        state.seen_version = "0.9.0".into();
        assert_eq!(ids(&state, &status), Some(format!("new-{}", crate::update::VERSION)), "an update is told of");
        dismiss(&mut state, &format!("new-{}", crate::update::VERSION));
        assert_eq!((ids(&state, &status), state.seen_version.as_str()), (None, crate::update::VERSION));

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
    }
}

//! The built-in version, and a once-a-day check for a newer release on GitHub.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::state::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_URL: &str = "https://github.com/mwo-dk/coxswain/releases/latest";
const API_URL: &str = "https://api.github.com/repos/mwo-dk/coxswain/releases/latest";
const DAY: u64 = 24 * 3600;

/// `major.minor.patch` from "1.2.3" or "v1.2.3"; pre-release suffixes are ignored.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Is `latest` newer than `current`? Unparseable versions never are.
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse(current), parse(latest)), (Some(c), Some(l)) if l > c)
}

pub fn fetch_latest() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    let body = agent
        .get(API_URL)
        .header("User-Agent", concat!("coxswain/", env!("CARGO_PKG_VERSION")))
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    Some(v["tag_name"].as_str()?.trim_start_matches('v').to_string())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Has a day passed since the last check?
pub fn due(st: &AppState) -> bool {
    now().saturating_sub(st.update_checked) >= DAY
}

pub fn record(st: &mut AppState, latest: String) {
    st.latest_version = latest;
    st.update_checked = now();
}

/// The newer version the last check found, if any.
pub fn available(st: &AppState) -> Option<String> {
    is_newer(VERSION, &st.latest_version).then(|| st.latest_version.clone())
}

/// For callers without their own state handling (the TUI): load, check if due, save.
/// Blocks for up to 5 s, so run it on a thread. Offline just means "no news".
pub fn check() -> Option<String> {
    let mut st = AppState::load();
    if due(&st) {
        if let Some(latest) = fetch_latest() {
            record(&mut st, latest);
            let _ = st.save();
        }
    }
    available(&st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_version_compare() {
        assert!(is_newer("1.0.0", "v1.0.1"));
        assert!(is_newer("1.9.9", "2.0.0"));
        assert!(is_newer("1.2.3", "1.10.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.1.0", "v1.0.9"));
        assert!(!is_newer("1.0.0", ""));
        assert!(!is_newer("1.0.0", "garbage"));
        assert!(is_newer("1.0.0", "1.1.0-rc.1"));
        assert!(parse(VERSION).is_some());
    }
}

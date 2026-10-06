//! Termux on Android: the terminal app in an Android terminal. Its programs live under
//! `$PREFIX` (`/data/data/com.termux/files/usr`); the phone's own folders are links under
//! `~/storage` once `termux-setup-storage` has been run; files open with `termux-open`, and the
//! clipboard and the battery are reached with the Termux:API commands when they are installed.
//! There is no session service: the search helper starts with the app and lingers after it.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Whether this runs inside Termux.
pub fn active() -> bool {
    is_termux(std::env::var_os("TERMUX_VERSION").is_some(), std::env::var_os("PREFIX").as_deref().map(Path::new))
}

fn is_termux(version: bool, prefix: Option<&Path>) -> bool {
    version || prefix.is_some_and(|p| p.to_string_lossy().contains("/com.termux/"))
}

/// A Termux program, when this is Termux and it is installed.
fn program(name: &str) -> Option<PathBuf> {
    active().then(|| crate::tools::which(name)).flatten()
}

/// The phone's folders that `termux-setup-storage` linked into `~/storage`, `shared` (the
/// whole internal storage) first; empty before it was run.
pub fn storage() -> Vec<(String, PathBuf)> {
    if !active() {
        return vec![];
    }
    let Some(dir) = std::env::home_dir().map(|h| h.join("storage")) else { return vec![] };
    storage_in(&dir)
}

fn storage_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        // A link to a card that was taken out leads nowhere.
        .filter(|(_, p)| p.is_dir())
        .collect();
    v.sort_by(|a, b| (a.0 != "shared", &a.0).cmp(&(b.0 != "shared", &b.0)));
    v
}

/// Whether the phone's folders still wait for `termux-setup-storage`.
pub fn storage_missing() -> bool {
    active() && std::env::home_dir().is_some_and(|h| !h.join("storage").exists())
}

/// Whether the Termux:API commands (clipboard, battery) are missing.
pub fn api_missing() -> bool {
    active() && crate::tools::which("termux-clipboard-set").is_none()
}

/// `termux-open`, the Android way to open a file with its app.
pub fn opener() -> Option<PathBuf> {
    program("termux-open")
}

/// `text` on Android's clipboard. False when Termux:API is not there to do it.
pub fn copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::Stdio;
    let Some(set) = program("termux-clipboard-set") else { return false };
    let Ok(mut child) = crate::tools::command(set).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let wrote = child.stdin.take().is_some_and(|mut i| i.write_all(text.as_bytes()).is_ok());
    // Without the Termux:API app the command waits for it forever.
    crate::tools::wait(&mut child, Duration::from_secs(3)).is_ok_and(|s| s.success()) && wrote
}

/// Whether the phone runs on its battery: `None` when Termux:API cannot tell.
pub fn on_battery() -> Option<bool> {
    let status = program("termux-battery-status")?;
    let out = crate::tools::output(&status, &[], Duration::from_secs(5), 4096)?;
    unplugged(&out)
}

/// `termux-battery-status` prints `"plugged": "UNPLUGGED"` (or `PLUGGED_AC`, `PLUGGED_USB` …).
fn unplugged(json: &[u8]) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    Some(v["plugged"].as_str()? == "UNPLUGGED")
}

/// Termux's own certificate store: Android's is out of reach of a program in a terminal, and
/// a static Linux binary looks for one under `/etc`, which Android does not have.
pub fn certificates() -> Option<Vec<u8>> {
    if !active() {
        return None;
    }
    let prefix = std::env::var_os("PREFIX").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/usr"));
    std::fs::read(prefix.join("etc/tls/cert.pem")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn termux_is_told_by_its_variables() {
        assert!(is_termux(true, None));
        assert!(is_termux(false, Some(Path::new("/data/data/com.termux/files/usr"))));
        assert!(!is_termux(false, Some(Path::new("/usr"))));
        assert!(!is_termux(false, None));
    }

    #[test]
    fn termux_battery_status_is_read() {
        let on = br#"{"health":"GOOD","percentage":81,"plugged":"UNPLUGGED","status":"DISCHARGING","temperature":27.5}"#;
        let off = br#"{"health":"GOOD","percentage":81,"plugged":"PLUGGED_USB","status":"CHARGING"}"#;
        assert_eq!(unplugged(on), Some(true));
        assert_eq!(unplugged(off), Some(false));
        assert_eq!(unplugged(b"Termux:API is not installed"), None);
    }

    #[test]
    fn termux_storage_lists_shared_first() {
        let d = std::env::temp_dir().join(format!("coxswain-termux-{}", std::process::id()));
        for n in ["downloads", "shared", "dcim"] {
            std::fs::create_dir_all(d.join(n)).unwrap();
        }
        std::fs::write(d.join("note"), "").unwrap();
        let names: Vec<String> = storage_in(&d).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["shared", "dcim", "downloads"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

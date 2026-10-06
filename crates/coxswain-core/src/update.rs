//! The built-in version, and a once-a-day check for a newer release on GitHub.

use std::path::{Path, PathBuf};
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
    // The system's certificate store, not a bundled one: a work machine behind a TLS-inspecting
    // proxy trusts its own CA, and the check would otherwise fail silently there.
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).tls_config(crate::meaning::tls()).build().into();
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

/// The command that upgrades this copy, from where it is installed. `None` means it came
/// from the releases page (or a source build), so that is where the new version is.
pub fn upgrade_hint() -> Option<&'static str> {
    if crate::tools::flatpak().is_some() {
        return Some(FLATPAK_UPDATE);
    }
    // Inside an AppImage the executable is in a temporary mount; $APPIMAGE is the file itself.
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    // A cask copies Coxswain.app into /Applications, so the path alone does not show Homebrew.
    let cask = ["/opt/homebrew", "/usr/local"].iter().any(|p| Path::new(p).join("Caskroom/coxswain-gui").is_dir());
    hint_for(&exe.to_string_lossy(), cfg!(target_os = "macos") && cask).or(SCRIPT_UPDATE)
}

/// A Flatpak updates with the others; this is the one for Coxswain alone.
pub const FLATPAK_UPDATE: &str = "flatpak update io.github.mwo_dk.Coxswain";

/// On FreeBSD, the other BSDs and illumos the install script updates both apps: it fetches the
/// latest release again. Each line uses the download tool the base system has.
pub const SCRIPT_UPDATE: Option<&str> = if cfg!(target_os = "freebsd") {
    Some("fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh")
} else if cfg!(target_os = "dragonfly") {
    Some("fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh")
} else if cfg!(any(target_os = "netbsd", target_os = "openbsd")) {
    Some("ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh")
} else if cfg!(any(target_os = "illumos", target_os = "solaris")) {
    Some("curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh")
} else {
    None
};

fn hint_for(exe: &str, mac_cask: bool) -> Option<&'static str> {
    let p = exe.replace('\\', "/").to_lowercase();
    Some(if p.contains("/caskroom/") || (mac_cask && p.contains("/applications/coxswain.app/")) {
        "brew upgrade --cask coxswain-gui"
    } else if p.contains("/cellar/") {
        "brew upgrade coxswain"
    } else if p.contains("/com.termux/files/usr/bin/") {
        "pkg upgrade coxswain"
    } else if p.contains("/.cargo/bin/") {
        "cargo install coxswain"
    } else if p.contains("/microsoft/winget/packages/mwo-dk.coxswain.terminal_") {
        // WinGet unpacks the terminal app's zip under its Packages folder. The desktop installer
        // lands where a download would, so it gets the releases page.
        "winget upgrade mwo-dk.Coxswain.Terminal"
    } else {
        return None;
    })
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

    #[test]
    fn update_hint_follows_the_install_source() {
        let h = |p| hint_for(p, false);
        assert_eq!(h("/home/linuxbrew/.linuxbrew/Cellar/coxswain/1.2.0/bin/coxswain"), Some("brew upgrade coxswain"));
        assert_eq!(h("/opt/homebrew/Cellar/coxswain/1.2.0/bin/coxswain"), Some("brew upgrade coxswain"));
        assert_eq!(h("/home/linuxbrew/.linuxbrew/Caskroom/coxswain-gui/1.2.0/Coxswain_1.2.0_amd64.AppImage"), Some("brew upgrade --cask coxswain-gui"));
        assert_eq!(hint_for("/Applications/Coxswain.app/Contents/MacOS/coxswain-gui", true), Some("brew upgrade --cask coxswain-gui"));
        assert_eq!(h("/Applications/Coxswain.app/Contents/MacOS/coxswain-gui"), None);
        assert_eq!(h("/home/me/.cargo/bin/coxswain"), Some("cargo install coxswain"));
        assert_eq!(h(r"C:\Users\me\scoop\apps\coxswain\1.2.0\coxswain.exe"), None);
        assert_eq!(h(r"C:\Users\me\AppData\Local\Microsoft\WinGet\Packages\mwo-dk.Coxswain.Terminal_Microsoft.Winget.Source_8wekyb3d8bbwe\coxswain-terminal-v1.31.0-x86_64-pc-windows-msvc\coxswain.exe"), Some("winget upgrade mwo-dk.Coxswain.Terminal"));
        assert_eq!(h(r"C:\Users\me\AppData\Local\Coxswain\coxswain-gui.exe"), None);
        assert_eq!(h(r"C:\Program Files\Coxswain\coxswain-gui.exe"), None);
        assert_eq!(h("/home/me/.local/bin/coxswain"), None);
        assert_eq!(h("/data/data/com.termux/files/usr/bin/coxswain"), Some("pkg upgrade coxswain"));
        assert_eq!(h("/data/data/com.termux/files/home/.cargo/bin/coxswain"), Some("cargo install coxswain"));
        assert_eq!(h("/home/me/Downloads/Coxswain_1.2.0_amd64.AppImage"), None);
    }
}

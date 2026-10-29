//! The built-in version, and a check for a newer release: when the app starts, then once a day
//! while it runs. It asks the channel this copy came from (WinGet, Flathub, nixpkgs, Termux),
//! or GitHub for the rest.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::state::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_URL: &str = "https://github.com/mwo-dk/coxswain/releases/latest";
const API_URL: &str = "https://api.github.com/repos/mwo-dk/coxswain/releases/latest";

/// Where this copy's update check goes, as Settings → Privacy and the first-run guide name it:
/// its channel's repository, then GitHub's releases when the channel gives no answer.
pub fn host() -> String {
    let gh = crate::settings::host(API_URL);
    match channel() {
        Channel::GitHub => gh.to_string(),
        c => format!("{} ({}), {gh}", crate::settings::host(&c.url()), c.repo()),
    }
}
const DAY: u64 = 24 * 3600;

/// `major.minor.patch` from "1.2.3" or "v1.2.3"; pre-release suffixes are ignored.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|p| p.parse().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Is `latest` newer than `current`? Unparseable versions never are.
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse(current), parse(latest)), (Some(c), Some(l)) if l > c)
}

/// `"v2.3.4"`, `"2.3.4.tar.gz"`, `"1:2.3.4"` (a Termux epoch) as `"2.3.4"`.
fn version_in(s: &str) -> Option<String> {
    let s = s.trim().trim_matches(['"', '\'', ';', ',']);
    let s = s.rsplit_once(':').map_or(s, |(_, v)| v);
    parse(s).map(|(a, b, c)| format!("{a}.{b}.{c}"))
}

/// The channel a copy came from, which is where it learns of a new version: announced only once
/// the command it shows can install it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Our releases page. It holds for the downloads, the install scripts, crates.io and our
    /// Homebrew tap (formula and cask) too: the Release run publishes the crates and writes the
    /// tap right after the GitHub release, in the same run. The Nix flake builds from our tags.
    GitHub,
    /// The package's highest version folder in microsoft/winget-pkgs: what `winget upgrade` finds.
    WinGet(&'static str),
    /// The manifest on Flathub's repository of the app.
    Flathub,
    /// The package file on nixpkgs' master: `coxswain` or `coxswain-gui`.
    Nixpkgs(&'static str),
    /// The recipe in termux-packages.
    Termux,
}

impl Channel {
    /// The name the "it can reach … later" note uses.
    pub const fn name(self) -> &'static str {
        match self {
            Channel::GitHub => "GitHub",
            Channel::WinGet(_) => "WinGet",
            Channel::Flathub => "Flathub",
            Channel::Nixpkgs(_) => "nixpkgs",
            Channel::Termux => "Termux",
        }
    }

    fn repo(self) -> &'static str {
        match self {
            Channel::GitHub => "mwo-dk/coxswain",
            Channel::WinGet(_) => "microsoft/winget-pkgs",
            Channel::Flathub => "flathub/io.github.mwo_dk.Coxswain",
            Channel::Nixpkgs(_) => "NixOS/nixpkgs",
            Channel::Termux => "termux/termux-packages",
        }
    }

    /// The one file (or folder listing) asked, all on GitHub.
    fn url(self) -> String {
        const RAW: &str = "https://raw.githubusercontent.com";
        match self {
            Channel::GitHub => API_URL.to_string(),
            // mwo-dk.Coxswain.Terminal → manifests/m/mwo-dk/Coxswain/Terminal
            Channel::WinGet(id) => format!(
                "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/{}/{}",
                id[..1].to_lowercase(),
                id.replace('.', "/")
            ),
            Channel::Flathub => format!(
                "{RAW}/flathub/io.github.mwo_dk.Coxswain/master/io.github.mwo_dk.Coxswain.yml"
            ),
            Channel::Nixpkgs(attr) => {
                format!("{RAW}/NixOS/nixpkgs/master/pkgs/by-name/co/{attr}/package.nix")
            }
            Channel::Termux => {
                format!("{RAW}/termux/termux-packages/master/packages/coxswain/build.sh")
            }
        }
    }

    /// The version in the channel's answer.
    fn read(self, body: &str) -> Option<String> {
        let line_value = |key: &str| {
            body.lines()
                .find_map(|l| l.trim().strip_prefix(key)?.trim_start().strip_prefix(['=', ':']))
                .and_then(version_in)
        };
        match self {
            Channel::GitHub => {
                let v: serde_json::Value = serde_json::from_str(body).ok()?;
                version_in(v["tag_name"].as_str()?)
            }
            // Version folders; the desktop package's folder also holds `Terminal`, no version.
            Channel::WinGet(_) => {
                let v: Vec<serde_json::Value> = serde_json::from_str(body).ok()?;
                v.iter()
                    .filter(|e| e["type"] == "dir")
                    .filter_map(|e| version_in(e["name"].as_str()?))
                    .max_by_key(|v| parse(v))
            }
            Channel::Nixpkgs(_) => line_value("version"),
            Channel::Termux => line_value("TERMUX_PKG_VERSION"),
            // Our source: an archive whose URL names the tag, or a git URL with a `tag:` below.
            Channel::Flathub => {
                let lines: Vec<&str> = body.lines().collect();
                lines.iter().enumerate().filter(|(_, l)| l.contains("mwo-dk/coxswain")).find_map(|(i, l)| {
                    l.split(['/', ' ']).filter(|p| p.starts_with('v')).find_map(version_in).or_else(|| {
                        lines[i + 1..].iter().take(4).find_map(|l| {
                            let l = l.replace('"', "");
                            version_in(l.trim().trim_start_matches('-').trim().strip_prefix("tag:")?)
                        })
                    })
                })
            }
        }
    }
}

fn get(url: &str) -> Option<String> {
    // The system's certificate store, not a bundled one: a work machine behind a TLS-inspecting
    // proxy trusts its own CA, and the check would otherwise fail silently there.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .tls_config(crate::meaning::tls())
        .build()
        .into();
    agent
        .get(url)
        .header(
            "User-Agent",
            concat!("coxswain/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()
}

/// The newest version this copy's channel offers, and the channel that said so. When the
/// channel cannot be asked (offline, not listed there yet), GitHub's answer, named as GitHub's,
/// so the notice adds that the channel may not have it yet. One request a day, two at most.
pub fn fetch_latest() -> Option<(String, &'static str)> {
    let c = channel();
    if c != Channel::GitHub
        && let Some(v) = get(&c.url()).and_then(|b| c.read(&b))
    {
        return Some((v, c.name()));
    }
    let gh = Channel::GitHub;
    Some((gh.read(&get(API_URL)?)?, gh.name()))
}

/// Has this run of the app asked yet? Each start asks once, then once a day while it runs.
static ASKED: AtomicBool = AtomicBool::new(false);

/// Is a check due: the app has just started, or a day has passed since the last one.
pub fn due(st: &AppState) -> bool {
    !ASKED.load(Ordering::Relaxed) || crate::fs::now().saturating_sub(st.update_checked) >= DAY
}

pub fn record(st: &mut AppState, (latest, from): (String, &'static str)) {
    ASKED.store(true, Ordering::Relaxed);
    st.latest_version = latest;
    st.latest_from = from.to_string();
    st.update_checked = crate::fs::now();
}

/// The newer version the last check found, if any.
pub fn available(st: &AppState) -> Option<String> {
    is_newer(VERSION, &st.latest_version).then(|| st.latest_version.clone())
}

/// How to upgrade to what the last check found: the note that the channel may not have it yet
/// only when the answer was GitHub's, not the channel's own.
pub fn hint(st: &AppState) -> Option<Upgrade> {
    upgrade_hint().map(|u| answered(u, st))
}

fn answered(mut u: Upgrade, st: &AppState) -> Upgrade {
    if st.latest_from == u.channel.name() {
        u.lags = None;
    }
    u
}

/// For callers without their own state handling (the TUI): load, check if due, save.
/// The terminal app calls it when it starts and then every hour, so a session left open for days
/// still hears.
/// Blocks for up to 5 s, so run it on a thread. Offline just means "no news".
pub fn check() -> Option<(String, Option<Upgrade>)> {
    let mut st = AppState::load();
    if due(&st) {
        if let Some(latest) = fetch_latest() {
            // Read again: the app may have saved its state during the fetch.
            st = AppState::load();
            record(&mut st, latest);
            let _ = st.save();
        }
    }
    available(&st).map(|v| (v, hint(&st)))
}

/// How this copy upgrades: the command, and the channel it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Upgrade {
    pub cmd: &'static str,
    /// The channel (WinGet, Flathub, Termux, nixpkgs) that takes a release days after GitHub,
    /// while the last answer was GitHub's (`hint`), so its command may find nothing new yet.
    pub lags: Option<&'static str>,
    /// Where the check asks.
    #[serde(skip)]
    pub channel: Channel,
}

impl Upgrade {
    const fn now(cmd: &'static str) -> Self {
        Self { cmd, lags: None, channel: Channel::GitHub }
    }
    const fn via(cmd: &'static str, channel: Channel) -> Self {
        Self { cmd, lags: Some(channel.name()), channel }
    }
}

/// Where this copy's check asks.
pub fn channel() -> Channel {
    upgrade_hint().map_or(Channel::GitHub, |u| u.channel)
}

/// What besides the path shows how a copy was installed.
#[derive(Clone, Copy, Default)]
struct Seen {
    /// Homebrew's cask folder exists, on macOS (a cask copies Coxswain.app to /Applications).
    mac_cask: bool,
    /// WinGet's own record of what it installed lists the desktop app's package.
    winget_desktop: bool,
    /// Built by our flake (`packaging/nix/*.nix` set `COXSWAIN_FROM_FLAKE`), not by nixpkgs.
    flake: bool,
}

/// The command that upgrades this copy, from where it is installed. `None` means it came
/// from the releases page (or a source build), so that is where the new version is. Worked out
/// once a run: where a copy came from does not change while it runs.
pub fn upgrade_hint() -> Option<Upgrade> {
    static HINT: std::sync::OnceLock<Option<Upgrade>> = std::sync::OnceLock::new();
    *HINT.get_or_init(|| {
        if crate::tools::flatpak().is_some() {
            return Some(Upgrade::via(FLATPAK_UPDATE, Channel::Flathub));
        }
        // Inside an AppImage the executable is in a temporary mount; $APPIMAGE is the file itself.
        let exe = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())?;
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        let exe = exe.to_string_lossy();
        let cask = ["/opt/homebrew", "/usr/local"]
            .iter()
            .any(|p| Path::new(p).join("Caskroom/coxswain-gui").is_dir());
        let seen = Seen {
            mac_cask: cfg!(target_os = "macos") && cask,
            winget_desktop: cfg!(windows)
                && is_desktop_exe(&exe)
                && winget_installed("mwo-dk.Coxswain"),
            flake: option_env!("COXSWAIN_FROM_FLAKE").is_some(),
        };
        hint_for(&exe, seen).or(SCRIPT_UPDATE.map(Upgrade::now))
    })
}

fn is_desktop_exe(exe: &str) -> bool {
    exe.replace('\\', "/").to_lowercase().ends_with("/coxswain-gui.exe")
}

/// Whether WinGet installed package `id`. The desktop app's setup.exe installs where a download
/// would, so its folder does not tell; WinGet's tracking catalog does: `installed.db` in a folder
/// per source under its local state (the Store's App Installer, or a WinGet without it), where
/// it records each package it installs and drops it when it uninstalls it. Nothing is run.
fn winget_installed(id: &str) -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return false;
    };
    [
        local.join(r"Packages\Microsoft.DesktopAppInstaller_8wekyb3d8bbwe\LocalState"),
        local.join(r"Microsoft\WinGet\State"),
    ]
    .iter()
    .filter_map(|d| std::fs::read_dir(d).ok())
    .flatten()
    .flatten()
    .any(|e| {
        use rusqlite::{Connection, OpenFlags};
        Connection::open_with_flags(e.path().join("installed.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .is_ok_and(|db| catalog_lists(&db, id))
    })
}

/// Does a WinGet catalog hold package `id`? Read by column, not by layout: every table with an
/// `id` column (`ids` in the older index schema, `packages` in the newer one).
fn catalog_lists(db: &rusqlite::Connection, id: &str) -> bool {
    let tables: Vec<String> = db
        .prepare(
            "SELECT m.name FROM sqlite_master m, pragma_table_info(m.name) c \
             WHERE m.type = 'table' AND c.name = 'id'",
        )
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    tables.iter().any(|t| {
        db.query_row(
            &format!("SELECT 1 FROM \"{}\" WHERE id = ?1 COLLATE NOCASE LIMIT 1", t.replace('"', "\"\"")),
            [id],
            |_| Ok(()),
        )
        .is_ok()
    })
}

/// A Flatpak updates with the others; this is the one for Coxswain alone.
pub const FLATPAK_UPDATE: &str = "flatpak update io.github.mwo_dk.Coxswain";

/// On FreeBSD, the other BSDs and illumos the install script updates both apps: it fetches the
/// latest release again. Each line uses the download tool the base system has.
pub const SCRIPT_UPDATE: Option<&str> = if cfg!(target_os = "freebsd") {
    Some(
        "fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh",
    )
} else if cfg!(any(target_os = "netbsd", target_os = "openbsd")) {
    Some(
        "ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh",
    )
} else if cfg!(any(target_os = "illumos", target_os = "solaris")) {
    Some(
        "curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh",
    )
} else {
    None
};

fn hint_for(exe: &str, seen: Seen) -> Option<Upgrade> {
    let p = exe.replace('\\', "/").to_lowercase();
    Some(
        if p.contains("/caskroom/") || (seen.mac_cask && p.contains("/applications/coxswain.app/")) {
            Upgrade::now("brew upgrade --cask coxswain-gui")
        } else if p.contains("/cellar/") {
            Upgrade::now("brew upgrade coxswain")
        } else if p.starts_with("/nix/store/") {
            // From the flake it has our release at once; nixpkgs' copy asks nixpkgs.
            if seen.flake {
                Upgrade::now("nix profile upgrade coxswain")
            } else if p.ends_with("/coxswain-gui") {
                Upgrade::via("nix profile upgrade coxswain-gui", Channel::Nixpkgs("coxswain-gui"))
            } else {
                Upgrade::via("nix profile upgrade coxswain", Channel::Nixpkgs("coxswain"))
            }
        } else if p.contains("/com.termux/files/usr/bin/") {
            Upgrade::via("pkg upgrade coxswain", Channel::Termux)
        } else if p.contains("/.cargo/bin/") {
            Upgrade::now("cargo install --locked coxswain")
        } else if p.contains("/microsoft/winget/packages/mwo-dk.coxswain.terminal_") {
            // WinGet unpacks the terminal app's zip under its Packages folder.
            Upgrade::via("winget upgrade mwo-dk.Coxswain.Terminal", Channel::WinGet("mwo-dk.Coxswain.Terminal"))
        } else if seen.winget_desktop && p.ends_with("/coxswain-gui.exe") {
            // The desktop installer lands where a download would; WinGet's record tells.
            Upgrade::via("winget upgrade mwo-dk.Coxswain", Channel::WinGet("mwo-dk.Coxswain"))
        } else {
            return None;
        },
    )
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

    // ASKED is process-wide, so only what holds after a record() is checked: never whether a
    // fresh run is due, which another test's record() may already have settled.
    #[test]
    fn update_due_after_asking_once_a_day() {
        let mut st = AppState::default();
        record(&mut st, ("1.0.0".into(), "GitHub"));
        assert!(!due(&st), "asked just now");
        st.update_checked = crate::fs::now() - DAY + 60;
        assert!(!due(&st), "less than a day ago");
        st.update_checked = crate::fs::now() - DAY;
        assert!(due(&st), "a day ago");
    }

    fn h(p: &str) -> Option<&'static str> {
        hint_for(p, Seen::default()).map(|u| u.cmd)
    }
    fn ch(p: &str, seen: Seen) -> Channel {
        hint_for(p, seen).map_or(Channel::GitHub, |u| u.channel)
    }
    const WINGET_TERMINAL: &str = r"C:\Users\me\AppData\Local\Microsoft\WinGet\Packages\mwo-dk.Coxswain.Terminal_Microsoft.Winget.Source_8wekyb3d8bbwe\coxswain-terminal-v1.31.0-x86_64-pc-windows-msvc\coxswain.exe";

    #[test]
    fn update_hint_follows_the_install_source() {
        assert_eq!(
            h("/home/linuxbrew/.linuxbrew/Cellar/coxswain/1.2.0/bin/coxswain"),
            Some("brew upgrade coxswain")
        );
        assert_eq!(
            h("/opt/homebrew/Cellar/coxswain/1.2.0/bin/coxswain"),
            Some("brew upgrade coxswain")
        );
        assert_eq!(
            h("/home/linuxbrew/.linuxbrew/Caskroom/coxswain-gui/1.2.0/Coxswain_1.2.0_amd64.AppImage"),
            Some("brew upgrade --cask coxswain-gui")
        );
        let mac_cask = Seen { mac_cask: true, ..Seen::default() };
        assert_eq!(
            hint_for("/Applications/Coxswain.app/Contents/MacOS/coxswain-gui", mac_cask).map(|u| u.cmd),
            Some("brew upgrade --cask coxswain-gui")
        );
        assert_eq!(h("/Applications/Coxswain.app/Contents/MacOS/coxswain-gui"), None);
        assert_eq!(h("/home/me/.cargo/bin/coxswain"), Some("cargo install --locked coxswain"));
        assert_eq!(h(r"C:\Users\me\scoop\apps\coxswain\1.2.0\coxswain.exe"), None);
        assert_eq!(h(WINGET_TERMINAL), Some("winget upgrade mwo-dk.Coxswain.Terminal"));
        assert_eq!(h(r"C:\Users\me\AppData\Local\Coxswain\coxswain-gui.exe"), None);
        assert_eq!(h(r"C:\Program Files\Coxswain\coxswain-gui.exe"), None);
        assert_eq!(h("/home/me/.local/bin/coxswain"), None);
        assert_eq!(h("/data/data/com.termux/files/usr/bin/coxswain"), Some("pkg upgrade coxswain"));
        assert_eq!(
            h("/data/data/com.termux/files/home/.cargo/bin/coxswain"),
            Some("cargo install --locked coxswain")
        );
        assert_eq!(h("/home/me/Downloads/Coxswain_1.2.0_amd64.AppImage"), None);
    }

    #[test]
    fn update_hint_knows_winget_installed_the_desktop_app() {
        let winget = Seen { winget_desktop: true, ..Seen::default() };
        for p in [
            r"C:\Users\me\AppData\Local\Coxswain\coxswain-gui.exe",
            r"C:\Program Files\Coxswain\coxswain-gui.exe",
        ] {
            assert_eq!(
                hint_for(p, winget),
                Some(Upgrade::via("winget upgrade mwo-dk.Coxswain", Channel::WinGet("mwo-dk.Coxswain"))),
                "{p}"
            );
        }
        // WinGet's record names the desktop package, never the terminal app beside it.
        assert_eq!(hint_for(r"C:\Users\me\bin\coxswain.exe", winget), None);
        assert_eq!(hint_for(WINGET_TERMINAL, winget).map(|u| u.cmd), Some("winget upgrade mwo-dk.Coxswain.Terminal"));
        assert!(is_desktop_exe(r"C:\Users\me\AppData\Local\Coxswain\Coxswain-GUI.exe"));
        assert!(!is_desktop_exe(r"C:\Users\me\AppData\Local\Coxswain\coxswain.exe"));
    }

    #[test]
    fn update_winget_catalog_lists_by_id() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        // The older index schema: ids by text, the manifest table's `id` a number.
        db.execute_batch(
            "CREATE TABLE ids(rowid INTEGER PRIMARY KEY, id TEXT NOT NULL);
             CREATE TABLE manifest(rowid INTEGER PRIMARY KEY, id INT64, version INT64);
             INSERT INTO ids(id) VALUES ('mwo-dk.Coxswain.Terminal');
             INSERT INTO manifest(id, version) VALUES (1, 1);",
        )
        .unwrap();
        assert!(!catalog_lists(&db, "mwo-dk.Coxswain"), "only the terminal app");
        assert!(catalog_lists(&db, "mwo-dk.Coxswain.Terminal"));
        db.execute("INSERT INTO ids(id) VALUES ('MWO-DK.Coxswain')", []).unwrap();
        assert!(catalog_lists(&db, "mwo-dk.Coxswain"), "WinGet ids ignore case");
        // The newer schema keeps packages in a table of their own.
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE packages(rowid INTEGER PRIMARY KEY, id TEXT, name TEXT); INSERT INTO packages(id, name) VALUES ('mwo-dk.Coxswain', 'Coxswain');").unwrap();
        assert!(catalog_lists(&db, "mwo-dk.Coxswain"));
        assert!(!catalog_lists(&rusqlite::Connection::open_in_memory().unwrap(), "mwo-dk.Coxswain"));
    }

    #[test]
    fn update_hint_knows_nix_and_which_channels_lag() {
        assert_eq!(
            hint_for("/nix/store/abc123-coxswain-2.31.0/bin/coxswain", Seen::default()),
            Some(Upgrade::via("nix profile upgrade coxswain", Channel::Nixpkgs("coxswain")))
        );
        assert_eq!(
            ch("/nix/store/abc123-coxswain-gui-2.31.0/bin/coxswain-gui", Seen::default()),
            Channel::Nixpkgs("coxswain-gui")
        );
        // Built by our flake: our tags, so GitHub, with nothing to wait for.
        let flake = Seen { flake: true, ..Seen::default() };
        assert_eq!(
            hint_for("/nix/store/abc123-coxswain-2.31.0/bin/coxswain", flake),
            Some(Upgrade::now("nix profile upgrade coxswain"))
        );
        // Only the store itself: a folder named nix elsewhere is no Nix install.
        assert_eq!(h("/home/me/nix/store/bin/coxswain"), None);
        let lags = |p| hint_for(p, Seen::default()).and_then(|u| u.lags);
        assert_eq!(lags("/data/data/com.termux/files/usr/bin/coxswain"), Some("Termux"));
        assert_eq!(lags(WINGET_TERMINAL), Some("WinGet"));
        // Our own tap, crates.io and the install scripts have a release as soon as GitHub does.
        for p in [
            "/opt/homebrew/Cellar/coxswain/1.2.0/bin/coxswain",
            "/home/linuxbrew/.linuxbrew/Caskroom/coxswain-gui/1.2.0/Coxswain.AppImage",
            "/home/me/.cargo/bin/coxswain",
        ] {
            assert!(hint_for(p, Seen::default()).is_some_and(|u| u.lags.is_none() && u.channel == Channel::GitHub), "{p}");
        }
    }

    #[test]
    fn update_each_channel_asks_its_own_source() {
        let s = Seen::default();
        assert_eq!(ch(WINGET_TERMINAL, s), Channel::WinGet("mwo-dk.Coxswain.Terminal"));
        assert_eq!(
            Channel::WinGet("mwo-dk.Coxswain.Terminal").url(),
            "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/m/mwo-dk/Coxswain/Terminal"
        );
        assert_eq!(
            Channel::WinGet("mwo-dk.Coxswain").url(),
            "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/m/mwo-dk/Coxswain"
        );
        assert_eq!(ch("/data/data/com.termux/files/usr/bin/coxswain", s), Channel::Termux);
        assert!(Channel::Termux.url().ends_with("/termux/termux-packages/master/packages/coxswain/build.sh"));
        assert!(Channel::Nixpkgs("coxswain").url().ends_with("/NixOS/nixpkgs/master/pkgs/by-name/co/coxswain/package.nix"));
        assert!(Channel::Flathub.url().starts_with("https://raw.githubusercontent.com/flathub/io.github.mwo_dk.Coxswain/"));
        // Every channel is on GitHub: no new server.
        for c in [Channel::GitHub, Channel::WinGet("mwo-dk.Coxswain"), Channel::Flathub, Channel::Nixpkgs("coxswain"), Channel::Termux] {
            let host = crate::settings::host(&c.url()).to_string();
            assert!(host == "api.github.com" || host == "raw.githubusercontent.com", "{c:?}");
        }
        for p in [
            "/home/me/.cargo/bin/coxswain",
            "/opt/homebrew/Cellar/coxswain/1.2.0/bin/coxswain",
            "/opt/homebrew/Caskroom/coxswain-gui/1.2.0/Coxswain.app/Contents/MacOS/coxswain-gui",
            r"C:\Users\me\AppData\Local\Coxswain\coxswain-gui.exe",
            "/usr/bin/coxswain",
        ] {
            assert_eq!(ch(p, s), Channel::GitHub, "{p}");
        }
    }

    // Answers as each channel gives them, cut down; no network.
    #[test]
    fn update_reads_each_channels_answer() {
        assert_eq!(
            Channel::GitHub.read(r#"{"url":"https://api.github.com/repos/mwo-dk/coxswain/releases/1","tag_name":"v2.34.0","name":"Coxswain 2.34.0","draft":false}"#),
            Some("2.34.0".into())
        );
        assert_eq!(Channel::GitHub.read(r#"{"message":"Not Found"}"#), None);
        // The contents API on a package folder; "2.9.0" sorts after "2.33.1" as text.
        let listing = |names: &[&str]| {
            serde_json::to_string(
                &names
                    .iter()
                    .map(|n| serde_json::json!({"name": n, "path": format!("manifests/m/mwo-dk/Coxswain/{n}"), "size": 0, "type": "dir", "download_url": null}))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let w = Channel::WinGet("mwo-dk.Coxswain.Terminal");
        assert_eq!(w.read(&listing(&["1.29.1", "2.33.1", "2.8.2", "2.9.0"])), Some("2.33.1".into()));
        // The desktop package's folder before its first version is listed: only Terminal.
        assert_eq!(Channel::WinGet("mwo-dk.Coxswain").read(&listing(&["Terminal"])), None);
        assert_eq!(Channel::WinGet("mwo-dk.Coxswain").read(&listing(&["Terminal", "2.35.0"])), Some("2.35.0".into()));
        assert_eq!(w.read(r#"{"message":"Not Found","status":"404"}"#), None);

        let nix = r#"{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "coxswain";
  version = "2.8.1";

  src = fetchFromGitHub {
    owner = "mwo-dk";
    repo = "coxswain";
    tag = "v${finalAttrs.version}";
"#;
        assert_eq!(Channel::Nixpkgs("coxswain").read(nix), Some("2.8.1".into()));

        let termux = r#"TERMUX_PKG_HOMEPAGE=https://github.com/mwo-dk/coxswain
TERMUX_PKG_DESCRIPTION="Two-panel file manager for the terminal, Norton Commander style"
TERMUX_PKG_LICENSE="MIT"
TERMUX_PKG_MAINTAINER="@termux"
TERMUX_PKG_VERSION="2.8.1"
TERMUX_PKG_SRCURL="https://github.com/mwo-dk/coxswain/archive/refs/tags/v${TERMUX_PKG_VERSION}.tar.gz"
"#;
        assert_eq!(Channel::Termux.read(termux), Some("2.8.1".into()));
        assert_eq!(Channel::Termux.read("TERMUX_PKG_VERSION=1:2.10.3\n"), Some("2.10.3".into()));

        let flathub_git = r#"id: io.github.mwo_dk.Coxswain
modules:
  - name: git
    sources:
      - type: archive
        url: https://www.kernel.org/pub/software/scm/git/git-2.56.0.tar.xz
  - name: coxswain
    sources:
      - type: git
        url: https://github.com/mwo-dk/coxswain.git
        tag: v2.36.0
        commit: 0123456789abcdef
"#;
        assert_eq!(Channel::Flathub.read(flathub_git), Some("2.36.0".into()));
        let flathub_archive = "      - type: archive\n        url: https://github.com/mwo-dk/coxswain/archive/refs/tags/v2.36.1.tar.gz\n";
        assert_eq!(Channel::Flathub.read(flathub_archive), Some("2.36.1".into()));
        assert_eq!(Channel::Flathub.read("id: io.github.mwo_dk.Coxswain\n"), None);
    }

    #[test]
    fn update_lags_only_when_github_answered_for_the_channel() {
        let mut st = AppState::default();
        record(&mut st, ("9.0.0".into(), "GitHub"));
        assert_eq!(st.latest_from, "GitHub");
        let u = Upgrade::via("pkg upgrade coxswain", Channel::Termux);
        let shown = |st: &AppState| answered(u, st).lags;
        assert_eq!(shown(&st), Some("Termux"));
        record(&mut st, ("9.0.0".into(), "Termux"));
        assert_eq!(shown(&st), None);
    }
}

//! "Start with my session": the search helper registered with the system, so it runs from
//! login instead of from the first app, and the backlog is read before an app is opened. A
//! systemd user unit on Linux, a LaunchAgent on macOS, a Run entry in the registry on Windows,
//! and an XDG autostart entry on FreeBSD and the other BSDs (the desktop session starts it) and
//! in a Flatpak, which has no systemd to ask.
//! Registered helpers run with `STAY`: they do not leave when the last app has gone.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The argument, after `helper::ARG`, of a helper that stays.
pub const STAY: &str = "--stay";

const NAME: &str = "coxswain-index";
/// The LaunchAgent's label.
const LABEL: &str = "dk.mwo.coxswain.index";

/// Started by the desktop session from an XDG autostart entry, which nothing restarts: the
/// BSDs, which have no per-user service manager, and a Flatpak, which cannot reach systemd.
fn autostart() -> bool {
    cfg!(not(any(windows, target_os = "linux", target_os = "macos"))) || crate::tools::flatpak().is_some()
}

/// Whether a service manager (systemd, launchd) starts the registered helper, so an app asks it
/// to before starting one of its own. Not in a Flatpak, whose entry is an autostart one.
pub fn supervised() -> bool {
    !autostart() && cfg!(any(target_os = "linux", target_os = "macos"))
}

/// Run `c`; an error with its output when it fails.
fn run(c: &mut Command) -> io::Result<()> {
    let out = c.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{:?}: {}", c.get_program(), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// launchd's name for this user's login session: `gui/501`.
fn domain() -> String {
    #[cfg(unix)]
    // Safety: getuid has no preconditions and cannot fail.
    return format!("gui/{}", unsafe { libc::getuid() });
    #[cfg(not(unix))]
    String::new()
}

/// The helper in it: `gui/501/dk.mwo.coxswain.index`.
fn service() -> String {
    format!("{}/{LABEL}", domain())
}

fn launchctl(args: &[&str]) -> io::Result<()> {
    run(crate::tools::command("launchctl").args(args))
}

fn systemctl(args: &[&str]) -> io::Result<()> {
    run(crate::tools::command("systemctl").arg("--user").args(args))
}

/// Hand the LaunchAgent at `file` to launchd, which starts it (`RunAtLoad`). One loaded before
/// goes first, and one switched off by an older version's `unload -w` is switched on again.
fn bootstrap(file: &Path) -> io::Result<()> {
    let _ = launchctl(&["enable", &service()]);
    let _ = launchctl(&["bootout", &service()]);
    // launchd may still be letting the old one go.
    let mut tries = 0;
    loop {
        match launchctl(&["bootstrap", &domain(), &file.to_string_lossy()]) {
            Err(_) if tries < 3 => tries += 1,
            done => return done,
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Where the registration lives, on the platforms that keep it in a file.
fn file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(std::env::home_dir()?.join("Library/LaunchAgents/dk.mwo.coxswain.index.plist"))
    } else if cfg!(windows) {
        None
    } else if crate::tools::flatpak().is_some() {
        // The host's own autostart folder: the Flatpak's config folder is not the session's.
        Some(std::env::home_dir()?.join(".config/autostart").join(format!("{NAME}.desktop")))
    } else if autostart() {
        Some(dirs::config_dir()?.join("autostart").join(format!("{NAME}.desktop")))
    } else {
        Some(dirs::config_dir()?.join("systemd/user").join(format!("{NAME}.service")))
    }
}

/// The file that registers `exe` as the helper; in the Flatpak `flatpak`, that app.
fn unit(exe: &Path, flatpak: Option<&str>) -> String {
    let exe = exe.display();
    if let Some(id) = flatpak {
        format!(
            "# Written by Coxswain (Settings → Finding files → Background reading → Start with my session).\n\
             [Desktop Entry]\nType=Application\nName=Coxswain file index\nExec=flatpak run --command=coxswain-gui {id} {} {STAY}\nNoDisplay=true\nTerminal=false\n",
            crate::helper::ARG
        )
    } else if cfg!(target_os = "macos") {
        // No `ProcessType` Background: macOS keeps such a job on the efficiency cores with its
        // disk reads throttled, and the backlog barely moves. `Nice` 10 lets the user go first.
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>AssociatedBundleIdentifiers</key><string>dk.mwo.coxswain</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>{}</string><string>{STAY}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>StandardErrorPath</key><string>{log}</string>
  <key>ThrottleInterval</key><integer>2</integer>
  <key>Nice</key><integer>10</integer>
</dict>
</plist>
"#,
            crate::helper::ARG,
            log = crate::helper::log().unwrap_or_default().display(),
        )
    } else if autostart() {
        format!(
            "# Written by Coxswain (Settings → Finding files → Background reading → Start with my session).\n\
             [Desktop Entry]\nType=Application\nName=Coxswain file index\nExec=\"{exe}\" {} {STAY}\nNoDisplay=true\nTerminal=false\n",
            crate::helper::ARG
        )
    } else {
        format!(
            "# Written by Coxswain (Settings → Finding files → Background reading → Start with my session).\n\
             [Unit]\nDescription=Coxswain's file index\nStartLimitIntervalSec=0\n\n\
             [Service]\nExecStart=\"{exe}\" {} {STAY}\nRestart=on-failure\nRestartSec=1\nNice=10\nIOSchedulingClass=idle\n\n\
             [Install]\nWantedBy=default.target\n",
            crate::helper::ARG
        )
    }
}

/// Whether the helper is registered.
pub fn installed() -> bool {
    if cfg!(windows) {
        crate::tools::command("reg").args(["query", RUN_KEY, "/v", NAME]).output().is_ok_and(|o| o.status.success())
    } else {
        file().is_some_and(|f| f.exists())
    }
}

/// The program the registration starts, as written in it.
fn registered() -> Option<PathBuf> {
    let text = if cfg!(windows) {
        String::from_utf8(crate::tools::command("reg").args(["query", RUN_KEY, "/v", NAME]).output().ok()?.stdout).ok()?
    } else {
        std::fs::read_to_string(file()?).ok()?
    };
    exe_in(&text)
}

/// The program in a registration's text: the systemd unit's `ExecStart`, the first of the
/// LaunchAgent's arguments, the quoted start of the Run entry or of the autostart `Exec`.
fn exe_in(text: &str) -> Option<PathBuf> {
    let (open, close) = if cfg!(target_os = "macos") {
        ("<array><string>", "</string>")
    } else if cfg!(windows) {
        ("\"", "\"")
    } else if autostart() {
        ("Exec=\"", "\"")
    } else {
        ("ExecStart=\"", "\"")
    };
    Some(PathBuf::from(text.split_once(open)?.1.split_once(close)?.0))
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(not(windows))]
const RUN_KEY: &str = "";

/// The name on PATH that leads to `exe`, if there is one: a package manager's link (Homebrew's
/// `bin/coxswain-gui`) stays when an upgrade replaces the versioned file it points to, so the
/// registration starts the new version.
fn stable(exe: &Path) -> PathBuf {
    let real = std::fs::canonicalize(exe).ok();
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .flat_map(|dir| ["coxswain-gui", "coxswain"].map(|n| dir.join(n)))
        .find(|p| p.as_path() != exe && real.is_some() && std::fs::canonicalize(p).ok() == real)
        .unwrap_or_else(|| exe.to_path_buf())
}

/// Whether the registration starts `exe`, by that name or by a link to it.
pub fn starts(exe: &Path) -> bool {
    // A Flatpak's entry starts the app by its id, whichever version is installed.
    if crate::tools::flatpak().is_some() {
        return installed();
    }
    registered().is_some_and(|r| r == exe || std::fs::canonicalize(&r).ok().is_some_and(|r| Some(r) == std::fs::canonicalize(exe).ok()))
}

/// Write again a registration an older version wrote, for the program it names: before 2.1.2
/// the LaunchAgent ran the helper as a background job, which macOS throttles until the backlog
/// barely moves, and systemd and launchd started again a helper that had left on purpose.
pub fn refresh() {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    if let Some(exe) = exe_in(&text).filter(|e| e.exists() && text != unit(e, crate::tools::flatpak())) {
        let _ = install(&exe);
    }
}

/// Ask systemd or launchd to start the registered helper now. On a Mac one that launchd does
/// not know (its loading failed, or it was taken out) is handed to it again.
pub fn kick() {
    if cfg!(target_os = "macos") {
        if launchctl(&["kickstart", &service()]).is_err() {
            let _ = file().map(|f| bootstrap(&f));
        }
    } else if supervised() {
        let _ = systemctl(&["start", NAME]);
    }
}

/// Register `exe` (this app) as the helper and start it.
pub fn install(exe: &Path) -> io::Result<()> {
    let exe = &stable(exe);
    if cfg!(windows) {
        let line = format!("\"{}\" {} {STAY}", exe.display(), crate::helper::ARG);
        run(crate::tools::command("reg").args(["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &line, "/f"]))?;
        // Now as well, not only from the next login.
        return crate::helper::detached(exe).arg(STAY).spawn().map(drop);
    }
    let file = file().ok_or_else(|| io::Error::other("no home folder"))?;
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&file, unit(exe, crate::tools::flatpak()))?;
    if autostart() {
        // The session starts it from the next login; now, this app does.
        crate::helper::detached(exe).arg(STAY).spawn().map(drop)
    } else if cfg!(target_os = "macos") {
        // Its errors go to helper.log, whose folder launchd does not make.
        if let Some(dir) = crate::helper::folder() {
            crate::fs::private(&dir, None)?;
        }
        bootstrap(&file)
    } else {
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "--now", NAME])
    }
}

/// Unregister the helper. The one running goes too; the next app starts its own.
pub fn uninstall() -> io::Result<()> {
    if cfg!(windows) {
        return run(crate::tools::command("reg").args(["delete", RUN_KEY, "/v", NAME, "/f"]));
    }
    let Some(file) = file().filter(|f| f.exists()) else { return Ok(()) };
    // An autostart entry has nothing to stop through: the helper running now stays until logout.
    if cfg!(target_os = "macos") {
        let _ = launchctl(&["bootout", &service()]);
    } else if supervised() {
        let _ = systemctl(&["disable", "--now", NAME]);
    }
    std::fs::remove_file(&file)?;
    if supervised() && cfg!(target_os = "linux") {
        let _ = systemctl(&["daemon-reload"]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_unit_runs_this_app_as_a_helper_that_stays() {
        let exe = Path::new("/opt/Cox Swain/coxswain-gui");
        let text = unit(exe, None);
        assert!(text.contains("/opt/Cox Swain/coxswain-gui"));
        assert!(text.contains(crate::helper::ARG) && text.contains(STAY));
        if cfg!(target_os = "linux") {
            assert!(text.contains("ExecStart=\"/opt/Cox Swain/coxswain-gui\" --index-helper --stay"));
            // A helper that left on purpose (another runs, or a restart) is not started again and again.
            assert!(text.contains("Restart=on-failure"));
            assert!(file().unwrap().ends_with("systemd/user/coxswain-index.service"));
        }
        if cfg!(target_os = "freebsd") {
            assert!(text.contains("Exec=\"/opt/Cox Swain/coxswain-gui\" --index-helper --stay"));
            assert!(file().unwrap().ends_with("autostart/coxswain-index.desktop"));
        }
        if !cfg!(windows) {
            assert_eq!(exe_in(&text).as_deref(), Some(exe));
        }
        if cfg!(target_os = "macos") {
            assert!(text.contains("<array><string>/opt/Cox Swain/coxswain-gui</string><string>--index-helper</string><string>--stay</string></array>"));
            assert!(text.contains("<key>SuccessfulExit</key><false/>"));
            assert!(!text.contains("Background") && !text.contains("LowPriorityIO"));
            assert!(text.contains(&format!("<key>StandardErrorPath</key><string>{}</string>", crate::helper::log().unwrap().display())));
            assert!(service().starts_with("gui/") && service().ends_with("/dk.mwo.coxswain.index") && domain() != "gui/0");
            // macOS reads it. By its full path: a build with a PATH of its own (Nix) has no
            // /usr/bin on it.
            let f = std::env::temp_dir().join(format!("coxswain-plist-{}.plist", std::process::id()));
            std::fs::write(&f, &text).unwrap();
            let lint = std::process::Command::new("/usr/bin/plutil").arg("-lint").arg(&f).output();
            let _ = std::fs::remove_file(&f);
            if let Ok(lint) = lint {
                assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
            }
        }
    }

    #[test]
    fn service_in_a_flatpak_starts_the_app_by_its_id() {
        let text = unit(Path::new("/app/bin/coxswain-gui"), Some("io.github.mwo_dk.Coxswain"));
        assert!(text.contains("Exec=flatpak run --command=coxswain-gui io.github.mwo_dk.Coxswain --index-helper --stay"), "{text}");
    }
}

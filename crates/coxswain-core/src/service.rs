//! "Start with my session": the search helper registered with the system, so it runs from
//! login instead of from the first app, and the backlog is read before an app is opened. A
//! systemd user unit on Linux, a LaunchAgent on macOS, a Run entry in the registry on Windows.
//! Registered helpers run with `STAY`: they do not leave when the last app has gone.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The argument, after `helper::ARG`, of a helper that stays.
pub const STAY: &str = "--stay";

const NAME: &str = "coxswain-index";

/// Run `c`; an error with its output when it fails.
fn run(c: &mut Command) -> io::Result<()> {
    let out = c.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{:?}: {}", c.get_program(), String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// Where the registration lives, on the platforms that keep it in a file.
fn file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(std::env::home_dir()?.join("Library/LaunchAgents/dk.mwo.coxswain.index.plist"))
    } else if cfg!(windows) {
        None
    } else {
        Some(dirs::config_dir()?.join("systemd/user").join(format!("{NAME}.service")))
    }
}

/// The file that registers `exe` as the helper.
fn unit(exe: &Path) -> String {
    let exe = exe.display();
    if cfg!(target_os = "macos") {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dk.mwo.coxswain.index</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>{}</string><string>{STAY}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>2</integer>
  <key>ProcessType</key><string>Background</string>
  <key>LowPriorityIO</key><true/>
  <key>Nice</key><integer>10</integer>
</dict>
</plist>
"#,
            crate::helper::ARG
        )
    } else {
        format!(
            "# Written by Coxswain (Settings → Search inside files → Start with my session).\n\
             [Unit]\nDescription=Coxswain's file index\nStartLimitIntervalSec=0\n\n\
             [Service]\nExecStart=\"{exe}\" {} {STAY}\nRestart=always\nRestartSec=1\nNice=10\nIOSchedulingClass=idle\n\n\
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

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(not(windows))]
const RUN_KEY: &str = "";

/// Register `exe` (this app) as the helper and start it.
pub fn install(exe: &Path) -> io::Result<()> {
    if cfg!(windows) {
        let line = format!("\"{}\" {} {STAY}", exe.display(), crate::helper::ARG);
        run(crate::tools::command("reg").args(["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &line, "/f"]))?;
        // Now as well, not only from the next login.
        return crate::helper::detached(exe).arg(STAY).spawn().map(drop);
    }
    let file = file().ok_or_else(|| io::Error::other("no home folder"))?;
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&file, unit(exe))?;
    if cfg!(target_os = "macos") {
        run(crate::tools::command("launchctl").arg("load").arg("-w").arg(&file))
    } else {
        run(crate::tools::command("systemctl").args(["--user", "daemon-reload"]))?;
        run(crate::tools::command("systemctl").args(["--user", "enable", "--now", NAME]))
    }
}

/// Unregister the helper. The one running goes too; the next app starts its own.
pub fn uninstall() -> io::Result<()> {
    if cfg!(windows) {
        return run(crate::tools::command("reg").args(["delete", RUN_KEY, "/v", NAME, "/f"]));
    }
    let Some(file) = file().filter(|f| f.exists()) else { return Ok(()) };
    if cfg!(target_os = "macos") {
        let _ = run(crate::tools::command("launchctl").arg("unload").arg("-w").arg(&file));
    } else {
        let _ = run(crate::tools::command("systemctl").args(["--user", "disable", "--now", NAME]));
    }
    std::fs::remove_file(&file)?;
    if !cfg!(target_os = "macos") {
        let _ = run(crate::tools::command("systemctl").args(["--user", "daemon-reload"]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_unit_runs_this_app_as_a_helper_that_stays() {
        let text = unit(Path::new("/opt/Cox Swain/coxswain-gui"));
        assert!(text.contains("/opt/Cox Swain/coxswain-gui"));
        assert!(text.contains(crate::helper::ARG) && text.contains(STAY));
        if cfg!(target_os = "linux") {
            assert!(text.contains("ExecStart=\"/opt/Cox Swain/coxswain-gui\" --index-helper --stay"));
            assert!(file().unwrap().ends_with("systemd/user/coxswain-index.service"));
        }
    }
}

//! Programs installed on the machine that Coxswain can use: found the way `which` finds them
//! (plus the folders their installers use off PATH), and run with a time limit.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Where installers put these outside PATH.
fn extra_paths(program: &str) -> Vec<PathBuf> {
    let v: &[&str] = match program {
        "soffice" if cfg!(target_os = "macos") => &["/Applications/LibreOffice.app/Contents/MacOS/soffice"],
        "soffice" if cfg!(windows) => &[r"C:\Program Files\LibreOffice\program\soffice.exe"],
        "tesseract" if cfg!(windows) => &[r"C:\Program Files\Tesseract-OCR\tesseract.exe"],
        _ => &[],
    };
    v.iter().map(PathBuf::from).collect()
}

/// A program on PATH (or in its usual install folder), like `which`.
pub fn which(program: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd"), format!("{program}.bat")] } else { vec![program.to_string()] };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .chain(extra_paths(program))
        .find(|p| p.is_file())
}

/// Video and sound in the desktop app's preview, on Linux: WebKit plays them with GStreamer,
/// and without the `autodetect` plugin (gst-plugins-good) the page's process aborts as soon as
/// a `<video>` or `<audio>` is shown. `None` when the plugins it needs are where GStreamer
/// looks; else the reason, said in the preview instead of playing.
pub fn media_missing() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let vars = |names: &[&str]| -> Option<Vec<PathBuf>> {
        let v: Vec<PathBuf> = names.iter().filter_map(std::env::var_os).flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>()).collect();
        (!v.is_empty()).then_some(v)
    };
    let appimage = std::env::var_os("APPDIR").is_some();
    // The system folder, unless the environment names it: an AppImage brings a GStreamer built
    // on Ubuntu, which looks only where Ubuntu keeps the plugins.
    // ponytail: the usual folders of the distributions, not the one compiled into libgstreamer;
    // asking GStreamer itself means linking it.
    let ubuntu = PathBuf::from(format!("/usr/lib/{}-linux-gnu/gstreamer-1.0", std::env::consts::ARCH));
    let system = vars(&["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH"]).unwrap_or_else(|| {
        if appimage {
            vec![ubuntu.clone()]
        } else {
            ["/usr/lib/gstreamer-1.0", "/usr/lib64/gstreamer-1.0", "/usr/local/lib/gstreamer-1.0", "/home/linuxbrew/.linuxbrew/lib/gstreamer-1.0"].into_iter().map(PathBuf::from).chain([ubuntu.clone()]).collect()
        }
    });
    let local = dirs::data_dir().map(|d| d.join("gstreamer-1.0/plugins"));
    let dirs: Vec<PathBuf> = vars(&["GST_PLUGIN_PATH_1_0", "GST_PLUGIN_PATH"]).unwrap_or_default().into_iter().chain(system).chain(local).collect();
    let has = |plugin: &str| dirs.iter().any(|d| d.join(format!("libgst{plugin}.so")).is_file());
    if has("autodetect") && has("playback") {
        return None;
    }
    Some(if appimage { crate::t!("preview.media_appimage") } else { crate::t!("preview.media_missing") })
}

/// This app's program: the AppImage file when it runs from one (the binary itself is inside a
/// mount that goes when the app closes), else the running binary.
pub fn this_app() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) => Ok(PathBuf::from(image)),
        None => std::env::current_exe(),
    }
}

/// A command for a program other than Coxswain. In an AppImage, Coxswain's environment points
/// at the libraries and GTK files packed inside it; other programs must not load those
/// (LibreOffice stops with a symbol lookup error in the packed libcurl's companions), so every
/// setting that names the AppImage's folder loses those entries, and what the AppImage set
/// for its own window goes.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(program);
    if let Some(appdir) = std::env::var_os("APPDIR") {
        outside(&mut c, Path::new(&appdir), std::env::vars_os());
    }
    c
}

/// `c` without what an AppImage unpacked at `appdir` put in the environment `vars`.
fn outside(c: &mut Command, appdir: &Path, vars: impl Iterator<Item = (OsString, OsString)>) {
    for (key, value) in vars {
        if !value.to_string_lossy().contains(&*appdir.to_string_lossy()) {
            continue;
        }
        let kept: Vec<PathBuf> = std::env::split_paths(&value).filter(|p| !p.starts_with(appdir) && !p.as_os_str().is_empty()).collect();
        match std::env::join_paths(&kept) {
            Ok(v) if !kept.is_empty() => c.env(&key, v),
            _ => c.env_remove(&key),
        };
    }
    for key in ["APPDIR", "APPIMAGE", "ARGV0", "GDK_BACKEND", "GTK_THEME"] {
        c.env_remove(key);
    }
}

/// Start `c` detached, with no input or output, but watch it for `grace`: a program that
/// stops at once with an error (not found, no application for the file) reports it, where a
/// plain spawn would say nothing. One that is still running is left to itself, and reaped
/// when it ends.
pub fn spawn_watched(mut c: Command, grace: Duration) -> std::io::Result<()> {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    let start = Instant::now();
    while start.elapsed() < grace {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            let err = err.trim();
            return Err(std::io::Error::other(if err.is_empty() { status.to_string() } else { err.to_string() }));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Its errors go nowhere from here (a closed pipe would stop it), and it is reaped.
    std::thread::spawn(move || {
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::copy(&mut e, &mut std::io::sink());
        }
        let _ = child.wait();
    });
    Ok(())
}

/// Wait for `child`, killing it when it runs past `timeout`.
pub fn wait(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "took too long"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `program` with `args` at the lowest priority, no input, its output kept up to `max` bytes;
/// `None` when it fails or runs past `timeout`.
pub fn output(program: &std::path::Path, args: &[&std::ffi::OsStr], timeout: Duration, max: u64) -> Option<Vec<u8>> {
    let mut c = low(program);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = c.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = (&mut out).take(max).read_to_end(&mut v);
        // Whatever is past the limit is drained, so the program is not stuck writing it.
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        v
    });
    let status = wait(&mut child, timeout).ok()?;
    let v = reader.join().ok()?;
    status.success().then_some(v)
}

/// `program` at the lowest priority the platform offers.
pub fn low(program: &std::path::Path) -> Command {
    match which("nice").filter(|_| cfg!(unix)) {
        Some(nice) => {
            let mut c = command(nice);
            c.args(["-n", "19"]).arg(program);
            c
        }
        None => {
            #[cfg_attr(not(windows), allow(unused_mut))]
            let mut c = command(program);
            #[cfg(windows)]
            // IDLE_PRIORITY_CLASS | CREATE_NO_WINDOW
            std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0000_0040 | 0x0800_0000);
            c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn tools_spawn_watched_reports_a_quick_failure() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo nope >&2; exit 3"]);
        let err = spawn_watched(c, Duration::from_secs(2)).unwrap_err();
        assert_eq!(err.to_string(), "nope");
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 2"]);
        assert!(spawn_watched(c, Duration::from_millis(100)).is_ok());
    }

    #[test]
    fn tools_run_with_a_limit() {
        assert_eq!(which("no-such-program-coxswain"), None);
        assert!(this_app().is_ok());
        if cfg!(unix) {
            let sh = which("sh").unwrap();
            let args = |s: &str| [std::ffi::OsStr::new("-c"), std::ffi::OsStr::new(s)].map(|a| a.to_owned());
            let run = |s: &str, t: u64, max: u64| output(&sh, &args(s).iter().map(|a| a.as_os_str()).collect::<Vec<_>>(), Duration::from_millis(t), max);
            assert_eq!(run("echo hello", 5000, 100), Some(b"hello\n".to_vec()));
            assert_eq!(run("yes | head -c 100000", 5000, 10).map(|v| v.len()), Some(10), "cut at the limit, the rest drained");
            assert_eq!(run("exit 3", 5000, 100), None);
            let start = Instant::now();
            assert_eq!(run("sleep 10", 200, 100), None);
            assert!(start.elapsed() < Duration::from_secs(5), "stopped at the time limit");
        }
    }

    /// AppImages are Linux's, with `:` between the folders of a path list.
    #[cfg(unix)]
    #[test]
    fn tools_start_programs_without_the_appimage_inside() {
        let mut c = Command::new("soffice");
        let vars = [
            ("LD_LIBRARY_PATH", "/tmp/.mount_Cox/usr/lib/:/tmp/.mount_Cox/usr/lib64/:/opt/lib"),
            ("GTK_PATH", "/tmp/.mount_Cox//usr/lib/gtk-3.0"),
            ("XDG_DATA_DIRS", "/tmp/.mount_Cox/usr/share:/usr/share"),
            ("HOME", "/home/me"),
        ];
        outside(&mut c, Path::new("/tmp/.mount_Cox"), vars.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
        let env: Vec<(String, Option<String>)> = c.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(get("LD_LIBRARY_PATH"), Some(Some("/opt/lib".into())), "only the AppImage's own folders go");
        assert_eq!(get("XDG_DATA_DIRS"), Some(Some("/usr/share".into())));
        assert_eq!(get("GTK_PATH"), Some(None), "all of it the AppImage's: gone");
        assert_eq!((get("HOME"), get("GDK_BACKEND")), (None, Some(None)), "the rest as it is; what the AppImage set for its window goes");
    }
}

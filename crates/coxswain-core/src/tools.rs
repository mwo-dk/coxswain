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

/// Video and sound in the desktop app's preview, on Linux and the BSDs: WebKit plays them with GStreamer,
/// and without the `autodetect` plugin (gst-plugins-good) the page's process aborts as soon as
/// a `<video>` or `<audio>` is shown. `None` when the plugins it needs are where GStreamer
/// looks; else the reason, said in the preview instead of playing.
pub fn media_missing() -> Option<String> {
    if cfg!(any(windows, target_os = "macos")) {
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
    Some(if appimage {
        crate::t!("preview.media_appimage")
    } else if install("gst-good").is_some() {
        crate::t!("preview.media_missing_cmd", "cmd" => install("gst-good").unwrap_or_default())
    } else {
        crate::t!("preview.media_missing")
    })
}

/// Japanese or Korean in use on Linux or a BSD, and fontconfig has no font for it: the desktop app would
/// show its letters as boxes. False elsewhere, or when `fc-list` cannot be asked.
pub fn cjk_font_missing() -> bool {
    let lang = crate::i18n::language();
    if cfg!(any(windows, target_os = "macos")) || !matches!(lang, "ja" | "ko") {
        return false;
    }
    let out = std::process::Command::new("fc-list").args([format!(":lang={lang}"), "family".into()]).output();
    out.is_ok_and(|o| o.status.success() && o.stdout.trim_ascii().is_empty())
}

/// The package managers whose install line Coxswain can write, as this system has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Pkg,
    Pkgin,
    PkgAdd,
    Ips,
    Pacman,
    Apt,
    Dnf,
    Zypper,
    Brew,
    Winget,
}

impl Manager {
    /// This system's: pkg on FreeBSD and DragonFly, pkgin on NetBSD, pkg_add on OpenBSD, IPS's
    /// pkg on illumos, on Linux the first of pacman, apt, dnf and zypper that is installed,
    /// Homebrew on a Mac, winget on Windows.
    pub fn here() -> Option<Manager> {
        if cfg!(any(target_os = "freebsd", target_os = "dragonfly")) {
            Some(Manager::Pkg)
        } else if cfg!(target_os = "netbsd") {
            Some(Manager::Pkgin)
        } else if cfg!(target_os = "openbsd") {
            Some(Manager::PkgAdd)
        } else if cfg!(any(target_os = "illumos", target_os = "solaris")) {
            Some(Manager::Ips)
        } else if cfg!(target_os = "macos") {
            Some(Manager::Brew)
        } else if cfg!(windows) {
            Some(Manager::Winget)
        } else {
            [("pacman", Manager::Pacman), ("apt", Manager::Apt), ("dnf", Manager::Dnf), ("zypper", Manager::Zypper)].into_iter().find(|(p, _)| which(p).is_some()).map(|(_, m)| m)
        }
    }
}

/// The line that installs `program` with `manager`, for the user to copy and run: Coxswain never
/// runs it. `program` is what Coxswain looks for: `tesseract`, `pdftoppm`, `soffice`, `latex`,
/// `plantuml`, `pandoc`, `nerd-font` (a font with the icons), or for the desktop app `gst-good`
/// (video and sound) and `cjk-font` (Japanese and Korean letters). `None` when that manager has no
/// package for it; the hint then says where to get it.
pub fn install_line(program: &str, manager: Manager) -> Option<String> {
    use Manager::*;
    // (pkg, pkgin, pkg_add, IPS, pacman, apt, dnf, zypper, brew, winget); empty: none.
    let row: [&str; 10] = match program {
        "tesseract" => ["tesseract", "tesseract", "tesseract", "", "tesseract tesseract-data-eng", "tesseract-ocr", "tesseract", "tesseract-ocr", "tesseract", "UB-Mannheim.TesseractOCR"],
        "pdftoppm" => ["poppler-utils", "poppler-utils", "poppler-utils", "", "poppler", "poppler-utils", "poppler-utils", "poppler-tools", "poppler", ""],
        "soffice" => ["libreoffice", "libreoffice", "libreoffice", "", "libreoffice-fresh", "libreoffice", "libreoffice", "libreoffice", "--cask libreoffice", "TheDocumentFoundation.LibreOffice"],
        "latex" => ["texlive-full", "", "texlive_texmf-full", "ooce/application/texlive", "texlive-basic texlive-latexextra texlive-binextra", "texlive-latex-extra latexmk", "texlive-scheme-medium latexmk", "texlive-latexmk texlive-collection-latexextra", "--cask mactex-no-gui", "MiKTeX.MiKTeX"],
        "plantuml" => ["plantuml", "", "plantuml", "", "plantuml", "plantuml", "plantuml", "plantuml", "plantuml", ""],
        "pandoc" => ["hs-pandoc", "pandoc-cli", "pandoc", "", "pandoc-cli", "pandoc", "pandoc", "pandoc", "pandoc", "JohnMacFarlane.Pandoc"],
        // What the desktop app's preview and its letters need, where a notice names the line.
        "gst-good" => ["gstreamer1-plugins-good", "gst-plugins1-good", "gstreamer1-plugins-good", "", "", "", "", "", "", ""],
        "cjk-font" => ["noto-sans-jp noto-sans-kr", "noto-cjk-fonts", "noto-cjk", "", "", "", "", "", "", ""],
        "nerd-font" => ["nerd-fonts", "nerd-fonts-Symbols", "nerd-fonts", "", "ttf-nerd-fonts-symbols", "", "", "", "--cask font-symbols-only-nerd-font", ""],
        _ => return None,
    };
    let package = row[match manager {
        Pkg => 0,
        Pkgin => 1,
        PkgAdd => 2,
        Ips => 3,
        Pacman => 4,
        Apt => 5,
        Dnf => 6,
        Zypper => 7,
        Brew => 8,
        Winget => 9,
    }];
    if package.is_empty() {
        return None;
    }
    Some(match manager {
        Pkg | Ips => format!("pkg install {package}"),
        Pkgin => format!("pkgin install {package}"),
        PkgAdd => format!("pkg_add {package}"),
        Pacman => format!("sudo pacman -S {package}"),
        Apt => format!("sudo apt install {package}"),
        Dnf => format!("sudo dnf install {package}"),
        Zypper => format!("sudo zypper install {package}"),
        Brew => format!("brew install {package}"),
        Winget => format!("winget install --id {package} -e"),
    })
}

/// The line that installs `program` here, if Coxswain knows one.
pub fn install(program: &str) -> Option<String> {
    install_line(program, Manager::here()?)
}

/// Whether a Nerd Font is installed, where that can be told: fontconfig on Linux and the BSDs,
/// the font folders on a Mac and on Windows. `None`: it cannot be told.
pub fn nerd_font() -> Option<bool> {
    let named = |name: &str| name.to_lowercase().contains("nerd");
    if cfg!(any(windows, target_os = "macos")) {
        let dirs: Vec<PathBuf> = if cfg!(windows) {
            [std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")), dirs::data_local_dir().map(|d| d.join("Microsoft/Windows/Fonts"))].into_iter().flatten().collect()
        } else {
            [Some(PathBuf::from("/Library/Fonts")), dirs::home_dir().map(|h| h.join("Library/Fonts"))].into_iter().flatten().collect()
        };
        return Some(dirs.iter().filter_map(|d| std::fs::read_dir(d).ok()).flatten().flatten().any(|e| named(&e.file_name().to_string_lossy())));
    }
    let out = std::process::Command::new("fc-list").args([":", "family"]).output().ok().filter(|o| o.status.success())?;
    Some(named(&String::from_utf8_lossy(&out.stdout)))
}

/// This app's program: the AppImage file when it runs from one (the binary itself is inside a
/// mount that goes when the app closes), else the running binary.
pub fn this_app() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) => Ok(PathBuf::from(image)),
        None => std::env::current_exe(),
    }
}

/// Take the PATH of the user's login shell: a Mac's Dock and launchd give the bare system one.
/// Called first thing, before other threads run, since it changes the environment.
pub fn login_path() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    if let Ok(out) = command(shell).args(["-lc", "echo $PATH"]).output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            // Safety: nothing else runs yet, so no other thread reads the environment.
            unsafe { std::env::set_var("PATH", path) };
        }
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
    fn tools_say_how_to_install_what_is_missing() {
        assert_eq!(install_line("tesseract", Manager::Pkg).as_deref(), Some("pkg install tesseract"));
        assert_eq!(install_line("pdftoppm", Manager::Apt).as_deref(), Some("sudo apt install poppler-utils"));
        assert_eq!(install_line("soffice", Manager::Brew).as_deref(), Some("brew install --cask libreoffice"));
        assert_eq!(install_line("latex", Manager::Winget).as_deref(), Some("winget install --id MiKTeX.MiKTeX -e"));
        assert_eq!(install_line("plantuml", Manager::Dnf).as_deref(), Some("sudo dnf install plantuml"));
        assert_eq!(install_line("pdftoppm", Manager::Winget), None, "no package: the hint says where to get it");
        assert_eq!(install_line("nope", Manager::Pacman), None);
        assert_eq!(install_line("pdftoppm", Manager::Pkgin).as_deref(), Some("pkgin install poppler-utils"));
        assert_eq!(install_line("tesseract", Manager::PkgAdd).as_deref(), Some("pkg_add tesseract"));
        assert_eq!(install_line("gst-good", Manager::Pkg).as_deref(), Some("pkg install gstreamer1-plugins-good"));
        assert_eq!(install_line("gst-good", Manager::Apt), None, "Linux: the notice names the packages of each distribution");
        if cfg!(target_os = "freebsd") {
            assert_eq!(Manager::here(), Some(Manager::Pkg));
        }
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
            assert_eq!(run("yes | head -n 50000", 5000, 10).map(|v| v.len()), Some(10), "cut at the limit, the rest drained");
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

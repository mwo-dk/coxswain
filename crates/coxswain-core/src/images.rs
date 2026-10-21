//! The container images of `[preview.images]`: what each one brings and costs, keeping the
//! pulled ones up to date (`[preview] image_updates`), and removing those no preview used for
//! 90 days (`[preview] image_prune`). The desktop app does both in the background; the terminal
//! app makes no container previews (only Make PDF searchable runs a container there), so it
//! only shows the settings.
//!
//! Only Coxswain's own images are ever touched: an update removes the copy it replaced by that
//! copy's id, never with `image prune`, which would take the user's other untagged images too.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::config::PreviewConfig;
use crate::state::AppState;
use crate::t;

const DAY: u64 = 24 * 3600;
/// An image no preview used for this many days goes, when `image_prune` is on.
pub const UNUSED_DAYS: u64 = 90;

/// Megabytes to download and on disk of the default images, measured (amd64, 2026-10).
const SIZES: &[(&str, u32, u32)] = &[
    ("docker.io/apache/tika:4.1.0@sha256:06bcdbd09aca073293e5a171ad161418cc93c91d58d9e4e205064eade84889d4", 190, 440),
    ("docker.io/texlive/texlive:latest", 2800, 5900),
    ("docker.io/texlive/texlive:latest-medium", 960, 2200),
    ("docker.io/plantuml/plantuml:latest", 141, 353),
    ("docker.io/pandoc/core:latest", 57, 246),
    ("docker.io/gotenberg/gotenberg:8-libreoffice", 440, 1030),
    ("docker.io/yuzutech/kroki:latest", 1200, 2830),
    ("docker.io/jbarlow83/ocrmypdf-alpine:latest", 310, 850),
    ("ghcr.io/typst/typst:latest", 29, 65),
    ("docker.io/mwader/static-ffmpeg:latest", 126, 315),
    ("docker.io/dstmodders/imagemagick:latest", 60, 152),
    ("ghcr.io/osgeo/gdal:alpine-small-3.13.3@sha256:58e21a794e3e49c2882a078cf39625aa8d1f86188e0273f74156e9f3f3cd0253", 40, 98),
    ("docker.io/openscad/openscad:trixie.2026-10-19@sha256:bbed39fad63b66f26113d82e2b39ce1e14b2568378426fb2cdfeb41e34d7837b", 241, 635),
];

const TOOLS: [&str; 15] = [
    "tika",
    "latex",
    "libreoffice",
    "plantuml",
    "pandoc",
    "duckdb",
    "kroki",
    "typst",
    "ghostscript",
    "ocrmypdf",
    "gxps",
    "ffmpeg",
    "imagemagick",
    "gdal",
    "openscad",
];

/// The image of `tool`: the diagram kinds Kroki's image draws share its `kroki` image, a
/// video's frame, a song's waveform and HEIC photos ffmpeg's.
pub fn key(tool: &str) -> &str {
    match tool {
        t if KROKI.contains(&t) => "kroki",
        "waveform" | "heif" => "ffmpeg",
        t => t,
    }
}

/// The diagram kinds whose programs Kroki's image has, each drawn from standard input to SVG.
pub const KROKI: &[&str] = &[
    "d2", "ditaa", "svgbob", "pikchr", "nomnoml", "wavedrom", "vega", "vegalite", "dbml", "erd",
    "bytefield",
];

/// `<rt> run` as every container of Coxswain's starts: removed when it ends, no network, no
/// SELinux relabelling of your folders, named `name` so a run that is stopped can be killed,
/// `src` read-only at /src and `out` (when there is one) at /out. It runs as you: in rootless
/// podman the container's root is you, so `--user 0:0`, where an image's own user (Gotenberg's,
/// OCRmyPDF's) could not write to /out; rootful docker or podman get your own uid and gid, so
/// no root-owned file is left, with `HOME=/tmp`, as that user has no home in the image.
pub fn sealed(rt: &str, rt_path: &Path, name: &str, src: &Path, out: Option<&Path>) -> Command {
    let mut c = crate::tools::command(rt_path);
    c.args(["run", "--rm", "-i", "--network=none", "--security-opt", "label=disable", "--name", name]);
    c.arg("--mount").arg(bind(src, "/src", true));
    if let Some(out) = out {
        c.arg("--mount").arg(bind(out, "/out", false));
    }
    c.args(user(rt == "podman" && rootless(rt_path), out.unwrap_or(src)));
    c
}

/// `--mount` of the folder `host` at `at`: not `-v`, which splits at every `:`, so a folder
/// named `10:30` would not mount. The source is one quoted CSV field, as
/// podman and docker read it, so a `,` or `"` in the path stays in it.
pub(crate) fn bind(host: &Path, at: &str, ro: bool) -> String {
    let src = host.display().to_string().replace('"', "\"\"");
    format!("type=bind,\"source={src}\",target={at}{}", if ro { ",readonly" } else { "" })
}

/// The options that hold ffmpeg's and ImageMagick's containers tighter than `sealed`: a
/// read-only root with a small `/tmp` in memory, bounded memory and processes, no
/// capabilities, no gaining of privileges. `label=disable` stays, as in `sealed`: relabelling
/// (`:Z`) would change the SELinux label of the user's own file.
pub const TIGHT: &[&str] = &[
    "--read-only",
    "--tmpfs",
    "/tmp:rw,size=1g",
    "--memory=3g",
    "--pids-limit=256",
    "--cap-drop=ALL",
    "--security-opt",
    "no-new-privileges",
];

/// `sealed`, seeing one file only: `file`, read-only at `at` (`/src/in.heic`), instead of a
/// folder at /src, with `TIGHT`'s limits. Nothing else of its folder is there to be read, and
/// `out` is the only place it can write.
pub fn sealed_file(rt: &str, rt_path: &Path, name: &str, file: &Path, at: &str, out: Option<&Path>) -> Command {
    let mut c = crate::tools::command(rt_path);
    c.args(["run", "--rm", "-i", "--network=none", "--security-opt", "label=disable", "--name", name]);
    c.args(TIGHT);
    c.arg("--mount").arg(bind(file, at, true));
    if let Some(out) = out {
        c.arg("--mount").arg(bind(out, "/out", false));
    }
    c.args(user(rt == "podman" && rootless(rt_path), out.unwrap_or(file)));
    c
}

/// The `--user` (and `HOME`) options of `sealed`.
fn user(rootless_podman: bool, owner: &Path) -> Vec<String> {
    if rootless_podman {
        return vec!["--user".into(), "0:0".into()];
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata(owner) {
            return vec!["--user".into(), format!("{}:{}", m.uid(), m.gid()), "-e".into(), "HOME=/tmp".into()];
        }
    }
    let _ = owner;
    vec![]
}

/// Does podman run rootless here? Asked once (`podman info`); when it cannot say, rootless,
/// which is how podman runs for a user.
fn rootless(rt: &Path) -> bool {
    static ROOTLESS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ROOTLESS.get_or_init(|| {
        crate::tools::command(rt)
            .args(["info", "--format", "{{.Host.Security.Rootless}}"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() != "false")
            .unwrap_or(true)
    })
}

/// Whether the runtime at `rt` can bound a container's memory and processes (`--memory`,
/// `--pids-limit`): a rootless podman whose cgroup controllers are not delegated cannot. Found
/// by a container of `image` that only runs `true` with both bounds, never from what a real run
/// printed (a file's name or a decoder's words could say anything). It fails closed: only a
/// probe that the runtime refused for want of those controllers turns the bounds off; any other
/// failure (the image not pulled yet, a hiccup) keeps them on and is asked again next time.
pub fn bounds_work(rt: &Path, image: &str) -> bool {
    static KNOWN: Mutex<BTreeMap<(PathBuf, String), bool>> = Mutex::new(BTreeMap::new());
    let key = (rt.to_path_buf(), image.to_string());
    if let Some(k) = KNOWN.lock().unwrap().get(&key) {
        return *k;
    }
    let out = crate::tools::capture(
        {
            let mut c = crate::tools::command(rt);
            c.args(["run", "--rm", "--pull=never", "--network=none", "--memory=64m", "--pids-limit=16", "--entrypoint", "true", image]);
            c
        },
        std::time::Duration::from_secs(60),
        4096,
    );
    let said = out.ok().and_then(|o| bounds_said(o.status.success(), &String::from_utf8_lossy(&o.stderr)));
    match said {
        Some(ok) => {
            if !ok {
                note(rt);
            }
            KNOWN.lock().unwrap().insert(key, ok);
            ok
        }
        None => true,
    }
}

fn note(rt: &Path) {
    crate::helper::note(format_args!("{}: no memory and process bounds for containers here", rt.display()));
}

/// What a probe of `bounds_work` says: it ran (they work), it was refused for want of the
/// controllers (they do not), or it failed some other way (not known: None). Its output is the
/// runtime's alone: the probe runs `true`, with nothing of the user's in it.
fn bounds_said(ran: bool, stderr: &str) -> Option<bool> {
    if ran {
        return Some(true);
    }
    let e = stderr.to_lowercase();
    (["cgroup", "memory.max", "pids.max", "controller"].iter().any(|w| e.contains(w)) && !e.contains("image not known") && !e.contains("no such image"))
        .then_some(false)
}

/// The last progress line of every pull under way, by image, for the apps to show.
static PULLING: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// The latest progress line of every pull under way, by image.
pub fn pulling() -> BTreeMap<String, String> {
    PULLING.lock().unwrap().clone()
}

/// Pull (or update) `image`, streaming its progress into `pulling()`. Not covered by any
/// timeout: images can be gigabytes. Two pulls of the same image at once are fine, the
/// runtime shares one download.
pub fn pull(rt: &Path, image: &str) -> Result<(), String> {
    pull_until(rt, image, &std::sync::atomic::AtomicBool::new(false))
}

/// `pull`, which `stop` ends: the pull is killed, and what it fetched so far the runtime
/// keeps for the next one.
pub fn pull_until(rt: &Path, image: &str, stop: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    PULLING.lock().unwrap().insert(image.to_string(), String::new());
    let result = (|| {
        let mut child = crate::tools::command(rt)
            .args(["pull", image])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        // docker reports on stdout, podman on stderr; errors come on stderr.
        let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let (a, b) = (image.to_string(), image.to_string());
        let t = std::thread::spawn(move || follow(out, &a));
        let e = std::thread::spawn(move || follow(err, &b));
        let status = loop {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(t!("searchable.stopped"));
            }
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let _ = t.join();
        let last_err = e.join().unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(t!("convert.pull_failed", "image" => image, "error" => last_err))
        }
    })();
    PULLING.lock().unwrap().remove(image);
    result
}

/// Keep the latest line (progress bars redraw with `\r`) of `r` in `PULLING` for `image`;
/// returns the last one.
fn follow(mut r: impl Read, image: &str) -> String {
    let (mut buf, mut acc, mut last) = ([0u8; 4096], String::new(), String::new());
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        acc.push_str(&String::from_utf8_lossy(&buf[..n]));
        let Some(i) = acc.rfind(['\n', '\r']) else { continue };
        if let Some(line) = acc[..i].rsplit(['\n', '\r']).map(str::trim).find(|l| !l.is_empty()) {
            last = line.to_string();
            PULLING.lock().unwrap().insert(image.to_string(), last.clone());
        }
        acc.drain(..=i);
    }
    last
}

/// An image's name as texts show it: without the digest it is pinned to (`…@sha256:…`).
pub fn short(image: &str) -> &str {
    image.split('@').next().unwrap_or(image)
}

/// Megabytes as "2.8 GB" or "141 MB".
fn shown(mb: u32) -> String {
    crate::i18n::digits(if mb >= 1000 {
        format!("{}.{} {}", mb / 1000, mb % 1000 / 100, t!("unit.GB"))
    } else {
        format!("{mb} {}", t!("unit.MB"))
    })
}

fn sizes(image: &str) -> Option<(u32, u32)> {
    let full = if image.starts_with(&format!("{}/", crate::settings::registry(image))) {
        image.to_string()
    } else {
        format!("docker.io/{image}")
    };
    SIZES.iter().find(|s| s.0 == full).map(|s| (s.1, s.2))
}

/// How much `image` downloads, "2.8 GB", when it is one of the default ones.
pub fn download(image: &str) -> Option<String> {
    sizes(image).map(|(d, _)| shown(d))
}

/// What the image of `tool` gives, its size, and what it costs: shown where it is pulled
/// (Settings → Previews) and in Disk use. None for a name under `[preview.images]` that is no
/// tool's.
pub fn about(tool: &str, image: &str) -> Option<String> {
    if !TOOLS.contains(&tool) {
        return None;
    }
    let size = match sizes(image) {
        Some((d, disk)) => t!("image.size", "download" => shown(d), "disk" => shown(disk)),
        None => t!("image.size_unknown"),
    };
    [
        crate::i18n::tr(&format!("image.what.{tool}"), &[]),
        size,
        crate::i18n::tr(&format!("image.cost.{tool}"), &[]),
    ]
    .join(" ")
    .into()
}

/// Days between two rounds of updates for `[preview] image_updates`; none when off.
pub fn every(updates: &str) -> Option<u64> {
    match updates {
        "weekly" => Some(7),
        "monthly" => Some(30),
        _ => None,
    }
}

/// Is a round of updates due at `now`?
pub fn update_due(cfg: &PreviewConfig, st: &AppState, now: u64) -> bool {
    cfg.container != "off"
        && every(&cfg.image_updates)
            .is_some_and(|d| now.saturating_sub(st.images_checked) >= d * DAY)
}

/// A preview just ran in `image`.
pub fn used(st: &mut AppState, image: &str, now: u64) {
    st.images_used.insert(image.to_string(), now);
}

/// Of `pulled`, the images no preview used for `UNUSED_DAYS`. One with no use recorded counts
/// as used now, so turning the option on never removes everything at once.
pub fn stale(st: &mut AppState, pulled: &[String], now: u64) -> Vec<String> {
    pulled
        .iter()
        .filter(|i| {
            let last = *st.images_used.entry(i.to_string()).or_insert(now);
            now.saturating_sub(last) >= UNUSED_DAYS * DAY
        })
        .cloned()
        .collect()
}

/// The configured images the runtime `rt` has, each once.
pub fn pulled(cfg: &PreviewConfig, rt: &Path) -> Vec<String> {
    let mut v: Vec<String> = cfg
        .images
        .values()
        .filter(|i| !i.is_empty())
        .cloned()
        .collect();
    v.sort();
    v.dedup();
    v.retain(|i| crate::tools::image_size(rt, i).is_some());
    v
}

/// The id of the copy of `image` the runtime has.
pub fn id(rt: &Path, image: &str) -> Option<String> {
    let out = crate::tools::command(rt)
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty()).then_some(id)
}

/// `<rt> image rm what`, a name or an id; the runtime's last error line when it refuses.
pub fn remove(rt: &Path, what: &str) -> Result<(), String> {
    let out = crate::tools::command(rt)
        .args(["image", "rm", what])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .to_string())
}

/// Update `image` with `pull`; when that brought a new copy, the one it replaced is removed by
/// its id at once. A copy that cannot go (a container still uses it, another name points at
/// it) stays, quietly.
pub fn update(
    rt: &Path,
    image: &str,
    pull: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let old = id(rt, image);
    pull()?;
    if let Some(old) = replaced(old, id(rt, image)) {
        let _ = remove(rt, &old);
    }
    Ok(())
}

/// A round of updates: every image in `images` is tried, each with `pull`. Done when there was
/// none or at least one pull worked; when all fail (offline) it stays due for the next hour,
/// and one image that always fails does not make the others pull every hour.
pub fn update_round(
    rt: &Path,
    images: &[String],
    pull: impl Fn(&str) -> Result<(), String>,
) -> bool {
    let ok: Vec<bool> = images
        .iter()
        .map(|i| update(rt, i, || pull(i)).is_ok())
        .collect();
    ok.is_empty() || ok.contains(&true)
}

/// The copy a pull replaced: the old id, only when a new copy with another id is there. A new
/// id that cannot be read keeps the old one, which may be the only copy.
fn replaced(old: Option<String>, new: Option<String>) -> Option<String> {
    old.filter(|o| new.is_some_and(|n| &n != o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_updates_come_weekly_or_monthly_and_never_without_containers() {
        let mut cfg = PreviewConfig::default();
        let mut st = AppState::default();
        let now = 100 * DAY;
        assert!(!update_due(&cfg, &st, now), "off by default");
        cfg.image_updates = "weekly".into();
        assert!(update_due(&cfg, &st, now));
        st.images_checked = now - 6 * DAY;
        assert!(!update_due(&cfg, &st, now));
        st.images_checked = now - 7 * DAY;
        assert!(update_due(&cfg, &st, now));
        cfg.image_updates = "monthly".into();
        assert!(!update_due(&cfg, &st, now));
        st.images_checked = now - 30 * DAY;
        assert!(update_due(&cfg, &st, now));
        cfg.container = "off".into();
        assert!(!update_due(&cfg, &st, now));
    }

    #[test]
    fn images_unused_for_90_days_are_stale_and_one_never_seen_starts_now() {
        let mut st = AppState::default();
        let now = 1000 * DAY;
        used(&mut st, "a", now - 91 * DAY);
        used(&mut st, "b", now - 10 * DAY);
        let pulled = ["a", "b", "c"].map(String::from);
        assert_eq!(stale(&mut st, &pulled, now), ["a"]);
        assert_eq!(
            st.images_used.get("c"),
            Some(&now),
            "first seen: counted as used now"
        );
        assert_eq!(stale(&mut st, &pulled, now + 89 * DAY), ["a", "b"]);
        assert_eq!(stale(&mut st, &pulled, now + 90 * DAY), ["a", "b", "c"]);
    }

    #[test]
    fn images_update_removes_the_old_copy_only_when_a_new_one_is_there() {
        let s = |x: &str| Some(x.to_string());
        assert_eq!(replaced(s("old"), s("new")), s("old"));
        assert_eq!(replaced(s("old"), s("old")), None, "nothing new pulled");
        assert_eq!(
            replaced(s("old"), None),
            None,
            "the new id unread: keep the old"
        );
        assert_eq!(replaced(None, s("new")), None);
    }

    #[test]
    fn images_update_round_tries_every_image() {
        // No runtime there: no id is read, so nothing is removed.
        let rt = Path::new("/nonexistent/coxswain-test-runtime");
        let images = ["a", "bad", "c"].map(String::from);
        let tried = std::cell::RefCell::new(vec![]);
        let pull = |i: &str| {
            tried.borrow_mut().push(i.to_string());
            if i == "bad" { Err("no".into()) } else { Ok(()) }
        };
        assert!(
            update_round(rt, &images, pull),
            "one failing image ends the round"
        );
        assert_eq!(
            *tried.borrow(),
            images,
            "the one after a failure is tried too"
        );
        assert!(
            !update_round(rt, &images, |_| Err("offline".into())),
            "all failing: still due"
        );
        assert!(update_round(rt, &[], |_| Err("never".into())));
    }

    /// Needs podman and the network: `cargo test -p coxswain-core -- --ignored real_pull_stops`.
    /// A large image not pulled yet; stopped after a second, the pull ends at once.
    #[test]
    #[ignore]
    fn real_pull_stops() {
        let rt = crate::tools::which("podman").unwrap();
        let image = "docker.io/library/python:3.13";
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let start = std::time::Instant::now();
        assert!(pull_until(&rt, image, &stop).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
        assert!(crate::tools::image_size(&rt, image).is_none(), "not pulled");
        assert!(!pulling().contains_key(image));
    }

    #[test]
    fn images_mount_folders_with_colons_commas_and_quotes() {
        assert_eq!(bind(Path::new("/m/10:30, \"x\""), "/src", true), "type=bind,\"source=/m/10:30, \"\"x\"\"\",target=/src,readonly");
        assert_eq!(bind(Path::new("/c"), "/out", false), "type=bind,\"source=/c\",target=/out");
    }

    #[test]
    fn images_bounds_stay_on_unless_the_runtime_lacks_them() {
        assert_eq!(bounds_said(true, ""), Some(true));
        assert_eq!(bounds_said(false, "Error: crun: opening file `memory.max` for writing: No such file or directory: OCI runtime attempted to invoke a command that was not found"), Some(false));
        assert_eq!(bounds_said(false, "Error: docker.io/apache/tika:latest: image not known"), None, "not pulled yet: unknown, bounds stay on");
        assert_eq!(bounds_said(false, "Error: cannot connect to Podman"), None);
        assert_eq!(bounds_said(false, ""), None);
    }

    #[test]
    fn follow_keeps_last_complete_line() {
        assert_eq!(follow("one\ntwo\rthree".as_bytes(), "t"), "two");
        assert_eq!(pulling().get("t").map(String::as_str), Some("two"));
    }

    #[test]
    fn images_diagram_kinds_share_krokis() {
        assert_eq!(key("d2"), "kroki");
        assert_eq!(key("latex"), "latex");
        assert_eq!((key("waveform"), key("heif"), key("raw")), ("ffmpeg", "ffmpeg", "raw"));
        let one = sealed_file("docker", Path::new("/nonexistent/docker"), "n", Path::new("/home/me/-x.heic"), "/src/in.heic", None);
        let args: Vec<String> = one.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|a| a == "type=bind,\"source=/home/me/-x.heic\",target=/src/in.heic,readonly") && args.contains(&"--network=none".to_string()), "{args:?}");
        assert!(!args.iter().any(|a| a.contains("target=/src,")), "only the one file");
        for want in ["--rm", "--network=none", "--read-only", "/tmp:rw,size=1g", "--memory=3g", "--pids-limit=256", "--cap-drop=ALL", "no-new-privileges"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        assert_eq!(args.iter().filter(|a| a.starts_with("type=bind") && !a.ends_with(",readonly")).count(), 0, "no writable mount without an output folder");
        let c = sealed("docker", Path::new("/nonexistent/docker"), "n", Path::new("/a"), Some(Path::new("/b")));
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        for want in ["--network=none", "type=bind,\"source=/a\",target=/src,readonly", "type=bind,\"source=/b\",target=/out"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        assert_eq!(user(true, Path::new("/")), ["--user", "0:0"], "rootless podman: you are its root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let tmp = std::env::temp_dir();
            let m = std::fs::metadata(&tmp).unwrap();
            assert_eq!(user(false, &tmp), ["--user".to_string(), format!("{}:{}", m.uid(), m.gid()), "-e".into(), "HOME=/tmp".into()], "rootful: the folder's owner");
        }
    }

    #[test]
    fn images_are_explained_with_their_sizes() {
        crate::i18n::set_language("en-GB");
        let about = about("latex", "docker.io/texlive/texlive:latest").unwrap();
        assert!(
            about.contains("2.8 GB to download, 5.9 GB on disk"),
            "{about}"
        );
        assert_eq!(
            download("texlive/texlive:latest-medium").as_deref(),
            Some("960 MB")
        );
        assert_eq!(download("ghcr.io/me/texlive:latest"), None);
        assert!(
            super::about("duckdb", "ghcr.io/me/duckdb")
                .unwrap()
                .contains(&t!("image.size_unknown"))
        );
        assert_eq!(super::about("other", "x"), None);
        for tool in TOOLS {
            assert!(
                !super::about(tool, "").unwrap().contains("image."),
                "{tool}: every text is there"
            );
        }
    }
}

//! The container images of `[preview.images]`: what each one brings and costs, keeping the
//! pulled ones up to date (`[preview] image_updates`), and removing those no preview used for
//! 90 days (`[preview] image_prune`). The desktop app does both in the background; the terminal
//! app makes no container previews (only Make PDF searchable and Scan for viruses run containers there), so it
//! only shows the settings.
//!
//! Every default image is pinned by its digest (`name:tag@sha256:…`, `preview_tools::IMAGES`):
//! a pull fetches exactly that build, never whatever the tag points at that day. A new version
//! of Coxswain brings new pins (`tools/images-pin.py` moves them to each tag's newest build); a
//! round of updates then pulls the new pin of each image whose old pin a preview ran in, and
//! removes the old copy (`superseded`, `move_pins`). An image the user set in `config.toml` is
//! never rewritten nor replaced.
//!
//! Only Coxswain's own images are ever touched: an update removes the copy it replaced by that
//! copy's id or its old pin, never with `image prune`, which would take the user's other
//! untagged images too.

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

/// The image of `tool` under `[preview.images]` (`preview_tools::image_key`).
pub fn key(tool: &str) -> &str {
    crate::preview_tools::image_key(tool)
}

/// `<rt> run` as every container of Coxswain's starts: removed when it ends, no network, no
/// SELinux relabelling of your folders, named `name` so a run that is stopped can be killed,
/// held by `TIGHT`, `src` read-only at /src and `out` (when there is one) at /out. It runs as
/// you: in rootless podman the container's root is you, so `--user 0:0`, where an image's own
/// user (Gotenberg's, OCRmyPDF's) could not write to /out; rootful docker or podman get your own
/// uid and gid, so no root-owned file is left. `HOME=/tmp`, as the root is read-only and that
/// user has no home in the image.
pub fn sealed(rt: &str, rt_path: &Path, name: &str, src: &Path, out: Option<&Path>) -> Command {
    sealed_at(rt, rt_path, name, src, "/src", out)
}

fn sealed_at(rt: &str, rt_path: &Path, name: &str, src: &Path, at: &str, out: Option<&Path>) -> Command {
    let mut c = crate::tools::command(rt_path);
    c.args(["run", "--rm", "-i", "--network=none", "--security-opt", "label=disable", "--name", name]);
    c.args(tight(rt_path));
    c.arg("--mount").arg(bind(src, at, true));
    if let Some(out) = out {
        c.arg("--mount").arg(bind(out, "/out", false));
    }
    c.args(user(rt == "podman" && rootless(rt_path), out.unwrap_or(src)));
    c.args(["-e", "HOME=/tmp"]);
    c
}

/// `--mount` of the folder `host` at `at`: not `-v`, which splits at every `:`, so a folder
/// named `10:30` would not mount. The source is one quoted CSV field, as
/// podman and docker read it, so a `,` or `"` in the path stays in it.
pub(crate) fn bind(host: &Path, at: &str, ro: bool) -> String {
    let src = host.display().to_string().replace('"', "\"\"");
    format!("type=bind,\"source={src}\",target={at}{}", if ro { ",readonly" } else { "" })
}

/// A result a run wrote into its scratch folder (`/out`, which the container could write
/// anything into), opened for reading only when it is a plain file of its own: never through a
/// link (to `~/.ssh/id_ed25519`, say), in a folder that is no link, not a FIFO or device (opened
/// without waiting, so a FIFO cannot hang it), and not a hard link to another file. The open
/// file and its length then; everything after reads from that handle, at most that length.
pub fn take_result(path: &Path) -> Result<(std::fs::File, u64), String> {
    let refuse = || format!("{}: {}", path.display(), t!("convert.no_result"));
    if let Some(dir) = path.parent()
        && std::fs::symlink_metadata(dir).map_err(|_| refuse())?.file_type().is_symlink()
    {
        return Err(refuse());
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).map_err(|_| refuse())?.file_type().is_symlink() {
        return Err(refuse());
    }
    let f = o.open(path).map_err(|_| refuse())?;
    let m = f.metadata().map_err(|_| refuse())?;
    #[cfg(unix)]
    let alone = std::os::unix::fs::MetadataExt::nlink(&m) == 1;
    #[cfg(not(unix))]
    let alone = true;
    if !m.is_file() || !alone {
        return Err(refuse());
    }
    Ok((f, m.len()))
}

/// `take_result`'s file read whole, when it is at most `max` bytes (`too_large` otherwise).
pub fn read_result(path: &Path, max: u64, too_large: impl FnOnce() -> String) -> Result<Vec<u8>, String> {
    let (f, len) = take_result(path)?;
    if len > max {
        return Err(too_large());
    }
    let mut b = Vec::with_capacity(len as usize);
    f.take(len).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// A file Coxswain writes into a scratch folder a container wrote into: whatever stands at
/// `path` (a link the container left there, say) is removed first, and the file is made anew,
/// so the write never goes through a link.
pub fn fresh_result(path: &Path) -> Result<std::fs::File, String> {
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The options that hold every container of Coxswain's (`sealed`, `sealed_file`, `sealed_bare`,
/// through `tight`; given once, as docker refuses a second `--tmpfs /tmp`): a read-only root
/// with a small `/tmp` in memory, bounded memory and processes, no capabilities, no gaining of
/// privileges. `label=disable` stays: relabelling (`:Z`) would change the SELinux label of the
/// user's own files.
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

/// `<rt> run` of a program that reads no file at all (the chess engine, talking over stdin
/// and stdout): `sealed` without a mount, with `TIGHT`'s limits, as the image's own user.
pub fn sealed_bare(_rt: &str, rt_path: &Path, name: &str) -> Command {
    let mut c = crate::tools::command(rt_path);
    c.args(["run", "--rm", "-i", "--network=none", "--security-opt", "label=disable", "--name", name]);
    c.args(tight(rt_path));
    c
}

/// `sealed`, seeing one file only: `file`, read-only at `at` (`/src/in.heic`), instead of a
/// folder at /src. Nothing else of its folder is there to be read, and `out` is the only place
/// it can write.
pub fn sealed_file(rt: &str, rt_path: &Path, name: &str, file: &Path, at: &str, out: Option<&Path>) -> Command {
    sealed_at(rt, rt_path, name, file, at, out)
}

/// The `--user` option of `sealed`.
pub(crate) fn user(rootless_podman: bool, owner: &Path) -> Vec<String> {
    if rootless_podman {
        return vec!["--user".into(), "0:0".into()];
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata(owner) {
            return vec!["--user".into(), format!("{}:{}", m.uid(), m.gid())];
        }
    }
    let _ = owner;
    vec![]
}

/// Does podman run rootless here? Asked once (`podman info`); when it cannot say, rootless,
/// which is how podman runs for a user.
pub(crate) fn rootless(rt: &Path) -> bool {
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

/// What `bounds_work` found, by runtime and image.
static BOUNDS: Mutex<BTreeMap<(PathBuf, String), bool>> = Mutex::new(BTreeMap::new());

/// `TIGHT`, less its memory and process bounds where a probe (`bounds_work`) found that `rt`
/// cannot keep them; all of it while nothing says so.
pub fn tight(rt: &Path) -> Vec<&'static str> {
    bounded(!BOUNDS.lock().unwrap().iter().any(|((r, _), ok)| r == rt && !ok))
}

/// `TIGHT`, with its memory and process bounds only when `bounds`: the one place every
/// container's options come from, those `sealed` starts and those built by hand (Tika's
/// server, calibre's).
pub fn bounded(bounds: bool) -> Vec<&'static str> {
    TIGHT.iter().copied().filter(|a| bounds || !(a.starts_with("--memory") || a.starts_with("--pids-limit"))).collect()
}

/// Whether the runtime at `rt` can bound a container's memory and processes (`--memory`,
/// `--pids-limit`): a rootless podman whose cgroup controllers are not delegated cannot. Found
/// by a container of `image` that only runs `true` with both bounds, never from what a real run
/// printed (a file's name or a decoder's words could say anything). It fails closed: only a
/// probe that the runtime refused for want of those controllers turns the bounds off; any other
/// failure (the image not pulled yet, a hiccup) keeps them on and is asked again next time.
pub fn bounds_work(rt: &Path, image: &str) -> bool {
    let key = (rt.to_path_buf(), image.to_string());
    if let Some(k) = BOUNDS.lock().unwrap().get(&key) {
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
            BOUNDS.lock().unwrap().insert(key, ok);
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
            Err(t!("convert.pull_failed", "image" => short(image), "error" => last_err))
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
    crate::preview_tools::sizes(image)
}

/// How much `image` downloads, "2.8 GB", when it is one of the default ones.
pub fn download(image: &str) -> Option<String> {
    sizes(image).map(|(d, _)| shown(d))
}

/// What the image of `tool` gives, its size, and what it costs: shown where it is pulled
/// (Settings → Previews) and in Disk use. None for a name under `[preview.images]` that is no
/// tool's.
pub fn about(tool: &str, image: &str) -> Option<String> {
    crate::preview_tools::image(tool)?;
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

/// Images that were the default for a tool once, under another name: (the image as it was set,
/// the `[preview.images]` key it was for). `superseded` replaces them, and Disk use lists them.
pub const FORMER: &[(&str, &str)] = &[("docker.io/dstmodders/imagemagick:latest", "imagemagick")];

/// The `[preview.images]` key `image` was once the default for, under any tag or digest.
pub fn former(image: &str) -> Option<&'static str> {
    let name = |i: &str| {
        let i = short(i);
        let i = i.strip_prefix("docker.io/").unwrap_or(i);
        let slash = i.rfind('/').map_or(0, |at| at + 1);
        i[..i[slash..].find(':').map_or(i.len(), |c| slash + c)].to_string()
    };
    FORMER.iter().find(|(old, _)| name(old) == name(image)).map(|(_, key)| *key)
}

/// Copies of default images Coxswain ran in that no tool is set to now, each with the default
/// that took its place: the same name and tag under another digest, so an older pin, or the
/// bare tag from before it was pinned; or a former default under another name (`FORMER`),
/// always looked for, since it may have been pulled and never used. Only defaults replace: an
/// image of the user's own, or one they set before, is never part of it.
pub fn superseded(cfg: &PreviewConfig, st: &AppState) -> Vec<(String, String)> {
    let defaults = crate::preview_tools::default_images();
    let set: Vec<&String> = cfg.images.values().filter(|i| !i.is_empty() && defaults.values().any(|d| d == *i)).collect();
    let all: Vec<&String> = cfg.images.values().collect();
    let mut olds: Vec<&str> = st.images_used.keys().map(String::as_str).collect();
    olds.extend(FORMER.iter().map(|(old, _)| *old).filter(|old| !st.images_used.contains_key(*old)));
    olds.into_iter()
        .filter(|old| !all.iter().any(|i| i.as_str() == *old))
        .filter_map(|old| {
            let new = match former(old) {
                Some(key) => cfg.images.get(key).filter(|i| set.contains(i)),
                None => set.iter().find(|new| short(new) == short(old)).copied(),
            };
            new.map(|new| (old.to_string(), new.clone()))
        })
        .collect()
}

/// Move each pin of `pairs` (`superseded`): when the runtime has the old copy, the new pin is
/// pulled and the old copy removed by its name. The old images done with are returned, to be
/// forgotten: those removed, and those the runtime no longer has. A pull that fails keeps the
/// old copy for the next round.
pub fn move_pins(rt: &Path, pairs: &[(String, String)], pull: impl Fn(&str) -> Result<(), String>) -> Vec<String> {
    pairs
        .iter()
        .filter(|(old, new)| crate::tools::image_size(rt, old).is_none() || (pull(new).is_ok() && remove(rt, old).is_ok()))
        .map(|(old, _)| old.clone())
        .collect()
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

/// How many bytes the runtime's images take on disk together, layers they share counted once
/// (`system df`): podman says it in bytes, docker in rounded units ("1.2GB").
pub fn storage(rt: &Path) -> Option<u64> {
    let out = crate::tools::command(rt)
        .args(["system", "df", "--format", "json"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| images_size(&String::from_utf8_lossy(&out.stdout))).flatten()
}

/// The images' row of `system df --format json`: podman's array, or docker's one object a line.
fn images_size(said: &str) -> Option<u64> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(said)
        .ok()
        .or_else(|| said.lines().map(|l| serde_json::from_str(l).ok()).collect())?;
    let row = rows.iter().find(|r| r["Type"] == "Images")?;
    row["RawSize"].as_u64().or_else(|| units(row["Size"].as_str()?))
}

/// "1.234GB", "512.5kB", "0B": docker's sizes, in powers of 1000.
fn units(s: &str) -> Option<u64> {
    let at = s.find(|c: char| c.is_ascii_alphabetic())?;
    let n: f64 = s[..at].trim().parse().ok()?;
    let times = match s[at..].trim().to_ascii_uppercase().as_str() {
        "B" => 1e0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((n * times) as u64)
}

/// Remove `image`: the bytes that freed, which are only the layers no other image uses
/// (`storage` before and after). `None` when the runtime would not say.
pub fn remove_freeing(rt: &Path, image: &str) -> Result<Option<u64>, String> {
    let before = storage(rt);
    remove(rt, image)?;
    Ok(before.zip(storage(rt)).map(|(b, a)| b.saturating_sub(a)))
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
    /// What a hijacked program could leave in `/out`: a link to a secret, a link in a folder
    /// that is a link, a hard link to a secret, a FIFO. Each is refused; a plain file is read;
    /// a file written over a link replaces the link and leaves the secret as it was.
    #[cfg(unix)]
    #[test]
    fn results_are_taken_only_as_plain_files_of_their_own() {
        use super::{fresh_result, read_result, take_result};
        use std::os::unix::fs::symlink;
        let d = std::env::temp_dir().join(format!("coxswain-take-result-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (out, home) = (d.join("out"), d.join("home"));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let secret = home.join("id_ed25519");
        std::fs::write(&secret, "SECRET").unwrap();
        symlink(&secret, out.join("out.png")).unwrap();
        symlink(&home, out.join("sheets")).unwrap();
        std::fs::hard_link(&secret, out.join("hard.png")).unwrap();
        let fifo = std::ffi::CString::new(out.join("fifo.png").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        for bad in [out.join("out.png"), out.join("sheets").join("id_ed25519"), out.join("hard.png"), out.join("fifo.png"), out.join("missing.png")] {
            assert!(take_result(&bad).is_err(), "{}", bad.display());
        }
        std::fs::write(out.join("ok.png"), "PNG").unwrap();
        assert_eq!(read_result(&out.join("ok.png"), 3, || "big".into()).unwrap(), b"PNG");
        assert_eq!(read_result(&out.join("ok.png"), 2, || "big".into()), Err("big".into()));
        std::io::Write::write_all(&mut fresh_result(&out.join("out.png")).unwrap(), b"made").unwrap();
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "SECRET");
        assert_eq!(take_result(&out.join("out.png")).unwrap().1, 4);
        std::fs::remove_dir_all(d).unwrap();
    }

    use super::*;

    #[test]
    fn images_size_from_podman_and_docker() {
        let podman = r#"[
    {"Type":"Images","Total":6,"Active":1,"RawSize":9024242441,"Size":"9.024GB"},
    {"Type":"Containers","Total":1,"Active":1,"RawSize":13146,"Size":"13.15kB"}
]"#;
        assert_eq!(images_size(podman), Some(9_024_242_441));
        let docker = "{\"Active\":\"1\",\"Size\":\"1.234GB\",\"TotalCount\":\"6\",\"Type\":\"Images\"}\n{\"Size\":\"0B\",\"Type\":\"Containers\"}\n";
        assert_eq!(images_size(docker), Some(1_234_000_000));
        assert_eq!(units("512.5kB"), Some(512_500));
        assert_eq!(units("0B"), Some(0));
        assert_eq!(images_size("nonsense"), None);
    }

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
    fn images_defaults_are_pinned_by_digest() {
        for (tool, image) in PreviewConfig::default().images {
            let pinned = image.split_once('@').is_some_and(|(name, d)| name.rsplit('/').next().is_some_and(|n| n.contains(':')) && d.len() == 71 && d.starts_with("sha256:"));
            assert!(image.is_empty() || pinned, "{tool}: {image} is not name:tag@sha256:…");
        }
    }

    #[test]
    fn images_an_older_pin_or_the_bare_tag_is_superseded_by_the_new_pin() {
        let mut cfg = PreviewConfig::default();
        let latex = cfg.images["latex"].clone();
        let pandoc = cfg.images["pandoc"].clone();
        let tag = short(&latex).to_string();
        cfg.images.insert("plantuml".into(), "ghcr.io/me/plantuml:1@sha256:mine".into());
        let mut st = AppState::default();
        for i in [&format!("{tag}@sha256:old"), &tag, "docker.io/texlive/texlive:latest-medium", &pandoc, "ghcr.io/me/plantuml:1", "docker.io/plantuml/plantuml:latest"] {
            used(&mut st, i, 1);
        }
        let mut got = superseded(&cfg, &st);
        got.sort();
        let magick = cfg.images["imagemagick"].clone();
        assert_eq!(
            got,
            [("docker.io/dstmodders/imagemagick:latest".into(), magick.clone()), (tag.clone(), latex.clone()), (format!("{tag}@sha256:old"), latex)],
            "another tag, the one in use, the user's own and a default the user replaced stay; a former default is always looked for"
        );
        // The former ImageMagick default, under another tag too, makes way for today's; not
        // when ImageMagick is set to an image of the user's own, nor when it is set to it.
        assert_eq!((former("dstmodders/imagemagick:7.1"), former("docker.io/dstmodders/imagemagick@sha256:x"), former("docker.io/dstmodders/ffmpeg:latest")), (Some("imagemagick"), Some("imagemagick"), None));
        let mut st = AppState::default();
        used(&mut st, "docker.io/dstmodders/imagemagick:7.1", 1);
        assert!(superseded(&cfg, &st).contains(&("docker.io/dstmodders/imagemagick:7.1".into(), magick)));
        cfg.images.insert("imagemagick".into(), "ghcr.io/me/magick:1".into());
        assert!(superseded(&cfg, &st).iter().all(|(old, _)| former(old).is_none()));
        cfg.images.insert("imagemagick".into(), FORMER[0].0.into());
        assert!(superseded(&cfg, &st).iter().all(|(old, _)| old != FORMER[0].0));
    }

    /// A runtime that has the images named in `has` and writes what it removes to `rm.log`.
    #[cfg(unix)]
    #[test]
    fn images_move_pins_pulls_the_new_and_removes_the_old() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-test-pins-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rt = d.join("podman");
        std::fs::write(&rt, format!("#!/bin/sh\ncase \"$1 $2\" in\n'image inspect') grep -qxF \"$5\" {0}/has && echo 1;;\n'image rm') echo \"$3\" >> {0}/rm.log;;\nesac\n", d.display())).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.join("has"), "a:1@sha256:old\nb:1\n").unwrap();
        let pairs = [("a:1@sha256:old", "a:1@sha256:new"), ("b:1", "b:1@sha256:new"), ("c:1@sha256:gone", "c:1@sha256:new")].map(|(o, n)| (o.to_string(), n.to_string()));
        let pulled = std::cell::RefCell::new(vec![]);
        let done = move_pins(&rt, &pairs, |i| {
            pulled.borrow_mut().push(i.to_string());
            if i.starts_with("b:") { Err("offline".into()) } else { Ok(()) }
        });
        assert_eq!(*pulled.borrow(), ["a:1@sha256:new", "b:1@sha256:new"], "nothing pulled for a copy not there");
        assert_eq!(done, ["a:1@sha256:old", "c:1@sha256:gone"], "a failed pull keeps the old copy");
        assert_eq!(std::fs::read_to_string(d.join("rm.log")).unwrap(), "a:1@sha256:old\n");
        std::fs::remove_dir_all(&d).unwrap();
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
        assert!(start.elapsed() < crate::test_limit(std::time::Duration::from_secs(5)), "{:?}", start.elapsed());
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
        assert_eq!((key("waveform"), key("heif"), key("raw")), ("ffmpeg", "imagemagick", "imagemagick"));
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
        for want in ["--network=none", "type=bind,\"source=/a\",target=/src,readonly", "type=bind,\"source=/b\",target=/out", "--read-only", "--cap-drop=ALL", "no-new-privileges", "HOME=/tmp"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        // Once: docker refuses a second `--tmpfs /tmp`.
        for one in ["--tmpfs", "--read-only", "--cap-drop=ALL"] {
            assert_eq!(args.iter().filter(|a| *a == one).count(), 1, "{one}: {args:?}");
        }
        let file = sealed_file("docker", Path::new("/nonexistent/docker"), "n", Path::new("/a/x"), "/src/x", None);
        assert_eq!(file.get_args().filter(|a| *a == "--tmpfs").count(), 1);
        assert_eq!(user(true, Path::new("/")), ["--user", "0:0"], "rootless podman: you are its root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let tmp = std::env::temp_dir();
            let m = std::fs::metadata(&tmp).unwrap();
            assert_eq!(user(false, &tmp), ["--user".to_string(), format!("{}:{}", m.uid(), m.gid())], "rootful: the folder's owner");
        }
    }

    #[test]
    fn images_are_explained_with_their_sizes() {
        crate::i18n::set_language("en-GB");
        let about = about("latex", short(&PreviewConfig::default().images["latex"])).unwrap();
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
        for tool in crate::preview_tools::IMAGES.iter().map(|i| i.key) {
            assert!(
                !super::about(tool, "").unwrap().contains("image."),
                "{tool}: every text is there"
            );
        }
    }
}

//! The container images of `[preview.images]`: what each one brings and costs, keeping the
//! pulled ones up to date (`[preview] image_updates`), and removing those no preview used for
//! 90 days (`[preview] image_prune`). The desktop app does both in the background. With
//! `image_updates` at "ask" the look for newer builds (`check`) runs in both apps and a notice
//! offers the update; the terminal app makes no container previews (only Make PDF searchable
//! and Scan for viruses run containers there), so it pulls and removes nothing by itself.
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

pub mod pulls;

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
    sweep(rt_path);
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
    sweep(rt_path);
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

// ---------------------------------------------------------------- names, and what a crash leaves

/// The name of a container of Coxswain's that runs `tool` (lower-case letters only):
/// `coxswain-<tool>-<pid>-<start>-<n>`, `<start>` when this process started (`started`), so a
/// later run can tell whether the run that made it is gone (`sweep`). `<start>` is 0 where it
/// cannot be read (in a Flatpak, whose process ids the host does not see, and on systems
/// `started` does not know): such a container is never swept.
pub fn name(tool: &str) -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("coxswain-{tool}-{}-{}-{n}", std::process::id(), own_start().unwrap_or(0))
}

fn own_start() -> Option<u64> {
    static START: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *START.get_or_init(|| if crate::tools::flatpak().is_some() { None } else { started(std::process::id()).flatten() })
}

/// Stops the container `name` when its run is stopped or runs past its time: killed at once
/// (`rm -f` alone would wait the stop timeout, as a tool that is PID 1 ignores SIGTERM), then
/// removed (`rm -f`), not left to `--rm` alone, which a `run` that is killed never gets to.
pub fn stop(rt: &Path, name: &str) {
    for verb in [&["kill", name][..], &["rm", "-f", name]] {
        let _ = crate::tools::command(rt).args(verb).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// The process id and start of a container's name `name`, when it is one Coxswain gives:
/// `coxswain-<tool>-<pid>-<start>-<n>`, or an older version's `coxswain-<tool>-<pid>-<n>`
/// (start `None`).
fn parse_name(name: &str) -> Option<(u32, Option<u64>)> {
    let mut parts = name.strip_prefix("coxswain-")?.split('-');
    let tool = parts.next()?;
    let nums: Vec<&str> = parts.collect();
    let number = |s: &str| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit());
    if tool.is_empty() || !tool.bytes().all(|b| b.is_ascii_lowercase()) || !nums.iter().all(|n| number(n)) {
        return None;
    }
    let pid = nums.first()?.parse().ok()?;
    match nums.len() {
        2 => Some((pid, None)),
        3 => Some((pid, Some(nums[1].parse().ok()?))),
        _ => None,
    }
}

/// Whether the container `name`, in `state` as `ps` says it, is what a run of Coxswain's that
/// is gone left behind: the name is Coxswain's own, it does not run (in storage only, created,
/// exited: never a running one), and its process is gone or is another program under the same
/// id (it started at another time). An older version's name has no start, so it counts as gone
/// only while no process has its id: one that took the id keeps it for a later run. A process
/// whose start cannot be read is taken as alive. `started` as the function of that name.
fn leftover(name: &str, state: &str, started: &dyn Fn(u32) -> Option<Option<u64>>) -> bool {
    let idle = ["storage", "created", "configured", "exited", "stopped", "dead"].iter().any(|s| state.eq_ignore_ascii_case(s));
    match parse_name(name) {
        _ if !idle => false,
        None | Some((_, Some(0))) => false,
        Some((pid, Some(start))) => match started(pid) {
            None => true,
            Some(t) => t.is_some_and(|t| t != start),
        },
        Some((pid, None)) => started(pid).is_none(),
    }
}

/// The containers to remove in `ps`'s lines (`<id> <name> <state>`, the state's first word:
/// podman can say `Exited (0) 2 hours ago`, `Up 3 minutes`): their ids, and whether each
/// is in storage only (podman's `rm --storage`).
fn leftovers<'a>(ps: &'a str, started: &dyn Fn(u32) -> Option<Option<u64>>) -> Vec<(&'a str, bool)> {
    ps.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let (id, name, state) = (w.next()?, w.next()?, w.next()?);
            let id_ok = !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit());
            (id_ok && leftover(name, state, started)).then(|| (id, state.eq_ignore_ascii_case("storage")))
        })
        .collect()
}

/// Once a run and runtime, before its first container starts: removes the containers runs of
/// Coxswain's that are gone left behind (`leftover`), as a run killed or crashed never gets to
/// `--rm`. Podman then keeps them in its storage (`ps -a --external`, state `storage`), each
/// holding its image, so the image cannot be removed. Only names Coxswain gives, never another
/// container, image or anything else.
pub(crate) fn sweep(rt: &Path) {
    static DONE: Mutex<std::collections::BTreeSet<PathBuf>> = Mutex::new(std::collections::BTreeSet::new());
    // Never in this crate's unit tests: it would act on the runtime of the machine they run on.
    // Other tests reach a real runtime only in `--ignored` runs, and only leftovers go.
    if cfg!(test) || own_start().is_none() || !DONE.lock().unwrap_or_else(|e| e.into_inner()).insert(rt.to_path_buf()) {
        return;
    }
    let docker = rt.file_stem().is_some_and(|s| s.to_string_lossy().starts_with("docker"));
    let mut c = crate::tools::command(rt);
    c.args(["ps", "-a", "--format", "{{.ID}} {{.Names}} {{.State}}"]).stdin(Stdio::null());
    if !docker {
        c.arg("--external");
    }
    let Ok(out) = crate::tools::capture(c, std::time::Duration::from_secs(30), 4 << 20) else {
        return;
    };
    if !out.status.success() {
        return;
    }
    for (id, storage) in leftovers(&String::from_utf8_lossy(&out.stdout), &started) {
        let mut c = crate::tools::command(rt);
        c.arg("rm").args(storage.then_some("--storage")).arg(id).stdin(Stdio::null());
        let _ = crate::tools::capture(c, std::time::Duration::from_secs(30), 4096);
    }
}

/// When the process `pid` started, in the system's own unit: `None` when there is no such
/// process, `Some(None)` when there is one but its start cannot be read.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn started(pid: u32) -> Option<Option<u64>> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // Hidden (`hidepid`) is not gone: another user's process answers kill(0) with EPERM.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => unlisted(pid),
        Err(_) => Some(None),
        // Field 22, `starttime`, in clock ticks since boot; counted after the `(name)`.
        Ok(s) => Some(s.rfind(')').and_then(|i| s[i + 1..].split_whitespace().nth(19)?.parse().ok())),
    }
}

#[cfg(target_os = "macos")]
fn started(pid: u32) -> Option<Option<u64>> {
    let mut i: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&raw mut i).cast(), size) };
    if n == size {
        return Some(Some(i.pbi_start_tvsec * 1_000_000 + i.pbi_start_tvusec));
    }
    (std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)).then_some(None)
}

#[cfg(windows)]
fn started(pid: u32) -> Option<Option<u64>> {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            // No such process; any other refusal (access denied) is one that lives.
            return (GetLastError() != ERROR_INVALID_PARAMETER).then_some(None);
        }
        let mut code = 0u32;
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut made, mut e, mut k, mut u) = (z, z, z, z);
        let ended = GetExitCodeProcess(h, &mut code) != 0 && code != STILL_ACTIVE as u32;
        let ok = GetProcessTimes(h, &mut made, &mut e, &mut k, &mut u) != 0;
        CloseHandle(h);
        if ended {
            return None;
        }
        Some(ok.then(|| ((made.dwHighDateTime as u64) << 32) | made.dwLowDateTime as u64))
    }
}

/// `started` for a process the system does not list: gone only when kill(0) says there is no
/// such process. Hidden (Linux's `hidepid`, FreeBSD's `see_other_uids`) is not gone: another
/// user's process answers with EPERM.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos"))]
fn unlisted(pid: u32) -> Option<Option<u64>> {
    // Above i32::MAX kill() would ask for a process group: no process has such an id.
    if pid > i32::MAX as u32 {
        return None;
    }
    let gone = unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    (!gone).then_some(None)
}

/// The BSDs: the process's `kinfo_proc` (NetBSD's `kinfo_proc2`) by sysctl. NetBSD keeps the
/// start as the clock said at fork: microseconds since 1970, as on macOS. FreeBSD and OpenBSD
/// add the boot time when asked, which moves whenever the clock is set (ntpd, `date`), so a live
/// run would read another start than its name has: there it is counted from boot instead
/// (FreeBSD in microseconds, OpenBSD in milliseconds).
#[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
fn started(pid: u32) -> Option<Option<u64>> {
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    type Info = libc::kinfo_proc;
    #[cfg(target_os = "netbsd")]
    type Info = libc::kinfo_proc2;
    let size = std::mem::size_of::<Info>();
    let Ok(p) = libc::c_int::try_from(pid) else {
        return None;
    };
    #[cfg(target_os = "freebsd")]
    let (mib, end) = ([libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, p], std::mem::offset_of!(Info, ki_start) + std::mem::size_of::<libc::timeval>());
    #[cfg(target_os = "netbsd")]
    let (mib, end) = ([libc::CTL_KERN, libc::KERN_PROC2, libc::KERN_PROC_PID, p, size as libc::c_int, 1], std::mem::offset_of!(Info, p_ustart_usec) + 4);
    #[cfg(target_os = "openbsd")]
    let (mib, end) = ([libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, p, size as libc::c_int, 1], std::mem::offset_of!(Info, p_ustart_usec) + 4);
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    let boot = || {
        let mut t: libc::timeval = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::timeval>();
        let mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        let rc = unsafe { libc::sysctl(mib.as_ptr(), 2, (&raw mut t).cast(), &mut len, std::ptr::null_mut(), 0) };
        (rc == 0 && len == std::mem::size_of::<libc::timeval>()).then(|| t.tv_sec as i64 * 1_000_000 + t.tv_usec as i64)
    };
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    let before = boot();
    let mut i: Info = unsafe { std::mem::zeroed() };
    let mut len = size;
    let rc = unsafe { libc::sysctl(mib.as_ptr(), mib.len() as libc::c_uint, (&raw mut i).cast(), &mut len, std::ptr::null_mut(), 0) };
    if rc != 0 {
        // FreeBSD says ESRCH for no such process; NetBSD and OpenBSD answer with nothing.
        return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) { unlisted(pid) } else { Some(None) };
    }
    if len == 0 {
        return unlisted(pid);
    }
    // FreeBSD's kernel may fill a kinfo_proc of another size than libc's: only the start counts.
    if len < end {
        return Some(None);
    }
    // A record of another process (FreeBSD looks a large number up as a thread id), or one
    // without its start (exiting, or NetBSD's lock taken), tells nothing.
    #[cfg(target_os = "freebsd")]
    let (own, sec, usec) = (i.ki_pid, i.ki_start.tv_sec as i64, i.ki_start.tv_usec as i64);
    #[cfg(not(target_os = "freebsd"))]
    let (own, sec, usec) = (i.p_pid, i.p_ustart_sec as i64, i.p_ustart_usec as i64);
    #[cfg(target_os = "freebsd")]
    let valid = sec != 0;
    #[cfg(not(target_os = "freebsd"))]
    let valid = i.p_uvalid != 0;
    if own != p || !valid {
        return Some(None);
    }
    let start = sec * 1_000_000 + usec;
    // The clock set between the two reads: the boot time is not the one the start was made with.
    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    let start = match before {
        Some(b) if before == boot() => start - b,
        _ => return Some(None),
    };
    // shortcut: OpenBSD rounds start+boot to microseconds after adding, so the difference can
    // flip by 1 µs after a clock step; milliseconds make that rare, not impossible.
    #[cfg(target_os = "openbsd")]
    let start = start / 1000;
    Some(u64::try_from(start).ok())
}

/// illumos: `pr_start` of the process's `/proc/<pid>/psinfo`, in microseconds since 1970, as on
/// NetBSD and macOS.
#[cfg(target_os = "illumos")]
fn started(pid: u32) -> Option<Option<u64>> {
    match std::fs::read(format!("/proc/{pid}/psinfo")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => unlisted(pid),
        Err(_) => Some(None),
        Ok(b) => Some(psinfo_start(&b, pid)),
    }
}

/// `pr_start` of a 64-bit `psinfo_t` (libc has no such struct): `pr_pid` at byte 8, `pr_start`
/// (a `timestruc_t`: seconds, nanoseconds) at byte 88. `None` unless `pr_pid` is `pid`, so a
/// layout read wrong is never taken for a start.
#[cfg(any(target_os = "illumos", test))]
fn psinfo_start(b: &[u8], pid: u32) -> Option<u64> {
    let int = |at: usize| Some(i64::from_ne_bytes(b.get(at..at + 8)?.try_into().ok()?));
    let own = i32::from_ne_bytes(b.get(8..12)?.try_into().ok()?);
    if own as u32 != pid {
        return None;
    }
    let (sec, nsec) = (u64::try_from(int(88)?).ok()?, u64::try_from(int(96)?).ok()?);
    Some(sec * 1_000_000 + nsec / 1000)
}

/// shortcut: other systems are not read, so every process is taken as alive and nothing is
/// swept there; read their process start to sweep.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows,
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "illumos"
)))]
fn started(_pid: u32) -> Option<Option<u64>> {
    Some(None)
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
    sweep(rt);
    let key = (rt.to_path_buf(), image.to_string());
    if let Some(k) = BOUNDS.lock().unwrap().get(&key) {
        return *k;
    }
    let out = crate::tools::capture(
        {
            let mut c = crate::tools::command(rt);
            c.args(["run", "--rm", "--pull=never", "--network=none", "--memory=64m", "--pids-limit=16", "--name", &name("bounds"), "--entrypoint", "true", image]);
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

/// How many times *Stop* was pressed on the pulls of each image (`stop_pull`).
static STOPS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// *Stop* on the pulls of `image` under way, from a preview or Settings: each one ends as if
/// its own stop flag were set. A pull started later is not stopped.
pub fn stop_pull(image: &str) {
    *STOPS.lock().unwrap().entry(image.to_string()).or_default() += 1;
}

/// How many times *Stop* was pressed on `image`'s pulls: one that saw fewer at its start ends.
pub(crate) fn stops(image: &str) -> u64 {
    STOPS.lock().unwrap().get(image).copied().unwrap_or(0)
}

/// `pull`, which `stop` (or `stop_pull`) ends: the pull is killed. The runtime registers no
/// image until the whole of it is there; the layers it finished stay in its store, and the
/// part it was downloading, which podman keeps in its temporary folder even when it ends on
/// a signal, goes with that folder: the pull is given one of its own (`TMPDIR`) under the
/// cache's `pull/`, removed when it ends, and by the next start when the run was killed.
pub fn pull_until(rt: &Path, image: &str, stop: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let since = stops(image);
    let tmp = crate::helper::folder().map(|c| c.join("pull").join(format!("{}-{}", crate::spill::run(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))));
    PULLING.lock().unwrap().insert(image.to_string(), String::new());
    let result = (|| {
        let mut c = crate::tools::command(rt);
        c.args(["pull", image]);
        // Without a folder of its own (none could be made) it uses the runtime's own. Not without
        // XDG_RUNTIME_DIR: rootless podman would then keep its runroot under TMPDIR, which goes.
        if let Some(t) = tmp.as_ref().filter(|t| (cfg!(test) || std::env::var_os("XDG_RUNTIME_DIR").is_some()) && std::fs::create_dir_all(t).is_ok()) {
            c.env("TMPDIR", t);
        }
        let mut child = crate::tools::hosted(c)
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
            if stop.load(std::sync::atomic::Ordering::Relaxed) || stops(image) > since {
                // SIGTERM first, which flatpak-spawn passes on to the host's podman; then SIGKILL.
                #[cfg(unix)]
                {
                    // SAFETY: a signal to our own child, which has not been waited for yet.
                    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
                    for _ in 0..20 {
                        if child.try_wait().ok().flatten().is_some() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(t!("convert.pull_stopped", "image" => short(image)));
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
    if let Some(t) = tmp {
        let _ = std::fs::remove_dir_all(t);
    }
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
            // Only while the pull is under way: a reader that outlives a stopped one adds nothing.
            if let Some(l) = PULLING.lock().unwrap().get_mut(image) {
                *l = last.clone();
            }
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

/// Days between two rounds for `[preview] image_updates`; none when off. "weekly" and
/// "monthly" pull what is newer; "ask" and "ask-monthly" only look, and ask (`asks`).
pub fn every(updates: &str) -> Option<u64> {
    match updates {
        "weekly" | "ask" => Some(7),
        "monthly" | "ask-monthly" => Some(30),
        _ => None,
    }
}

/// Does `[preview] image_updates` look for newer builds and ask, instead of pulling them?
pub fn asks(updates: &str) -> bool {
    matches!(updates, "ask" | "ask-monthly")
}

const ACCEPT: &str = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";

/// (registry host, repository, tag) of an image name whose registry it writes out
/// (`docker.io/…`, `ghcr.io/…`); none for one pinned by a digest, which a pull fetches exactly
/// and no newer build replaces. None too for a short name (`alpine`, `pandoc/core`), which
/// podman may resolve through registries.conf to a private registry, and for `localhost`: their
/// name is never sent to a registry that may not be theirs.
fn reference(image: &str) -> Option<(String, String, String)> {
    if image.contains('@') {
        return None;
    }
    let (host, repo) = image.split_once('/')?;
    // The host without its port: `localhost` and the loopback addresses are this machine's own.
    let bare = host.rsplit_once(':').filter(|(h, p)| !h.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).map_or(host, |(h, _)| h);
    let own = bare.eq_ignore_ascii_case("localhost") || bare.starts_with("127.") || bare == "[::1]";
    if repo.is_empty() || !(host.contains('.') || host.contains(':')) || own {
        return None;
    }
    let (host, repo) = (host.to_string(), repo.to_string());
    let slash = repo.rfind('/').map_or(0, |i| i + 1);
    let (repo, tag) = match repo[slash..].find(':') {
        Some(c) => (repo[..slash + c].to_string(), repo[slash + c + 1..].to_string()),
        None => (repo, "latest".to_string()),
    };
    let (host, repo) = if host == "docker.io" || host == "index.docker.io" {
        ("registry-1.docker.io".to_string(), if repo.contains('/') { repo } else { format!("library/{repo}") })
    } else {
        (host, repo)
    };
    Some((host, repo, tag))
}

/// The digest the registry says the tag of `image` has now: an anonymous `HEAD` of its manifest
/// (with the anonymous pull token the registry asks for), nothing else leaves the machine.
/// None: no answer (offline). Some(None): answered without a digest (pinned, refused).
pub fn remote_digest(image: &str) -> Option<Option<String>> {
    let Some((host, repo, tag)) = reference(image) else { return Some(None) };
    let agent = crate::meaning::agent(|b| b.timeout_global(Some(std::time::Duration::from_secs(30))).http_status_as_error(false));
    let url = format!("https://{host}/v2/{repo}/manifests/{tag}");
    let head = |token: Option<&str>| {
        let mut req = agent.head(&url).header("Accept", ACCEPT);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        req.call().ok()
    };
    // No answer at all (offline) is None; any answer, even a refusal, is one.
    let mut res = head(None)?;
    let got = digest_of(&agent, &mut res, &repo, &head);
    Some(got)
}

fn digest_of(agent: &ureq::Agent, res: &mut ureq::http::Response<ureq::Body>, repo: &str, head: &dyn Fn(Option<&str>) -> Option<ureq::http::Response<ureq::Body>>) -> Option<String> {
    let mut second;
    let mut res = res;
    if res.status().as_u16() == 401 {
        let challenge = res.headers().get("www-authenticate")?.to_str().ok()?.to_string();
        // Only an anonymous bearer token over https; anything else is not asked.
        if !challenge.starts_with("Bearer ") {
            return None;
        }
        let field = |k: &str| {
            let at = challenge.find(&format!("{k}=\""))? + k.len() + 2;
            challenge[at..].split('"').next().map(str::to_string)
        };
        let (realm, service) = (field("realm").filter(|r| r.starts_with("https://"))?, field("service").unwrap_or_default());
        let got = agent.get(&realm).query("service", &service).query("scope", format!("repository:{repo}:pull")).call().ok()?;
        let body: serde_json::Value = serde_json::from_str(&got.into_body().read_to_string().ok()?).ok()?;
        let token = body["token"].as_str().or_else(|| body["access_token"].as_str())?.to_string();
        second = head(Some(&token))?;
        res = &mut second;
    }
    if !res.status().is_success() {
        return None;
    }
    Some(res.headers().get("docker-content-digest")?.to_str().ok()?.to_string())
}

/// Is `image` one a check asks its registry about (`reference`)?
pub fn checkable(image: &str) -> bool {
    reference(image).is_some()
}

/// The digests the runtime has for `image` (`RepoDigests`).
pub fn local_digests(rt: &Path, image: &str) -> Vec<String> {
    let Ok(out) = crate::tools::command(rt).args(["image", "inspect", "--format", "{{json .RepoDigests}}", image]).stdin(Stdio::null()).output() else {
        return vec![];
    };
    let all: Vec<String> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    all.iter().filter_map(|d| d.split_once('@').map(|(_, d)| d.to_string())).collect()
}

/// The look of a round that only asks: image -> the build that is newer than the pulled copy
/// (the registry's digest; "pin" for the new pin of a default whose older build is pulled), and
/// whether the round is done. `pins` are the `superseded` pairs whose old copy is pulled. Only the registry is asked, once per unpinned image; one with no
/// digest on either side is not called newer, and a round where every lookup failed stays due.
pub fn check(
    pulled: &[String],
    pins: &[(String, String)],
    local: impl Fn(&str) -> Vec<String>,
    remote: impl Fn(&str) -> Option<Option<String>>,
) -> (BTreeMap<String, String>, bool) {
    let mut found = BTreeMap::new();
    let mut asked = 0;
    let mut answered = 0;
    for image in pulled.iter().filter(|i| checkable(i)) {
        asked += 1;
        if let Some(answer) = remote(image) {
            answered += 1;
            let Some(d) = answer else { continue };
            let have = local(image);
            if !have.is_empty() && !have.contains(&d) {
                found.insert(image.clone(), d);
            }
        }
    }
    for (_, new) in pins {
        found.insert(new.clone(), "pin".to_string());
    }
    (found, asked == 0 || answered > 0)
}

/// `check` on what the runtime `rt` has: the digest lookups are the only network use.
pub fn check_now(cfg: &PreviewConfig, st: &AppState, rt: &Path) -> (BTreeMap<String, String>, bool) {
    let pins: Vec<(String, String)> = superseded(cfg, st).into_iter().filter(|(old, _)| crate::tools::image_size(rt, old).is_some()).collect();
    check(&pulled(cfg, rt), &pins, |i| local_digests(rt, i), remote_digest)
}

/// The ask mode's round when it is due (`update_due` with `asks`), never on battery and never
/// with containers off: the newer builds to offer, when it is done. None: not now.
pub fn ask_round(cfg: &PreviewConfig, st: &AppState, now: u64) -> Option<BTreeMap<String, String>> {
    if !asks(&cfg.image_updates) || !update_due(cfg, st, now) || crate::machine::on_battery() {
        return None;
    }
    let (_, rt) = crate::tools::container_runtime(cfg)?;
    let (found, done) = check_now(cfg, st, &rt);
    done.then_some(found)
}

/// The terminal app's side of the ask mode: once an hour (the first time after ten minutes) a
/// thread looks when a round is due, and keeps what it found in the state file for the notice.
pub fn watch_in_terminal() {
    std::thread::spawn(|| {
        let mut wait = 600;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            wait = 3600;
            let Ok(cfg) = crate::config::Config::load() else { continue };
            let now = crate::fs::now();
            let st = AppState::load();
            if let Some(found) = ask_round(&cfg.preview, &st, now) {
                // Read again: the desktop app shares the file, and the look took a while.
                let mut again = AppState::load();
                if keep_found(&mut again, st.images_checked, found, now) {
                    let _ = again.save();
                }
            }
        }
    });
}

/// Keep what a round found in `st` (read again just now), unless another round was kept since
/// this one read `checked_before` (the other app's): then its finding stands. Whether `st` changed.
pub fn keep_found(st: &mut AppState, checked_before: u64, found: BTreeMap<String, String>, now: u64) -> bool {
    if st.images_checked != checked_before {
        return false;
    }
    (st.images_newer, st.images_checked) = (found, now);
    true
}

/// `image` was pulled to its newest (Update, Pull, **P**): no newer build waits for it, and its
/// notice is dismissed. Whether `st` changed.
pub fn updated(st: &mut AppState, image: &str) -> bool {
    let Some(build) = st.images_newer.remove(image) else { return false };
    crate::notices::dismiss(st, &crate::notices::image_id(image, &build));
    true
}

/// *Update* on a notice: `image` is pulled, and the copy it replaced removed (`drop_old`; for
/// the new pin of a default, the old pin's copy). The old images done with are returned, to be
/// forgotten.
pub fn apply(rt: &Path, image: &str, pins: &[(String, String)], pull: impl Fn(&str) -> Result<(), String>) -> Result<Vec<String>, String> {
    let mine: Vec<&(String, String)> = pins.iter().filter(|(_, new)| new == image).collect();
    if mine.is_empty() {
        update(rt, image, || pull(image))?;
        return Ok(vec![]);
    }
    let mut gone = vec![];
    for (old, new) in mine {
        if crate::tools::image_size(rt, old).is_some() {
            pull(new)?;
            // Not removed now (a container uses it): kept in mind for the next round.
            if drop_pin(rt, old, new).is_err() {
                continue;
            }
        }
        gone.push(old.clone());
    }
    Ok(gone)
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
/// pulled and the old copy removed (`drop_old`). The old images done with are returned, to be
/// forgotten: those removed, and those the runtime no longer has. A pull that fails keeps the
/// old copy for the next round.
pub fn move_pins(rt: &Path, pairs: &[(String, String)], pull: impl Fn(&str) -> Result<(), String>) -> Vec<String> {
    pairs
        .iter()
        .filter(|(old, new)| crate::tools::image_size(rt, old).is_none() || (pull(new).is_ok() && drop_pin(rt, old, new).is_ok()))
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

/// Update `image` with `pull`; when that brought a new copy, the one it replaced is removed at
/// once (`drop_old`). A copy that cannot go (a container still uses it, another name points at
/// it) stays, quietly.
pub fn update(
    rt: &Path,
    image: &str,
    pull: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let old = id(rt, image);
    pull()?;
    if let Some(old) = replaced(old, id(rt, image)) {
        let _ = drop_old(rt, &old, image);
    }
    Ok(())
}

/// The old pin `old`'s copy after `new` was pulled, by `drop_old`; nothing when both pins name
/// the same copy (only the digest moved), which removing `old` could take with it.
fn drop_pin(rt: &Path, old: &str, new: &str) -> Result<(), String> {
    match replaced(id(rt, old), id(rt, new)) {
        Some(_) => drop_old(rt, old, old),
        None => Ok(()),
    }
}

/// Remove the old copy `what` (an id or a pin's name) that `ours`, the name Coxswain pulled it
/// as, no longer needs, looking at its names as they are now, after the pull: with none left
/// it goes by its id; with only `ours` it goes by that name; any other name (one the user
/// gave it) keeps it whole. Never forced, so a copy a container uses stays too.
fn drop_old(rt: &Path, what: &str, ours: &str) -> Result<(), String> {
    let out = crate::tools::command(rt)
        .args(["image", "inspect", "--format", "{{json .RepoTags}}", what])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let said = String::from_utf8_lossy(&out.stdout);
    let tags: Vec<String> = match serde_json::from_str::<Option<Vec<String>>>(said.trim()) {
        Ok(t) if out.status.success() => t.unwrap_or_default(),
        _ => return Err(format!("{what}: its names could not be read")),
    };
    match dropped_by(&tags, ours) {
        Dropped::Kept => Ok(()),
        Dropped::Id => remove(rt, what),
        Dropped::Names(names) => names.iter().try_for_each(|t| remove(rt, t)),
    }
}

#[derive(Debug, PartialEq)]
enum Dropped {
    /// Another name points at it: the copy is the user's now.
    Kept,
    /// No name left at all.
    Id,
    /// Only Coxswain's own name: removed by it.
    Names(Vec<String>),
}

/// How an old copy with the names `tags` goes, when `ours` is the name Coxswain pulled it as.
// shortcut: a name by digest only (`repo@sha256:…`) is not in RepoTags, so a copy with only
// that left goes by its id; it came from a registry and can be pulled again.
fn dropped_by(tags: &[String], ours: &str) -> Dropped {
    match tags {
        [] => Dropped::Id,
        _ if tags.iter().all(|t| full(t) == full(ours)) => Dropped::Names(tags.to_vec()),
        _ => Dropped::Kept,
    }
}

/// `name:tag` written out as podman lists it, so docker's short `alpine:3` and podman's
/// `docker.io/library/alpine:3` compare equal; a digest is left off, a missing tag is `latest`.
fn full(name: &str) -> String {
    let name = short(name);
    let first = name.split('/').next().unwrap_or_default();
    let host = name.contains('/') && (first.contains(['.', ':']) || first == "localhost");
    let mut s = if host { name.to_string() } else if name.contains('/') { format!("docker.io/{name}") } else { format!("docker.io/library/{name}") };
    if !s[s.rfind('/').map_or(0, |i| i + 1)..].contains(':') {
        s.push_str(":latest");
    }
    s
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
    /// Every tool's name parses back to this process; names Coxswain never gives do not.
    #[test]
    fn container_names_are_coxswains_own_and_parse_back() {
        use super::{name, parse_name};
        let tools = ["stockfish", "calibre", "export", "media", "probe", "ocr", "clamscan", "freshclam", "whisper", "preview", "bounds"];
        for tool in tools {
            let n = name(tool);
            assert!(n.starts_with(&format!("coxswain-{tool}-{}-", std::process::id())), "{n}");
            let (pid, start) = parse_name(&n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(pid, std::process::id());
            assert!(start.is_some(), "{n}");
        }
        assert_eq!(parse_name("coxswain-stockfish-4242-0"), Some((4242, None)));
        assert_eq!(parse_name("coxswain-stockfish-4242-77-3"), Some((4242, Some(77))));
        for other in [
            "coxswain-tika-1234abcd",
            "coxswain-tika-12345678",
            "coxswain-tika-test-4242",
            "coxswain-preview-out.pdf",
            "coxswain-stockfish-4242",
            "coxswain-stockfish-4242-1-2-3",
            "coxswain-Stockfish-4242-0",
            "coxswain-stock-fish-4242-0-1",
            "coxswain--4242-0",
            "coxswain-stockfish-42a-0",
            "coxswain-stockfish--0",
            "mycoxswain-stockfish-4242-0",
            "stockfish-4242-0",
            "nginx",
            "",
        ] {
            assert_eq!(parse_name(other), None, "{other}");
        }
    }

    /// Only an idle container of Coxswain's whose run is gone is a leftover: never a running
    /// one, another name, a live run's, or one whose start (0) was not known.
    #[test]
    fn only_idle_containers_of_runs_that_are_gone_are_leftovers() {
        use super::{leftover, leftovers};
        // pid 10 lives, started at 500; 11 lives, start unreadable; every other pid is gone.
        let started = |pid: u32| match pid {
            10 => Some(Some(500)),
            11 => Some(None),
            _ => None,
        };
        let is = |n: &str, s: &str| leftover(n, s, &started);
        assert!(is("coxswain-stockfish-20-500-0", "storage"));
        assert!(is("coxswain-stockfish-20-500-0", "Exited"));
        assert!(is("coxswain-ocr-20-500-3", "created"));
        // The pid is another program's now: it started at another time.
        assert!(is("coxswain-stockfish-10-400-0", "storage"));
        // The live run's own, idle or not.
        assert!(!is("coxswain-stockfish-10-500-0", "storage"));
        assert!(!is("coxswain-stockfish-10-500-0", "created"));
        // Its start cannot be read: taken as alive.
        assert!(!is("coxswain-stockfish-11-400-0", "storage"));
        // Running, paused or on its way: never, whoever made it.
        for s in ["running", "paused", "stopping", "removing", "restarting", "unknown", ""] {
            assert!(!is("coxswain-stockfish-20-500-0", s), "{s}");
        }
        // Start 0: made where it could not be read (a Flatpak): never.
        assert!(!is("coxswain-stockfish-20-0-0", "storage"));
        // An older version's name: gone only when no process has its pid.
        assert!(is("coxswain-stockfish-20-0", "storage"));
        assert!(!is("coxswain-stockfish-10-0", "storage"));
        assert!(!is("coxswain-stockfish-11-0", "storage"));
        // Not Coxswain's.
        assert!(!is("my-stockfish-20-500-0", "storage"));
        assert!(!is("coxswain-tika-1234abcd", "exited"));

        let ps = "\
abc123 coxswain-stockfish-20-500-0 storage
def456 coxswain-media-20-500-1 exited
0a0a0a coxswain-stockfish-10-500-0 storage
111111 coxswain-whisper-20-500-2 running
222222 someone-else storage
333333 coxswain-tika-1234abcd exited
--rm coxswain-stockfish-20-500-9 storage
444444 coxswain-stockfish-20-500-8 storage extra
555555 coxswain-calibre-20-0 created
666666 coxswain-media-20-500-4 Exited (0) 2 hours ago
777777 coxswain-media-20-500-5 Up 3 minutes
888888 coxswain-ocr-20-500-6 Created
";
        assert_eq!(
            leftovers(ps, &started),
            vec![("abc123", true), ("def456", false), ("444444", true), ("555555", false), ("666666", false), ("888888", false)]
        );
    }

    /// This process is alive by `started`, with a start, and one that cannot exist is gone.
    /// Where the start is microseconds since 1970 (macOS, NetBSD, illumos), it is before now.
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        windows,
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "illumos"
    ))]
    #[test]
    fn this_process_has_a_start_and_a_missing_one_is_gone() {
        let me = super::started(std::process::id());
        assert!(matches!(me, Some(Some(t)) if t > 0), "{me:?}");
        assert_eq!(me, super::started(std::process::id()));
        assert_eq!(super::started(u32::MAX - 1), None);
        #[cfg(any(target_os = "macos", target_os = "netbsd", target_os = "illumos"))]
        {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64;
            let t = me.flatten().unwrap();
            assert!(t <= now && now - t < 86_400_000_000 * 365, "start {t}, now {now}");
        }
    }

    /// A child started after this process has a start no earlier than this one's; once it has
    /// ended and been waited for, it is gone.
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos"))]
    #[test]
    fn a_child_starts_later_and_is_gone_once_ended() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        // A just-forked process may not have its start yet (NetBSD's `p_uvalid` is 0 until the
        // fork is done): wait by state until it is known, `started` itself keeps saying unknown.
        let limit = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(10));
        let mut it = super::started(child.id());
        while matches!(it, Some(None)) && std::time::Instant::now() < limit {
            std::thread::sleep(std::time::Duration::from_millis(1));
            it = super::started(child.id());
        }
        let me = super::started(std::process::id()).flatten().unwrap();
        assert!(matches!(it, Some(Some(t)) if t >= me), "{it:?} against {me}");
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(super::started(child.id()), None);
    }

    /// psinfo's `pr_start` is read at its place, and only for the process it names.
    #[test]
    fn psinfo_start_is_read_for_its_own_process_only() {
        let mut b = vec![0u8; 416];
        b[8..12].copy_from_slice(&42i32.to_ne_bytes());
        b[88..96].copy_from_slice(&1_700_000_000i64.to_ne_bytes());
        b[96..104].copy_from_slice(&250_000_999i64.to_ne_bytes());
        assert_eq!(super::psinfo_start(&b, 42), Some(1_700_000_000_250_000));
        assert_eq!(super::psinfo_start(&b, 43), None);
        assert_eq!(super::psinfo_start(&b[..100], 42), None);
    }

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
    fn asking_rounds_come_weekly_or_monthly_and_only_ask() {
        assert!(asks("ask") && asks("ask-monthly") && !asks("weekly") && !asks("off"));
        assert_eq!((every("ask"), every("ask-monthly")), (Some(7), Some(30)));
        let (mut cfg, st) = (PreviewConfig::default(), AppState::default());
        cfg.image_updates = "ask".into();
        assert!(update_due(&cfg, &st, 100 * DAY));
    }

    #[test]
    fn references_are_read_as_a_pull_reads_them() {
        let r = |i| reference(i);
        assert_eq!(r("docker.io/pandoc/core:latest"), Some(("registry-1.docker.io".into(), "pandoc/core".into(), "latest".into())));
        assert_eq!(r("docker.io/alpine"), Some(("registry-1.docker.io".into(), "library/alpine".into(), "latest".into())));
        assert_eq!(r("index.docker.io/alpine:3"), Some(("registry-1.docker.io".into(), "library/alpine".into(), "3".into())));
        assert_eq!(r("docker.io/library/alpine:3"), Some(("registry-1.docker.io".into(), "library/alpine".into(), "3".into())));
        assert_eq!(r("ghcr.io/mwo-dk/coxswain-magick:stable"), Some(("ghcr.io".into(), "mwo-dk/coxswain-magick".into(), "stable".into())));
        assert_eq!(r("registry.example:5000/x/y"), Some(("registry.example:5000".into(), "x/y".into(), "latest".into())));
        assert_eq!(r("docker.io/pandoc/core:latest@sha256:abc"), None, "a pinned image has no newer build to ask for");
    }

    #[test]
    fn a_name_without_its_registry_or_on_localhost_is_never_sent_anywhere() {
        // podman may resolve a short name through registries.conf to a private registry: its
        // name must not go to docker.io, nor raise a false newer build from there.
        for local in ["alpine", "alpine:3", "pandoc/core:latest", "me/private", "localhost/x", "localhost/x:1", "localhost:5000/x/y", "LOCALHOST:5000/x", "127.0.0.1:5000/x", "[::1]:5000/x"] {
            assert_eq!(reference(local), None, "{local}");
            assert!(!checkable(local), "{local}");
            assert_eq!(remote_digest(local), Some(None), "{local}: answered here, nothing sent");
        }
        let pulled = ["alpine:3", "localhost/x:1", "docker.io/library/alpine:3"].map(String::from);
        let asked = std::cell::RefCell::new(vec![]);
        let (found, done) = check(&pulled, &[], |_| vec!["sha256:old".into()], |i| {
            asked.borrow_mut().push(i.to_string());
            Some(Some("sha256:new".into()))
        });
        assert_eq!(asked.into_inner(), ["docker.io/library/alpine:3"]);
        assert_eq!(found.keys().collect::<Vec<_>>(), ["docker.io/library/alpine:3"]);
        assert!(done);
        assert!(check(&["localhost/x:1".to_string()], &[], |_| vec![], |_| panic!("asked")).1, "nothing to ask is done");
    }

    #[test]
    fn a_round_kept_by_the_other_app_meanwhile_is_not_written_over() {
        let mut st = AppState { images_checked: 5, ..AppState::default() };
        let found = BTreeMap::from([("docker.io/a:1".to_string(), "sha256:n".to_string())]);
        assert!(!keep_found(&mut st, 4, found.clone(), 9), "the other app kept a round at 5");
        assert!(st.images_newer.is_empty() && st.images_checked == 5);
        assert!(keep_found(&mut st, 5, found.clone(), 9));
        assert_eq!((st.images_newer, st.images_checked), (found, 9));
    }

    #[test]
    fn a_check_names_the_images_whose_registry_digest_is_not_the_pulled_one() {
        let pulled = ["docker.io/a:1", "docker.io/b:1", "docker.io/c:1", "docker.io/d:1@sha256:x"].map(String::from);
        let local = |i: &str| if i == "docker.io/c:1" { vec![] } else { vec!["sha256:old".to_string()] };
        let remote = |i: &str| match i {
            "docker.io/a:1" => Some(Some("sha256:new".to_string())),
            "docker.io/b:1" => Some(Some("sha256:old".to_string())),
            "docker.io/c:1" => Some(Some("sha256:new".to_string())),
            _ => panic!("a pinned image is never asked about: {i}"),
        };
        let pins = [("old".to_string(), "p:1@sha256:p".to_string())];
        let (found, done) = check(&pulled, &pins, local, remote);
        assert_eq!(found.keys().map(String::as_str).collect::<Vec<_>>(), ["docker.io/a:1", "p:1@sha256:p"], "b is current, c has no digest to compare");
        assert_eq!(found["docker.io/a:1"], "sha256:new");
        assert!(done);
        // Every lookup failed (offline): not done, so the round stays due.
        assert!(!check(&pulled, &[], local, |_| None).1);
        // A refusal is an answer: the round is done and not asked again every hour.
        assert!(check(&["docker.io/p:1".to_string()], &[], local, |_| Some(None)).1);
        assert!(check(&[], &[], local, |_| None).1, "nothing to look at is done");
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
    fn images_an_old_copy_goes_only_when_no_other_name_points_at_it() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dropped_by(&[], "docker.io/library/alpine:3"), Dropped::Id, "no name left: by its id");
        assert_eq!(dropped_by(&t(&["docker.io/library/alpine:3.19"]), "docker.io/library/alpine:3"), Dropped::Kept, "the user's own name keeps it");
        assert_eq!(dropped_by(&t(&["alpine:3", "mine:1"]), "docker.io/library/alpine:3"), Dropped::Kept, "ours and another: kept");
        assert_eq!(dropped_by(&t(&["alpine:1"]), "b/x:1"), Dropped::Kept);
        assert_eq!(dropped_by(&t(&["alpine:3.19"]), "docker.io/library/alpine:3.19@sha256:old"), Dropped::Names(t(&["alpine:3.19"])), "only ours: by that name");
        assert_eq!(full("alpine"), "docker.io/library/alpine:latest");
        assert_eq!(full("user/x:1@sha256:a"), "docker.io/user/x:1");
        assert_eq!(full("localhost:5000/x"), "localhost:5000/x:latest");
        assert_eq!(full("ghcr.io/a/b:2"), "ghcr.io/a/b:2");
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

    /// Stop on an image ends its pull (inside a job here): the runtime is killed, the pull says it was stopped, and
    /// the temporary folder it was given (`TMPDIR`), with what it downloaded there, is gone. A
    /// pull started after the press runs to its end.
    #[cfg(unix)]
    #[test]
    fn images_stop_pull_ends_it_and_removes_its_folder() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-test-stop-pull-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rt = d.join("podman");
        // `slow:1` downloads into its TMPDIR until killed; anything else is pulled at once.
        std::fs::write(&rt, format!("#!/bin/sh
[ \"$2\" = slow:1 ] || exit 0
echo \"$TMPDIR\" > {0}/tmpdir
echo part > \"$TMPDIR/blob\"
echo 'Copying blob 1'
exec sleep 60
", d.display())).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).unwrap();
        let image = "slow:1";
        let stopper = std::thread::spawn({
            let d = d.clone();
            move || {
                let start = std::time::Instant::now();
                while !d.join("tmpdir").exists() || !pulling().contains_key(image) {
                    assert!(start.elapsed() < crate::test_limit(std::time::Duration::from_secs(30)), "the pull never started");
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                stop_pull(image);
            }
        });
        let r = pulls::in_job(&rt, image, &std::sync::atomic::AtomicBool::new(false));
        stopper.join().unwrap();
        assert_eq!(r, Err(t!("convert.pull_stopped", "image" => image)));
        assert_eq!(pulls::failure(image), None, "a stopped pull is no failure to try again");
        let tmp = std::fs::read_to_string(d.join("tmpdir")).unwrap();
        let tmp = Path::new(tmp.trim());
        assert!(tmp.ends_with(Path::new("pull").join(tmp.file_name().unwrap())) && tmp.to_string_lossy().contains(crate::spill::run()), "{tmp:?}");
        assert!(!tmp.exists(), "the part downloaded goes with its folder");
        assert!(!pulling().contains_key(image));
        assert_eq!(pull_until(&rt, "quick:1", &std::sync::atomic::AtomicBool::new(false)), Ok(()));
        stop_pull("quick:1");
        assert_eq!(pull_until(&rt, "quick:1", &std::sync::atomic::AtomicBool::new(false)), Ok(()), "a press before the pull does not stop it");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A runtime that has the images named in `has` and writes what it removes to `rm.log`.
    #[cfg(unix)]
    #[test]
    fn images_move_pins_pulls_the_new_and_removes_the_old() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-test-pins-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rt = d.join("podman");
        std::fs::write(&rt, format!("#!/bin/sh\ncase \"$1 $2\" in\n'image inspect') grep -qxF \"$5\" {0}/has || exit 1; case \"$4\" in *RepoTags*) echo '[]';; *Id*) echo \"$5\";; *) echo 1;; esac;;\n'image rm') echo \"$3\" >> {0}/rm.log;;\nesac\n", d.display())).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.join("has"), "a:1@sha256:old\nb:1\n").unwrap();
        let pairs = [("a:1@sha256:old", "a:1@sha256:new"), ("b:1", "b:1@sha256:new"), ("c:1@sha256:gone", "c:1@sha256:new")].map(|(o, n)| (o.to_string(), n.to_string()));
        let pulled = std::cell::RefCell::new(vec![]);
        let done = move_pins(&rt, &pairs, |i| {
            pulled.borrow_mut().push(i.to_string());
            if i.starts_with("b:") { return Err("offline".into()) }
            let mut has = std::fs::read_to_string(d.join("has")).unwrap();
            has.push_str(&format!("{i}\n"));
            std::fs::write(d.join("has"), has).unwrap();
            Ok(())
        });
        assert_eq!(*pulled.borrow(), ["a:1@sha256:new", "b:1@sha256:new"], "nothing pulled for a copy not there");
        assert_eq!(done, ["a:1@sha256:old", "c:1@sha256:gone"], "a failed pull keeps the old copy");
        assert_eq!(std::fs::read_to_string(d.join("rm.log")).unwrap(), "a:1@sha256:old\n");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The hand test of 2.38.0: the user tagged their `alpine:3.19` as `alpine:3`, the name a
    /// tool is set to. Updating `alpine:3` moves that name to the new copy; the old one, left
    /// with the user's `alpine:3.19` only, must stay. With no name left it goes by its id.
    #[cfg(unix)]
    #[test]
    fn images_update_keeps_an_old_copy_another_name_points_at() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-test-update-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rt = d.join("podman");
        // ids/<name> and tags/<name or id>: what inspect says; rm logs its argument.
        // Termux has no /bin/sh: the shell is under $PREFIX.
        let sh = std::env::var_os("PREFIX").map(|p| Path::new(&p).join("bin/sh")).filter(|p| p.exists()).unwrap_or_else(|| "/bin/sh".into());
        std::fs::write(&rt, format!("#!{1}\nf=$(echo \"$5\" | tr / _)\ncase \"$1 $2 $4\" in\n'image inspect {{{{.Id}}}}') cat {0}/ids/$f;;\n'image inspect {{{{json .RepoTags}}}}') cat {0}/tags/$f;;\n'image rm'*) echo \"$3\" >> {0}/rm.log;;\nesac\n", d.display(), sh.display())).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).unwrap();
        let w = |f: &str, s: &str| std::fs::write(d.join(f), s).unwrap();
        let rm = || std::fs::read_to_string(d.join("rm.log")).unwrap_or_default();
        let image = "docker.io/library/alpine:3";
        for f in ["ids", "tags"] { std::fs::create_dir_all(d.join(f)).unwrap() }
        w("ids/docker.io_library_alpine:3", "old\n");
        update(&rt, image, || {
            w("ids/docker.io_library_alpine:3", "new\n");
            w("tags/old", "[\"docker.io/library/alpine:3.19\"]\n");
            Ok(())
        }).unwrap();
        assert_eq!(rm(), "", "the user's alpine:3.19 is kept");
        w("ids/docker.io_library_alpine:3", "old\n");
        update(&rt, image, || {
            w("ids/docker.io_library_alpine:3", "new\n");
            w("tags/old", "null\n");
            Ok(())
        }).unwrap();
        assert_eq!(rm(), "old\n", "no name left: removed by its id");
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
        PULLING.lock().unwrap().insert("t".into(), String::new());
        assert_eq!(follow("one\ntwo\rthree".as_bytes(), "t"), "two");
        assert_eq!(pulling().get("t").map(String::as_str), Some("two"));
        PULLING.lock().unwrap().remove("t");
        // A reader that outlives its pull (stopped) adds no line that would stay for good.
        assert_eq!(follow("late\n".as_bytes(), "t"), "late");
        assert_eq!(pulling().get("t"), None);
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

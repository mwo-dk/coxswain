//! An image that cannot be pulled: why, in words a user knows, when it is tried again, and the
//! loop that tries (`until_there`). Shared by every image Coxswain pulls in the background
//! (Apache Tika, calibre for the search): a passing fault is tried again after 5, 10, 20 and 40
//! minutes, then every hour; a lasting one waits, Off, with a plain reason; an image pulled by
//! hand is noticed within a minute; the runtime's own error goes to helper.log and, for
//! *Details*, to the apps.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::t;

/// What Settings and the status lines say of an image's tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The image is downloading: the runtime's last progress line.
    Pulling(String),
    /// The image is there: files are read.
    Ready,
    /// Neither podman nor docker is installed (or `[preview] container` is off).
    NoRuntime,
    /// The image could not be pulled for a passing fault: it is tried again at `at` (seconds
    /// since 1970). `details`: the runtime's own error.
    Waiting {
        fault: Fault,
        at: u64,
        #[serde(default)]
        details: String,
    },
    /// The image could not be pulled, and trying again by itself would not help: why.
    Unpulled {
        fault: Fault,
        #[serde(default)]
        details: String,
    },
    /// The tool would not start: why.
    Failed(String),
}

/// The first version whose apps know `State::Waiting` and `State::Unpulled`.
const STATES_SINCE: &str = "2.36.0";

impl State {
    /// The state as an app of `version` can read it: before `STATES_SINCE`, a pull that failed
    /// is a `Failed` with its words, as that app knew it. `name`: the tool, in those words.
    pub fn for_app(self, version: &str, name: &str) -> State {
        if !crate::helper::older(version, STATES_SINCE) {
            return self;
        }
        match self {
            State::Waiting { fault, at, .. } => State::Failed(t!("settings.status.pull_again", "name" => name, "fault" => fault.text(), "time" => crate::i18n::time(at))),
            State::Unpulled { fault, .. } => State::Failed(t!("settings.status.pull_unpulled", "name" => name, "fault" => fault.text())),
            s => s,
        }
    }

    /// What Settings → Previews says of an image a job could not pull; None for other states.
    pub fn job_line(&self) -> Option<String> {
        match self {
            State::Waiting { fault, at, .. } => Some(t!("ptools.pull_waiting", "fault" => fault.job_text(), "time" => crate::i18n::time(*at))),
            State::Unpulled { fault, .. } => Some(t!("ptools.pull_unpulled", "fault" => fault.job_text())),
            _ => None,
        }
    }

    /// The runtime's own error, when the state has one to show.
    pub fn details(&self) -> Option<&str> {
        match self {
            State::Waiting { details, .. } | State::Unpulled { details, .. } if !details.is_empty() => Some(details),
            _ => None,
        }
    }
}

/// Why a pull failed, in words a user knows; the runtime's own error goes to helper.log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// No name lookup, no connection, a timeout, the registry busy or failing for a while.
    Network,
    /// No room for the image.
    Space,
    /// The registry has no such image, or will not give it.
    Gone,
    /// Anything else the runtime said.
    Other,
}

impl Fault {
    /// The fault a runtime's error tells, by its words (podman's and docker's are English).
    pub fn of(error: &str) -> Fault {
        let e = error.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
        if has(&["no space left", "disk quota"]) {
            Fault::Space
        // The registry's refusal only: a plain "permission denied" (rootless podman's own storage
        // or socket) is the machine's, and tried again.
        } else if has(&["manifest unknown", "name unknown", "not found", "unauthorized", "denied: requested access", "access denied"]) {
            Fault::Gone
        } else if has(&[
            "name resolution", "no such host", "lookup ", "dial tcp", "connection refused", "connection reset",
            "network is unreachable", "timeout", "timed out", "tls handshake", "toomanyrequests",
            "too many requests", "service unavailable", "bad gateway", "internal server error", "unexpected eof",
            "cannot connect", "daemon",
        ]) {
            Fault::Network
        } else {
            Fault::Other
        }
    }

    /// Its words for a job's message: the space is cleared by hand, nothing is ticked again.
    pub fn job_text(self) -> String {
        match self {
            Fault::Space => t!("convert.pull_fault_space"),
            f => f.text(),
        }
    }

    /// Whether trying again later may help: not for a full disk or an image the registry lacks.
    pub fn passing(self) -> bool {
        matches!(self, Fault::Network | Fault::Other)
    }

    /// Its words, for the status line.
    pub fn text(self) -> String {
        t!(match self {
            Fault::Network => "settings.status.pull_fault_network",
            Fault::Space => "settings.status.pull_fault_space",
            Fault::Gone => "settings.status.pull_fault_gone",
            Fault::Other => "settings.status.pull_fault_other",
        })
    }
}

/// What a pull that failed left: why, when it is tried again (None: not by itself), and the
/// runtime's own error.
type Failure = (Fault, Option<u64>, String);

/// Every image whose last pull failed.
static FAILED: Mutex<BTreeMap<String, Failure>> = Mutex::new(BTreeMap::new());

/// How `image` stands after a failed pull: waiting to be tried again, or lasting. None when
/// its last pull did not fail.
pub fn failure(image: &str) -> Option<State> {
    let (fault, again, details) = FAILED.lock().unwrap().get(image)?.clone();
    Some(match again {
        Some(at) => State::Waiting { fault, at, details },
        None => State::Unpulled { fault, details },
    })
}

/// What the pulls inside jobs remember of an image: the failed pulls in a row, and how many
/// times *Retry now* was pressed by then.
static JOBS: Mutex<BTreeMap<String, (u32, u64)>> = Mutex::new(BTreeMap::new());

/// Every image whose last pull failed, as `failure` says it. The apps' Settings list them.
pub fn failures() -> BTreeMap<String, State> {
    let names: Vec<String> = FAILED.lock().unwrap().keys().cloned().collect();
    names.into_iter().filter_map(|i| failure(&i).map(|s| (i, s))).collect()
}

/// Where *Retry now* is, in the words of the job's message.
fn retry_where() -> String {
    format!("{} → {} → {}", crate::t!("action.settings"), crate::t!("settings.area.previews"), crate::t!("ptools.title"))
}

/// A pull inside a job (a conversion, the virus scan, Make PDF searchable, *Pull* in Settings):
/// `pull_until`, and what failed is remembered. A passing fault is tried again by the next job
/// after the same waits as the background pulls (`wait`), at once after *Retry now*, and for a
/// network fault as soon as the registry answers; a lasting one by the next job. Before then
/// the job fails at once with the reason, not after the runtime's own timeouts.
pub fn in_job(rt: &std::path::Path, image: &str, stop: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    pull_job(rt, image, stop, false)
}

/// `in_job`; `by_hand` (*Retry now*, *Pull* in Settings) pulls whatever the schedule says and
/// fails with the runtime's own words, as Settings shows them.
fn pull_job(rt: &std::path::Path, image: &str, stop: &std::sync::atomic::AtomicBool, by_hand: bool) -> Result<(), String> {
    // Only an image that failed wakes the waiting pulls: Pull on a healthy one wakes none.
    if by_hand && FAILED.lock().unwrap().contains_key(image) {
        retry_now();
    }
    let retries = RETRIES.load(Ordering::SeqCst);
    let held = FAILED.lock().unwrap().get(image).cloned();
    let tries = JOBS.lock().unwrap().get(image).copied();
    if let (Some((fault, Some(at), _)), Some((_, seen))) = (&held, tries)
        && *at > crate::fs::now()
        && retries <= seen
        && (*fault != Fault::Network || !reachable(image))
    {
        return Err(job_message(image, *fault, Some(*at)));
    }
    match super::pull_until(rt, image, stop) {
        Ok(()) => {
            FAILED.lock().unwrap().remove(image);
            JOBS.lock().unwrap().remove(image);
            Ok(())
        }
        Err(e) if stop.load(Ordering::Relaxed) => Err(e),
        Err(e) => {
            crate::helper::note(format_args!("{image}: {e}"));
            let fault = Fault::of(&e);
            let n = tries.map_or(0, |t| t.0);
            let at = fault.passing().then(|| crate::fs::now() + wait(n).as_secs());
            let details: String = e.trim().chars().take(4000).collect();
            FAILED.lock().unwrap().insert(image.to_string(), (fault, at, details));
            JOBS.lock().unwrap().insert(image.to_string(), (n + 1, retries));
            Err(if by_hand { e } else { job_message(image, fault, at) })
        }
    }
}

/// What a job says of an image it could not pull: why, when the next job tries again, and
/// where *Retry now* is.
fn job_message(image: &str, fault: Fault, at: Option<u64>) -> String {
    let (image, fault, place) = (super::short(image), fault.job_text(), retry_where());
    match at {
        Some(at) => crate::t!("convert.pull_waiting", "image" => image, "fault" => fault, "time" => crate::i18n::time(at), "where" => place),
        None => crate::t!("convert.pull_unpulled", "image" => image, "fault" => fault, "where" => place),
    }
}

/// *Retry now* on one image, from Settings: pulled at once, whatever the schedule says.
pub fn retry_job(rt: &std::path::Path, image: &str) -> Result<(), String> {
    pull_job(rt, image, &std::sync::atomic::AtomicBool::new(false), true)
}

/// How many times *Retry now* was pressed: a waiting pull that saw fewer goes at once.
static RETRIES: AtomicU64 = AtomicU64::new(0);

/// *Retry now*: every image waiting to be pulled again is pulled at once, also one whose fault
/// was a lasting one (the disk was cleared, the config changed).
pub fn retry_now() {
    RETRIES.fetch_add(1, Ordering::SeqCst);
}

/// How many times the system said its network changed (`watch_network`).
static NET: AtomicU64 = AtomicU64::new(0);

/// Wait `d`, in steps of a second; true when *Retry now* came after `since`. Ended early (and
/// false) when the system says its network changed while a pull waits on a network fault, so
/// the registry is asked at once rather than at the next minute.
pub fn rest(d: Duration, since: u64) -> bool {
    let end = std::time::Instant::now() + d;
    let net = NET.load(Ordering::SeqCst);
    while std::time::Instant::now() < end {
        if RETRIES.load(Ordering::SeqCst) > since {
            return true;
        }
        // shortcut: an early end counts as the whole step in `until_there`, so a network that
        // changes often brings the scheduled pull nearer; count real time if that matters.
        if NET.load(Ordering::SeqCst) != net && FAILED.lock().unwrap().values().any(|f| f.0 == Fault::Network) {
            // A new address comes before its route and name servers: a moment for them.
            std::thread::sleep(Duration::from_secs(3).min(end.saturating_duration_since(std::time::Instant::now())));
            return RETRIES.load(Ordering::SeqCst) > since;
        }
        std::thread::sleep(Duration::from_secs(1).min(end.saturating_duration_since(std::time::Instant::now())));
    }
    RETRIES.load(Ordering::SeqCst) > since
}

/// While a pull waits to be tried again, how often the runtime is asked whether the image is
/// there by now (and, after a network fault, whether the registry answers): at first, then
/// twice as long each time, `LOOK_MOST` at most; a network fault is looked at every `LOOK`.
const LOOK: Duration = Duration::from_secs(60);
const LOOK_MOST: Duration = Duration::from_secs(600);

/// How long after the `tries`-th failed pull (from 0) the next is made: 5 minutes, then twice
/// as long each time, an hour at most.
pub fn wait(tries: u32) -> Duration {
    Duration::from_secs((300u64 << tries.min(4)).min(3600))
}

/// The host a registry's address names: `docker.io/apache/tika:4` is Docker Hub's.
fn registry(image: &str) -> String {
    let first = image.split('/').next().unwrap_or_default();
    if !image.contains('/') || !(first.contains('.') || first.contains(':') || first == "localhost") {
        return "registry-1.docker.io:443".into();
    }
    match first {
        "docker.io" | "index.docker.io" => "registry-1.docker.io:443".into(),
        h if h.contains(':') => h.into(),
        h => format!("{h}:443"),
    }
}

/// Whether the registry of `image` answers a connection: the network is back. Nothing is sent.
pub fn reachable(image: &str) -> bool {
    use std::net::ToSocketAddrs;
    registry(image)
        .to_socket_addrs()
        .is_ok_and(|mut a| a.any(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(5)).is_ok()))
}

/// `pull` until the image is `there`: after a passing fault again after `wait`, after a lasting
/// one never; between, `there` is asked after `LOOK`, then less often, and after a network
/// fault `reachable` is asked every `LOOK` (a registry that answers after it did not: pull
/// at once). `rest(d, since)` waits and says whether *Retry now* came after `since`, which
/// pulls again at once. What failed and when it is tried again is in `failure`, until the image
/// is there. `name` is the tool in helper.log.
pub fn until_there(
    image: &str,
    name: &str,
    mut pull: impl FnMut() -> Result<(), String>,
    mut there: impl FnMut() -> bool,
    mut reachable: impl FnMut() -> bool,
    mut rest: impl FnMut(Duration, u64) -> bool,
) {
    let mut tries = 0;
    // The network coming back pulls at once once: a link that flaps goes by the schedule.
    let mut back = false;
    loop {
        let since = RETRIES.load(Ordering::SeqCst);
        let e = match pull() {
            Ok(()) => break,
            Err(e) => e,
        };
        crate::helper::note(format_args!("{name}: {e}"));
        let fault = Fault::of(&e);
        let wait = fault.passing().then(|| wait(tries));
        tries += 1;
        let details: String = e.trim().chars().take(4000).collect();
        FAILED.lock().unwrap().insert(image.to_string(), (fault, wait.map(|w| crate::fs::now() + w.as_secs()), details));
        if fault == Fault::Network {
            watch_network();
        }
        let mut down = fault == Fault::Network && !reachable();
        let (mut waited, mut look) = (Duration::ZERO, LOOK);
        let found = loop {
            let step = wait.map_or(look, |w| look.min(w.saturating_sub(waited)));
            if rest(step, since) {
                tries = tries.saturating_sub(1);
                break false;
            }
            waited += step;
            look = if fault == Fault::Network { LOOK } else { (look * 2).min(LOOK_MOST) };
            if there() {
                break true;
            }
            if fault == Fault::Network {
                let up = reachable();
                if up && down && !back {
                    back = true;
                    break false;
                }
                down = !up;
            }
            if wait.is_some_and(|w| waited >= w) {
                break false;
            }
        };
        if found {
            break;
        }
    }
    FAILED.lock().unwrap().remove(image);
    JOBS.lock().unwrap().remove(image);
}

/// Listens, once, in a thread of its own, for the system's word that its network changed: an
/// address or a route added (Linux: netlink; macOS: the routing socket), the address table
/// changed (Windows). Elsewhere (the BSDs, illumos, Android) the once-a-minute look stays alone.
fn watch_network() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new().name("network-watch".into()).spawn(|| {
            if let Err(e) = listen(&|| {
                NET.fetch_add(1, Ordering::SeqCst);
            }) {
                crate::helper::note(format_args!("no network signal, the registry is asked every minute: {e}"));
            }
        });
    });
}

/// Calls `changed` on every address or route the kernel adds, until the socket fails.
#[cfg(target_os = "linux")]
fn listen(changed: &dyn Fn()) -> std::io::Result<()> {
    // SAFETY: a socket of our own, a zeroed address with its family and groups set, and a
    // buffer as long as the length given.
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::NETLINK_ROUTE);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut sa: libc::sockaddr_nl = std::mem::zeroed();
        sa.nl_family = libc::AF_NETLINK as u16;
        sa.nl_groups = (libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV6_IFADDR | libc::RTMGRP_IPV4_ROUTE | libc::RTMGRP_IPV6_ROUTE) as u32;
        if libc::bind(fd, (&raw const sa).cast(), std::mem::size_of::<libc::sockaddr_nl>() as u32) < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let mut buf = [0u8; 16384];
        loop {
            let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
            if n < 0 {
                let e = std::io::Error::last_os_error();
                // Interrupted, or more came than the socket holds: something changed.
                match e.raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::ENOBUFS) => {
                        changed();
                        continue;
                    }
                    _ => {
                        libc::close(fd);
                        return Err(e);
                    }
                }
            }
            if added(&buf[..n as usize]) {
                changed();
            }
        }
    }
}

/// Whether the netlink messages in `b` add an address or a route (not only take one away).
#[cfg(any(target_os = "linux", test))]
fn added(mut b: &[u8]) -> bool {
    const NEWADDR: u16 = 20;
    const NEWROUTE: u16 = 24;
    while b.len() >= 16 {
        let len = u32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as usize;
        if matches!(u16::from_ne_bytes([b[4], b[5]]), NEWADDR | NEWROUTE) {
            return true;
        }
        // Each message is padded to 4 bytes.
        let step = (len + 3) & !3;
        if step < 16 || step > b.len() {
            break;
        }
        b = &b[step..];
    }
    false
}

/// Calls `changed` on every address added or interface changed, until the socket fails.
#[cfg(target_os = "macos")]
fn listen(changed: &dyn Fn()) -> std::io::Result<()> {
    // SAFETY: a socket of our own and a buffer as long as the length given.
    unsafe {
        let fd = libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, libc::AF_UNSPEC);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut buf = [0u8; 8192];
        loop {
            let n = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                libc::close(fd);
                return Err(e);
            }
            // rt_msghdr: its length (2 bytes), version, then type. Routes cloned for each new
            // connection are not looked at, so asking the registry wakes nothing.
            if n >= 4 && matches!(buf[3] as libc::c_int, libc::RTM_NEWADDR | libc::RTM_IFINFO) {
                changed();
            }
        }
    }
}

/// Calls `changed` every time the table of IP addresses changes, until that call fails.
#[cfg(windows)]
fn listen(changed: &dyn Fn()) -> std::io::Result<()> {
    loop {
        // SAFETY: no handle and no overlapped: the call waits until the table changes.
        let r = unsafe { windows_sys::Win32::NetworkManagement::IpHelper::NotifyAddrChange(std::ptr::null_mut(), std::ptr::null()) };
        if r != 0 {
            return Err(std::io::Error::from_raw_os_error(r as i32));
        }
        changed();
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn listen(_: &dyn Fn()) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DNS: &str = "Pulling docker.io/apache/tika:4.1.0 failed: Error: initializing source docker://apache/tika:4.1.0: pinging container registry registry-1.docker.io: Get \"https://registry-1.docker.io/v2/\": dial tcp: lookup registry-1.docker.io: Temporary failure in name resolution";

    #[test]
    fn faults_are_told_by_the_runtimes_words() {
        assert_eq!(Fault::of(DNS), Fault::Network);
        assert_eq!(Fault::of("Error: writing blob: no space left on device"), Fault::Space);
        assert_eq!(Fault::of("reading manifest 4.1.0 in docker.io/apache/tika: manifest unknown"), Fault::Gone);
        assert_eq!(Fault::of("exit status 125"), Fault::Other);
        assert_eq!(Fault::of("Error: creating container storage: open /home/x/.local/share/containers/storage/overlay: permission denied"), Fault::Other, "the machine's, not the registry's");
        assert_eq!(Fault::of("denied: requested access to the resource is denied"), Fault::Gone);
        assert_eq!(Fault::of("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"), Fault::Network);
    }

    #[test]
    fn registries_are_found_in_the_name() {
        assert_eq!(registry("docker.io/apache/tika:4.1.0@sha256:ab"), "registry-1.docker.io:443");
        assert_eq!(registry("ubuntu:24.04"), "registry-1.docker.io:443");
        assert_eq!(registry("ghcr.io/x/y:1"), "ghcr.io:443");
        assert_eq!(registry("localhost:5000/y"), "localhost:5000");
    }

    /// Two pulls fail for no network, the third works: slept until each was due, the state
    /// waiting in between.
    #[test]
    fn a_passing_fault_is_pulled_again_when_due() {
        let image = "test.invalid/pass:1";
        let (mut pulls, slept, mut seen) = (0, std::cell::Cell::new(Duration::ZERO), vec![]);
        let mut waits = vec![];
        until_there(
            image,
            "test",
            || {
                pulls += 1;
                waits.push(slept.get());
                if pulls < 3 { Err(DNS.to_string()) } else { Ok(()) }
            },
            || false,
            || false,
            |d, _| {
                slept.set(slept.get() + d);
                seen.push(failure(image));
                false
            },
        );
        assert_eq!(waits, [Duration::ZERO, wait(0), wait(0) + wait(1)]);
        assert_eq!((wait(0).as_secs(), wait(1).as_secs(), wait(9).as_secs()), (300, 600, 3600));
        assert!(seen.iter().all(|s| matches!(s, Some(State::Waiting { fault: Fault::Network, details, .. }) if details.contains("name resolution"))), "{seen:?}");
        assert_eq!(failure(image), None, "pulled: no fault left");
    }

    /// The registry lacks the image: never pulled again, until it is there another way; the
    /// runtime asked less and less often, every 10 minutes at most.
    #[test]
    fn a_lasting_fault_is_not_pulled_again_but_the_image_is_looked_for() {
        let image = "test.invalid/gone:1";
        let (mut pulls, mut looks, mut steps) = (0, 0, vec![]);
        until_there(
            image,
            "test",
            || {
                pulls += 1;
                Err("manifest unknown".to_string())
            },
            || {
                looks += 1;
                assert!(matches!(failure(image), Some(State::Unpulled { fault: Fault::Gone, .. })));
                looks == 100
            },
            || unreachable!("only a network fault asks"),
            |d, _| {
                steps.push(d.as_secs());
                false
            },
        );
        assert_eq!((pulls, looks), (1, 100));
        assert_eq!(steps[..6], [60, 120, 240, 480, 600, 600]);
        assert_eq!(failure(image), None);
    }

    /// *Retry now* pulls again at once, also after a lasting fault; the network coming back
    /// does so after a network fault, but a registry that answered all along does not.
    #[test]
    fn retry_now_and_the_network_coming_back_pull_at_once() {
        let image = "test.invalid/retry:1";
        let mut pulls = 0;
        until_there(image, "test", || { pulls += 1; if pulls < 2 { Err("manifest unknown".into()) } else { Ok(()) } }, || false, || false, |_, _| true);
        assert_eq!(pulls, 2);
        // Down, then up: the pull comes after the second look, not after a whole wait.
        let (mut pulls, mut probes, mut rests) = (0, vec![false, true].into_iter(), 0);
        until_there(image, "test", || { pulls += 1; if pulls < 2 { Err(DNS.into()) } else { Ok(()) } }, || false, || probes.next().unwrap(), |_, _| { rests += 1; false });
        assert_eq!((pulls, rests), (2, 1));
        // The registry answered all along: only the schedule pulls.
        let (mut pulls, mut rests) = (0, 0);
        until_there(image, "test", || { pulls += 1; if pulls < 2 { Err(DNS.into()) } else { Ok(()) } }, || false, || true, |_, _| { rests += 1; false });
        assert_eq!(pulls, 2);
        assert_eq!(rests, 5, "60 s looks until the 300 s were up");
    }

    /// Only netlink messages that add an address or a route say the network may be back.
    #[test]
    fn netlink_additions_are_told_apart() {
        let msg = |kind: u16| {
            let mut m = vec![0u8; 20];
            m[..4].copy_from_slice(&18u32.to_ne_bytes());
            m[4..6].copy_from_slice(&kind.to_ne_bytes());
            m
        };
        assert!(added(&msg(20)) && added(&msg(24)), "a new address, a new route");
        assert!(!added(&msg(21)) && !added(&msg(25)) && !added(&[]), "only taken away");
        assert!(added(&[msg(21), msg(24)].concat()), "a new route after a removed address");
    }

    /// A pull inside a job that fails is remembered and the next job fails at once with the
    /// reason, without pulling; *Retry now* pulls whatever the schedule says.
    #[test]
    fn a_job_that_cannot_pull_is_held_until_due_or_retried() {
        let (rt, image, stop) = (std::path::Path::new("/nonexistent/coxswain-test-runtime"), "test.invalid/job:1", std::sync::atomic::AtomicBool::new(false));
        let first = in_job(rt, image, &stop).unwrap_err();
        assert!(first.contains("test.invalid/job:1") && first.contains("→"), "{first}");
        assert!(matches!(failure(image), Some(State::Waiting { .. })));
        assert!(failures().contains_key(image));
        let tries = |n| assert_eq!(JOBS.lock().unwrap().get(image).map(|j| j.0), Some(n));
        tries(1);
        assert_eq!(in_job(rt, image, &stop).unwrap_err(), first);
        tries(1);
        assert!(retry_job(rt, image).is_err());
        tries(2);
        FAILED.lock().unwrap().remove(image);
        JOBS.lock().unwrap().remove(image);
    }

    /// Pull on an image that never failed wakes no waiting pull; an image that is there again
    /// forgets its failed pulls in a row.
    #[test]
    fn a_healthy_image_wakes_nothing_and_a_found_one_forgets() {
        let (rt, stop) = (std::path::Path::new("/nonexistent/coxswain-test-runtime"), std::sync::atomic::AtomicBool::new(false));
        let before = RETRIES.load(Ordering::SeqCst);
        let healthy = "test.invalid/healthy:1";
        let _ = retry_job(rt, healthy);
        // It failed just now, so it is a failed image from here on; before that nothing woke.
        assert_eq!(RETRIES.load(Ordering::SeqCst), before, "a healthy image bumped the counter");
        FAILED.lock().unwrap().remove(healthy);
        JOBS.lock().unwrap().remove(healthy);
        let image = "test.invalid/forget:1";
        JOBS.lock().unwrap().insert(image.to_string(), (3, 0));
        until_there(image, "test", || Ok(()), || true, || true, |_, _| false);
        assert!(JOBS.lock().unwrap().get(image).is_none());
    }

    /// An app older than `STATES_SINCE` gets a failed pull as the `Failed` it knows; a newer
    /// one, or another state, as it is.
    #[test]
    fn states_reach_an_older_app_in_its_own_form() {
        let w = State::Waiting { fault: Fault::Network, at: 0, details: String::new() };
        assert!(matches!(w.clone().for_app("2.35.0", "Tika"), State::Failed(t) if t.contains(&Fault::Network.text())));
        assert!(matches!(State::Unpulled { fault: Fault::Gone, details: String::new() }.for_app("2.34.2", "Tika"), State::Failed(_)));
        assert_eq!(w.clone().for_app(STATES_SINCE, "Tika"), w);
        assert_eq!(w.clone().for_app("3.0.0", "Tika"), w);
        assert_eq!(State::Ready.for_app("2.0.0", "Tika"), State::Ready);
        let old = serde_json::to_string(&w.for_app("2.35.0", "Tika")).unwrap();
        assert!(old.starts_with("{\"failed\":"), "{old}");
    }
}

//! What the search helper asks of the machine: whether it runs on its battery, and which disk
//! a folder is on, so a removable disk is known again wherever it is mounted.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether the machine runs on its battery now. Asked at most every half minute.
pub fn on_battery() -> bool {
    static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if let Some((at, known)) = *last {
        if at.elapsed() < Duration::from_secs(30) {
            return known;
        }
    }
    let now = battery_now();
    *last = Some((Instant::now(), now));
    now
}

#[cfg(target_os = "linux")]
fn battery_now() -> bool {
    let read = |p: PathBuf, f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
    let supplies: Vec<PathBuf> = std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten().map(|e| e.path()).collect();
    let mains: Vec<&PathBuf> = supplies.iter().filter(|p| read(p.to_path_buf(), "type") == "Mains").collect();
    let battery = supplies.iter().any(|p| read(p.to_path_buf(), "type") == "Battery");
    if !mains.is_empty() {
        return battery && !mains.iter().any(|p| read(p.to_path_buf(), "online") == "1");
    }
    supplies.iter().any(|p| read(p.to_path_buf(), "type") == "Battery" && read(p.to_path_buf(), "status") == "Discharging")
}

#[cfg(target_os = "macos")]
fn battery_now() -> bool {
    crate::tools::command("pmset").args(["-g", "batt"]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("'Battery Power'"))
}

/// `hw.acpi.acline` (FreeBSD, DragonFly) is 1 on mains power and 0 on the battery; machines without ACPI power
/// reporting (desktops, most virtual machines) have no such variable.
#[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
fn battery_now() -> bool {
    crate::tools::command("sysctl").args(["-n", "hw.acpi.acline"]).output().is_ok_and(|o| o.status.success() && o.stdout.trim_ascii() == b"0")
}

/// envstat(8)'s AC adapter: `connected: FALSE` is on the battery. A machine without one counts
/// as on mains.
#[cfg(target_os = "netbsd")]
fn battery_now() -> bool {
    crate::tools::command("envstat").output().is_ok_and(|o| ac_unplugged(&String::from_utf8_lossy(&o.stdout)))
}

/// Whether envstat's text has an `acpiacad` adapter that is not connected.
#[cfg_attr(not(target_os = "netbsd"), allow(dead_code))]
fn ac_unplugged(envstat: &str) -> bool {
    let mut adapter = false;
    envstat.lines().map(str::trim).any(|l| {
        if l.starts_with('[') {
            adapter = l.starts_with("[acpiacad");
        }
        adapter && l.strip_prefix("connected:").is_some_and(|v| v.trim() == "FALSE")
    })
}

/// `hw.power` is 1 on mains power and 0 on the battery.
#[cfg(target_os = "openbsd")]
fn battery_now() -> bool {
    crate::tools::command("sysctl").args(["-n", "hw.power"]).output().is_ok_and(|o| o.status.success() && o.stdout.trim_ascii() == b"0")
}

/// acpi_drv's kstat `power` says `AC` or `battery`; machines without ACPI batteries have none.
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
fn battery_now() -> bool {
    crate::tools::command("kstat").args(["-p", "acpi_drv:0:power:power"]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().last().is_some_and(|v| v.eq_ignore_ascii_case("battery")))
}

#[cfg(windows)]
fn battery_now() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // ACLineStatus: 0 offline, 1 online, 255 unknown.
    unsafe { GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 0 }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd", target_os = "dragonfly", target_os = "netbsd", target_os = "openbsd", target_os = "illumos", target_os = "solaris", windows)))]
fn battery_now() -> bool {
    false
}

// ---------------------------------------------------------------- disks

/// A mounted file system: what is mounted, where, of which type, its size and the space free
/// to the user, in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounted {
    pub device: String,
    pub mount: PathBuf,
    pub fstype: String,
    pub total: u64,
    pub free: u64,
}

/// File systems that are the kernel's, not places to go.
#[cfg_attr(not(any(target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly", target_os = "illumos", target_os = "solaris")), allow(dead_code))]
const PSEUDO: &[&str] = &["proc", "procfs", "kernfs", "ptyfs", "devfs", "tmpfs", "mfs", "ctfs", "objfs", "mntfs", "fd", "sharefs", "dev", "bootfs", "lofs", "null", "nullfs", "autofs"];

/// The mounted file systems, for the desktop app's sidebar on the systems its disk library does
/// not know: getmntinfo(3) on NetBSD, OpenBSD and DragonFly.
#[cfg(any(target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly"))]
pub fn mounted() -> Vec<Mounted> {
    use std::ffi::CStr;
    // getmntinfo hands out one buffer of its own, overwritten by the next call.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = std::ptr::null_mut();
    // SAFETY: getmntinfo points `list` at `n` entries it keeps until its next call; MNT_NOWAIT (2)
    // reads what the kernel has without asking each file system (a dead NFS server would hang).
    let n = unsafe { libc::getmntinfo(&mut list, 2) };
    if n <= 0 || list.is_null() {
        return vec![];
    }
    // SAFETY: as above; the names end with a NUL inside their arrays.
    let all = unsafe { std::slice::from_raw_parts(list, n as usize) };
    let text = |a: &[libc::c_char]| unsafe { CStr::from_ptr(a.as_ptr()) }.to_string_lossy().into_owned();
    all.iter()
        .map(|f| {
            #[cfg(target_os = "netbsd")]
            let block = f.f_frsize as u64;
            #[cfg(not(target_os = "netbsd"))]
            let block = f.f_bsize as u64;
            Mounted { device: text(&f.f_mntfromname), mount: PathBuf::from(text(&f.f_mntonname)), fstype: text(&f.f_fstypename), total: (f.f_blocks as u64).saturating_mul(block), free: (f.f_bavail.max(0) as u64).saturating_mul(block) }
        })
        .filter(|m| !PSEUDO.contains(&m.fstype.as_str()))
        .collect()
}

/// The mounted file systems, from /etc/mnttab, with their sizes from statvfs(2): illumos.
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
pub fn mounted() -> Vec<Mounted> {
    let text = std::fs::read_to_string("/etc/mnttab").unwrap_or_default();
    parse_mnttab(&text)
        .into_iter()
        .filter(|m| !PSEUDO.contains(&m.fstype.as_str()))
        .map(|mut m| {
            use std::os::unix::ffi::OsStrExt;
            if let Ok(c) = std::ffi::CString::new(m.mount.as_os_str().as_bytes()) {
                // SAFETY: statvfs fills the zeroed struct it is given.
                let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
                if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
                    let block = st.f_frsize as u64;
                    (m.total, m.free) = ((st.f_blocks as u64).saturating_mul(block), (st.f_bavail as u64).saturating_mul(block));
                }
            }
            m
        })
        .collect()
}

/// /etc/mnttab's lines: device, mount point, type, options and time, by tabs. Sizes are 0.
#[cfg_attr(not(any(target_os = "illumos", target_os = "solaris")), allow(dead_code))]
pub(crate) fn parse_mnttab(text: &str) -> Vec<Mounted> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (device, mount, fstype) = (f.next()?, f.next()?, f.next()?);
            mount.starts_with('/').then(|| Mounted { device: device.into(), mount: PathBuf::from(mount), fstype: fstype.into(), total: 0, free: 0 })
        })
        .collect()
}

/// The disk `path` is on, and where `path` is inside that disk's file system: its UUID (Linux,
/// macOS) or volume serial (Windows), and the path from the file system's top. `None` when it
/// cannot be told (tmpfs, network shares).
pub fn volume(path: &Path) -> Option<(String, PathBuf)> {
    volume_of(path)
}

/// Where `inside` on the disk with this id is now, when that disk is mounted.
pub fn locate(id: &str, inside: &Path) -> Option<PathBuf> {
    locate_on(id, inside)
}

/// `inside`, found below `top` (the part of the file system mounted) at `point`.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos", windows)), allow(dead_code))]
fn under(point: &Path, top: &Path, inside: &Path) -> Option<PathBuf> {
    let rest = inside.strip_prefix(top).ok()?;
    Some(if rest.as_os_str().is_empty() { point.to_path_buf() } else { point.join(rest) })
}

/// Mounted file systems as (device, part of it mounted, mount point), from mountinfo.
#[cfg(target_os = "linux")]
fn mounts() -> Vec<(PathBuf, PathBuf, PathBuf)> {
    // `id parent major:minor root mountpoint options … - fstype source superoptions`, with
    // spaces in paths written as \040. `root` is a btrfs subvolume, or a bind mount's folder.
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let mut f = left.split(' ').skip(3);
            let (top, point) = (f.next()?, f.next()?);
            let source = right.split(' ').nth(1)?;
            source.starts_with("/dev/").then(|| (PathBuf::from(unescape(source)), PathBuf::from(unescape(top)), PathBuf::from(unescape(point))))
        })
        .collect()
}

/// UUID -> device, from /dev/disk/by-uuid.
#[cfg(target_os = "linux")]
fn uuids() -> Vec<(String, PathBuf)> {
    std::fs::read_dir("/dev/disk/by-uuid").into_iter().flatten().flatten().filter_map(|e| Some((e.file_name().into_string().ok()?, std::fs::canonicalize(e.path()).ok()?))).collect()
}

#[cfg(target_os = "linux")]
fn volume_of(path: &Path) -> Option<(String, PathBuf)> {
    let (device, top, point) = mounts().into_iter().filter(|(.., point)| path.starts_with(point)).max_by_key(|(.., point)| point.as_os_str().len())?;
    let device = std::fs::canonicalize(device).ok()?;
    let (id, _) = uuids().into_iter().find(|(_, d)| *d == device)?;
    Some((id, top.join(path.strip_prefix(&point).ok()?)))
}

#[cfg(target_os = "linux")]
fn locate_on(id: &str, inside: &Path) -> Option<PathBuf> {
    let (_, device) = uuids().into_iter().find(|(u, _)| u == id)?;
    // The mount that holds the most of the path: the subvolume it is in.
    mounts()
        .into_iter()
        .filter(|(d, top, _)| inside.starts_with(top) && std::fs::canonicalize(d).ok().as_ref() == Some(&device))
        .max_by_key(|(_, top, point)| (top.as_os_str().len(), usize::MAX - point.as_os_str().len()))
        .and_then(|(_, top, point)| under(&point, &top, inside))
}

/// `diskutil info` of a path or volume, as (VolumeUUID, MountPoint).
#[cfg(target_os = "macos")]
fn diskutil(what: &Path) -> Option<(String, PathBuf)> {
    let out = crate::tools::command("diskutil").arg("info").arg("-plist").arg(what).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let value = |key: &str| text.split(&format!("<key>{key}</key>")).nth(1)?.split("<string>").nth(1)?.split("</string>").next().map(str::to_string);
    Some((value("VolumeUUID")?, PathBuf::from(value("MountPoint")?)))
}

#[cfg(target_os = "macos")]
fn volume_of(path: &Path) -> Option<(String, PathBuf)> {
    let (id, point) = diskutil(path)?;
    Some((id, Path::new("/").join(path.strip_prefix(&point).ok()?)))
}

#[cfg(target_os = "macos")]
fn locate_on(id: &str, inside: &Path) -> Option<PathBuf> {
    let (_, point) = diskutil(Path::new(id))?;
    (!point.as_os_str().is_empty()).then(|| under(&point, Path::new("/"), inside)).flatten()
}

/// The serial of the volume at a drive root such as `D:\`.
#[cfg(windows)]
fn serial(root: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain([0]).collect();
    let mut serial = 0u32;
    let ok = unsafe { GetVolumeInformationW(wide.as_ptr(), std::ptr::null_mut(), 0, &mut serial, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
    (ok != 0).then(|| format!("{serial:08X}"))
}

#[cfg(windows)]
fn volume_of(path: &Path) -> Option<(String, PathBuf)> {
    let std::path::Component::Prefix(prefix) = path.components().next()? else { return None };
    let root = PathBuf::from(format!("{}\\", prefix.as_os_str().to_string_lossy()));
    Some((serial(&root)?, Path::new("\\").join(path.strip_prefix(&root).ok()?)))
}

#[cfg(windows)]
fn locate_on(id: &str, inside: &Path) -> Option<PathBuf> {
    let root = (b'A'..=b'Z').map(|l| PathBuf::from(format!("{}:\\", l as char))).find(|root| root.exists() && serial(root).as_deref() == Some(id))?;
    under(&root, Path::new("\\"), inside)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn volume_of(_: &Path) -> Option<(String, PathBuf)> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn locate_on(_: &str, _: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_knows_its_disks() {
        // Wherever the tests run, a folder on a real disk is found again by its disk.
        for dir in [std::env::temp_dir(), std::env::current_dir().unwrap()] {
            if let Some((id, inside)) = volume(&dir) {
                assert_eq!(locate(&id, &inside).as_deref(), Some(dir.as_path()), "{id} {inside:?}");
            }
        }
        assert_eq!(locate("no-such-disk", Path::new("/")), None);
        assert_eq!(under(Path::new("/run/media/me/Backup"), Path::new("/"), Path::new("/Photos")), Some(PathBuf::from("/run/media/me/Backup/Photos")));
        assert_eq!(under(Path::new("/home"), Path::new("/@home"), Path::new("/@home/me")), Some(PathBuf::from("/home/me")));
        assert_eq!(under(Path::new("/home"), Path::new("/@home"), Path::new("/@/etc")), None);
        let _ = on_battery();
    }

    #[test]
    fn machine_reads_illumos_mnttab() {
        let text = "rpool/ROOT/omnios\t/\tzfs\tdev=4010002\t1700000000\n/devices\t/devices\tdevfs\tdev=8880000\t1700000000\nrpool/export/home\t/export/home dir\tzfs\trw\t1700000001\n";
        let m = parse_mnttab(text);
        assert_eq!(m.len(), 3);
        assert_eq!((m[0].device.as_str(), m[0].mount.as_path(), m[0].fstype.as_str()), ("rpool/ROOT/omnios", Path::new("/"), "zfs"));
        assert_eq!(m[2].mount, Path::new("/export/home dir"), "spaces stay: the fields are split by tabs");
        assert!(PSEUDO.contains(&m[1].fstype.as_str()));
    }

    #[test]
    fn machine_reads_netbsd_envstat() {
        let text = "                Current  CritMax  Unit\n[acpiacad0]\n    connected:     FALSE\n[acpibat0]\n    present:      TRUE\n    connected:     TRUE\n";
        assert!(ac_unplugged(text));
        assert!(!ac_unplugged(&text.replacen("FALSE", "TRUE", 1)));
        assert!(!ac_unplugged("[acpibat0]\n    connected: FALSE\n"), "a battery's line is not the adapter's");
        assert!(!ac_unplugged(""));
    }
}

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

#[cfg(windows)]
fn battery_now() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // ACLineStatus: 0 offline, 1 online, 255 unknown.
    unsafe { GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 0 }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn battery_now() -> bool {
    false
}

// ---------------------------------------------------------------- disks

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
}

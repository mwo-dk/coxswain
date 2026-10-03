//! Files that are only in the cloud. OneDrive, Dropbox, Google Drive, Proton Drive, iCloud and
//! the like keep a file on the disk by its name and size alone until something reads it; then
//! they download it. Coxswain tells such files by their metadata (never by opening them), and
//! reads none of them unless the user asks for that one file, or said to read them all.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Windows attributes of a file whose data is not on the disk: the Cloud Files API's (OneDrive,
/// Dropbox, Google Drive for desktop, Proton Drive, iCloud for Windows) and older storage
/// managers' (`OFFLINE`).
pub const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
pub const RECALL_ON_OPEN: u32 = 0x0004_0000;
pub const OFFLINE: u32 = 0x0000_1000;
/// "Always keep on this device" and "Free up space": what the user wants, not where the data is
/// now. A pinned file still downloading has `RECALL_ON_DATA_ACCESS` too.
pub const PINNED: u32 = 0x0008_0000;
pub const UNPINNED: u32 = 0x0010_0000;
/// macOS `st_flags` of a File Provider file (iCloud, and OneDrive, Dropbox, Google Drive under
/// ~/Library/CloudStorage) whose data is in the cloud.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Whether Windows attributes say the data is in the cloud. Pinned or not does not matter.
pub fn online_attributes(attributes: u32) -> bool {
    attributes & (RECALL_ON_DATA_ACCESS | RECALL_ON_OPEN | OFFLINE) != 0
}

/// Whether macOS `st_flags` say the data is in the cloud.
pub fn dataless(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

/// Whether the data of the file `meta` (from lstat or the folder's listing) describes is only in
/// the cloud: reading it would download it. On Linux, every file on a cloud mount (rclone,
/// google-drive-ocamlfuse, onedriver, gvfs) is.
pub fn online_meta(meta: &Metadata, path: &Path) -> bool {
    #[cfg(test)]
    if PRETEND.lock().unwrap().iter().any(|p| p == path) {
        return true;
    }
    #[cfg(windows)]
    {
        let _ = path;
        online_attributes(std::os::windows::fs::MetadataExt::file_attributes(meta))
    }
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        dataless(std::os::macos::fs::MetadataExt::st_flags(meta))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = meta;
        mount_of(path).is_some()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (meta, path);
        false
    }
}

/// The metadata of `path` without opening it: of the link itself unless it is one.
fn meta(path: &Path) -> Option<Metadata> {
    let m = std::fs::symlink_metadata(path).ok()?;
    if m.file_type().is_symlink() { std::fs::metadata(path).ok() } else { Some(m) }
}

/// Whether `path` is only in the cloud.
pub fn online(path: &Path) -> bool {
    meta(path).is_some_and(|m| online_meta(&m, path))
}

/// `cloud = "all"`: files only in the cloud are read (downloaded) like the rest.
static READ_ALL: AtomicBool = AtomicBool::new(false);
/// Folders in the cloud whose files are read anyway (`cloud_read`), a mount on Linux, say.
static READ_IN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Follow the settings: every config that is loaded or saved passes here.
pub fn follow(cfg: &crate::config::SearchConfig) {
    READ_ALL.store(cfg.cloud == "all", Ordering::Relaxed);
    *READ_IN.lock().unwrap() = cfg.cloud_read.clone();
}

/// Whether the settings say to read `path` even when it is only in the cloud.
fn wanted(path: &Path) -> bool {
    READ_ALL.load(Ordering::Relaxed) || READ_IN.lock().unwrap().iter().any(|r| path.starts_with(r))
}

/// Whether reading `path` unasked (to index it, hash it, look into it) would download it, and
/// the settings do not want that. What is on the disk, pinned or downloaded earlier, is read.
pub fn keep_out(path: &Path) -> bool {
    meta(path).is_some_and(|m| keep_out_meta(&m, path))
}

/// `keep_out` with the metadata in hand.
pub fn keep_out_meta(meta: &Metadata, path: &Path) -> bool {
    online_meta(meta, path) && !wanted(path)
}

/// The error of a read that was not made because the file is only in the cloud.
pub fn not_here(path: &Path) -> std::io::Error {
    std::io::Error::other(format!("{}: {}", path.display(), crate::t!("cloud.online_only")))
}

/// Git in a repository whose files are only in the cloud would download them: `dir` is in one
/// when its `.git` is.
// ponytail: only the index and HEAD are looked at; a repository whose old packs alone went
// back to the cloud has them downloaded by git.
pub fn git_kept_out(dir: &Path) -> bool {
    let Some(root) = dir.ancestors().find(|a| a.join(".git").exists()) else { return false };
    let git = root.join(".git");
    [git.join("index"), git.join("HEAD"), git].iter().any(|p| keep_out(p))
}

/// Which cloud `path` (a file only in the cloud) is in, and the folder that holds that cloud:
/// ("OneDrive", `C:\Users\me\OneDrive`). For a cloud that cannot be named, its folder.
pub fn place(path: &Path) -> (String, PathBuf) {
    #[cfg(target_os = "linux")]
    if let Some((point, fs)) = mount_of(path) {
        let name = point.file_name().and_then(|n| provider(&n.to_string_lossy())).map_or_else(|| fs.trim_start_matches("fuse.").to_string(), str::to_string);
        return (name, point);
    }
    // The outermost folder named for a cloud: `Dropbox/notes/OneDrive.txt` is in Dropbox.
    let mut found = None;
    for a in path.ancestors().skip(1) {
        let name = a.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let parent = a.parent().and_then(Path::file_name).map(|n| n.to_string_lossy()).unwrap_or_default();
        // macOS: ~/Library/CloudStorage/OneDrive-Personal, ~/Library/Mobile Documents.
        let named = if parent == "CloudStorage" { Some(provider(&name).unwrap_or(name.split('-').next().unwrap_or_default()).to_string()) } else { provider(&name).map(str::to_string) };
        if let Some(n) = named {
            found = Some((n, a.to_path_buf()));
        }
    }
    found.unwrap_or_else(|| ("cloud".into(), path.parent().unwrap_or(path).to_path_buf()))
}

/// The cloud a folder of this name belongs to, as its client names its folder.
fn provider(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let starts = [("onedrive", "OneDrive"), ("dropbox", "Dropbox"), ("google drive", "Google Drive"), ("googledrive", "Google Drive"), ("my drive", "Google Drive"), ("proton drive", "Proton Drive"), ("protondrive", "Proton Drive"), ("icloud", "iCloud Drive"), ("mobile documents", "iCloud Drive"), ("pcloud", "pCloud"), ("nextcloud", "Nextcloud")];
    let whole = [("box", "Box"), ("mega", "MEGA")];
    starts.iter().find(|(p, _)| n.starts_with(p)).or_else(|| whole.iter().find(|(p, _)| n == *p)).map(|(_, name)| *name)
}

// ---------------------------------------------------------------- Linux: cloud mounts

/// FUSE file systems that keep their files here: everything else on FUSE is taken for a cloud
/// or a server (rclone, google-drive-ocamlfuse, onedriver, gvfs, sshfs).
const LOCAL_FUSE: [&str; 13] = ["portal", "doc", "lxcfs", "squashfuse", "snapfuse", "fuse-overlayfs", "mergerfs", "bindfs", "gocryptfs", "encfs", "cryfs", "ntfs-3g", "vmhgfs-fuse"];

/// Whether a file system of this type (`fuse.rclone`) holds files that are elsewhere.
pub fn cloud_fs(fstype: &str) -> bool {
    let Some(kind) = fstype.strip_prefix("fuse.") else { return false };
    // An AppImage is mounted under its own name.
    !LOCAL_FUSE.contains(&kind) && !kind.to_lowercase().contains("appimage")
}

/// The cloud mounts in `/proc/self/mountinfo`'s text: (mount point, file system type).
pub fn cloud_mounts(mountinfo: &str) -> Vec<(PathBuf, String)> {
    // `id parent major:minor root mountpoint options … - fstype source superoptions`, with
    // spaces in paths written as \040.
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\");
    mountinfo
        .lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let fstype = right.split(' ').next()?;
            cloud_fs(fstype).then(|| (PathBuf::from(unescape(point)), fstype.to_string()))
        })
        .collect()
}

/// The cloud mount `path` is on, and its type. The mounts are read again after ten seconds.
#[cfg(target_os = "linux")]
pub fn mount_of(path: &Path) -> Option<(PathBuf, String)> {
    use std::time::{Duration, Instant};
    type Mounts = Option<(Instant, std::sync::Arc<Vec<(PathBuf, String)>>)>;
    static MOUNTS: Mutex<Mounts> = Mutex::new(None);
    let mounts = {
        let mut m = MOUNTS.lock().unwrap();
        match &*m {
            Some((at, v)) if at.elapsed() < Duration::from_secs(10) => v.clone(),
            _ => {
                let v = std::sync::Arc::new(cloud_mounts(&std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default()));
                *m = Some((Instant::now(), v.clone()));
                v
            }
        }
    };
    mounts.iter().filter(|(p, _)| path.starts_with(p)).max_by_key(|(p, _)| p.as_os_str().len()).cloned()
}

/// Whether `dir` is a cloud mount whose files the settings do not read: the search store does
/// not walk it, as listing it may be the network's work.
pub fn unread_mount(dir: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        mount_of(dir).is_some_and(|(point, _)| point == dir) && !wanted(dir)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        false
    }
}

// ---------------------------------------------------------------- tests

/// Files the tests say are only in the cloud.
#[cfg(test)]
static PRETEND: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Take `path` for a file only in the cloud, or on the disk again.
#[cfg(test)]
pub(crate) fn pretend(path: &Path, online: bool) {
    let mut all = PRETEND.lock().unwrap();
    all.retain(|p| p != path);
    if online {
        all.push(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_attributes_and_flags_tell_online_files() {
        // OneDrive "online only": unpinned, recalled on access. "Always keep": pinned, here.
        assert!(online_attributes(RECALL_ON_DATA_ACCESS | UNPINNED | 0x20));
        assert!(online_attributes(RECALL_ON_DATA_ACCESS | PINNED), "pinned, not downloaded yet");
        assert!(online_attributes(RECALL_ON_OPEN));
        assert!(online_attributes(OFFLINE), "older storage managers");
        assert!(!online_attributes(PINNED | 0x20), "kept on this device");
        assert!(!online_attributes(0x20), "downloaded earlier, still here");
        assert!(!online_attributes(0x400 | 0x20), "a reparse point alone is not online");
        assert!(dataless(SF_DATALESS) && dataless(SF_DATALESS | 0x20));
        assert!(!dataless(0) && !dataless(0x8000));
    }

    #[test]
    fn cloud_mounts_are_read_from_mountinfo() {
        let info = "22 1 0:21 / /proc rw,nosuid - proc proc rw
29 1 259:2 / / rw,relatime - btrfs /dev/nvme0n1p2 rw
101 29 0:52 / /home/me/Google\\040Drive rw,nosuid - fuse.rclone gdrive: rw,user_id=1000
102 29 0:53 / /run/user/1000/doc rw - fuse.portal portal rw
103 29 0:54 / /home/me/OneDrive rw - fuse.onedriver onedriver rw
104 29 0:55 / /tmp/.mount_Obsidi rw - fuse.Obsidian-1.5.AppImage Obsidian rw
105 29 0:56 / /run/user/1000/gvfs rw - fuse.gvfsd-fuse gvfsd-fuse rw
106 29 8:17 / /media/usb rw - fuseblk /dev/sdb1 rw";
        let found: Vec<(PathBuf, String)> = cloud_mounts(info);
        assert_eq!(
            found,
            [("/home/me/Google Drive", "fuse.rclone"), ("/home/me/OneDrive", "fuse.onedriver"), ("/run/user/1000/gvfs", "fuse.gvfsd-fuse")].map(|(p, t)| (PathBuf::from(p), t.to_string()))
        );
        assert!(cloud_fs("fuse.google-drive-ocamlfuse") && cloud_fs("fuse.sshfs"));
        assert!(!cloud_fs("fuse.portal") && !cloud_fs("fuseblk") && !cloud_fs("ext4") && !cloud_fs("fuse.mergerfs"));
    }

    #[test]
    fn cloud_places_are_named_for_their_client() {
        let p = |s: &str| place(Path::new(s));
        assert_eq!(p("/c/Users/me/OneDrive - Contoso/Plans/q3.xlsx"), ("OneDrive".into(), PathBuf::from("/c/Users/me/OneDrive - Contoso")));
        assert_eq!(p("/Users/me/Library/CloudStorage/GoogleDrive-me@x.com/My Drive/a.pdf"), ("Google Drive".into(), PathBuf::from("/Users/me/Library/CloudStorage/GoogleDrive-me@x.com")));
        assert_eq!(p("/Users/me/Library/CloudStorage/Acme-Work/a.pdf").0, "Acme");
        assert_eq!(p("/home/me/Dropbox/notes/OneDrive.txt").0, "Dropbox");
        assert_eq!(p("/x/Sync/a.txt"), ("cloud".into(), PathBuf::from("/x/Sync")));
    }

    #[test]
    fn cloud_files_are_kept_out_unless_the_settings_read_them() {
        let d = std::env::temp_dir().join(format!("coxswain-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("report.docx");
        std::fs::write(&f, "x").unwrap();
        assert!(!online(&f) && !keep_out(&f));
        pretend(&f, true);
        assert!(online(&f) && keep_out(&f));
        assert!(!keep_out(&d.join("missing")), "a file that is not there is nobody's business here");
        pretend(&f, false);
        assert!(!keep_out(&f));
        std::fs::remove_dir_all(d).unwrap();
    }
}

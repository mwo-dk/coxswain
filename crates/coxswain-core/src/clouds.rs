//! The cloud accounts on this machine, for the sidebar and the terminal app's go-to list: one
//! entry per account ("OneDrive – Personal", "Google Drive – you@example.com"), each the folder
//! its sync app or mount keeps. Found only from what is on the disk: the folders and settings of
//! the sync apps and the mounts. Coxswain never logs in to a cloud or asks one anything.

use crate::bsd::Place;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `places`, or none when it takes longer than `wait`: a FUSE mount whose server has gone
/// (rclone, sshfs, gvfs) can hang every look at its folder, and a list of clouds must not hang
/// the app with it. A look still hanging is not started again: none until it is back.
pub fn places_within(wait: Duration) -> Option<Vec<Place>> {
    static LOOKING: AtomicBool = AtomicBool::new(false);
    within(&LOOKING, wait, places)
}

fn within<T: Send + 'static>(
    busy: &'static AtomicBool,
    wait: Duration,
    look: fn() -> T,
) -> Option<T> {
    if busy.swap(true, Ordering::AcqRel) {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let found = look();
        busy.store(false, Ordering::Release);
        let _ = tx.send(found);
    });
    rx.recv_timeout(wait).ok()
}

/// The cloud accounts, as places of kind `cloud`, sorted by name, each folder once.
pub fn places() -> Vec<Place> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = crate::dirs::home_dir() {
        found.extend(cloud_storage(&home.join("Library/CloudStorage")));
        let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        if icloud.is_dir() {
            found.push(("iCloud Drive".into(), icloud));
        }
    }
    #[cfg(windows)]
    found.extend(sync_roots());
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let mounts = crate::machine::mountinfo(&text).map(|m| (m.point, m.fstype, m.source));
        found.extend(mounts_named(mounts));
    }
    #[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
    found.extend(mounts_named(crate::machine::mounted().into_iter().map(|m| {
        // FreeBSD names FUSE mounts `fusefs.rclone`, Linux `fuse.rclone`.
        let fstype = m.fstype.replacen("fusefs", "fuse", 1).replacen("puffs", "fuse", 1);
        (m.mount, fstype, m.device)
    })));
    found.extend(gvfs());
    if let Some(dir) = crate::dirs::preference_dir() {
        for (app, cfg) in [("Nextcloud", "nextcloud.cfg"), ("ownCloud", "owncloud.cfg")] {
            if let Ok(text) = std::fs::read_to_string(dir.join(app).join(cfg)) {
                found.extend(sync_folders(app, &text));
            }
        }
    }
    found.extend(dropbox_files().iter().flat_map(|f| {
        std::fs::read_to_string(f)
            .map(|t| dropbox(&t))
            .unwrap_or_default()
    }));
    let mut v: Vec<Place> = Vec::new();
    for (name, path) in found {
        if path.is_dir() && !v.iter().any(|p| p.path.as_deref() == Some(&path)) {
            v.push(Place {
                kind: "cloud",
                name,
                path: Some(path),
                note: String::new(),
            });
        }
    }
    v.sort_by_cached_key(|p| crate::collate::key(&p.name));
    v
}

/// "OneDrive – Personal", or the service alone when there is no account to tell.
fn label(service: &str, account: &str) -> String {
    if account.is_empty() || account.eq_ignore_ascii_case(service) {
        service.to_string()
    } else {
        format!("{service} – {account}")
    }
}

/// macOS: the File Provider folders, `OneDrive-Personal`, `GoogleDrive-you@example.com`,
/// `Dropbox`, `Box-Box`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn cloud_storage(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (service, account) = name.split_once('-').unwrap_or((&name, ""));
            let service = crate::cloud::provider(service).unwrap_or(service);
            (!name.starts_with('.')).then(|| (label(service, account), e.path()))
        })
        .collect()
}

/// Mounts of clouds: rclone, onedriver, google-drive-ocamlfuse, davfs2. A mount of a server
/// (sshfs) or gvfs's own (its accounts are read from inside it) is no cloud account.
fn mounts_named(
    mounts: impl Iterator<Item = (PathBuf, String, String)>,
) -> Vec<(String, PathBuf)> {
    mounts
        .filter(|(_, fstype, _)| {
            crate::cloud::cloud_fs(fstype) && !matches!(fstype.as_str(), "fuse.sshfs" | "fuse.gvfsd-fuse")
        })
        .map(|(point, fstype, source)| {
            let base = point
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // rclone's source is its remote, `gdrive:` or `work:Documents`.
            let remote = source.split(':').next().unwrap_or_default();
            let tool = fstype.trim_start_matches("fuse.");
            let service = crate::cloud::provider(&base)
                .or_else(|| crate::cloud::provider(remote))
                .or_else(|| crate::cloud::provider(tool))
                .unwrap_or(match tool {
                    "google-drive-ocamlfuse" => "Google Drive",
                    "davfs" => "WebDAV",
                    // FUSE that does not say what it is: its folder's name.
                    "fuse" => base.as_str(),
                    t => t,
                });
            let account = if tool == "rclone" && !remote.is_empty() { remote } else { &base };
            (label(service, account), point)
        })
        .collect()
}

/// GNOME's online accounts, mounted by gvfs as `google-drive:host=gmail.com,user=you`,
/// `onedrive:…` and `dav(s):…` folders under `/run/user/<uid>/gvfs`. Listing that folder asks
/// the local gvfs daemon, not the cloud.
fn gvfs() -> Vec<(String, PathBuf)> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("gvfs");
        if let Ok(rd) = std::fs::read_dir(dir) {
            return rd
                .flatten()
                .filter_map(|e| Some((gvfs_name(&e.file_name().to_string_lossy())?, e.path())))
                .collect();
        }
    }
    vec![]
}

/// The account a gvfs folder name stands for; none for shares that are no cloud (sftp, smb).
fn gvfs_name(name: &str) -> Option<String> {
    let (scheme, rest) = name.split_once(':')?;
    let field = |k: &str| {
        rest.split(',')
            .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
            .map(|v| v.replace("%40", "@").replace("%2F", "/"))
            .unwrap_or_default()
    };
    let (user, host) = (field("user"), field("host"));
    let at = if user.contains('@') || user.is_empty() || host.is_empty() {
        user.clone()
    } else {
        format!("{user}@{host}")
    };
    let service = match scheme {
        "google-drive" => "Google Drive",
        "onedrive" => "OneDrive",
        // GNOME's Nextcloud accounts are WebDAV at Nextcloud's own path.
        "dav" | "davs" if field("prefix").contains("remote.php") => "Nextcloud",
        "dav" | "davs" => "WebDAV",
        _ => return None,
    };
    Some(label(service, &at))
}

/// Nextcloud's and ownCloud's sync folders from their settings file: `0\Folders\1\localPath=`
/// per folder, `0\dav_user=` and `0\url=` per account.
fn sync_folders(app: &str, cfg: &str) -> Vec<(String, PathBuf)> {
    let value = |key: String| {
        cfg.lines()
            .find_map(|l| l.strip_prefix(&key)?.strip_prefix('=').map(str::trim))
            .unwrap_or_default()
            .to_string()
    };
    cfg.lines()
        .filter_map(|l| {
            let (key, path) = l.split_once('=')?;
            let mut parts = key.split('\\');
            let (account, list, _, last) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            let folders = matches!(list, "Folders" | "FoldersWithPlaceholders" | "Multifolders");
            (folders && last == "localPath" && parts.next().is_none()).then(|| {
                let user = [value(format!("{account}\\dav_user")), value(format!("{account}\\user"))]
                    .into_iter()
                    .find(|u| !u.is_empty())
                    .unwrap_or_default();
                let url = value(format!("{account}\\url"));
                let host = url.split("://").last().unwrap_or_default().split('/').next().unwrap_or_default();
                let who = match (user.is_empty(), host.is_empty()) {
                    (false, false) if !user.contains('@') => format!("{user}@{host}"),
                    (true, _) => host.to_string(),
                    _ => user,
                };
                let path = path.trim().trim_matches('"');
                (label(app, &who), PathBuf::from(path.trim_end_matches(['/', '\\'])))
            })
        })
        .collect()
}

/// Where the Dropbox app says which folders it keeps.
fn dropbox_files() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(home) = crate::dirs::home_dir() {
        v.push(home.join(".dropbox/info.json"));
    }
    #[cfg(windows)]
    v.extend([crate::dirs::config_dir(), crate::dirs::data_local_dir()].into_iter().flatten().map(|d| d.join("Dropbox\\info.json")));
    v
}

/// Dropbox's `info.json`: `{"personal": {"path": …}, "business": {"path": …, "team_name"?}}`.
fn dropbox(json: &str) -> Vec<(String, PathBuf)> {
    let Ok(serde_json::Value::Object(o)) = serde_json::from_str(json) else {
        return vec![];
    };
    o.iter()
        .filter_map(|(kind, v)| {
            let path = v.get("path")?.as_str()?;
            let account = match kind.as_str() {
                "personal" => "Personal".to_string(),
                "business" => v.get("team_name").and_then(|t| t.as_str()).unwrap_or("Business").to_string(),
                k => k.to_string(),
            };
            Some((label("Dropbox", &account), PathBuf::from(path)))
        })
        .collect()
}

/// Windows: the sync roots the Cloud Files API keeps (OneDrive, Dropbox, Google Drive, Proton
/// Drive, iCloud, Nextcloud's virtual files), with the names their apps gave them.
#[cfg(windows)]
fn sync_roots() -> Vec<(String, PathBuf)> {
    const ROOTS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager";
    let mut v = vec![];
    for id in reg::subkeys(ROOTS) {
        let key = format!(r"{ROOTS}\{id}");
        // The key is `provider!SID!account`; the display name may be a resource reference.
        let mut parts = id.split('!');
        let provider = parts.next().unwrap_or_default();
        let account = parts.nth(1).unwrap_or_default();
        let name = reg::string(&key, "DisplayNameResource")
            .filter(|n| !n.is_empty() && !n.starts_with('@'))
            .unwrap_or_else(|| {
                let service = crate::cloud::provider(provider).unwrap_or(provider);
                label(service, account.split('|').next().unwrap_or_default())
            });
        // One value per user, named by their SID. Other users' folders are not readable by
        // you, so the is_dir in `places` leaves them out.
        for path in reg::values(&format!(r"{key}\UserSyncRoots")) {
            v.push((name.clone(), PathBuf::from(path)));
        }
    }
    v
}

#[cfg(windows)]
mod reg {
    use windows_sys::Win32::System::Registry::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn open(path: &str) -> Option<HKEY> {
        let mut key = std::ptr::null_mut();
        // SAFETY: the path is NUL-terminated; `key` is written on success and closed by callers.
        let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(path).as_ptr(), 0, KEY_READ, &mut key) };
        (r == 0).then_some(key)
    }

    /// The names of the subkeys of HKLM\`path`.
    pub fn subkeys(path: &str) -> Vec<String> {
        let Some(key) = open(path) else { return vec![] };
        let mut v = vec![];
        let mut buf = [0u16; 512];
        for i in 0.. {
            let mut len = buf.len() as u32;
            // SAFETY: `buf` holds `len` characters.
            let r = unsafe {
                RegEnumKeyExW(key, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            if r != 0 {
                break;
            }
            v.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        // SAFETY: opened above.
        unsafe { RegCloseKey(key) };
        v
    }

    /// The string data of every value of HKLM\`path`.
    pub fn values(path: &str) -> Vec<String> {
        let Some(key) = open(path) else { return vec![] };
        let mut v = vec![];
        let (mut name, mut data) = ([0u16; 256], [0u16; 1024]);
        for i in 0.. {
            let (mut nlen, mut dlen, mut kind) = (name.len() as u32, (data.len() * 2) as u32, 0u32);
            // SAFETY: `name` holds `nlen` characters, `data` `dlen` bytes.
            let r = unsafe {
                RegEnumValueW(key, i, name.as_mut_ptr(), &mut nlen, std::ptr::null(), &mut kind, data.as_mut_ptr().cast(), &mut dlen)
            };
            if r != 0 {
                break;
            }
            if kind == REG_SZ || kind == REG_EXPAND_SZ {
                let s = String::from_utf16_lossy(&data[..dlen as usize / 2]);
                v.push(s.trim_end_matches('\0').to_string());
            }
        }
        // SAFETY: opened above.
        unsafe { RegCloseKey(key) };
        v
    }

    /// The string value `name` of HKLM\`path`.
    pub fn string(path: &str, name: &str) -> Option<String> {
        let mut data = [0u16; 512];
        let mut len = (data.len() * 2) as u32;
        // SAFETY: `data` holds `len` bytes; RRF_RT_REG_SZ ends the string with a NUL.
        let r = unsafe {
            RegGetValueW(HKEY_LOCAL_MACHINE, wide(path).as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), data.as_mut_ptr().cast(), &mut len)
        };
        (r == 0).then(|| String::from_utf16_lossy(&data[..(len as usize / 2).saturating_sub(1)]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hanging_look_gives_up_and_is_not_started_twice() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        // The look hangs until it is let go, so a slow machine cannot make it finish in time.
        static GO: AtomicBool = AtomicBool::new(false);
        let slow = || {
            while !GO.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        assert_eq!(within(&BUSY, Duration::from_millis(50), slow), None, "gave up");
        assert_eq!(within(&BUSY, Duration::from_secs(5), || ()), None, "the first still looks");
        GO.store(true, std::sync::atomic::Ordering::SeqCst);
        let until = std::time::Instant::now() + Duration::from_secs(30);
        while BUSY.load(std::sync::atomic::Ordering::SeqCst) && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(within(&BUSY, Duration::from_secs(5), || 7), Some(7), "back once it is done");
        assert!(places_within(Duration::from_secs(30)).is_some());
    }

    #[test]
    fn cloud_storage_folders_are_named_by_service_and_account() {
        let dir = std::env::temp_dir().join(format!("coxswain-clouds-{}", std::process::id()));
        for n in ["OneDrive-Personal", "OneDrive-Contoso", "GoogleDrive-you@example.com", "Dropbox", "Box-Box", ".Trash"] {
            std::fs::create_dir_all(dir.join(n)).unwrap();
        }
        let mut names: Vec<String> = cloud_storage(&dir).into_iter().map(|(n, _)| n).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        names.sort();
        assert_eq!(names, ["Box", "Dropbox", "Google Drive – you@example.com", "OneDrive – Contoso", "OneDrive – Personal"]);
    }

    #[test]
    fn cloud_mounts_are_named_by_remote_or_folder() {
        let m = |p: &str, t: &str, s: &str| (PathBuf::from(p), t.to_string(), s.to_string());
        let got = mounts_named(
            [
                m("/home/me/gdrive", "fuse.rclone", "gdrive:"),
                m("/home/me/work", "fuse.rclone", "work:Documents"),
                m("/home/me/OneDrive", "fuse.onedriver", "onedriver"),
                m("/home/me/server", "fuse.sshfs", "me@host:/"),
                m("/run/user/1000/gvfs", "fuse.gvfsd-fuse", "gvfsd-fuse"),
                m("/", "ext4", "/dev/sda1"),
            ]
            .into_iter(),
        );
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["rclone – gdrive", "rclone – work", "OneDrive"]);
    }

    #[test]
    fn gvfs_folders_tell_their_accounts() {
        assert_eq!(gvfs_name("google-drive:host=gmail.com,user=you").as_deref(), Some("Google Drive – you@gmail.com"));
        assert_eq!(
            gvfs_name("davs:host=cloud.example.com,ssl=true,user=me,prefix=%2Fremote.php%2Fwebdav").as_deref(),
            Some("Nextcloud – me@cloud.example.com")
        );
        assert_eq!(gvfs_name("sftp:host=server"), None);
        assert_eq!(gvfs_name("smb-share:server=nas,share=media"), None);
    }

    #[test]
    fn nextcloud_settings_give_each_sync_folder() {
        let cfg = "[Accounts]\n0\\url=https://cloud.example.com\n0\\dav_user=me\n\
                   0\\Folders\\1\\localPath=/home/me/Nextcloud/\n0\\Folders\\1\\targetPath=/\n\
                   1\\url=https://other.example.org/nc\n1\\dav_user=you@example.org\n\
                   1\\FoldersWithPlaceholders\\1\\localPath=C:/Users/you/Other/\n";
        let got = sync_folders("Nextcloud", cfg);
        assert_eq!(got[0], ("Nextcloud – me@cloud.example.com".into(), PathBuf::from("/home/me/Nextcloud")));
        assert_eq!(got[1].0, "Nextcloud – you@example.org");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn dropbox_info_gives_personal_and_team_folders() {
        let json = r#"{"personal": {"path": "/home/me/Dropbox"}, "business": {"path": "/home/me/Dropbox (Acme)", "team_name": "Acme"}}"#;
        let mut got = dropbox(json);
        got.sort();
        assert_eq!(got[0], ("Dropbox – Acme".into(), PathBuf::from("/home/me/Dropbox (Acme)")));
        assert_eq!(got[1], ("Dropbox – Personal".into(), PathBuf::from("/home/me/Dropbox")));
        assert!(dropbox("not json").is_empty());
    }
}

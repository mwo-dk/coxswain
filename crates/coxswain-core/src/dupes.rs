//! Duplicate files and folders.
//!
//! Only files that can match are ever read:
//! 1. Walk the roots in parallel and group files by size; a unique size has no duplicate.
//! 2. Hash the first 16 KB of each remaining file with BLAKE3; most false candidates split here.
//! 3. Hash the rest of the survivors in full (BLAKE3 uses SIMD: AVX2/AVX-512 on x86, NEON on ARM).
//!
//! Hardlinks to one file are not duplicates (they share the space) and are counted once.
//! Full hashes are kept in the search store by path, size and modification time, so a second
//! scan of an old disk only reads what changed; the helper hashes the home folder's files ahead. Folders whose whole contents match (names and bytes) are
//! reported as one folder group instead of one group per file inside.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use crate::store::Store;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Options {
    pub roots: Vec<PathBuf>,
    /// Smaller files are ignored (empty files are never duplicates worth reporting).
    pub min_size: u64,
    pub hidden: bool,
    /// Also report whole folders with identical contents.
    pub folders: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { roots: vec![], min_size: 1, hidden: false, folders: true }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DupFile {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub modified: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Group {
    /// BLAKE3 of the contents, hex.
    pub hash: String,
    pub size: u64,
    /// Bytes that deleting all but one copy would free.
    pub wasted: u64,
    pub files: Vec<DupFile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FolderGroup {
    pub size: u64,
    pub files: u64,
    pub wasted: u64,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub groups: Vec<Group>,
    pub folders: Vec<FolderGroup>,
    pub scanned_files: u64,
    pub scanned_bytes: u64,
    pub hashed_bytes: u64,
    pub wasted: u64,
}

/// What the scan is doing, for a progress bar. Updated from worker threads.
#[derive(Default)]
pub struct Progress {
    /// 0 walking, 1 hashing the first 16 KB, 2 hashing in full, 3 done.
    pub phase: AtomicU64,
    pub files: AtomicU64,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
}

const HEAD: usize = 16 * 1024;

struct Found {
    path: PathBuf,
    size: u64,
    modified: u64,
    /// (device, inode) on Unix; hardlinks share it.
    id: Option<(u64, u64)>,
}

#[cfg(unix)]
fn file_id(m: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_id(_: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

fn walk(dir: &Path, opts: &Options, p: &Progress, out: &mut Vec<Found>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut subdirs = vec![];
    for de in rd.flatten() {
        if p.cancel.load(Ordering::Relaxed) {
            return;
        }
        let name = de.file_name();
        if !opts.hidden && name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(ft) = de.file_type() else { continue };
        if ft.is_symlink() {
            continue; // never follow links: they would count files twice
        }
        if ft.is_dir() {
            subdirs.push(de.path());
        } else if ft.is_file() {
            let Ok(m) = de.metadata() else { continue };
            p.files.fetch_add(1, Ordering::Relaxed);
            out.push(Found {
                size: m.len(),
                modified: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
                id: file_id(&m),
                path: de.path(),
            });
        }
    }
    let nested: Vec<Vec<Found>> = subdirs
        .par_iter()
        .map(|d| {
            let mut v = vec![];
            walk(d, opts, p, &mut v);
            v
        })
        .collect();
    out.extend(nested.into_iter().flatten());
}

/// BLAKE3 of the file, or of its first `limit` bytes. `read` counts the bytes as they go.
pub(crate) fn hash_file(path: &Path, limit: Option<usize>, cancel: &AtomicBool, read: Option<&AtomicU64>) -> io::Result<blake3::Hash> {
    let mut f = fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut left = limit.unwrap_or(usize::MAX);
    while left > 0 {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let want = buf.len().min(left);
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        if let Some(read) = read {
            read.fetch_add(n as u64, Ordering::Relaxed);
        }
        left -= n;
    }
    Ok(h.finalize())
}

/// Split `groups` by `key`, keeping only subgroups with two or more files.
fn split<K: std::hash::Hash + Eq + Send>(groups: Vec<Vec<Found>>, key: impl Fn(&Found) -> Option<K> + Sync) -> Vec<Vec<Found>> {
    groups
        .into_par_iter()
        .flat_map_iter(|g| {
            let mut by: HashMap<K, Vec<Found>> = HashMap::new();
            for f in g {
                if let Some(k) = key(&f) {
                    by.entry(k).or_default().push(f);
                }
            }
            by.into_values().filter(|v| v.len() > 1)
        })
        .collect()
}

pub fn scan(opts: &Options, p: &Progress) -> Report {
    let mut all = vec![];
    for root in &opts.roots {
        walk(root, opts, p, &mut all);
    }
    let scanned_files = all.len() as u64;
    let scanned_bytes = all.iter().map(|f| f.size).sum();

    // Hardlinks: keep one path per (device, inode).
    let mut seen = std::collections::HashSet::new();
    all.retain(|f| f.id.is_none_or(|id| seen.insert(id)));

    let mut by_size: HashMap<u64, Vec<Found>> = HashMap::new();
    for f in all.into_iter().filter(|f| f.size >= opts.min_size.max(1)) {
        by_size.entry(f.size).or_default().push(f);
    }
    let candidates: Vec<Vec<Found>> = by_size.into_values().filter(|v| v.len() > 1).collect();

    // Groups whose every file the store has hashed are not read at all.
    let store = Store::path().and_then(|path| Store::open(&path).ok());
    let _ = Store::path().map(|path| fs::remove_file(path.with_file_name("dupes-hashes.json")));
    let stored = |f: &Found| store.as_ref().and_then(|s| s.hash(&f.path, f.size, f.modified));
    let (mut candidates, unknown): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|g| g.iter().all(|f| stored(f).is_some()));

    // Head hashes (skipped for files that fit in the head: their full hash comes next anyway).
    p.phase.store(1, Ordering::Relaxed);
    p.total.store(unknown.iter().map(|g| g.len() as u64).sum(), Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    candidates.extend(split(unknown, |f| {
        p.done.fetch_add(1, Ordering::Relaxed);
        if f.size as usize <= HEAD { Some(*blake3::hash(&[]).as_bytes()) } else { hash_file(&f.path, Some(HEAD), &p.cancel, Some(&p.bytes)).ok().map(|h| *h.as_bytes()) }
    }));

    // Full hashes, from the store where the file has not changed.
    p.phase.store(2, Ordering::Relaxed);
    p.total.store(candidates.iter().map(|g| g.len() as u64).sum(), Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    let (new, hashes) = (std::sync::Mutex::new(vec![]), std::sync::Mutex::new(HashMap::new()));
    let full = split(candidates, |f| {
        p.done.fetch_add(1, Ordering::Relaxed);
        let h = match stored(f) {
            Some(h) => h,
            None => {
                let h = hash_file(&f.path, None, &p.cancel, Some(&p.bytes)).ok()?.to_hex().to_string();
                new.lock().ok()?.push((f.path.clone(), f.size, f.modified, h.clone()));
                h
            }
        };
        hashes.lock().ok()?.insert(f.path.clone(), h.clone());
        Some(h)
    });
    // Whatever was hashed is kept, also when the scan was cancelled.
    if let Some(store) = &store {
        let _ = store.put_hashes(&new.into_inner().unwrap_or_default());
    }
    let hashes = hashes.into_inner().unwrap_or_default();

    let mut hash_of: HashMap<PathBuf, String> = HashMap::new();
    let mut groups: Vec<Group> = full
        .into_iter()
        .map(|g| {
            let hash = hashes[&g[0].path].clone();
            for f in &g {
                hash_of.insert(f.path.clone(), hash.clone());
            }
            let size = g[0].size;
            let mut files: Vec<DupFile> = g.into_iter().map(|f| DupFile { path: f.path, modified: f.modified }).collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            Group { wasted: size * (files.len() as u64 - 1), hash, size, files }
        })
        .collect();

    let folders = if opts.folders { folder_groups(opts, &hash_of) } else { vec![] };
    // Files inside duplicate folders are already covered by the folder group.
    let covered: Vec<&PathBuf> = folders.iter().flat_map(|g| g.paths.iter()).collect();
    groups.retain(|g| !g.files.iter().all(|f| covered.iter().any(|d| f.path.starts_with(d))));
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then_with(|| a.files[0].path.cmp(&b.files[0].path)));

    p.phase.store(3, Ordering::Relaxed);
    let wasted = groups.iter().map(|g| g.wasted).sum::<u64>() + folders.iter().map(|g| g.wasted).sum::<u64>();
    Report { groups, folders, scanned_files, scanned_bytes, hashed_bytes: p.bytes.load(Ordering::Relaxed), wasted }
}

/// Folders with identical contents: the same files by content, whatever the files and
/// subfolders are called and however they are arranged. A folder containing any file without a
/// duplicate (never hashed) has no twin, so only folders made entirely of duplicated files can
/// match. Only the topmost matching folders are reported.
fn folder_groups(opts: &Options, hash_of: &HashMap<PathBuf, String>) -> Vec<FolderGroup> {
    // Every file's (hash, size) below `dir`, or None when one of them has no duplicate. The
    // sorted list is the folder's signature; `out` gets (signature, size, files) per folder.
    fn contents(dir: &Path, opts: &Options, hash_of: &HashMap<PathBuf, String>, out: &mut HashMap<PathBuf, (blake3::Hash, u64, u64)>) -> Option<Vec<(String, u64)>> {
        let mut all = vec![];
        let mut complete = true;
        for de in fs::read_dir(dir).ok()?.flatten() {
            if !opts.hidden && de.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(ft) = de.file_type() else { return None };
            if ft.is_symlink() {
                complete = false;
                continue;
            }
            let p = de.path();
            if ft.is_dir() {
                // Keep descending even when this folder cannot match: a subfolder still might.
                match contents(&p, opts, hash_of, out) {
                    Some(v) => all.extend(v),
                    None => complete = false,
                }
            } else {
                match (hash_of.get(&p), de.metadata()) {
                    (Some(h), Ok(m)) => all.push((h.clone(), m.len())),
                    _ => complete = false,
                }
            }
        }
        if !complete || all.is_empty() {
            return None;
        }
        all.sort();
        let mut h = blake3::Hasher::new();
        for (hash, _) in &all {
            h.update(hash.as_bytes());
        }
        out.insert(dir.to_path_buf(), (h.finalize(), all.iter().map(|x| x.1).sum(), all.len() as u64));
        Some(all)
    }
    let mut sigs = HashMap::new();
    for root in &opts.roots {
        contents(root, opts, hash_of, &mut sigs);
    }
    let mut by: HashMap<blake3::Hash, Vec<(PathBuf, u64, u64)>> = HashMap::new();
    for (dir, (h, size, files)) in sigs {
        by.entry(h).or_default().push((dir, size, files));
    }
    let mut groups: Vec<FolderGroup> = by
        .into_values()
        .map(|mut v| {
            // A folder holding nothing but one subfolder has that subfolder's contents: keep
            // the outer one.
            let all: Vec<PathBuf> = v.iter().map(|x| x.0.clone()).collect();
            v.retain(|x| !all.iter().any(|q| *q != x.0 && x.0.starts_with(q)));
            v
        })
        .filter(|v| v.len() > 1)
        .map(|mut v| {
            v.sort();
            let (size, files) = (v[0].1, v[0].2);
            FolderGroup { size, files, wasted: size * (v.len() as u64 - 1), paths: v.into_iter().map(|x| x.0).collect() }
        })
        .collect();
    // Keep only the topmost: drop a group whose folders all sit inside another group's folders.
    let all_paths: Vec<PathBuf> = groups.iter().flat_map(|g| g.paths.clone()).collect();
    groups.retain(|g| !g.paths.iter().all(|p| all_paths.iter().any(|q| q != p && p.starts_with(q))));
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dupes_files_folders_and_hardlinks() {
        let d = std::env::temp_dir().join(format!("coxswain-test-dupes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for sub in ["a/pics", "backup/pics", "other"] {
            fs::create_dir_all(d.join(sub)).unwrap();
        }
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let mut big2 = big.clone();
        *big2.last_mut().unwrap() ^= 1; // same size and head, different tail
        fs::write(d.join("a/pics/one.jpg"), &big).unwrap();
        fs::write(d.join("a/pics/two.jpg"), b"small but duplicated").unwrap();
        fs::write(d.join("backup/pics/renamed.jpg"), &big).unwrap(); // names do not matter
        fs::write(d.join("backup/pics/two.jpg"), b"small but duplicated").unwrap();
        fs::write(d.join("other/near.jpg"), &big2).unwrap();
        fs::write(d.join("other/copy.txt"), b"small but duplicated").unwrap();
        fs::write(d.join("other/unique.txt"), b"only me").unwrap();
        // Without this, `a` and `backup` would be identical folders themselves.
        fs::write(d.join("a/notes.txt"), b"a has more than pics").unwrap();
        #[cfg(unix)]
        fs::hard_link(d.join("other/unique.txt"), d.join("other/link.txt")).unwrap();

        let r = scan(&Options { roots: vec![d.clone()], ..Default::default() }, &Progress::default());
        // a/pics and backup/pics are identical folders: one folder group.
        assert_eq!(r.folders.len(), 1);
        // `backup` holds only `pics`, so it is the copy reported (its contents are the same).
        assert_eq!(r.folders[0].paths, [d.join("a/pics"), d.join("backup")], "same contents, different names");
        assert_eq!(r.folders[0].files, 2);
        // The small file also exists outside those folders, so its group stays; the 100 KB
        // file with a different last byte is not a duplicate; the hardlink is not either.
        assert_eq!(r.groups.len(), 1, "{:?}", r.groups);
        assert_eq!(r.groups[0].files.len(), 3);
        assert_eq!(r.groups[0].size, 20);
        assert_eq!(r.wasted, 2 * 20 + 100_020);

        // Without folder grouping every duplicated file shows as a file group.
        let r = scan(&Options { roots: vec![d.clone()], folders: false, ..Default::default() }, &Progress::default());
        assert_eq!(r.groups.len(), 2);
        assert_eq!(r.groups[0].size, 100_000, "sorted by wasted space");
        fs::remove_dir_all(d).unwrap();
    }
}

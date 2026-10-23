//! Split and join (docs/files/split-and-join.md), in both apps: a big file cut into parts of a
//! chosen size, `name.001`, `name.002` …, with `name.sha256` of the whole file beside them; and
//! the parts joined back into `name`, checked against that `name.sha256` when it is there. Read
//! and written a megabyte at a time, with progress and Stop; nothing is written over, and a
//! stopped or failed run leaves nothing half made.

use crate::checksum::{self, Algo, Progress};
use sha2::Digest;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MB: u64 = 1 << 20;

/// The sizes offered, with the name of each: (bytes, `split.size.<id>`).
pub const PRESETS: [(u64, &str); 5] = [
    (100 * MB, "100m"),
    (650 * MB, "cd650"),
    (700 * MB, "cd700"),
    // FAT32 takes files up to 4 GiB less a byte.
    ((4 << 30) - 1, "fat32"),
    (4_700_000_000, "dvd"),
];

/// A size as typed: `250M`, `1.5G`, `700 MB`, `4096K`, or bytes. K, M, G and T count in 1024s, as
/// `split -b` does.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_uppercase();
    let s = s.trim_end_matches('B').trim_end_matches('I').trim();
    let (num, mul) = match s.chars().last()? {
        'K' => (&s[..s.len() - 1], 1u64 << 10),
        'M' => (&s[..s.len() - 1], 1 << 20),
        'G' => (&s[..s.len() - 1], 1 << 30),
        'T' => (&s[..s.len() - 1], 1 << 40),
        _ => (s, 1),
    };
    let n: f64 = num.trim().replace(',', ".").parse().ok()?;
    let bytes = (n * mul as f64) as u64;
    (n.is_finite() && bytes > 0).then_some(bytes)
}

/// The name of part `i` (from 1) of `file`: `file.001`; four digits and more past 999.
fn part(file: &Path, i: usize, digits: usize) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{i:0digits$}"));
    file.with_file_name(name)
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn taken(p: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, crate::t!("split.taken", "name" => p.file_name().unwrap_or_default().to_string_lossy()))
}

/// What a split made.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Split {
    pub parts: Vec<PathBuf>,
    pub sums: PathBuf,
}

/// Copy `n` bytes from `r` to `w` (all of it when `n` is `None`), hashing them, a megabyte at a
/// time; `read` hears each piece.
fn copy(r: &mut impl Read, w: &mut impl Write, n: Option<u64>, h: &mut sha2::Sha256, stop: &AtomicBool, read: &mut impl FnMut(u64)) -> io::Result<u64> {
    let mut buf = vec![0u8; MB as usize];
    let mut done = 0u64;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, crate::t!("split.stopped")));
        }
        let want = n.map_or(buf.len() as u64, |n| (n - done).min(buf.len() as u64)) as usize;
        if want == 0 {
            break;
        }
        let got = match r.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(g) => g,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        w.write_all(&buf[..got])?;
        h.update(&buf[..got]);
        done += got as u64;
        read(got as u64);
    }
    Ok(done)
}

fn hex(h: sha2::Sha256) -> String {
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Cut `file` into parts of `size` bytes beside it, and write `file.sha256` of the whole. Refused
/// when a part's name or the list's is taken, or the file fits in one part. A stopped or failed
/// split removes what it made.
pub fn split(file: &Path, size: u64, stop: &AtomicBool, mut at: impl FnMut(&Progress)) -> io::Result<Split> {
    let len = fs::metadata(file)?.len();
    if size == 0 || len <= size {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, crate::t!("split.too_small", "size" => crate::settings::human(len))));
    }
    let n = len.div_ceil(size) as usize;
    let digits = n.to_string().len().max(3);
    let parts: Vec<PathBuf> = (1..=n).map(|i| part(file, i, digits)).collect();
    let mut sums_name = file.file_name().unwrap_or_default().to_os_string();
    sums_name.push(".sha256");
    let sums = file.with_file_name(sums_name);
    if let Some(p) = parts.iter().chain([&sums]).find(|p| exists(p)) {
        return Err(taken(p));
    }
    let mut made: Vec<PathBuf> = vec![];
    let r = (|| {
        let mut src = File::open(file)?;
        let mut h = sha2::Sha256::new();
        let mut p = Progress { all: n, total: len, ..Default::default() };
        for (i, path) in parts.iter().enumerate() {
            (p.at, p.name) = (i, path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            at(&p);
            let mut out = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
            made.push(path.clone());
            copy(&mut src, &mut out, Some(size), &mut h, stop, &mut |k| {
                p.done += k;
                at(&p);
            })?;
            out.sync_all()?;
        }
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&sums)?;
        made.push(sums.clone());
        f.write_all(format!("{}  {name}\n", hex(h)).as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = r {
        made.iter().for_each(|p| drop(fs::remove_file(p)));
        return Err(e);
    }
    Ok(Split { parts, sums })
}

/// `name.001` is the first part of `name`: what it joins into.
pub fn joins_into(first: &Path) -> Option<PathBuf> {
    let name = first.file_name()?.to_str()?;
    let (stem, n) = name.rsplit_once('.')?;
    (n.len() >= 3 && n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u32>() == Ok(1) && !stem.is_empty())
        .then(|| first.with_file_name(stem))
}

/// The parts that follow `first`, in order: `.001`, `.002` … while they are there.
pub fn parts_of(first: &Path) -> Vec<PathBuf> {
    let Some(target) = joins_into(first) else { return vec![] };
    let digits = first.extension().map_or(3, |e| e.len());
    (1..).map(|i| part(&target, i, digits)).take_while(|p| fs::metadata(p).is_ok_and(|m| m.is_file())).collect()
}

/// What a join made, and whether it matches `name.sha256` (`None`: there is none, or it does not
/// name this file).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Joined {
    pub path: PathBuf,
    pub parts: usize,
    pub matches: Option<bool>,
}

/// Join `first` and the parts after it into the file they were cut from, beside them. A file of
/// that name is replaced only with `replace`. Written beside it under another name first, so a
/// stopped or failed join leaves the old file (or none) as it was.
pub fn join(first: &Path, replace: bool, stop: &AtomicBool, mut at: impl FnMut(&Progress)) -> io::Result<Joined> {
    let target = joins_into(first).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, crate::t!("split.not_first")))?;
    if exists(&target) && !replace {
        return Err(taken(&target));
    }
    let parts = parts_of(first);
    let total = parts.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let tmp = target.with_file_name(format!(".{name}.coxswain-join-{}.tmp", std::process::id()));
    let mut h = sha2::Sha256::new();
    let r = (|| {
        let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let mut p = Progress { all: parts.len(), total, ..Default::default() };
        for (i, path) in parts.iter().enumerate() {
            (p.at, p.name) = (i, path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            at(&p);
            copy(&mut File::open(path)?, &mut out, None, &mut h, stop, &mut |k| {
                p.done += k;
                at(&p);
            })?;
        }
        out.sync_all()?;
        fs::rename(&tmp, &target)
    })();
    if let Err(e) = r {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // The whole file's sum, when `name.sha256` gives it.
    let mut list = target.clone().into_os_string();
    list.push(".sha256");
    let list = PathBuf::from(list);
    let matches = fs::read_to_string(&list)
        .ok()
        .and_then(|text| checksum::parse(&text, &list).into_iter().find(|e| e.name == name && e.algo == Algo::Sha256))
        .map(|e| e.hash == hex(h));
    Ok(Joined { path: target, parts: parts.len(), matches })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_split_and_joined_back_and_checked() {
        let d = crate::fs::Scratch::named("split");
        let f = d.join("big.bin");
        let data: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&f, &data).unwrap();
        let no = AtomicBool::new(false);
        let s = split(&f, MB, &no, |_| {}).unwrap();
        assert_eq!(s.parts.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect::<Vec<_>>(), ["big.bin.001", "big.bin.002", "big.bin.003"]);
        assert_eq!(fs::metadata(&s.parts[2]).unwrap().len(), 2_500_000 - 2 * MB);
        // Parts are never written over.
        assert_eq!(split(&f, MB, &no, |_| {}).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        // The joined file is not written over without asking.
        assert_eq!(join(&s.parts[0], false, &no, |_| {}).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        fs::remove_file(&f).unwrap();
        assert_eq!(joins_into(&s.parts[0]), Some(f.clone()));
        assert!(joins_into(&s.parts[1]).is_none());
        let j = join(&s.parts[0], false, &no, |_| {}).unwrap();
        assert_eq!((j.parts, j.matches), (3, Some(true)));
        assert_eq!(fs::read(&f).unwrap(), data);
        // A part changed: the join says it does not match.
        fs::write(&s.parts[1], vec![0u8; MB as usize]).unwrap();
        assert_eq!(join(&s.parts[0], true, &no, |_| {}).unwrap().matches, Some(false));
        // Stopped: nothing is left of a split, and the file is as it was after a join.
        let g = d.join("other.bin");
        fs::write(&g, &data).unwrap();
        let yes = AtomicBool::new(true);
        assert!(split(&g, MB, &yes, |_| {}).is_err());
        assert_eq!(fs::read_dir(&*d).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("other")).count(), 1);
        let before = fs::read(&f).unwrap();
        assert!(join(&s.parts[0], true, &yes, |_| {}).is_err());
        assert_eq!(fs::read(&f).unwrap(), before);
        assert!(split(&g, 10 * MB, &no, |_| {}).is_err(), "it fits in one part");
    }

    #[test]
    fn sizes_are_read_as_typed() {
        assert_eq!(parse_size("250M"), Some(250 * MB));
        assert_eq!(parse_size("1.5 GB"), Some(3 << 29));
        assert_eq!(parse_size("4096k"), Some(4 * MB));
        assert_eq!(parse_size("700 MiB"), Some(700 * MB));
        assert_eq!(parse_size("1000"), Some(1000));
        assert_eq!(parse_size("0"), None);
        assert_eq!(parse_size("lots"), None);
    }
}

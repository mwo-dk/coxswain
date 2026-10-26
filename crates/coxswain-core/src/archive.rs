//! Archives: zip (and the formats that are zips: jar, apk, whl, nupkg, vsix), tar, tar
//! compressed with gzip, bzip2, xz or zstd, and 7z. List, extract, and inside them read and
//! write as in a folder. All in pure Rust. Entries that would land outside the destination
//! (`..`, absolute paths) are refused. Disk images open here too, read-only, when that option
//! is on: `disk_image` lists them and copies out of them with an installed 7-Zip. So do a
//! comic (`.cbz`, a zip of pictures) and a mailbox (`.mbox`, read-only: one `.eml` a message),
//! which search does not look into as archives.

use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ArchiveEntry {
    /// Path inside the archive, `/`-separated.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// Seconds since the Unix epoch; 0 when the archive does not say.
    pub modified: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Zip,
    Tar(Pack),
    SevenZ,
    /// A disk image, read with 7-Zip (`disk_image`): never written.
    Disk,
    /// A mailbox, its messages as files (`extract::mail::mailbox`): never written.
    Mbox,
    /// A container image, its layers merged (`container_image`): never written.
    Image,
}

/// How a tar is compressed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Pack {
    None,
    Gz,
    Bz2,
    Xz,
    Zst,
}

/// The endings each kind is known by, longest first where one ends another.
const ENDINGS: &[(&str, Kind)] = &[
    (".tar.gz", Kind::Tar(Pack::Gz)),
    (".tgz", Kind::Tar(Pack::Gz)),
    (".tar.bz2", Kind::Tar(Pack::Bz2)),
    (".tbz2", Kind::Tar(Pack::Bz2)),
    (".tbz", Kind::Tar(Pack::Bz2)),
    (".tar.xz", Kind::Tar(Pack::Xz)),
    (".txz", Kind::Tar(Pack::Xz)),
    (".tar.zst", Kind::Tar(Pack::Zst)),
    (".tzst", Kind::Tar(Pack::Zst)),
    (".tar", Kind::Tar(Pack::None)),
    (".7z", Kind::SevenZ),
    // The first part of a 7z split into parts: the parts are read joined, never written.
    (".7z.001", Kind::SevenZ),
    (".zip", Kind::Zip),
    // A zip split into parts (7-Zip's -v, or `split`): read joined, never written.
    (".zip.001", Kind::Zip),
    (".jar", Kind::Zip),
    (".apk", Kind::Zip),
    (".nupkg", Kind::Zip),
    (".whl", Kind::Zip),
    (".vsix", Kind::Zip),
];

/// What opens like a folder but is no archive search looks into: a comic, a mailbox.
const OPEN_ONLY: &[(&str, Kind)] = &[(".cbz", Kind::Zip), (".mbox", Kind::Mbox)];

/// What `path` opens as: an archive by its ending, a comic or a mailbox, else a disk image
/// when those open, or an OCI layout's `index.json`. A plain tar that is a container image
/// (read once per change) opens as that image.
fn kind(path: &Path) -> Option<Kind> {
    match named_kind(path)? {
        Kind::Tar(Pack::None) if crate::container_image::is_image(path) => Some(Kind::Image),
        k => Some(k),
    }
}

/// `kind`, from the name alone: a container image's tar is still a tar.
fn named_kind(path: &Path) -> Option<Kind> {
    ending_kind(path)
        .or_else(|| ends_in(path, OPEN_ONLY))
        .or_else(|| crate::disk_image::opens(path).then_some(Kind::Disk))
        .or_else(|| crate::container_image::layout_index(path).then_some(Kind::Image))
}

/// The archive `path` is by its ending (never a disk image).
fn ending_kind(path: &Path) -> Option<Kind> {
    ends_in(path, ENDINGS)
}

fn ends_in(path: &Path, list: &[(&str, Kind)]) -> Option<Kind> {
    // Asked of every file the name index walks: no allocation.
    let n = path.file_name()?.as_encoded_bytes();
    list.iter()
        .find(|(e, _)| {
            n.len() >= e.len() && n[n.len() - e.len()..].eq_ignore_ascii_case(e.as_bytes())
        })
        .map(|(_, k)| *k)
}

/// An archive by its ending, or an OCI layout's `index.json`: one search looks into. Never a
/// disk image (`opens`).
pub fn is_archive(path: &Path) -> bool {
    search_kind(path).is_some()
}

/// `is_archive` of the file `name` in `dir`, the folder looked at only for an `index.json`.
pub(crate) fn is_archive_in(dir: &Path, name: &str) -> bool {
    is_archive(Path::new(name)) || name == "index.json" && is_archive(&dir.join(name))
}

/// What search looks into `path` as: its ending's kind, or an image for an OCI layout.
fn search_kind(path: &Path) -> Option<Kind> {
    ending_kind(path).or_else(|| crate::container_image::layout_index(path).then_some(Kind::Image))
}

/// `path` opens like a folder: an archive, or a disk image when those open.
pub fn opens(path: &Path) -> bool {
    named_kind(path).is_some()
}

/// What Enter does on `path` on the disk when it opens like a folder (`enter`).
pub(crate) fn opens_as(path: &Path) -> Option<crate::enter::Enter> {
    kind(path).map(enter_of)
}

/// What Enter does on the file `path` inside an archive when it opens like a folder there.
pub(crate) fn opens_inside_as(path: &Path) -> Option<crate::enter::Enter> {
    opens_inside(path).then(|| ending_kind(path).or_else(|| ends_in(path, OPEN_ONLY)).map_or(crate::enter::Enter::DiskImage, enter_of))
}

fn enter_of(k: Kind) -> crate::enter::Enter {
    use crate::enter::Enter;
    match k {
        Kind::Zip | Kind::Tar(_) | Kind::SevenZ => Enter::Archive,
        Kind::Disk => Enter::DiskImage,
        Kind::Mbox => Enter::Mailbox,
        Kind::Image => Enter::Image,
    }
}

/// `archive` is opened like a folder read-only: a disk image, a mailbox, a container image, or
/// an archive split into parts.
pub fn read_only(archive: &Path) -> bool {
    matches!(kind(archive), Some(Kind::Disk | Kind::Mbox | Kind::Image)) || in_parts(archive) || split_zip(archive)
}

/// `name` is the first part of a 7z or zip split into parts by 7-Zip or `split` (`x.7z.001`,
/// `x.zip.001`; `.002` … next to it).
fn in_parts(name: &Path) -> bool {
    let n = name.to_string_lossy().to_ascii_lowercase();
    n.ends_with(".7z.001") || n.ends_with(".zip.001")
}

/// The number of a later part of a 7z split into parts (`x.7z.002`: 2), and the name of its
/// first part (`x.7z.001`); `None` for any other name, the first part's too.
pub fn part_of(name: &str) -> Option<(u32, String)> {
    let (first, n) = name.rsplit_once('.')?;
    if n.len() < 3 || !n.bytes().all(|b| b.is_ascii_digit()) || !first.to_ascii_lowercase().ends_with(".7z") {
        return None;
    }
    let n: u32 = n.parse().ok()?;
    (n > 1).then(|| (n, format!("{first}.{:0w$}", 1, w = 3)))
}

/// The parts before `name`, the last part of a zip split into parts (`x.zip`, with `x.z01`,
/// `x.z02` … beside it, as Info-ZIP and WinZip split one): their names in order, as long as
/// `exists` says each is there. Empty for any other name.
fn zip_parts(name: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let Some(stem) = name.len().checked_sub(4).filter(|&i| name.is_char_boundary(i) && name[i..].eq_ignore_ascii_case(".zip")).map(|i| &name[..i]) else {
        return vec![];
    };
    let z = if name.ends_with(".ZIP") { 'Z' } else { 'z' };
    (1..=99).map(|n| format!("{stem}.{z}{n:02}")).take_while(|p| exists(p)).collect()
}

/// `path` is a zip split into parts on disk: `x.z01` is beside it.
fn split_zip(path: &Path) -> bool {
    !zip_parts(&path.to_string_lossy(), |p| Path::new(p).is_file()).is_empty()
}

/// The number of an earlier part of a zip split into parts (`x.z02`: 2), and the name of the
/// part it opens from (`x.zip`); `None` for any other name.
pub fn zip_part_of(name: &str) -> Option<(u32, String)> {
    let (stem, e) = name.rsplit_once('.')?;
    let n = e.strip_prefix(['z', 'Z']).filter(|d| d.len() == 2 && d.bytes().all(|b| b.is_ascii_digit()))?;
    let n: u32 = n.parse().ok().filter(|&n| n > 0)?;
    Some((n, format!("{stem}.{}", if e.starts_with('Z') { "ZIP" } else { "zip" })))
}

/// The name of part `n` of the 7z split into parts whose first part is `first` (`x.7z.001`).
fn part_name(first: &str, n: u32) -> String {
    format!("{}{n:03}", &first[..first.len() - 3])
}

/// An archive inside another that is stored there as it is (a zip's entry not packed, a
/// plain tar's file): read in place, as a range of the outer file, never copied. Its name in
/// the cache (`marker`) is an empty file, so a path through it is a path as any other.
struct Window {
    marker: PathBuf,
    file: PathBuf,
    start: u64,
    len: u64,
}

static WINDOWS: std::sync::Mutex<Vec<Window>> = std::sync::Mutex::new(Vec::new());

/// The file and the range of it that `path` is, when it is an archive read in place.
fn window_of(path: &Path) -> Option<(PathBuf, u64, u64)> {
    WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|w| w.marker == path)
        .map(|w| (w.file.clone(), w.start, w.len))
}

/// An archive's bytes: its file, or the range of another it is read in place from.
enum Raw {
    File(File),
    Part { f: File, start: u64, len: u64, at: u64 },
}

impl Read for Raw {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Raw::File(f) => f.read(buf),
            Raw::Part { f, start, len, at } => {
                let want = buf.len().min(usize::try_from(len.saturating_sub(*at)).unwrap_or(usize::MAX));
                if want == 0 {
                    return Ok(0);
                }
                f.seek(io::SeekFrom::Start(*start + *at))?;
                let n = f.read(&mut buf[..want])?;
                *at += n as u64;
                Ok(n)
            }
        }
    }
}

impl Seek for Raw {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        match self {
            Raw::File(f) => f.seek(to),
            Raw::Part { len, at, .. } => {
                let next = match to {
                    io::SeekFrom::Start(n) => Some(n),
                    io::SeekFrom::End(d) => len.checked_add_signed(d),
                    io::SeekFrom::Current(d) => at.checked_add_signed(d),
                };
                *at = next.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start"))?;
                Ok(*at)
            }
        }
    }
}

/// The bytes of the archive at `path`: its file, or the range it is read in place from.
fn open_raw(path: &Path) -> io::Result<Raw> {
    Ok(match window_of(path) {
        Some((file, start, len)) => Raw::Part { f: File::open(file)?, start, len, at: 0 },
        None => Raw::File(File::open(path)?),
    })
}

/// How long the archive at `path` is (`open_raw`).
fn raw_len(path: &Path) -> io::Result<u64> {
    match window_of(path) {
        Some((.., len)) => Ok(len),
        None => Ok(std::fs::metadata(path)?.len()),
    }
}

/// The files of a 7z or zip read as one: `x.7z` alone, every part of one split into parts, from
/// its first, `x.7z.001` (or `x.zip.001`), to the last of `.002` … that is there, or a zip split
/// into parts (`spanned`).
struct Joined {
    /// Each part, and where in the whole it starts.
    parts: Vec<(Raw, u64)>,
    /// What stands in for the parts from `tail_at` on: a split zip's list of contents, rewritten.
    tail: Vec<u8>,
    tail_at: u64,
    len: u64,
    at: u64,
}

/// The most a split zip's list of contents may be: it is held in memory, rewritten.
const ZIP_PARTS_LIST_MAX: u64 = 64 << 20;

impl Joined {
    fn open(path: &Path) -> io::Result<Joined> {
        let zipped = zip_parts(&path.to_string_lossy(), |p| Path::new(p).is_file());
        if !zipped.is_empty() {
            let mut all: Vec<PathBuf> = zipped.into_iter().map(PathBuf::from).collect();
            all.push(path.to_path_buf());
            return Joined::spanned(&all);
        }
        let mut paths = vec![path.to_path_buf()];
        if in_parts(path) {
            let first = path.to_string_lossy().into_owned();
            for n in 2..=999 {
                let p = PathBuf::from(part_name(&first, n));
                if !p.is_file() {
                    break;
                }
                paths.push(p);
            }
        }
        let (mut parts, mut len) = (vec![], 0);
        for p in paths {
            let size = raw_len(&p)?;
            parts.push((open_raw(&p)?, len));
            len += size;
        }
        Ok(Joined { parts, tail: vec![], tail_at: len, len, at: 0 })
    }

    /// A zip split into parts (`x.z01` … then `x.zip`, the last), as the one zip the zip
    /// reader takes: the parts joined, and the list of contents (from the last part) rewritten
    /// so that each entry's place counts from the start of the first part and every part
    /// number is 0. A part missing is named; a zip64 one is refused.
    fn spanned(paths: &[PathBuf]) -> io::Result<Joined> {
        let mut j = Joined { parts: vec![], tail: vec![], tail_at: 0, len: 0, at: 0 };
        for p in paths {
            let size = raw_len(p)?;
            j.parts.push((open_raw(p)?, j.len));
            j.len += size;
        }
        j.tail_at = j.len;
        let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, format!("not a zip split into parts: {what}"));
        let last_start = j.parts.last().map_or(0, |p| p.1);
        let back = (j.len - last_start).min(22 + 65_535);
        let mut end = vec![0u8; back as usize];
        j.seek(io::SeekFrom::Start(j.len - back))?;
        j.read_exact(&mut end)?;
        let e = end.windows(4).rposition(|w| w == b"PK\x05\x06").filter(|&e| end.len() - e >= 22).ok_or_else(|| bad("no end record"))?;
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let record = end[e..].to_vec();
        let (disk, cd_disk, entries, cd_size, cd_off) = (u16_at(&record, 4), u16_at(&record, 6), u16_at(&record, 10), u32_at(&record, 12), u32_at(&record, 16));
        if disk as usize + 1 > paths.len() {
            let name = paths[0].file_name().unwrap_or_default().to_string_lossy().into_owned();
            let missing = format!("{}{:02}", &name[..name.len() - 2], paths.len());
            return Err(io::Error::new(io::ErrorKind::NotFound, crate::t!("archive.zip_part_missing", "name" => missing)));
        }
        if disk as usize + 1 != paths.len() || cd_disk > disk {
            return Err(bad("its parts do not match"));
        }
        if entries == u16::MAX || cd_off == u32::MAX || cd_size == u32::MAX {
            return Err(io::Error::other(crate::t!("archive.zip_parts_zip64")));
        }
        if cd_size as u64 > ZIP_PARTS_LIST_MAX {
            return Err(io::Error::other(crate::t!("archive.header_too_big", "mb" => cd_size >> 20, "max" => ZIP_PARTS_LIST_MAX >> 20)));
        }
        let starts: Vec<u64> = j.parts.iter().map(|p| p.1).collect();
        let start = |d: u16| starts.get(d as usize).copied().ok_or_else(|| bad("a part number past the parts"));
        let cd_at = start(cd_disk)? + cd_off as u64;
        if cd_at + cd_size as u64 > j.len {
            return Err(bad("its list of contents is past the end"));
        }
        let mut cd = vec![0u8; cd_size as usize];
        j.seek(io::SeekFrom::Start(cd_at))?;
        j.read_exact(&mut cd)?;
        let mut i = 0usize;
        for _ in 0..entries {
            if i + 46 > cd.len() || cd[i..i + 4] != *b"PK\x01\x02" {
                return Err(bad("an entry of its list of contents"));
            }
            let (d, off) = (u16_at(&cd, i + 34), u32_at(&cd, i + 42));
            if d == u16::MAX || off == u32::MAX {
                return Err(io::Error::other(crate::t!("archive.zip_parts_zip64")));
            }
            let at = u32::try_from(start(d)? + off as u64).map_err(|_| io::Error::other(crate::t!("archive.zip_parts_zip64")))?;
            cd[i + 34..i + 36].copy_from_slice(&0u16.to_le_bytes());
            cd[i + 42..i + 46].copy_from_slice(&at.to_le_bytes());
            i += 46 + u16_at(&cd, i + 28) as usize + u16_at(&cd, i + 30) as usize + u16_at(&cd, i + 32) as usize;
        }
        let cd_at32 = u32::try_from(cd_at).map_err(|_| io::Error::other(crate::t!("archive.zip_parts_zip64")))?;
        let mut tail = cd;
        let mut rec = record;
        rec[4..8].copy_from_slice(&[0, 0, 0, 0]);
        rec[8..10].copy_from_slice(&entries.to_le_bytes());
        rec[16..20].copy_from_slice(&cd_at32.to_le_bytes());
        tail.extend(rec);
        j.len = cd_at + tail.len() as u64;
        j.tail_at = cd_at;
        j.tail = tail;
        j.at = 0;
        Ok(j)
    }
}

impl Read for Joined {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.tail_at {
            let from = usize::try_from(self.at - self.tail_at).unwrap_or(usize::MAX).min(self.tail.len());
            let n = buf.len().min(self.tail.len() - from);
            buf[..n].copy_from_slice(&self.tail[from..from + n]);
            self.at += n as u64;
            return Ok(n);
        }
        let i = self.parts.partition_point(|(_, start)| *start <= self.at).saturating_sub(1);
        let end = self.parts.get(i + 1).map_or(self.tail_at, |(_, s)| *s).min(self.tail_at);
        if self.at >= end || buf.is_empty() {
            return Ok(0);
        }
        let (f, start) = &mut self.parts[i];
        f.seek(io::SeekFrom::Start(self.at - *start))?;
        let want = buf.len().min(usize::try_from(end - self.at).unwrap_or(usize::MAX));
        let n = f.read(&mut buf[..want])?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Joined {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        let at = match to {
            io::SeekFrom::Start(n) => Some(n),
            io::SeekFrom::End(d) => self.len.checked_add_signed(d),
            io::SeekFrom::Current(d) => self.at.checked_add_signed(d),
        };
        self.at = at.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start"))?;
        Ok(self.at)
    }
}

/// `archive` is a container image (`container_image`).
pub fn is_image(archive: &Path) -> bool {
    kind(archive) == Some(Kind::Image)
}

/// `archive` is a comic: a zip of pictures, its pages shown one by one in the desktop app.
pub fn is_comic(path: &Path) -> bool {
    ends_in(path, &OPEN_ONLY[..1]).is_some()
}

/// The pages of a comic: its pictures, in the order of their names (`collate`: 2 before 10).
pub fn pages(comic: &Path) -> io::Result<Vec<String>> {
    let mut v: Vec<String> = items(comic)?
        .iter()
        .filter(|it| !it.dir && crate::pictures::is_page(Path::new(&it.name)))
        .map(|it| it.name.clone())
        .collect();
    v.sort_by_cached_key(|n| crate::collate::key(n));
    Ok(v)
}

/// The messages of a mailbox, from the last look when it has not changed since.
fn messages(mbox: &Path) -> io::Result<Arc<[crate::extract::mail::Listed]>> {
    type Kept = Option<(PathBuf, Stamp, Arc<[crate::extract::mail::Listed]>)>;
    static KEPT: std::sync::Mutex<Kept> = std::sync::Mutex::new(None);
    let st = stamp(mbox)?;
    if let Some((p, s, m)) = KEPT.lock().unwrap().as_ref()
        && p == mbox
        && *s == st
    {
        return Ok(m.clone());
    }
    let m: Arc<[_]> = crate::extract::mail::mailbox(mbox)?.into();
    *KEPT.lock().unwrap() = Some((mbox.to_path_buf(), st, m.clone()));
    Ok(m)
}

/// The file name a message of a mailbox has inside it: its number, date, sender and subject.
fn message_name(i: usize, digits: usize, m: &crate::extract::mail::Listed) -> String {
    let date = chrono::DateTime::from_timestamp(m.secs, 0)
        .filter(|_| m.secs > 0)
        .map(|d| d.format("%Y-%m-%d").to_string());
    let words: Vec<&str> = [date.as_deref().unwrap_or(""), &m.from, &m.subject]
        .into_iter()
        .filter(|w| !w.trim().is_empty())
        .collect();
    // Controls, the characters no file system takes, and those that turn text around or hide
    // (U+200B–U+200F, U+202A–U+202E, U+2066–U+2069) become `_`; at most 100 bytes, a whole
    // character at the end, so the name stays far below the 255 bytes a file name may have.
    let hidden = |c: char| matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let mut name = String::new();
    for c in words.join(" - ").chars() {
        let c = if c.is_control() || hidden(c) || "/\\:*?\"<>|".contains(c) { '_' } else { c };
        if name.len() + c.len_utf8() > 100 {
            break;
        }
        name.push(c);
    }
    name = name.trim().trim_end_matches('.').to_string();
    format!("{:0digits$} {name}.eml", i + 1)
}

fn not_archive(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{} is not an archive Coxswain reads", path.display()),
    )
}

/// A tar, its compression undone as it is read.
fn tar_reader(path: &Path, k: Kind) -> io::Result<tar::Archive<Box<dyn Read>>> {
    tar_reader_noting(path, k, None)
}

/// A tar, its compression undone as it is read, the places a gzip or zstd can later be
/// unpacked from noted in `note`.
fn tar_reader_noting(
    path: &Path,
    k: Kind,
    note: Option<crate::tar_index::Points>,
) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let Kind::Tar(pack) = k else {
        return Err(not_archive(path));
    };
    // Stop is looked at on every read, so unpacking a large file stops inside it.
    let f = BufReader::new(crate::stop::Reader(File::open(path)?));
    let r: Box<dyn Read> = match pack {
        Pack::None => Box::new(f),
        Pack::Gz => Box::new(crate::tar_index::Gz::new(f, note)),
        Pack::Bz2 if note.is_some() => Box::new(crate::tar_index::Bz::new(f, note)),
        Pack::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(f)),
        Pack::Xz => Box::new(lzma_rust2::XzReader::new(f, true)),
        Pack::Zst => Box::new(crate::tar_index::Zst::new(f, note)),
    };
    Ok(tar::Archive::new(r))
}

/// What is unpacked at most after a tar's end, to reach the end of its bzip2 stream.
const TAR_TRAIL: u64 = 16 << 20;

/// The most bytes of a tar's long name or PAX record: the tar crate reads those whole into
/// memory, however large a crafted header says they are.
pub(crate) const TAR_META: u64 = 64 * 1024;

/// Each entry of `entries` with its whole name and link target (GNU long names and PAX paths,
/// read here with `TAR_META` at most), until `f` says enough. Those records are not entries.
pub(crate) fn tar_named<'a, R: Read>(
    entries: tar::Entries<'a, R>,
    mut f: impl FnMut(String, Option<String>, &mut tar::Entry<'a, R>) -> io::Result<bool>,
) -> io::Result<()> {
    tar_named_at(entries, |name, link, e, _| f(name, link, e))
}

/// `tar_named`, with where each entry starts in the unpacked tar: its first header, the long
/// name or PAX record before it if it has one (where reading it again can start).
fn tar_named_at<'a, R: Read>(
    entries: tar::Entries<'a, R>,
    mut f: impl FnMut(String, Option<String>, &mut tar::Entry<'a, R>, u64) -> io::Result<bool>,
) -> io::Result<()> {
    let mut start = None;
    let text = |b: &[u8]| String::from_utf8_lossy(b.strip_suffix(&[0]).unwrap_or(b)).into_owned();
    let (mut long, mut link): (Option<String>, Option<String>) = (None, None);
    for e in entries.raw(true) {
        let mut e = e?;
        let at = *start.get_or_insert(e.raw_header_position());
        let t = e.header().entry_type();
        if t.is_gnu_longname()
            || t.is_gnu_longlink()
            || t.is_pax_local_extensions()
            || t.is_pax_global_extensions()
        {
            if e.size() > TAR_META {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a name in this tar is too long",
                ));
            }
            let mut buf = vec![];
            e.read_to_end(&mut buf)?;
            if t.is_gnu_longname() {
                long = Some(text(&buf));
            } else if t.is_gnu_longlink() {
                link = Some(text(&buf));
            } else if t.is_pax_local_extensions() {
                for x in tar::PaxExtensions::new(&buf).filter_map(Result::ok) {
                    match x.key_bytes() {
                        b"path" => long = Some(text(x.value_bytes())),
                        b"linkpath" => link = Some(text(x.value_bytes())),
                        _ => {}
                    }
                }
            }
            continue;
        }
        let name = match long.take() {
            Some(n) => n,
            None => e.path()?.to_string_lossy().into_owned(),
        };
        let link = match link.take() {
            Some(l) => Some(l),
            None => e.link_name()?.map(|l| l.to_string_lossy().into_owned()),
        };
        start = None;
        if !f(
            name.trim_start_matches("./")
                .trim_end_matches('/')
                .to_string(),
            link,
            &mut e,
            at,
        )? {
            break;
        }
    }
    Ok(())
}

/// A new file at `path` for an archive being written, never one that is there: a link put
/// in its place (by someone else, in a shared folder) is removed, not written through.
/// `private`: for this user alone while it is written (an archive that is there gets its own
/// permissions back when it is whole).
fn create_fresh(path: &Path, private: bool) -> io::Result<File> {
    let _ = std::fs::remove_file(path);
    let mut new = std::fs::OpenOptions::new();
    new.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut new, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    new.open(path)
}

/// A tar written plain at `tar`, compressed into `to` as `pack` says.
fn compress(tar: &Path, to: &Path, pack: Pack, private: bool) -> io::Result<()> {
    let (mut from, mut out) = (
        BufReader::new(crate::stop::Reader(File::open(tar)?)),
        create_fresh(to, private)?,
    );
    match pack {
        Pack::None => io::copy(&mut from, &mut out).map(drop),
        Pack::Gz => {
            let mut w = flate2::write::GzEncoder::new(out, flate2::Compression::default());
            io::copy(&mut from, &mut w)?;
            w.finish().map(drop)
        }
        Pack::Bz2 => {
            let mut w = bzip2::write::BzEncoder::new(out, bzip2::Compression::default());
            io::copy(&mut from, &mut w)?;
            w.finish().map(drop)
        }
        Pack::Xz => {
            let mut w = lzma_rust2::XzWriter::new(out, lzma_rust2::XzOptions::with_preset(6))
                .map_err(io::Error::other)?;
            io::copy(&mut from, &mut w)?;
            w.finish().map(drop).map_err(io::Error::other)
        }
        Pack::Zst => {
            let (mut r, mut w) = (Keep::new(from), Keep::new(io::BufWriter::new(out)));
            zstd_into(&mut r, &mut w)?;
            w.inner.flush()
        }
    }
}

/// zstd-compress `r` into `w`. ruzstd unwraps its reads and writes, so both go through
/// `Keep`, which stores the first error and lets the compressor run out; that error, or a
/// panic of the compressor's own, comes back here as an `Err`.
fn zstd_into<R: Read, W: Write>(r: &mut Keep<R>, w: &mut Keep<W>) -> io::Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ruzstd::encoding::compress(
            &mut *r,
            &mut *w,
            ruzstd::encoding::CompressionLevel::Fastest,
        )
    }))
    .map_err(|_| io::Error::other("zstd compression failed"))?;
    match r.err.take().or(w.err.take()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// A reader or writer that never fails: the first error is kept in `err`, and from then on
/// reads give the end and writes take everything.
struct Keep<T> {
    inner: T,
    err: Option<io::Error>,
}

impl<T> Keep<T> {
    fn new(inner: T) -> Self {
        Keep { inner, err: None }
    }
}

impl<T: Read> Read for Keep<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.err.is_none() {
            match self.inner.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => self.err = Some(e),
                r => return r,
            }
        }
        Ok(0)
    }
}

impl<T: Write> Write for Keep<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        while self.err.is_none() {
            match self.inner.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => self.err = Some(e),
                r => return r,
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.err.is_none()
            && let Err(e) = self.inner.flush()
        {
            self.err = Some(e);
        }
        Ok(())
    }
}

/// Passwords given for locked archives while the app runs: in memory only, never written, and
/// gone when the app ends. A 7z whose names are locked needs one even to be looked into.
static PASSWORDS: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

/// Remember `password` for `archive` until the app ends (or `forget`).
pub fn remember(archive: &Path, password: &str) {
    let mut all = PASSWORDS.lock().unwrap();
    all.retain(|(a, _)| a != archive);
    all.push((archive.to_path_buf(), password.to_string()));
    drop(all);
    uncache(archive);
}

/// Forget the password given for `archive`.
pub fn forget(archive: &Path) {
    PASSWORDS.lock().unwrap().retain(|(a, _)| a != archive);
    uncache(archive);
}

/// The password to use: the one that opened `archive` before (kept only once it did), else
/// the one given, which may be another archive's when a copy goes from one into another.
fn password_for(archive: &Path, given: Option<&str>) -> Option<String> {
    PASSWORDS
        .lock()
        .unwrap()
        .iter()
        .find(|(a, _)| a == archive)
        .map(|(_, p)| p.clone())
        .or_else(|| given.map(String::from))
}

/// The entries of the archives last looked into, newest last, with their size and modified
/// time: browsing an archive lists it once per folder, and a compressed tar is read in full
/// each time. A few, so the preview of another archive does not push out the one browsed.
static LAST: std::sync::Mutex<Vec<Looked>> = std::sync::Mutex::new(Vec::new());
type Looked = (PathBuf, Stamp, Arc<[Item]>);
const LAST_KEPT: usize = 4;

type Stamp = (u64, Option<std::time::SystemTime>);

/// A file's size and time; for an archive split into parts, of all its parts together, so a
/// part changed, added or taken away is noticed.
fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = std::fs::metadata(path)?;
    let (mut len, mut time) = (m.len(), m.modified().ok());
    let name = path.to_string_lossy();
    let mut others = zip_parts(&name, |p| Path::new(p).is_file());
    if in_parts(path) {
        others.extend((2..=999).map(|n| part_name(&name, n)).take_while(|p| Path::new(p).is_file()));
    }
    for p in others {
        let m = std::fs::metadata(p)?;
        len = len.wrapping_add(m.len()).wrapping_add(1);
        time = time.max(m.modified().ok());
    }
    Ok((len, time))
}

fn uncache(archive: &Path) {
    LAST.lock().unwrap().retain(|(p, ..)| p != archive);
}

/// Whether a 7z's contents are locked: any of its blocks is AES-encrypted.
fn seven_locked(a: &sevenz_rust2::Archive) -> bool {
    a.blocks.iter().any(|b| {
        b.coders
            .iter()
            .any(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256)
    })
}

/// A 7z entry being read out, noting whether the reading failed: with no password, or a wrong
/// one, that is how a locked entry fails, as an error in what it reads rather than a word.
/// An entry that ends before its `left` bytes failed too: sevenz checks the checksum only at
/// the end, and a wrong key can make the stream end at once (LZMA2's end byte, about 1 in 256).
struct Noting<'a> {
    from: &'a mut dyn Read,
    left: u64,
    failed: bool,
}

impl Read for Noting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let r = match self.from.read(buf) {
            Ok(0) if !buf.is_empty() && self.left > 0 => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the entry ends early",
            )),
            r => r,
        };
        match &r {
            Ok(n) => self.left = self.left.saturating_sub(*n as u64),
            Err(_) => self.failed = true,
        }
        r
    }
}

/// The error of copying an entry: `LOCKED` when a locked entry could not be read.
fn copy_error(err: io::Error, noting: &Noting, locked: bool) -> io::Error {
    if locked && noting.failed {
        self::locked()
    } else {
        err
    }
}

/// Why the 7z split into parts whose first part is `path` does not open: a part is missing
/// (`archive.part_missing`, naming it). `None` for any other file, and one with all its parts.
pub fn missing_part(path: &Path) -> Option<String> {
    if !in_parts(path) {
        return None;
    }
    part_missing(&mut Joined::open(path).ok()?, path).ok().flatten()
}

/// `missing_part` on the parts opened: read from their start, left there.
fn part_missing(file: &mut Joined, path: &Path) -> io::Result<Option<String>> {
    if !in_parts(path) {
        return Ok(None);
    }
    // Parts that stop short of its list of contents: one is missing.
    let mut start = [0u8; 32];
    let mut why = None;
    if file.read(&mut start)? == 32 && start[..6] == *b"7z\xBC\xAF\x27\x1C" {
        let at = u64::from_le_bytes(start[12..20].try_into().unwrap());
        let len = u64::from_le_bytes(start[20..28].try_into().unwrap());
        if 32u64.saturating_add(at).saturating_add(len) > file.len {
            let first = path.file_name().unwrap_or_default().to_string_lossy();
            let missing = part_name(&first, file.parts.len() as u32 + 1);
            why = Some(crate::t!("archive.part_missing", "name" => missing));
        }
    }
    file.seek(io::SeekFrom::Start(0))?;
    Ok(why)
}

/// A 7z, opened with `password` (none: an empty one).
fn seven(path: &Path, password: Option<&str>) -> io::Result<sevenz_rust2::ArchiveReader<Joined>> {
    let pw = password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::from);
    let mut file = Joined::open(path)?;
    if let Some(why) = part_missing(&mut file, path)? {
        return Err(io::Error::new(io::ErrorKind::NotFound, why));
    }
    let size = seven_header_size(&mut file)?;
    if size > SEVEN_HEADER_MAX {
        return Err(io::Error::other(
            crate::t!("archive.header_too_big", "mb" => size >> 20, "max" => SEVEN_HEADER_MAX >> 20),
        ));
    }
    file.seek(io::SeekFrom::Start(0))?;
    let mut r = sevenz_rust2::ArchiveReader::new(file, pw).map_err(seven_error)?;
    // One thread: with more, the crate unpacks each LZMA2 run between two resets whole into
    // memory, and a few hundred kilobytes of one can unpack to gigabytes.
    r.set_thread_count(1);
    Ok(r)
}

/// The most a 7z's packed list of contents may unpack to: it is unpacked whole, into memory,
/// before anything is read, so a few bytes that unpack to gigabytes would take as much.
/// The crate also makes room for every file the list names before reading them, over a
/// hundred bytes each, however few bytes the list gives each. 64 MB lets a real one name about
/// 900,000 files (a collection of Go games in one 7z names 891,188 in 61 MB, and takes about
/// 600 MB of memory to list).
const SEVEN_HEADER_MAX: u64 = 64 << 20;

/// What a 7z's packed list of contents (its encoded header) says it unpacks to, the largest of
/// its streams; 0 when it is not packed. Read from the archive's own bytes, before opening it.
fn seven_header_size(file: &mut (impl Read + Seek)) -> io::Result<u64> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "a broken 7z header");
    let mut start = [0u8; 32];
    if file.read(&mut start)? < 32 || start[..6] != *b"7z\xBC\xAF\x27\x1C" {
        return Ok(0);
    }
    let at = u64::from_le_bytes(start[12..20].try_into().unwrap());
    let len = u64::from_le_bytes(start[20..28].try_into().unwrap());
    file.seek(io::SeekFrom::Start(32u64.saturating_add(at)))?;
    // The streams of a packed header take a few dozen bytes.
    let mut head = vec![];
    file.take(len.min(1 << 16)).read_to_end(&mut head)?;
    let r = &mut head.as_slice();
    fn byte(r: &mut &[u8]) -> io::Result<u8> {
        let (&b, rest) = r.split_first().ok_or(io::ErrorKind::UnexpectedEof)?;
        *r = rest;
        Ok(b)
    }
    fn skip(r: &mut &[u8], n: u64) -> io::Result<()> {
        *r = r
            .get(usize::try_from(n).map_err(|_| io::ErrorKind::UnexpectedEof)?..)
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        Ok(())
    }
    // 7-Zip's numbers: the first byte's leading ones say how many bytes follow.
    fn number(r: &mut &[u8]) -> io::Result<u64> {
        let first = byte(r)? as u64;
        let mut value = 0;
        for i in 0..8 {
            let mask = 0x80 >> i;
            if first & mask == 0 {
                return Ok(value | (first & (mask - 1)) << (8 * i));
            }
            value |= (byte(r)? as u64) << (8 * i);
        }
        Ok(value)
    }
    // Every count below is of things that take a byte or more, so a false one ends at the
    // header's end.
    if byte(r)? != 0x17 {
        return Ok(0);
    }
    let mut id = byte(r)?;
    if id == 0x06 {
        // Where the packed streams are, their sizes and checksums.
        number(r)?;
        let n = number(r)?;
        loop {
            match byte(r)? {
                0x00 => break,
                0x09 => (0..n).try_for_each(|_| number(r).map(drop))?,
                0x0A => {
                    let defined = if byte(r)? != 0 {
                        n
                    } else {
                        let bits = &r[..r.len().min(n.div_ceil(8) as usize)];
                        skip(r, n.div_ceil(8))?;
                        bits.iter().map(|b| b.count_ones() as u64).sum()
                    };
                    skip(r, defined.saturating_mul(4))?;
                }
                _ => return Err(bad()),
            }
        }
        id = byte(r)?;
    }
    // The blocks, the coders of each and how they are chained, then the size of each stream.
    if id != 0x07 || byte(r)? != 0x0B {
        return Err(bad());
    }
    let blocks = number(r)?;
    if byte(r)? != 0 {
        return Err(bad());
    }
    let mut outs = 0u64;
    for _ in 0..blocks {
        let (mut ins, mut out) = (0u64, 0u64);
        for _ in 0..number(r)? {
            let bits = byte(r)?;
            skip(r, (bits & 0xF) as u64)?;
            let (i, o) = if bits & 0x10 != 0 {
                (number(r)?, number(r)?)
            } else {
                (1, 1)
            };
            (ins, out) = (ins.saturating_add(i), out.saturating_add(o));
            if bits & 0x20 != 0 {
                let props = number(r)?;
                skip(r, props)?;
            }
        }
        let binds = out.saturating_sub(1);
        (0..binds.saturating_mul(2)).try_for_each(|_| number(r).map(drop))?;
        let packed = ins.saturating_sub(binds);
        if packed > 1 {
            (0..packed).try_for_each(|_| number(r).map(drop))?;
        }
        outs = outs.saturating_add(out);
    }
    if byte(r)? != 0x0C {
        return Err(bad());
    }
    (0..outs).try_fold(0, |most, _| Ok(number(r)?.max(most)))
}

/// A 7z's error, a missing or wrong password said as `LOCKED`.
fn seven_error(e: sevenz_rust2::Error) -> io::Error {
    match e {
        sevenz_rust2::Error::PasswordRequired | sevenz_rust2::Error::MaybeBadPassword(_) => {
            locked()
        }
        e => io::Error::other(e),
    }
}

/// The first `max` entries, and whether there were more.
pub fn list(path: &Path, max: usize) -> io::Result<(Vec<ArchiveEntry>, bool)> {
    // Not looked into yet: only as far as is shown (a big compressed tar is not unpacked to
    // its end for a preview's first rows).
    // Read to its end within that, it is kept as `items` keeps it.
    let st = stamp(path)?;
    // A disk image is listed once, whole, by 7-Zip, and kept: the preview, Enter and F5 use it.
    // So is a container image, its layers merged.
    let disk = matches!(kind(path), Some(Kind::Disk | Kind::Image));
    let (all, more) = match cached(path, st) {
        _ if disk => {
            let all = items(path)?;
            let more = all.len() > max || crate::disk_image::was_cut(path);
            (all, more)
        }
        Some(all) => {
            let more = all.len() > max;
            (all, more)
        }
        None => {
            let mut raw = read_raw_items(
                path,
                password_for(path, None).as_deref(),
                max.saturating_add(1),
            )?;
            let more = raw.len() > max;
            raw.retain(|it| sound(&it.name));
            let all: Arc<[Item]> = raw.into();
            if !more {
                keep_items(path, st, all.clone());
            }
            (all, more)
        }
    };
    Ok((
        all.iter()
            .take(max)
            .map(|it| ArchiveEntry {
                name: it.name.clone(),
                size: it.size,
                is_dir: it.dir,
                modified: it.modified,
            })
            .collect(),
        more,
    ))
}

/// Extract into a new folder named after the archive inside `dest_dir`. Returns that folder.
pub fn extract(path: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    extract_locked(path, dest_dir, None)
}

/// The folder `extract` makes for `path` in `dest_dir`: the archive's name without its ending.
pub(crate) fn extract_target(path: &Path, dest_dir: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_lowercase();
    let stem_len = ENDINGS
        .iter()
        .map(|(e, _)| *e)
        .find(|e| lower.ends_with(e))
        .map_or_else(
            || name.rfind('.').unwrap_or(name.len()),
            |e| name.len() - e.len(),
        );
    // An OCI layout's `index.json` comes out under its folder's name.
    match path.parent().and_then(Path::file_name) {
        Some(d) if crate::container_image::layout_index(path) => dest_dir.join(d),
        _ => dest_dir.join(&name[..stem_len]),
    }
}

/// `extract`, with the password of a locked archive. Stopped half way, the folder keeps what
/// came out so far; the file it was on is removed.
pub fn extract_locked(path: &Path, dest_dir: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    let k = kind(path).ok_or_else(|| not_archive(path))?;
    let to = extract_target(path, dest_dir);
    std::fs::create_dir_all(dest_dir)?;
    // Made here, never one that is there (a link to elsewhere included).
    match std::fs::create_dir(&to) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists", to.display()),
            ));
        }
        r => r?,
    }
    // A 7z or a zip is read once (a locked one with the password): no links made, nothing
    // sized by what the archive claims. A tar's files and folders as the crate unpacks them,
    // which keeps them inside; its names are read first (from the last look, when it was
    // browsed), since the crate would read a crafted long-name record whole, however large:
    // `TAR_META` at most.
    let r = if matches!(k, Kind::SevenZ | Kind::Zip | Kind::Disk | Kind::Mbox | Kind::Image) {
        copy_out_with(
            path,
            "",
            &to,
            password_for(path, password).as_deref(),
            Copy::default(),
        )
        .map(drop)
    } else {
        items(path).and_then(|_| unpack_tar(tar_reader(path, k)?, &to))
    };
    // A mailbox some messages of which could not come out keeps the others; what came out
    // before a Stop stays.
    if r.is_err()
        && !crate::stop::asked()
        && !(k == Kind::Mbox && std::fs::read_dir(&to).is_ok_and(|mut d| d.next().is_some()))
    {
        let _ = std::fs::remove_dir_all(&to);
    }
    r.map(|_| to)
}

/// The permission bits an archive cannot give what comes out of it: write for the group and
/// for everyone else (a script anyone may change, a folder anyone may put files in), as tar
/// does for a user who is not root.
const NOT_SHARED: u32 = 0o022;

/// A tar's files, hard links and folders into `to`, as `tar::Archive::unpack` does, its
/// folders last so a read-only one does not keep out what is in it. Symbolic links (which may
/// point anywhere, and need a right of their own on Windows) and devices are left out.
fn unpack_tar(mut tar: tar::Archive<Box<dyn Read>>, to: &Path) -> io::Result<()> {
    tar.set_mask(NOT_SHARED);
    // Long paths on Windows: `\\?\` in front, as the crate does.
    let to = &to.canonicalize().unwrap_or_else(|_| to.to_path_buf());
    let mut dirs = vec![];
    for e in tar.entries()? {
        let mut e = e?;
        let t = e.header().entry_type();
        if cfg!(windows) && e.path().is_ok_and(|p| p.iter().any(|c| device(&c.to_string_lossy()))) {
            continue;
        }
        if t.is_dir() {
            dirs.push(e);
        } else if (t.is_file() || t.is_gnu_sparse() || t.is_hard_link())
            && let Err(err) = e.unpack_in(to)
        {
            // Stopped inside it: the file half written goes (only a plain name below `to`,
            // which was made empty for this).
            if crate::stop::asked()
                && let Ok(p) = e.path()
                && p.components().all(|c| matches!(c, std::path::Component::Normal(_)))
                && std::fs::symlink_metadata(to.join(&p)).is_ok_and(|m| m.is_file())
            {
                let _ = std::fs::remove_file(to.join(&p));
            }
            return Err(err);
        }
    }
    dirs.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut d in dirs {
        d.unpack_in(to)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- inside an archive

/// What an error says when a file inside an archive is locked and no password, or a wrong one,
/// was given: the apps ask for one then.
pub const LOCKED: &str = "locked: a password is needed";

fn locked() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, LOCKED)
}

/// Where a path goes through an archive: the archive file, and the path inside it
/// (`/`-separated, "" for its top). `None` for a path that is not inside one.
pub fn split(path: &Path) -> Option<(PathBuf, String)> {
    let mut inner: Vec<String> = vec![];
    for a in path.ancestors() {
        if a.is_file() {
            return opens(a).then(|| {
                inner.reverse();
                (a.to_path_buf(), inner.join("/"))
            });
        }
        if a.is_dir() {
            return None;
        }
        inner.push(a.file_name()?.to_string_lossy().into_owned());
    }
    None
}

// ---------------------------------------------------------------- archives inside archives

/// The most archives inside each other that are opened, the outer one not counted.
const NESTED_DEPTH: usize = 8;
/// The most bytes one archive inside another is unpacked to without being copied out by hand:
/// what it claims first, then counted as it comes out (a claim may be a lie). Larger ones say
/// to copy them out with F5, a choice the user makes knowing the size.
const NESTED_MAX: u64 = if cfg!(test) { 1 << 20 } else { 512 << 20 };
/// The most bytes all the copies kept take together; the oldest go first to make room.
const NESTED_ALL: u64 = if cfg!(test) { 3 << 20 } else { 2 << 30 };
/// Archives inside archives kept unpacked in the cache this run; the oldest goes past this.
const NESTED_KEPT: usize = 8;

/// What a file is, for the copies of archives inside it: its size, modified time, and on Unix
/// its device and inode, so another file at the same path is never taken for it.
type Identity = (Stamp, u64, u64);

fn identity(path: &Path) -> io::Result<Identity> {
    let m = std::fs::metadata(path)?;
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        (m.dev(), m.ino())
    };
    #[cfg(not(unix))]
    let id = (0, 0);
    Ok(((m.len(), m.modified().ok()), id.0, id.1))
}

/// A copy of an archive inside another: the archive it is in, as it was then, its name in
/// it, the copy, and its size.
struct Spilled {
    outer: PathBuf,
    id: Identity,
    name: String,
    copy: PathBuf,
    size: u64,
}

/// The copies made this run, oldest first.
static NESTED: std::sync::Mutex<Vec<Spilled>> = std::sync::Mutex::new(Vec::new());
/// The copies being made now, by archive and name: a second look waits for the first.
static MAKING: std::sync::Mutex<Vec<(PathBuf, String, Arc<std::sync::Mutex<()>>)>> =
    std::sync::Mutex::new(Vec::new());
/// The copy the last preview of an archive inside an archive returned (`peek`), held until
/// the next: the preview opens it after `peek` returns, and copies made meanwhile must not
/// take it away.
static PEEKED: std::sync::Mutex<Option<Held>> = std::sync::Mutex::new(None);
/// The copies an operation goes through now (`Held`): never evicted while it runs.
static HELD: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Copies of archives inside archives held for as long as this lives: those `resolve` went
/// through, so a copy is never taken away while it is read, however many come meanwhile.
#[derive(Default)]
pub(crate) struct Held(Vec<PathBuf>);

impl Drop for Held {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        for p in &self.0 {
            if let Some(i) = held.iter().position(|h| h == p) {
                held.swap_remove(i);
            }
        }
    }
}

/// The number of the next copy's folder: never one used before in this run.
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where the first archive inside `archive` on the way to `inner` ends in `inner`: the length
/// of its name. `into`: `inner` is looked into, so an archive at its very end counts too;
/// otherwise `inner` is a file to act on, and an archive at its end is only that file.
fn nested_cut(archive: &Path, inner: &str, into: bool) -> io::Result<Option<usize>> {
    let ends: Vec<usize> = inner
        .match_indices('/')
        .map(|(i, _)| i)
        .chain(into.then_some(inner.len()))
        .filter(|&i| i > 0 && opens_inside(Path::new(&inner[..i])))
        .collect();
    if ends.is_empty() {
        return Ok(None);
    }
    // Only now the listing: a path through no archive by name costs nothing.
    let all = items(archive)?;
    Ok(ends
        .into_iter()
        .find(|&i| all.iter().any(|it| !it.dir && it.name == inner[..i])))
}

/// The archive file `inner` of `archive` is in, and its path inside that one: `archive` itself,
/// or the copy in the cache of the archive inside it that `inner` goes through (`nested_cut`),
/// as deep as they go. Each such archive is unpacked to the cache once, never into memory. The
/// copies gone through are held until the `Held` is dropped.
fn resolve(archive: &Path, inner: &str, into: bool) -> io::Result<(PathBuf, String, Held)> {
    let (mut at, mut rest, mut held) = (archive.to_path_buf(), inner.to_string(), Held::default());
    for depth in 0.. {
        let Some(cut) = nested_cut(&at, &rest, into)? else {
            break;
        };
        if depth == NESTED_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::t!("archive.nested_too_deep", "depth" => NESTED_DEPTH),
            ));
        }
        let mut copy = spilled(&at, &rest[..cut])?;
        at = copy.0[0].clone();
        held.0.append(&mut copy.0);
        rest = rest[cut..].trim_start_matches('/').to_string();
    }
    Ok((at, rest, held))
}

/// Whether the file `path` inside an archive opens like a folder too: by its name alone, an
/// archive, a comic or a mailbox, and a disk image by its sure name while those open (looked
/// into from a copy, as an archive inside one is).
pub fn opens_inside(path: &Path) -> bool {
    ending_kind(path).or_else(|| ends_in(path, OPEN_ONLY)).is_some()
        || (crate::disk_image::open_on() && crate::disk_image::sure(path))
}

/// Whether `inner` of `archive` lies inside an archive inside it, where nothing is changed.
/// `into` as for `nested_cut`.
pub fn nested(archive: &Path, inner: &str, into: bool) -> bool {
    nested_cut(archive, inner, into).is_ok_and(|c| c.is_some())
}

/// Whether the folder `dir` (a path through an archive) is inside an archive inside another.
pub fn nested_dir(dir: &Path) -> bool {
    split(dir).is_some_and(|(a, inner)| nested(&a, &inner, true))
}

fn nested_read_only() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        crate::t!("archive.nested_read_only"),
    )
}

/// The copy in the cache of `name`, an archive inside `archive`: made when there is none of
/// it as `archive` is now. In this run's own folder, readable by this user alone, in a fresh
/// folder of its own (never a number used before in this run) under the name it has in the
/// archive (`sound` names only: no `..`, no separators). At most `NESTED_MAX` bytes a copy and
/// `NESTED_ALL` for all, `NESTED_KEPT` copies; the oldest not held go first, and all when the
/// app ends. The copy is held (`Held`, taken while the list is locked) until that is dropped.
fn spilled(archive: &Path, name: &str) -> io::Result<Held> {
    let id = identity(archive)?;
    let find = |kept: &mut Vec<Spilled>| -> Option<Held> {
        let i = kept.iter().position(|k| {
            k.outer == archive && k.id == id && k.name == name && k.copy.is_file()
        })?;
        let it = kept.remove(i);
        let copy = it.copy.clone();
        kept.push(it);
        HELD.lock().unwrap_or_else(|e| e.into_inner()).push(copy.clone());
        Some(Held(vec![copy]))
    };
    if let Some(copy) = find(&mut NESTED.lock().unwrap_or_else(|e| e.into_inner())) {
        return Ok(copy);
    }
    // One copy of the same archive at a time; others are made meanwhile.
    let key = {
        let mut making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
        match making.iter().find(|(a, n, _)| a == archive && n == name) {
            Some((.., k)) => k.clone(),
            None => {
                let k = Arc::new(std::sync::Mutex::new(()));
                making.push((archive.to_path_buf(), name.to_string(), k.clone()));
                k
            }
        }
    };
    let made = {
        let _one = key.lock().unwrap_or_else(|e| e.into_inner());
        // The guard goes before `spill`, which takes the lock again.
        let found = find(&mut NESTED.lock().unwrap_or_else(|e| e.into_inner()));
        match found {
            Some(copy) => Ok(copy),
            None => spill(archive, name, id),
        }
    };
    MAKING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(a, n, _)| !(a == archive && n == name));
    made
}

/// `spilled`'s copy, made: what the archive claims is looked at first.
fn spill(archive: &Path, name: &str, id: Identity) -> io::Result<Held> {
    let all = items(archive)?;
    let size_of = |n: &str| all.iter().find(|it| it.name == n && !it.dir).map(|it| it.size);
    // A 7z split into parts is copied with all its parts there, next to each other.
    let mut names = vec![name.to_string()];
    if in_parts(Path::new(name)) {
        names.extend((2..=999).map(|n| part_name(name, n)).take_while(|n| size_of(n).is_some()));
    }
    // A zip split into parts: its earlier parts (`x.z01` …) beside it too.
    names.extend(zip_parts(name, |n| size_of(n).is_some()));
    // A zip or 7z (all its parts) stored as it is in a zip or a plain tar: read in place,
    // however large, never copied.
    let places = if matches!(ending_kind(Path::new(name)), Some(Kind::Zip | Kind::SevenZ)) {
        let mut all = vec![];
        for n in &names {
            all.push(in_place(archive, n)?);
        }
        all.into_iter().collect::<Option<Vec<_>>>()
    } else {
        None
    };
    if let Some(places) = places {
        let base = run_folder("nested", false)?;
        let dir = base.join(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string());
        let _ = std::fs::remove_dir_all(&dir);
        crate::fs::private_dir(&dir)?;
        let marker_of = |n: &str| dir.join(n.rsplit('/').next().unwrap_or("archive"));
        let marker = marker_of(name);
        for (n, (file, start, len)) in names.iter().zip(places) {
            let m = marker_of(n);
            File::create_new(&m)?;
            WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).push(Window { marker: m, file, start, len });
        }
        let mut kept = NESTED.lock().unwrap_or_else(|e| e.into_inner());
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        // The oldest not in use go; while all are, there are more for a while.
        while kept.len() >= NESTED_KEPT {
            let Some(i) = kept.iter().position(|k| !held.contains(&k.copy)) else { break };
            evict(&mut kept, i);
        }
        kept.push(Spilled { outer: archive.to_path_buf(), id, name: name.to_string(), copy: marker.clone(), size: 0 });
        held.push(marker.clone());
        return Ok(Held(vec![marker]));
    }
    let claimed: u64 = names.iter().map(|n| size_of(n).unwrap_or(0)).sum();
    if claimed > NESTED_MAX {
        return Err(too_large(name));
    }
    let base = run_folder("nested", false)?;
    let dir = base.join(
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string(),
    );
    let _ = std::fs::remove_dir_all(&dir);
    crate::fs::private_dir(&dir)?;
    let base_name = |n: &str| dir.join(n.rsplit('/').next().unwrap_or("archive"));
    let copy = base_name(name);
    let mut left = NESTED_MAX;
    let made = names.iter().try_fold(0, |size, n| {
        copy_out_with(
            archive,
            n,
            &base_name(n),
            password_for(archive, None).as_deref(),
            Copy {
                limit: left,
                ..Copy::default()
            },
        )?;
        let m = std::fs::metadata(base_name(n))?.len();
        left = left.saturating_sub(m);
        Ok::<_, io::Error>(size + m)
    });
    let size = match made {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let mut kept = NESTED.lock().unwrap_or_else(|e| e.into_inner());
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    // The oldest not in use go; while all are, there are more for a while.
    while kept.len() >= NESTED_KEPT || kept.iter().map(|k| k.size).sum::<u64>() + size > NESTED_ALL {
        let Some(i) = kept.iter().position(|k| !held.contains(&k.copy)) else { break };
        evict(&mut kept, i);
    }
    kept.push(Spilled {
        outer: archive.to_path_buf(),
        id,
        name: name.to_string(),
        copy: copy.clone(),
        size,
    });
    held.push(copy.clone());
    Ok(Held(vec![copy]))
}

/// Where the file `name` inside `archive` is stored as it is: the file its bytes are in, and
/// their start and length there (through `archive`'s own range when it is read in place too).
/// `None` when it is packed or locked, or `archive` is no zip or plain tar.
fn in_place(archive: &Path, name: &str) -> io::Result<Option<(PathBuf, u64, u64)>> {
    let range = match kind(archive) {
        Some(Kind::Zip) => {
            let mut f = BufReader::new(open_raw(archive)?);
            match crate::zip_dir::find(&mut f, name) {
                Ok(Some(c)) => c.stored(&mut f)?,
                _ => None,
            }
        }
        Some(Kind::Tar(Pack::None)) if window_of(archive).is_none() => {
            let mut found = None;
            tar_named(
                tar::Archive::new(BufReader::new(File::open(archive)?)).entries_with_seek()?,
                |n, _, e| {
                    if n == name && e.header().entry_type().is_file() {
                        found = Some((e.raw_file_position(), e.size()));
                        return Ok(false);
                    }
                    Ok(true)
                },
            )?;
            found
        }
        _ => None,
    };
    Ok(range.map(|(start, len)| match window_of(archive) {
        Some((file, outer, _)) => (file, outer + start, len),
        None => (archive.to_path_buf(), start, len),
    }))
}

/// Takes the copy `kept[i]` away, and every copy made out of it, as deep as they go.
fn evict(kept: &mut Vec<Spilled>, i: usize) {
    let gone = kept.remove(i);
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).retain(|w| w.marker.parent() != gone.copy.parent());
    uncache(&gone.copy);
    forget(&gone.copy);
    if let Some(d) = gone.copy.parent() {
        let _ = std::fs::remove_dir_all(d);
    }
    while let Some(j) = kept.iter().position(|k| k.outer == gone.copy) {
        evict(kept, j);
    }
}

/// Removes this run's copies of archives inside archives: the apps call it as they end.
/// Copies a run left behind (it crashed) go after a day.
pub fn clean_up() {
    PEEKED.lock().unwrap_or_else(|e| e.into_inner()).take();
    let mut kept = NESTED.lock().unwrap_or_else(|e| e.into_inner());
    kept.clear();
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    if let Some(d) = crate::helper::folder() {
        let me = crate::spill::run();
        let _ = std::fs::remove_dir_all(d.join("nested").join(me));
        // ChessBase archives this run unpacked (up to 16 GB each): no other run reads them.
        let _ = std::fs::remove_dir_all(d.join("cbv").join(me));
    }
}

/// Seconds since the Unix epoch of a 7z entry's modified time (0: none). Counted here: a time
/// past what the system's clock holds must not panic.
fn seven_secs(e: &sevenz_rust2::ArchiveEntry) -> u64 {
    (u64::from(e.last_modified_date) / 10_000_000).saturating_sub(11_644_473_600)
}

/// Seconds since the Unix epoch of a zip's date and time, which is local time without a zone
/// (as zip tools write it); a time that does not exist (a clock change) takes the one after.
fn unix(t: zip::DateTime) -> u64 {
    use chrono::TimeZone;
    let local = chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32)
        .and_then(|d| d.and_hms_opt(t.hour() as u32, t.minute() as u32, t.second() as u32));
    local
        .and_then(|l| chrono::Local.from_local_datetime(&l).earliest())
        .map_or(0, |d| d.timestamp().max(0) as u64)
}

/// An entry of an archive: its path inside, size, whether a folder, modified (seconds), locked.
#[derive(Clone)]
struct Item {
    name: String,
    size: u64,
    dir: bool,
    modified: u64,
    locked: bool,
}

/// The archive's entries, from the last look at it when it has not changed since.
fn items(archive: &Path) -> io::Result<Arc<[Item]>> {
    let stamp = stamp(archive)?;
    if let Some(items) = cached(archive, stamp) {
        return Ok(items);
    }
    let out: Arc<[Item]> = read_items(archive)?.into();
    keep_items(archive, stamp, out.clone());
    Ok(out)
}

/// The entries of `archive` from the last look, if it has not changed since.
fn cached(archive: &Path, stamp: Stamp) -> Option<Arc<[Item]>> {
    LAST.lock()
        .unwrap()
        .iter()
        .find(|(p, s, _)| p == archive && *s == stamp)
        .map(|(.., items)| items.clone())
}

/// Keep all the entries of `archive`, read when it was as `stamp` says, for the next look.
fn keep_items(archive: &Path, stamp: Stamp, items: Arc<[Item]>) {
    let mut last = LAST.lock().unwrap();
    last.retain(|(p, ..)| p != archive);
    if last.len() >= LAST_KEPT {
        last.remove(0);
    }
    last.push((archive.to_path_buf(), stamp, items));
}

fn read_items(archive: &Path) -> io::Result<Vec<Item>> {
    read_items_with(archive, password_for(archive, None).as_deref(), usize::MAX)
}

/// The first `max` entries, a locked 7z's names read with `password`; those whose names
/// could not be a path through it (`..`, an empty part, on Windows a drive or `\\`) left out,
/// so nothing that acts on an entry is led outside the archive.
fn read_items_with(archive: &Path, password: Option<&str>, max: usize) -> io::Result<Vec<Item>> {
    let mut out = read_raw_items(archive, password, max)?;
    out.retain(|it| sound(&it.name));
    Ok(out)
}

fn read_raw_items(archive: &Path, password: Option<&str>, max: usize) -> io::Result<Vec<Item>> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    let mut out = vec![];
    if k == Kind::Disk {
        // An entry without a time (a partition) has the image's.
        let own = modified(archive);
        // All of it, whatever `max`: it is kept (`list`), and Enter and F5 need it all.
        let _ = max;
        return Ok(crate::disk_image::list(archive)?
            .into_iter()
            .filter(|e| !e.link)
            .map(|e| Item {
                name: e.name,
                size: e.size,
                dir: e.dir,
                modified: if e.modified == 0 { own } else { e.modified },
                locked: false,
            })
            .collect());
    }
    if k == Kind::Image {
        let _ = max;
        return Ok(crate::container_image::tree(archive)?
            .files
            .iter()
            .map(|(name, n)| Item {
                name: name.clone(),
                size: n.size,
                dir: n.dir,
                modified: n.modified,
                locked: false,
            })
            .collect());
    }
    if k == Kind::Mbox {
        let all = messages(archive)?;
        let digits = all.len().to_string().len().max(3);
        return Ok(all
            .iter()
            .take(max)
            .enumerate()
            .map(|(i, m)| Item {
                name: message_name(i, digits, m),
                size: m.len,
                dir: false,
                modified: m.secs.max(0) as u64,
                locked: false,
            })
            .collect());
    }
    if k == Kind::Zip {
        // Its directory gone through an entry at a time, never built whole (`zip_dir`).
        let mut walked = vec![];
        match crate::zip_dir::entries(&mut BufReader::new(Joined::open(archive)?), &mut |c| {
            walked.push(Item {
                name: c.name.trim_end_matches('/').to_string(),
                size: c.size,
                dir: c.dir(),
                modified: c.modified().map_or(0, unix),
                locked: c.encrypted(),
            });
            walked.len() < max
        }) {
            Ok(()) => return Ok(walked),
            Err(e) if !crate::zip_dir::is_unsure(&e) => return Err(e),
            Err(_) => {}
        }
        let mut z =
            zip::ZipArchive::new(BufReader::new(Joined::open(archive)?)).map_err(io::Error::other)?;
        for i in 0..z.len().min(max) {
            let e = z.by_index_raw(i).map_err(io::Error::other)?;
            out.push(Item {
                name: e
                    .name()
                    .map_err(io::Error::other)?
                    .trim_end_matches('/')
                    .to_string(),
                size: e.size(),
                dir: e.is_dir(),
                modified: e.last_modified().map_or(0, unix),
                locked: e.encrypted(),
            });
        }
        return Ok(out);
    }
    if k == Kind::SevenZ {
        // Its list read as it is unpacked (`seven_list`), so a list too large to unpack whole
        // is listed too; one with its names locked, or one it does not read, by the crate.
        if let Ok(l) = crate::seven_list::read(&mut BufReader::new(Joined::open(archive)?), max) {
            let locked = l.locked();
            return Ok(l
                .entries
                .into_iter()
                .map(|e| Item {
                    name: e.name.replace('\\', "/").trim_end_matches('/').to_string(),
                    size: e.size,
                    dir: e.dir,
                    modified: e.modified,
                    locked: locked && e.has_stream(),
                })
                .collect());
        }
        // Not read there (or found broken there): the crate's say.
        let r = seven(archive, password)?;
        let locked = seven_locked(r.archive());
        for f in r.archive().files.iter().take(max) {
            let modified = seven_secs(f);
            out.push(Item {
                name: f.name.replace('\\', "/").trim_end_matches('/').to_string(),
                size: f.size,
                dir: f.is_directory,
                modified,
                locked: locked && f.has_stream,
            });
        }
        return Ok(out);
    }
    let mut item = |name: String, h: &tar::Header, size: u64| {
        out.push(Item {
            name,
            size,
            dir: h.entry_type().is_dir(),
            modified: h.mtime().unwrap_or(0),
            locked: false,
        });
        Ok(out.len() < max)
    };
    if k == Kind::Tar(Pack::None) {
        // A plain tar's headers are read by seeking past what lies between them.
        tar_named(
            tar::Archive::new(BufReader::new(File::open(archive)?)).entries_with_seek()?,
            |n, _, e| item(n, e.header(), e.size()),
        )?;
    } else if let Some(kept) = crate::tar_index::load(archive) {
        // Unpacked once before, in this run or an earlier one: its list is kept.
        for l in kept.into_iter().take(max) {
            out.push(Item {
                name: l.name,
                size: l.size,
                dir: l.dir,
                modified: l.modified,
                locked: false,
            });
        }
    } else {
        let (mut all, mut whole) = (vec![], true);
        let points = crate::tar_index::Points::default();
        let mut tar = tar_reader_noting(archive, k, Some(points.clone()))?;
        tar_named_at(tar.entries()?, |name, _, e, at| {
            let h = e.header();
            all.push(crate::tar_index::Listed {
                name: name.clone(),
                size: e.size(),
                dir: h.entry_type().is_dir(),
                modified: h.mtime().unwrap_or(0),
                at,
            });
            let go = item(name, e.header(), e.size())?;
            whole &= go;
            Ok(go)
        })?;
        if whole {
            // What follows the tar's end (zeros filling its last record, as a rule) is
            // unpacked too: a bzip2's points are kept only once its stream's end checks them.
            if k == Kind::Tar(Pack::Bz2) {
                let _ = io::copy(&mut tar.into_inner().take(TAR_TRAIL), &mut io::sink());
            }
            crate::tar_index::save(archive, &all);
            crate::tar_index::save_points(archive, &points.borrow());
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- for search

/// The most entries of one archive that search knows.
pub(crate) const SEARCH_ENTRIES: usize = 50_000;
/// A compressed tar or a 7z is unpacked from its start to be read, a compressed tar even to be
/// listed, a container image's layers all to know its files: larger ones are found by their
/// own name only, until they were opened once.
pub(crate) const SEARCH_UNPACK: u64 = 256 << 20;
/// The most a 7z's list of contents unpacks to for search to look into it: what it reads stays
/// small in memory. A 7z split into parts is found by its own name only.
const SEARCH_SEVEN_HEADER: u64 = 16 << 20;

fn search_skips_seven(archive: &Path) -> bool {
    in_parts(archive)
        || Joined::open(archive)
            .and_then(|mut f| seven_header_size(&mut f))
            .map_or(true, |h| h > SEARCH_SEVEN_HEADER)
}
/// The most bytes read out of one archive for its files' text.
pub(crate) const SEARCH_READ: u64 = 128 << 20;

/// Whether getting at `archive`'s entries means unpacking all before them.
fn streamed(archive: &Path) -> bool {
    matches!(kind(archive), Some(Kind::Tar(p)) if p != Pack::None)
        || kind(archive) == Some(Kind::SevenZ)
}

/// A name inside an archive that can be a path through it: no `..`, nothing empty, and no
/// deeper than 64 folders (search builds its tree of them by recursion).
fn sound(name: &str) -> bool {
    name.split('/').count() <= 64
        && name.split('/').all(|p| {
            !p.is_empty() && p != "." && p != ".." && !(cfg!(windows) && (p.contains([':', '\\']) || device(p)))
        })
}

/// Whether `part` is one of Windows' device names (`NUL`, `CON`, `COM1` …, with any ending):
/// a file of that name there is the device, not a file.
pub(crate) fn device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

/// What search may know of `archive` (`size` bytes) by name: its first `SEARCH_ENTRIES`
/// entries, as far as they are seen without a password, those whose names could not be a path
/// through it left out. `None` for one it does not look into: not an archive, too large to be
/// unpacked for a listing, unreadable, or only in the cloud (reading it would download it).
pub(crate) fn search_entries(archive: &Path, size: u64) -> Option<Vec<ArchiveEntry>> {
    entries_at(archive, size, 0)
}

/// How deep search looks into archives inside archives, the outer one not counted.
const SEARCH_DEPTH: usize = 3;

/// Whether an entry named `name` is an archive search looks into when it is inside another.
fn nests(name: &str) -> bool {
    let p = Path::new(name);
    ending_kind(p).is_some() && !in_parts(p)
}

/// `search_entries` of an archive `depth` archives deep. The archives inside it, up to
/// `SEARCH_UNPACK` each, are each copied into this run's own folder, listed there as one on disk
/// is, and the copy goes; their entries follow under their names, `SEARCH_ENTRIES` in all.
fn entries_at(archive: &Path, size: u64, depth: usize) -> Option<Vec<ArchiveEntry>> {
    // Never a disk image: they can be huge, and need 7-Zip.
    let k = search_kind(archive)?;
    if crate::cloud::keep_out(archive) || waits_to_be_opened(archive, size) {
        return None;
    }
    if k == Kind::SevenZ && search_skips_seven(archive) {
        return None;
    }
    // Never a password given in this run: what search knows is what anyone sees.
    let items = read_items_with(archive, None, SEARCH_ENTRIES).ok()?;
    let mut out: Vec<ArchiveEntry> = items
        .into_iter()
        .take(SEARCH_ENTRIES)
        .map(|it| ArchiveEntry {
            name: it.name,
            size: it.size,
            is_dir: it.dir,
            modified: it.modified,
        })
        .collect();
    let inner: std::collections::HashSet<String> = out
        .iter()
        .filter(|e| depth < SEARCH_DEPTH && !e.is_dir && nests(&e.name))
        .map(|e| e.name.clone())
        .collect();
    if !inner.is_empty() {
        let mut more = vec![];
        let room = SEARCH_ENTRIES - out.len().min(SEARCH_ENTRIES);
        let sizes: std::collections::HashMap<String, u64> = out.iter().filter(|e| inner.contains(&e.name)).map(|e| (e.name.clone(), e.size)).collect();
        let _ = search_read_at(archive, size, depth, &|n, _| inner.contains(n), &|_| false, &mut |names, from| {
            let name = names[0];
            let _ = copied_for_search(archive, name, sizes.get(name).copied().unwrap_or(u64::MAX), from, &mut |copy, len| {
                for e in entries_at(copy, len, depth + 1).unwrap_or_default() {
                    more.push(ArchiveEntry { name: format!("{name}/{}", e.name), ..e });
                }
            });
            more.len() < room
        });
        more.truncate(room);
        out.extend(more);
    }
    Some(out)
}

/// `name` (`size` bytes), an archive inside `archive`, for `f` to look into with its size: read
/// in place when it is stored there as it is (`in_place`), whatever its size; else `from` is
/// copied into a folder of its own in this run's own folder of the cache, no more than
/// `SEARCH_UNPACK` bytes, and the copy goes after. `false` when it could not be looked into.
// ponytail: a packed archive inside one is copied to be looked into, even a tar that could be
// read as it streams by; read tars in place if the copies show.
fn copied_for_search(archive: &Path, name: &str, size: u64, from: &mut dyn Read, f: &mut dyn FnMut(&Path, u64)) -> bool {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let place = in_place(archive, name).ok().flatten().filter(|_| matches!(ending_kind(Path::new(name)), Some(Kind::Zip | Kind::SevenZ)));
    if place.is_none() && size > SEARCH_UNPACK {
        return false;
    }
    let Ok(base) = run_folder("search-nested", false) else { return false };
    let dir = base.join(N.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string());
    let _ = std::fs::remove_dir_all(&dir);
    if crate::fs::private_dir(&dir).is_err() {
        return false;
    }
    let copy = dir.join(name.rsplit('/').next().unwrap_or("archive"));
    let ok = match place {
        // Its name only, empty: its bytes are read where they are.
        Some((file, start, len)) => {
            let made = File::create_new(&copy).is_ok();
            if made {
                WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).push(Window { marker: copy.clone(), file, start, len });
                f(&copy, len);
                WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).retain(|w| w.marker != copy);
            }
            made
        }
        None => {
            let made = File::create_new(&copy).and_then(|mut to| io::copy(&mut from.take(SEARCH_UNPACK + 1), &mut to));
            let ok = matches!(made, Ok(n) if n <= SEARCH_UNPACK);
            if let (true, Ok(n)) = (ok, made) {
                f(&copy, n);
            }
            ok
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// `archive` (`size` bytes) is one search lists only once it was opened, its list or tree then
/// kept: a compressed tar or a container image too large to unpack for a listing, whose list
/// or tree is not kept yet. Opening it later is no change of the file: the store asks again.
pub(crate) fn waits_to_be_opened(archive: &Path, size: u64) -> bool {
    match search_kind(archive) {
        Some(Kind::Tar(p)) if p != Pack::None => {
            size > SEARCH_UNPACK && crate::tar_index::load(archive).is_none()
        }
        // A container image: the files of its merged layers, never its blobs. An OCI layout's
        // size is that of its layers, not of its `index.json`.
        Some(Kind::Tar(Pack::None) | Kind::Image) if crate::container_image::is_image(archive) => {
            let size = match crate::container_image::layout_index(archive) {
                true => crate::container_image::image(archive)
                    .map_or(0, |i| i.layers.iter().map(|l| l.1).sum()),
                false => size,
            };
            size > SEARCH_UNPACK && !crate::container_image::tree_known(archive)
        }
        _ => false,
    }
}

/// Read the files of `archive` (`size` bytes) that `wanted` picks by name and size, in the
/// order they are in it: `read` gets each with its names (more than one when the same bytes
/// are there under several: links in a container image), and says whether to go on. Locked
/// files are never read, and no password is ever used. Archives that would be unpacked past
/// `SEARCH_UNPACK` are not read, nor archives only in the cloud. An archive inside it that
/// `inside` picks by name (up to `SEARCH_DEPTH` deep, `SEARCH_UNPACK` each) is copied out and
/// read so too, its files named through it (`inner.zip/notes.txt`).
pub(crate) fn search_read(
    archive: &Path,
    size: u64,
    wanted: &dyn Fn(&str, u64) -> bool,
    inside: &dyn Fn(&str) -> bool,
    read: &mut dyn FnMut(&[&str], &mut dyn Read) -> bool,
) -> io::Result<()> {
    search_read_at(archive, size, 0, wanted, inside, read)
}

/// `search_read` of an archive `depth` archives deep.
fn search_read_at(
    archive: &Path,
    size: u64,
    depth: usize,
    wanted: &dyn Fn(&str, u64) -> bool,
    inside: &dyn Fn(&str) -> bool,
    read: &mut dyn FnMut(&[&str], &mut dyn Read) -> bool,
) -> io::Result<()> {
    // An archive inside picked: read in place or through a copy, its files named through it.
    let looked = |name: &str, _: u64| depth < SEARCH_DEPTH && nests(name) && inside(name);
    let mut read = |names: &[&str], from: &mut dyn Read| -> bool {
        let name = names[0];
        if !(depth < SEARCH_DEPTH && nests(name) && inside(name)) {
            return read(names, from);
        }
        let mut go = true;
        copied_for_search(archive, name, 0, from, &mut |copy, len| {
            let _ = search_read_at(
                copy,
                len,
                depth + 1,
                &|n, s| wanted(&format!("{name}/{n}"), s),
                &|n| inside(&format!("{name}/{n}")),
                &mut |n, r| {
                    let full: Vec<String> = n.iter().map(|n| format!("{name}/{n}")).collect();
                    go = read(&full.iter().map(String::as_str).collect::<Vec<_>>(), r);
                    go
                },
            );
        });
        go
    };
    let read = &mut read;
    let wanted = &|name: &str, size: u64| wanted(name, size) || looked(name, size);
    let k = search_kind(archive).ok_or_else(|| not_archive(archive))?;
    if crate::cloud::keep_out(archive) {
        return Err(crate::cloud::not_here(archive));
    }
    if streamed(archive) && size > SEARCH_UNPACK {
        return Ok(());
    }
    if matches!(k, Kind::Tar(Pack::None) | Kind::Image) && crate::container_image::is_image(archive) {
        if waits_to_be_opened(archive, size) {
            return Ok(());
        }
        return crate::container_image::search_read(archive, &|name, n| sound(name) && wanted(name, n), read);
    }
    match k {
        Kind::Zip => {
            // Its directory gone through an entry at a time (`zip_dir`), each wanted file
            // unpacked from its place; the directory names where they are first, so they are
            // read from it in that order.
            let mut file = BufReader::new(Joined::open(archive)?);
            let mut picked = vec![];
            let mut seen = 0;
            let walked = crate::zip_dir::entries(&mut file, &mut |c| {
                seen += 1;
                if !c.dir() && !c.link() && !c.encrypted() && sound(&c.name) && wanted(&c.name, c.size) {
                    picked.push(c);
                }
                seen < SEARCH_ENTRIES
            });
            match walked {
                Ok(()) => {
                    for c in picked {
                        // An entry that cannot be opened is passed over, not the rest.
                        let Ok(mut f) = c.open(&mut file) else { continue };
                        if !read(&[&c.name], &mut f) {
                            break;
                        }
                    }
                    return Ok(());
                }
                Err(e) if !crate::zip_dir::is_unsure(&e) => return Err(e),
                Err(_) => {}
            }
            let mut z = zip::ZipArchive::new(file).map_err(io::Error::other)?;
            for i in 0..z.len().min(SEARCH_ENTRIES) {
                // An entry that cannot be opened is passed over, not the rest of the archive.
                let (name, ok) = {
                    let Ok(e) = z.by_index_raw(i) else { continue };
                    let Ok(name) = e.name().map(|n| n.trim_end_matches('/').to_string()) else {
                        continue;
                    };
                    let ok =
                        e.is_file() && !e.encrypted() && sound(&name) && wanted(&name, e.size());
                    (name, ok)
                };
                if ok
                    && let Ok(mut f) = z.by_index(i)
                    && !read(&[&name], &mut f)
                {
                    break;
                }
            }
        }
        Kind::SevenZ => {
            if search_skips_seven(archive) {
                return Ok(());
            }
            let mut r = seven(archive, None)?;
            if seven_locked(r.archive()) {
                return Ok(());
            }
            // An entry not read to its end must still be read past: in a solid 7z the next one
            // comes from the same stream.
            r.for_each_entries(|e, from| {
                let name = e.name.replace('\\', "/").trim_end_matches('/').to_string();
                let go = e.is_directory
                    || !e.has_stream
                    || !sound(&name)
                    || !wanted(&name, e.size)
                    || read(&[&name], from);
                if go {
                    io::copy(from, &mut io::sink())?;
                }
                Ok(go)
            })
            .map_err(seven_error)?;
        }
        Kind::Disk | Kind::Mbox | Kind::Image => {}
        Kind::Tar(_) => {
            let mut seen = 0;
            tar_named(tar_reader(archive, k)?.entries()?, |name, _, e| {
                seen += 1;
                Ok(seen <= SEARCH_ENTRIES
                    && !(e.header().entry_type().is_file()
                        && sound(&name)
                        && wanted(&name, e.size())
                        && !read(&[&name], e)))
            })?;
        }
    }
    Ok(())
}

/// Each line of the whole text of `path`, a file inside an archive (not inside one in it), as
/// `extract::whole_lines` or `extract::plain::each_line` gives a file's on disk: text, code,
/// CSV and logs as they are unpacked; a document (`extract::whole_document`) from a copy of
/// it, no larger than `SEARCH_UNPACK`, in this run's own folder, gone once read. `None` when
/// it cannot be read now, as `search_read` would not read it.
pub(crate) fn whole_lines_inside(path: &Path, f: &mut dyn FnMut(&str)) -> Option<bool> {
    let (archive, inner) = split(path).filter(|(_, i)| !i.is_empty())?;
    let size = std::fs::metadata(&archive).ok()?.len();
    let name = Path::new(&inner);
    let document = crate::extract::whole_document(name);
    let mut out = None;
    let wanted = |n: &str, size: u64| n == inner && (!document || size <= SEARCH_UNPACK);
    let inside = |n: &str| inner.strip_prefix(n).is_some_and(|rest| rest.starts_with('/'));
    search_read(&archive, size, &wanted, &inside, &mut |_, from| {
        out = if document {
            run_folder("whole", true).ok().and_then(|dir| {
                let copy = dir.join(inner.rsplit('/').next().unwrap_or("file"));
                let made = File::create(&copy)
                    .and_then(|mut to| io::copy(&mut from.take(SEARCH_UNPACK + 1), &mut to));
                let lines = made
                    .ok()
                    .filter(|&n| n <= SEARCH_UNPACK)
                    .and_then(|_| crate::extract::whole_lines(&copy, f));
                let _ = std::fs::remove_file(&copy);
                lines
            })
        } else {
            crate::extract::plain::each_line_from(from, crate::code::is_code(&inner), f).map(|()| true)
        };
        false
    })
    .ok()?;
    out
}

/// What is inside `archive` at `inner`, as a folder listing: its files, and the folders in it,
/// also those the archive only names in the paths of its files. `..` leads back out.
pub fn list_in(archive: &Path, inner: &str) -> io::Result<Vec<crate::fs::Entry>> {
    listing(archive, inner).map(|(entries, _)| entries)
}

/// `list_in`, and whether anything in that folder or below it is locked. A folder's size is
/// that of the files in it.
pub fn listing(archive: &Path, inner: &str) -> io::Result<(Vec<crate::fs::Entry>, bool)> {
    let (real, inside, _held) = resolve(archive, inner, true)?;
    // Part by part, so the paths have the system's separator throughout, as the search's do.
    let at = inner
        .split('/')
        .filter(|p| !p.is_empty())
        .fold(archive.to_path_buf(), |p, part| p.join(part));
    let prefix = if inside.is_empty() {
        String::new()
    } else {
        format!("{inside}/")
    };
    let mut seen = std::collections::BTreeMap::new();
    let mut locked = false;
    for it in items(&real)?.iter() {
        let Some(rest) = it.name.strip_prefix(&prefix).filter(|r| !r.is_empty()) else {
            continue;
        };
        locked |= it.locked;
        let (first, deeper) = match rest.split_once('/') {
            Some((first, _)) => (first.to_string(), true),
            None => (rest.to_string(), it.dir),
        };
        let e = seen
            .entry(first.clone())
            .or_insert_with(|| crate::fs::Entry {
                path: at.join(&first),
                hidden: first.starts_with('.'),
                name: first,
                is_dir: deeper,
                is_symlink: false,
                is_exec: false,
                size: 0,
                modified: it.modified,
                created: 0,
                online: false,
                referenced: 0,
            });
        if deeper {
            e.is_dir = true;
            e.size += it.size;
        } else if it.dir {
            e.modified = it.modified;
        } else {
            (e.size, e.modified) = (it.size, it.modified);
        }
    }
    let up = at.parent().unwrap_or(archive).to_path_buf();
    let mut out = vec![crate::fs::Entry {
        name: "..".into(),
        path: up,
        is_dir: true,
        is_symlink: false,
        is_exec: false,
        hidden: false,
        size: 0,
        modified: 0,
        created: 0,
        online: false,
        referenced: 0,
    }];
    out.extend(seen.into_values());
    Ok((out, locked))
}

/// Whether `inner` is a folder in the archive (its top, a folder entry, or a folder only named
/// in the paths of its files).
pub fn is_folder(archive: &Path, inner: &str) -> io::Result<bool> {
    let (archive, inner, _held) = resolve(archive, inner, true)?;
    let prefix = format!("{inner}/");
    Ok(inner.is_empty()
        || items(&archive)?
            .iter()
            .any(|it| (it.dir && it.name == inner) || it.name.starts_with(&prefix)))
}

/// Whether anything at or below `inner` is locked with a password ("" for the whole archive).
pub fn locked_at(archive: &Path, inner: &str) -> io::Result<bool> {
    let (archive, inner, _held) = resolve(archive, inner, true)?;
    let inner = inner.as_str();
    let prefix = format!("{inner}/");
    Ok(items(&archive)?.iter().any(|it| {
        it.locked && (inner.is_empty() || it.name == inner || it.name.starts_with(&prefix))
    }))
}

/// What copying `inner` ("" for everything) out of `archive` takes: its bytes and files, and
/// whether a file of it is locked with no password known this run.
pub fn measure(archive: &Path, inner: &str) -> io::Result<(u64, usize, bool)> {
    let prefix = format!("{inner}/");
    let known = password_for(archive, None).is_some();
    let mut out = (0, 0, false);
    for it in items(archive)?
        .iter()
        .filter(|it| inner.is_empty() || it.name == inner || it.name.starts_with(&prefix))
    {
        if !it.dir {
            out.0 += it.size;
            out.1 += 1;
        }
        out.2 |= it.locked && !known;
    }
    Ok(out)
}

/// Copy `inner` (a file, or a folder and everything in it) out of `archive` into `dest`, as
/// `fs::copy` would: into it when it is a folder. Returns where it landed. A locked file needs
/// `password`; without it, or with a wrong one, the error is `LOCKED`. `whole`: every entry
/// below `inner` comes out, or none does (a move takes them out of the archive after).
pub fn copy_out(
    archive: &Path,
    inner: &str,
    dest: &Path,
    password: Option<&str>,
    whole: bool,
) -> io::Result<PathBuf> {
    let name = inner
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to copy"))?;
    let to = if dest.is_dir() {
        dest.join(name)
    } else {
        dest.to_path_buf()
    };
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", to.display()),
        ));
    }
    let (archive, inner, _held) = resolve(archive, inner, false)?;
    copy_out_with(
        &archive,
        &inner,
        &to,
        password_for(&archive, password).as_deref(),
        Copy {
            whole,
            ..Copy::default()
        },
    )
    .map(|_| to)
}

/// Files larger than this inside an archive are not copied out just to be looked at.
pub const PEEK_MAX: u64 = 256 * 1024 * 1024;

/// `name` is too large to be copied out just to be looked at (also a history's).
pub(crate) fn too_large(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        crate::t!("archive.too_large", "name" => name),
    )
}

/// A copy of the file at `path` (a path through an archive), to look at: the
/// preview pane, F3. It keeps the file's name, so it is previewed by its kind. The copy before
/// it goes, and copies left by earlier runs go after a day. A locked file needs its password
/// remembered: `LOCKED` without it.
pub fn peek(path: &Path) -> io::Result<PathBuf> {
    let (archive, inner) = split(path).filter(|(_, i)| !i.is_empty()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, crate::t!("err.not_in_archive"))
    })?;
    let (archive, inner, _held) = resolve(&archive, &inner, false)?;
    let it = items(&archive)?
        .iter()
        .find(|it| it.name == inner)
        .cloned()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                crate::t!("err.not_in_the_archive", "name" => inner),
            )
        })?;
    if it.dir {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("err.is_folder", "name" => inner),
        ));
    }
    // The first part of a 7z split into parts: looked at with all its parts beside it, from
    // the copy opening it makes (`spilled`).
    let zipped = || {
        let all = items(&archive).unwrap_or_default();
        !zip_parts(&inner, |n| all.iter().any(|it| it.name == n && !it.dir)).is_empty()
    };
    if in_parts(Path::new(&inner)) || zipped() {
        // Held until the next such preview (`PEEKED`).
        let held = spilled(&archive, &inner)?;
        let copy = held.0[0].clone();
        *PEEKED.lock().unwrap_or_else(|e| e.into_inner()) = Some(held);
        return Ok(copy);
    }
    if it.size > PEEK_MAX {
        return Err(too_large(&inner));
    }
    // One at a time: each starts in a fresh folder, which would take away the copy of one
    // still being looked at, and peeks piling up while the cursor moves each unpack.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let folder = peek_folder()?;
    let to = folder.join(inner.rsplit('/').next().filter(|n| one_name(n)).unwrap_or("file"));
    // What comes out is counted too: a size the archive claims may be a lie.
    copy_out_with(
        &archive,
        &inner,
        &to,
        password_for(&archive, None).as_deref(),
        Copy {
            limit: PEEK_MAX,
            ..Copy::default()
        },
    )?;
    // The copy has the date of the file in the archive (else the archive's), so what is made
    // from it and kept by its date (a LaTeX or LibreOffice preview) is found again next time.
    let when = if it.modified > 0 {
        Some(crate::fs::epoch(it.modified))
    } else {
        std::fs::metadata(&archive).and_then(|m| m.modified()).ok()
    };
    if let Some(when) = when {
        let _ = File::options()
            .write(true)
            .open(&to)
            .and_then(|f| f.set_modified(when));
    }
    // A ChessBase or SCID database is read with the files beside it under its name (`.cbg`,
    // `.sg4` …): those come along, so its games show as outside.
    if (crate::chess::cbh::is_cbh(&to) && !crate::chess::cbv::is_cbv(&to)) || crate::chess::scid::is_scid(&to) {
        let (within, stem) = match inner.rsplit_once('/') {
            Some((f, n)) => (format!("{f}/"), n),
            None => (String::new(), inner.as_str()),
        };
        let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
        let all = items(&archive)?;
        let beside: Vec<&Item> = all
            .iter()
            .filter(|i| {
                !i.dir
                    && i.name != inner
                    && i.name.strip_prefix(&within).is_some_and(|n| {
                        one_name(n)
                            && n.rsplit_once('.').is_some_and(|(s, _)| s.eq_ignore_ascii_case(stem))
                    })
            })
            .collect();
        if beside.iter().map(|i| i.size).sum::<u64>() > PEEK_MAX {
            return Err(too_large(&inner));
        }
        for i in beside {
            let at = folder.join(&i.name[within.len()..]);
            // Never outside the peek folder, whatever the name says.
            if at.parent() != Some(folder.as_path()) {
                continue;
            }
            copy_out_with(
                &archive,
                &i.name,
                &at,
                password_for(&archive, None).as_deref(),
                Copy {
                    limit: PEEK_MAX,
                    ..Copy::default()
                },
            )?;
        }
    }
    Ok(to)
}

/// A name that is one file's in a folder: no `/` or `..`, and on Windows no `\\`, `:` (a drive
/// or a stream) or device name, which are only characters elsewhere.
fn one_name(n: &str) -> bool {
    !n.contains('/')
        && !(cfg!(windows) && (n.contains(['\\', ':']) || device(n)))
        && matches!(
            Path::new(n).components().collect::<Vec<_>>()[..],
            [std::path::Component::Normal(_)]
        )
}

/// A fresh folder for this run's copy to look at (also a history's, `history::peek`): the copy
/// before it goes, and copies left by earlier runs go after a day.
pub(crate) fn peek_folder() -> io::Result<PathBuf> {
    // Tests run side by side in one process, each on its own thread: a folder each, so one
    // test's fresh folder never takes away the copy another is still reading.
    #[cfg(test)]
    return run_folder(&format!("peek-{:?}", std::thread::current().id()), true);
    #[cfg(not(test))]
    run_folder("peek", true)
}

/// The runs' folders in `base` last written more than `age` ago, removed.
pub(crate) fn sweep(base: &Path, age: std::time::Duration) {
    sweep_but(base, age, None);
}

/// `sweep`, leaving the folder named `keep` (this run's own) whatever its age, and any a run
/// still alive holds (`spill::in_use`: a desktop app open for days keeps what it opened).
fn sweep_but(base: &Path, age: std::time::Duration, keep: Option<&str>) {
    for old in std::fs::read_dir(base).into_iter().flatten().flatten() {
        if keep.is_none_or(|k| old.file_name() != k)
            && crate::spill::alive_by_name(&old.file_name().to_string_lossy()) != Some(true)
            && old
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().unwrap_or_default() > age)
        {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
}

/// This run's own folder in the cache's `sub`, for this user's eyes only (a copy may be of a
/// locked file). Folders left by runs that ended go after a day; `fresh` empties this run's too.
pub(crate) fn run_folder(sub: &str, fresh: bool) -> io::Result<PathBuf> {
    let base = crate::helper::folder()
        .ok_or_else(|| io::Error::other(crate::t!("err.no_cache_folder")))?
        .join(sub);
    // This run's own folder stays however long ago it last changed: what is in it may be in use.
    let me = crate::spill::run();
    sweep_but(&base, std::time::Duration::from_secs(24 * 3600), Some(me));
    let run = base.join(me);
    if fresh {
        let _ = std::fs::remove_dir_all(&run);
    }
    crate::fs::private_dir(&run)?;
    Ok(run)
}

/// `inner` (everything with "") out of `archive` to `to`, which is where it lands: the file, or
/// the folder it is unpacked into. Whatever was written is removed when it fails. A password
/// that opened a locked file is kept for the rest of this run.
fn copy_out_with(
    archive: &Path,
    inner: &str,
    to: &Path,
    password: Option<&str>,
    how: Copy,
) -> io::Result<()> {
    let (mut any, mut used) = (false, false);
    let r = copy_entries(archive, inner, to, password, how, &mut any, &mut used);
    // Stopped, what came out stays, unless it all must (a move).
    if r.is_err() && !inner.is_empty() && (how.whole || !crate::stop::asked()) {
        let _ = if to.is_dir() {
            std::fs::remove_dir_all(to)
        } else {
            std::fs::remove_file(to)
        };
    }
    r?;
    if !any && !inner.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{inner} is not in {}", archive.display()),
        ));
    }
    if let (true, Some(pw)) = (used, password) {
        remember(archive, pw);
    }
    Ok(())
}

/// How `copy_out_with` copies: `whole`, an entry below it that cannot come out is an error, not
/// left behind; `limit`, the most bytes it writes in all.
#[derive(Clone, Copy)]
struct Copy {
    whole: bool,
    limit: u64,
}

impl Default for Copy {
    fn default() -> Copy {
        Copy {
            whole: false,
            limit: u64::MAX,
        }
    }
}

fn copy_entries(
    archive: &Path,
    inner: &str,
    to: &Path,
    password: Option<&str>,
    how: Copy,
    any: &mut bool,
    used: &mut bool,
) -> io::Result<()> {
    let prefix = format!("{inner}/");
    let below = |name: &str| inner.is_empty() || name == inner || name.starts_with(&prefix);
    // Where an entry lands, never outside `to`: `..` and absolute parts are refused (on Windows
    // also a drive or a `\\`, which are only characters elsewhere).
    let place = |name: &str| -> Option<PathBuf> {
        let rest = if inner.is_empty() {
            name
        } else if name == inner {
            ""
        } else {
            name.strip_prefix(&prefix)?
        };
        let mut p = to.to_path_buf();
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            if part == ".." || (cfg!(windows) && (part.contains(['\\', ':']) || device(part))) {
                return None;
            }
            p.push(part);
        }
        Some(p)
    };
    // An entry below `inner` that is not copied: left out, or for `whole` the copy fails.
    let skip = |name: &str| -> io::Result<()> {
        if how.whole && below(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{name} cannot come out of the archive here, so nothing was moved"),
            ));
        }
        Ok(())
    };
    let left = std::cell::Cell::new(how.limit);
    // `secs`: the entry's modified time, which the file out of the archive keeps (0: none).
    // Written under a passing name (noted, so a kill leaves nothing that looks whole) and given
    // its own only when whole.
    let write = |path: &Path, from: &mut dyn Read, secs: u64| -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(crate::fs::exists(path));
        }
        let tmp = crate::fs::beside(path);
        let mut f = File::create_new(&tmp)?;
        // Stop is looked at on every read; a file stopped or failed half way goes.
        let n = io::copy(
            &mut crate::stop::Reader(from.take(left.get().saturating_add(1))),
            &mut f,
        )
        .inspect_err(|_| drop(std::fs::remove_file(&tmp)))?;
        if n > left.get() {
            drop(f);
            let _ = std::fs::remove_file(&tmp);
            return Err(too_large(
                &path.file_name().unwrap_or_default().to_string_lossy(),
            ));
        }
        left.set(left.get() - n);
        if secs > 0 {
            let _ = f.set_modified(crate::fs::epoch(secs));
        }
        drop(f);
        crate::fs::put(&tmp, path).inspect_err(|_| drop(std::fs::remove_file(&tmp)))
    };
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    if k == Kind::Image {
        // Folders made here, files read out of their layers, each layer once at most.
        let tree = crate::container_image::tree(archive)?;
        let mut files = vec![];
        for (name, n) in tree.files.iter().filter(|(name, _)| below(name)) {
            let Some(path) = place(name) else {
                skip(name)?;
                continue;
            };
            *any = true;
            match n.src {
                None => std::fs::create_dir_all(&path)?,
                Some(src) => files.push((src, path, n.mode, n.modified)),
            }
        }
        let wanted: Vec<_> = files.iter().map(|(src, path, _, secs)| (*src, path.clone(), *secs)).collect();
        crate::container_image::read_out(archive, &wanted, &write)?;
        // A script stays one; nothing but the permission bits is taken.
        #[cfg(unix)]
        for (_, path, mode, _) in files.iter().filter(|(_, _, m, _)| m & 0o111 != 0) {
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode & 0o755))?;
        }
        return Ok(());
    }
    if k == Kind::Mbox {
        // Each message is its bytes in the mailbox, under the name it is listed by.
        let all = messages(archive)?;
        let digits = all.len().to_string().len().max(3);
        let mut f = File::open(archive)?;
        // All of it: a message that cannot be written is left out and counted, the rest come.
        let mut skipped = 0u64;
        for (i, m) in all.iter().enumerate() {
            let name = message_name(i, digits, m);
            if !below(&name) {
                continue;
            }
            let Some(path) = place(&name) else { continue };
            *any = true;
            f.seek(io::SeekFrom::Start(m.start))?;
            match write(&path, &mut (&mut f).take(m.len), m.secs.max(0) as u64) {
                Err(e) if inner.is_empty() && !crate::stop::asked() => {
                    let _ = (e, std::fs::remove_file(&path));
                    skipped += 1;
                }
                r => r?,
            }
        }
        if skipped > 0 {
            return Err(io::Error::other(crate::tn!("archive.messages_skipped", skipped)));
        }
        return Ok(());
    }
    if k == Kind::Disk {
        // What is listed decides (the names 7-Zip would take as they are): a file is streamed
        // out, a folder (or all of it) unpacked through a folder of its own.
        let all = items(archive)?;
        let file = all.iter().find(|it| it.name == inner && !it.dir);
        if let Some(it) = file {
            *any = true;
            let path = place(&it.name).ok_or_else(|| io::Error::other(crate::t!("disk_image.read_only")))?;
            return crate::disk_image::read_file(archive, inner, it.size, &mut |r| write(&path, r, it.modified));
        }
        if !inner.is_empty() && !all.iter().any(|it| it.name.starts_with(&prefix)) {
            return Ok(());
        }
        *any = true;
        if inner.is_empty() {
            // `to` is the folder made for it (`extract`): unpacked next to it, then in its place.
            std::fs::remove_dir(to)?;
        }
        // p7zip takes them one by one, from what is listed (no links: they are left out).
        let entries: Vec<crate::disk_image::Entry> = all
            .iter()
            .filter(|it| below(&it.name))
            .map(|it| crate::disk_image::Entry {
                name: it.name.clone(),
                size: it.size,
                dir: it.dir,
                modified: it.modified,
                link: false,
            })
            .collect();
        return crate::disk_image::copy_tree(archive, inner, &entries, to);
    }
    if k == Kind::Zip {
        // One file, not locked: found in the directory as it is gone through, and unpacked from
        // its place, without building the whole directory (`zip_dir`).
        let mut file = BufReader::new(Joined::open(archive)?);
        let one = match (!inner.is_empty()).then(|| crate::zip_dir::find(&mut file, inner)) {
            Some(Ok(c)) => c.filter(|c| !c.encrypted()),
            Some(Err(e)) if !crate::zip_dir::is_unsure(&e) => return Err(e),
            _ => None,
        };
        if let Some(c) = one {
            let Some(path) = place(&c.name) else {
                return skip(&c.name);
            };
            *any = true;
            let mut e = c.open(&mut file)?;
            let mut from = Noting {
                from: &mut e,
                left: c.size,
                failed: false,
            };
            if let Err(err) = write(&path, &mut from, c.modified().map_or(0, unix)) {
                return Err(copy_error(err, &from, false));
            }
            #[cfg(unix)]
            if let Some(m) = c.unix_mode().filter(|m| m & 0o111 != 0) {
                std::fs::set_permissions(
                    &path,
                    std::os::unix::fs::PermissionsExt::from_mode(m & 0o777 & !NOT_SHARED),
                )?;
            }
            return Ok(());
        }
        let mut z = zip::ZipArchive::new(file).map_err(io::Error::other)?;
        // One file asked for: that entry alone, never the entries below a folder of its name.
        let one = (!inner.is_empty())
            .then(|| z.index_for_name(inner))
            .flatten()
            .filter(|&i| z.by_index_raw(i).is_ok_and(|e| !e.is_dir()));
        // Files that share their packed bytes unpack to many times the zip (a few kilobytes to
        // gigabytes): one file of such a zip may come out, not a folder of them.
        if one.is_none() && z.has_overlapping_files().map_err(io::Error::other)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                crate::t!("archive.overlapping"),
            ));
        }
        for i in one.map_or(0..z.len(), |i| i..i + 1) {
            let (name, dir, encrypted, mode, secs) = {
                let e = z.by_index_raw(i).map_err(io::Error::other)?;
                (
                    e.name()
                        .map_err(io::Error::other)?
                        .trim_end_matches('/')
                        .to_string(),
                    e.is_dir(),
                    e.encrypted(),
                    e.unix_mode(),
                    e.last_modified().map_or(0, unix),
                )
            };
            let Some(path) = place(&name) else {
                skip(&name)?;
                continue;
            };
            *any = true;
            if dir {
                std::fs::create_dir_all(&path)?;
                continue;
            }
            let mut e = match (encrypted, password) {
                (true, None) => return Err(locked()),
                (true, Some(pw)) => z.by_index_decrypt(i, pw.as_bytes()).map_err(|e| match e {
                    zip::result::ZipError::InvalidPassword => locked(),
                    e => io::Error::other(e),
                })?,
                (false, _) => z.by_index(i).map_err(io::Error::other)?,
            };
            // A wrong ZipCrypto password is only seen in what comes out: the check fails.
            let size = e.size();
            let mut from = Noting {
                from: &mut e,
                left: size,
                failed: false,
            };
            if let Err(err) = write(&path, &mut from, secs) {
                return Err(copy_error(err, &from, encrypted));
            }
            // A script stays one; nothing but the permission bits is taken.
            #[cfg(unix)]
            if let Some(m) = mode.filter(|m| m & 0o111 != 0) {
                std::fs::set_permissions(
                    &path,
                    std::os::unix::fs::PermissionsExt::from_mode(m & 0o777 & !NOT_SHARED),
                )?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            *used |= encrypted;
            if name == inner {
                break;
            }
        }
    } else if k == Kind::SevenZ {
        // One file, not locked: through the list read as it is unpacked, its block unpacked
        // up to it (`seven_list`), however long the list; else by the crate.
        if !inner.is_empty() {
            let mut file = BufReader::new(Joined::open(archive)?);
            match crate::seven_list::find(&mut file, inner) {
                Ok(Some((l, i))) if !l.locked() => {
                    let e = &l.entries[i];
                    if let Ok(mut from) = l.open(&mut file, e) {
                        let Some(path) = place(inner) else {
                            return skip(inner);
                        };
                        *any = true;
                        let mut from = Noting {
                            from: &mut from,
                            left: e.size,
                            failed: false,
                        };
                        return write(&path, &mut from, e.modified).map_err(|err| copy_error(err, &from, false));
                    }
                }
                // Not read there (or found broken there): the crate's say.
                _ => {}
            }
        }
        let mut r = seven(archive, password)?;
        let locked = seven_locked(r.archive());
        let mut failed = None;
        let mut each = |e: &sevenz_rust2::ArchiveEntry,
                        from: &mut dyn Read|
         -> Result<bool, sevenz_rust2::Error> {
            let name = e.name.replace('\\', "/").trim_end_matches('/').to_string();
            // Read past, not skipped: in a solid 7z the next entry comes from the same stream.
            let Some(path) = place(&name) else {
                if let Err(err) = skip(&name) {
                    failed = Some(err);
                    return Ok(false);
                }
                return io::copy(from, &mut io::sink())
                    .map(|_| true)
                    .map_err(Into::into);
            };
            *any = true;
            let mut from = Noting {
                from,
                left: e.size,
                failed: false,
            };
            let done = if e.is_directory {
                std::fs::create_dir_all(&path)
            } else {
                write(&path, &mut from, seven_secs(e))
            };
            if let Err(err) = done {
                failed = Some(copy_error(err, &from, locked));
                return Ok(false);
            }
            // The one file asked for is out: nothing after it is unpacked.
            Ok(e.is_directory || name != inner)
        };
        // One file: only its own block is unpacked, not those before it (a 7z in many blocks).
        let a = r.archive();
        let one = a
            .files
            .iter()
            .position(|f| !f.is_directory && f.name.replace('\\', "/") == inner);
        match one.map(|i| (i, a.stream_map.file_block_index[i])) {
            Some((_, Some(b))) => {
                let pw = password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::from);
                sevenz_rust2::BlockDecoder::new(1, b, a, &pw, &mut Joined::open(archive)?)
                    .for_each_entries(&mut each)
                    .map(drop)
            }
            // An empty file is in no block.
            Some((i, None)) => each(&a.files[i], &mut io::empty()).map(drop),
            None => r.for_each_entries(each),
        }
        .map_err(seven_error)?;
        if let Some(err) = failed {
            return Err(err);
        }
        *used |= locked;
    } else {
        // One file of a compressed tar: from a place before it, when its place is known and the
        // file can be unpacked from there (an xz in blocks, a gzip or a zstd of frames listed
        // whole before), not from the start.
        let Kind::Tar(pack) = k else { unreachable!() };
        let from = crate::tar_index::load(archive)
            .and_then(|all| all.into_iter().find(|l| l.name == inner && !l.dir))
            .map(|l| -> io::Result<Option<Box<dyn Read>>> {
                Ok(match crate::tar_index::from(archive, pack, l.at)? {
                    Some((mut r, skip)) => {
                        if io::copy(&mut (&mut r).take(skip), &mut io::sink())? != skip {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        }
                        Some(r)
                    }
                    None => None,
                })
            })
            .transpose()?
            .flatten();
        let mut tar = match from {
            Some(r) => tar::Archive::new(r),
            None => tar_reader(archive, k)?,
        };
        tar_named(tar.entries()?, |name, _, e| {
            let Some(path) = place(&name) else {
                skip(&name)?;
                return Ok(true);
            };
            *any = true;
            let t = e.header().entry_type();
            if t.is_dir() {
                std::fs::create_dir_all(&path)?;
            } else if t.is_file() {
                let secs = e.header().mtime().unwrap_or(0);
                write(&path, e, secs)?;
                // The one file asked for is out: nothing after it is unpacked.
                if name == inner {
                    return Ok(false);
                }
            } else {
                // A link or a device: not made here.
                skip(&name)?;
            }
            Ok(true)
        })?;
    }
    Ok(())
}

/// What goes into an archive being written: a file from disk, or a folder.
enum New {
    File(PathBuf),
    Dir,
}

/// A zip's date and time (local, without a zone) from seconds since the Unix epoch.
fn zip_time(secs: u64) -> zip::DateTime {
    use chrono::{Datelike, TimeZone, Timelike};
    let Some(l) = chrono::Local.timestamp_opt(secs as i64, 0).single() else {
        return zip::DateTime::default();
    };
    zip::DateTime::from_date_and_time(
        l.year() as u16,
        l.month() as u8,
        l.day() as u8,
        l.hour() as u8,
        l.minute() as u8,
        l.second() as u8,
    )
    .unwrap_or_default()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// How a 7z is locked with `pw`: AES-256, its key made with 2^19 rounds as 7-Zip makes it (the
/// crate's own 2^8 makes guessing the password two thousand times cheaper), then LZMA2.
pub(crate) fn seven_locking(pw: &str) -> Vec<sevenz_rust2::EncoderConfiguration> {
    let mut aes =
        sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(pw));
    aes.num_cycles_power = 19;
    vec![aes.into(), sevenz_rust2::EncoderMethod::LZMA2.into()]
}

/// One archive is written at a time: two changes to one archive at once would each write it
/// anew from the same old one, and the change done last would drop the other.
// ponytail: one lock for all archives; a lock per archive if two panes ever wait on each other.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The same across processes (the desktop app and the terminal app, two windows): a lock on a
/// file in Coxswain's cache folder, held while the returned file is open. `None` when there is
/// no cache folder to hold it.
fn writing_lock() -> io::Result<Option<File>> {
    let Some(dir) = crate::helper::folder() else {
        return Ok(None);
    };
    crate::fs::private_dir(&dir)?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("archive-writing.lock"))?;
    f.lock()?;
    Ok(Some(f))
}

/// What the pack and copy dialogs say when the disk may be short of room to write `sources`
/// into `archive` (a new one, or one that is there): it is written beside it first, and one that
/// is there takes a second copy of itself while that runs. `None`: room enough, or not known.
pub fn short_of_room(archive: &Path, sources: &[PathBuf]) -> Option<String> {
    let free = crate::fs::free_space(archive.parent()?)?;
    let have = std::fs::metadata(archive).map_or(0, |m| m.len());
    // ponytail: walks the sources when the dialog opens; the sizes kept by `sizes` if a huge
    // tree makes that slow.
    let new: u64 = sources.iter().map(|s| crate::fs::dir_size(s).0).sum();
    let need = have.saturating_add(new);
    (need > free).then(|| {
        crate::t!("archive.room_short", "need" => crate::settings::human(need), "free" => crate::settings::human(free))
    })
}

/// Date `tmp`, the archive written anew, after `archive` as it was. Every list kept of an
/// archive (this run's, the compressed tars' index kept between runs, another app's) is told
/// apart by its size and modified time; Windows dates a file by a clock that moves in ticks of
/// up to 16 ms, so a rewrite of the same size within one (a renamed tar, a short name moved)
/// looked unchanged and showed its old contents. Up a millisecond, or two seconds where the
/// file system keeps no finer time (FAT).
fn dated_after(tmp: &Path, archive: &Path) {
    let Ok(old) = std::fs::metadata(archive).and_then(|m| m.modified()) else {
        return;
    };
    let Ok(f) = File::options().write(true).open(tmp) else {
        return;
    };
    for step in [1, 2000] {
        if f.metadata().and_then(|m| m.modified()).is_ok_and(|t| t > old) {
            break;
        }
        let _ = f.set_modified(old + std::time::Duration::from_millis(step));
    }
}

/// Write `archive` anew: each old entry by what `keep` says of its name (`None`: left out,
/// `Some(name)`: kept under that name, as it was, locked or not), then `add`. Into a new
/// file first, which then takes the old one's place, so a failure leaves the archive whole.
/// A 7z with locked contents is read with `password` (or the one remembered), which is then
/// kept for this run. A new archive (`create`) is locked with `password`, if any, and a new
/// 7z's names too with `hide_names`.
fn rewrite(
    archive: &Path,
    keep: &dyn Fn(&str) -> Option<String>,
    add: &[(String, New)],
    password: Option<&str>,
    hide_names: bool,
) -> io::Result<()> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    if matches!(k, Kind::Disk | Kind::Mbox) || in_parts(archive) || split_zip(archive) {
        return Err(crate::disk_image::read_only());
    }
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let _all = writing_lock()?;
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    // Passing names with the run's own name: apps that do not share the lock above (a Flatpak's
    // own cache folder, another user or machine on a shared folder) never write into each
    // other's. Noted in this run's list, so what a crash left goes at the next start.
    let me = crate::spill::run();
    crate::spill::staging_in(archive.parent().unwrap_or(Path::new(".")));
    let tmp = archive.with_file_name(format!(".{name}.{me}.coxswain-tmp"));
    let plain = archive.with_file_name(format!(".{name}-tar.{me}.coxswain-tmp"));
    let exists = archive.exists();
    let mut used = false;
    let written = (|| -> io::Result<()> {
        if k == Kind::Zip {
            let mut w = zip::ZipWriter::new(create_fresh(&tmp, exists)?);
            // New files go into a locked zip locked too, with its password; a wrong one, or
            // none, is refused, so the apps ask.
            let mut pw = if exists {
                None
            } else {
                password.map(String::from)
            };
            used = pw.is_some();
            if exists {
                let mut z = zip::ZipArchive::new(BufReader::new(Joined::open(archive)?))
                    .map_err(io::Error::other)?;
                let mut first_locked = None;
                for i in 0..z.len() {
                    crate::stop::check()?;
                    let e = z.by_index_raw(i).map_err(io::Error::other)?;
                    if e.encrypted() && !e.is_dir() {
                        first_locked.get_or_insert(i);
                    }
                    let full = e.name().map_err(io::Error::other)?;
                    let old = full.trim_end_matches('/').to_string();
                    let slash = full.ends_with('/');
                    // Locked the old way (ZipCrypto): the crate's raw copy drops that lock and
                    // leaves the bytes scrambled, so the file is read with the password and
                    // locked anew with AES-256 (as new files are). Without one: ask.
                    let old_lock = e.encrypted()
                        && !e.is_dir()
                        && zip::read::HasZipMetadata::get_metadata(&e)
                            .aes_settings()
                            .is_none();
                    match keep(&old) {
                        Some(new) if old_lock => {
                            drop(e);
                            let given = password_for(archive, password).ok_or_else(locked)?;
                            let mut from =
                                z.by_index_decrypt(i, given.as_bytes()).map_err(|e| {
                                    if matches!(e, zip::result::ZipError::InvalidPassword) {
                                        locked()
                                    } else {
                                        io::Error::other(e)
                                    }
                                })?;
                            let opts = zip::write::SimpleFileOptions::default()
                                .compression_method(zip::CompressionMethod::Deflated)
                                .last_modified_time(from.last_modified().unwrap_or_default())
                                .with_aes_encryption(zip::AesMode::Aes256, &given);
                            #[cfg(unix)]
                            let opts = match from.unix_mode() {
                                Some(m) => opts.unix_permissions(m),
                                None => opts,
                            };
                            w.start_file(new, opts).map_err(io::Error::other)?;
                            // A wrong password the check byte let through fails the checksum here.
                            io::copy(&mut from, &mut w).map_err(|_| locked())?;
                            used = true;
                            Ok(())
                        }
                        Some(new) if new == old => w.raw_copy_file(e).map_err(io::Error::other),
                        Some(new) => w
                            .raw_copy_file_rename(e, if slash { format!("{new}/") } else { new })
                            .map_err(io::Error::other),
                        None => Ok(()),
                    }?;
                }
                if let (Some(i), true) = (
                    first_locked,
                    add.iter().any(|(_, n)| matches!(n, New::File(_))),
                ) {
                    let given = password_for(archive, password).ok_or_else(locked)?;
                    let mut e = z.by_index_decrypt(i, given.as_bytes()).map_err(|e| {
                        if matches!(e, zip::result::ZipError::InvalidPassword) {
                            locked()
                        } else {
                            io::Error::other(e)
                        }
                    })?;
                    // ZipCrypto's own check lets about one wrong password in 256 through: the
                    // file is read to its end, where its checksum tells.
                    io::copy(&mut e, &mut io::sink()).map_err(|_| locked())?;
                    pw = Some(given);
                }
            }
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, new) in add {
                match new {
                    // Dated now, as the tar's are: a zip's default is 1980.
                    New::Dir => w
                        .add_directory(format!("{name}/"), opts.last_modified_time(zip_time(now_secs())))
                        .map_err(io::Error::other)?,
                    New::File(path) => {
                        let opts = opts.last_modified_time(zip_time(modified(path)));
                        let opts = match &pw {
                            Some(pw) => opts.with_aes_encryption(zip::AesMode::Aes256, pw),
                            None => opts,
                        };
                        #[cfg(unix)]
                        let opts = opts.unix_permissions(std::os::unix::fs::PermissionsExt::mode(
                            &std::fs::metadata(path)?.permissions(),
                        ));
                        w.start_file(name.as_str(), opts)
                            .map_err(io::Error::other)?;
                        io::copy(&mut crate::stop::Reader(File::open(path)?), &mut w)?;
                    }
                }
            }
            w.finish().map_err(io::Error::other)?;
        } else if k == Kind::SevenZ {
            // 7z has no copying of entries as they are: each is unpacked and packed again on
            // its way from the old file to the new, as it was but for its name.
            let mut w =
                sevenz_rust2::ArchiveWriter::new(create_fresh(&tmp, exists)?).map_err(io::Error::other)?;
            if exists {
                // Names that were locked (the header encrypted) stay locked; open ones stay open.
                let names_locked = seven(archive, None).is_err_and(|e| e.to_string() == LOCKED);
                let mut r = seven(archive, password_for(archive, password).as_deref())?;
                let locked = seven_locked(r.archive());
                // A locked archive stays locked, with the password that opened it.
                if locked {
                    let pw = password_for(archive, password).unwrap_or_default();
                    w.set_content_methods(seven_locking(&pw));
                    w.set_encrypt_header(names_locked);
                }
                let mut failed = None;
                r.for_each_entries(|e, from| {
                    let old = e.name.replace('\\', "/").trim_end_matches('/').to_string();
                    // Read past, not skipped: in a solid 7z the next entry is in the same stream.
                    let Some(new) = keep(&old) else {
                        return io::copy(from, &mut io::sink())
                            .map(|_| true)
                            .map_err(Into::into);
                    };
                    let mut entry = e.clone();
                    entry.name = new;
                    let mut from = Noting {
                        from,
                        left: e.size,
                        failed: false,
                    };
                    let pushed = if e.is_directory {
                        w.push_archive_entry::<File>(entry, None)
                    } else {
                        w.push_archive_entry(entry, Some(crate::stop::Reader(&mut from)))
                    };
                    if let Err(err) = pushed {
                        failed = Some(copy_error(io::Error::other(err), &from, locked));
                        return Ok(false);
                    }
                    Ok(true)
                })
                .map_err(seven_error)?;
                if let Some(err) = failed {
                    return Err(err);
                }
                used = locked;
            } else if let Some(pw) = password {
                w.set_content_methods(seven_locking(pw));
                w.set_encrypt_header(hide_names);
                used = true;
            }
            for (name, new) in add {
                match new {
                    New::Dir => w.push_archive_entry::<File>(
                        {
                            let mut d = sevenz_rust2::ArchiveEntry::new_directory(name);
                            if let Ok(t) = sevenz_rust2::NtTime::try_from(std::time::SystemTime::now()) {
                                (d.last_modified_date, d.has_last_modified_date) = (t, true);
                            }
                            d
                        },
                        None,
                    ),
                    New::File(path) => w.push_archive_entry(
                        sevenz_rust2::ArchiveEntry::from_path(path, name.clone()),
                        Some(crate::stop::Reader(File::open(path)?)),
                    ),
                }
                .map_err(io::Error::other)?;
            }
            w.finish().map(drop).map_err(io::Error::other)?;
        } else {
            let Kind::Tar(pack) = k else { unreachable!() };
            // Written plain first, then compressed: every compression the same way.
            let mut b = tar::Builder::new(io::BufWriter::new(create_fresh(&plain, true)?));
            if exists {
                // Names and link targets written whole: past 100 bytes they take a GNU long
                // name of their own, which `append` with the old header would drop.
                tar_named(tar_reader(archive, k)?.entries()?, |old, link, e| {
                    let Some(new) = keep(&old) else {
                        return Ok(true);
                    };
                    let mut header = e.header().clone();
                    let t = header.entry_type();
                    match link {
                        Some(target) if t.is_symlink() || t.is_hard_link() => {
                            b.append_link(&mut header, &new, target)?
                        }
                        _ => b.append_data(&mut header, &new, e)?,
                    }
                    Ok(true)
                })?;
            }
            for (name, new) in add {
                match new {
                    New::Dir => {
                        let mut h = tar::Header::new_gnu();
                        h.set_entry_type(tar::EntryType::Directory);
                        h.set_mode(0o755);
                        h.set_size(0);
                        h.set_mtime(now_secs());
                        b.append_data(&mut h, name, io::empty())?;
                    }
                    // As `append_path_with_name` would, read through Stop.
                    New::File(path) => {
                        let mut h = tar::Header::new_gnu();
                        h.set_metadata(&std::fs::metadata(path)?);
                        b.append_data(&mut h, name, crate::stop::Reader(File::open(path)?))?
                    }
                }
            }
            b.into_inner()?.flush()?;
            let done = compress(&plain, &tmp, pack, exists);
            let _ = std::fs::remove_file(&plain);
            done?;
        }
        Ok(())
    })();
    let done = match written {
        // As private as the archive was.
        Ok(()) => {
            dated_after(&tmp, archive);
            std::fs::metadata(archive)
                .and_then(|m| std::fs::set_permissions(&tmp, m.permissions()))
                .or_else(|e| if exists { Err(e) } else { Ok(()) })
                .and_then(|_| std::fs::rename(&tmp, archive))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::remove_file(&plain);
            Err(e)
        }
    };
    uncache(archive);
    if let (Ok(()), true, Some(pw)) = (&done, used, password) {
        remember(archive, pw);
    }
    done
}

/// Take `inner` (files, or folders and everything in them) out of the archive: it is written
/// anew without them, the rest as it was (a zip file locked with AES stays locked and needs
/// no password; one locked the old way, or a locked 7z, needs its `password`, or the one
/// remembered).
pub fn remove(archive: &Path, inner: &[String], password: Option<&str>) -> io::Result<()> {
    if inner.iter().any(|i| nested(archive, i, false)) {
        return Err(nested_read_only());
    }
    let gone = |name: &str| {
        inner
            .iter()
            .any(|i| name == i || name.starts_with(&format!("{i}/")))
    };
    rewrite(
        archive,
        &|name| (!gone(name)).then(|| name.to_string()),
        &[],
        password,
        false,
    )
}

/// The entries each `source` (a file, or a folder and everything in it) becomes under its name.
fn entries_of(into: &[(String, PathBuf)]) -> io::Result<Vec<(String, New)>> {
    let mut out = vec![];
    fn walk(path: &Path, name: String, out: &mut Vec<(String, New)>) -> io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            out.push((name.clone(), New::Dir));
            let mut kids: Vec<_> = std::fs::read_dir(path)?.flatten().collect();
            kids.sort_by_key(|k| k.file_name());
            for k in kids {
                walk(
                    &k.path(),
                    format!("{name}/{}", k.file_name().to_string_lossy()),
                    out,
                )?;
            }
        } else if meta.is_file() {
            out.push((name, New::File(path.to_path_buf())));
        }
        // Symlinks are left out: what they point at may be anywhere.
        Ok(())
    }
    for (name, src) in into {
        walk(src, name.clone(), &mut out)?;
    }
    Ok(out)
}

/// The sources each under their own name.
fn named(sources: &[PathBuf]) -> io::Result<Vec<(String, PathBuf)>> {
    sources
        .iter()
        .map(|src| {
            let name = src
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to add"))?;
            Ok((name.to_string_lossy().into_owned(), src.clone()))
        })
        .collect()
}

/// Copy files from disk into `archive`, each source under the name given for it inside
/// (`docs/a.txt`). What is there by a name already is kept and the copy refused, as a copy
/// between folders never overwrites.
pub fn add(archive: &Path, into: &[(String, PathBuf)], password: Option<&str>) -> io::Result<()> {
    if into.iter().any(|(name, _)| nested(archive, name, false)) {
        return Err(nested_read_only());
    }
    let new = entries_of(into)?;
    let have: std::collections::HashSet<String> =
        items(archive)?.iter().map(|it| it.name.clone()).collect();
    if let Some((name, _)) = new
        .iter()
        .find(|(name, n)| !matches!(n, New::Dir) && have.contains(name))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{name} exists in {}", archive.display()),
        ));
    }
    // A folder the archive has already is not written twice.
    let new: Vec<_> = new
        .into_iter()
        .filter(|(name, n)| !(matches!(n, New::Dir) && have.contains(name)))
        .collect();
    rewrite(
        archive,
        &|name| Some(name.to_string()),
        &new,
        password,
        false,
    )
}

/// A new, empty folder inside the archive.
pub fn mkdir(archive: &Path, inner: &str, password: Option<&str>) -> io::Result<()> {
    if nested(archive, inner, false) {
        return Err(nested_read_only());
    }
    if items(archive)?.iter().any(|it| it.name == inner) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{inner} exists"),
        ));
    }
    rewrite(
        archive,
        &|name| Some(name.to_string()),
        &[(inner.to_string(), New::Dir)],
        password,
        false,
    )
}

/// Rename or move `from` (a file, or a folder and all in it) to `to`, inside the archive.
pub fn rename_in(archive: &Path, from: &str, to: &str, password: Option<&str>) -> io::Result<()> {
    if nested(archive, from, false) || nested(archive, to, false) {
        return Err(nested_read_only());
    }
    if to == from || to.starts_with(&format!("{from}/")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("err.move_into_itself"),
        ));
    }
    let names: Vec<String> = items(archive)?.iter().map(|it| it.name.clone()).collect();
    if names
        .iter()
        .any(|n| n == to || n.starts_with(&format!("{to}/")))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{to} exists"),
        ));
    }
    let prefix = format!("{from}/");
    rewrite(
        archive,
        &|name| {
            Some(if name == from {
                to.to_string()
            } else if let Some(rest) = name.strip_prefix(&prefix) {
                format!("{to}/{rest}")
            } else {
                name.to_string()
            })
        },
        &[],
        password,
        false,
    )
}

/// A new archive at `path` (its kind by its name: .zip, .7z, .tar, .tar.gz / .tgz, ...) with
/// `sources`.
pub fn create(path: &Path, sources: &[PathBuf]) -> io::Result<()> {
    create_locked(path, sources, None, false)
}

/// Whether an archive by this name can have a password: a zip or a 7z (a tar cannot).
pub fn takes_password(path: &Path) -> bool {
    matches!(kind(path), Some(Kind::Zip | Kind::SevenZ))
}

/// Every ending an archive is known by, longest first where one ends another, and those of
/// the archives that can have a password (`takes_password`): for the desktop app's page.
pub fn endings() -> (Vec<&'static str>, Vec<&'static str>) {
    let all = ENDINGS.iter().chain(OPEN_ONLY).map(|(e, _)| *e).collect();
    let locked = ENDINGS
        .iter()
        .chain(OPEN_ONLY)
        .filter(|(_, k)| matches!(k, Kind::Zip | Kind::SevenZ))
        .map(|(e, _)| *e)
        .collect();
    (all, locked)
}

/// A format Pack writes: its name, the endings that pick it (the one suggested first), and
/// whether it can be locked with a password. `id` names its hint (`archive.format_<id>`).
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct PackFormat {
    pub id: &'static str,
    pub label: &'static str,
    pub endings: &'static [&'static str],
    pub password: bool,
}

/// What Pack can write, in the order offered.
pub const PACK_FORMATS: &[PackFormat] = &[
    PackFormat {
        id: "zip",
        label: "Zip",
        endings: &[".zip"],
        password: true,
    },
    PackFormat {
        id: "7z",
        label: "7z",
        endings: &[".7z"],
        password: true,
    },
    PackFormat {
        id: "tar",
        label: "tar",
        endings: &[".tar"],
        password: false,
    },
    PackFormat {
        id: "tar_gz",
        label: "tar.gz",
        endings: &[".tar.gz", ".tgz"],
        password: false,
    },
    PackFormat {
        id: "tar_bz2",
        label: "tar.bz2",
        endings: &[".tar.bz2", ".tbz2", ".tbz"],
        password: false,
    },
    PackFormat {
        id: "tar_xz",
        label: "tar.xz",
        endings: &[".tar.xz", ".txz"],
        password: false,
    },
    PackFormat {
        id: "tar_zst",
        label: "tar.zst",
        endings: &[".tar.zst", ".tzst"],
        password: false,
    },
];

/// The pack format a name's ending picks, if any.
pub fn pack_format(name: &str) -> Option<&'static PackFormat> {
    let lower = name.to_ascii_lowercase();
    PACK_FORMATS
        .iter()
        .find(|f| f.endings.iter().any(|e| lower.ends_with(e)))
}

/// The format `dest`'s name picks, kept in `st` to be suggested next time, in both apps.
/// Says whether `st` changed, to save it.
pub fn remember_format(st: &mut crate::state::AppState, dest: &str) -> bool {
    match pack_format(dest) {
        Some(f) if st.pack_ending != f.endings[0] => {
            st.pack_ending = f.endings[0].into();
            true
        }
        _ => false,
    }
}

/// `name` with its archive ending (`.tar.gz` whole) swapped for `ending`, or `ending` added.
pub fn with_ending(name: &str, ending: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let cut = ENDINGS
        .iter()
        .map(|(e, _)| *e)
        .find(|e| lower.ends_with(e))
        .map_or(name.len(), |e| name.len() - e.len());
    format!("{}{ending}", &name[..cut])
}

/// `name` with the next pack format's ending (`back`: the previous one), round the list; a
/// name with no pack format's ending gets the first (`back`: the last).
pub fn cycle_ending(name: &str, back: bool) -> String {
    let n = PACK_FORMATS.len();
    let i = pack_format(name).and_then(|f| PACK_FORMATS.iter().position(|g| g == f));
    let next = match (i, back) {
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
        (None, false) => 0,
        (None, true) => n - 1,
    };
    with_ending(name, PACK_FORMATS[next].endings[0])
}

/// `create`, locked with `password` (AES-256: every zip entry, a 7z's contents, and with
/// `hide_names` a 7z's names too). The password is kept for this run, to look in without it.
pub fn create_locked(
    path: &Path,
    sources: &[PathBuf],
    password: Option<&str>,
    hide_names: bool,
) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", path.display()),
        ));
    }
    ending_kind(path).ok_or_else(|| not_archive(path))?;
    let password = password.filter(|p| !p.is_empty());
    if password.is_some() && !takes_password(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tar archives have no passwords",
        ));
    }
    rewrite(
        path,
        &|_| None,
        &entries_of(&named(sources)?)?,
        password,
        hide_names,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Held by every test that makes copies of archives inside archives: they share `NESTED`
    /// and its folder, which `clean_up` empties, so they run one at a time.
    fn nested_alone() -> std::sync::MutexGuard<'static, ()> {
        static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ONE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// FAT keeps a file's time in steps of two seconds, exFAT in steps of 10 ms: an archive
    /// written anew within one step of the old one is still dated after it, so no list of the
    /// archive takes it for unchanged. CI makes such disks on Windows and names them in
    /// `COXSWAIN_FAT_DIRS` (`;` between them there, `:` elsewhere).
    #[test]
    #[ignore = "needs COXSWAIN_FAT_DIRS: folders on FAT32 or exFAT disks"]
    fn on_a_fat_disk_a_rewrite_is_dated_after_the_old_archive() {
        let dirs = std::env::var_os("COXSWAIN_FAT_DIRS").expect("COXSWAIN_FAT_DIRS");
        let mut seen = 0;
        for dir in std::env::split_paths(&dirs).filter(|d| !d.as_os_str().is_empty()) {
            let (archive, tmp) = (dir.join("old.zip"), dir.join("new.zip"));
            std::fs::write(&archive, "old").unwrap();
            std::fs::write(&tmp, "new").unwrap();
            // The same time, as a write within one step gives.
            let t = std::fs::metadata(&archive).unwrap().modified().unwrap();
            File::options().write(true).open(&tmp).unwrap().set_modified(t).unwrap();
            dated_after(&tmp, &archive);
            let (old, new) = (std::fs::metadata(&archive).unwrap().modified().unwrap(), std::fs::metadata(&tmp).unwrap().modified().unwrap());
            assert!(new > old, "{}: {new:?} is not after {old:?}", dir.display());
            // And no further than the step needs: two seconds at most.
            assert!(new.duration_since(old).unwrap() <= std::time::Duration::from_millis(2001), "{}", dir.display());
            // A whole rewrite there too: the archive renamed into, dated after.
            let zip = dir.join("keep.zip");
            std::fs::write(dir.join("a.txt"), "a").unwrap();
            std::fs::write(dir.join("b.txt"), "b").unwrap();
            create(&zip, &[dir.join("a.txt"), dir.join("b.txt")]).unwrap();
            let before = std::fs::metadata(&zip).unwrap().modified().unwrap();
            remove(&zip, &["b.txt".to_string()], None).unwrap();
            let after = std::fs::metadata(&zip).unwrap().modified().unwrap();
            assert!(after > before, "{}: a rewrite kept its date", dir.display());
            for f in [archive, tmp, zip, dir.join("a.txt"), dir.join("b.txt")] {
                let _ = std::fs::remove_file(f);
            }
            seen += 1;
        }
        assert!(seen > 0, "no folder in COXSWAIN_FAT_DIRS");
    }

    #[test]
    fn a_mailbox_opens_like_a_folder_of_messages_read_only() {
        let d = crate::pictures::tests::folder("mbox");
        let mbox = d.join("Launch.mbox");
        let msg = |from: &str, date: &str, subject: &str, body: &str| {
            format!("From {from} Mon Sep 28 10:00:00 2026\nFrom: {from}\nDate: {date}\nSubject: {subject}\n\n{body}\n\n")
        };
        let text = [
            msg("ada@example.com", "Mon, 28 Sep 2026 10:00:00 +0000", "Go/no-go", "Go for launch."),
            msg("bob@example.com", "Tue, 29 Sep 2026 11:00:00 +0000", "Weather: fine", "Clear skies.\nFrom here it looks good."),
        ]
        .concat();
        std::fs::write(&mbox, &text).unwrap();
        assert!(opens(&mbox) && read_only(&mbox) && !is_archive(&mbox), "opens, but search reads it as mail");
        let (list, more) = list(&mbox, 100).unwrap();
        assert!(!more);
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["001 2026-09-28 - ada@example.com - Go_no-go.eml", "002 2026-09-29 - bob@example.com - Weather_ fine.eml"]);
        let copy = peek(&mbox.join(names[1])).unwrap();
        let got = std::fs::read_to_string(&copy).unwrap();
        assert!(got.starts_with("From: bob@example.com") && got.contains("From here it looks good."), "{got}");
        let m = crate::extract::mail::summary(&copy, u64::MAX).unwrap();
        assert_eq!(m.subject, "Weather: fine");
        // Out, all of it: one file a message.
        let out = extract(&mbox, &d).unwrap();
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2);
        assert!(add(&mbox, &[("x.txt".into(), copy.clone())], None).is_err(), "never written");
        assert!(split(&mbox.join(names[0])).is_some());
        let (all, _) = endings();
        assert!(all.contains(&".mbox") && all.contains(&".cbz"));
        // A long subject in a script of three bytes a letter, with a character that turns text
        // around: a name of at most 100 bytes in all but its number, and no such character.
        let long = crate::extract::mail::Listed { start: 0, len: 1, secs: 0, from: "\u{202e}gpj.exe".into(), subject: "発射".repeat(80) };
        // A message that cannot be written is left out, counted, and the others come.
        let to = d.join("some");
        std::fs::create_dir_all(to.join(names[0])).unwrap();
        let err = copy_out_with(&mbox, "", &to, None, Copy::default()).unwrap_err();
        assert!(err.to_string().contains('1'), "{err}");
        assert!(to.join(names[1]).is_file());
        let name = message_name(0, 3, &long);
        assert!(name.len() <= 3 + 1 + 100 + 4 && !name.contains('\u{202e}'), "{name}");
        assert!(is_comic(Path::new("a.CBZ")) && !is_comic(&mbox));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_zip_split_into_parts_opens_as_one_read_only() {
        let d = crate::pictures::tests::folder("zip-cut-parts");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // Bytes that do not pack, so the parts hold the entries' data too.
        let noise: Vec<u8> = (0..60_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        std::fs::write(src.join("noise.bin"), &noise).unwrap();
        std::fs::write(src.join("note.txt"), b"in parts").unwrap();
        let whole = d.join("whole.zip");
        create(&whole, &[src.join("noise.bin"), src.join("note.txt")]).unwrap();
        let bytes = std::fs::read(&whole).unwrap();
        for (i, part) in bytes.chunks(25_000).enumerate() {
            std::fs::write(d.join(format!("split.zip.{:03}", i + 1)), part).unwrap();
        }
        let first = d.join("split.zip.001");
        assert!(opens(&first) && read_only(&first));
        let names: Vec<String> = crate::fs::list(&first, true).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["..", "noise.bin", "note.txt"]);
        assert_eq!(std::fs::read(peek(&first.join("note.txt")).unwrap()).unwrap(), b"in parts");
        assert_eq!(std::fs::read(peek(&first.join("noise.bin")).unwrap()).unwrap(), noise);
        assert_eq!(crate::enter::enter(&first, false, crate::enter::Place::Disk), crate::enter::Enter::Archive);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_games_database_looked_at_inside_an_archive_brings_the_files_beside_it() {
        let _alone = nested_alone();
        let d = crate::pictures::tests::folder("peek-base");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        for n in ["Club.cbh", "Club.cbg", "Club.CBA", "club.si4", "club.sg4", "club.sn4", "other.cbg"] {
            std::fs::write(src.join(n), n).unwrap();
        }
        // On its own in a zip, and two levels down (zip in tar.gz in zip).
        let zip = d.join("games.zip");
        create(&zip, &[src.clone()]).unwrap();
        let mid = d.join("mid.tar.gz");
        create(&mid, std::slice::from_ref(&zip)).unwrap();
        let outer = d.join("outer.zip");
        create(&outer, std::slice::from_ref(&mid)).unwrap();
        for at in [zip.join("src"), outer.join("mid.tar.gz/games.zip/src")] {
            let cbh = peek(&at.join("Club.cbh")).unwrap();
            let beside = |n: &str| cbh.with_file_name(n);
            assert_eq!(std::fs::read(beside("Club.cbg")).unwrap(), b"Club.cbg", "{at:?}");
            assert!(beside("Club.CBA").is_file() && !beside("other.cbg").exists());
            let si4 = peek(&at.join("club.si4")).unwrap();
            assert!(si4.with_file_name("club.sg4").is_file() && si4.with_file_name("club.sn4").is_file());
            // Any other file comes alone.
            let one = peek(&at.join("other.cbg")).unwrap();
            assert_eq!(std::fs::read_dir(one.parent().unwrap()).unwrap().count(), 1);
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_archive_stored_as_it_is_inside_another_is_read_in_place_whatever_its_size() {
        let _alone = nested_alone();
        let d = crate::pictures::tests::folder("in-place");
        // A 7z over the copy limit (stored, its bytes as they are), with a note and a game.
        let src = d.join("src");
        std::fs::create_dir_all(src.join("games")).unwrap();
        let mut x = 9u64;
        let noise: Vec<u8> = (0..(NESTED_MAX as usize * 2)).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 }).collect();
        std::fs::write(src.join("games/noise.bin"), &noise).unwrap();
        std::fs::write(src.join("games/note.txt"), b"a game of go, black wins").unwrap();
        let seven = d.join("ranks.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&seven).unwrap();
        w.set_content_methods(vec![sevenz_rust2::EncoderMethod::COPY.into()]);
        w.push_source_path(&src, |_| true).unwrap();
        w.finish().unwrap();
        assert!(std::fs::metadata(&seven).unwrap().len() > NESTED_MAX);
        let zip_of = |to: &Path, method: zip::CompressionMethod| {
            let mut z = zip::ZipWriter::new(File::create(to).unwrap());
            z.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"the dataset").unwrap();
            z.start_file("ranks.7z", zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
            z.write_all(&std::fs::read(&seven).unwrap()).unwrap();
            z.finish().unwrap();
        };
        let (stored, packed) = (d.join("dataset.zip"), d.join("packed.zip"));
        zip_of(&stored, zip::CompressionMethod::Stored);
        zip_of(&packed, zip::CompressionMethod::Deflated);
        // Bytes written to the cache's copies of archives inside archives, all of them.
        let nested_bytes = || -> u64 {
            fn walk(p: &Path) -> u64 {
                std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| {
                    let m = e.metadata().unwrap();
                    if m.is_dir() { walk(&e.path()) } else { m.len() }
                }).sum()
            }
            crate::helper::folder().map_or(0, |f| walk(&f.join("nested")) + walk(&f.join("search-nested")))
        };
        let before = nested_bytes();
        // Browsed, previewed, copied out: from the zip's own bytes.
        let (listed, _) = listing(&stored, "ranks.7z/games").unwrap();
        let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "noise.bin", "note.txt"]);
        let note = stored.join("ranks.7z").join("games").join("note.txt");
        assert_eq!(std::fs::read(peek(&note).unwrap()).unwrap(), b"a game of go, black wins");
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        copy_out(&stored, "ranks.7z/games/noise.bin", &out, None, false).unwrap();
        assert!(std::fs::read(out.join("noise.bin")).unwrap() == noise);
        // Search knows its files, reading it in place too.
        let found = search_entries(&stored, std::fs::metadata(&stored).unwrap().len()).unwrap();
        assert!(found.iter().any(|e| e.name == "ranks.7z/games/note.txt"), "search lists it");
        let mut read = vec![];
        search_read(&stored, 0, &|n, _| n == "ranks.7z/games/note.txt", &|n| n == "ranks.7z", &mut |n, from| {
            let mut t = String::new();
            from.read_to_string(&mut t).unwrap();
            read.push((n[0].to_string(), t));
            true
        })
        .unwrap();
        assert_eq!(read, [("ranks.7z/games/note.txt".to_string(), "a game of go, black wins".to_string())]);
        assert_eq!(nested_bytes(), before, "nothing was copied into the cache");
        // Split into parts, each stored: its parts read in place, one after another.
        let bytes = std::fs::read(&seven).unwrap();
        let half = bytes.len() / 2;
        let split = d.join("split.zip");
        let mut z = zip::ZipWriter::new(File::create(&split).unwrap());
        for (n, part) in [("2d/2d.7z.001", &bytes[..half]), ("2d/2d.7z.002", &bytes[half..])] {
            z.start_file(n, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            z.write_all(part).unwrap();
        }
        z.finish().unwrap();
        let note = split.join("2d").join("2d.7z.001").join("games").join("note.txt");
        assert_eq!(std::fs::read(peek(&note).unwrap()).unwrap(), b"a game of go, black wins");
        assert_eq!(nested_bytes(), before, "nothing was copied into the cache for the parts either");
        // Packed, it is still copied, and over the limit refused as before.
        let err = listing(&packed, "ranks.7z").unwrap_err();
        assert!(err.to_string().contains("ranks.7z"), "{err}");
        clean_up();
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_archive_inside_an_archive_opens_from_a_copy_and_is_never_changed() {
        let _alone = nested_alone();
        let d = crate::pictures::tests::folder("nested");
        // inner.tar.gz (deep/note.txt) in mid.zip, in outer.zip beside top.txt.
        let src = d.join("src");
        std::fs::create_dir_all(src.join("deep")).unwrap();
        std::fs::write(src.join("deep/note.txt"), b"three levels down").unwrap();
        let inner = d.join("inner.tar.gz");
        create(&inner, &[src.join("deep")]).unwrap();
        let mid = d.join("mid.zip");
        create(&mid, std::slice::from_ref(&inner)).unwrap();
        std::fs::write(d.join("top.txt"), b"top").unwrap();
        let outer = d.join("outer.zip");
        create(&outer, &[mid.clone(), d.join("top.txt")]).unwrap();
        let names = |dir: &Path| -> Vec<String> {
            crate::fs::list(dir, true).unwrap().into_iter().map(|e| e.name).collect()
        };
        assert_eq!(names(&outer), ["..", "mid.zip", "top.txt"]);
        // Each level lists as a folder, its paths through all of them.
        let at_mid = outer.join("mid.zip");
        assert!(opens_inside(&at_mid) && is_folder(&outer, "mid.zip").unwrap());
        assert_eq!(names(&at_mid), ["..", "inner.tar.gz"]);
        let at_inner = at_mid.join("inner.tar.gz");
        let (listed, _) = listing(&outer, "mid.zip/inner.tar.gz/deep").unwrap();
        assert_eq!(listed[1].path, at_inner.join("deep").join("note.txt"));
        assert_eq!(listed[0].path, at_inner);
        // Looked at, copied out: from the copies in the cache, as deep as they go.
        let note = at_inner.join("deep/note.txt");
        assert_eq!(std::fs::read(peek(&note).unwrap()).unwrap(), b"three levels down");
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        copy_out(&outer, "mid.zip/inner.tar.gz/deep", &out, None, false).unwrap();
        assert_eq!(std::fs::read(out.join("deep/note.txt")).unwrap(), b"three levels down");
        // The inner archive itself is still a file to copy out.
        copy_out(&outer, "mid.zip", &out, None, false).unwrap();
        assert_eq!(std::fs::read(out.join("mid.zip")).unwrap(), std::fs::read(&mid).unwrap());
        // Nothing inside is changed, by the archive's calls or by the file operations.
        let before = std::fs::read(&outer).unwrap();
        assert!(nested(&outer, "mid.zip/inner.tar.gz", false) && !nested(&outer, "mid.zip", false));
        assert!(nested_dir(&at_inner) && nested_dir(&at_mid) && !nested_dir(&outer));
        let err = add(&outer, &[("mid.zip/x.txt".into(), d.join("top.txt"))], None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(remove(&outer, &["mid.zip/inner.tar.gz".into()], None).is_err());
        assert!(mkdir(&outer, "mid.zip/new", None).is_err());
        assert!(rename_in(&outer, "mid.zip/inner.tar.gz", "mid.zip/b.tar.gz", None).is_err());
        assert!(crate::fs::delete(&note).is_err() && crate::fs::copy(&d.join("top.txt"), &at_mid).is_err());
        assert_eq!(std::fs::read(&outer).unwrap(), before);
        // The archive inside itself can still go, as a file of the outer one.
        assert!(!nested(&outer, "top.txt", false));
        // A copy is made once while its archive does not change; the oldest go past the most kept.
        let copies = |n: &str| {
            NESTED.lock().unwrap().iter().filter(|k| k.copy.file_name().is_some_and(|f| f == n)).count()
        };
        let _ = listing(&outer, "mid.zip").unwrap();
        assert_eq!(copies("mid.zip"), 1);
        clean_up();
        assert_eq!(copies("mid.zip"), 0);
        assert_eq!(names(&at_mid), ["..", "inner.tar.gz"], "made again after clean_up");
        // Only so deep (in the same test: `clean_up` empties what all of them share).
        std::fs::write(d.join("a.txt"), b"a").unwrap();
        let mut last = d.join("a.txt");
        let mut path = String::new();
        for i in 0..=NESTED_DEPTH + 1 {
            let z = d.join(format!("l{i}.zip"));
            create(&z, std::slice::from_ref(&last)).unwrap();
            if i > 0 {
                path = format!("{}/{path}", last.file_name().unwrap().to_string_lossy());
            }
            last = z;
        }
        // The outermost holds NESTED_DEPTH + 1 archives inside each other.
        let path = path.trim_end_matches('/').to_string();
        let err = listing(&last, &path).unwrap_err();
        assert!(err.to_string().contains(&NESTED_DEPTH.to_string()), "{err}");
        let shallower = path.rsplit_once('/').unwrap().0;
        assert!(listing(&last, shallower).is_ok());
        assert!(NESTED.lock().unwrap().len() <= NESTED_KEPT, "the oldest copies went");
        // No copy is left whose archive went with an older one.
        assert!(NESTED.lock().unwrap().iter().all(|k| k.outer.is_file() && k.copy.is_file()));
        clean_up();
        // Too large: what it claims is refused before anything is unpacked, and all the copies
        // together stay within NESTED_ALL, the oldest going first.
        let noise = |n: usize, seed: u64| -> Vec<u8> {
            let mut x = seed | 1;
            (0..n).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 }).collect()
        };
        let pack = |name: &str, bytes: &[u8]| -> PathBuf {
            let f = d.join(name);
            std::fs::write(&f, bytes).unwrap();
            let z = d.join(format!("{name}.zip"));
            create(&z, std::slice::from_ref(&f)).unwrap();
            z
        };
        let big = pack("big.bin", &noise(NESTED_MAX as usize + 1, 3));
        let parts: Vec<PathBuf> =
            (0..4).map(|i| pack(&format!("part{i}.bin"), &noise(NESTED_MAX as usize * 9 / 10, i + 5))).collect();
        let many = d.join("many.zip");
        create(&many, &[&[big.clone()][..], &parts].concat()).unwrap();
        let err = listing(&many, "big.bin.zip").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge, "{err}");
        assert!(NESTED.lock().unwrap().is_empty(), "nothing unpacked for it");
        for i in 0..4 {
            listing(&many, &format!("part{i}.bin.zip")).unwrap();
            let kept = NESTED.lock().unwrap();
            assert!(kept.iter().map(|k| k.size).sum::<u64>() <= NESTED_ALL);
        }
        assert!(copies("part0.bin.zip") == 0 && copies("part3.bin.zip") == 1, "the oldest went");
        // Held while an operation goes through it: part0's copy stays, however many come.
        let (_, _, held) = resolve(&many, "part0.bin.zip/part0.bin", false).unwrap();
        for i in [1, 2, 3, 1, 2] {
            listing(&many, &format!("part{i}.bin.zip")).unwrap();
        }
        assert_eq!(copies("part0.bin.zip"), 1, "held, kept");
        drop(held);
        assert!(HELD.lock().unwrap().is_empty());
        clean_up();
        // Another file at the same path, the same size and time: never taken for the first.
        let same = |text: &[u8], to: &Path| {
            let f = d.join("same.txt");
            std::fs::write(&f, text).unwrap();
            let z = d.join("same.zip");
            let _ = std::fs::remove_file(&z);
            create(&z, std::slice::from_ref(&f)).unwrap();
            let o = d.join("o-new.zip");
            let _ = std::fs::remove_file(&o);
            create(&o, std::slice::from_ref(&z)).unwrap();
            std::fs::rename(&o, to).unwrap();
        };
        let twin = d.join("twin.zip");
        same(b"first", &twin);
        let when = std::fs::metadata(&twin).unwrap().modified().unwrap();
        let first = peek(&twin.join("same.zip/same.txt")).unwrap();
        assert_eq!(std::fs::read(first).unwrap(), b"first");
        same(b"other", &twin);
        File::options().write(true).open(&twin).unwrap().set_modified(when).unwrap();
        let again = peek(&twin.join("same.zip/same.txt")).unwrap();
        #[cfg(unix)]
        assert_eq!(std::fs::read(again).unwrap(), b"other");
        #[cfg(not(unix))]
        let _ = again;
        clean_up();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_zip_whose_files_share_their_bytes_is_not_unpacked_whole() {
        // Two names for one packed file: twice its size out of one copy of it (Fifield's zip
        // bomb takes thousands, a few kilobytes to gigabytes).
        let d = crate::pictures::tests::folder("overlap");
        let data = vec![0u8; 1 << 16];
        let packed = {
            let mut e = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::best());
            e.write_all(&data).unwrap();
            e.finish().unwrap()
        };
        let crc = {
            let mut h = flate2::Crc::new();
            h.update(&data);
            h.sum()
        };
        let mut z = vec![];
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        le32(&mut z, 0x0403_4b50);
        for x in [20, 0, 8, 0, 0] {
            le16(&mut z, x);
        }
        for x in [crc, packed.len() as u32, data.len() as u32] {
            le32(&mut z, x);
        }
        le16(&mut z, 1);
        le16(&mut z, 0);
        z.push(b'a');
        z.extend_from_slice(&packed);
        let dir = z.len() as u32;
        for name in [&b"a"[..], b"b", b"b/c", b"f/x"] {
            le32(&mut z, 0x0201_4b50);
            for x in [20, 20, 0, 8, 0, 0] {
                le16(&mut z, x);
            }
            for x in [crc, packed.len() as u32, data.len() as u32] {
                le32(&mut z, x);
            }
            for x in [name.len() as u16, 0, 0, 0, 0] {
                le16(&mut z, x);
            }
            le32(&mut z, 0);
            le32(&mut z, 0);
            z.extend_from_slice(name);
        }
        let dir_len = z.len() as u32 - dir;
        le32(&mut z, 0x0605_4b50);
        for x in [0, 0, 4, 4] {
            le16(&mut z, x);
        }
        le32(&mut z, dir_len);
        le32(&mut z, dir);
        le16(&mut z, 0);
        let zip = d.join("bomb.zip");
        std::fs::write(&zip, &z).unwrap();
        assert_eq!(list(&zip, 10).unwrap().0.len(), 4, "listed as it is");
        let err = extract(&zip, &d).unwrap_err();
        assert_eq!(err.to_string(), crate::t!("archive.overlapping"));
        assert!(!d.join("bomb").exists(), "nothing left behind");
        // One file of it comes out, and only that one: not the entries named as below it.
        let one = copy_out(&zip, "b", &d, None, false).unwrap();
        assert_eq!(std::fs::read(&one).unwrap(), data);
        std::fs::remove_file(one).unwrap();
        assert_eq!(copy_out(&zip, "b/c", &d, None, false).map(|p| p.is_file()).ok(), Some(true));
        // A folder of it does not.
        let err = copy_out(&zip, "f", &d, None, false).unwrap_err();
        assert_eq!(err.to_string(), crate::t!("archive.overlapping"));
        assert!(!d.join("f").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nothing_comes_out_writable_by_others() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::pictures::tests::folder("modes");
        let mut b = tar::Builder::new(vec![]);
        for (name, mode, kind) in [("d", 0o777, tar::EntryType::Directory), ("d/run.sh", 0o4777, tar::EntryType::Regular)] {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(kind);
            h.set_mode(mode);
            h.set_size(u64::from(kind == tar::EntryType::Regular));
            b.append_data(&mut h, name, &b"x"[..u64::from(kind == tar::EntryType::Regular) as usize]).unwrap();
        }
        std::fs::write(d.join("a.tar"), b.into_inner().unwrap()).unwrap();
        let zip = d.join("b.zip");
        let mut z = zip::ZipWriter::new(File::create(&zip).unwrap());
        z.start_file("run.sh", zip::write::SimpleFileOptions::default().unix_permissions(0o4777)).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        let t = extract(&d.join("a.tar"), &d).unwrap();
        assert_eq!((mode(t.join("d")), mode(t.join("d/run.sh"))), (0o755, 0o755));
        let z = extract(&zip, &d).unwrap();
        assert_eq!(mode(z.join("run.sh")), 0o755);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_entry_dated_past_any_clock_is_listed_looked_at_and_extracted() {
        let d = crate::pictures::tests::folder("far-future");
        let mut b = tar::Builder::new(vec![]);
        let mut h = tar::Header::new_gnu();
        h.set_mtime(u64::MAX);
        h.set_size(2);
        h.set_mode(0o644);
        b.append_data(&mut h, "later.txt", &b"hi"[..]).unwrap();
        let tar = d.join("later.tar");
        std::fs::write(&tar, b.into_inner().unwrap()).unwrap();
        let (list, _) = list(&tar, 10).unwrap();
        let _ = crate::i18n::date_time(list[0].modified);
        assert_eq!(std::fs::read(peek(&tar.join("later.txt")).unwrap()).unwrap(), b"hi");
        assert!(extract(&tar, &d).unwrap().join("later.txt").is_file());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_pack_format_is_remembered_once() {
        let mut st = crate::state::AppState::default();
        assert!(remember_format(&mut st, "/x/a.TAR.GZ"));
        assert_eq!(st.pack_ending, ".tar.gz");
        assert!(!remember_format(&mut st, "b.tar.gz"), "unchanged");
        assert!(!remember_format(&mut st, "c.txt"), "no format");
    }

    #[test]
    fn a_zstd_write_that_fails_comes_back_as_an_error() {
        struct Full(usize);
        impl Write for Full {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                if self.0 < buf.len() {
                    return Err(io::Error::from(io::ErrorKind::StorageFull));
                }
                self.0 -= buf.len();
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let data: Vec<u8> = (0..1u32 << 20)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let err = zstd_into(&mut Keep::new(&data[..]), &mut Keep::new(Full(1000))).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::StorageFull);
        let mut out = Vec::new();
        zstd_into(&mut Keep::new(&data[..]), &mut Keep::new(&mut out)).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn pack_endings_swap_and_cycle() {
        assert_eq!(with_ending("/a/b.tar.gz", ".7z"), "/a/b.7z");
        assert_eq!(with_ending("b.TGZ", ".zip"), "b.zip");
        assert_eq!(with_ending("my.notes", ".zip"), "my.notes.zip");
        assert_eq!(with_ending("b.zip", ".tar.zst"), "b.tar.zst");
        assert_eq!(pack_format("X.TAR.BZ2").map(|f| f.id), Some("tar_bz2"));
        assert_eq!(pack_format("x.jar"), None);
        assert_eq!(cycle_ending("b.zip", false), "b.7z");
        assert_eq!(cycle_ending("b.zip", true), "b.tar.zst");
        assert_eq!(cycle_ending("b.tzst", false), "b.zip");
        assert_eq!(cycle_ending("b", false), "b.zip");
        // Every ending is one the archive code writes, the password as it allows.
        for f in PACK_FORMATS {
            for e in f.endings {
                let p = Path::new("x").with_extension(&e[1..]);
                assert!(is_archive(&p), "{e}");
                assert_eq!(takes_password(&p), f.password, "{e}");
            }
        }
        // The page's lists say what opens and takes_password say.
        let (all, locked) = endings();
        for e in &all {
            let p = Path::new("x").with_extension(&e[1..]);
            assert!(opens(&p), "{e}");
            assert_eq!(takes_password(&p), locked.contains(e), "{e}");
        }
        assert!(locked.iter().all(|e| all.contains(e)));
    }

    /// A 7z locked here has its key made as 7-Zip makes it: 2^19 rounds, not the crate's 2^8.
    #[test]
    fn a_locked_zip_entry_that_cannot_be_written_says_why_and_asks_no_password() {
        let d = std::env::temp_dir().join(format!("coxswain-test-zipwrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "secret").unwrap();
        let p = d.join("locked.zip");
        create_locked(&p, &[d.join("a.txt")], Some("hunter2"), false).unwrap();
        // The folder to write into is a file, so the write fails, not the password.
        std::fs::write(d.join("plain"), "").unwrap();
        let err = copy_out(
            &p,
            "a.txt",
            &d.join("plain").join("a.txt"),
            Some("hunter2"),
            false,
        )
        .unwrap_err();
        assert_ne!(err.to_string(), LOCKED);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_7z_key_is_made_as_7zip_makes_it() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7zkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "secret").unwrap();
        let p = d.join("locked.7z");
        create_locked(&p, &[d.join("a.txt")], Some("hunter2"), false).unwrap();
        let r = seven(&p, Some("hunter2")).unwrap();
        let aes = r
            .archive()
            .blocks
            .iter()
            .flat_map(|b| &b.coders)
            .find(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256)
            .expect("an AES coder");
        assert_eq!(aes.properties()[0] & 0x3F, 19);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Archives made to hurt: what they hold is never lost on a move, never written past a
    /// limit, never leads outside, and a huge long-name record is refused, not read into memory.
    /// A zip's time is local time without a zone, as every zip tool writes it: Coxswain reads
    /// and writes it in the system's zone, so a zip made elsewhere shows the time its maker
    /// saw, and one Coxswain makes shows the right time in other tools.
    #[test]
    fn archive_zip_times_are_local() {
        use chrono::{Datelike, TimeZone, Timelike};
        // A zip's time has two-second steps. The clock, not a file's time: a build from a
        // package (Nix) dates its sources at 1970, before the first time a zip can hold.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            & !1;
        let local = chrono::Local.timestamp_opt(now as i64, 0).unwrap();
        let t = zip_time(now);
        assert_eq!(
            (
                t.year() as i32,
                t.month() as u32,
                t.day() as u32,
                t.hour() as u32
            ),
            (local.year(), local.month(), local.day(), local.hour())
        );
        assert_eq!(unix(t), now, "and back");
    }

    #[test]
    fn archive_crafted_entries_are_harmless() {
        use zip::write::SimpleFileOptions;
        let d = std::env::temp_dir().join(format!("coxswain-test-crafted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        let tar_of = |path: &Path, f: &dyn Fn(&mut tar::Builder<File>)| {
            let mut t = tar::Builder::new(File::create(path).unwrap());
            f(&mut t);
            t.into_inner().unwrap();
        };
        let file = |t: &mut tar::Builder<File>, name: &str, text: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_size(text.len() as u64);
            h.set_mode(0o644);
            t.append_data(&mut h, name, text).unwrap();
        };

        // A name with `:` is a name on Linux and macOS: moved out with the rest.
        if !cfg!(windows) {
            let tp = d.join("logs.tar");
            tar_of(&tp, &|t| {
                file(t, "logs/10:30.log", b"at half past");
                file(t, "logs/a.log", b"a");
            });
            crate::fs::rename(&tp.join("logs"), &d.join("out")).unwrap();
            assert_eq!(
                std::fs::read_to_string(d.join("out/logs/10:30.log")).unwrap(),
                "at half past"
            );
        }
        // A link inside cannot come out: the move is refused, and the archive keeps it all.
        let tp = d.join("links.tar");
        tar_of(&tp, &|t| {
            file(t, "dir/a.txt", b"a");
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            t.append_link(&mut h, "dir/link", "a.txt").unwrap();
        });
        assert!(crate::fs::rename(&tp.join("dir"), &d.join("out")).is_err());
        assert!(!d.join("out/dir").exists(), "nothing half moved");
        assert_eq!(list(&tp, 10).unwrap().0.len(), 2, "the archive kept both");
        // Extracted, the link is left out (it could point anywhere) and the file comes.
        let got = extract(&tp, &d).unwrap();
        assert_eq!(std::fs::read(got.join("dir/a.txt")).unwrap(), b"a");
        assert!(std::fs::symlink_metadata(got.join("dir/link")).is_err());
        std::fs::remove_dir_all(got).unwrap();

        // A long name stays whole when the tar is written anew.
        let long = format!("deep/{}/file.txt", "x".repeat(150));
        let tp = d.join("long.tar");
        tar_of(&tp, &|t| {
            file(t, &long, b"long");
            file(t, "gone.txt", b"g");
        });
        remove(&tp, &["gone.txt".into()], None).unwrap();
        assert_eq!(
            list(&tp, 10)
                .unwrap()
                .0
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>(),
            std::slice::from_ref(&long)
        );

        // A long-name record far past any name is refused before it is read.
        let tp = d.join("bomb.tar");
        tar_of(&tp, &|t| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::GNULongName);
            h.set_size(TAR_META + 1);
            t.append_data(
                &mut h,
                "././@LongLink",
                &vec![b'a'; TAR_META as usize + 1][..],
            )
            .unwrap();
            file(t, "x", b"x");
        });
        assert!(list(&tp, 10).is_err());
        assert!(
            extract(&tp, &d.join("out")).is_err() && !d.join("out/bomb").exists(),
            "extracted neither"
        );

        // Names that lead outside are not in the listing; a link in a zip comes out as a file.
        let zp = d.join("odd.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        w.start_file("../evil.txt", SimpleFileOptions::default())
            .unwrap();
        w.start_file("ok.txt", SimpleFileOptions::default())
            .unwrap();
        io::Write::write_all(&mut w, b"fine").unwrap();
        w.start_file(
            "run.sh",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        w.add_symlink("link", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        w.finish().unwrap();
        let names: Vec<String> = list_in(&zp, "")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["..", "link", "ok.txt", "run.sh"]);
        let out = extract(&zp, &d.join("out")).unwrap();
        assert!(
            !std::fs::symlink_metadata(out.join("link"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "no link made"
        );
        assert!(!d.join("evil.txt").exists() && !d.join("out/evil.txt").exists());
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(out.join("run.sh")).unwrap().permissions()
            ) & 0o777,
            0o755
        );

        // What comes out is counted: past the limit it stops.
        let err = copy_out_with(
            &zp,
            "ok.txt",
            &d.join("out/ok.txt"),
            None,
            Copy {
                limit: 3,
                ..Copy::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert!(!d.join("out/ok.txt").exists());

        // Written anew, an archive stays as private as it was.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&zp, std::fs::Permissions::from_mode(0o600)).unwrap();
            remove(&zp, &["ok.txt".into()], None).unwrap();
            assert_eq!(
                std::fs::metadata(&zp).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_zip_and_tar_roundtrip() {
        let d = std::env::temp_dir().join(format!("coxswain-test-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        let zp = d.join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        w.add_directory("sub/", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.start_file("sub/a.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        io::Write::write_all(&mut w, b"hello").unwrap();
        w.finish().unwrap();
        let (entries, more) = list(&zp, 10).unwrap();
        assert!(!more);
        assert_eq!(
            entries[1],
            ArchiveEntry {
                name: "sub/a.txt".into(),
                size: 5,
                is_dir: false,
                modified: entries[1].modified
            }
        );
        assert!(entries[0].is_dir);
        assert_eq!(list(&zp, 1).unwrap(), (vec![entries[0].clone()], true));
        let out = extract(&zp, &d).unwrap();
        assert_eq!(out, d.join("pack"));
        assert_eq!(
            std::fs::read_to_string(out.join("sub/a.txt")).unwrap(),
            "hello"
        );
        assert!(
            extract(&zp, &d).is_err(),
            "never extracts over an existing folder"
        );

        let tp = d.join("pack2.tar.gz");
        let gz =
            flate2::write::GzEncoder::new(File::create(&tp).unwrap(), flate2::Compression::fast());
        let mut t = tar::Builder::new(gz);
        t.append_path_with_name(d.join("pack/sub/a.txt"), "x/a.txt")
            .unwrap();
        t.into_inner().unwrap().finish().unwrap();
        let listed = list(&tp, 10).unwrap().0;
        assert_eq!(
            listed,
            [ArchiveEntry {
                name: "x/a.txt".into(),
                size: 5,
                is_dir: false,
                modified: listed[0].modified
            }]
        );
        assert!(listed[0].modified > 0, "a tar's time comes along");
        assert_eq!(
            std::fs::read_to_string(extract(&tp, &d).unwrap().join("x/a.txt")).unwrap(),
            "hello"
        );

        assert!(list(&d.join("pack/sub/a.txt"), 10).is_err());

        // A look at a file inside: a copy by its own name; the next look replaces it.
        let seen = peek(&zp.join("sub/a.txt")).unwrap();
        assert_eq!(
            (
                seen.file_name().unwrap().to_str(),
                std::fs::read_to_string(&seen).unwrap().as_str()
            ),
            (Some("a.txt"), "hello")
        );
        let next = peek(&tp.join("x/a.txt")).unwrap();
        assert!(
            next.exists() && (next == seen || !seen.exists()),
            "one copy at a time"
        );
        let date = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        let first = date(&next);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(
            date(&peek(&tp.join("x/a.txt")).unwrap()),
            first,
            "a copy has the file's date, not the time it was made"
        );
        assert!(peek(&zp.join("sub")).is_err(), "a folder is not looked at");
        assert!(peek(&zp.join("nothing.txt")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A database's files beside it come along with a look at it, only those under its name in
    /// its folder, and nothing lands outside the folder of the copy whatever an entry is named.
    #[test]
    fn a_database_comes_with_its_files_and_no_name_escapes() {
        use zip::write::SimpleFileOptions;
        let d = std::env::temp_dir().join(format!("coxswain-test-peek-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zp = d.join("db.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        for name in [
            "x/evil.cbh",
            "x/evil.cbg",
            "x/EVIL.cbp",
            "x/other.cbg",
            "evil.cbt",
            "x/..\\..\\evil.cbg",
            "x/C:evil.cbg",
            "x/../evil.cbg",
            "x/evil.cbg/..",
            "x/..\\..\\evil.cbh",
            "x/C:evil.cbh",
        ] {
            w.start_file(name, SimpleFileOptions::default()).unwrap();
            io::Write::write_all(&mut w, name.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        let copy = peek(&zp.join("x/evil.cbh")).unwrap();
        let folder = copy.parent().unwrap();
        let mut got: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, ["EVIL.cbp", "evil.cbg", "evil.cbh"]);
        assert_eq!(std::fs::read(folder.join("evil.cbg")).unwrap(), b"x/evil.cbg");
        // Crafted names looked at themselves stay in the folder too (or are not listed).
        for name in ["x/..\\..\\evil.cbh", "x/C:evil.cbh"] {
            if let Ok(c) = peek(&zp.join(name)) {
                assert_eq!(c.parent(), Some(folder), "{name}");
            }
        }
        for up in [folder.parent().unwrap(), folder.parent().unwrap().parent().unwrap()] {
            let names: Vec<String> = std::fs::read_dir(up).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            assert!(!names.iter().any(|n| n.contains("evil")), "{names:?}");
        }
        assert!(one_name("a.cbg") && !one_name("../a.cbg") && !one_name("..") && !one_name("a/b"));
        assert_eq!(one_name("..\\a.cbg"), !cfg!(windows));
        assert_eq!(one_name("C:a.cbg"), !cfg!(windows));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_is_a_folder_you_can_copy_out_of_and_take_out_of() {
        use zip::write::SimpleFileOptions;
        let d = std::env::temp_dir().join(format!("coxswain-test-inside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        let zp = d.join("tools.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        let file =
            |w: &mut zip::ZipWriter<File>, name: &str, text: &[u8], opts: SimpleFileOptions| {
                w.start_file(name, opts).unwrap();
                io::Write::write_all(w, text).unwrap();
            };
        let plain = SimpleFileOptions::default();
        file(&mut w, "readme.txt", b"hello", plain);
        // Folders only named in the paths of their files.
        file(&mut w, "bin/run.sh", b"echo go", plain);
        file(&mut w, "bin/lib/core.so", b"\x7fELF", plain);
        file(
            &mut w,
            "keys/old.pem",
            b"secret one",
            plain.with_aes_encryption(zip::AesMode::Aes128, "hunter2"),
        );
        file(
            &mut w,
            "keys/new.pem",
            b"secret two",
            plain.with_aes_encryption(zip::AesMode::Aes256, "hunter2"),
        );
        w.finish().unwrap();

        // A path through the archive lists like a folder, `..` first.
        let names = |p: &Path| {
            crate::fs::list(p, false)
                .unwrap()
                .iter()
                .map(|e| (e.name.clone(), e.is_dir))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&zp),
            [
                ("..".into(), true),
                ("bin".into(), true),
                ("keys".into(), true),
                ("readme.txt".into(), false)
            ]
        );
        assert_eq!(
            names(&zp.join("bin")),
            [
                ("..".into(), true),
                ("lib".into(), true),
                ("run.sh".into(), false)
            ]
        );
        assert_eq!(
            crate::fs::list(&zp.join("bin"), false).unwrap()[0].path,
            zp,
            "`..` leads back to the archive's top"
        );
        assert_eq!(
            split(&zp.join("bin/lib")),
            Some((zp.clone(), "bin/lib".into()))
        );
        assert_eq!(split(&d.join("out")), None);

        // Copy out: a file, a folder with all in it.
        assert_eq!(
            std::fs::read_to_string(
                crate::fs::copy(&zp.join("readme.txt"), &d.join("out")).unwrap()
            )
            .unwrap(),
            "hello"
        );
        let bin = crate::fs::copy(&zp.join("bin"), &d.join("out")).unwrap();
        assert_eq!(std::fs::read(bin.join("lib/core.so")).unwrap(), b"\x7fELF");
        assert!(
            crate::fs::copy(&zp.join("readme.txt"), &d.join("out")).is_err(),
            "never over a file that is there"
        );

        // Locked files want the password (AES here; the zip crate writes no ZipCrypto, which reads the same way).
        assert!(locked_at(&zp, "keys").unwrap() && !locked_at(&zp, "bin").unwrap());
        let err = crate::fs::copy(&zp.join("keys"), &d.join("out")).unwrap_err();
        assert_eq!(err.to_string(), LOCKED);
        assert_eq!(
            crate::fs::copy_locked(&zp.join("keys/new.pem"), &d.join("out"), Some("wrong"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        let keys =
            crate::fs::copy_locked(&zp.join("keys"), &d.join("out"), Some("hunter2")).unwrap();
        assert_eq!(
            (
                std::fs::read(keys.join("old.pem")).unwrap(),
                std::fs::read(keys.join("new.pem")).unwrap()
            ),
            (b"secret one".to_vec(), b"secret two".to_vec())
        );

        // Taken out of the archive: the rest stays as it was, locked files still locked.
        crate::fs::delete(&zp.join("bin")).unwrap();
        assert_eq!(
            names(&zp),
            [
                ("..".into(), true),
                ("keys".into(), true),
                ("readme.txt".into(), false)
            ]
        );
        std::fs::remove_dir_all(d.join("out/keys")).unwrap();
        // The password that opened it was kept for the run; forgotten, it is locked again.
        assert!(crate::fs::copy(&zp.join("keys/old.pem"), &d.join("out")).is_ok());
        std::fs::remove_file(d.join("out/old.pem")).unwrap();
        forget(&zp);
        assert_eq!(
            crate::fs::copy(&zp.join("keys/old.pem"), &d.join("out"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        // A new file goes into a locked zip locked too, with the archive's password: without
        // one, or with a wrong one, it is refused.
        std::fs::write(d.join("out/note.txt"), "new secret").unwrap();
        assert_eq!(
            crate::fs::copy(&d.join("out/note.txt"), &zp.join("keys"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert_eq!(
            crate::fs::copy_locked(&d.join("out/note.txt"), &zp.join("keys"), Some("wrong"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        crate::fs::copy_locked(&d.join("out/note.txt"), &zp.join("keys"), Some("hunter2")).unwrap();
        forget(&zp);
        assert!(
            locked_at(&zp, "keys/note.txt").unwrap(),
            "the new file is locked"
        );
        assert_eq!(
            crate::fs::copy(&zp.join("keys/note.txt"), &d)
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert_eq!(
            std::fs::read_to_string(
                crate::fs::copy_locked(&zp.join("keys/note.txt"), &d, Some("hunter2")).unwrap()
            )
            .unwrap(),
            "new secret"
        );
        forget(&zp);
        // Moved out: copied, then gone from the archive.
        std::fs::remove_file(d.join("out/readme.txt")).unwrap();
        crate::fs::rename(&zp.join("readme.txt"), &d.join("out")).unwrap();
        assert!(d.join("out/readme.txt").is_file());
        assert_eq!(names(&zp).len(), 2);

        // A tar.gz: the same, written anew without what was taken out.
        let tp = d.join("pack.tar.gz");
        let mut t = tar::Builder::new(flate2::write::GzEncoder::new(
            File::create(&tp).unwrap(),
            flate2::Compression::fast(),
        ));
        t.append_path_with_name(d.join("out/readme.txt"), "a/one.txt")
            .unwrap();
        t.append_path_with_name(d.join("out/readme.txt"), "a/two.txt")
            .unwrap();
        t.into_inner().unwrap().finish().unwrap();
        crate::fs::delete(&tp.join("a/one.txt")).unwrap();
        assert_eq!(
            names(&tp.join("a")),
            [("..".into(), true), ("two.txt".into(), false)]
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_archive_written_anew_never_keeps_its_size_and_time() {
        // Every list kept of an archive (this run's, the compressed tars' index between runs,
        // another app's) is told apart by its size and modified time: a rewrite that keeps both
        // (a tar renamed within the clock's tick) would show the old contents.
        let d = std::env::temp_dir().join(format!("coxswain-test-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a1"), "one").unwrap();
        for pack in ["s.tar", "s.tar.gz"] {
            let zp = d.join(pack);
            create(&zp, &[d.join("a1")]).unwrap();
            let mut name = "a1";
            for _ in 0..30 {
                let before = stamp(&zp).unwrap();
                let to = if name == "a1" { "a2" } else { "a1" };
                rename_in(&zp, name, to, None).unwrap();
                name = to;
                assert_ne!(stamp(&zp).unwrap(), before, "{pack}");
                let listed: Vec<_> = items(&zp).unwrap().iter().map(|i| i.name.clone()).collect();
                assert_eq!(listed, [name], "{pack}");
            }
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn archive_takes_files_in_renames_and_packs() {
        let d = std::env::temp_dir().join(format!("coxswain-test-crud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/a.txt"), "one").unwrap();
        std::fs::write(d.join("src/deep/b.txt"), "two").unwrap();
        let names = |p: &Path| {
            crate::fs::list(p, true)
                .unwrap()
                .into_iter()
                .skip(1)
                .map(|e| e.name)
                .collect::<Vec<_>>()
        };
        for pack in [
            "new.zip",
            "new.tar.gz",
            "new.tar",
            "new.tar.bz2",
            "new.tar.xz",
            "new.tar.zst",
            "new.7z",
        ] {
            let zp = d.join(pack);
            // Pack, then add, a folder inside, rename, move between archives.
            create(&zp, &[d.join("src/a.txt")]).unwrap();
            assert!(
                create(&zp, &[d.join("src/a.txt")]).is_err(),
                "never over an archive that is there"
            );
            crate::fs::copy(&d.join("src/deep"), &zp).unwrap();
            assert_eq!(names(&zp), ["a.txt", "deep"], "{pack}");
            assert_eq!(names(&zp.join("deep")), ["b.txt"]);
            assert!(
                crate::fs::copy(&d.join("src/a.txt"), &zp).is_err(),
                "never over a file inside either"
            );
            crate::fs::mkdir(&zp.join("docs")).unwrap();
            crate::fs::rename(&zp.join("a.txt"), &zp.join("docs")).unwrap();
            assert!(
                rename_in(&zp, "docs", "docs/inner", None).is_err(),
                "not into itself"
            );
            assert!(rename_in(&zp, "docs", "docs", None).is_err());
            assert_eq!(names(&zp.join("docs")), ["a.txt"], "{pack}");
            // Copied in under a new name: not one folder too deep (docs/b.txt/b.txt).
            assert_eq!(
                crate::fs::copy(&d.join("src/deep/b.txt"), &zp.join("docs/c.txt")).unwrap(),
                zp.join("docs/c.txt")
            );
            assert_eq!(names(&zp.join("docs")), ["a.txt", "c.txt"], "{pack}");
            assert_eq!(
                std::fs::read_to_string(crate::fs::copy(&zp.join("docs/c.txt"), &d).unwrap())
                    .unwrap(),
                "two"
            );
            std::fs::remove_file(d.join("c.txt")).unwrap();
            crate::fs::delete(&zp.join("docs/c.txt")).unwrap();
            // A folder's size is that of the files in it, and the dates of what was added stay.
            let docs = crate::fs::list(&zp, true)
                .unwrap()
                .into_iter()
                .find(|e| e.name == "docs")
                .unwrap();
            assert_eq!((docs.is_dir, docs.size), (true, 3), "{pack}");
            let a = crate::fs::list(&zp.join("docs"), true)
                .unwrap()
                .into_iter()
                .find(|e| e.name == "a.txt")
                .unwrap();
            assert_eq!(
                a.modified / 60,
                modified(&d.join("src/a.txt")) / 60,
                "{pack}: modified kept through the rewrites"
            );
            crate::fs::rename(&zp.join("deep"), &zp.join("renamed")).unwrap();
            assert_eq!(names(&zp), ["docs", "renamed"]);
            let other = d.join(format!("other-{pack}"));
            create(&other, &[d.join("src/a.txt")]).unwrap();
            crate::fs::copy(&zp.join("renamed"), &other).unwrap();
            assert_eq!(
                names(&other.join("renamed")),
                ["b.txt"],
                "from one archive into another"
            );
            let back =
                crate::fs::copy(&other.join("renamed/b.txt"), &d.join("src/deep")).unwrap_err();
            assert_eq!(back.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(
                std::fs::read_to_string(crate::fs::copy(&other.join("renamed/b.txt"), &d).unwrap())
                    .unwrap(),
                "two"
            );
            std::fs::remove_file(d.join("b.txt")).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_search_sees_only_names_that_are_paths_and_never_locked_text() {
        let d = std::env::temp_dir().join(format!(
            "coxswain-test-archive-search-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zp = d.join("odd.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        let deep = vec!["a"; 65].join("/");
        for name in [
            "../evil.txt",
            "/abs.txt",
            "ok/./x.txt",
            "fine/name.txt",
            deep.as_str(),
        ] {
            w.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            io::Write::write_all(&mut w, b"words").unwrap();
        }
        w.start_file(
            "locked.txt",
            zip::write::SimpleFileOptions::default()
                .with_aes_encryption(zip::AesMode::Aes256, "pw"),
        )
        .unwrap();
        io::Write::write_all(&mut w, b"secret").unwrap();
        w.finish().unwrap();
        let size = std::fs::metadata(&zp).unwrap().len();
        let names: Vec<String> = search_entries(&zp, size)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["fine/name.txt", "locked.txt"]);
        let mut read = vec![];
        search_read(&zp, size, &|_, _| true, &|_| false, &mut |name, from| {
            let mut text = String::new();
            from.read_to_string(&mut text).unwrap();
            read.push((name[0].to_string(), text));
            true
        })
        .unwrap();
        assert_eq!(read, [("fine/name.txt".to_string(), "words".to_string())]);
        // A compressed tar too large to unpack for a listing is not looked into.
        assert!(search_entries(&d.join("big.tar.xz"), SEARCH_UNPACK + 1).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn search_reads_past_a_zip_entry_it_cannot_open() {
        let d = std::env::temp_dir().join(format!("coxswain-test-zipskip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zp = d.join("a.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        for name in ["odd.txt", "fine.txt"] {
            let o = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file(name, o).unwrap();
            io::Write::write_all(&mut w, b"words").unwrap();
        }
        w.finish().unwrap();
        // The first entry says it is packed by method 7, which nothing reads.
        let mut bytes = std::fs::read(&zp).unwrap();
        bytes[8] = 7;
        let central = memchr::memmem::find(&bytes, b"PK\x01\x02").unwrap();
        bytes[central + 10] = 7;
        std::fs::write(&zp, &bytes).unwrap();
        let mut read = vec![];
        search_read(&zp, bytes.len() as u64, &|_, _| true, &|_| false, &mut |name, _| {
            read.push(name[0].to_string());
            true
        })
        .unwrap();
        assert_eq!(read, ["fine.txt"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_packed_with_a_password_opens_only_with_it() {
        let d = std::env::temp_dir().join(format!("coxswain-test-pack-pw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/plan.txt"), "launch at noon").unwrap();
        std::fs::write(d.join("src/deep/crew.txt"), "four").unwrap();
        for (pack, hide) in [("pw.zip", false), ("pw.7z", true), ("open-names.7z", false)] {
            let zp = d.join(pack);
            create_locked(
                &zp,
                &[d.join("src/plan.txt"), d.join("src/deep")],
                Some("hunter2"),
                hide,
            )
            .unwrap();
            // Kept for this run: it opens without asking.
            let out = d.join(format!("out-{pack}"));
            std::fs::create_dir_all(&out).unwrap();
            assert_eq!(
                std::fs::read_to_string(crate::fs::copy(&zp.join("plan.txt"), &out).unwrap())
                    .unwrap(),
                "launch at noon",
                "{pack}"
            );
            forget(&zp);
            // Without it: locked, the names too only when hidden.
            assert_eq!(
                list_in(&zp, "").is_err_and(|e| e.to_string() == LOCKED),
                hide,
                "{pack}"
            );
            assert_eq!(
                extract(&zp, &out).unwrap_err().to_string(),
                LOCKED,
                "{pack}"
            );
            assert_eq!(
                extract_locked(&zp, &out, Some("wrong"))
                    .unwrap_err()
                    .to_string(),
                LOCKED,
                "{pack}"
            );
            let x = extract_locked(&zp, &out, Some("hunter2")).unwrap();
            assert_eq!(
                std::fs::read_to_string(x.join("deep/crew.txt")).unwrap(),
                "four",
                "{pack}"
            );
            if hide {
                let raw = std::fs::read(&zp).unwrap();
                assert!(
                    !raw.windows(8).any(|w| w == b"crew.txt")
                        && !raw.windows(16).any(|w| w
                            == "crew.txt"
                                .encode_utf16()
                                .flat_map(u16::to_le_bytes)
                                .collect::<Vec<_>>()),
                    "names hidden"
                );
            }
        }
        // A zip: every file AES-256.
        let mut z = zip::ZipArchive::new(File::open(d.join("pw.zip")).unwrap()).unwrap();
        for i in 0..z.len() {
            let e = z.by_index_raw(i).unwrap();
            assert!(e.is_dir() || e.encrypted(), "{:?}", e.name());
        }
        // Tar has no passwords; an empty one is none.
        assert!(
            create_locked(
                &d.join("pw.tar.gz"),
                &[d.join("src/plan.txt")],
                Some("x"),
                true
            )
            .is_err()
        );
        assert!(!d.join("pw.tar.gz").exists());
        create_locked(
            &d.join("open.zip"),
            &[d.join("src/plan.txt")],
            Some(""),
            true,
        )
        .unwrap();
        assert!(!locked_at(&d.join("open.zip"), "").unwrap());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A zip locked the old way (ZipCrypto, as 7-Zip's `-mem=ZipCrypto` makes it): its check
    /// byte lets about one wrong password in 256 through, so adding to it with such a password
    /// would lock the new file with the wrong one. The whole first file tells.
    #[test]
    fn archive_zipcrypto_add_refuses_a_wrong_password_the_check_byte_lets_through() {
        let d =
            std::env::temp_dir().join(format!("coxswain-test-zipcrypto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // `a.txt` holding "secret one", password "hunter2".
        let hex = "504b030414000100000032aa435d67dce7bc160000000a00000005000000612e747874a44b0bfe5b507702f57a5e41c4a571f7c3bbf68088e4504b01023f0314000100000032aa435d67dce7bc160000000a000000050024000000000000002080a48100000000612e7478740a00200000000000010018004e6675d86b53dd0100000000000000000000000000000000504b0506000000000100010057000000390000000000";
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let zp = d.join("old.zip");
        std::fs::write(&zp, bytes).unwrap();
        std::fs::write(d.join("b.txt"), "new").unwrap();
        let mut z = zip::ZipArchive::new(File::open(&zp).unwrap()).unwrap();
        let wrong = (0..100_000)
            .map(|i| format!("w{i}"))
            .find(|pw| z.by_index_decrypt(0, pw.as_bytes()).is_ok())
            .expect("a wrong password the check byte lets through");
        drop(z);
        std::fs::create_dir_all(d.join("out")).unwrap();
        assert_eq!(
            crate::fs::copy_locked(&zp.join("a.txt"), &d.join("out"), Some(&wrong))
                .unwrap_err()
                .to_string(),
            LOCKED,
            "a copy out notices by the checksum"
        );
        assert!(!d.join("out/a.txt").exists());
        assert_eq!(
            add(&zp, &[("b.txt".into(), d.join("b.txt"))], Some(&wrong))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert_eq!(list(&zp, 10).unwrap().0.len(), 1, "unchanged");
        // Any change to it needs the password: the crate's raw copy would leave the old lock's
        // bytes scrambled, so the file is locked anew with AES-256 and still reads.
        assert_eq!(
            crate::fs::mkdir(&zp.join("docs")).unwrap_err().to_string(),
            LOCKED
        );
        add(&zp, &[("b.txt".into(), d.join("b.txt"))], Some("hunter2")).unwrap();
        crate::fs::mkdir(&zp.join("docs")).unwrap();
        forget(&zp);
        let mut z = zip::ZipArchive::new(File::open(&zp).unwrap()).unwrap();
        let a = z.by_index_raw(0).unwrap();
        assert!(
            a.encrypted()
                && zip::read::HasZipMetadata::get_metadata(&a)
                    .aes_settings()
                    .is_some(),
            "locked anew with AES"
        );
        drop(a);
        drop(z);
        for (name, text) in [("a.txt", "secret one"), ("b.txt", "new")] {
            assert_eq!(
                crate::fs::copy_locked(&zp.join(name), &d.join("out"), Some("wrong"))
                    .unwrap_err()
                    .to_string(),
                LOCKED
            );
            assert_eq!(
                std::fs::read_to_string(
                    crate::fs::copy_locked(&zp.join(name), &d.join("out"), Some("hunter2"))
                        .unwrap()
                )
                .unwrap(),
                text
            );
            forget(&zp);
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Three processes (the desktop app, the terminal app, another window) adding to one
    /// archive at once: each waits for the others, so every change is in it.
    #[test]
    fn archive_writes_wait_for_other_processes() {
        if let Ok(job) = std::env::var("COXSWAIN_TEST_ADD") {
            let [zip, file, tag] = job.split('|').collect::<Vec<_>>()[..] else {
                panic!("{job}")
            };
            for i in 0..6 {
                add(
                    Path::new(zip),
                    &[(format!("{tag}-{i}.txt"), PathBuf::from(file))],
                    None,
                )
                .unwrap();
            }
            return;
        }
        let d = std::env::temp_dir().join(format!("coxswain-test-xlock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("x.txt"), "x".repeat(100_000)).unwrap();
        let zip = d.join("shared.zip");
        create(&zip, &[d.join("x.txt")]).unwrap();
        let kids: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|tag| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "archive::tests::archive_writes_wait_for_other_processes",
                        "--exact",
                        "--test-threads=1",
                    ])
                    .env(
                        "COXSWAIN_TEST_ADD",
                        format!("{}|{}|{tag}", zip.display(), d.join("x.txt").display()),
                    )
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut k in kids {
            assert!(k.wait().unwrap().success());
        }
        let (names, _) = list(&zip, 100).unwrap();
        assert_eq!(
            names.len(),
            1 + 3 * 6,
            "{:?}",
            names.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_7z_header_that_unpacks_too_big_is_refused() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7zbomb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // A packed header of 10 bytes that says it unpacks to 2 GB (LZMA, one block).
        let head = [
            0x17, 0x06, 0x00, 0x01, 0x09, 0x0A, 0x00, 0x07, 0x0B, 0x01, 0x00, 0x01, 0x03, 0x03,
            0x01, 0x01, 0x0C, 0xF0, 0, 0, 0, 0x80, 0x00, 0x00,
        ];
        let mut bomb = b"7z\xBC\xAF\x27\x1C\x00\x04".to_vec();
        bomb.extend([0; 4]);
        bomb.extend(10u64.to_le_bytes());
        bomb.extend((head.len() as u64).to_le_bytes());
        bomb.extend([0; 4]);
        bomb.extend([0x5D; 10]);
        bomb.extend(head);
        let p = d.join("bomb.7z");
        std::fs::write(&p, &bomb).unwrap();
        assert_eq!(
            seven_header_size(&mut File::open(&p).unwrap()).unwrap(),
            1 << 31
        );
        let err = list(&p, 10).unwrap_err().to_string();
        assert!(err.contains("2048") && err.contains("64 MB"), "{err}");
        // At the limit it is opened (and fails as the broken header it is); a byte past, refused.
        for (claim, refused) in [(SEVEN_HEADER_MAX, false), (SEVEN_HEADER_MAX + 1, true)] {
            let mut b = bomb.clone();
            let at = b.len() - 6;
            b[at..at + 4].copy_from_slice(&(claim as u32).to_le_bytes());
            let p = d.join("edge.7z");
            std::fs::write(&p, &b).unwrap();
            assert_eq!(seven_header_size(&mut File::open(&p).unwrap()).unwrap(), claim);
            let err = list(&p, 10).unwrap_err().to_string();
            assert_eq!(err.contains("64 MB"), refused, "{claim}: {err}");
        }
        assert!(peek(&p.join("a.txt")).is_err() && search_entries(&p, bomb.len() as u64).is_none());
        // Real ones, their headers packed, and locked with their names: read as before.
        for i in 0..200 {
            std::fs::write(d.join(format!("file-with-a-long-name-{i}.txt")), "x").unwrap();
        }
        let files: Vec<PathBuf> = (0..200)
            .map(|i| d.join(format!("file-with-a-long-name-{i}.txt")))
            .collect();
        for (name, pw) in [("plain.7z", None), ("hidden.7z", Some("hunter2"))] {
            let p = d.join(name);
            create_locked(&p, &files, pw, pw.is_some()).unwrap();
            let size = seven_header_size(&mut File::open(&p).unwrap()).unwrap();
            assert!(size > 0 && size < 1 << 20, "{name}: {size}");
            assert_eq!(seven(&p, pw).unwrap().archive().files.len(), 200, "{name}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_solid_7z_reads_every_entry() {
        // A solid 7z (as 7-Zip makes them): every entry is a window on one stream, so one not
        // read to its end must be read past, or the next fails its check.
        let d = std::env::temp_dir().join(format!("coxswain-test-solid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        let names = ["a.txt", "b.txt", "c.txt"];
        let zp = d.join("solid.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&zp).unwrap();
        let entries = names.iter().map(|n| {
            let mut e = sevenz_rust2::ArchiveEntry::new_file(n);
            e.size = 4000;
            e
        });
        let readers = names
            .iter()
            .map(|n| sevenz_rust2::SourceReader::new(std::io::Cursor::new(n.repeat(1000))))
            .collect();
        w.push_archive_entries(entries.collect(), readers).unwrap();
        w.finish().unwrap();
        assert_eq!(
            std::fs::read_to_string(peek(&zp.join("c.txt")).unwrap()).unwrap(),
            "c.txt".repeat(1000),
            "the last, past two not wanted"
        );
        let mut seen = vec![];
        search_read(&zp, 1, &|n, _| n != "a.txt", &|_| false, &mut |n, from| {
            let mut head = [0u8; 5];
            from.read_exact(&mut head).unwrap();
            seen.push((n[0].to_string(), head));
            true
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                ("b.txt".to_string(), *b"b.txt"),
                ("c.txt".to_string(), *b"c.txt")
            ],
            "read in part, then the next"
        );
        remove(&zp, &["a.txt".to_string()], None).unwrap();
        assert_eq!(
            list_in(&zp, "")
                .unwrap()
                .iter()
                .filter(|e| !e.is_parent())
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["b.txt", "c.txt"]
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A zip as Info-ZIP's `zip -s` splits it: the spanning mark, then the bytes cut at `cuts`
    /// into `x.z01` … and the last part `x.zip`, each entry's place and the list's counted from
    /// the start of its own part.
    fn span(whole: &[u8], cuts: &[usize], dir: &Path, stem: &str) -> PathBuf {
        let mut bytes = b"PK\x07\x08".to_vec();
        bytes.extend(whole);
        let starts: Vec<usize> = std::iter::once(0).chain(cuts.iter().map(|c| c + 4)).collect();
        let disk_of = |at: usize| starts.iter().rposition(|&s| s <= at).unwrap();
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
        let e = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
        let (count, cd) = (u16_at(&bytes, e + 10), u32_at(&bytes, e + 16) + 4);
        let mut i = cd;
        for _ in 0..count {
            let at = u32_at(&bytes, i + 42) + 4;
            let d = disk_of(at);
            bytes[i + 34..i + 36].copy_from_slice(&(d as u16).to_le_bytes());
            bytes[i + 42..i + 46].copy_from_slice(&((at - starts[d]) as u32).to_le_bytes());
            i += 46 + u16_at(&bytes, i + 28) + u16_at(&bytes, i + 30) + u16_at(&bytes, i + 32);
        }
        let last = starts.len() - 1;
        assert!(disk_of(cd) == last, "the list stays in the last part");
        bytes[e + 4..e + 6].copy_from_slice(&(last as u16).to_le_bytes());
        bytes[e + 6..e + 8].copy_from_slice(&(last as u16).to_le_bytes());
        bytes[e + 16..e + 20].copy_from_slice(&((cd - starts[last]) as u32).to_le_bytes());
        for (n, w) in starts.iter().enumerate() {
            let end = starts.get(n + 1).copied().unwrap_or(bytes.len());
            let name = if n == last { format!("{stem}.zip") } else { format!("{stem}.z{:02}", n + 1) };
            std::fs::write(dir.join(name), &bytes[*w..end]).unwrap();
        }
        dir.join(format!("{stem}.zip"))
    }

    #[test]
    fn a_zip_split_into_parts_opens_from_its_last_part_read_only() {
        let _alone = nested_alone();
        let d = crate::pictures::tests::folder("zip-parts");
        let text = |n: &str| format!("(;GM[1]SZ[19]C[{n}]{})", ";B[pd];W[dp]".repeat(300));
        let mut w = zip::ZipWriter::new(io::Cursor::new(vec![]));
        for n in ["a.sgf", "b.sgf", "c.sgf"] {
            w.start_file(n, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            io::Write::write_all(&mut w, text(n).as_bytes()).unwrap();
        }
        let whole = w.finish().unwrap().into_inner();
        // Cut inside the first file and inside the third: an entry across two parts.
        let one = text("a.sgf").len();
        let last = span(&whole, &[one / 2, one * 2 + one / 2], &d, "go");
        assert!(opens(&last) && read_only(&last));
        let listed: Vec<String> = crate::fs::list(&last, true).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(listed, ["..", "a.sgf", "b.sgf", "c.sgf"]);
        for n in ["a.sgf", "c.sgf"] {
            assert_eq!(std::fs::read_to_string(peek(&last.join(n)).unwrap()).unwrap(), text(n));
        }
        assert!(add(&last, &[("x.sgf".into(), d.join("go.z01"))], None).is_err());
        // The earlier parts say what they are; the games are counted.
        assert_eq!(zip_part_of("go.z02"), Some((2, "go.zip".into())));
        assert_eq!((zip_part_of("go.zip"), zip_part_of("go.z1"), zip_part_of("go.z00")), (None, None, None));
        let facts = crate::facts::of(&d.join("go.z01")).unwrap();
        assert!(facts[0].1.contains("go.zip") && facts[0].1.starts_with('1'), "{facts:?}");
        let games = crate::facts::of(&last).unwrap();
        assert!(games[0].1.starts_with("3 ") && games[0].1.contains(" 3 "), "{games:?}");
        // Inside another archive: copied out with its parts, then opened.
        let outer = d.join("outer.zip");
        let mut w = zip::ZipWriter::new(File::create(&outer).unwrap());
        for n in ["go.z01", "go.z02", "go.zip"] {
            w.start_file(format!("set/{n}"), zip::write::SimpleFileOptions::default()).unwrap();
            io::Write::write_all(&mut w, &std::fs::read(d.join(n)).unwrap()).unwrap();
        }
        w.finish().unwrap();
        let inner = outer.join("set").join("go.zip");
        let listed: Vec<String> = crate::fs::list(&inner, true).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(listed, ["..", "a.sgf", "b.sgf", "c.sgf"]);
        assert_eq!(std::fs::read_to_string(peek(&inner.join("b.sgf")).unwrap()).unwrap(), text("b.sgf"));
        // A part missing: named.
        std::fs::remove_file(d.join("go.z02")).unwrap();
        let err = list(&last, 10).unwrap_err().to_string();
        assert!(err.contains("go.z02"), "{err}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_7z_split_into_parts_opens_from_its_first_part_read_only() {
        let _alone = nested_alone();
        let d = crate::pictures::tests::folder("parts");
        let names = ["a.sgf", "b.sgf", "c.sgf"];
        let whole = d.join("whole.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&whole).unwrap();
        let text = |n: &str| format!("(;GM[1]SZ[19]C[{n}]{})", ";B[pd];W[dp]".repeat(400));
        let entries = names.iter().map(|n| {
            let mut e = sevenz_rust2::ArchiveEntry::new_file(n);
            e.size = text(n).len() as u64;
            e
        });
        let readers = names
            .iter()
            .map(|n| sevenz_rust2::SourceReader::new(std::io::Cursor::new(text(n).into_bytes())))
            .collect();
        w.push_archive_entries(entries.collect(), readers).unwrap();
        w.finish().unwrap();
        // Cut as 7-Zip cuts it: bytes in turn, the header at the end of the last part.
        let bytes = std::fs::read(&whole).unwrap();
        let cut = bytes.len().div_ceil(3);
        let parts = d.join("parts");
        std::fs::create_dir_all(&parts).unwrap();
        for (i, c) in bytes.chunks(cut).enumerate() {
            std::fs::write(parts.join(format!("go.7z.{:03}", i + 1)), c).unwrap();
        }
        let first = parts.join("go.7z.001");
        assert!(opens(&first) && is_archive(&first) && read_only(&first) && !read_only(&whole));
        assert!(!opens(&parts.join("go.7z.002")));
        let facts = crate::facts::of(&parts.join("go.7z.002")).unwrap();
        assert!(facts[0].1.contains("go.7z.001") && facts[0].1.starts_with('2'), "{facts:?}");
        assert_eq!(part_of("go.7z.003"), Some((3, "go.7z.001".into())));
        assert_eq!((part_of("go.7z.001"), part_of("go.7z"), part_of("x.zip.002")), (None, None, None));
        let listed = |dir: &Path| -> Vec<String> {
            crate::fs::list(dir, true).unwrap().into_iter().map(|e| e.name).collect()
        };
        assert_eq!(listed(&first), ["..", "a.sgf", "b.sgf", "c.sgf"]);
        assert_eq!(std::fs::read_to_string(peek(&first.join("c.sgf")).unwrap()).unwrap(), text("c.sgf"));
        let out = extract(&first, &d).unwrap();
        assert_eq!((out.file_name().unwrap(), std::fs::read_to_string(out.join("a.sgf")).unwrap()), ("go".as_ref(), text("a.sgf")));
        // Nothing is written into it.
        assert!(search_entries(&first, cut as u64).is_none() && search_entries(&whole, 1).is_some());
        assert!(add(&first, &[("x.sgf".into(), whole.clone())], None).is_err());
        assert!(remove(&first, &["a.sgf".into()], None).is_err());
        assert!(crate::fs::copy(&whole, &first).is_err());
        assert_eq!(std::fs::read(&first).unwrap(), bytes[..cut]);
        // Inside another archive, with all its parts copied out next to each other.
        let outer = d.join("outer.zip");
        create(&outer, std::slice::from_ref(&parts)).unwrap();
        let inside = outer.join("parts").join("go.7z.001");
        assert!(opens_inside(&inside));
        assert_eq!(listed(&inside), ["..", "a.sgf", "b.sgf", "c.sgf"]);
        // Its preview lists it, all its parts there; the copy held until the next such preview.
        let copy = peek(&inside).unwrap();
        assert!(HELD.lock().unwrap().contains(&copy), "held after peek returns");
        assert_eq!(list(&copy, 10).unwrap().0.len(), 3);
        let other = d.join("other.zip");
        create(&other, std::slice::from_ref(&parts)).unwrap();
        let next = peek(&other.join("parts").join("go.7z.001")).unwrap();
        let held = HELD.lock().unwrap().clone();
        assert!(held.contains(&next) && !held.contains(&copy), "the next preview's instead");
        PEEKED.lock().unwrap().take();
        assert_eq!(std::fs::read_to_string(peek(&inside.join("b.sgf")).unwrap()).unwrap(), text("b.sgf"));
        assert_eq!(missing_part(&first), None, "all its parts there");
        // A part missing: it fails, never reads past it, and names the part.
        std::fs::remove_file(parts.join("go.7z.002")).unwrap();
        uncache(&first);
        let err = crate::fs::list(&first, true).unwrap_err().to_string();
        assert!(err.contains("go.7z.002"), "{err}");
        assert!(missing_part(&first).is_some_and(|m| err.contains(&m)), "{err}");
        assert_eq!(missing_part(&parts.join("go.7z.003")), None, "only the first part says it");
        clean_up();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_locked_7z_wants_its_password() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7z-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        std::fs::write(d.join("plan.txt"), "launch at noon").unwrap();
        let zp = d.join("secret.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&zp).unwrap();
        w.set_content_methods(vec![
            sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(
                "hunter2",
            ))
            .into(),
            sevenz_rust2::EncoderMethod::LZMA2.into(),
        ]);
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::from_path(d.join("plan.txt"), "plan.txt".into()),
            Some(File::open(d.join("plan.txt")).unwrap()),
        )
        .unwrap();
        w.finish().unwrap();
        // Even its names are locked: looking in needs the password, which is kept for the run.
        assert_eq!(list_in(&zp, "").unwrap_err().to_string(), LOCKED);
        assert_eq!(
            crate::fs::copy(&zp.join("plan.txt"), &d.join("out"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        remember(&zp, "wrong");
        assert_eq!(list_in(&zp, "").unwrap_err().to_string(), LOCKED);
        remember(&zp, "hunter2");
        assert_eq!(list_in(&zp, "").unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(crate::fs::copy(&zp.join("plan.txt"), &d.join("out")).unwrap())
                .unwrap(),
            "launch at noon"
        );
        // Changed, it is written anew: still locked, with the same password.
        crate::fs::mkdir(&zp.join("docs")).unwrap();
        crate::fs::rename(&zp.join("plan.txt"), &zp.join("docs")).unwrap();
        forget(&zp);
        assert_eq!(
            list_in(&zp, "").unwrap_err().to_string(),
            LOCKED,
            "a changed archive is as locked as before"
        );
        remember(&zp, "hunter2");
        std::fs::create_dir_all(d.join("again")).unwrap();
        assert_eq!(
            std::fs::read_to_string(
                crate::fs::copy(&zp.join("docs/plan.txt"), &d.join("again")).unwrap()
            )
            .unwrap(),
            "launch at noon"
        );
        forget(&zp);
        assert!(list_in(&zp, "").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A wrong 7z key can end an entry's stream at once, and sevenz checks the checksum only
    /// at the end: an entry shorter than its size is a failed read (once flaky on FreeBSD CI).
    #[test]
    fn archive_7z_entry_that_ends_early_fails() {
        let mut empty: &[u8] = b"";
        let mut r = Noting {
            from: &mut empty,
            left: 14,
            failed: false,
        };
        assert_eq!(
            io::copy(&mut r, &mut io::sink()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(r.failed);
        let mut full: &[u8] = b"launch at noon";
        let mut r = Noting {
            from: &mut full,
            left: 14,
            failed: false,
        };
        assert_eq!(io::copy(&mut r, &mut io::sink()).unwrap(), 14);
        assert!(!r.failed);
    }

    #[test]
    fn archive_7z_with_locked_contents_and_open_names_changes_with_its_password() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7z-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        std::fs::write(d.join("plan.txt"), "launch at noon").unwrap();
        std::fs::write(d.join("crew.txt"), "four").unwrap();
        let zp = d.join("secret.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&zp).unwrap();
        w.set_encrypt_header(false);
        w.set_content_methods(vec![
            sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(
                "hunter2",
            ))
            .into(),
            sevenz_rust2::EncoderMethod::LZMA2.into(),
        ]);
        w.push_archive_entry(
            sevenz_rust2::ArchiveEntry::from_path(d.join("plan.txt"), "plan.txt".into()),
            Some(File::open(d.join("plan.txt")).unwrap()),
        )
        .unwrap();
        w.finish().unwrap();
        // The names are open: it lists, and says it is locked. Changing it needs the password.
        let (entries, locked) = listing(&zp, "").unwrap();
        assert!(locked && entries.len() == 2);
        assert_eq!(
            crate::fs::mkdir(&zp.join("docs")).unwrap_err().to_string(),
            LOCKED
        );
        assert_eq!(
            crate::fs::copy(&d.join("crew.txt"), &zp)
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert_eq!(
            crate::fs::delete(&zp.join("plan.txt"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert_eq!(
            crate::fs::mkdir_locked(&zp.join("docs"), Some("wrong"))
                .unwrap_err()
                .to_string(),
            LOCKED
        );
        assert!(
            list_in(&zp, "").unwrap().iter().all(|e| e.name != "docs"),
            "a failed change leaves the archive as it was"
        );
        // With it, the change is made, and it is kept: the next change needs no password.
        crate::fs::mkdir_locked(&zp.join("docs"), Some("hunter2")).unwrap();
        crate::fs::copy(&d.join("crew.txt"), &zp.join("docs")).unwrap();
        crate::fs::rename(&zp.join("plan.txt"), &zp.join("docs")).unwrap();
        let names = |p: &Path| {
            crate::fs::list(p, true)
                .unwrap()
                .into_iter()
                .skip(1)
                .map(|e| e.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&zp), ["docs"]);
        assert_eq!(names(&zp.join("docs")), ["crew.txt", "plan.txt"]);
        // Extraction reads it once, with the password; without one, the changed archive is as
        // locked as it was.
        forget(&zp);
        assert_eq!(
            extract(&zp, &d.join("out")).unwrap_err().to_string(),
            LOCKED
        );
        assert_eq!(
            list_in(&zp, "").unwrap().len(),
            2,
            "the names stay open, as they were"
        );
        let out = extract_locked(&zp, &d.join("out"), Some("hunter2")).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("docs/plan.txt")).unwrap(),
            "launch at noon"
        );
        assert_eq!(
            std::fs::read_to_string(out.join("docs/crew.txt")).unwrap(),
            "four"
        );
        crate::fs::delete(&zp.join("docs/crew.txt")).unwrap();
        assert_eq!(names(&zp.join("docs")), ["plan.txt"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A disk image is listed by 7-Zip once, whole, even for a preview of its first entries:
    /// Enter and F5 then use what was kept.
    #[test]
    fn a_disk_image_is_listed_once_and_kept() {
        if crate::disk_image::seven_zip().is_none() {
            return;
        }
        let d = crate::disk_image::tests::folder("kept");
        let image = d.join("demo.iso");
        std::fs::write(&image, crate::disk_image::tests::iso("DEMO", b"hi\n")).unwrap();
        crate::disk_image::FORCE.with(|f| f.set(true));
        let (first, more) = list(&image, 0).unwrap();
        assert!(first.is_empty() && more);
        assert!(cached(&image, stamp(&image).unwrap()).is_some(), "kept after a preview of none");
        crate::disk_image::FORCE.with(|f| f.set(false));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// What comes out of an archive keeps the date it has in it (zip, 7z, tar), and a folder
    /// made inside one is dated now, not 1980.
    #[test]
    fn dates_survive_extracting_and_new_folders_are_dated_now() {
        let d = crate::pictures::tests::folder("archive-dates");
        let src = d.join("old.txt");
        std::fs::write(&src, b"old").unwrap();
        let then = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        File::options().write(true).open(&src).unwrap().set_modified(then).unwrap();
        let now = now_secs();
        for ending in ["zip", "7z", "tar.gz"] {
            let a = d.join(format!("a.{ending}"));
            create(&a, std::slice::from_ref(&src)).unwrap();
            let out = extract(&a, &d.join(format!("out-{}", ending.replace('.', "-")))).unwrap();
            let got = std::fs::metadata(out.join("old.txt")).unwrap().modified().unwrap();
            let off = got.duration_since(then).unwrap_or_else(|e| e.duration());
            // A zip keeps local time to two seconds.
            assert!(off.as_secs() <= 2, "{ending}: {got:?}");
            let one = d.join(format!("one-{}.txt", ending.replace('.', "-")));
            copy_out(&a, "old.txt", &one, None, false).unwrap();
            let got = std::fs::metadata(&one).unwrap().modified().unwrap();
            assert!(got.duration_since(then).unwrap_or_else(|e| e.duration()).as_secs() <= 2, "{ending} F5");
            mkdir(&a, "new", None).unwrap();
            let (all, _) = list(&a, 100).unwrap();
            let new = all.iter().find(|e| e.name == "new").unwrap();
            assert!(new.is_dir && new.modified + 120 >= now, "{ending}: new folder dated {}", new.modified);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}


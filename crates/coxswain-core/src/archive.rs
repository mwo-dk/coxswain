//! Archives: zip (and the formats that are zips: jar, apk, whl, nupkg, vsix), tar, tar
//! compressed with gzip, bzip2, xz or zstd, and 7z. List, extract, and inside them read and
//! write as in a folder. All in pure Rust. Entries that would land outside the destination
//! (`..`, absolute paths) are refused.

use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};

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
}

/// How a tar is compressed.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pack {
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
    (".zip", Kind::Zip),
    (".jar", Kind::Zip),
    (".apk", Kind::Zip),
    (".nupkg", Kind::Zip),
    (".whl", Kind::Zip),
    (".vsix", Kind::Zip),
];

fn kind(path: &Path) -> Option<Kind> {
    // Asked of every file the name index walks: no allocation.
    let n = path.file_name()?.as_encoded_bytes();
    ENDINGS.iter().find(|(e, _)| n.len() >= e.len() && n[n.len() - e.len()..].eq_ignore_ascii_case(e.as_bytes())).map(|(_, k)| *k)
}

pub fn is_archive(path: &Path) -> bool {
    kind(path).is_some()
}

fn not_archive(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not an archive Coxswain reads", path.display()))
}

/// A tar, its compression undone as it is read.
fn tar_reader(path: &Path, k: Kind) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let Kind::Tar(pack) = k else { return Err(not_archive(path)) };
    let f = BufReader::new(File::open(path)?);
    let r: Box<dyn Read> = match pack {
        Pack::None => Box::new(f),
        Pack::Gz => Box::new(flate2::read::MultiGzDecoder::new(f)),
        Pack::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(f)),
        Pack::Xz => Box::new(lzma_rust2::XzReader::new(f, true)),
        Pack::Zst => Box::new(ruzstd::decoding::StreamingDecoder::new(f).map_err(io::Error::other)?),
    };
    Ok(tar::Archive::new(r))
}

/// The most bytes of a tar's long name or PAX record: the tar crate reads those whole into
/// memory, however large a crafted header says they are.
const TAR_META: u64 = 64 * 1024;

/// Each entry of `entries` with its whole name and link target (GNU long names and PAX paths,
/// read here with `TAR_META` at most), until `f` says enough. Those records are not entries.
fn tar_named<'a, R: Read>(entries: tar::Entries<'a, R>, mut f: impl FnMut(String, Option<String>, &mut tar::Entry<'a, R>) -> io::Result<bool>) -> io::Result<()> {
    let text = |b: &[u8]| String::from_utf8_lossy(b.strip_suffix(&[0]).unwrap_or(b)).into_owned();
    let (mut long, mut link): (Option<String>, Option<String>) = (None, None);
    for e in entries.raw(true) {
        let mut e = e?;
        let t = e.header().entry_type();
        if t.is_gnu_longname() || t.is_gnu_longlink() || t.is_pax_local_extensions() || t.is_pax_global_extensions() {
            if e.size() > TAR_META {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "a name in this tar is too long"));
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
        if !f(name.trim_start_matches("./").trim_end_matches('/').to_string(), link, &mut e)? {
            break;
        }
    }
    Ok(())
}

/// A new file at `path` for an archive being written, never one that is there: a link put
/// in its place (by someone else, in a shared folder) is removed, not written through.
fn create_fresh(path: &Path) -> io::Result<File> {
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// A tar written plain at `tar`, compressed into `to` as `pack` says.
fn compress(tar: &Path, to: &Path, pack: Pack) -> io::Result<()> {
    let (mut from, out) = (BufReader::new(File::open(tar)?), create_fresh(to)?);
    match pack {
        Pack::None => io::copy(&mut from, &mut io::BufWriter::new(out)).map(drop),
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
            let mut w = lzma_rust2::XzWriter::new(out, lzma_rust2::XzOptions::with_preset(6)).map_err(io::Error::other)?;
            io::copy(&mut from, &mut w)?;
            w.finish().map(drop).map_err(io::Error::other)
        }
        Pack::Zst => {
            ruzstd::encoding::compress(from, io::BufWriter::new(out), ruzstd::encoding::CompressionLevel::Fastest);
            Ok(())
        }
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
    PASSWORDS.lock().unwrap().iter().find(|(a, _)| a == archive).map(|(_, p)| p.clone()).or_else(|| given.map(String::from))
}

/// The entries of the archives last looked into, newest last, with their size and modified
/// time: browsing an archive lists it once per folder, and a compressed tar is read in full
/// each time. A few, so the preview of another archive does not push out the one browsed.
static LAST: std::sync::Mutex<Vec<(PathBuf, Stamp, Vec<Item>)>> = std::sync::Mutex::new(Vec::new());
const LAST_KEPT: usize = 4;

type Stamp = (u64, Option<std::time::SystemTime>);

fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = std::fs::metadata(path)?;
    Ok((m.len(), m.modified().ok()))
}

fn uncache(archive: &Path) {
    LAST.lock().unwrap().retain(|(p, ..)| p != archive);
}

/// Whether a 7z's contents are locked: any of its blocks is AES-encrypted.
fn seven_locked(a: &sevenz_rust2::Archive) -> bool {
    a.blocks.iter().any(|b| b.coders.iter().any(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256))
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
            Ok(0) if !buf.is_empty() && self.left > 0 => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the entry ends early")),
            r => r,
        };
        match &r {
            Ok(n) => self.left = self.left.saturating_sub(*n as u64),
            Err(_) => self.failed = true,
        }
        r
    }
}

/// The error of copying a 7z entry: `LOCKED` when a locked entry could not be read.
fn seven_copy_error(err: io::Error, noting: &Noting, locked: bool) -> io::Error {
    if locked && noting.failed { self::locked() } else { err }
}

/// A 7z, opened with `password` (none: an empty one).
fn seven(path: &Path, password: Option<&str>) -> io::Result<sevenz_rust2::ArchiveReader<File>> {
    let pw = password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::from);
    let mut file = File::open(path)?;
    let size = seven_header_size(&mut file)?;
    if size > SEVEN_HEADER_MAX {
        return Err(io::Error::other(crate::t!("archive.header_too_big", "mb" => size >> 20, "max" => SEVEN_HEADER_MAX >> 20)));
    }
    file.seek(io::SeekFrom::Start(0))?;
    sevenz_rust2::ArchiveReader::new(file, pw).map_err(seven_error)
}

/// The most a 7z's packed list of contents may unpack to: it is unpacked whole, into memory,
/// before anything is read, so a few bytes that unpack to gigabytes would take as much.
const SEVEN_HEADER_MAX: u64 = 64 << 20;

/// What a 7z's packed list of contents (its encoded header) says it unpacks to, the largest of
/// its streams; 0 when it is not packed. Read from the archive's own bytes, before opening it.
fn seven_header_size(file: &mut File) -> io::Result<u64> {
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
        *r = r.get(usize::try_from(n).map_err(|_| io::ErrorKind::UnexpectedEof)?..).ok_or(io::ErrorKind::UnexpectedEof)?;
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
                    let defined = if byte(r)? != 0 { n } else {
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
            let (i, o) = if bits & 0x10 != 0 { (number(r)?, number(r)?) } else { (1, 1) };
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
        sevenz_rust2::Error::PasswordRequired | sevenz_rust2::Error::MaybeBadPassword(_) => locked(),
        e => io::Error::other(e),
    }
}

/// The first `max` entries, and whether there were more.
pub fn list(path: &Path, max: usize) -> io::Result<(Vec<ArchiveEntry>, bool)> {
    // Not looked into yet: only as far as is shown (a big compressed tar is not unpacked to
    // its end for a preview's first rows).
    let cached = stamp(path).ok().and_then(|st| LAST.lock().unwrap().iter().find(|(p, s, _)| p == path && *s == st).map(|(.., items)| items.clone()));
    let all = match cached {
        Some(all) => all,
        None => read_items_with(path, password_for(path, None).as_deref(), max.saturating_add(1))?,
    };
    let more = all.len() > max;
    Ok((all.into_iter().take(max).map(|it| ArchiveEntry { name: it.name, size: it.size, is_dir: it.dir, modified: it.modified }).collect(), more))
}

/// Extract into a new folder named after the archive inside `dest_dir`. Returns that folder.
pub fn extract(path: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    extract_locked(path, dest_dir, None)
}

/// `extract`, with the password of a locked archive.
pub fn extract_locked(path: &Path, dest_dir: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    let k = kind(path).ok_or_else(|| not_archive(path))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_lowercase();
    let stem_len = ENDINGS.iter().map(|(e, _)| *e).find(|e| lower.ends_with(e)).map_or_else(|| name.rfind('.').unwrap_or(name.len()), |e| name.len() - e.len());
    let to = dest_dir.join(&name[..stem_len]);
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    std::fs::create_dir_all(&to)?;
    // A 7z or a zip is read once (a locked one with the password): no links made, nothing
    // sized by what the archive claims. A tar as the crate unpacks it, which keeps it inside;
    // its names are read first (from the last look, when it was browsed), since the crate
    // would read a crafted long-name record whole, however large: `TAR_META` at most.
    let r = if k == Kind::SevenZ || k == Kind::Zip {
        copy_out_with(path, "", &to, password_for(path, password).as_deref(), Copy::default()).map(drop)
    } else {
        items(path).and_then(|_| tar_reader(path, k)?.unpack(&to))
    };
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&to);
    }
    r.map(|_| to)
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
            return is_archive(a).then(|| {
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

/// Seconds since the Unix epoch of a zip's date and time, which is local time without a zone
/// (as zip tools write it); a time that does not exist (a clock change) takes the one after.
fn unix(t: zip::DateTime) -> u64 {
    use chrono::TimeZone;
    let local = chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32).and_then(|d| d.and_hms_opt(t.hour() as u32, t.minute() as u32, t.second() as u32));
    local.and_then(|l| chrono::Local.from_local_datetime(&l).earliest()).map_or(0, |d| d.timestamp().max(0) as u64)
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
fn items(archive: &Path) -> io::Result<Vec<Item>> {
    let stamp = stamp(archive)?;
    if let Some((_, _, items)) = LAST.lock().unwrap().iter().find(|(p, s, _)| p == archive && *s == stamp) {
        return Ok(items.clone());
    }
    let out = read_items(archive)?;
    let mut last = LAST.lock().unwrap();
    last.retain(|(p, ..)| p != archive);
    if last.len() >= LAST_KEPT {
        last.remove(0);
    }
    last.push((archive.to_path_buf(), stamp, out.clone()));
    Ok(out)
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
    if k == Kind::Zip {
        let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
        for i in 0..z.len().min(max) {
            let e = z.by_index_raw(i).map_err(io::Error::other)?;
            out.push(Item { name: e.name().trim_end_matches('/').to_string(), size: e.size(), dir: e.is_dir(), modified: e.last_modified().map_or(0, unix), locked: e.encrypted() });
        }
        return Ok(out);
    }
    if k == Kind::SevenZ {
        let r = seven(archive, password)?;
        let locked = seven_locked(r.archive());
        for f in r.archive().files.iter().take(max) {
            // Counted here: a time past what the system's clock holds must not panic.
            let modified = (u64::from(f.last_modified_date) / 10_000_000).saturating_sub(11_644_473_600);
            out.push(Item { name: f.name.replace('\\', "/").trim_end_matches('/').to_string(), size: f.size, dir: f.is_directory, modified, locked: locked && f.has_stream });
        }
        return Ok(out);
    }
    let mut item = |name: String, h: &tar::Header, size: u64| {
        out.push(Item { name, size, dir: h.entry_type().is_dir(), modified: h.mtime().unwrap_or(0), locked: false });
        Ok(out.len() < max)
    };
    if k == Kind::Tar(Pack::None) {
        // A plain tar's headers are read by seeking past what lies between them.
        tar_named(tar::Archive::new(BufReader::new(File::open(archive)?)).entries_with_seek()?, |n, _, e| item(n, e.header(), e.size()))?;
    } else {
        tar_named(tar_reader(archive, k)?.entries()?, |n, _, e| item(n, e.header(), e.size()))?;
    }
    Ok(out)
}

// ---------------------------------------------------------------- for search

/// The most entries of one archive that search knows.
pub(crate) const SEARCH_ENTRIES: usize = 50_000;
/// A compressed tar or a 7z is unpacked from its start to be read, a compressed tar even to be
/// listed: larger ones are found by their own name only.
pub(crate) const SEARCH_UNPACK: u64 = 256 << 20;
/// The most bytes read out of one archive for its files' text.
pub(crate) const SEARCH_READ: u64 = 128 << 20;

/// Whether getting at `archive`'s entries means unpacking all before them.
fn streamed(archive: &Path) -> bool {
    matches!(kind(archive), Some(Kind::Tar(p)) if p != Pack::None) || kind(archive) == Some(Kind::SevenZ)
}

/// A name inside an archive that can be a path through it: no `..`, nothing empty, and no
/// deeper than 64 folders (search builds its tree of them by recursion).
fn sound(name: &str) -> bool {
    name.split('/').count() <= 64 && name.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !(cfg!(windows) && p.contains([':', '\\'])))
}

/// What search may know of `archive` (`size` bytes) by name: its first `SEARCH_ENTRIES`
/// entries, as far as they are seen without a password, those whose names could not be a path
/// through it left out. `None` for one it does not look into: not an archive, too large to be
/// unpacked for a listing, unreadable, or only in the cloud (reading it would download it).
pub(crate) fn search_entries(archive: &Path, size: u64) -> Option<Vec<ArchiveEntry>> {
    let k = kind(archive)?;
    if crate::cloud::keep_out(archive) {
        return None;
    }
    if matches!(k, Kind::Tar(p) if p != Pack::None) && size > SEARCH_UNPACK {
        return None;
    }
    // Never a password given in this run: what search knows is what anyone sees.
    let items = read_items_with(archive, None, SEARCH_ENTRIES).ok()?;
    Some(items.into_iter().map(|it| ArchiveEntry { name: it.name, size: it.size, is_dir: it.dir, modified: it.modified }).collect())
}

/// Read the files of `archive` (`size` bytes) that `wanted` picks by name and size, in the
/// order they are in it: `read` gets each with its name, and says whether to go on. Locked
/// files are never read, and no password is ever used. Archives that would be unpacked past
/// `SEARCH_UNPACK` are not read, nor archives only in the cloud.
pub(crate) fn search_read(archive: &Path, size: u64, wanted: &dyn Fn(&str, u64) -> bool, read: &mut dyn FnMut(&str, &mut dyn Read) -> bool) -> io::Result<()> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    if crate::cloud::keep_out(archive) {
        return Err(crate::cloud::not_here(archive));
    }
    if streamed(archive) && size > SEARCH_UNPACK {
        return Ok(());
    }
    match k {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
            for i in 0..z.len().min(SEARCH_ENTRIES) {
                let (name, ok) = {
                    let e = z.by_index_raw(i).map_err(io::Error::other)?;
                    let name = e.name().trim_end_matches('/').to_string();
                    let ok = e.is_file() && !e.encrypted() && sound(&name) && wanted(&name, e.size());
                    (name, ok)
                };
                if ok && !read(&name, &mut z.by_index(i).map_err(io::Error::other)?) {
                    break;
                }
            }
        }
        Kind::SevenZ => {
            let mut r = seven(archive, None)?;
            if seven_locked(r.archive()) {
                return Ok(());
            }
            // An entry not read to its end must still be read past: in a solid 7z the next one
            // comes from the same stream.
            r.for_each_entries(|e, from| {
                let name = e.name.replace('\\', "/").trim_end_matches('/').to_string();
                let go = e.is_directory || !e.has_stream || !sound(&name) || !wanted(&name, e.size) || read(&name, from);
                if go {
                    io::copy(from, &mut io::sink())?;
                }
                Ok(go)
            })
            .map_err(seven_error)?;
        }
        Kind::Tar(_) => {
            let mut seen = 0;
            tar_named(tar_reader(archive, k)?.entries()?, |name, _, e| {
                seen += 1;
                Ok(seen <= SEARCH_ENTRIES && !(e.header().entry_type().is_file() && sound(&name) && wanted(&name, e.size()) && !read(&name, e)))
            })?;
        }
    }
    Ok(())
}

/// What is inside `archive` at `inner`, as a folder listing: its files, and the folders in it,
/// also those the archive only names in the paths of its files. `..` leads back out.
pub fn list_in(archive: &Path, inner: &str) -> io::Result<Vec<crate::fs::Entry>> {
    listing(archive, inner).map(|(entries, _)| entries)
}

/// `list_in`, and whether anything in that folder or below it is locked. A folder's size is
/// that of the files in it.
pub fn listing(archive: &Path, inner: &str) -> io::Result<(Vec<crate::fs::Entry>, bool)> {
    // Part by part, so the paths have the system's separator throughout, as the search's do.
    let at = inner.split('/').filter(|p| !p.is_empty()).fold(archive.to_path_buf(), |p, part| p.join(part));
    let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
    let mut seen = std::collections::BTreeMap::new();
    let mut locked = false;
    for it in items(archive)? {
        let Some(rest) = it.name.strip_prefix(&prefix).filter(|r| !r.is_empty()) else { continue };
        locked |= it.locked;
        let (first, deeper) = match rest.split_once('/') {
            Some((first, _)) => (first.to_string(), true),
            None => (rest.to_string(), it.dir),
        };
        let e = seen.entry(first.clone()).or_insert_with(|| crate::fs::Entry {
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
    let mut out = vec![crate::fs::Entry { name: "..".into(), path: up, is_dir: true, is_symlink: false, is_exec: false, hidden: false, size: 0, modified: 0, created: 0, online: false, referenced: 0 }];
    out.extend(seen.into_values());
    Ok((out, locked))
}

/// Whether `inner` is a folder in the archive (its top, a folder entry, or a folder only named
/// in the paths of its files).
pub fn is_folder(archive: &Path, inner: &str) -> io::Result<bool> {
    let prefix = format!("{inner}/");
    Ok(inner.is_empty() || items(archive)?.iter().any(|it| (it.dir && it.name == inner) || it.name.starts_with(&prefix)))
}

/// Whether anything at or below `inner` is locked with a password ("" for the whole archive).
pub fn locked_at(archive: &Path, inner: &str) -> io::Result<bool> {
    let prefix = format!("{inner}/");
    Ok(items(archive)?.iter().any(|it| it.locked && (inner.is_empty() || it.name == inner || it.name.starts_with(&prefix))))
}

/// Copy `inner` (a file, or a folder and everything in it) out of `archive` into `dest`, as
/// `fs::copy` would: into it when it is a folder. Returns where it landed. A locked file needs
/// `password`; without it, or with a wrong one, the error is `LOCKED`. `whole`: every entry
/// below `inner` comes out, or none does (a move takes them out of the archive after).
pub fn copy_out(archive: &Path, inner: &str, dest: &Path, password: Option<&str>, whole: bool) -> io::Result<PathBuf> {
    let name = inner.rsplit('/').next().filter(|n| !n.is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to copy"))?;
    let to = if dest.is_dir() { dest.join(name) } else { dest.to_path_buf() };
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    copy_out_with(archive, inner, &to, password_for(archive, password).as_deref(), Copy { whole, ..Copy::default() }).map(|_| to)
}

/// Files larger than this inside an archive are not copied out just to be looked at.
pub const PEEK_MAX: u64 = 256 * 1024 * 1024;

/// A copy of the file at `path` (a path through an archive), to look at: the
/// preview pane, F3. It keeps the file's name, so it is previewed by its kind. The copy before
/// it goes, and copies left by earlier runs go after a day. A locked file needs its password
/// remembered: `LOCKED` without it.
pub fn peek(path: &Path) -> io::Result<PathBuf> {
    let (archive, inner) = split(path).filter(|(_, i)| !i.is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not inside an archive"))?;
    let it = items(&archive)?.into_iter().find(|it| it.name == inner).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{inner} is not in the archive")))?;
    if it.dir {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{inner} is a folder")));
    }
    if it.size > PEEK_MAX {
        return Err(io::Error::new(io::ErrorKind::FileTooLarge, format!("{inner} is too large to look at inside the archive; copy it out with F5")));
    }
    // One at a time: each starts in a fresh folder, which would take away the copy of one
    // still being looked at, and peeks piling up while the cursor moves each unpack.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let to = peek_folder()?.join(inner.rsplit('/').next().unwrap_or("file"));
    // What comes out is counted too: a size the archive claims may be a lie.
    copy_out_with(&archive, &inner, &to, password_for(&archive, None).as_deref(), Copy { limit: PEEK_MAX, ..Copy::default() })?;
    // The copy has the date of the file in the archive (else the archive's), so what is made
    // from it and kept by its date (a LaTeX or LibreOffice preview) is found again next time.
    let when = if it.modified > 0 { Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(it.modified)) } else { std::fs::metadata(&archive).and_then(|m| m.modified()).ok() };
    if let Some(when) = when {
        let _ = File::options().write(true).open(&to).and_then(|f| f.set_modified(when));
    }
    Ok(to)
}

/// A fresh folder for this run's copy to look at (also a history's, `history::peek`): the copy
/// before it goes, and copies left by earlier runs go after a day.
pub(crate) fn peek_folder() -> io::Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| io::Error::other("no cache folder"))?.join("coxswain").join("peek");
    let day = std::time::Duration::from_secs(24 * 3600);
    for old in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        if old.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > day) {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    let run = base.join(std::process::id().to_string());
    let _ = std::fs::remove_dir_all(&run);
    // The copy may be of a locked file: for this user's eyes only.
    crate::fs::private_dir(&run)?;
    Ok(run)
}

/// `inner` (everything with "") out of `archive` to `to`, which is where it lands: the file, or
/// the folder it is unpacked into. Whatever was written is removed when it fails. A password
/// that opened a locked file is kept for the rest of this run.
fn copy_out_with(archive: &Path, inner: &str, to: &Path, password: Option<&str>, how: Copy) -> io::Result<()> {
    let (mut any, mut used) = (false, false);
    let r = copy_entries(archive, inner, to, password, how, &mut any, &mut used);
    if r.is_err() && !inner.is_empty() {
        let _ = if to.is_dir() { std::fs::remove_dir_all(to) } else { std::fs::remove_file(to) };
    }
    r?;
    if !any && !inner.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{inner} is not in {}", archive.display())));
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
        Copy { whole: false, limit: u64::MAX }
    }
}

fn copy_entries(archive: &Path, inner: &str, to: &Path, password: Option<&str>, how: Copy, any: &mut bool, used: &mut bool) -> io::Result<()> {
    let prefix = format!("{inner}/");
    let below = |name: &str| inner.is_empty() || name == inner || name.starts_with(&prefix);
    // Where an entry lands, never outside `to`: `..` and absolute parts are refused (on Windows
    // also a drive or a `\\`, which are only characters elsewhere).
    let place = |name: &str| -> Option<PathBuf> {
        let rest = if inner.is_empty() { name } else if name == inner { "" } else { name.strip_prefix(&prefix)? };
        let mut p = to.to_path_buf();
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            if part == ".." || (cfg!(windows) && part.contains(['\\', ':'])) {
                return None;
            }
            p.push(part);
        }
        Some(p)
    };
    // An entry below `inner` that is not copied: left out, or for `whole` the copy fails.
    let skip = |name: &str| -> io::Result<()> {
        if how.whole && below(name) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{name} cannot come out of the archive here, so nothing was moved")));
        }
        Ok(())
    };
    let left = std::cell::Cell::new(how.limit);
    let write = |path: &Path, from: &mut dyn Read| -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let n = io::copy(&mut from.take(left.get().saturating_add(1)), &mut File::create(path)?)?;
        if n > left.get() {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, format!("{} is too large to look at inside the archive; copy it out with F5", path.display())));
        }
        left.set(left.get() - n);
        Ok(())
    };
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    if k == Kind::Zip {
        let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
        for i in 0..z.len() {
            let (name, dir, encrypted, mode) = {
                let e = z.by_index_raw(i).map_err(io::Error::other)?;
                (e.name().trim_end_matches('/').to_string(), e.is_dir(), e.encrypted(), e.unix_mode())
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
            if let Err(err) = write(&path, &mut e) {
                return Err(if encrypted && err.kind() != io::ErrorKind::FileTooLarge { locked() } else { err });
            }
            // A script stays one; nothing but the permission bits is taken.
            #[cfg(unix)]
            if let Some(m) = mode.filter(|m| m & 0o111 != 0) {
                std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(m & 0o777))?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            *used |= encrypted;
            if name == inner {
                break;
            }
        }
    } else if k == Kind::SevenZ {
        let mut r = seven(archive, password)?;
        let locked = seven_locked(r.archive());
        let mut failed = None;
        r.for_each_entries(|e, from| {
            let name = e.name.replace('\\', "/").trim_end_matches('/').to_string();
            // Read past, not skipped: in a solid 7z the next entry comes from the same stream.
            let Some(path) = place(&name) else {
                if let Err(err) = skip(&name) {
                    failed = Some(err);
                    return Ok(false);
                }
                return io::copy(from, &mut io::sink()).map(|_| true).map_err(Into::into);
            };
            *any = true;
            let mut from = Noting { from, left: e.size, failed: false };
            let done = if e.is_directory { std::fs::create_dir_all(&path) } else { write(&path, &mut from) };
            if let Err(err) = done {
                failed = Some(seven_copy_error(err, &from, locked));
                return Ok(false);
            }
            // The one file asked for is out: nothing after it is unpacked.
            Ok(e.is_directory || name != inner)
        })
        .map_err(seven_error)?;
        if let Some(err) = failed {
            return Err(err);
        }
        *used |= locked;
    } else {
        tar_named(tar_reader(archive, k)?.entries()?, |name, _, e| {
            let Some(path) = place(&name) else {
                skip(&name)?;
                return Ok(true);
            };
            *any = true;
            let t = e.header().entry_type();
            if t.is_dir() {
                std::fs::create_dir_all(&path)?;
            } else if t.is_file() {
                write(&path, e)?;
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
    let Some(l) = chrono::Local.timestamp_opt(secs as i64, 0).single() else { return zip::DateTime::default() };
    zip::DateTime::from_date_and_time(l.year() as u16, l.month() as u8, l.day() as u8, l.hour() as u8, l.minute() as u8, l.second() as u8).unwrap_or_default()
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// How a 7z is locked with `pw`: AES-256, its key made with 2^19 rounds as 7-Zip makes it (the
/// crate's own 2^8 makes guessing the password two thousand times cheaper), then LZMA2.
pub(crate) fn seven_locking(pw: &str) -> Vec<sevenz_rust2::EncoderConfiguration> {
    let mut aes = sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(pw));
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
    let Some(dir) = crate::helper::folder() else { return Ok(None) };
    crate::fs::private_dir(&dir)?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("archive-writing.lock"))?;
    f.lock()?;
    Ok(Some(f))
}

/// Write `archive` anew: each old entry by what `keep` says of its name (`None`: left out,
/// `Some(name)`: kept under that name, as it was, locked or not), then `add`. Into a new
/// file first, which then takes the old one's place, so a failure leaves the archive whole.
/// A 7z with locked contents is read with `password` (or the one remembered), which is then
/// kept for this run. A new archive (`create`) is locked with `password`, if any, and a new
/// 7z's names too with `hide_names`.
fn rewrite(archive: &Path, keep: &dyn Fn(&str) -> Option<String>, add: &[(String, New)], password: Option<&str>, hide_names: bool) -> io::Result<()> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let _all = writing_lock()?;
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let tmp = archive.with_file_name(format!("{name}.{}.coxswain-tmp", std::process::id()));
    let exists = archive.exists();
    let mut used = false;
    let written = (|| -> io::Result<()> {
        if k == Kind::Zip {
            let mut w = zip::ZipWriter::new(create_fresh(&tmp)?);
            // New files go into a locked zip locked too, with its password; a wrong one, or
            // none, is refused, so the apps ask.
            let mut pw = if exists { None } else { password.map(String::from) };
            used = pw.is_some();
            if exists {
                let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
                let mut first_locked = None;
                for i in 0..z.len() {
                    let e = z.by_index_raw(i).map_err(io::Error::other)?;
                    if e.encrypted() && !e.is_dir() {
                        first_locked.get_or_insert(i);
                    }
                    let old = e.name().trim_end_matches('/').to_string();
                    let slash = e.name().ends_with('/');
                    // Locked the old way (ZipCrypto): the crate's raw copy drops that lock and
                    // leaves the bytes scrambled, so the file is read with the password and
                    // locked anew with AES-256 (as new files are). Without one: ask.
                    let old_lock = e.encrypted() && !e.is_dir() && zip::read::HasZipMetadata::get_metadata(&e).aes_mode.is_none();
                    match keep(&old) {
                        Some(new) if old_lock => {
                            drop(e);
                            let given = password_for(archive, password).ok_or_else(locked)?;
                            let mut from = z.by_index_decrypt(i, given.as_bytes()).map_err(|e| if matches!(e, zip::result::ZipError::InvalidPassword) { locked() } else { io::Error::other(e) })?;
                            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).last_modified_time(from.last_modified().unwrap_or_default()).with_aes_encryption(zip::AesMode::Aes256, &given);
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
                        Some(new) => w.raw_copy_file_rename(e, if slash { format!("{new}/") } else { new }).map_err(io::Error::other),
                        None => Ok(()),
                    }?;
                }
                if let (Some(i), true) = (first_locked, add.iter().any(|(_, n)| matches!(n, New::File(_)))) {
                    let given = password_for(archive, password).ok_or_else(locked)?;
                    let mut e = z.by_index_decrypt(i, given.as_bytes()).map_err(|e| if matches!(e, zip::result::ZipError::InvalidPassword) { locked() } else { io::Error::other(e) })?;
                    // ZipCrypto's own check lets about one wrong password in 256 through: the
                    // file is read to its end, where its checksum tells.
                    io::copy(&mut e, &mut io::sink()).map_err(|_| locked())?;
                    pw = Some(given);
                }
            }
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, new) in add {
                match new {
                    New::Dir => w.add_directory(format!("{name}/"), opts).map_err(io::Error::other)?,
                    New::File(path) => {
                        let opts = opts.last_modified_time(zip_time(modified(path)));
                        let opts = match &pw {
                            Some(pw) => opts.with_aes_encryption(zip::AesMode::Aes256, pw),
                            None => opts,
                        };
                        #[cfg(unix)]
                        let opts = opts.unix_permissions(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path)?.permissions()));
                        w.start_file(name.as_str(), opts).map_err(io::Error::other)?;
                        io::copy(&mut File::open(path)?, &mut w)?;
                    }
                }
            }
            w.finish().map_err(io::Error::other)?;
        } else if k == Kind::SevenZ {
            // 7z has no copying of entries as they are: each is unpacked and packed again on
            // its way from the old file to the new, as it was but for its name.
            let mut w = sevenz_rust2::ArchiveWriter::new(create_fresh(&tmp)?).map_err(io::Error::other)?;
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
                    let Some(new) = keep(&old) else { return io::copy(from, &mut io::sink()).map(|_| true).map_err(Into::into) };
                    let mut entry = e.clone();
                    entry.name = new;
                    let mut from = Noting { from, left: e.size, failed: false };
                    let pushed = if e.is_directory { w.push_archive_entry::<File>(entry, None) } else { w.push_archive_entry(entry, Some(&mut from)) };
                    if let Err(err) = pushed {
                        failed = Some(seven_copy_error(io::Error::other(err), &from, locked));
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
                    New::Dir => w.push_archive_entry::<File>(sevenz_rust2::ArchiveEntry::new_directory(name), None),
                    New::File(path) => w.push_archive_entry(sevenz_rust2::ArchiveEntry::from_path(path, name.clone()), Some(File::open(path)?)),
                }
                .map_err(io::Error::other)?;
            }
            w.finish().map(drop).map_err(io::Error::other)?;
        } else {
            let Kind::Tar(pack) = k else { unreachable!() };
            // Written plain first, then compressed: every compression the same way.
            let plain = tmp.with_extension("coxswain-tar");
            let mut b = tar::Builder::new(io::BufWriter::new(create_fresh(&plain)?));
            if exists {
                // Names and link targets written whole: past 100 bytes they take a GNU long
                // name of their own, which `append` with the old header would drop.
                tar_named(tar_reader(archive, k)?.entries()?, |old, link, e| {
                    let Some(new) = keep(&old) else { return Ok(true) };
                    let mut header = e.header().clone();
                    let t = header.entry_type();
                    match link {
                        Some(target) if t.is_symlink() || t.is_hard_link() => b.append_link(&mut header, &new, target)?,
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
                        h.set_mtime(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
                        b.append_data(&mut h, name, io::empty())?;
                    }
                    New::File(path) => b.append_path_with_name(path, name)?,
                }
            }
            b.into_inner()?.flush()?;
            let done = compress(&plain, &tmp, pack);
            let _ = std::fs::remove_file(&plain);
            done?;
        }
        Ok(())
    })();
    let done = match written {
        // As private as the archive was.
        Ok(()) => std::fs::metadata(archive).and_then(|m| std::fs::set_permissions(&tmp, m.permissions())).or_else(|e| if exists { Err(e) } else { Ok(()) }).and_then(|_| std::fs::rename(&tmp, archive)),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
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
    let gone = |name: &str| inner.iter().any(|i| name == i || name.starts_with(&format!("{i}/")));
    rewrite(archive, &|name| (!gone(name)).then(|| name.to_string()), &[], password, false)
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
                walk(&k.path(), format!("{name}/{}", k.file_name().to_string_lossy()), out)?;
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
            let name = src.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to add"))?;
            Ok((name.to_string_lossy().into_owned(), src.clone()))
        })
        .collect()
}

/// Copy files from disk into `archive`, each source under the name given for it inside
/// (`docs/a.txt`). What is there by a name already is kept and the copy refused, as a copy
/// between folders never overwrites.
pub fn add(archive: &Path, into: &[(String, PathBuf)], password: Option<&str>) -> io::Result<()> {
    let new = entries_of(into)?;
    let have: std::collections::HashSet<String> = items(archive)?.into_iter().map(|it| it.name).collect();
    if let Some((name, _)) = new.iter().find(|(name, n)| !matches!(n, New::Dir) && have.contains(name)) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{name} exists in {}", archive.display())));
    }
    // A folder the archive has already is not written twice.
    let new: Vec<_> = new.into_iter().filter(|(name, n)| !(matches!(n, New::Dir) && have.contains(name))).collect();
    rewrite(archive, &|name| Some(name.to_string()), &new, password, false)
}

/// A new, empty folder inside the archive.
pub fn mkdir(archive: &Path, inner: &str, password: Option<&str>) -> io::Result<()> {
    if items(archive)?.iter().any(|it| it.name == inner) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{inner} exists")));
    }
    rewrite(archive, &|name| Some(name.to_string()), &[(inner.to_string(), New::Dir)], password, false)
}

/// Rename or move `from` (a file, or a folder and all in it) to `to`, inside the archive.
pub fn rename_in(archive: &Path, from: &str, to: &str, password: Option<&str>) -> io::Result<()> {
    let names: Vec<String> = items(archive)?.into_iter().map(|it| it.name).collect();
    if names.iter().any(|n| n == to || n.starts_with(&format!("{to}/"))) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{to} exists")));
    }
    let prefix = format!("{from}/");
    rewrite(archive, &|name| Some(if name == from { to.to_string() } else if let Some(rest) = name.strip_prefix(&prefix) { format!("{to}/{rest}") } else { name.to_string() }), &[], password, false)
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
    PackFormat { id: "zip", label: "Zip", endings: &[".zip"], password: true },
    PackFormat { id: "7z", label: "7z", endings: &[".7z"], password: true },
    PackFormat { id: "tar", label: "tar", endings: &[".tar"], password: false },
    PackFormat { id: "tar_gz", label: "tar.gz", endings: &[".tar.gz", ".tgz"], password: false },
    PackFormat { id: "tar_bz2", label: "tar.bz2", endings: &[".tar.bz2", ".tbz2", ".tbz"], password: false },
    PackFormat { id: "tar_xz", label: "tar.xz", endings: &[".tar.xz", ".txz"], password: false },
    PackFormat { id: "tar_zst", label: "tar.zst", endings: &[".tar.zst", ".tzst"], password: false },
];

/// The pack format a name's ending picks, if any.
pub fn pack_format(name: &str) -> Option<&'static PackFormat> {
    let lower = name.to_ascii_lowercase();
    PACK_FORMATS.iter().find(|f| f.endings.iter().any(|e| lower.ends_with(e)))
}

/// `name` with its archive ending (`.tar.gz` whole) swapped for `ending`, or `ending` added.
pub fn with_ending(name: &str, ending: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let cut = ENDINGS.iter().map(|(e, _)| *e).find(|e| lower.ends_with(e)).map_or(name.len(), |e| name.len() - e.len());
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
pub fn create_locked(path: &Path, sources: &[PathBuf], password: Option<&str>, hide_names: bool) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", path.display())));
    }
    kind(path).ok_or_else(|| not_archive(path))?;
    let password = password.filter(|p| !p.is_empty());
    if password.is_some() && !takes_password(path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "tar archives have no passwords"));
    }
    rewrite(path, &|_| None, &entries_of(&named(sources)?)?, password, hide_names)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    /// A 7z locked here has its key made as 7-Zip makes it: 2^19 rounds, not the crate's 2^8.
    #[test]
    fn archive_7z_key_is_made_as_7zip_makes_it() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7zkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "secret").unwrap();
        let p = d.join("locked.7z");
        create_locked(&p, &[d.join("a.txt")], Some("hunter2"), false).unwrap();
        let r = seven(&p, Some("hunter2")).unwrap();
        let aes = r.archive().blocks.iter().flat_map(|b| &b.coders).find(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256).expect("an AES coder");
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
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() & !1;
        let local = chrono::Local.timestamp_opt(now as i64, 0).unwrap();
        let t = zip_time(now);
        assert_eq!((t.year() as i32, t.month() as u32, t.day() as u32, t.hour() as u32), (local.year(), local.month(), local.day(), local.hour()));
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
            assert_eq!(std::fs::read_to_string(d.join("out/logs/10:30.log")).unwrap(), "at half past");
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

        // A long name stays whole when the tar is written anew.
        let long = format!("deep/{}/file.txt", "x".repeat(150));
        let tp = d.join("long.tar");
        tar_of(&tp, &|t| {
            file(t, &long, b"long");
            file(t, "gone.txt", b"g");
        });
        remove(&tp, &["gone.txt".into()], None).unwrap();
        assert_eq!(list(&tp, 10).unwrap().0.iter().map(|e| e.name.clone()).collect::<Vec<_>>(), [long.clone()]);

        // A long-name record far past any name is refused before it is read.
        let tp = d.join("bomb.tar");
        tar_of(&tp, &|t| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::GNULongName);
            h.set_size(TAR_META + 1);
            t.append_data(&mut h, "././@LongLink", &vec![b'a'; TAR_META as usize + 1][..]).unwrap();
            file(t, "x", b"x");
        });
        assert!(list(&tp, 10).is_err());
        assert!(extract(&tp, &d.join("out")).is_err() && !d.join("out/bomb").exists(), "extracted neither");

        // Names that lead outside are not in the listing; a link in a zip comes out as a file.
        let zp = d.join("odd.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        w.start_file("../evil.txt", SimpleFileOptions::default()).unwrap();
        w.start_file("ok.txt", SimpleFileOptions::default()).unwrap();
        io::Write::write_all(&mut w, b"fine").unwrap();
        w.start_file("run.sh", SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
        w.add_symlink("link", "/etc/passwd", SimpleFileOptions::default()).unwrap();
        w.finish().unwrap();
        let names: Vec<String> = list_in(&zp, "").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["..", "link", "ok.txt", "run.sh"]);
        let out = extract(&zp, &d.join("out")).unwrap();
        assert!(!std::fs::symlink_metadata(out.join("link")).unwrap().file_type().is_symlink(), "no link made");
        assert!(!d.join("evil.txt").exists() && !d.join("out/evil.txt").exists());
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(out.join("run.sh")).unwrap().permissions()) & 0o777, 0o755);

        // What comes out is counted: past the limit it stops.
        let err = copy_out_with(&zp, "ok.txt", &d.join("out/ok.txt"), None, Copy { limit: 3, ..Copy::default() }).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert!(!d.join("out/ok.txt").exists());

        // Written anew, an archive stays as private as it was.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&zp, std::fs::Permissions::from_mode(0o600)).unwrap();
            remove(&zp, &["ok.txt".into()], None).unwrap();
            assert_eq!(std::fs::metadata(&zp).unwrap().permissions().mode() & 0o777, 0o600);
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
        w.add_directory("sub/", zip::write::SimpleFileOptions::default()).unwrap();
        w.start_file("sub/a.txt", zip::write::SimpleFileOptions::default()).unwrap();
        io::Write::write_all(&mut w, b"hello").unwrap();
        w.finish().unwrap();
        let (entries, more) = list(&zp, 10).unwrap();
        assert!(!more);
        assert_eq!(entries[1], ArchiveEntry { name: "sub/a.txt".into(), size: 5, is_dir: false, modified: entries[1].modified });
        assert!(entries[0].is_dir);
        assert_eq!(list(&zp, 1).unwrap(), (vec![entries[0].clone()], true));
        let out = extract(&zp, &d).unwrap();
        assert_eq!(out, d.join("pack"));
        assert_eq!(std::fs::read_to_string(out.join("sub/a.txt")).unwrap(), "hello");
        assert!(extract(&zp, &d).is_err(), "never extracts over an existing folder");

        let tp = d.join("pack2.tar.gz");
        let gz = flate2::write::GzEncoder::new(File::create(&tp).unwrap(), flate2::Compression::fast());
        let mut t = tar::Builder::new(gz);
        t.append_path_with_name(d.join("pack/sub/a.txt"), "x/a.txt").unwrap();
        t.into_inner().unwrap().finish().unwrap();
        let listed = list(&tp, 10).unwrap().0;
        assert_eq!(listed, [ArchiveEntry { name: "x/a.txt".into(), size: 5, is_dir: false, modified: listed[0].modified }]);
        assert!(listed[0].modified > 0, "a tar's time comes along");
        assert_eq!(std::fs::read_to_string(extract(&tp, &d).unwrap().join("x/a.txt")).unwrap(), "hello");

        assert!(list(&d.join("pack/sub/a.txt"), 10).is_err());

        // A look at a file inside: a copy by its own name; the next look replaces it.
        let seen = peek(&zp.join("sub/a.txt")).unwrap();
        assert_eq!((seen.file_name().unwrap().to_str(), std::fs::read_to_string(&seen).unwrap().as_str()), (Some("a.txt"), "hello"));
        let next = peek(&tp.join("x/a.txt")).unwrap();
        assert!(next.exists() && (next == seen || !seen.exists()), "one copy at a time");
        let date = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        let first = date(&next);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(date(&peek(&tp.join("x/a.txt")).unwrap()), first, "a copy has the file's date, not the time it was made");
        assert!(peek(&zp.join("sub")).is_err(), "a folder is not looked at");
        assert!(peek(&zp.join("nothing.txt")).is_err());
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
        let file = |w: &mut zip::ZipWriter<File>, name: &str, text: &[u8], opts: SimpleFileOptions| {
            w.start_file(name, opts).unwrap();
            io::Write::write_all(w, text).unwrap();
        };
        let plain = SimpleFileOptions::default();
        file(&mut w, "readme.txt", b"hello", plain);
        // Folders only named in the paths of their files.
        file(&mut w, "bin/run.sh", b"echo go", plain);
        file(&mut w, "bin/lib/core.so", b"\x7fELF", plain);
        file(&mut w, "keys/old.pem", b"secret one", plain.with_aes_encryption(zip::AesMode::Aes128, "hunter2"));
        file(&mut w, "keys/new.pem", b"secret two", plain.with_aes_encryption(zip::AesMode::Aes256, "hunter2"));
        w.finish().unwrap();

        // A path through the archive lists like a folder, `..` first.
        let names = |p: &Path| crate::fs::list(p, false).unwrap().iter().map(|e| (e.name.clone(), e.is_dir)).collect::<Vec<_>>();
        assert_eq!(names(&zp), [("..".into(), true), ("bin".into(), true), ("keys".into(), true), ("readme.txt".into(), false)]);
        assert_eq!(names(&zp.join("bin")), [("..".into(), true), ("lib".into(), true), ("run.sh".into(), false)]);
        assert_eq!(crate::fs::list(&zp.join("bin"), false).unwrap()[0].path, zp, "`..` leads back to the archive's top");
        assert_eq!(split(&zp.join("bin/lib")), Some((zp.clone(), "bin/lib".into())));
        assert_eq!(split(&d.join("out")), None);

        // Copy out: a file, a folder with all in it.
        assert_eq!(std::fs::read_to_string(crate::fs::copy(&zp.join("readme.txt"), &d.join("out")).unwrap()).unwrap(), "hello");
        let bin = crate::fs::copy(&zp.join("bin"), &d.join("out")).unwrap();
        assert_eq!(std::fs::read(bin.join("lib/core.so")).unwrap(), b"\x7fELF");
        assert!(crate::fs::copy(&zp.join("readme.txt"), &d.join("out")).is_err(), "never over a file that is there");

        // Locked files want the password (AES here; the zip crate writes no ZipCrypto, which reads the same way).
        assert!(locked_at(&zp, "keys").unwrap() && !locked_at(&zp, "bin").unwrap());
        let err = crate::fs::copy(&zp.join("keys"), &d.join("out")).unwrap_err();
        assert_eq!(err.to_string(), LOCKED);
        assert_eq!(crate::fs::copy_locked(&zp.join("keys/new.pem"), &d.join("out"), Some("wrong")).unwrap_err().to_string(), LOCKED);
        let keys = crate::fs::copy_locked(&zp.join("keys"), &d.join("out"), Some("hunter2")).unwrap();
        assert_eq!((std::fs::read(keys.join("old.pem")).unwrap(), std::fs::read(keys.join("new.pem")).unwrap()), (b"secret one".to_vec(), b"secret two".to_vec()));

        // Taken out of the archive: the rest stays as it was, locked files still locked.
        crate::fs::delete(&zp.join("bin")).unwrap();
        assert_eq!(names(&zp), [("..".into(), true), ("keys".into(), true), ("readme.txt".into(), false)]);
        std::fs::remove_dir_all(d.join("out/keys")).unwrap();
        // The password that opened it was kept for the run; forgotten, it is locked again.
        assert!(crate::fs::copy(&zp.join("keys/old.pem"), &d.join("out")).is_ok());
        std::fs::remove_file(d.join("out/old.pem")).unwrap();
        forget(&zp);
        assert_eq!(crate::fs::copy(&zp.join("keys/old.pem"), &d.join("out")).unwrap_err().to_string(), LOCKED);
        // A new file goes into a locked zip locked too, with the archive's password: without
        // one, or with a wrong one, it is refused.
        std::fs::write(d.join("out/note.txt"), "new secret").unwrap();
        assert_eq!(crate::fs::copy(&d.join("out/note.txt"), &zp.join("keys")).unwrap_err().to_string(), LOCKED);
        assert_eq!(crate::fs::copy_locked(&d.join("out/note.txt"), &zp.join("keys"), Some("wrong")).unwrap_err().to_string(), LOCKED);
        crate::fs::copy_locked(&d.join("out/note.txt"), &zp.join("keys"), Some("hunter2")).unwrap();
        forget(&zp);
        assert!(locked_at(&zp, "keys/note.txt").unwrap(), "the new file is locked");
        assert_eq!(crate::fs::copy(&zp.join("keys/note.txt"), &d).unwrap_err().to_string(), LOCKED);
        assert_eq!(std::fs::read_to_string(crate::fs::copy_locked(&zp.join("keys/note.txt"), &d, Some("hunter2")).unwrap()).unwrap(), "new secret");
        forget(&zp);
        // Moved out: copied, then gone from the archive.
        std::fs::remove_file(d.join("out/readme.txt")).unwrap();
        crate::fs::rename(&zp.join("readme.txt"), &d.join("out")).unwrap();
        assert!(d.join("out/readme.txt").is_file());
        assert_eq!(names(&zp).len(), 2);

        // A tar.gz: the same, written anew without what was taken out.
        let tp = d.join("pack.tar.gz");
        let mut t = tar::Builder::new(flate2::write::GzEncoder::new(File::create(&tp).unwrap(), flate2::Compression::fast()));
        t.append_path_with_name(d.join("out/readme.txt"), "a/one.txt").unwrap();
        t.append_path_with_name(d.join("out/readme.txt"), "a/two.txt").unwrap();
        t.into_inner().unwrap().finish().unwrap();
        crate::fs::delete(&tp.join("a/one.txt")).unwrap();
        assert_eq!(names(&tp.join("a")), [("..".into(), true), ("two.txt".into(), false)]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn archive_takes_files_in_renames_and_packs() {
        let d = std::env::temp_dir().join(format!("coxswain-test-crud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/a.txt"), "one").unwrap();
        std::fs::write(d.join("src/deep/b.txt"), "two").unwrap();
        let names = |p: &Path| crate::fs::list(p, true).unwrap().into_iter().skip(1).map(|e| e.name).collect::<Vec<_>>();
        for pack in ["new.zip", "new.tar.gz", "new.tar", "new.tar.bz2", "new.tar.xz", "new.tar.zst", "new.7z"] {
            let zp = d.join(pack);
            // Pack, then add, a folder inside, rename, move between archives.
            create(&zp, &[d.join("src/a.txt")]).unwrap();
            assert!(create(&zp, &[d.join("src/a.txt")]).is_err(), "never over an archive that is there");
            crate::fs::copy(&d.join("src/deep"), &zp).unwrap();
            assert_eq!(names(&zp), ["a.txt", "deep"], "{pack}");
            assert_eq!(names(&zp.join("deep")), ["b.txt"]);
            assert!(crate::fs::copy(&d.join("src/a.txt"), &zp).is_err(), "never over a file inside either");
            crate::fs::mkdir(&zp.join("docs")).unwrap();
            crate::fs::rename(&zp.join("a.txt"), &zp.join("docs")).unwrap();
            assert_eq!(names(&zp.join("docs")), ["a.txt"]);
            // Copied in under a new name: not one folder too deep (docs/b.txt/b.txt).
            assert_eq!(crate::fs::copy(&d.join("src/deep/b.txt"), &zp.join("docs/c.txt")).unwrap(), zp.join("docs/c.txt"));
            assert_eq!(names(&zp.join("docs")), ["a.txt", "c.txt"], "{pack}");
            assert_eq!(std::fs::read_to_string(crate::fs::copy(&zp.join("docs/c.txt"), &d).unwrap()).unwrap(), "two");
            std::fs::remove_file(d.join("c.txt")).unwrap();
            crate::fs::delete(&zp.join("docs/c.txt")).unwrap();
            // A folder's size is that of the files in it, and the dates of what was added stay.
            let docs = crate::fs::list(&zp, true).unwrap().into_iter().find(|e| e.name == "docs").unwrap();
            assert_eq!((docs.is_dir, docs.size), (true, 3), "{pack}");
            let a = crate::fs::list(&zp.join("docs"), true).unwrap().into_iter().find(|e| e.name == "a.txt").unwrap();
            assert_eq!(a.modified / 60, modified(&d.join("src/a.txt")) / 60, "{pack}: modified kept through the rewrites");
            crate::fs::rename(&zp.join("deep"), &zp.join("renamed")).unwrap();
            assert_eq!(names(&zp), ["docs", "renamed"]);
            let other = d.join(format!("other-{pack}"));
            create(&other, &[d.join("src/a.txt")]).unwrap();
            crate::fs::copy(&zp.join("renamed"), &other).unwrap();
            assert_eq!(names(&other.join("renamed")), ["b.txt"], "from one archive into another");
            let back = crate::fs::copy(&other.join("renamed/b.txt"), &d.join("src/deep")).unwrap_err();
            assert_eq!(back.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(std::fs::read_to_string(crate::fs::copy(&other.join("renamed/b.txt"), &d).unwrap()).unwrap(), "two");
            std::fs::remove_file(d.join("b.txt")).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_search_sees_only_names_that_are_paths_and_never_locked_text() {
        let d = std::env::temp_dir().join(format!("coxswain-test-archive-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zp = d.join("odd.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        let deep = vec!["a"; 65].join("/");
        for name in ["../evil.txt", "/abs.txt", "ok/./x.txt", "fine/name.txt", deep.as_str()] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            io::Write::write_all(&mut w, b"words").unwrap();
        }
        w.start_file("locked.txt", zip::write::SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "pw")).unwrap();
        io::Write::write_all(&mut w, b"secret").unwrap();
        w.finish().unwrap();
        let size = std::fs::metadata(&zp).unwrap().len();
        let names: Vec<String> = search_entries(&zp, size).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["fine/name.txt", "locked.txt"]);
        let mut read = vec![];
        search_read(&zp, size, &|_, _| true, &mut |name, from| {
            let mut text = String::new();
            from.read_to_string(&mut text).unwrap();
            read.push((name.to_string(), text));
            true
        })
        .unwrap();
        assert_eq!(read, [("fine/name.txt".to_string(), "words".to_string())]);
        // A compressed tar too large to unpack for a listing is not looked into.
        assert!(search_entries(&d.join("big.tar.xz"), SEARCH_UNPACK + 1).is_none());
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
            create_locked(&zp, &[d.join("src/plan.txt"), d.join("src/deep")], Some("hunter2"), hide).unwrap();
            // Kept for this run: it opens without asking.
            let out = d.join(format!("out-{pack}"));
            std::fs::create_dir_all(&out).unwrap();
            assert_eq!(std::fs::read_to_string(crate::fs::copy(&zp.join("plan.txt"), &out).unwrap()).unwrap(), "launch at noon", "{pack}");
            forget(&zp);
            // Without it: locked, the names too only when hidden.
            assert_eq!(list_in(&zp, "").is_err_and(|e| e.to_string() == LOCKED), hide, "{pack}");
            assert_eq!(extract(&zp, &out).unwrap_err().to_string(), LOCKED, "{pack}");
            assert_eq!(extract_locked(&zp, &out, Some("wrong")).unwrap_err().to_string(), LOCKED, "{pack}");
            let x = extract_locked(&zp, &out, Some("hunter2")).unwrap();
            assert_eq!(std::fs::read_to_string(x.join("deep/crew.txt")).unwrap(), "four", "{pack}");
            if hide {
                let raw = std::fs::read(&zp).unwrap();
                assert!(!raw.windows(8).any(|w| w == b"crew.txt") && !raw.windows(16).any(|w| w == "crew.txt".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()), "names hidden");
            }
        }
        // A zip: every file AES-256.
        let mut z = zip::ZipArchive::new(File::open(d.join("pw.zip")).unwrap()).unwrap();
        for i in 0..z.len() {
            let e = z.by_index_raw(i).unwrap();
            assert!(e.is_dir() || e.encrypted(), "{}", e.name());
        }
        // Tar has no passwords; an empty one is none.
        assert!(create_locked(&d.join("pw.tar.gz"), &[d.join("src/plan.txt")], Some("x"), true).is_err());
        assert!(!d.join("pw.tar.gz").exists());
        create_locked(&d.join("open.zip"), &[d.join("src/plan.txt")], Some(""), true).unwrap();
        assert!(!locked_at(&d.join("open.zip"), "").unwrap());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A zip locked the old way (ZipCrypto, as 7-Zip's `-mem=ZipCrypto` makes it): its check
    /// byte lets about one wrong password in 256 through, so adding to it with such a password
    /// would lock the new file with the wrong one. The whole first file tells.
    #[test]
    fn archive_zipcrypto_add_refuses_a_wrong_password_the_check_byte_lets_through() {
        let d = std::env::temp_dir().join(format!("coxswain-test-zipcrypto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // `a.txt` holding "secret one", password "hunter2".
        let hex = "504b030414000100000032aa435d67dce7bc160000000a00000005000000612e747874a44b0bfe5b507702f57a5e41c4a571f7c3bbf68088e4504b01023f0314000100000032aa435d67dce7bc160000000a000000050024000000000000002080a48100000000612e7478740a00200000000000010018004e6675d86b53dd0100000000000000000000000000000000504b0506000000000100010057000000390000000000";
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let zp = d.join("old.zip");
        std::fs::write(&zp, bytes).unwrap();
        std::fs::write(d.join("b.txt"), "new").unwrap();
        let mut z = zip::ZipArchive::new(File::open(&zp).unwrap()).unwrap();
        let wrong = (0..100_000).map(|i| format!("w{i}")).find(|pw| z.by_index_decrypt(0, pw.as_bytes()).is_ok()).expect("a wrong password the check byte lets through");
        drop(z);
        std::fs::create_dir_all(d.join("out")).unwrap();
        assert_eq!(crate::fs::copy_locked(&zp.join("a.txt"), &d.join("out"), Some(&wrong)).unwrap_err().to_string(), LOCKED, "a copy out notices by the checksum");
        assert!(!d.join("out/a.txt").exists());
        assert_eq!(add(&zp, &[("b.txt".into(), d.join("b.txt"))], Some(&wrong)).unwrap_err().to_string(), LOCKED);
        assert_eq!(list(&zp, 10).unwrap().0.len(), 1, "unchanged");
        // Any change to it needs the password: the crate's raw copy would leave the old lock's
        // bytes scrambled, so the file is locked anew with AES-256 and still reads.
        assert_eq!(crate::fs::mkdir(&zp.join("docs")).unwrap_err().to_string(), LOCKED);
        add(&zp, &[("b.txt".into(), d.join("b.txt"))], Some("hunter2")).unwrap();
        crate::fs::mkdir(&zp.join("docs")).unwrap();
        forget(&zp);
        let mut z = zip::ZipArchive::new(File::open(&zp).unwrap()).unwrap();
        let a = z.by_index_raw(0).unwrap();
        assert!(a.encrypted() && zip::read::HasZipMetadata::get_metadata(&a).aes_mode.is_some(), "locked anew with AES");
        drop(a);
        drop(z);
        for (name, text) in [("a.txt", "secret one"), ("b.txt", "new")] {
            assert_eq!(crate::fs::copy_locked(&zp.join(name), &d.join("out"), Some("wrong")).unwrap_err().to_string(), LOCKED);
            assert_eq!(std::fs::read_to_string(crate::fs::copy_locked(&zp.join(name), &d.join("out"), Some("hunter2")).unwrap()).unwrap(), text);
            forget(&zp);
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Three processes (the desktop app, the terminal app, another window) adding to one
    /// archive at once: each waits for the others, so every change is in it.
    #[test]
    fn archive_writes_wait_for_other_processes() {
        if let Ok(job) = std::env::var("COXSWAIN_TEST_ADD") {
            let [zip, file, tag] = job.split('|').collect::<Vec<_>>()[..] else { panic!("{job}") };
            for i in 0..6 {
                add(Path::new(zip), &[(format!("{tag}-{i}.txt"), PathBuf::from(file))], None).unwrap();
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
                    .args(["archive::tests::archive_writes_wait_for_other_processes", "--exact", "--test-threads=1"])
                    .env("COXSWAIN_TEST_ADD", format!("{}|{}|{tag}", zip.display(), d.join("x.txt").display()))
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut k in kids {
            assert!(k.wait().unwrap().success());
        }
        let (names, _) = list(&zip, 100).unwrap();
        assert_eq!(names.len(), 1 + 3 * 6, "{:?}", names.iter().map(|e| &e.name).collect::<Vec<_>>());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn archive_7z_header_that_unpacks_too_big_is_refused() {
        let d = std::env::temp_dir().join(format!("coxswain-test-7zbomb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // A packed header of 10 bytes that says it unpacks to 2 GB (LZMA, one block).
        let head = [0x17, 0x06, 0x00, 0x01, 0x09, 0x0A, 0x00, 0x07, 0x0B, 0x01, 0x00, 0x01, 0x03, 0x03, 0x01, 0x01, 0x0C, 0xF0, 0, 0, 0, 0x80, 0x00, 0x00];
        let mut bomb = b"7z\xBC\xAF\x27\x1C\x00\x04".to_vec();
        bomb.extend([0; 4]);
        bomb.extend(10u64.to_le_bytes());
        bomb.extend((head.len() as u64).to_le_bytes());
        bomb.extend([0; 4]);
        bomb.extend([0x5D; 10]);
        bomb.extend(head);
        let p = d.join("bomb.7z");
        std::fs::write(&p, &bomb).unwrap();
        assert_eq!(seven_header_size(&mut File::open(&p).unwrap()).unwrap(), 1 << 31);
        let err = list(&p, 10).unwrap_err().to_string();
        assert!(err.contains("2048") && err.contains("64"), "{err}");
        assert!(peek(&p.join("a.txt")).is_err() && search_entries(&p, bomb.len() as u64).is_none());
        // Real ones, their headers packed, and locked with their names: read as before.
        for i in 0..200 {
            std::fs::write(d.join(format!("file-with-a-long-name-{i}.txt")), "x").unwrap();
        }
        let files: Vec<PathBuf> = (0..200).map(|i| d.join(format!("file-with-a-long-name-{i}.txt"))).collect();
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
        let readers = names.iter().map(|n| sevenz_rust2::SourceReader::new(std::io::Cursor::new(n.repeat(1000)))).collect();
        w.push_archive_entries(entries.collect(), readers).unwrap();
        w.finish().unwrap();
        assert_eq!(std::fs::read_to_string(peek(&zp.join("c.txt")).unwrap()).unwrap(), "c.txt".repeat(1000), "the last, past two not wanted");
        let mut seen = vec![];
        search_read(&zp, 1, &|n, _| n != "a.txt", &mut |n, from| {
            let mut head = [0u8; 5];
            from.read_exact(&mut head).unwrap();
            seen.push((n.to_string(), head));
            true
        })
        .unwrap();
        assert_eq!(seen, [("b.txt".to_string(), *b"b.txt"), ("c.txt".to_string(), *b"c.txt")], "read in part, then the next");
        remove(&zp, &["a.txt".to_string()], None).unwrap();
        assert_eq!(list_in(&zp, "").unwrap().iter().filter(|e| !e.is_parent()).map(|e| e.name.as_str()).collect::<Vec<_>>(), ["b.txt", "c.txt"]);
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
        w.set_content_methods(vec![sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from("hunter2")).into(), sevenz_rust2::EncoderMethod::LZMA2.into()]);
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::from_path(d.join("plan.txt"), "plan.txt".into()), Some(File::open(d.join("plan.txt")).unwrap())).unwrap();
        w.finish().unwrap();
        // Even its names are locked: looking in needs the password, which is kept for the run.
        assert_eq!(list_in(&zp, "").unwrap_err().to_string(), LOCKED);
        assert_eq!(crate::fs::copy(&zp.join("plan.txt"), &d.join("out")).unwrap_err().to_string(), LOCKED);
        remember(&zp, "wrong");
        assert_eq!(list_in(&zp, "").unwrap_err().to_string(), LOCKED);
        remember(&zp, "hunter2");
        assert_eq!(list_in(&zp, "").unwrap().len(), 2);
        assert_eq!(std::fs::read_to_string(crate::fs::copy(&zp.join("plan.txt"), &d.join("out")).unwrap()).unwrap(), "launch at noon");
        // Changed, it is written anew: still locked, with the same password.
        crate::fs::mkdir(&zp.join("docs")).unwrap();
        crate::fs::rename(&zp.join("plan.txt"), &zp.join("docs")).unwrap();
        forget(&zp);
        assert_eq!(list_in(&zp, "").unwrap_err().to_string(), LOCKED, "a changed archive is as locked as before");
        remember(&zp, "hunter2");
        std::fs::create_dir_all(d.join("again")).unwrap();
        assert_eq!(std::fs::read_to_string(crate::fs::copy(&zp.join("docs/plan.txt"), &d.join("again")).unwrap()).unwrap(), "launch at noon");
        forget(&zp);
        assert!(list_in(&zp, "").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A wrong 7z key can end an entry's stream at once, and sevenz checks the checksum only
    /// at the end: an entry shorter than its size is a failed read (once flaky on FreeBSD CI).
    #[test]
    fn archive_7z_entry_that_ends_early_fails() {
        let mut empty: &[u8] = b"";
        let mut r = Noting { from: &mut empty, left: 14, failed: false };
        assert_eq!(io::copy(&mut r, &mut io::sink()).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert!(r.failed);
        let mut full: &[u8] = b"launch at noon";
        let mut r = Noting { from: &mut full, left: 14, failed: false };
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
        w.set_content_methods(vec![sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from("hunter2")).into(), sevenz_rust2::EncoderMethod::LZMA2.into()]);
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::from_path(d.join("plan.txt"), "plan.txt".into()), Some(File::open(d.join("plan.txt")).unwrap())).unwrap();
        w.finish().unwrap();
        // The names are open: it lists, and says it is locked. Changing it needs the password.
        let (entries, locked) = listing(&zp, "").unwrap();
        assert!(locked && entries.len() == 2);
        assert_eq!(crate::fs::mkdir(&zp.join("docs")).unwrap_err().to_string(), LOCKED);
        assert_eq!(crate::fs::copy(&d.join("crew.txt"), &zp).unwrap_err().to_string(), LOCKED);
        assert_eq!(crate::fs::delete(&zp.join("plan.txt")).unwrap_err().to_string(), LOCKED);
        assert_eq!(crate::fs::mkdir_locked(&zp.join("docs"), Some("wrong")).unwrap_err().to_string(), LOCKED);
        assert!(list_in(&zp, "").unwrap().iter().all(|e| e.name != "docs"), "a failed change leaves the archive as it was");
        // With it, the change is made, and it is kept: the next change needs no password.
        crate::fs::mkdir_locked(&zp.join("docs"), Some("hunter2")).unwrap();
        crate::fs::copy(&d.join("crew.txt"), &zp.join("docs")).unwrap();
        crate::fs::rename(&zp.join("plan.txt"), &zp.join("docs")).unwrap();
        let names = |p: &Path| crate::fs::list(p, true).unwrap().into_iter().skip(1).map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names(&zp), ["docs"]);
        assert_eq!(names(&zp.join("docs")), ["crew.txt", "plan.txt"]);
        // Extraction reads it once, with the password; without one, the changed archive is as
        // locked as it was.
        forget(&zp);
        assert_eq!(extract(&zp, &d.join("out")).unwrap_err().to_string(), LOCKED);
        assert_eq!(list_in(&zp, "").unwrap().len(), 2, "the names stay open, as they were");
        let out = extract_locked(&zp, &d.join("out"), Some("hunter2")).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("docs/plan.txt")).unwrap(), "launch at noon");
        assert_eq!(std::fs::read_to_string(out.join("docs/crew.txt")).unwrap(), "four");
        crate::fs::delete(&zp.join("docs/crew.txt")).unwrap();
        assert_eq!(names(&zp.join("docs")), ["plan.txt"]);
        std::fs::remove_dir_all(d).unwrap();
    }
}

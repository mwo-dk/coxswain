//! Archives: zip (and the formats that are zips: jar, apk, whl, nupkg, vsix), tar, tar
//! compressed with gzip, bzip2, xz or zstd, and 7z. List, extract, and inside them read and
//! write as in a folder. All in pure Rust. Entries that would land outside the destination
//! (`..`, absolute paths) are refused.

use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ArchiveEntry {
    /// Path inside the archive, `/`-separated.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
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
    let n = path.file_name()?.to_string_lossy().to_lowercase();
    ENDINGS.iter().find(|(e, _)| n.ends_with(e)).map(|(_, k)| *k)
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

/// A tar written plain at `tar`, compressed into `to` as `pack` says.
fn compress(tar: &Path, to: &Path, pack: Pack) -> io::Result<()> {
    let (mut from, out) = (BufReader::new(File::open(tar)?), File::create(to)?);
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
}

/// Forget the password given for `archive`.
pub fn forget(archive: &Path) {
    PASSWORDS.lock().unwrap().retain(|(a, _)| a != archive);
}

/// The password to use: the one given, else the one remembered.
fn password_for(archive: &Path, given: Option<&str>) -> Option<String> {
    given.map(String::from).or_else(|| PASSWORDS.lock().unwrap().iter().find(|(a, _)| a == archive).map(|(_, p)| p.clone()))
}

/// A 7z, opened with `password` (none: an empty one).
fn seven(path: &Path, password: Option<&str>) -> io::Result<sevenz_rust2::ArchiveReader<File>> {
    let pw = password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::from);
    sevenz_rust2::ArchiveReader::open(path, pw).map_err(seven_error)
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
    let all = items(path)?;
    let more = all.len() > max;
    Ok((all.into_iter().take(max).map(|it| ArchiveEntry { name: it.name, size: it.size, is_dir: it.dir }).collect(), more))
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
    // A locked zip goes entry by entry, with the password; the rest as the crates unpack it.
    let r = if k == Kind::SevenZ || (k == Kind::Zip && locked_at(path, "")?) {
        list_in(path, "")?.into_iter().skip(1).try_for_each(|e| copy_out(path, &e.name, &to, password).map(drop))
    } else if k == Kind::Zip {
        zip::ZipArchive::new(BufReader::new(File::open(path)?)).and_then(|mut z| z.extract(&to)).map_err(io::Error::other)
    } else {
        tar_reader(path, k)?.unpack(&to)
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

/// Seconds since the Unix epoch of a zip's date and time (which have no time zone).
fn unix(t: zip::DateTime) -> u64 {
    // Days from the civil date, as in Howard Hinnant's algorithm.
    let (y, m, d) = (t.year() as i64 - if t.month() <= 2 { 1 } else { 0 }, t.month() as i64, t.day() as i64);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    (days * 86_400 + t.hour() as i64 * 3600 + t.minute() as i64 * 60 + t.second() as i64).max(0) as u64
}

/// An entry of an archive: its path inside, size, whether a folder, modified (seconds), locked.
struct Item {
    name: String,
    size: u64,
    dir: bool,
    modified: u64,
    locked: bool,
}

fn items(archive: &Path) -> io::Result<Vec<Item>> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    let mut out = vec![];
    if k == Kind::Zip {
        let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
        for i in 0..z.len() {
            let e = z.by_index_raw(i).map_err(io::Error::other)?;
            out.push(Item { name: e.name().trim_end_matches('/').to_string(), size: e.size(), dir: e.is_dir(), modified: e.last_modified().map_or(0, unix), locked: e.encrypted() });
        }
        return Ok(out);
    }
    if k == Kind::SevenZ {
        let r = seven(archive, password_for(archive, None).as_deref())?;
        for f in &r.archive().files {
            let modified = std::time::SystemTime::from(f.last_modified_date).duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            out.push(Item { name: f.name.replace('\\', "/").trim_end_matches('/').to_string(), size: f.size, dir: f.is_directory, modified, locked: false });
        }
        return Ok(out);
    }
    for e in tar_reader(archive, k)?.entries()? {
        let e = e?;
        let name = e.path()?.to_string_lossy().trim_start_matches("./").trim_end_matches('/').to_string();
        out.push(Item { name, size: e.size(), dir: e.header().entry_type().is_dir(), modified: e.header().mtime().unwrap_or(0), locked: false });
    }
    Ok(out)
}

/// What is inside `archive` at `inner`, as a folder listing: its files, and the folders in it,
/// also those the archive only names in the paths of its files. `..` leads back out.
pub fn list_in(archive: &Path, inner: &str) -> io::Result<Vec<crate::fs::Entry>> {
    let at = if inner.is_empty() { archive.to_path_buf() } else { archive.join(inner) };
    let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
    let mut seen = std::collections::BTreeMap::new();
    for it in items(archive)? {
        let Some(rest) = it.name.strip_prefix(&prefix).filter(|r| !r.is_empty()) else { continue };
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
        });
        if deeper {
            e.is_dir = true;
        } else {
            (e.size, e.modified) = (it.size, it.modified);
        }
    }
    let up = at.parent().unwrap_or(archive).to_path_buf();
    let mut out = vec![crate::fs::Entry { name: "..".into(), path: up, is_dir: true, is_symlink: false, is_exec: false, hidden: false, size: 0, modified: 0, created: 0 }];
    out.extend(seen.into_values());
    Ok(out)
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
/// `password`; without it, or with a wrong one, the error is `LOCKED`.
pub fn copy_out(archive: &Path, inner: &str, dest: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    let to = copy_out_with(archive, inner, dest, password_for(archive, password).as_deref())?;
    // A password that opened it is kept for the rest of this run.
    if let Some(pw) = password {
        remember(archive, pw);
    }
    Ok(to)
}

fn copy_out_with(archive: &Path, inner: &str, dest: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    let name = inner.rsplit('/').next().filter(|n| !n.is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to copy"))?;
    let to = if dest.is_dir() { dest.join(name) } else { dest.to_path_buf() };
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    let prefix = format!("{inner}/");
    // Where an entry lands, never outside `to`: `..` and absolute parts are refused.
    let place = |name: &str| -> Option<PathBuf> {
        let rest = if name == inner { "" } else { name.strip_prefix(&prefix)? };
        let mut p = to.clone();
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            if part == ".." || part.contains(['\\', ':']) {
                return None;
            }
            p.push(part);
        }
        Some(p)
    };
    let write = |path: &Path, from: &mut dyn Read| -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        io::copy(from, &mut File::create(path)?).map(drop)
    };
    let mut any = false;
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    if k == Kind::Zip {
        let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
        for i in 0..z.len() {
            let (name, dir, encrypted) = {
                let e = z.by_index_raw(i).map_err(io::Error::other)?;
                (e.name().trim_end_matches('/').to_string(), e.is_dir(), e.encrypted())
            };
            let Some(path) = place(&name) else { continue };
            any = true;
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
                let _ = std::fs::remove_file(&path);
                return Err(if encrypted { locked() } else { err });
            }
        }
    } else if k == Kind::SevenZ {
        let mut r = seven(archive, password)?;
        let mut failed = None;
        r.for_each_entries(|e, from| {
            let name = e.name.replace('\\', "/").trim_end_matches('/').to_string();
            let Some(path) = place(&name) else { return Ok(true) };
            any = true;
            let done = if e.is_directory { std::fs::create_dir_all(&path) } else { write(&path, from) };
            if let Err(err) = done {
                failed = Some(err);
                return Ok(false);
            }
            Ok(true)
        })
        .map_err(seven_error)?;
        if let Some(err) = failed {
            return Err(err);
        }
    } else {
        for e in tar_reader(archive, k)?.entries()? {
            let mut e = e?;
            let name = e.path()?.to_string_lossy().trim_start_matches("./").trim_end_matches('/').to_string();
            let Some(path) = place(&name) else { continue };
            any = true;
            if e.header().entry_type().is_dir() {
                std::fs::create_dir_all(&path)?;
            } else if e.header().entry_type().is_file() {
                write(&path, &mut e)?;
            }
        }
    }
    if !any {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{inner} is not in {}", archive.display())));
    }
    Ok(to)
}

/// What goes into an archive being written: a file from disk, or a folder.
enum New {
    File(PathBuf),
    Dir,
}

/// A zip's date and time from seconds since the Unix epoch.
fn zip_time(secs: u64) -> zip::DateTime {
    // The civil date from days, as in Howard Hinnant's algorithm.
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = ((doy - (153 * mp + 2) / 5 + 1) as u8, if mp < 10 { mp + 3 } else { mp - 9 } as u8);
    let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as u16;
    let s = secs % 86_400;
    zip::DateTime::from_date_and_time(y, m, d, (s / 3600) as u8, (s % 3600 / 60) as u8, (s % 60) as u8).unwrap_or_default()
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// Write `archive` anew: each old entry by what `keep` says of its name (`None`: left out,
/// `Some(name)`: kept under that name, as it was, locked or not), then `add`. Into a new
/// file first, which then takes the old one's place, so a failure leaves the archive whole.
fn rewrite(archive: &Path, keep: &dyn Fn(&str) -> Option<String>, add: &[(String, New)]) -> io::Result<()> {
    let k = kind(archive).ok_or_else(|| not_archive(archive))?;
    let tmp = archive.with_extension("coxswain-tmp");
    let exists = archive.exists();
    let written = (|| -> io::Result<()> {
        if k == Kind::Zip {
            let mut w = zip::ZipWriter::new(File::create(&tmp)?);
            if exists {
                let mut z = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
                for i in 0..z.len() {
                    let e = z.by_index_raw(i).map_err(io::Error::other)?;
                    let old = e.name().trim_end_matches('/').to_string();
                    let slash = e.name().ends_with('/');
                    match keep(&old) {
                        Some(new) if new == old => w.raw_copy_file(e),
                        Some(new) => w.raw_copy_file_rename(e, if slash { format!("{new}/") } else { new }),
                        None => Ok(()),
                    }
                    .map_err(io::Error::other)?;
                }
            }
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, new) in add {
                match new {
                    New::Dir => w.add_directory(format!("{name}/"), opts).map_err(io::Error::other)?,
                    New::File(path) => {
                        w.start_file(name.as_str(), opts.last_modified_time(zip_time(modified(path)))).map_err(io::Error::other)?;
                        io::copy(&mut File::open(path)?, &mut w)?;
                    }
                }
            }
            w.finish().map_err(io::Error::other)?;
        } else if k == Kind::SevenZ {
            // 7z has no copying of entries as they are: everything is read out, then packed again.
            let scratch = std::env::temp_dir().join(format!("coxswain-7z-{}-{}", std::process::id(), tmp.file_name().unwrap_or_default().len()));
            let _ = std::fs::remove_dir_all(&scratch);
            std::fs::create_dir_all(&scratch)?;
            let result = (|| -> io::Result<()> {
                let mut kept: Vec<(String, New)> = vec![];
                if exists {
                    let mut r = seven(archive, password_for(archive, None).as_deref())?;
                    let mut n = 0usize;
                    let mut failed = None;
                    r.for_each_entries(|e, from| {
                        let old = e.name.replace('\\', "/").trim_end_matches('/').to_string();
                        let Some(new) = keep(&old) else { return Ok(true) };
                        if e.is_directory {
                            kept.push((new, New::Dir));
                        } else {
                            n += 1;
                            let file = scratch.join(n.to_string());
                            match File::create(&file).and_then(|mut f| io::copy(from, &mut f)) {
                                Ok(_) => kept.push((new, New::File(file))),
                                Err(err) => {
                                    failed = Some(err);
                                    return Ok(false);
                                }
                            }
                        }
                        Ok(true)
                    })
                    .map_err(seven_error)?;
                    if let Some(err) = failed {
                        return Err(err);
                    }
                }
                let mut w = sevenz_rust2::ArchiveWriter::create(&tmp).map_err(io::Error::other)?;
                for (name, new) in kept.iter().chain(add) {
                    match new {
                        New::Dir => w.push_archive_entry::<File>(sevenz_rust2::ArchiveEntry::new_directory(name), None),
                        New::File(path) => w.push_archive_entry(sevenz_rust2::ArchiveEntry::from_path(path, name.clone()), Some(File::open(path)?)),
                    }
                    .map_err(io::Error::other)?;
                }
                w.finish().map(drop)
            })();
            let _ = std::fs::remove_dir_all(&scratch);
            result?;
        } else {
            let Kind::Tar(pack) = k else { unreachable!() };
            // Written plain first, then compressed: every compression the same way.
            let plain = archive.with_extension("coxswain-tar");
            let mut b = tar::Builder::new(io::BufWriter::new(File::create(&plain)?));
            if exists {
                for e in tar_reader(archive, k)?.entries()? {
                    let mut e = e?;
                    let old = e.path()?.to_string_lossy().trim_start_matches("./").trim_end_matches('/').to_string();
                    if let Some(new) = keep(&old) {
                        let mut header = e.header().clone();
                        if new != old {
                            header.set_path(&new)?;
                            header.set_cksum();
                        }
                        b.append(&header, &mut e)?;
                    }
                }
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
    match written {
        Ok(()) => std::fs::rename(&tmp, archive),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Take `inner` (files, or folders and everything in them) out of the archive: it is written
/// anew without them, the rest as it was (a locked file stays locked; no password is needed).
pub fn remove(archive: &Path, inner: &[String]) -> io::Result<()> {
    let gone = |name: &str| inner.iter().any(|i| name == i || name.starts_with(&format!("{i}/")));
    rewrite(archive, &|name| (!gone(name)).then(|| name.to_string()), &[])
}

/// The entries `sources` (files, or folders and everything in them) become under `inner`.
fn entries_of(inner: &str, sources: &[PathBuf]) -> io::Result<Vec<(String, New)>> {
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
    for src in sources {
        let name = src.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to add"))?.to_string_lossy().into_owned();
        walk(src, if inner.is_empty() { name } else { format!("{inner}/{name}") }, &mut out)?;
    }
    Ok(out)
}

/// Copy `sources` from disk into `archive`, into its folder `inner`. What is there by a name
/// already is kept and the copy refused, as a copy between folders never overwrites.
pub fn add(archive: &Path, inner: &str, sources: &[PathBuf]) -> io::Result<()> {
    let new = entries_of(inner, sources)?;
    let have: std::collections::HashSet<String> = items(archive)?.into_iter().map(|it| it.name).collect();
    if let Some((name, _)) = new.iter().find(|(name, n)| !matches!(n, New::Dir) && have.contains(name)) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{name} exists in {}", archive.display())));
    }
    // A folder the archive has already is not written twice.
    let new: Vec<_> = new.into_iter().filter(|(name, n)| !(matches!(n, New::Dir) && have.contains(name))).collect();
    rewrite(archive, &|name| Some(name.to_string()), &new)
}

/// A new, empty folder inside the archive.
pub fn mkdir(archive: &Path, inner: &str) -> io::Result<()> {
    if items(archive)?.iter().any(|it| it.name == inner) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{inner} exists")));
    }
    rewrite(archive, &|name| Some(name.to_string()), &[(inner.to_string(), New::Dir)])
}

/// Rename or move `from` (a file, or a folder and all in it) to `to`, inside the archive.
pub fn rename_in(archive: &Path, from: &str, to: &str) -> io::Result<()> {
    let names: Vec<String> = items(archive)?.into_iter().map(|it| it.name).collect();
    if names.iter().any(|n| n == to || n.starts_with(&format!("{to}/"))) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{to} exists")));
    }
    let prefix = format!("{from}/");
    rewrite(archive, &|name| Some(if name == from { to.to_string() } else if let Some(rest) = name.strip_prefix(&prefix) { format!("{to}/{rest}") } else { name.to_string() }), &[])
}

/// A new archive at `path` (its kind by its name: .zip, .tar, .tar.gz / .tgz) with `sources`.
pub fn create(path: &Path, sources: &[PathBuf]) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", path.display())));
    }
    kind(path).ok_or_else(|| not_archive(path))?;
    rewrite(path, &|_| None, &entries_of("", sources)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(entries[1], ArchiveEntry { name: "sub/a.txt".into(), size: 5, is_dir: false });
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
        assert_eq!(list(&tp, 10).unwrap().0, [ArchiveEntry { name: "x/a.txt".into(), size: 5, is_dir: false }]);
        assert_eq!(std::fs::read_to_string(extract(&tp, &d).unwrap().join("x/a.txt")).unwrap(), "hello");

        assert!(list(&d.join("pack/sub/a.txt"), 10).is_err());
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
        forget(&zp);
        assert!(list_in(&zp, "").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}

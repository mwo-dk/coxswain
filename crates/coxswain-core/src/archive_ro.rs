//! Archives that are read, never written: cpio (its newc and odc headers), a Debian package
//! (`.deb`, an ar of its control and data tars), an RPM package (its cpio, unpacked from gzip,
//! bzip2, xz, zstd or lzma) and a Windows cabinet or a set of them (`.cab`, `archive_cab`). Each is read as it
//! streams by, an entry at a time. Sizes in a header are only read up to, never allocated; a
//! name is `NAME_MAX` bytes at most. Names come out `/`-separated, without a `./` or `/` in
//! front; `archive` leaves out those that could lead outside it.

use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Format {
    Cpio,
    /// A cpio packed in gzip, bzip2, xz, zstd or lzma (`.cpio.gz`): found by its first bytes.
    CpioPacked,
    Deb,
    Rpm,
    Cab,
}

pub(crate) struct Entry {
    pub name: String,
    pub size: u64,
    pub dir: bool,
    /// A plain file (not a folder, a link or a device): only those have bytes to read.
    pub file: bool,
    /// Seconds since the Unix epoch; 0 when the archive does not say.
    pub modified: u64,
    /// Its permission bits, as far as the archive says: whether it runs.
    pub mode: u32,
}

/// The longest name read from a header.
const NAME_MAX: u64 = 4096;

/// Each entry of the archive `r`, in order, until `f` says enough. `f` gets a file's bytes when
/// `wanted` picks it, else nothing to read: in a cabinet a file not wanted is never unpacked.
/// Stop is looked at on every read.
pub(crate) fn each<R: Read + Seek + 'static>(
    r: R,
    at: &Path,
    format: Format,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let r = crate::stop::Reader(r);
    match format {
        Format::Cpio => cpio(&mut BufReader::new(r), seek_past, wanted, f),
        Format::CpioPacked => unpacked(BufReader::new(r), wanted, f),
        Format::Deb => ar(&mut BufReader::new(r), wanted, f),
        Format::Rpm => rpm(BufReader::new(r), wanted, f),
        // A crafted cabinet must not take the app down with it.
        Format::Cab => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::archive_cab::each(Box::new(r), &|name| sibling(at, name), wanted, f)
        }))
            .unwrap_or_else(|_| Err(bad("a broken cabinet"))),
    }
}

pub(crate) fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

/// The cabinet named `name` beside the one opened (`at`): by its own spelling, else however its
/// letters are cased.
fn sibling(at: &Path, name: &str) -> io::Result<Box<dyn crate::archive_cab::Src>> {
    let dir = at.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut path = dir.join(name);
    if !path.is_file() {
        let found = std::fs::read_dir(dir)?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name));
        path = found.ok_or(io::ErrorKind::NotFound)?.path();
    }
    Ok(Box::new(crate::stop::Reader(std::fs::File::open(path)?)))
}

fn seek_past<R: Read + Seek>(r: &mut BufReader<R>, n: u64) -> io::Result<()> {
    r.seek_relative(i64::try_from(n).map_err(|_| bad("a size past the end"))?)
}

fn read_past<R: Read>(r: &mut R, n: u64) -> io::Result<()> {
    if io::copy(&mut r.take(n), &mut io::sink())? < n {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

/// `name` as a path inside: `\` as `/`, no `./` or `/` in front, none at the end.
pub(crate) fn clean(name: &str) -> String {
    let mut n = name.trim_end_matches('/');
    loop {
        let next = n.trim_start_matches('/');
        let next = next.strip_prefix("./").unwrap_or(next);
        if next == n {
            break;
        }
        n = next;
    }
    if n == "." { String::new() } else { n.to_string() }
}

/// The number written in `digits` (ASCII, in `radix`, spaces around it allowed).
fn number(digits: &[u8], radix: u32) -> io::Result<u64> {
    let s = std::str::from_utf8(digits).map_err(|_| bad("a broken header"))?.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, radix).map_err(|_| bad("a broken header"))
}

/// Hands the entry `e`, `size` bytes of `r` from here, to `f`, then reads past what it left.
fn hand<R: Read>(
    r: &mut R,
    e: &Entry,
    size: u64,
    skip: fn(&mut R, u64) -> io::Result<()>,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<bool> {
    let open = e.file && wanted(e);
    let mut body = (&mut *r).take(if open { size } else { 0 });
    let go = if e.name.is_empty() { true } else { f(e, &mut body)? };
    let left = if open { body.limit() } else { size };
    skip(r, left)?;
    Ok(go)
}

/// A cpio's entries: "newc" (`070701`, `070702`, as RPM and Linux's initramfs write) or "odc"
/// (`070707`, POSIX), up to `TRAILER!!!`.
fn cpio<R: Read>(
    r: &mut R,
    skip: fn(&mut R, u64) -> io::Result<()>,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    loop {
        let mut magic = [0u8; 6];
        // One cut short before its trailer ends there: what came before it stands.
        match fill(r, &mut magic)? {
            0 => return Ok(()),
            6 => {}
            _ => return Err(io::ErrorKind::UnexpectedEof.into()),
        }
        // Each field as numbers, how a name and the bytes after it are padded, and how long the
        // header is. `linked`: a newc hard link whose bytes come with another of its names.
        let (mode, mtime, size, name_len, align, header, linked) = match &magic {
            b"070701" | b"070702" => {
                let mut h = [0u8; 104];
                r.read_exact(&mut h)?;
                let x = |i: usize| number(&h[i * 8..i * 8 + 8], 16);
                (x(1)?, x(5)?, x(6)?, x(11)?, 4, 110, x(4)? > 1 && x(6)? == 0)
            }
            b"070707" => {
                let mut h = [0u8; 70];
                r.read_exact(&mut h)?;
                let o = |a: usize, b: usize| number(&h[a..b], 8);
                (o(12, 18)?, o(42, 53)?, o(59, 70)?, o(53, 59)?, 1, 76, false)
            }
            // The binary header (cpio without -c): 16-bit words in the byte order the magic
            // 0o070707 shows, a time and a size as two words, the high one first.
            [0xc7, 0x71, ..] | [0x71, 0xc7, ..] => {
                let big = magic[0] == 0x71;
                let mut h = [0u8; 26];
                h[..6].copy_from_slice(&magic);
                r.read_exact(&mut h[6..])?;
                let w = |i: usize| u64::from(if big { u16::from_be_bytes([h[i], h[i + 1]]) } else { u16::from_le_bytes([h[i], h[i + 1]]) });
                (w(6), w(16) << 16 | w(18), w(22) << 16 | w(24), w(20), 2, 26, false)
            }
            _ => return Err(bad("not a cpio header")),
        };
        if name_len == 0 || name_len > NAME_MAX {
            return Err(bad("a cpio name too long"));
        }
        let mut name = vec![0u8; name_len as usize];
        r.read_exact(&mut name)?;
        let pad = |n: u64| (align - n % align) % align;
        skip(r, pad(header + name_len))?;
        let name = String::from_utf8_lossy(name.strip_suffix(&[0]).unwrap_or(&name)).into_owned();
        if name == "TRAILER!!!" {
            return Ok(());
        }
        // A hard link without its bytes is a link here, not an empty file.
        let t = if linked { 0 } else { mode & 0o170000 };
        let e = Entry {
            name: clean(&name),
            size: if t == 0o100000 { size } else { 0 },
            dir: t == 0o040000,
            file: t == 0o100000,
            modified: mtime,
            mode: (mode & 0o777) as u32,
        };
        if !hand(r, &e, size, skip, wanted, f)? {
            return Ok(());
        }
        skip(r, pad(size))?;
    }
}

/// Reads into `buf` until it is full or the input ends: how many bytes came.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// An ar's members (a Debian package: `debian-binary`, `control.tar.*`, `data.tar.*`).
fn ar<R: Read + Seek>(
    r: &mut BufReader<R>,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != b"!<arch>\n" {
        return Err(bad("not an ar archive"));
    }
    loop {
        let mut h = [0u8; 60];
        match fill(r, &mut h)? {
            0 => return Ok(()),
            60 if &h[58..60] == b"`\n" => {}
            _ => return Err(bad("a broken ar header")),
        }
        let size = number(&h[48..58], 10)?;
        let name = String::from_utf8_lossy(&h[..16]);
        // GNU's names end in `/`; its symbol table (`/`) and long names (`//`) are no files.
        let e = Entry {
            name: clean(name.trim_end()),
            size,
            dir: false,
            file: true,
            modified: number(&h[16..28], 10)?,
            mode: (number(&h[40..48], 8)? & 0o777) as u32,
        };
        if !hand(r, &e, size, seek_past, wanted, f)? {
            return Ok(());
        }
        seek_past(r, size % 2)?;
    }
}

/// An RPM package's files: past its lead and its two headers, its payload, a cpio, unpacked
/// as it is read.
fn rpm<R: Read + Seek>(
    mut r: BufReader<R>,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let mut lead = [0u8; 96];
    r.read_exact(&mut lead)?;
    if lead[..4] != [0xed, 0xab, 0xee, 0xdb] {
        return Err(bad("not an RPM package"));
    }
    // The signature header, padded to 8 bytes, then the header: each its index and its data.
    for padded in [true, false] {
        let mut h = [0u8; 16];
        r.read_exact(&mut h)?;
        if h[..4] != [0x8e, 0xad, 0xe8, 0x01] {
            return Err(bad("a broken RPM header"));
        }
        let n = u64::from(u32::from_be_bytes([h[8], h[9], h[10], h[11]]));
        let data = u64::from(u32::from_be_bytes([h[12], h[13], h[14], h[15]]));
        let len = n * 16 + data;
        seek_past(&mut r, len + if padded { (8 - len % 8) % 8 } else { 0 })?;
    }
    unpacked(r, wanted, f)
}

/// The cpio in `r`, unpacked as it is read from gzip, bzip2, xz, zstd or lzma, or as it is.
fn unpacked<R: Read>(
    mut r: R,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let mut magic = [0u8; 6];
    r.read_exact(&mut magic)?;
    let r = io::Cursor::new(magic).chain(r);
    let mut payload: Box<dyn Read> = match magic {
        [0x1f, 0x8b, ..] => Box::new(flate2::read::MultiGzDecoder::new(r)),
        [b'B', b'Z', b'h', ..] => Box::new(bzip2::read::MultiBzDecoder::new(r)),
        [0xfd, b'7', b'z', b'X', b'Z', 0] => Box::new(lzma_rust2::XzReader::new(r, true)),
        [0x28, 0xb5, 0x2f, 0xfd, ..] => Box::new(crate::tar_index::Zst::new(BufReader::new(r), None)),
        // Plain lzma ("lzma alone", RPM before 2010): properties byte 0x5d, as `lzma` writes it
        // by default. Its dictionary is capped, so a header cannot ask for gigabytes.
        [0x5d, ..] => Box::new(lzma_rust2::LzmaReader::new_mem_limit(r, LZMA_MEMORY_KB, None)?),
        [b'0', b'7', b'0', b'7', ..] => Box::new(r),
        [0xc7, 0x71, ..] | [0x71, 0xc7, ..] => Box::new(r),
        _ => return Err(bad("a payload packed in a way Coxswain does not read")),
    };
    cpio(&mut payload, read_past, wanted, f)
}

/// The most memory (KiB) a plain lzma stream may ask for.
const LZMA_MEMORY_KB: u32 = 256 << 10;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// A newc cpio of `files` (name, bytes; a name ending in `/` a folder, one in `.sh` a
    /// program).
    pub(crate) fn newc(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = vec![];
        let put = |name: &str, mode: u32, data: &[u8], out: &mut Vec<u8>| {
            let fields = [1, mode, 0, 0, 1, 1_700_000_000, data.len() as u32, 0, 0, 0, 0, name.len() as u32 + 1, 0];
            out.extend_from_slice(b"070701");
            for v in fields {
                out.extend_from_slice(format!("{v:08x}").as_bytes());
            }
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            out.extend_from_slice(data);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        };
        for (name, data) in files {
            match name.strip_suffix('/') {
                Some(dir) => put(dir, 0o040755, b"", &mut out),
                None => put(name, if name.ends_with(".sh") { 0o100755 } else { 0o100644 }, data, &mut out),
            }
        }
        put("TRAILER!!!", 0, b"", &mut out);
        out
    }

    /// An odc cpio of `files`.
    fn odc(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = vec![];
        let all = files.iter().map(|(n, d)| (*n, *d, 0o100644)).chain([("TRAILER!!!", &b""[..], 0)]);
        for (name, data, mode) in all {
            write!(out, "070707{:06o}{:06o}{:06o}{:06o}{:06o}{:06o}{:06o}{:011o}{:06o}{:011o}", 0, 1, mode, 0, 0, 1, 0, 1_700_000_000u64, name.len() + 1, data.len()).unwrap();
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.extend_from_slice(data);
        }
        out
    }

    /// A cpio in the old binary header, in the byte order of a big or a little machine.
    fn binary(files: &[(&str, &[u8])], big: bool) -> Vec<u8> {
        let w = |v: u32, out: &mut Vec<u8>| out.extend_from_slice(&if big { (v as u16).to_be_bytes() } else { (v as u16).to_le_bytes() });
        let mut out = vec![];
        let all = files.iter().map(|(n, d)| (n.strip_suffix('/').unwrap_or(n), *d, if n.ends_with('/') { 0o040755 } else { 0o100644 })).chain([("TRAILER!!!", &b""[..], 0)]);
        for (name, data, mode) in all {
            for v in [0o070707, 0, 1, mode, 0, 0, 1, 0, 1_700_000_000 >> 16, 1_700_000_000 & 0xffff, name.len() as u32 + 1, data.len() as u32 >> 16, data.len() as u32 & 0xffff] {
                w(v, &mut out);
            }
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            if out.len() % 2 == 1 {
                out.push(0);
            }
            out.extend_from_slice(data);
            if out.len() % 2 == 1 {
                out.push(0);
            }
        }
        out
    }

    /// An ar of `members`, as a Debian package is one.
    pub(crate) fn ar_of(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = b"!<arch>\n".to_vec();
        for (name, data) in members {
            write!(out, "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", name, 1_700_000_000, 0, 0, 100644, data.len()).unwrap();
            out.extend_from_slice(data);
            if data.len() % 2 == 1 {
                out.push(b'\n');
            }
        }
        out
    }

    /// An RPM package around `payload` (its cpio, packed as it is).
    pub(crate) fn rpm_of(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xed, 0xab, 0xee, 0xdb];
        out.resize(96, 0);
        // A signature of one index entry and 5 bytes of data (padded to 8), a header of none.
        for (n, data) in [(1u32, 5u32), (0, 3)] {
            out.extend_from_slice(&[0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0]);
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&data.to_be_bytes());
            out.resize(out.len() + (n * 16 + data) as usize, 7);
            if n == 1 {
                while out.len() % 8 != 0 {
                    out.push(0);
                }
            }
        }
        out.extend_from_slice(payload);
        out
    }

    pub(crate) fn gz(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// A cabinet of `files`, packed with MSZIP.
    pub(crate) fn cab_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = cab::CabinetBuilder::new();
        let folder = b.add_folder(cab::CompressionType::MsZip);
        for (name, _) in files {
            folder.add_file(*name);
        }
        let mut w = b.build(Cursor::new(vec![])).unwrap();
        let mut i = 0;
        while let Some(mut f) = w.next_file().unwrap() {
            f.write_all(files[i].1).unwrap();
            i += 1;
        }
        w.finish().unwrap().into_inner()
    }

    fn read_all(bytes: Vec<u8>, format: Format) -> io::Result<Vec<(String, bool, Vec<u8>)>> {
        let mut out = vec![];
        each(Cursor::new(bytes), Path::new("t"), format, &|_| true, &mut |e, r| {
            let mut data = vec![];
            r.read_to_end(&mut data)?;
            out.push((e.name.clone(), e.dir, data));
            Ok(true)
        })?;
        Ok(out)
    }

    fn names(got: &[(String, bool, Vec<u8>)]) -> Vec<&str> {
        got.iter().map(|g| g.0.as_str()).collect()
    }

    #[test]
    fn cpio_newc_and_odc_are_read() {
        let files: &[(&str, &[u8])] = &[("./usr/", b""), ("./usr/a.txt", b"hello"), ("b", b"odd!!")];
        let got = read_all(newc(files), Format::Cpio).unwrap();
        assert_eq!(names(&got), ["usr", "usr/a.txt", "b"]);
        assert!(got[0].1);
        assert_eq!(got[1].2, b"hello");
        assert_eq!(got[2].2, b"odd!!");
        let got = read_all(odc(&[("x/y.txt", b"one"), ("z", b"twotwo")]), Format::Cpio).unwrap();
        assert_eq!(names(&got), ["x/y.txt", "z"]);
        assert_eq!(got[1].2, b"twotwo");
    }

    #[test]
    fn a_file_not_wanted_or_half_read_is_passed_over() {
        let bytes = newc(&[("a", b"0123456789"), ("b", b"after")]);
        let mut seen = vec![];
        each(Cursor::new(bytes), Path::new("t"), Format::Cpio, &|e| e.name == "b", &mut |e, r| {
            let mut one = [0u8; 2];
            let n = r.read(&mut one)?;
            seen.push((e.name.clone(), n));
            Ok(true)
        })
        .unwrap();
        assert_eq!(seen, [("a".to_string(), 0), ("b".to_string(), 2)]);
    }

    #[test]
    fn hostile_headers_are_refused() {
        // A name said to be 4 GB long.
        let mut bytes = newc(&[("a", b"x")]);
        bytes[6 + 11 * 8..6 + 12 * 8].copy_from_slice(b"ffffffff");
        assert!(read_all(bytes, Format::Cpio).is_err());
        // A file said to be larger than the archive: what is there, no hang and no huge buffer;
        // a copy out says it ends early (`archive::Noting`).
        let mut bytes = newc(&[("a", b"x")]);
        bytes[6 + 6 * 8..6 + 7 * 8].copy_from_slice(b"7fffffff");
        let got = read_all(bytes, Format::Cpio).unwrap();
        assert!(got.len() == 1 && got[0].2.len() < 200);
        assert!(read_all(b"garbage!".to_vec(), Format::Deb).is_err());
        assert!(read_all(vec![0; 200], Format::Rpm).is_err());
        assert!(read_all(vec![b'M', b'S', b'C', b'F', 0, 0, 0, 0, 0xff], Format::Cab).is_err());
        // Names that would lead outside come out as they are, for `archive` to leave out.
        let got = read_all(newc(&[("../evil", b"x"), ("/etc/passwd", b"y")]), Format::Cpio).unwrap();
        assert_eq!(names(&got), ["../evil", "etc/passwd"]);
    }

    #[test]
    fn a_deb_shows_its_members() {
        let data = gz(b"not really a tar");
        let got = read_all(ar_of(&[("debian-binary", b"2.0\n"), ("control.tar.gz", &data), ("data.tar.gz/", b"odd")]), Format::Deb).unwrap();
        assert_eq!(names(&got), ["debian-binary", "control.tar.gz", "data.tar.gz"]);
        assert_eq!(got[1].2, data);
        assert_eq!(got[2].2, b"odd");
    }

    #[test]
    fn a_cpio_packed_or_in_the_old_binary_header_is_read() {
        let files: &[(&str, &[u8])] = &[("./a/", b""), ("./a/odd.txt", b"seven!!"), ("even", b"four")];
        let want = ["a", "a/odd.txt", "even"];
        let check = |bytes: Vec<u8>, format| {
            let got = read_all(bytes, format).unwrap();
            assert_eq!(names(&got), want);
            assert_eq!(got[1].2, b"seven!!");
            assert_eq!(got[2].2, b"four");
        };
        check(gz(&newc(files)), Format::CpioPacked);
        let mut xz = lzma_rust2::XzWriter::new(vec![], lzma_rust2::XzOptions::default()).unwrap();
        xz.write_all(&newc(files)).unwrap();
        check(xz.finish().unwrap(), Format::CpioPacked);
        check(newc(files), Format::CpioPacked);
        for big in [false, true] {
            check(binary(files, big), Format::Cpio);
            check(gz(&binary(files, big)), Format::CpioPacked);
        }
        assert!(read_all(b"nothing known".to_vec(), Format::CpioPacked).is_err());
    }

    #[test]
    fn an_rpm_packed_in_plain_lzma_is_read() {
        let cpio = newc(&[("./usr/bin/tool", b"#!/bin/sh\n")]);
        let mut w = lzma_rust2::LzmaWriter::new(vec![], &lzma_rust2::LzmaOptions::default(), true, false, Some(cpio.len() as u64)).unwrap();
        w.write_all(&cpio).unwrap();
        let got = read_all(rpm_of(&w.finish().unwrap()), Format::Rpm).unwrap();
        assert_eq!(names(&got), ["usr/bin/tool"]);
        // A header asking for a dictionary of 4 GB is refused, not allocated.
        let mut hostile = vec![0x5d];
        hostile.extend_from_slice(&u32::MAX.to_le_bytes());
        hostile.extend_from_slice(&[0xff; 20]);
        assert!(read_all(rpm_of(&hostile), Format::Rpm).is_err());
    }

    #[test]
    fn an_rpm_shows_its_files() {
        let cpio = newc(&[("./usr/bin/", b""), ("./usr/bin/tool", b"#!/bin/sh\n")]);
        for payload in [gz(&cpio), cpio.clone()] {
            let got = read_all(rpm_of(&payload), Format::Rpm).unwrap();
            assert_eq!(names(&got), ["usr/bin", "usr/bin/tool"]);
            assert_eq!(got[1].2, b"#!/bin/sh\n");
        }
    }

    #[test]
    fn a_cpio_cut_short_keeps_what_came_and_links_have_no_bytes() {
        let mut bytes = newc(&[("a", b"one"), ("b", b"two")]);
        bytes.truncate(bytes.len() - 124);
        assert_eq!(names(&read_all(bytes, Format::Cpio).unwrap()), ["a", "b"]);
        // A hard link whose bytes come with its last name: nlink 2, no bytes, so not a file.
        let mut bytes = newc(&[("first", b"")]);
        bytes[6 + 4 * 8..6 + 5 * 8].copy_from_slice(b"00000002");
        let mut files = vec![];
        each(Cursor::new(bytes), Path::new("t"), Format::Cpio, &|_| true, &mut |e, _| {
            files.push(e.file);
            Ok(true)
        })
        .unwrap();
        assert_eq!(files, [false]);
    }

    #[test]
    fn many_files_out_of_one_large_cab_folder_are_read_in_one_pass() {
        let big = vec![7u8; 400 << 10];
        let files: Vec<(String, Vec<u8>)> = (0..4).map(|i| (format!("f{i}"), big.clone())).collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let got = read_all(cab_of(&refs), Format::Cab).unwrap();
        assert!(got.iter().all(|g| g.2 == big), "each file, once, from one pass");
    }

    #[test]
    fn a_cab_asked_for_more_than_cab_work_stops() {
        let big = vec![7u8; 1 << 20];
        let bytes = cab_of(&[("a", &big), ("b", &big), ("c", &big)]);
        assert!(read_all(bytes, Format::Cab).is_err());
    }

    #[test]
    fn a_cab_shows_its_files() {
        let got = read_all(cab_of(&[("readme.txt", b"hi there"), ("sub\\b.bin", &[1, 2, 3])]), Format::Cab).unwrap();
        assert_eq!(names(&got), ["readme.txt", "sub/b.bin"]);
        assert_eq!(got[0].2, b"hi there");
        assert_eq!(got[1].2, [1, 2, 3]);
    }
}

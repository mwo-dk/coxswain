//! A zip's central directory read as it is gone through, an entry at a time, never built whole:
//! a zip of 200,000 entries is listed holding only its list, and one file of it is found and
//! unpacked without building that list at all (the `zip` crate builds the whole directory, with
//! a few hundred bytes for each entry, every time a zip is opened).
//!
//! Only what this reads exactly as the `zip` crate does is read here: a name that is no UTF-8,
//! or one given again in a Unicode path field, a split zip, or a directory that does not add up
//! is `Unsure`, and the caller opens the zip with the crate instead.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

/// An entry of the central directory.
pub(crate) struct Central {
    pub name: String,
    pub size: u64,
    pub packed: u64,
    pub crc: u32,
    /// Where its local header is, in the file.
    pub at: u64,
    method: u16,
    flags: u16,
    made_by: u16,
    external: u32,
    time: u16,
    date: u16,
}

impl Central {
    pub fn dir(&self) -> bool {
        self.name.ends_with('/') || self.name.ends_with('\\')
    }

    pub fn encrypted(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn modified(&self) -> Option<zip::DateTime> {
        zip::DateTime::try_from_msdos(self.date, self.time).ok()
    }

    /// Its Unix permission bits, when a Unix tool wrote it (as `zip`'s `unix_mode` has them).
    pub fn unix_mode(&self) -> Option<u32> {
        (self.made_by >> 8 == 3 && self.external != 0).then_some((self.external >> 16) & 0o777)
    }

    /// Where its bytes are in `file` when they are stored as they are (not packed, not locked):
    /// its start and length, past its local header. `None` for one packed or locked.
    pub fn stored<R: Read + Seek>(&self, file: &mut R) -> io::Result<Option<(u64, u64)>> {
        if self.method != 0 || self.encrypted() || self.size != self.packed {
            return Ok(None);
        }
        let mut local = [0u8; 30];
        file.seek(SeekFrom::Start(self.at))?;
        file.read_exact(&mut local)?;
        if local[..4] != *b"PK\x03\x04" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "no file at its place in the zip"));
        }
        let start = self.at + 30 + u16_at(&local, 26) as u64 + u16_at(&local, 28) as u64;
        let end = file.seek(SeekFrom::End(0))?;
        Ok((start.checked_add(self.size).is_some_and(|e| e <= end)).then_some((start, self.size)))
    }

    /// Whether a Unix tool wrote it as a symbolic link.
    pub fn link(&self) -> bool {
        self.made_by >> 8 == 3 && (self.external >> 16) & 0o170000 == 0o120000
    }

    /// The file's bytes, unpacked and checked, read from `file` at its local header. Put away
    /// before its end, the rest of its packed bytes are read past (not unpacked).
    // ponytail: a few hundred MB read past when a huge entry's text stops early; a reader that
    // stops reading when put away if that shows.
    pub fn open<'a, R: Read + Seek>(
        &self,
        file: &'a mut R,
    ) -> io::Result<zip::read::ZipFile<'a, R>> {
        file.seek(SeekFrom::Start(self.at))?;
        let options = zip::read::ZipReadOptions::new()
            .override_compressed_size(self.packed)
            .override_uncompressed_size(self.size)
            .override_crc(self.crc);
        zip::read::read_zipfile_from_stream_with_options(file, options)
            .map_err(io::Error::other)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "no file at its place in the zip",
                )
            })
    }
}

/// Not read here: the caller opens the zip with the `zip` crate.
pub(crate) fn unsure() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "zip read by the zip crate")
}

pub(crate) fn is_unsure(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Unsupported
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

/// Where the central directory starts, how long it is, how many entries it has, and how many
/// bytes come before the zip (a program it is appended to).
fn directory(file: &mut (impl Read + Seek)) -> io::Result<(u64, u64, u64, u64)> {
    let len = file.seek(SeekFrom::End(0))?;
    let tail = len.min(22 + 65_535);
    file.seek(SeekFrom::Start(len - tail))?;
    let mut buf = vec![0; tail as usize];
    file.read_exact(&mut buf)?;
    let end = (0..buf.len().saturating_sub(21))
        .rev()
        .find(|&i| buf[i..i + 4] == *b"PK\x05\x06")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a zip"))?;
    let e = &buf[end..];
    let at_end = len - tail + end as u64;
    let (disk, cd_disk) = (u16_at(e, 4), u16_at(e, 6));
    let (mut count, mut size, mut start) = (
        u16_at(e, 10) as u64,
        u32_at(e, 12) as u64,
        u32_at(e, 16) as u64,
    );
    let mut dir_end = at_end;
    if count == 0xFFFF || size == 0xFFFF_FFFF || start == 0xFFFF_FFFF {
        // Zip64: its locator just before, then its own end record.
        let loc = end.checked_sub(20).ok_or_else(unsure)?;
        if buf[loc..loc + 4] != *b"PK\x06\x07" {
            return Err(unsure());
        }
        let at64 = u64_at(&buf, loc + 8);
        let mut r = [0u8; 56];
        file.seek(SeekFrom::Start(at64))?;
        file.read_exact(&mut r)?;
        if r[..4] != *b"PK\x06\x06" {
            return Err(unsure());
        }
        (count, size, start) = (u64_at(&r, 32), u64_at(&r, 40), u64_at(&r, 48));
        dir_end = at64;
    }
    if disk != 0 || cd_disk != 0 {
        return Err(unsure());
    }
    // Bytes before the zip move every offset by as many.
    let before = dir_end
        .checked_sub(size)
        .and_then(|s| s.checked_sub(start))
        .ok_or_else(unsure)?;
    Ok((start + before, size, count, before))
}

/// Each entry of the zip in `file`, in the order of its directory, to `each` until it says to
/// stop. `Unsure` (`is_unsure`) when the `zip` crate should read it.
pub(crate) fn entries<R: Read + Seek>(
    file: &mut R,
    each: &mut dyn FnMut(Central) -> bool,
) -> io::Result<()> {
    let (start, size, count, before) = directory(file)?;
    file.seek(SeekFrom::Start(start))?;
    let mut r = BufReader::with_capacity(1 << 16, file.take(size));
    let mut head = [0u8; 46];
    let mut rest = vec![];
    for _ in 0..count {
        r.read_exact(&mut head).map_err(|_| unsure())?;
        if head[..4] != *b"PK\x01\x02" {
            return Err(unsure());
        }
        let (name_len, extra_len, comment_len) = (
            u16_at(&head, 28) as usize,
            u16_at(&head, 30) as usize,
            u16_at(&head, 32) as usize,
        );
        rest.resize(name_len + extra_len, 0);
        r.read_exact(&mut rest).map_err(|_| unsure())?;
        io::copy(&mut (&mut r).take(comment_len as u64), &mut io::sink())?;
        let (raw, extra) = rest.split_at(name_len);
        let name = std::str::from_utf8(raw).map_err(|_| unsure())?.to_string();
        let mut c = Central {
            name,
            size: u32_at(&head, 24) as u64,
            packed: u32_at(&head, 20) as u64,
            crc: u32_at(&head, 16),
            at: u32_at(&head, 42) as u64,
            method: u16_at(&head, 10),
            flags: u16_at(&head, 8),
            made_by: u16_at(&head, 4),
            external: u32_at(&head, 38),
            time: u16_at(&head, 12),
            date: u16_at(&head, 14),
        };
        // Extra fields: zip64 sizes and place, in this order, each only when its own is full.
        let mut x = extra;
        while x.len() >= 4 {
            let (id, len) = (u16_at(x, 0), u16_at(x, 2) as usize);
            let body = x.get(4..4 + len).ok_or_else(unsure)?;
            match id {
                0x0001 => {
                    let mut b = body;
                    let mut take = |v: &mut u64| -> io::Result<()> {
                        if *v == 0xFFFF_FFFF {
                            let (n, more) = b.split_first_chunk::<8>().ok_or_else(unsure)?;
                            *v = u64::from_le_bytes(*n);
                            b = more;
                        }
                        Ok(())
                    };
                    take(&mut c.size)?;
                    take(&mut c.packed)?;
                    take(&mut c.at)?;
                }
                // A name given again (Info-ZIP's Unicode path): the crate decides which.
                0x7075 => return Err(unsure()),
                _ => {}
            }
            x = &x[4 + len..];
        }
        c.at = c.at.checked_add(before).ok_or_else(unsure)?;
        if !each(c) {
            return Ok(());
        }
    }
    Ok(())
}

/// The entry named `name` (a file, not a folder), found by going through the directory.
pub(crate) fn find<R: Read + Seek>(file: &mut R, name: &str) -> io::Result<Option<Central>> {
    let mut found = None;
    entries(file, &mut |c| {
        if c.name == name && !c.dir() {
            found = Some(c);
            return false;
        }
        true
    })?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    /// The entries and their bytes are those the `zip` crate gives, in a zip with a program
    /// before it, in a zip64, stored and deflated, with and without data descriptors.
    #[test]
    fn entries_are_the_zip_crates() {
        let d = crate::extract::tests::folder("zip-dir");
        let mut cases = vec![];
        for (prefix, large) in [
            (&b""[..], false),
            (&b"#!/bin/sh\nexit 0\n"[..], false),
            (&b""[..], true),
        ] {
            let p = d.join(format!("case{}.zip", cases.len()));
            // A program before the zip, its offsets as if it were not there (`zip -A` fixes them).
            let mut bytes = prefix.to_vec();
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
            for i in 0..300 {
                let o = zip::write::SimpleFileOptions::default()
                    .compression_method(if i % 2 == 0 {
                        zip::CompressionMethod::Stored
                    } else {
                        zip::CompressionMethod::Deflated
                    })
                    .large_file(large)
                    .unix_permissions(if i % 3 == 0 { 0o755 } else { 0o644 });
                if i % 50 == 0 {
                    z.add_directory(format!("dir{i}/"), o).unwrap();
                }
                z.start_file(format!("dir{}/file {i} – ü.txt", i / 50 * 50), o)
                    .unwrap();
                z.write_all(format!("the text of file {i}\n").repeat(i).as_bytes())
                    .unwrap();
            }
            bytes.extend(z.finish().unwrap().into_inner());
            std::fs::write(&p, &bytes).unwrap();
            cases.push(p);
        }
        for p in cases {
            let mut crate_zip = zip::ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
            let mut f = std::fs::File::open(&p).unwrap();
            let mut ours = vec![];
            super::entries(&mut f, &mut |c| {
                ours.push(c);
                true
            })
            .unwrap();
            assert_eq!(ours.len(), crate_zip.len(), "{}", p.display());
            for (i, c) in ours.iter().enumerate() {
                let (name, dir, size, mode, when) = {
                    let e = crate_zip.by_index_raw(i).unwrap();
                    (
                        e.name().unwrap().to_string(),
                        e.is_dir(),
                        e.size(),
                        e.unix_mode(),
                        e.last_modified(),
                    )
                };
                assert_eq!(
                    (c.name.as_str(), c.dir(), c.size, c.modified()),
                    (name.as_str(), dir, size, when)
                );
                // What matters of the mode is a file's run bits (a zip written on Windows has a DOS one,
                // which the crate turns into a mode with none for a file).
                if !dir {
                    assert_eq!(c.unix_mode().map_or(0, |m| m & 0o111), mode.map_or(0, |m| m & 0o111), "{name}");
                }
                if !dir {
                    let mut want = vec![];
                    crate_zip
                        .by_index(i)
                        .unwrap()
                        .read_to_end(&mut want)
                        .unwrap();
                    let mut got = vec![];
                    c.open(&mut f).unwrap().read_to_end(&mut got).unwrap();
                    assert_eq!(got, want, "{name}");
                }
            }
            let last = super::find(&mut f, "dir250/file 299 – ü.txt")
                .unwrap()
                .unwrap();
            assert_eq!(last.size, "the text of file 299\n".len() as u64 * 299);
            assert!(
                super::find(&mut f, "dir250/").unwrap().is_none(),
                "a folder is no file"
            );
        }
        let _ = std::fs::remove_dir_all(d);
    }

    /// A name that is no UTF-8 is left to the crate, as is something that is no zip.
    #[test]
    fn what_it_cannot_be_sure_of_is_the_crates() {
        let d = crate::extract::tests::folder("zip-dir-unsure");
        let p = d.join("cp437.zip");
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        z.start_file("plain.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"x").unwrap();
        let mut bytes = z.finish().unwrap().into_inner();
        // "plain.txt" becomes "pl\x84in.txt" in place, in both headers: CP437's ä.
        for i in 0..bytes.len() - 9 {
            if &bytes[i..i + 9] == b"plain.txt" {
                bytes[i + 2] = 0x84;
            }
        }
        std::fs::write(&p, &bytes).unwrap();
        let e = super::entries(&mut std::fs::File::open(&p).unwrap(), &mut |_| true).unwrap_err();
        assert!(super::is_unsure(&e), "{e}");
        std::fs::write(d.join("no.zip"), b"not a zip at all").unwrap();
        assert!(
            super::entries(
                &mut std::fs::File::open(d.join("no.zip")).unwrap(),
                &mut |_| true
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(d);
    }
}

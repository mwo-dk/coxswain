//! What a compressed tar holds, and where each entry starts in it unpacked, kept between runs
//! in the cache (`archive-index/`), by the file's identity: its path, size, modified time and,
//! on Unix, its device and inode. A tar.gz, .bz2, .xz or .zst has no list of its contents: it
//! is unpacked from its start to be listed. With this it is unpacked once; the next listing,
//! in this run or a later one, is read from the cache.
//!
//! An `.xz` made in blocks (xz 5.6 and later make them by default, and `xz -T`) has an index
//! of them at its end: an entry is read from the start of the block it is in, not from the
//! start of the file. gzip, bzip2 and zstd
//! are still unpacked from their start up to the entry.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// An entry as kept: what `archive` lists, and `at`, where it starts in the unpacked tar.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Listed {
    pub name: String,
    pub size: u64,
    pub dir: bool,
    pub modified: u64,
    pub at: u64,
}

/// Lists kept at most; the oldest go past this.
const KEPT: usize = 256;
/// Bytes of names a kept list holds at most: an archive of a few megabytes can name 50,000
/// entries of 64 KB each, gigabytes to keep. Such a list is not kept; the archive is listed again.
const KEPT_NAMES: usize = 64 << 20;
/// What is kept changes form: an older form is never read.
const FORM: &str = "1";

fn folder() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("archive-index"))
}

/// The file the list of `archive`, as it is now, is kept in.
fn file_for(archive: &Path) -> Option<PathBuf> {
    let m = std::fs::metadata(archive).ok()?;
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (m.dev(), m.ino())
    };
    #[cfg(not(unix))]
    let (dev, ino) = (0u64, 0u64);
    let when = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut h = blake3::Hasher::new();
    h.update(FORM.as_bytes());
    h.update(archive.as_os_str().as_encoded_bytes());
    for n in [m.len(), dev, ino] {
        h.update(&n.to_le_bytes());
    }
    h.update(&when.to_le_bytes());
    Some(folder()?.join(format!("{}.json", &h.finalize().to_hex()[..32])))
}

/// The kept list of `archive`, if it has not changed since it was made and can be read.
pub(crate) fn load(archive: &Path) -> Option<Vec<Listed>> {
    // Only of an archive that can be read now: what a user may no longer read is not told.
    File::open(archive).ok()?;
    let f = File::open(file_for(archive)?).ok()?;
    serde_json::from_reader(io::BufReader::new(f)).ok()
}

/// Keeps the whole list of `archive`, written whole or not at all; the oldest lists go past
/// `KEPT`.
pub(crate) fn save(archive: &Path, all: &[Listed]) {
    if all.iter().map(|l| l.name.len()).sum::<usize>() > KEPT_NAMES {
        return;
    }
    let (Some(to), Some(dir)) = (file_for(archive), folder()) else {
        return;
    };
    if crate::fs::private_dir(&dir).is_err() {
        return;
    }
    let mut old: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if old.len() >= KEPT {
        old.sort();
        for (_, p) in &old[..=old.len() - KEPT] {
            let _ = std::fs::remove_file(p);
        }
    }
    let tmp = to.with_extension("tmp");
    let written = File::create(&tmp).and_then(|f| {
        let mut w = io::BufWriter::new(f);
        serde_json::to_writer(&mut w, all).map_err(io::Error::other)?;
        io::Write::flush(&mut w)
    });
    if written.and_then(|()| std::fs::rename(&tmp, &to)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ---------------------------------------------------------------- xz in blocks

/// A block of an xz file: where it starts in the file, its size there (unpadded), and how
/// much it unpacks to.
#[derive(Clone, Copy, Debug)]
struct Block {
    start: u64,
    unpadded: u64,
    unpacked: u64,
}

/// An xz number: seven bits a byte, the lowest first, nine bytes at most.
fn number(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut n = 0u64;
    for k in 0..9 {
        let byte = *b.get(*i)?;
        *i += 1;
        n |= u64::from(byte & 0x7f) << (7 * k);
        if byte & 0x80 == 0 {
            return Some(n);
        }
    }
    None
}

fn put_number(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(b);
    c.sum()
}

/// The most bytes of an xz index read, and the most blocks kept of it: a million blocks (24 MB
/// in memory) is a terabyte in xz's own blocks of a megabyte or more. Past either, the file is
/// read from its start, as one not in blocks.
const INDEX_MAX: u64 = 16 << 20;
const MOST_BLOCKS: u64 = 1 << 20;

/// The stream header and blocks of an xz file of one stream, and where its index starts.
/// `None` for anything else (several streams, padding after it, a damaged index): it is then
/// read from its start, as before.
fn blocks(f: &mut File) -> io::Result<Option<([u8; 12], Vec<Block>, u64)>> {
    let len = f.metadata()?.len();
    if len < 24 {
        return Ok(None);
    }
    let mut head = [0u8; 12];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut head)?;
    let mut foot = [0u8; 12];
    f.seek(SeekFrom::End(-12))?;
    f.read_exact(&mut foot)?;
    if &head[..6] != b"\xfd7zXZ\0" || &foot[10..] != b"YZ" || head[6..8] != foot[8..10] {
        return Ok(None);
    }
    if crc32(&foot[4..10]) != u32::from_le_bytes(foot[..4].try_into().unwrap()) {
        return Ok(None);
    }
    let size = (u64::from(u32::from_le_bytes(foot[4..8].try_into().unwrap())) + 1) * 4;
    if size > INDEX_MAX || size + 24 > len {
        return Ok(None);
    }
    let index_start = len - 12 - size;
    let mut index = vec![0u8; size as usize];
    f.seek(SeekFrom::Start(index_start))?;
    f.read_exact(&mut index)?;
    let body = &index[..index.len() - 4];
    if index[0] != 0
        || crc32(body) != u32::from_le_bytes(index[index.len() - 4..].try_into().unwrap())
    {
        return Ok(None);
    }
    let mut i = 1;
    let Some(n) = number(body, &mut i).filter(|&n| n <= MOST_BLOCKS) else {
        return Ok(None);
    };
    let (mut out, mut start) = (vec![], 12u64);
    for _ in 0..n {
        let (Some(unpadded), Some(unpacked)) = (number(body, &mut i), number(body, &mut i)) else {
            return Ok(None);
        };
        if unpadded == 0 || start.checked_add(unpadded).is_none_or(|e| e > index_start) {
            return Ok(None);
        }
        out.push(Block {
            start,
            unpadded,
            unpacked,
        });
        start += unpadded.div_ceil(4) * 4;
    }
    // Blocks and index meet: one stream, nothing between.
    Ok((start == index_start && n > 1).then_some((head, out, index_start)))
}

/// The unpacked tar `path` (an xz file) from `at` on, when it is in blocks: an xz stream of
/// its own made of the block `at` is in and those after it (the file's header, those blocks as
/// they are in the file, and an index of them), and how many unpacked bytes of it come before
/// `at`. `None` when the file is not in blocks.
pub(crate) fn xz_from(path: &Path, at: u64) -> io::Result<Option<(Box<dyn Read>, u64)>> {
    let mut f = File::open(path)?;
    let Some((head, all, index_start)) = blocks(&mut f)? else {
        return Ok(None);
    };
    let mut before = 0u64;
    let Some(k) = all.iter().position(|b| {
        let here = at < before + b.unpacked;
        if !here {
            before += b.unpacked;
        }
        here
    }) else {
        return Ok(None);
    };
    let mut index = vec![0u8];
    put_number(&mut index, (all.len() - k) as u64);
    for b in &all[k..] {
        put_number(&mut index, b.unpadded);
        put_number(&mut index, b.unpacked);
    }
    while index.len() % 4 != 0 {
        index.push(0);
    }
    let c = crc32(&index);
    index.extend_from_slice(&c.to_le_bytes());
    let mut foot = vec![];
    foot.extend_from_slice(&((index.len() / 4 - 1) as u32).to_le_bytes());
    foot.extend_from_slice(&head[6..8]);
    let c = crc32(&foot);
    let mut tail = c.to_le_bytes().to_vec();
    tail.extend_from_slice(&foot);
    tail.extend_from_slice(b"YZ");
    index.extend_from_slice(&tail);
    f.seek(SeekFrom::Start(all[k].start))?;
    let blocks = io::BufReader::new(f).take(index_start - all[k].start);
    let stream = io::Cursor::new(head.to_vec())
        .chain(blocks)
        .chain(io::Cursor::new(index));
    Ok(Some((
        Box::new(lzma_rust2::XzReader::new(stream, false)),
        at - before,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_xz_in_blocks_is_read_from_the_block_an_offset_is_in() {
        let d = crate::pictures::tests::folder("xz-blocks");
        let data: Vec<u8> = (0..3_000_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        let xz = d.join("data.xz");
        // Blocks of 1 MB, as xz -T makes them (of its own size).
        let mut opts = lzma_rust2::XzOptions::with_preset(1);
        opts.set_block_size(Some(std::num::NonZeroU64::new(1 << 20).unwrap()));
        let mut w = lzma_rust2::XzWriter::new(File::create(&xz).unwrap(), opts).unwrap();
        io::Write::write_all(&mut w, &data).unwrap();
        w.finish().unwrap();
        for at in [
            0u64,
            1,
            (1 << 20) - 1,
            1 << 20,
            5_000_000,
            data.len() as u64 - 1,
        ] {
            let (mut r, skip) = xz_from(&xz, at).unwrap().expect("in blocks");
            io::copy(&mut (&mut r).take(skip), &mut io::sink()).unwrap();
            let mut got = vec![];
            r.read_to_end(&mut got).unwrap();
            assert_eq!(got, &data[at as usize..], "from {at}");
        }
        assert!(
            xz_from(&xz, data.len() as u64).unwrap().is_none(),
            "past the end"
        );
        // One block, or no xz at all: read from the start, as before.
        let one = d.join("one.xz");
        let mut w = lzma_rust2::XzWriter::new(
            File::create(&one).unwrap(),
            lzma_rust2::XzOptions::with_preset(1),
        )
        .unwrap();
        io::Write::write_all(&mut w, &data[..1000]).unwrap();
        w.finish().unwrap();
        assert!(xz_from(&one, 10).unwrap().is_none());
        std::fs::write(d.join("not.xz"), b"not an xz file, not at all, no").unwrap();
        assert!(xz_from(&d.join("not.xz"), 0).unwrap().is_none());
        // A damaged index is never followed.
        let mut bad = std::fs::read(&xz).unwrap();
        let n = bad.len();
        bad[n - 20] ^= 0xff;
        std::fs::write(d.join("bad.xz"), &bad).unwrap();
        assert!(xz_from(&d.join("bad.xz"), 0).unwrap().is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_compressed_tar_is_listed_once_and_its_files_read_from_their_block() {
        let d = crate::pictures::tests::folder("tar-index");
        // Files of 400 KB of numbers, a long name among them, in an xz of 256 KB blocks.
        let mut b = tar::Builder::new(vec![]);
        let body = |i: u32| -> Vec<u8> {
            (0..100_000u32)
                .flat_map(|n| (n ^ i).to_le_bytes())
                .collect()
        };
        let long = format!("{}/last.bin", "deep".repeat(40));
        let names: Vec<String> = (0..6)
            .map(|i| format!("part{i}.bin"))
            .chain([long.clone()])
            .collect();
        for (i, name) in names.iter().enumerate() {
            let data = body(i as u32);
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            b.append_data(&mut h, name, &data[..]).unwrap();
        }
        let plain = b.into_inner().unwrap();
        let xz = d.join("parts.tar.xz");
        let mut opts = lzma_rust2::XzOptions::with_preset(1);
        opts.set_block_size(Some(std::num::NonZeroU64::new(256 << 10).unwrap()));
        let mut w = lzma_rust2::XzWriter::new(File::create(&xz).unwrap(), opts).unwrap();
        io::Write::write_all(&mut w, &plain).unwrap();
        w.finish().unwrap();
        assert!(load(&xz).is_none());
        let (listed, more) = crate::archive::list(&xz, 1000).unwrap();
        assert!(!more && listed.len() == names.len());
        let kept = load(&xz).expect("kept after a whole listing");
        assert_eq!(
            kept.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            names
        );
        // Each one comes out whole, from its block.
        for (i, name) in names.iter().enumerate() {
            let out = d.join(format!("out{i}"));
            std::fs::create_dir_all(&out).unwrap();
            let got = crate::archive::copy_out(&xz, name, &out, None, false).unwrap();
            assert_eq!(std::fs::read(got).unwrap(), body(i as u32), "{name}");
        }
        // Changed: listed again, never from the old list.
        std::fs::write(&xz, b"no longer an archive").unwrap();
        assert!(load(&xz).is_none());
        // A list of names too long to keep is not kept.
        save(&xz, &[Listed { name: "n".repeat(KEPT_NAMES + 1), size: 0, dir: false, modified: 0, at: 0 }]);
        assert!(load(&xz).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}

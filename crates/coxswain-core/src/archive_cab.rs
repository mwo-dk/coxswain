//! A Windows cabinet (`.cab`) or a set of them, read an entry at a time. The cabinets of a
//! set are found by the names each holds for its neighbours, in the folder of the one opened
//! (`bare_name`); a folder of data that runs on from one cabinet into the next is read as one.
//! Packed with nothing, MSZIP, LZX or Quantum (`archive_quantum`). Every size in a header is
//! checked against a limit before anything is allocated for it.

use crate::archive_ro::{Entry, bad, clean};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

pub(crate) trait Src: Read + Seek {}
impl<T: Read + Seek> Src for T {}

/// Opens the cabinet a set names (by its bare file name).
pub(crate) type Open<'a> = &'a dyn Fn(&str) -> io::Result<Box<dyn Src>>;

/// The most a cabinet set is unpacked in all to reach the files wanted in it.
const CAB_WORK: u64 = if cfg!(test) { 2 << 20 } else { 4 << 30 };
/// The most cabinets in one set, and files in all of them.
const MAX_CABS: usize = 512;
const MAX_FILES: usize = 300_000;
/// A name in a header, as long as `archive_ro` reads any.
const NAME_MAX: usize = 4096;

struct Cab {
    set: u16,
    index: u16,
    prev: Option<String>,
    next: Option<String>,
    folders: Vec<Folder>,
    files: Vec<File>,
    /// The bytes each data block holds back for the maker of the cabinet.
    block_reserve: u64,
}

struct Folder {
    at: u64,
    blocks: u16,
    method: u16,
}

struct File {
    name: String,
    size: u32,
    offset: u32,
    folder: u16,
    date: u16,
    time: u16,
    attrs: u16,
}

const FROM_PREV: u16 = 0xfffd;
const TO_NEXT: u16 = 0xfffe;
const BOTH: u16 = 0xffff;

fn zstr(r: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut v = vec![];
    r.take(NAME_MAX as u64 + 1).read_until(0, &mut v)?;
    if v.pop() != Some(0) {
        return Err(bad("a broken cabinet name"));
    }
    Ok(v)
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn header(src: &mut dyn Src) -> io::Result<Cab> {
    let mut r = BufReader::new(src);
    let mut h = [0u8; 36];
    r.read_exact(&mut h)?;
    if &h[..4] != b"MSCF" || h[25] != 1 {
        return Err(bad("not a cabinet"));
    }
    let (files_at, n_folders, n_files, flags) = (le32(&h, 16), le16(&h, 26), le16(&h, 28), le16(&h, 30));
    let (mut folder_reserve, mut block_reserve) = (0, 0);
    if flags & 4 != 0 {
        let mut x = [0u8; 4];
        r.read_exact(&mut x)?;
        folder_reserve = i64::from(x[2]);
        block_reserve = u64::from(x[3]);
        r.seek_relative(i64::from(le16(&x, 0)))?;
    }
    let mut name = |on: bool| -> io::Result<Option<String>> {
        if !on {
            return Ok(None);
        }
        let n = String::from_utf8_lossy(&zstr(&mut r)?).into_owned();
        zstr(&mut r)?; // the label of the disk
        Ok(Some(n))
    };
    let (prev, next) = (name(flags & 1 != 0)?, name(flags & 2 != 0)?);
    let mut folders = Vec::with_capacity(usize::from(n_folders).min(1024));
    for _ in 0..n_folders {
        let mut x = [0u8; 8];
        r.read_exact(&mut x)?;
        r.seek_relative(folder_reserve)?;
        folders.push(Folder { at: u64::from(le32(&x, 0)), blocks: le16(&x, 4), method: le16(&x, 6) });
    }
    r.seek(SeekFrom::Start(u64::from(files_at)))?;
    let mut files = Vec::with_capacity(usize::from(n_files).min(1024));
    for _ in 0..n_files {
        let mut x = [0u8; 16];
        r.read_exact(&mut x)?;
        let attrs = le16(&x, 14);
        let raw = zstr(&mut r)?;
        // A name is UTF-8 when the attribute says so, else a byte-per-character code page.
        let name = if attrs & 0x80 != 0 { String::from_utf8_lossy(&raw).into_owned() } else { raw.iter().map(|&b| char::from(b)).collect() };
        files.push(File { name, size: le32(&x, 0), offset: le32(&x, 4), folder: le16(&x, 8), date: le16(&x, 10), time: le16(&x, 12), attrs });
    }
    Ok(Cab { set: le16(&h, 32), index: le16(&h, 34), prev, next, folders, files, block_reserve })
}

/// A name from a header that is a file beside the cabinet and nothing else: no folder, no
/// drive, no `..`.
fn bare_name(n: &str) -> io::Result<&str> {
    let bare = !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', ':', '\0']);
    if bare { Ok(n) } else { Err(bad("a cabinet set names a file outside its folder")) }
}

/// What a read says when a part of the set is not there: the file it ran over into is cut short.
fn missing(name: &str) -> io::Error {
    io::Error::other(crate::t!("archive.cab_part_missing", "name" => name))
}

/// The cabinets found, in order, and the name of a neighbour that was not there.
type Set = (Vec<Box<dyn Src>>, Vec<Cab>, Option<String>);

fn set(first: Box<dyn Src>, open: Open) -> io::Result<Set> {
    let mut first = first;
    let c = header(&mut *first)?;
    let (mut before, mut after) = (vec![], vec![]);
    let mut gone = None;
    let (mut lo, mut hi) = (c.prev.clone(), c.next.clone());
    let (set, mut lo_index, mut hi_index) = (c.set, c.index, c.index);
    while let Some(name) = lo {
        if before.len() + 1 >= MAX_CABS {
            return Err(bad("too many cabinets in a set"));
        }
        let mut s = match open(bare_name(&name)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                gone = Some(name);
                break;
            }
            r => r?,
        };
        let c = header(&mut *s)?;
        if c.set != set || lo_index.checked_sub(1) != Some(c.index) {
            return Err(bad("a cabinet of another set"));
        }
        lo_index = c.index;
        lo = c.prev.clone();
        before.push((s, c));
    }
    while let Some(name) = hi {
        if before.len() + after.len() + 2 >= MAX_CABS {
            return Err(bad("too many cabinets in a set"));
        }
        let mut s = match open(bare_name(&name)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                gone = Some(name);
                break;
            }
            r => r?,
        };
        let c = header(&mut *s)?;
        if c.set != set || hi_index.checked_add(1) != Some(c.index) {
            return Err(bad("a cabinet of another set"));
        }
        hi_index = c.index;
        hi = c.next.clone();
        after.push((s, c));
    }
    let (mut srcs, mut cabs) = (vec![], vec![]);
    for (s, c) in before.into_iter().rev().chain([(first, c)]).chain(after) {
        srcs.push(s);
        cabs.push(c);
    }
    Ok((srcs, cabs, gone))
}

/// A folder of data: the parts of it in each cabinet, in order.
struct Run {
    method: u16,
    parts: Vec<(usize, u64, u16)>,
}

struct Item {
    entry: Entry,
    run: usize,
    offset: u64,
}

fn items(cabs: &[Cab]) -> io::Result<(Vec<Run>, Vec<Item>)> {
    let (mut runs, mut items) = (vec![], vec![]);
    let mut seen = std::collections::HashSet::new();
    for (i, cab) in cabs.iter().enumerate() {
        if cab.files.len() + items.len() > MAX_FILES {
            return Err(bad("too many files in a cabinet set"));
        }
        // The first folder goes on from the last of the cabinet before when a file says so.
        let goes_on = cab.files.iter().any(|f| f.folder == FROM_PREV || f.folder == BOTH);
        let first_run = runs.len().checked_sub(usize::from(goes_on && i > 0)).ok_or_else(|| bad("a broken cabinet"))?;
        for (k, f) in cab.folders.iter().enumerate() {
            let part = (i, f.at, f.blocks);
            match (k, goes_on && i > 0) {
                (0, true) => {
                    let run: &mut Run = runs.last_mut().ok_or_else(|| bad("a broken cabinet"))?;
                    if run.method != f.method {
                        return Err(bad("a broken cabinet"));
                    }
                    run.parts.push(part);
                }
                _ => runs.push(Run { method: f.method, parts: vec![part] }),
            }
        }
        for f in &cab.files {
            // A file that goes on from the cabinet before is listed there, where it starts.
            let k = match f.folder {
                FROM_PREV | BOTH => continue,
                TO_NEXT => cab.folders.len().checked_sub(1),
                k => Some(usize::from(k)).filter(|&k| k < cab.folders.len()),
            };
            let run = first_run + k.ok_or_else(|| bad("a file in no folder"))?;
            if !seen.insert(f.name.clone()) {
                continue;
            }
            items.push(Item { entry: entry(f), run, offset: u64::from(f.offset) });
        }
    }
    // Each folder is read once, front to back.
    items.sort_by_key(|i| (i.run, i.offset));
    Ok((runs, items))
}

fn entry(f: &File) -> Entry {
    let (d, t) = (f.date, f.time);
    // The local time it was written in, without a zone: taken as a moment here.
    let modified = chrono::NaiveDate::from_ymd_opt(1980 + i32::from(d >> 9), u32::from(d >> 5 & 15), u32::from(d & 31))
        .and_then(|d| d.and_hms_opt(u32::from(t >> 11), u32::from(t >> 5 & 63), u32::from(t & 31) * 2))
        .and_then(|l| {
            use chrono::TimeZone;
            chrono::Local.from_local_datetime(&l).earliest()
        })
        .map_or(0, |d| d.timestamp().max(0) as u64);
    Entry {
        name: clean(&f.name.replace('\\', "/")),
        size: u64::from(f.size),
        dir: false,
        file: true,
        modified,
        mode: if f.attrs & 0x40 != 0 { 0o755 } else { 0o644 },
    }
}

pub(crate) fn each(
    first: Box<dyn Src>,
    open: Open,
    wanted: &dyn Fn(&Entry) -> bool,
    f: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let (mut srcs, cabs, gone) = set(first, open)?;
    let (runs, items) = items(&cabs)?;
    let cut = |e: io::Error| match &gone {
        Some(n) if matches!(e.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData) => missing(n),
        _ => e,
    };
    let mut work = 0u64;
    let mut open_run: Option<(usize, Unpack)> = None;
    for it in &items {
        if it.entry.name.is_empty() {
            continue;
        }
        let go = if wanted(&it.entry) {
            // A file that starts before the folder's place (two with one offset): from the front again.
            if open_run.as_ref().is_none_or(|(r, u)| *r != it.run || u.pos > it.offset) {
                open_run = Some((it.run, Unpack::new(&runs[it.run])?));
            }
            let u = &mut open_run.as_mut().unwrap().1;
            u.skip_to(it.offset, &mut srcs, &cabs, &mut work).map_err(cut)?;
            let mut body = Body { u, srcs: &mut srcs, cabs: &cabs, work: &mut work }.take(it.entry.size);
            f(&it.entry, &mut body).map_err(cut)?
        } else {
            f(&it.entry, &mut io::empty())?
        };
        if !go {
            break;
        }
    }
    Ok(())
}

/// Bytes of compressed data one block may hold, joined from the parts in the cabinets.
const BLOCK_PACKED_MAX: usize = 2 * 65536;

enum Dec {
    Stored,
    /// Each block is a raw deflate stream that may refer to the 32 KB before it.
    MsZip { z: flate2::Decompress, dict: Vec<u8> },
    Lzx(Box<lzxd::Lzxd>),
    Quantum(Box<crate::archive_quantum::Quantum>),
}

/// A folder of data unpacked block by block, front to back.
struct Unpack {
    parts: Vec<(usize, u64, u16)>,
    next: usize,
    cab: usize,
    left: u16,
    dec: Dec,
    buf: Vec<u8>,
    at: usize,
    /// How many bytes of the folder were handed out or passed over.
    pos: u64,
}

impl Unpack {
    fn new(run: &Run) -> io::Result<Unpack> {
        let bits = u32::from(run.method >> 8 & 0x1f);
        let dec = match run.method & 0xf {
            0 => Dec::Stored,
            1 => Dec::MsZip { z: flate2::Decompress::new(false), dict: vec![] },
            2 if (10..=21).contains(&bits) => Dec::Quantum(Box::new(crate::archive_quantum::Quantum::new(bits))),
            3 if (15..=21).contains(&bits) => {
                use lzxd::WindowSize::*;
                let w = [KB32, KB64, KB128, KB256, KB512, MB1, MB2][bits as usize - 15];
                Dec::Lzx(Box::new(lzxd::Lzxd::new(w)))
            }
            _ => return Err(bad("a cabinet packed in a way Coxswain does not read")),
        };
        Ok(Unpack { parts: run.parts.clone(), next: 0, cab: 0, left: 0, dec, buf: vec![], at: 0, pos: 0 })
    }

    /// The next block of the folder in `buf`; false when there is none.
    fn next_block(&mut self, srcs: &mut [Box<dyn Src>], cabs: &[Cab], work: &mut u64) -> io::Result<bool> {
        let mut packed: Vec<u8> = vec![];
        let size = loop {
            while self.left == 0 {
                let Some(&(cab, at, blocks)) = self.parts.get(self.next) else {
                    return if packed.is_empty() { Ok(false) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
                };
                self.next += 1;
                (self.cab, self.left) = (cab, blocks);
                srcs[cab].seek(SeekFrom::Start(at))?;
            }
            self.left -= 1;
            let src = &mut srcs[self.cab];
            let mut h = [0u8; 8];
            src.read_exact(&mut h)?;
            src.seek_relative(cabs[self.cab].block_reserve as i64)?;
            let n = usize::from(le16(&h, 4));
            if packed.len() + n > BLOCK_PACKED_MAX {
                return Err(bad("a broken cabinet block"));
            }
            let start = packed.len();
            packed.resize(start + n, 0);
            src.read_exact(&mut packed[start..])?;
            // A block that goes on in the next cabinet says 0 here, and all it holds there.
            if le16(&h, 6) != 0 {
                break usize::from(le16(&h, 6));
            }
        };
        if size > crate::archive_quantum::BLOCK {
            return Err(bad("a broken cabinet block"));
        }
        *work += size as u64;
        if *work > CAB_WORK {
            return Err(io::Error::other(crate::t!("archive.cab_too_much")));
        }
        self.buf.clear();
        self.at = 0;
        match &mut self.dec {
            Dec::Stored if packed.len() == size => self.buf = packed,
            Dec::Stored => return Err(bad("a broken cabinet block")),
            Dec::MsZip { z, dict } => {
                let Some(packed) = packed.strip_prefix(b"CK") else { return Err(bad("a broken cabinet block")) };
                z.reset(false);
                if !dict.is_empty() {
                    let n = dict.len() as u16;
                    let mut stored = vec![0];
                    stored.extend_from_slice(&n.to_le_bytes());
                    stored.extend_from_slice(&(!n).to_le_bytes());
                    stored.extend_from_slice(dict);
                    let mut sink = Vec::with_capacity(dict.len());
                    z.decompress_vec(&stored, &mut sink, flate2::FlushDecompress::Sync).map_err(io::Error::other)?;
                }
                let mut out = Vec::with_capacity(size);
                z.decompress_vec(packed, &mut out, flate2::FlushDecompress::Finish).map_err(io::Error::other)?;
                if out.len() != size {
                    return Err(bad("a broken cabinet block"));
                }
                dict.extend_from_slice(&out);
                if dict.len() > 0x8000 {
                    dict.drain(..dict.len() - 0x8000);
                }
                self.buf = out;
            }
            Dec::Lzx(lz) => self.buf.extend_from_slice(lz.decompress_next(&packed, size).map_err(io::Error::other)?),
            Dec::Quantum(q) => q.block(&packed, size, &mut self.buf)?,
        }
        Ok(true)
    }

    /// Passes over the folder up to `offset`.
    fn skip_to(&mut self, offset: u64, srcs: &mut [Box<dyn Src>], cabs: &[Cab], work: &mut u64) -> io::Result<()> {
        if offset < self.pos {
            return Err(bad("files overlap in a cabinet"));
        }
        while self.pos < offset {
            if self.at == self.buf.len() && !self.next_block(srcs, cabs, work)? {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let n = (self.buf.len() - self.at).min((offset - self.pos).min(usize::MAX as u64) as usize);
            self.at += n;
            self.pos += n as u64;
        }
        Ok(())
    }
}

struct Body<'a> {
    u: &'a mut Unpack,
    srcs: &'a mut [Box<dyn Src>],
    cabs: &'a [Cab],
    work: &'a mut u64,
}

impl Read for Body<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let u = &mut *self.u;
        if out.is_empty() || u.at == u.buf.len() && !u.next_block(self.srcs, self.cabs, self.work)? {
            return Ok(0);
        }
        let n = out.len().min(u.buf.len() - u.at);
        out[..n].copy_from_slice(&u.buf[u.at..u.at + n]);
        u.at += n;
        u.pos += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Cursor;

    pub(crate) struct Block {
        pub packed: Vec<u8>,
        /// What it unpacks to; 0 for a block that goes on in the next cabinet.
        pub size: u16,
    }

    /// A cabinet: `folders` are (method, blocks) and `files` (name, size, offset, folder).
    pub(crate) fn cab_bytes(
        (set, index): (u16, u16),
        (prev, next): (Option<&str>, Option<&str>),
        folders: &[(u16, Vec<Block>)],
        files: &[(&str, u32, u32, u16)],
    ) -> Vec<u8> {
        let mut names = vec![];
        for n in [prev, next].into_iter().flatten() {
            names.extend_from_slice(n.as_bytes());
            names.extend_from_slice(b"\0disk\0");
        }
        let mut file_part = vec![];
        for (name, size, offset, folder) in files {
            for v in [*size, *offset] {
                file_part.extend_from_slice(&v.to_le_bytes());
            }
            file_part.extend_from_slice(&folder.to_le_bytes());
            // 2 Nov 2020, 10:20:30.
            file_part.extend_from_slice(&[0x42, 0x51, 0x34, 0x50, 0, 0]);
            file_part.extend_from_slice(name.as_bytes());
            file_part.push(0);
        }
        let files_at = 36 + names.len() + 8 * folders.len();
        let mut data_at = files_at + file_part.len();
        let mut folder_part = vec![];
        let mut data = vec![];
        for (method, blocks) in folders {
            folder_part.extend_from_slice(&(data_at as u32 + data.len() as u32).to_le_bytes());
            folder_part.extend_from_slice(&(blocks.len() as u16).to_le_bytes());
            folder_part.extend_from_slice(&method.to_le_bytes());
            for b in blocks {
                data.extend_from_slice(&[0; 4]);
                data.extend_from_slice(&(b.packed.len() as u16).to_le_bytes());
                data.extend_from_slice(&b.size.to_le_bytes());
                data.extend_from_slice(&b.packed);
            }
        }
        data_at = 0;
        let _ = data_at;
        let flags = u16::from(prev.is_some()) | u16::from(next.is_some()) << 1;
        let mut out = b"MSCF\0\0\0\0".to_vec();
        out.extend_from_slice(&((files_at + file_part.len() + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files_at as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[3, 1]);
        for v in [folders.len() as u16, files.len() as u16, flags, set, index] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend(names);
        out.extend(folder_part);
        out.extend(file_part);
        out.extend(data);
        out
    }

    pub(crate) fn stored(data: &[u8]) -> Block {
        Block { packed: data.to_vec(), size: data.len() as u16 }
    }

    fn read_all(first: Vec<u8>, others: &HashMap<&str, Vec<u8>>) -> io::Result<Vec<(String, Vec<u8>)>> {
        let mut out = vec![];
        let open = |n: &str| -> io::Result<Box<dyn Src>> {
            others.get(n).map(|b| Box::new(Cursor::new(b.clone())) as Box<dyn Src>).ok_or_else(|| io::ErrorKind::NotFound.into())
        };
        each(Box::new(Cursor::new(first)), &open, &|_| true, &mut |e, r| {
            let mut v = vec![];
            r.read_to_end(&mut v)?;
            out.push((e.name.clone(), v));
            Ok(true)
        })?;
        Ok(out)
    }

    #[test]
    fn a_quantum_folder_is_read_block_after_block() {
        let text: Vec<u8> = (0..6000u32).map(|i| b"cabinet of curiosities "[(i % 23) as usize] ^ (i / 997) as u8).collect();
        let mut enc = crate::archive_quantum::Quantum::new(16);
        let blocks: Vec<Block> = text.chunks(2000).map(|c| Block { packed: crate::archive_quantum::tests::pack(&mut enc, c), size: c.len() as u16 }).collect();
        // 16 bits of window, level 4: 0x1042.
        let cab = cab_bytes((1, 0), (None, None), &[(0x1042, blocks)], &[("a\\one.txt", 1000, 0, 0), ("two.txt", 5000, 1000, 0)]);
        let got = read_all(cab, &HashMap::new()).unwrap();
        assert_eq!(got.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["a/one.txt", "two.txt"]);
        assert_eq!(got[0].1, text[..1000]);
        assert_eq!(got[1].1, text[1000..]);
    }

    #[test]
    fn an_lzx_folder_of_one_uncompressed_block_is_read() {
        // Header bit 0 (no translation), block type 3 (uncompressed), its size in 24 bits, to
        // the 16-bit boundary, then the three recent offsets and the bytes, padded to even.
        let body = b"lzx without the lz";
        let bits: u32 = (0b011 << 28) | (body.len() as u32) << 4;
        let mut lzx = vec![];
        for w in [(bits >> 16) as u16, bits as u16] {
            lzx.extend_from_slice(&w.to_le_bytes());
        }
        for _ in 0..3 {
            lzx.extend_from_slice(&1u32.to_le_bytes());
        }
        lzx.extend_from_slice(body);
        let packed = Block { packed: lzx, size: body.len() as u16 };
        // 2^15 window: 0x0f03.
        let cab = cab_bytes((1, 0), (None, None), &[(0x0f03, vec![packed])], &[("x.bin", body.len() as u32, 0, 0)]);
        assert_eq!(read_all(cab, &HashMap::new()).unwrap()[0].1, body);
    }

    #[test]
    fn a_set_is_read_from_any_of_its_cabinets_with_a_block_split_between_two() {
        let all: Vec<u8> = (0..160u32).map(|i| (i * 3) as u8).collect();
        // One block of 120 bytes split 50/70 between the cabinets, after one of 40.
        let one = cab_bytes(
            (7, 0),
            (None, Some("two.cab")),
            &[(0, vec![stored(&all[..40]), Block { packed: all[40..90].to_vec(), size: 0 }])],
            &[("a", 60, 0, 0), ("b", 90, 60, TO_NEXT)],
        );
        let two = cab_bytes(
            (7, 1),
            (Some("one.cab"), Some("three.cab")),
            &[(0, vec![Block { packed: all[90..160].to_vec(), size: 120 }])],
            &[("b", 90, 60, FROM_PREV), ("c", 10, 150, 0)],
        );
        let three = cab_bytes((7, 2), (Some("two.cab"), None), &[(0, vec![stored(b"last")])], &[("d", 4, 0, 0)]);
        let others: HashMap<&str, Vec<u8>> = [("one.cab", one.clone()), ("two.cab", two.clone()), ("three.cab", three.clone())].into();
        for first in [&one, &two, &three] {
            let got = read_all(first.clone(), &others).unwrap();
            assert_eq!(got.iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
            assert_eq!(got[0].1, all[..60]);
            assert_eq!(got[1].1, all[60..150]);
            assert_eq!(got[2].1, all[150..]);
            assert_eq!(got[3].1, b"last");
        }
        // A missing part is named.
        let e = read_all(one, &HashMap::new()).unwrap_err();
        assert!(e.to_string().contains("two.cab"), "{e}");
    }

    #[test]
    fn a_set_cannot_lead_outside_its_folder_or_loop() {
        let named = |next: &str, index| cab_bytes((1, index), (None, Some(next)), &[(0, vec![stored(b"x")])], &[("f", 1, 0, 0)]);
        for evil in ["../secret.cab", "/etc/passwd", "sub/x.cab", "C:x.cab", ".."] {
            let others: HashMap<&str, Vec<u8>> = [(evil, named("a.cab", 1))].into();
            let e = read_all(named(evil, 0), &others).unwrap_err();
            assert!(e.to_string().contains("outside"), "{evil}: {e}");
        }
        // A cabinet that names itself, or a ring: the numbers must go up.
        let looped: HashMap<&str, Vec<u8>> = [("me.cab", named("me.cab", 0))].into();
        assert!(read_all(named("me.cab", 0), &looped).is_err());
        // A cabinet of another set is not a part of this one.
        let other = cab_bytes((2, 1), (None, None), &[(0, vec![stored(b"y")])], &[("g", 1, 0, 0)]);
        assert!(read_all(named("o.cab", 0), &[("o.cab", other)].into()).is_err());
    }

    #[test]
    fn broken_cabinets_are_refused_not_followed() {
        let one = |method, size: u16, packed: &[u8]| cab_bytes((1, 0), (None, None), &[(method, vec![Block { packed: packed.to_vec(), size }])], &[("f", u32::from(size), 0, 0)]);
        let none = HashMap::new();
        // A method nobody knows, a block bigger than a block may be, a stored block of the wrong
        // length, an MSZIP block without its signature, a file in a folder that is not there.
        assert!(read_all(one(9, 4, b"abcd"), &none).is_err());
        assert!(read_all(one(0, 40000, &vec![0; 40000]), &none).is_err());
        assert!(read_all(one(0, 5, b"abcd"), &none).is_err());
        assert!(read_all(one(1, 4, b"abcd"), &none).is_err());
        assert!(read_all(cab_bytes((1, 0), (None, None), &[], &[("f", 1, 0, 3)]), &none).is_err());
        // A file said to be past the data: it comes out short or the read fails, never a hang.
        let far = cab_bytes((1, 0), (None, None), &[(0, vec![stored(b"abcd")])], &[("f", 1 << 30, 3, 0)]);
        let got = read_all(far, &none).unwrap();
        assert_eq!(got[0].1, b"d");
        // Cut anywhere: an error or less, no panic.
        let whole = one(0, 4, b"abcd");
        for n in 0..whole.len() {
            let _ = read_all(whole[..n].to_vec(), &none);
        }
    }

    #[test]
    fn cabinets_with_bytes_changed_never_panic_or_run_away() {
        let text: Vec<u8> = (0..3000u32).map(|i| (i % 31 * 7) as u8).collect();
        let mut enc = crate::archive_quantum::Quantum::new(12);
        let q: Vec<Block> = text.chunks(1500).map(|c| Block { packed: crate::archive_quantum::tests::pack(&mut enc, c), size: c.len() as u16 }).collect();
        let quantum = cab_bytes((1, 0), (None, Some("n.cab")), &[(0x0c42, q)], &[("q.bin", 3000, 0, 0)]);
        let next = cab_bytes((1, 1), (Some("p.cab"), None), &[(0, vec![stored(b"tail")])], &[("t", 4, 0, 0)]);
        let others: HashMap<&str, Vec<u8>> = [("n.cab", next.clone()), ("p.cab", quantum.clone())].into();
        assert_eq!(read_all(quantum.clone(), &others).unwrap().len(), 2);
        let mut seed = 12345u32;
        let mut rnd = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as usize
        };
        for _ in 0..3000 {
            let mut bytes = quantum.clone();
            for _ in 0..1 + rnd() % 4 {
                let at = rnd() % bytes.len();
                bytes[at] = rnd() as u8;
            }
            let _ = read_all(bytes, &others);
        }
    }
}

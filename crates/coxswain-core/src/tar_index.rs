//! What a compressed tar holds, and where each entry starts in it unpacked, kept between runs
//! in the cache (`archive-index/`), by the file's identity: its path, size, modified time and,
//! on Unix, its device and inode. A tar.gz, .bz2, .xz or .zst has no list of its contents: it
//! is unpacked from its start to be listed. With this it is unpacked once; the next listing,
//! in this run or a later one, is read from the cache.
//!
//! An `.xz` made in blocks (xz 5.6 and later make them by default, and `xz -T`) has an index
//! of them at its end: an entry is read from the start of the block it is in, not from the
//! start of the file. A gzip is unpacked once from its start; on the way, every 4 MB, the
//! place a deflate block starts and the 32 KB before it are kept beside the list
//! (`<name>.seek`): an entry is then unpacked from the last such place before it. A zstd of
//! several frames (`pzstd`, `zstd --adapt` in parts) keeps where each frame starts. One of a
//! single frame, and bzip2, are still unpacked from their start up to the entry.

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
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if old.len() >= KEPT {
        old.sort();
        for (_, p) in &old[..=old.len() - KEPT] {
            let _ = std::fs::remove_file(p);
            let _ = std::fs::remove_file(p.with_extension("seek"));
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
fn xz_from(path: &Path, at: u64) -> io::Result<Option<(Box<dyn Read>, u64)>> {
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

// ---------------------------------------------------------------- gzip and zstd restart points

/// A place a compressed tar can be unpacked from besides its start: `file` bytes into the
/// file, `out` bytes into the tar. In a gzip, a boundary between deflate blocks: the `bits`
/// last bits of the byte before `file` (kept in `bit_buf`) begin the next block, and `window`
/// is what was unpacked just before it (32 KB at most), which the next blocks refer back into.
/// In a zstd, the start of a frame, which needs nothing before it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Point {
    file: u64,
    out: u64,
    bits: u8,
    bit_buf: u8,
    window: Vec<u8>,
}

/// Where the points of a pass are gathered, to be kept once the tar was listed whole.
pub(crate) type Points = std::rc::Rc<std::cell::RefCell<Vec<Point>>>;

/// Unpacked bytes between two points of a gzip, at first: at most this is unpacked to no use
/// to reach an entry.
const GZ_EVERY: u64 = 4 << 20;
/// Points of a gzip at most (with their windows, 32 MB unpacked): past it every other one goes
/// and they are twice as far apart.
const GZ_POINTS: usize = 1024;
/// Points of a zstd at most: one a frame, past it every other one goes.
const ZST_POINTS: usize = 1 << 16;

struct Record {
    points: Points,
    every: u64,
    next: u64,
    max: usize,
}

impl Record {
    fn new(points: Points, every: u64, max: usize) -> Self {
        Record { points, every, next: every, max }
    }

    fn push(&mut self, p: Point) {
        let mut all = self.points.borrow_mut();
        self.next = p.out + self.every;
        all.push(p);
        if all.len() > self.max {
            let mut keep = false;
            all.retain(|_| {
                keep = !keep;
                !keep
            });
            self.every *= 2;
        }
    }
}

const WINDOW: usize = 32 << 10;

#[derive(PartialEq)]
enum Stage {
    Header,
    Body,
    Trailer,
    End,
}

/// A gzip (of one member or more) unpacked as it is read, noting restart points when asked.
pub(crate) struct Gz<R> {
    src: R,
    /// Bytes of `src` used.
    file: u64,
    dec: Box<miniz_oxide::inflate::core::DecompressorOxide>,
    /// The last 32 KB unpacked, wrapping round at `at`.
    dict: Box<[u8]>,
    at: usize,
    /// Unpacked and not yet handed on: from, how many.
    ready: (usize, usize),
    out: u64,
    stage: Stage,
    members: u64,
    /// The check of the member, unless it was entered in its middle.
    crc: Option<flate2::Crc>,
    record: Option<Record>,
}

impl<R: io::BufRead> Gz<R> {
    pub(crate) fn new(src: R, note: Option<Points>) -> Self {
        Gz {
            src,
            file: 0,
            dec: Box::default(),
            dict: vec![0; WINDOW].into_boxed_slice(),
            at: 0,
            ready: (0, 0),
            out: 0,
            stage: Stage::Header,
            members: 0,
            crc: None,
            record: note.map(|p| Record::new(p, GZ_EVERY, GZ_POINTS)),
        }
    }

    /// Unpacked from `p` on, `src` read from `p.file` on.
    fn from(src: R, p: &Point) -> Self {
        use miniz_oxide::inflate::core::{BlockBoundaryState, DecompressorOxide};
        let mut g = Gz::new(src, None);
        g.dec = Box::new(DecompressorOxide::from_block_boundary_state(&BlockBoundaryState {
            num_bits: p.bits,
            bit_buf: p.bit_buf,
            ..Default::default()
        }));
        g.dict[WINDOW - p.window.len()..].copy_from_slice(&p.window);
        (g.file, g.out, g.stage, g.members) = (p.file, p.out, Stage::Body, 1);
        g
    }

    fn exact(&mut self, b: &mut [u8]) -> io::Result<()> {
        self.src.read_exact(b)?;
        self.file += b.len() as u64;
        Ok(())
    }

    fn past_zero(&mut self) -> io::Result<()> {
        loop {
            let b = self.src.fill_buf()?;
            if b.is_empty() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let (n, done) = match b.iter().position(|&c| c == 0) {
                Some(i) => (i + 1, true),
                None => (b.len(), false),
            };
            self.src.consume(n);
            self.file += n as u64;
            if done {
                return Ok(());
            }
        }
    }

    /// Reads a member's header; false at the end, or before what follows the last member that
    /// is no gzip (as `gzip -d` leaves it).
    fn header(&mut self) -> io::Result<bool> {
        let first = self.members == 0;
        if !first && self.src.fill_buf()?.len() < 3 {
            return Ok(false);
        }
        if !first && self.src.fill_buf()?[..3] != [0x1f, 0x8b, 8] {
            return Ok(false);
        }
        let mut h = [0u8; 10];
        self.exact(&mut h)?;
        if h[..3] != [0x1f, 0x8b, 8] {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a gzip file"));
        }
        let flags = h[3];
        if flags & 4 != 0 {
            let mut n = [0u8; 2];
            self.exact(&mut n)?;
            let n = u64::from(u16::from_le_bytes(n));
            if io::copy(&mut (&mut self.src).take(n), &mut io::sink())? != n {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.file += n;
        }
        for bit in [8, 16] {
            if flags & bit != 0 {
                self.past_zero()?;
            }
        }
        if flags & 2 != 0 {
            self.exact(&mut [0u8; 2])?;
        }
        self.members += 1;
        Ok(true)
    }

    fn inflate(&mut self) -> io::Result<()> {
        use miniz_oxide::inflate::TINFLStatus as S;
        use miniz_oxide::inflate::core::inflate_flags as f;
        let input = self.src.fill_buf()?;
        let mut flags = if input.is_empty() { 0 } else { f::TINFL_FLAG_HAS_MORE_INPUT };
        if self.record.as_ref().is_some_and(|r| self.out >= r.next) {
            flags |= f::TINFL_FLAG_STOP_ON_BLOCK_BOUNDARY;
        }
        let (status, used, made) =
            miniz_oxide::inflate::core::decompress(&mut self.dec, input, &mut self.dict, self.at, flags);
        self.src.consume(used);
        self.file += used as u64;
        if let Some(c) = &mut self.crc {
            c.update(&self.dict[self.at..self.at + made]);
        }
        self.ready = (self.at, made);
        self.at = (self.at + made) % WINDOW;
        self.out += made as u64;
        match status {
            S::Done => self.stage = Stage::Trailer,
            S::BlockBoundary => {
                if let (Some(st), Some(r)) = (self.dec.block_boundary_state(), &mut self.record) {
                    let have = self.out.min(WINDOW as u64) as usize;
                    let mut window = Vec::with_capacity(WINDOW);
                    window.extend_from_slice(&self.dict[self.at..]);
                    window.extend_from_slice(&self.dict[..self.at]);
                    window.drain(..WINDOW - have);
                    r.push(Point {
                        file: self.file,
                        out: self.out,
                        bits: st.num_bits,
                        bit_buf: st.bit_buf,
                        window,
                    });
                }
            }
            S::NeedsMoreInput | S::HasMoreOutput => {}
            S::FailedCannotMakeProgress => return Err(io::ErrorKind::UnexpectedEof.into()),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "damaged gzip data")),
        }
        Ok(())
    }
}

impl<R: io::BufRead> Read for Gz<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.ready.1 > 0 || buf.is_empty() {
                let n = self.ready.1.min(buf.len());
                buf[..n].copy_from_slice(&self.dict[self.ready.0..self.ready.0 + n]);
                self.ready = (self.ready.0 + n, self.ready.1 - n);
                return Ok(n);
            }
            match self.stage {
                Stage::End => return Ok(0),
                Stage::Header => {
                    if self.header()? {
                        *self.dec = Default::default();
                        self.crc = Some(flate2::Crc::new());
                        self.stage = Stage::Body;
                    } else {
                        self.stage = Stage::End;
                    }
                }
                Stage::Body => self.inflate()?,
                Stage::Trailer => {
                    let mut t = [0u8; 8];
                    self.exact(&mut t)?;
                    if let Some(c) = self.crc.take()
                        && (c.sum().to_le_bytes() != t[..4] || c.amount().to_le_bytes() != t[4..])
                    {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "gzip check failed"));
                    }
                    self.stage = Stage::Header;
                }
            }
        }
    }
}

/// A reader that counts what it gives.
struct Counted<R> {
    r: R,
    n: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = self.r.read(b)?;
        self.n += n as u64;
        Ok(n)
    }
}

/// A zstd of one frame or more (skippable frames passed over) unpacked as it is read, noting
/// where each frame starts when asked.
pub(crate) struct Zst<R> {
    src: Counted<R>,
    dec: ruzstd::decoding::FrameDecoder,
    open: bool,
    out: u64,
    record: Option<Record>,
}

impl<R: io::BufRead> Zst<R> {
    pub(crate) fn new(src: R, note: Option<Points>) -> Self {
        Zst {
            src: Counted { r: src, n: 0 },
            dec: ruzstd::decoding::FrameDecoder::new(),
            open: false,
            out: 0,
            record: note.map(|p| Record { points: p, every: 0, next: 0, max: ZST_POINTS }),
        }
    }

    /// Unpacked from the frame at `p` on, `src` read from `p.file` on.
    fn from(src: R, p: &Point) -> Self {
        let mut z = Zst::new(src, None);
        (z.src.n, z.out) = (p.file, p.out);
        z
    }
}

impl<R: io::BufRead> Read for Zst<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use ruzstd::decoding::errors::{FrameDecoderError as E, ReadFrameHeaderError as H};
        loop {
            if self.open {
                if self.dec.can_collect() > 0 || buf.is_empty() {
                    let n = self.dec.read(buf)?;
                    self.out += n as u64;
                    return Ok(n);
                }
                if !self.dec.is_finished() {
                    self.dec
                        .decode_blocks(&mut self.src, ruzstd::decoding::BlockDecodingStrategy::UptoBlocks(1))
                        .map_err(io::Error::other)?;
                    continue;
                }
                self.open = false;
            }
            if self.src.r.fill_buf()?.is_empty() {
                return Ok(0);
            }
            let (file, out) = (self.src.n, self.out);
            match self.dec.reset(&mut self.src) {
                Ok(()) => {
                    self.open = true;
                    if let Some(r) = &mut self.record {
                        r.push(Point { file, out, bits: 0, bit_buf: 0, window: vec![] });
                    }
                }
                Err(E::ReadFrameHeaderError(H::SkipFrame { length, .. })) => {
                    let n = u64::from(length);
                    if io::copy(&mut (&mut self.src).take(n), &mut io::sink())? != n {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                }
                Err(e) => return Err(io::Error::other(e)),
            }
        }
    }
}

/// The file the restart points of `archive`'s kept list are kept in, next to it.
fn points_file(archive: &Path) -> Option<PathBuf> {
    Some(file_for(archive)?.with_extension("seek"))
}

const POINTS_MAGIC: &[u8; 8] = b"CXSEEK1\n";

/// Keeps the restart points of `archive`, if there is more than one: one at its start is no use.
pub(crate) fn save_points(archive: &Path, all: &[Point]) {
    let Some(to) = points_file(archive) else {
        return;
    };
    if all.iter().filter(|p| p.out > 0).count() == 0 {
        let _ = std::fs::remove_file(&to);
        return;
    }
    let mut b = POINTS_MAGIC.to_vec();
    b.extend_from_slice(&(all.len() as u32).to_le_bytes());
    for p in all {
        let w = if p.window.is_empty() {
            vec![]
        } else {
            miniz_oxide::deflate::compress_to_vec(&p.window, 1)
        };
        b.extend_from_slice(&p.file.to_le_bytes());
        b.extend_from_slice(&p.out.to_le_bytes());
        b.extend_from_slice(&[p.bits, p.bit_buf]);
        b.extend_from_slice(&(p.window.len() as u32).to_le_bytes());
        b.extend_from_slice(&(w.len() as u32).to_le_bytes());
        b.extend_from_slice(&w);
    }
    let tmp = to.with_extension("seek-tmp");
    if std::fs::write(&tmp, &b).and_then(|()| std::fs::rename(&tmp, &to)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The kept restart points of `archive`, in order, if they can be read whole.
fn load_points(archive: &Path) -> Option<Vec<Point>> {
    let b = std::fs::read(points_file(archive)?).ok()?;
    let mut b = b.strip_prefix(POINTS_MAGIC.as_slice())?;
    let mut take = |n: usize| -> Option<&[u8]> {
        let (h, t) = b.split_at_checked(n)?;
        b = t;
        Some(h)
    };
    let n = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let mut all: Vec<Point> = vec![];
    for _ in 0..n {
        let file = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let out = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let (bits, bit_buf) = (take(1)?[0], take(1)?[0]);
        let len = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let packed = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let w = take(packed)?;
        let window = if len == 0 {
            vec![]
        } else {
            miniz_oxide::inflate::decompress_to_vec_with_limit(w, WINDOW).ok()?
        };
        if window.len() != len || bits > 7 || all.last().is_some_and(|l| l.out > out || l.file > file) {
            return None;
        }
        all.push(Point { file, out, bits, bit_buf, window });
    }
    Some(all)
}

/// The unpacked tar `path` (a gzip or zstd) from a kept restart point at or before `at` on, and
/// how many unpacked bytes of it come before `at`. `None` when none is kept before `at`.
fn kept_from(path: &Path, pack: crate::archive::Pack, at: u64) -> io::Result<Option<(Box<dyn Read>, u64)>> {
    let Some(p) = load_points(path).and_then(|all| all.into_iter().rev().find(|p| p.out <= at && p.out > 0)) else {
        return Ok(None);
    };
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(p.file))?;
    let src = io::BufReader::new(crate::stop::Reader(f));
    let r: Box<dyn Read> = match pack {
        crate::archive::Pack::Gz => Box::new(Gz::from(src, &p)),
        _ => Box::new(Zst::from(src, &p)),
    };
    Ok(Some((r, at - p.out)))
}

/// The unpacked tar `path` from a place at or before `at` on that is not its start, when
/// there is one (an xz in blocks, a gzip or zstd listed whole before), and how many unpacked
/// bytes of it come before `at`.
pub(crate) fn from(path: &Path, pack: crate::archive::Pack, at: u64) -> io::Result<Option<(Box<dyn Read>, u64)>> {
    match pack {
        crate::archive::Pack::Xz => xz_from(path, at),
        crate::archive::Pack::Gz | crate::archive::Pack::Zst => kept_from(path, pack, at),
        _ => Ok(None),
    }
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

    /// Data that compresses as real files do, neither all alike nor all noise.
    fn sample(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed.wrapping_mul(2_654_435_761) | 1;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if i % 7 < 4 { b"the quick brown fox "[i % 20] } else { (x % 40) as u8 + b'0' }
            })
            .collect()
    }

    fn tar_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut b = tar::Builder::new(vec![]);
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            b.append_data(&mut h, name, &data[..]).unwrap();
        }
        b.into_inner().unwrap()
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut w = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        io::Write::write_all(&mut w, data).unwrap();
        w.finish().unwrap()
    }

    #[test]
    fn a_gzip_is_unpacked_from_its_start_and_from_each_point_alike() {
        let data = sample(13 << 20, 1);
        // Two members, as `cat a.gz b.gz` makes, the second with a name in its header.
        let mut gz = gzip(&data[..9 << 20]);
        let mut second = flate2::GzBuilder::new().filename("b").write(vec![], flate2::Compression::default());
        io::Write::write_all(&mut second, &data[9 << 20..]).unwrap();
        gz.extend(second.finish().unwrap());
        let points = Points::default();
        let mut got = vec![];
        Gz::new(&gz[..], Some(points.clone())).read_to_end(&mut got).unwrap();
        assert!(got == data, "unpacked whole");
        let all = points.borrow().clone();
        assert!(all.len() >= 2, "a point every 4 MB: {}", all.len());
        for p in &all {
            let mut r = Gz::from(&gz[p.file as usize..], p);
            let mut got = vec![];
            r.read_to_end(&mut got).unwrap();
            assert!(got == data[p.out as usize..], "from {}", p.out);
        }
        // A damaged check is told, never passed over.
        let mut bad = gz.clone();
        let n = bad.len();
        bad[n - 6] ^= 1;
        assert!(Gz::new(&bad[..], None).read_to_end(&mut vec![]).is_err());
        assert!(Gz::new(&b"not gzip at all"[..], None).read_to_end(&mut vec![]).is_err());
        // What follows the last member that is no gzip is left, as gzip -d leaves it.
        let mut tail = gzip(b"hello");
        tail.extend_from_slice(&[0; 100]);
        let mut got = vec![];
        Gz::new(&tail[..], None).read_to_end(&mut got).unwrap();
        assert_eq!(got, b"hello");
    }

    #[test]
    fn a_zstd_of_frames_is_unpacked_from_each_frame() {
        let data = sample(3 << 20, 2);
        let mut zst = vec![];
        for (i, part) in data.chunks(1 << 20).enumerate() {
            if i == 1 {
                // A skippable frame between them.
                zst.extend_from_slice(&0x184D_2A50u32.to_le_bytes());
                zst.extend_from_slice(&3u32.to_le_bytes());
                zst.extend_from_slice(b"abc");
            }
            zst.extend(ruzstd::encoding::compress_to_vec(part, ruzstd::encoding::CompressionLevel::Fastest));
        }
        let points = Points::default();
        let mut got = vec![];
        Zst::new(&zst[..], Some(points.clone())).read_to_end(&mut got).unwrap();
        assert!(got == data, "every frame unpacked");
        let all = points.borrow().clone();
        assert_eq!(all.iter().map(|p| p.out).collect::<Vec<_>>(), [0, 1 << 20, 2 << 20]);
        for p in &all {
            let mut got = vec![];
            Zst::from(&zst[p.file as usize..], p).read_to_end(&mut got).unwrap();
            assert!(got == data[p.out as usize..], "from {}", p.out);
        }
    }

    #[test]
    fn a_tar_gz_and_a_tar_zst_are_listed_once_and_their_files_read_from_a_point() {
        let d = crate::pictures::tests::folder("tar-gz-zst-index");
        let files: Vec<(String, Vec<u8>)> =
            (0..14).map(|i| (format!("dir/part{i}.txt"), sample(700_000 + i * 977, i as u32))).collect();
        let plain = tar_of(&files);
        let gz = d.join("parts.tar.gz");
        std::fs::write(&gz, gzip(&plain)).unwrap();
        let zst = d.join("parts.tar.zst");
        let mut frames = vec![];
        for part in plain.chunks(1 << 20) {
            frames.extend(ruzstd::encoding::compress_to_vec(part, ruzstd::encoding::CompressionLevel::Fastest));
        }
        std::fs::write(&zst, frames).unwrap();
        for archive in [&gz, &zst] {
            let (listed, more) = crate::archive::list(archive, 1000).unwrap();
            assert!(!more && listed.len() == files.len());
            assert!(load(archive).is_some());
            let points = load_points(archive).expect("points kept");
            assert!(points.iter().filter(|p| p.out > 0).count() >= 2, "{}", archive.display());
            for (i, (name, body)) in files.iter().enumerate() {
                let at = load(archive).unwrap().into_iter().find(|l| &l.name == name).unwrap().at;
                assert_eq!(
                    kept_from(archive, if archive == &gz { crate::archive::Pack::Gz } else { crate::archive::Pack::Zst }, at)
                        .unwrap()
                        .is_some(),
                    points.iter().any(|p| p.out > 0 && p.out <= at),
                );
                let out = d.join(format!("out-{i}-{}", archive.extension().unwrap().to_string_lossy()));
                std::fs::create_dir_all(&out).unwrap();
                let got = crate::archive::copy_out(archive, name, &out, None, false).unwrap();
                assert!(std::fs::read(got).unwrap() == *body, "{name}");
            }
        }
        // Damaged points are not followed: the file is unpacked from its start.
        std::fs::write(points_file(&gz).unwrap(), b"CXSEEK1\nrubbish").unwrap();
        assert!(load_points(&gz).is_none());
        let out = d.join("after-damage");
        std::fs::create_dir_all(&out).unwrap();
        let got = crate::archive::copy_out(&gz, &files[13].0, &out, None, false).unwrap();
        assert!(std::fs::read(got).unwrap() == files[13].1);
        std::fs::remove_dir_all(d).unwrap();
    }

}

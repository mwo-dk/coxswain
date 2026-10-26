//! A 7z's list of contents read as it is unpacked, never held whole, and one of its files read
//! out through it: a 7z naming 891,000 files is listed holding only the names and sizes (about
//! 100 bytes a file), where `sevenz_rust2` unpacks the whole list into memory and then makes a
//! few hundred bytes for every file (600 MB for that one), before anything can be read.
//!
//! Only what this reads exactly as `sevenz_rust2` does is read here: a list of contents locked
//! with a password, packed with anything but LZMA, LZMA2 or nothing, or with parts this does
//! not know is `Unsure`, and the caller opens the 7z with the crate instead. A file is read
//! out here when its block is one LZMA, LZMA2 or stored stream; any other is `Unsure` too.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

/// The most files a list read here may name: about 100 bytes of memory each.
pub(crate) const MAX_FILES: u64 = 4_000_000;
/// The most a list of contents may unpack to: it is read as it is unpacked, so this bounds the
/// time it takes, not the memory.
const MAX_HEADER: u64 = 1 << 30;
/// The largest dictionary a stream is unpacked with here (7-Zip's own most is 1.5 GB; its
/// "ultra" takes 64 MB): the dictionary is memory.
const MAX_DICT: u32 = 256 << 20;

/// A file or folder of the 7z.
pub(crate) struct Entry {
    pub name: String,
    pub size: u64,
    pub dir: bool,
    /// Seconds since the Unix epoch; 0 when it has none.
    pub modified: u64,
    /// The block its bytes are in and where in it they start, unpacked; `None` for a folder
    /// or an empty file.
    block: Option<(usize, u64)>,
    crc: Option<u32>,
}

impl Entry {
    pub fn has_stream(&self) -> bool {
        self.block.is_some()
    }
}

/// How a block is packed, when it is one stream this reads.
#[derive(Clone, Copy)]
enum Codec {
    Copy,
    Lzma { props: u8, dict: u32 },
    Lzma2 { dict: u32 },
}

struct Block {
    /// Where its packed bytes are in the file, and how many.
    at: u64,
    packed: u64,
    unpacked: u64,
    codec: Option<Codec>,
    locked: bool,
}

/// What the list says.
pub(crate) struct Listing {
    pub entries: Vec<Entry>,
    blocks: Vec<Block>,
}

impl Listing {
    /// Whether anything in it is locked with a password.
    pub fn locked(&self) -> bool {
        self.blocks.iter().any(|b| b.locked)
    }

    /// The bytes of `e` (one of `entries`), unpacked from its block's start and checked
    /// against its CRC at its end. `Unsure` when its block is not one this reads.
    pub fn open<'a, R: Read + Seek + 'a>(
        &self,
        mut file: R,
        e: &Entry,
    ) -> io::Result<Box<dyn Read + 'a>> {
        let Some((b, skip)) = e.block else {
            return Ok(Box::new(io::empty()));
        };
        let block = &self.blocks[b];
        let codec = block.codec.filter(|_| !block.locked).ok_or_else(unsure)?;
        file.seek(SeekFrom::Start(block.at))?;
        let mut r = unpack(
            BufReader::new(file.take(block.packed)),
            codec,
            block.unpacked,
        )?;
        if io::copy(&mut (&mut r).take(skip), &mut io::sink())? != skip {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(Box::new(Checked {
            from: r.take(e.size),
            left: e.size,
            crc: flate2::Crc::new(),
            want: e.crc,
        }))
    }
}

/// A file's bytes, its CRC checked when the last is read.
struct Checked<R> {
    from: R,
    left: u64,
    crc: flate2::Crc,
    want: Option<u32>,
}

impl<R: Read> Read for Checked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.from.read(buf)?;
        if n == 0 && self.left > 0 && !buf.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.crc.update(&buf[..n]);
        self.left -= n as u64;
        if self.left == 0 && n > 0 && self.want.is_some_and(|w| w != self.crc.sum()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a file of the 7z does not check out (CRC)",
            ));
        }
        Ok(n)
    }
}

/// Not read here: the caller opens the 7z with `sevenz_rust2`.
pub(crate) fn unsure() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "7z read by sevenz_rust2")
}

#[cfg(test)]
fn is_unsure(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Unsupported
}

fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "a broken 7z header")
}

fn unpack<'a, R: Read + 'a>(from: R, codec: Codec, size: u64) -> io::Result<Box<dyn Read + 'a>> {
    Ok(match codec {
        Codec::Copy => Box::new(from),
        Codec::Lzma { props, dict } => Box::new(
            lzma_rust2::LzmaReader::new_with_props(from, size, props, dict, None)
                .map_err(|_| bad())?,
        ),
        Codec::Lzma2 { dict } => Box::new(lzma_rust2::Lzma2Reader::new(from, dict, None)),
    })
}

/// The bytes of a header, as they come: counted, so nothing past them is read.
struct Hdr<R> {
    r: R,
}

impl<R: Read> Hdr<R> {
    fn byte(&mut self) -> io::Result<u8> {
        let mut b = [0];
        self.r.read_exact(&mut b).map_err(|_| bad())?;
        Ok(b[0])
    }

    fn bytes<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut b = [0; N];
        self.r.read_exact(&mut b).map_err(|_| bad())?;
        Ok(b)
    }

    /// 7-Zip's numbers: the first byte's leading ones say how many bytes follow.
    fn number(&mut self) -> io::Result<u64> {
        let first = self.byte()? as u64;
        let mut value = 0;
        for i in 0..8 {
            let mask = 0x80 >> i;
            if first & mask == 0 {
                return Ok(value | (first & (mask - 1)) << (8 * i));
            }
            value |= (self.byte()? as u64) << (8 * i);
        }
        Ok(value)
    }

    /// A count of things that each take a byte or more of the header: never more than `most`.
    fn count(&mut self, most: u64) -> io::Result<usize> {
        let n = self.number()?;
        if n > most {
            return Err(unsure());
        }
        Ok(n as usize)
    }

    fn skip(&mut self, n: u64) -> io::Result<()> {
        if io::copy(&mut (&mut self.r).take(n), &mut io::sink())? != n {
            return Err(bad());
        }
        Ok(())
    }

    /// `n` bits, the first the top bit of the first byte.
    fn bits(&mut self, n: usize) -> io::Result<Vec<bool>> {
        let mut out = Vec::with_capacity(n);
        let mut byte = 0;
        for i in 0..n {
            if i % 8 == 0 {
                byte = self.byte()?;
            }
            out.push(byte & (0x80 >> (i % 8)) != 0);
        }
        Ok(out)
    }

    /// "All defined" as a byte, else the bits saying which.
    fn defined(&mut self, n: usize) -> io::Result<Vec<bool>> {
        if self.byte()? != 0 {
            Ok(vec![true; n])
        } else {
            self.bits(n)
        }
    }
}

struct Coder {
    id: Vec<u8>,
    props: Vec<u8>,
}

struct Folder {
    coders: Vec<Coder>,
    /// How many packed streams it reads.
    packed: usize,
    /// The size of its last output: what it unpacks to.
    unpacked: u64,
    crc: Option<u32>,
}

/// What a header's streams info says: where packed streams start, their sizes, the folders,
/// and per folder its files' sizes and CRCs (`None`: one file, the folder's own).
struct Streams {
    pack_pos: u64,
    pack_sizes: Vec<u64>,
    folders: Vec<Folder>,
    files: Vec<Vec<(u64, Option<u32>)>>,
}

const AES: &[u8] = &[0x06, 0xF1, 0x07, 0x01];

fn streams<R: Read>(h: &mut Hdr<R>, most: u64) -> io::Result<Streams> {
    let mut s = Streams {
        pack_pos: 0,
        pack_sizes: vec![],
        folders: vec![],
        files: vec![],
    };
    let mut id = h.byte()?;
    if id == 0x06 {
        s.pack_pos = h.number()?;
        let n = h.count(most)?;
        loop {
            match h.byte()? {
                0x00 => break,
                0x09 => {
                    for _ in 0..n {
                        s.pack_sizes.push(h.number()?);
                    }
                }
                0x0A => {
                    let d = h.defined(n)?;
                    h.skip(d.iter().filter(|&&d| d).count() as u64 * 4)?;
                }
                _ => return Err(unsure()),
            }
        }
        if s.pack_sizes.len() != n {
            return Err(bad());
        }
        id = h.byte()?;
    }
    if id == 0x07 {
        if h.byte()? != 0x0B {
            return Err(unsure());
        }
        let n = h.count(most)?;
        if h.byte()? != 0 {
            return Err(unsure());
        }
        let mut outs = vec![];
        for _ in 0..n {
            let (mut coders, mut ins, mut out) = (vec![], 0u64, 0u64);
            for _ in 0..h.count(64)? {
                let flags = h.byte()?;
                let mut id = vec![0; (flags & 0x0F) as usize];
                h.r.read_exact(&mut id).map_err(|_| bad())?;
                let (i, o) = if flags & 0x10 != 0 {
                    (h.number()?, h.number()?)
                } else {
                    (1, 1)
                };
                let mut props = vec![];
                if flags & 0x20 != 0 {
                    props = vec![0; h.count(most.min(1 << 16))?];
                    h.r.read_exact(&mut props).map_err(|_| bad())?;
                }
                if flags & 0x80 != 0 || i > 64 || o > 64 {
                    return Err(unsure());
                }
                (ins, out) = (ins + i, out + o);
                coders.push(Coder { id, props });
            }
            let binds = out.checked_sub(1).ok_or_else(bad)?;
            let mut bound_outs = vec![];
            for _ in 0..binds {
                h.number()?;
                bound_outs.push(h.number()?);
            }
            let packed = ins.checked_sub(binds).ok_or_else(bad)?;
            if packed > 1 {
                for _ in 0..packed {
                    h.number()?;
                }
            }
            // Its output: the one no other coder takes in.
            let last = (0..out).find(|o| !bound_outs.contains(o)).ok_or_else(bad)?;
            outs.push((out, last));
            s.folders.push(Folder {
                coders,
                packed: packed as usize,
                unpacked: 0,
                crc: None,
            });
        }
        if h.byte()? != 0x0C {
            return Err(unsure());
        }
        for (f, &(out, last)) in s.folders.iter_mut().zip(&outs) {
            for o in 0..out {
                let size = h.number()?;
                if o == last {
                    f.unpacked = size;
                }
            }
        }
        loop {
            match h.byte()? {
                0x00 => break,
                0x0A => {
                    let d = h.defined(n)?;
                    for (f, d) in s.folders.iter_mut().zip(d) {
                        if d {
                            f.crc = Some(u32::from_le_bytes(h.bytes()?));
                        }
                    }
                }
                _ => return Err(unsure()),
            }
        }
        id = h.byte()?;
    }
    // One file a folder unless it says more.
    let mut per: Vec<usize> = vec![1; s.folders.len()];
    let mut sizes: Option<Vec<u64>> = None;
    let mut crcs: Option<Vec<Option<u32>>> = None;
    if id == 0x08 {
        loop {
            match h.byte()? {
                0x00 => break,
                0x0D => {
                    for p in per.iter_mut() {
                        *p = h.count(most)?;
                    }
                }
                0x09 => {
                    let mut all = vec![];
                    for (f, &p) in s.folders.iter().zip(&per) {
                        if p == 0 {
                            continue;
                        }
                        let mut sum = 0u64;
                        for _ in 1..p {
                            let n = h.number()?;
                            sum = sum.checked_add(n).ok_or_else(bad)?;
                            all.push(n);
                        }
                        all.push(f.unpacked.checked_sub(sum).ok_or_else(bad)?);
                    }
                    sizes = Some(all);
                }
                0x0A => {
                    // CRCs of the files whose CRC is not their folder's own.
                    let unknown: usize = s
                        .folders
                        .iter()
                        .zip(&per)
                        .map(|(f, &p)| if p == 1 && f.crc.is_some() { 0 } else { p })
                        .sum();
                    let d = h.defined(unknown)?;
                    let mut given = d.into_iter();
                    let mut all = vec![];
                    for (f, &p) in s.folders.iter().zip(&per) {
                        if p == 1 && f.crc.is_some() {
                            all.push(f.crc);
                            continue;
                        }
                        for _ in 0..p {
                            all.push(if given.next() == Some(true) {
                                Some(u32::from_le_bytes(h.bytes()?))
                            } else {
                                None
                            });
                        }
                    }
                    crcs = Some(all);
                }
                _ => return Err(unsure()),
            }
        }
        id = h.byte()?;
    }
    if id != 0x00 {
        return Err(unsure());
    }
    let mut sizes = sizes
        .unwrap_or_else(|| {
            s.folders
                .iter()
                .zip(&per)
                .filter(|(_, p)| **p > 0)
                .map(|(f, _)| f.unpacked)
                .collect()
        })
        .into_iter();
    let mut crcs = crcs.map(Vec::into_iter);
    for (f, &p) in s.folders.iter().zip(&per) {
        let mut files = vec![];
        for _ in 0..p {
            let size = sizes.next().ok_or_else(bad)?;
            let crc = match crcs.as_mut() {
                Some(c) => c.next().ok_or_else(bad)?,
                None if p == 1 => f.crc,
                None => None,
            };
            files.push((size, crc));
        }
        s.files.push(files);
    }
    Ok(s)
}

fn codec(f: &Folder) -> Option<Codec> {
    let [c] = &f.coders[..] else { return None };
    if f.packed != 1 {
        return None;
    }
    let dict = |d: u32| (d <= MAX_DICT).then_some(d);
    match (&c.id[..], &c.props[..]) {
        ([0x00], _) => Some(Codec::Copy),
        ([0x03, 0x01, 0x01], [props, d @ ..]) if d.len() == 4 => {
            dict(u32::from_le_bytes(d.try_into().ok()?)).map(|dict| Codec::Lzma {
                props: *props,
                dict,
            })
        }
        ([0x21], [b]) if *b <= 40 => {
            let d = if *b == 40 {
                u32::MAX
            } else {
                (2 | (*b as u32 & 1)) << (*b / 2 + 11)
            };
            dict(d).map(|dict| Codec::Lzma2 { dict })
        }
        _ => None,
    }
}

/// The blocks of `s`, where each one's packed bytes are in the file.
fn blocks(s: &Streams) -> io::Result<Vec<Block>> {
    let mut at = 32u64.checked_add(s.pack_pos).ok_or_else(bad)?;
    let mut next = 0;
    let mut out = vec![];
    for f in &s.folders {
        let sizes = s.pack_sizes.get(next..next + f.packed).ok_or_else(bad)?;
        next += f.packed;
        let packed = sizes
            .iter()
            .try_fold(0u64, |a, &b| a.checked_add(b))
            .ok_or_else(bad)?;
        out.push(Block {
            at,
            packed,
            unpacked: f.unpacked,
            codec: codec(f),
            locked: f.coders.iter().any(|c| c.id == AES),
        });
        at = at.checked_add(packed).ok_or_else(bad)?;
    }
    Ok(out)
}

/// The list of the 7z in `file`, its first `max` entries kept. `Unsure` when `sevenz_rust2`
/// should read it.
pub(crate) fn read<R: Read + Seek>(file: &mut R, max: usize) -> io::Result<Listing> {
    let mut n = 0;
    read_keeping(file, &mut |_| {
        n += 1;
        n <= max
    })
}

/// The file (not a folder) named `name` in the 7z in `file`, with the list it is in (holding
/// only it): nothing else of the list is kept.
pub(crate) fn find<R: Read + Seek>(
    file: &mut R,
    name: &str,
) -> io::Result<Option<(Listing, usize)>> {
    let l = read_keeping(file, &mut |n| n == name)?;
    let i = l.entries.iter().position(|e| !e.dir);
    Ok(i.map(|i| (l, i)))
}

/// The list of the 7z in `file`, with the entries whose names `keep` takes.
fn read_keeping<R: Read + Seek>(
    file: &mut R,
    keep: &mut dyn FnMut(&str) -> bool,
) -> io::Result<Listing> {
    let mut start = [0u8; 32];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut start).map_err(|_| bad())?;
    if start[..6] != *b"7z\xBC\xAF\x27\x1C" {
        return Err(bad());
    }
    let at = u64::from_le_bytes(start[12..20].try_into().unwrap());
    let len = u64::from_le_bytes(start[20..28].try_into().unwrap());
    let header_at = 32u64.checked_add(at).ok_or_else(bad)?;
    file.seek(SeekFrom::Start(header_at))?;
    let mut raw = Hdr {
        r: BufReader::new((&mut *file).take(len)),
    };
    match raw.byte()? {
        0x01 => files(&mut raw, keep, len),
        0x17 => {
            let s = streams(&mut raw, len)?;
            drop(raw);
            let b = blocks(&s)?;
            let block = match &b[..] {
                [one] if !one.locked && one.unpacked <= MAX_HEADER => one,
                _ => return Err(unsure()),
            };
            let codec = block.codec.ok_or_else(unsure)?;
            file.seek(SeekFrom::Start(block.at))?;
            let packed = BufReader::new((&mut *file).take(block.packed));
            let mut h = Hdr {
                r: BufReader::with_capacity(
                    1 << 16,
                    unpack(packed, codec, block.unpacked)?.take(block.unpacked),
                ),
            };
            if h.byte()? != 0x01 {
                return Err(unsure());
            }
            files(&mut h, keep, block.unpacked)
        }
        _ => Err(unsure()),
    }
}

/// The rest of a header after its first byte: its streams and its files.
fn files<R: Read>(
    h: &mut Hdr<R>,
    keep: &mut dyn FnMut(&str) -> bool,
    most: u64,
) -> io::Result<Listing> {
    let mut id = h.byte()?;
    if id == 0x02 {
        // Archive properties: none this needs.
        loop {
            if h.byte()? == 0 {
                break;
            }
            let n = h.number()?;
            h.skip(n)?;
        }
        id = h.byte()?;
    }
    if id == 0x03 {
        return Err(unsure());
    }
    let s = if id == 0x04 {
        let s = streams(h, most)?;
        id = h.byte()?;
        s
    } else {
        Streams {
            pack_pos: 0,
            pack_sizes: vec![],
            folders: vec![],
            files: vec![],
        }
    };
    let blocks = blocks(&s)?;
    let mut entries = vec![];
    if id == 0x05 {
        let n = h.count(MAX_FILES.min(most))?;
        let (mut empty_stream, mut empty_file) = (None::<Vec<bool>>, None::<Vec<bool>>);
        // The names kept, by their place in the list, and how many there are.
        let (mut names, mut count, mut times) = (vec![], 0usize, vec![0u64; 0]);
        loop {
            let kind = h.byte()?;
            if kind == 0 {
                break;
            }
            let size = h.number()?;
            match kind {
                0x0E => empty_stream = Some(h.bits(n)?),
                0x0F => {
                    let k = empty_stream
                        .as_ref()
                        .map(|e| e.iter().filter(|&&e| e).count())
                        .ok_or_else(bad)?;
                    empty_file = Some(h.bits(k)?);
                }
                0x11 => {
                    if h.byte()? != 0 || size == 0 || (size - 1) % 2 != 0 {
                        return Err(unsure());
                    }
                    // UTF-16, each name ending in a zero.
                    let mut left = size - 1;
                    let mut name = vec![];
                    while left > 0 {
                        let u = u16::from_le_bytes(h.bytes()?);
                        left -= 2;
                        if u != 0 {
                            name.push(u);
                            continue;
                        }
                        if count >= n {
                            return Err(bad());
                        }
                        // Those not kept only counted.
                        let s = String::from_utf16(&name).map_err(|_| unsure())?;
                        if keep(&s.replace('\\', "/")) {
                            names.push((count, s));
                        }
                        count += 1;
                        name.clear();
                    }
                    if count != n || !name.is_empty() {
                        return Err(bad());
                    }
                }
                0x14 => {
                    let d = h.defined(n)?;
                    if h.byte()? != 0 {
                        return Err(unsure());
                    }
                    times = Vec::with_capacity(n);
                    for d in d {
                        times.push(if d { u64::from_le_bytes(h.bytes()?) } else { 0 });
                    }
                }
                // Where a file starts in another (the crate refuses it too).
                0x18 => return Err(unsure()),
                // Times of making and reading, attributes, and what else: not needed.
                _ => h.skip(size)?,
            }
        }
        if count != n {
            return Err(unsure());
        }
        // Each file with bytes takes the next stream; its block and place follow.
        let mut streams = s.files.iter().enumerate().flat_map(|(b, files)| {
            files.iter().scan(0u64, move |at, &(size, crc)| {
                let here = *at;
                *at += size;
                Some((b, here, size, crc))
            })
        });
        let mut empty_i = 0;
        // Room for all at once: growing by doubling would hold up to twice as much a while.
        entries.reserve_exact(names.len());
        let mut names = names.into_iter().peekable();
        for i in 0..n {
            let has_stream = !empty_stream.as_ref().is_some_and(|e| e[i]);
            let (block, size, crc, dir) = if has_stream {
                let (b, at, size, crc) = streams.next().ok_or_else(bad)?;
                (Some((b, at)), size, crc, false)
            } else {
                let file = empty_file
                    .as_ref()
                    .is_some_and(|f| f.get(empty_i) == Some(&true));
                empty_i += 1;
                (None, 0, None, !file)
            };
            let Some((_, name)) = names.next_if(|(at, _)| *at == i) else {
                continue;
            };
            let nt = times.get(i).copied().unwrap_or(0);
            entries.push(Entry {
                name,
                size,
                dir,
                modified: (nt / 10_000_000).saturating_sub(11_644_473_600),
                block,
                crc,
            });
        }
        id = h.byte()?;
    }
    if id != 0x00 {
        return Err(unsure());
    }
    Ok(Listing { entries, blocks })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    /// A 7z made by `sevenz_rust2`, solid and not, with folders, empty files and a locked
    /// one: what this lists and reads is what the crate does.
    #[test]
    fn listed_and_read_as_the_crate_does() {
        let d = crate::extract::tests::folder("seven-list");
        for (n, solid) in [(0, true), (1, false)] {
            let src = d.join(format!("src{n}"));
            std::fs::create_dir_all(src.join("sub/deeper")).unwrap();
            std::fs::create_dir_all(src.join("empty dir")).unwrap();
            for i in 0..40 {
                let dir = if i % 3 == 0 { "sub/deeper" } else { "sub" };
                std::fs::write(
                    src.join(dir).join(format!("f{i} – é.txt")),
                    format!("the text of file {i}\n").repeat(i * 37),
                )
                .unwrap();
            }
            std::fs::write(src.join("nothing.txt"), b"").unwrap();
            let p = d.join(format!("case{n}.7z"));
            if solid {
                sevenz_rust2::compress_to_path(&src, &p).unwrap();
            } else {
                let mut w = sevenz_rust2::ArchiveWriter::create(&p).unwrap();
                w.set_content_methods(vec![sevenz_rust2::EncoderMethod::LZMA.into()]);
                w.push_source_path_non_solid(&src, |_| true).unwrap();
                w.finish().unwrap();
            }
            let mut f = std::fs::File::open(&p).unwrap();
            let ours = super::read(&mut f, usize::MAX).unwrap();
            let mut crate_r =
                sevenz_rust2::ArchiveReader::open(&p, sevenz_rust2::Password::empty()).unwrap();
            let theirs = &crate_r.archive().files;
            assert_eq!(ours.entries.len(), theirs.len());
            assert!(!ours.locked());
            for (a, b) in ours.entries.iter().zip(theirs.iter()) {
                let secs =
                    (u64::from(b.last_modified_date) / 10_000_000).saturating_sub(11_644_473_600);
                assert_eq!(
                    (a.name.as_str(), a.size, a.dir, a.has_stream(), a.modified),
                    (b.name.as_str(), b.size, b.is_directory, b.has_stream, secs)
                );
            }
            for e in ours.entries.iter().filter(|e| !e.dir) {
                let mut got = vec![];
                ours.open(std::fs::File::open(&p).unwrap(), e)
                    .unwrap()
                    .read_to_end(&mut got)
                    .unwrap();
                let want = crate_r.read_file(&e.name).unwrap();
                assert_eq!(got, want, "{}", e.name);
            }
            // The first `max` kept, all counted.
            assert_eq!(super::read(&mut f, 5).unwrap().entries.len(), 5);
        }
        let _ = std::fs::remove_dir_all(d);
    }

    /// A file whose bytes do not check out is an error at its end, not wrong bytes.
    #[test]
    fn a_wrong_crc_is_an_error() {
        let d = crate::extract::tests::folder("seven-list-crc");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), "stored bytes ".repeat(100)).unwrap();
        let p = d.join("a.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&p).unwrap();
        w.set_content_methods(vec![sevenz_rust2::EncoderMethod::COPY.into()]);
        w.push_source_path(&src, |_| true).unwrap();
        w.finish().unwrap();
        let mut bytes = std::fs::read(&p).unwrap();
        let i = bytes.windows(6).position(|w| w == b"stored").unwrap();
        bytes[i] = b'S';
        std::fs::write(&p, &bytes).unwrap();
        let mut f = std::fs::File::open(&p).unwrap();
        let l = super::read(&mut f, usize::MAX).unwrap();
        let e = l.entries.iter().find(|e| e.name == "a.txt").unwrap();
        let mut out = vec![];
        assert!(
            l.open(std::fs::File::open(&p).unwrap(), e)
                .unwrap()
                .read_to_end(&mut out)
                .is_err()
        );
        let _ = std::fs::remove_dir_all(d);
    }

    /// A 7z whose names are locked (`-mhe`) is the crate's, as is what is no 7z.
    #[test]
    fn locked_names_are_the_crates() {
        let d = crate::extract::tests::folder("seven-list-locked");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("secret.txt"), "the secret").unwrap();
        let p = d.join("locked.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&p).unwrap();
        w.set_content_methods(crate::archive::seven_locking("pw"));
        w.set_encrypt_header(true);
        w.push_source_path(&src, |_| true).unwrap();
        w.finish().unwrap();
        let e = super::read(&mut std::fs::File::open(&p).unwrap(), usize::MAX)
            .err()
            .unwrap();
        assert!(super::is_unsure(&e), "{e}");
        std::fs::write(d.join("no.7z"), b"not a 7z, not at all, no").unwrap();
        assert!(super::read(&mut std::fs::File::open(d.join("no.7z")).unwrap(), 1).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}

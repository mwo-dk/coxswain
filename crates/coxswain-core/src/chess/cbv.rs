//! ChessBase archives (`.cbv`): a database packed into one file, opened as the database it holds
//! (docs/previews/chess.md#chessbase-databases). The files the games need (`.cbh`, `.cbg`,
//! `.cba`, `.cbp`, `.cbt`, `.cbj`) are unpacked into this run's own folder in the cache, once
//! per archive as it is, and read from there; the archive is never changed.
//!
//! Written from the specification of facts in the MIT-licensed `cbvault` (github.com/itshak/
//! cbvault, `docs/format-spec-uncbv.md`), no code of its or of any GPL program read: an 8-byte
//! head (`08 00`, the member count, the record length), a table of members (a name of 128
//! bytes, its place, packed and unpacked lengths), then each member's blocks: a payload length,
//! a word not needed, and the payload, whose first byte says how it is packed (stored, LZ,
//! Huffman, or Huffman then LZ).
//!
//! Every byte is untrusted: lengths are checked against the file before use; a member is
//! streamed to its file (never held whole) and unpacked to no more than its stated length; a
//! stated length far past what its packed bytes could honestly hold, archives over `MAX_TOTAL`
//! in all or more than the disk has free are refused before a byte is written; and a damaged
//! member is an error.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most one archive unpacks to, all its members together: the biggest real databases'
/// `.cbg` files run to a few GB.
const MAX_TOTAL: u64 = 16 << 30;
/// A member stated larger than this many times its packed bytes is refused (real ones pack a
/// few times smaller; a crafted one of runs and back-references could claim 1370 times), unless
/// it is no larger than `SMALL`.
const MAX_RATIO: u64 = 100;
const SMALL: u64 = 64 << 20;
/// What the disk must keep free after an archive is unpacked.
const SPARE: u64 = 1 << 30;
/// How many unpacked archives stay in the cache at a time.
const KEPT: usize = 4;
/// The members a database's games need.
const NEEDED: [&str; 6] = ["cbh", "cbg", "cba", "cbp", "cbt", "cbj"];

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("not a ChessBase archive: {what}"))
}

/// Whether `path` is a ChessBase archive, by its name.
pub fn is_cbv(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cbv"))
}

/// A member of the archive: its name, where its blocks start, their length and what it unpacks to.
struct Member {
    name: String,
    at: u64,
    packed: u64,
    size: u64,
}

fn members(f: &mut File) -> io::Result<Vec<Member>> {
    let len = f.metadata()?.len();
    let mut head = [0u8; 8];
    f.read_exact(&mut head).map_err(|_| bad("too short"))?;
    if head[..2] != [8, 0] {
        return Err(bad("head"));
    }
    let (count, rec) = (u16::from_le_bytes([head[2], head[3]]) as u64, head[4] as u64);
    let pool = 8 + count * rec;
    if rec < 140 || pool > len {
        return Err(bad("table"));
    }
    let mut table = vec![0u8; (count * rec) as usize];
    f.read_exact(&mut table)?;
    let mut at = pool;
    let mut out = vec![];
    for r in table.chunks_exact(rec as usize) {
        let u32_at = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]) as u64;
        let end = r[..128].iter().position(|&c| c == 0).unwrap_or(128);
        let name = super::games::text(&r[..end]).into_owned();
        let (packed, size) = (u32_at(132), u32_at(136));
        if at + packed > len {
            return Err(bad("member past the end"));
        }
        out.push(Member { name, at, packed, size });
        at += packed;
    }
    Ok(out)
}

/// Where a member's bytes go: its file, through a window that keeps the last bytes for LZ's
/// back-references (at most 4098 back), so a member of any length is held in no more than
/// `FLUSH` bytes. More than `max` bytes in all is an error.
struct Out<W: Write> {
    w: W,
    win: Vec<u8>,
    written: u64,
    max: u64,
}

/// The bytes kept for back-references, and how many gather before they are written.
const WINDOW: usize = 1 << 13;
const FLUSH: usize = 1 << 16;

impl<W: Write> Out<W> {
    fn new(w: W, max: u64) -> Self {
        Out { w, win: Vec::with_capacity(FLUSH + 8192), written: 0, max }
    }

    fn len(&self) -> u64 {
        self.written + self.win.len() as u64
    }

    /// After each token: over the stated length is an error, a full buffer is written out.
    fn check(&mut self) -> io::Result<()> {
        if self.len() > self.max {
            return Err(bad("member longer than stated"));
        }
        if self.win.len() >= FLUSH {
            let cut = self.win.len() - WINDOW;
            self.w.write_all(&self.win[..cut])?;
            self.win.drain(..cut);
            self.written += cut as u64;
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<(W, u64)> {
        self.w.write_all(&self.win)?;
        let n = self.len();
        Ok((self.w, n))
    }
}

/// A member's `packed` bytes of blocks from `r`, unpacked to `to`, `size` long; `stop` is asked
/// and `progress` told the packed bytes read after each block.
fn unpack(r: impl Read, packed: u64, size: u64, to: impl Write, stop: &dyn Fn() -> bool, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
    let mut r = io::BufReader::with_capacity(1 << 16, r.take(packed));
    let mut out = Out::new(to, size);
    let (mut left, mut payload) = (packed, vec![0u8; 1 << 16]);
    while left >= 4 {
        if stop() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
        }
        let mut head = [0u8; 4];
        r.read_exact(&mut head)?;
        let n = u16::from_le_bytes([head[0], head[1]]) as usize;
        if 4 + n as u64 > left {
            return Err(bad("block past the member"));
        }
        left -= 4 + n as u64;
        let payload = &mut payload[..n];
        r.read_exact(payload)?;
        progress(packed - left);
        let Some((&mode, data)) = payload.split_first() else { continue };
        match mode {
            0 => out.win.extend_from_slice(data),
            1 => lz(data, &mut out)?,
            2 => out.win.extend(huffman(data)?),
            3 => lz(&huffman(data)?, &mut out)?,
            _ => return Err(bad("block mode")),
        }
        out.check()?;
    }
    if out.finish()?.1 != size {
        return Err(bad("member shorter than stated"));
    }
    Ok(())
}

/// A Huffman block: its length (2 bytes, big-endian), a code for each byte value in order (4
/// bits of length, then the path, a bit at a time from the top), then the coded bytes.
fn huffman(d: &[u8]) -> io::Result<Vec<u8>> {
    let want = u16::from_be_bytes([*d.first().ok_or_else(|| bad("huffman"))?, *d.get(1).ok_or_else(|| bad("huffman"))?]) as usize;
    let mut pos = 16usize;
    let mut bits = |n: usize| -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *d.get(pos / 8)?;
            v = v << 1 | (byte >> (7 - pos % 8) & 1) as u32;
            pos += 1;
        }
        Some(v)
    };
    // The tree: per node its two children (0 none), or the byte it ends in (`LEAF` + value).
    const LEAF: u32 = 1 << 16;
    let mut tree: Vec<[u32; 2]> = vec![[0, 0]];
    let mut sum = 0u32;
    for value in 0..256u32 {
        let n = bits(4).ok_or_else(|| bad("huffman table"))? as usize;
        if n == 0 {
            continue;
        }
        let path = bits(n).ok_or_else(|| bad("huffman table"))?;
        sum += 1 << (15 - n);
        let mut node = 0usize;
        for k in (0..n).rev() {
            let side = (path >> k & 1) as usize;
            if k == 0 {
                if tree[node][side] != 0 {
                    return Err(bad("huffman table"));
                }
                tree[node][side] = LEAF + value;
                break;
            }
            match tree[node][side] {
                0 => {
                    tree.push([0, 0]);
                    tree[node][side] = (tree.len() - 1) as u32;
                }
                c if c >= LEAF => return Err(bad("huffman table")),
                _ => {}
            }
            node = tree[node][side] as usize;
        }
    }
    if sum != 1 << 15 {
        return Err(bad("huffman table"));
    }
    let mut out = Vec::with_capacity(want);
    let mut node = 0usize;
    while out.len() < want {
        let Some(bit) = bits(1) else { break };
        match tree[node][bit as usize] {
            0 => return Err(bad("huffman code")),
            c if c >= LEAF => {
                out.push((c - LEAF) as u8);
                node = 0;
            }
            c => node = c as usize,
        }
    }
    Ok(out)
}

/// An LZ stream, added to `out` (back-references reach into what the member has so far):
/// groups of a 2-byte control word (little-endian) and up to 16 tokens, its bits from the top:
/// 0 a literal byte, 1 a coded token (a tag byte: its top half the kind, its low half part of
/// the length or distance).
fn lz<W: Write>(d: &[u8], o: &mut Out<W>) -> io::Result<()> {
    let mut i = 0;
    let byte = |i: usize| d.get(i).copied().ok_or_else(|| bad("lz past the end"));
    while i < d.len() {
        if i + 1 == d.len() {
            o.win.push(d[i]);
            break;
        }
        let control = u16::from_le_bytes([d[i], d[i + 1]]);
        i += 2;
        for k in (0..16).rev() {
            if i >= d.len() {
                break;
            }
            if control >> k & 1 == 0 {
                o.win.push(d[i]);
                i += 1;
                continue;
            }
            let tag = byte(i)?;
            let (high, low) = ((tag >> 4) as usize, (tag & 15) as usize);
            match high {
                0 => {
                    let b = byte(i + 1)?;
                    o.win.extend(std::iter::repeat_n(b, low + 3));
                    i += 2;
                }
                1 => {
                    let (b1, b2) = (byte(i + 1)? as usize, byte(i + 2)?);
                    o.win.extend(std::iter::repeat_n(b2, low + (b1 << 4) + 0x13));
                    i += 3;
                }
                _ => {
                    let b1 = byte(i + 1)? as usize;
                    let back = (b1 << 4) + low + 3;
                    let n = if high == 2 { byte(i + 2)? as usize + 0x10 } else { high };
                    i += if high == 2 { 3 } else { 2 };
                    // The window always holds the last `WINDOW` bytes or all there are.
                    let from = o.win.len().checked_sub(back).ok_or_else(|| bad("lz distance"))?;
                    for k in 0..n {
                        o.win.push(o.win[from + k]);
                    }
                }
            }
            o.check()?;
        }
    }
    Ok(())
}

/// The archives a `Library` reads now, once per reader: their unpacking is never evicted.
static IN_USE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// An archive in use for as long as this lives (`hold`).
pub(crate) struct Held(PathBuf);

/// Marks `cbv` in use until the `Held` is dropped: its unpacked files are kept meanwhile.
pub(crate) fn hold(cbv: &Path) -> Held {
    IN_USE.lock().unwrap_or_else(|e| e.into_inner()).push(cbv.to_path_buf());
    Held(cbv.to_path_buf())
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut in_use = IN_USE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = in_use.iter().position(|p| *p == self.0) {
            in_use.swap_remove(i);
        }
    }
}

/// Tells other runs' sweeps (`archive::sweep`, a day unchanged) that this run's folder is in use.
fn touch(run: &Path) {
    let _ = File::open(run).and_then(|f| f.set_modified(std::time::SystemTime::now()));
}

/// Archives unpacked in this run: by path, size and time, the `.cbh` file in the cache.
type Unpacked = (PathBuf, u64, Option<std::time::SystemTime>, PathBuf);
static UNPACKED: Mutex<Vec<Unpacked>> = Mutex::new(Vec::new());

/// The `.cbh` file of the database archive `cbv` holds, unpacked into this run's folder in the
/// cache (with the files beside it its games need) when it is not there yet as the archive is
/// now. Only one database, the archive's first `.cbh`; its pictures and the rest are left.
pub fn unpacked(cbv: &Path) -> io::Result<PathBuf> {
    unpacked_with(cbv, &|| false, &|_, _| {})
}

/// `unpacked`, asking `stop` between blocks and telling `progress` the packed bytes read of all.
pub(crate) fn unpacked_with(cbv: &Path, stop: &dyn Fn() -> bool, progress: &dyn Fn(u64, u64)) -> io::Result<PathBuf> {
    let meta = std::fs::metadata(cbv)?;
    let (size, time) = (meta.len(), meta.modified().ok());
    let current = |done: &[Unpacked]| done.iter().find(|d| d.0 == cbv && d.1 == size && d.2 == time && d.3.is_file()).map(|d| d.3.clone());
    // The lock is held only to look and to note: other archives open while this one unpacks.
    if let Some(out) = current(&UNPACKED.lock().unwrap_or_else(|e| e.into_inner())) {
        if let Some(run) = out.parent().and_then(Path::parent) {
            touch(run);
        }
        return Ok(out);
    }
    let mut f = File::open(cbv)?;
    let list = members(&mut f)?;
    // Members at the root only (`name.ext`), never a folder's (`name.ext\file`).
    let root = |m: &Member| !m.name.contains(['\\', '/']);
    let cbh = list.iter().find(|m| root(m) && m.name.to_ascii_lowercase().ends_with(".cbh")).ok_or_else(|| bad("no .cbh in it"))?;
    let stem = cbh.name[..cbh.name.len() - 4].to_string();
    let run = crate::archive::run_folder("cbv", false)?;
    let dir = run.join(format!("{:016x}", {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (cbv, size, time).hash(&mut h);
        h.finish()
    }));
    let mut wanted: HashMap<String, &Member> = HashMap::new();
    for m in list.iter().filter(|m| root(m)) {
        if let Some((s, ext)) = m.name.rsplit_once('.')
            && s.eq_ignore_ascii_case(&stem)
            && NEEDED.contains(&ext.to_ascii_lowercase().as_str())
        {
            wanted.insert(ext.to_ascii_lowercase(), m);
        }
    }
    // What it would unpack to, weighed before a byte is written.
    if wanted.values().any(|m| m.size > SMALL.max(m.packed.saturating_mul(MAX_RATIO))) {
        return Err(bad("a member says it is far larger than its packed bytes can hold"));
    }
    let (total, packed) = wanted.values().fold((0, 0), |(t, p), m| (t + m.size, p + m.packed));
    if total > MAX_TOTAL {
        return Err(bad("larger unpacked than any real database"));
    }
    if crate::fs::free_space(&run).is_some_and(|free| free < total.saturating_add(SPARE)) {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "not enough free space in the cache to unpack the archive"));
    }
    // Unpacked into a folder of its own first, so two opens of the same archive never write
    // into one folder, and a folder in the list is always whole.
    static PART: AtomicU64 = AtomicU64::new(0);
    let part = run.join(format!("part-{}", PART.fetch_add(1, Ordering::Relaxed)));
    crate::fs::private_dir(&part)?;
    let mut read = 0u64;
    let written = wanted.iter().try_for_each(|(ext, m)| {
        f.seek(SeekFrom::Start(m.at))?;
        let to = File::create(part.join(format!("db.{ext}")))?;
        unpack(&mut f, m.packed, m.size, to, stop, &mut |n| progress(read + n, packed))?;
        read += m.packed;
        Ok(())
    });
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    let mut done = UNPACKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(out) = current(&done) {
        let _ = std::fs::remove_dir_all(&part);
        return Ok(out);
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::rename(&part, &dir) {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    // This archive's older unpacking goes, then the oldest not in use past the four last kept
    // (the search store reading many archives keeps no more than that in the cache). When all
    // are in use, there are more for a while.
    let mut gone: Vec<Unpacked> = vec![];
    done.retain(|d| d.0 != cbv || {
        gone.push(d.clone());
        false
    });
    let in_use = IN_USE.lock().unwrap_or_else(|e| e.into_inner());
    while done.len() >= KEPT {
        let Some(i) = done.iter().position(|d| !in_use.contains(&d.0)) else { break };
        gone.push(done.remove(i));
    }
    drop(in_use);
    for old in gone {
        if let Some(d) = old.3.parent()
            && d != dir
        {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    let out = dir.join("db.cbh");
    done.push((cbv.to_path_buf(), size, time, out.clone()));
    touch(&run);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lz_vec(d: &[u8]) -> io::Result<Vec<u8>> {
        let mut o = Out::new(vec![], u64::MAX);
        lz(d, &mut o)?;
        Ok(o.finish()?.0)
    }

    fn unpack_vec(b: &[u8], size: u64) -> io::Result<Vec<u8>> {
        let mut out = vec![];
        unpack(b, b.len() as u64, size, &mut out, &|| false, &mut |_| {})?;
        Ok(out)
    }

    /// A stored block, an LZ block (a literal run, a short run, a long run, back-references
    /// near and far) and a Huffman block of four values: each unpacks to what it says.
    #[test]
    fn blocks_unpack() {
        let block = |payload: &[u8]| [&(payload.len() as u16).to_le_bytes()[..], &[0, 0], payload].concat();
        // LZ: control 0b0001_1111 << 11: three literals "abc", then five tokens.
        let lz_data = [
            &[0b0000_0000, 0b0001_1111][..],
            b"abc",
            &[0x02, b'x'],             // short run: 5 x
            &[0x10, 0x00, b'y'],       // long run: 19 y
            &[0x30, 0x00],             // short back-reference: 3 bytes from 3 back
            &[0x23, 0x00, 0x00],       // long back-reference: 16 bytes from 6 back
            &[0x40, 0x01],             // 4 bytes from 19 back
        ]
        .concat();
        let mut want = b"abc".to_vec();
        want.extend([b'x'; 5]);
        want.extend([b'y'; 19]);
        want.extend(b"yyy");
        for _ in 0..16 {
            want.push(want[want.len() - 6]);
        }
        for _ in 0..4 {
            want.push(want[want.len() - 19]);
        }
        assert_eq!(lz_vec(&lz_data).unwrap(), want);
        // Huffman: 'a' 0, 'b' 10, 'c' 110, 'd' 111; "abcd" is 0 10 110 111.
        let mut bits: Vec<u8> = vec![];
        let mut push = |v: u32, n: usize| (0..n).rev().for_each(|k| bits.push((v >> k & 1) as u8));
        for value in 0..256u32 {
            match value as u8 {
                b'a' => (push(1, 4), push(0, 1)),
                b'b' => (push(2, 4), push(2, 2)),
                b'c' => (push(3, 4), push(6, 3)),
                b'd' => (push(3, 4), push(7, 3)),
                _ => (push(0, 4), ()),
            };
        }
        for (v, n) in [(0, 1), (2, 2), (6, 3), (7, 3)] {
            push(v, n);
        }
        let mut huff = vec![0u8, 4];
        huff.extend(bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |b, (i, &v)| b | v << (7 - i))));
        assert_eq!(huffman(&huff).unwrap(), b"abcd");
        let member = [block(&[&[0][..], b"st"].concat()), block(&[&[1][..], &lz_data].concat()), block(&[&[2][..], &huff].concat())].concat();
        let all = [&b"st"[..], &want, b"abcd"].concat();
        assert_eq!(unpack_vec(&member, all.len() as u64).unwrap(), all);
        assert!(unpack_vec(&member, all.len() as u64 + 1).is_err(), "shorter than stated");
        // A table that is no prefix code, a distance before the start, junk: errors, no panic.
        assert!(huffman(&[0, 4, 0x10]).is_err());
        assert!(lz_vec(&[0x00, 0x80, 0x30, 0x05]).is_err());
        let mut x = 7u32;
        for len in 0..300 {
            let junk: Vec<u8> = (0..len)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x as u8
                })
                .collect();
            let _ = unpack_vec(&junk, 1000);
            let _ = huffman(&junk);
            let _ = lz_vec(&junk);
        }
    }

    /// An LZ block of nothing but the longest runs: 65,500 bytes that unpack to 86 MB.
    fn bomb() -> Vec<u8> {
        let mut payload = vec![1u8];
        while payload.len() + 50 <= 65_535 {
            payload.extend([0xff, 0xff]);
            for _ in 0..16 {
                payload.extend([0x1f, 0xff, b'z']);
            }
        }
        [&(payload.len() as u16).to_le_bytes()[..], &[0, 0], &payload].concat()
    }

    /// A crafted archive cannot take the memory, the disk or the time: a member is streamed
    /// through a window of fixed size, never past its stated length, stops when told, and one
    /// that claims gigabytes from a few packed bytes is refused before a byte is written.
    #[test]
    fn a_bomb_is_bounded_and_refused() {
        let block = bomb();
        let mut o = Out::new(io::sink(), u64::MAX);
        lz(&block[5..], &mut o).unwrap();
        assert!(o.len() > 80 << 20);
        assert_eq!(o.win.capacity(), FLUSH + 8192, "the window never grew");
        // Past its stated length: an error at that length, the rest never unpacked.
        let three = block.repeat(3);
        let mut counted = 0u64;
        let r = unpack(&three[..], three.len() as u64, 10 << 20, CountWriter(&mut counted), &|| false, &mut |_| {});
        assert!(r.is_err() && counted <= 10 << 20);
        // Stopped: at once.
        let r = unpack(&three[..], three.len() as u64, u32::MAX as u64, io::sink(), &|| true, &mut |_| {});
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::Interrupted);
        // An archive whose one member claims 4 GB from 65 KB: refused, nothing left behind.
        let d = std::env::temp_dir().join(format!("coxswain-cbv-bomb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rec = 140usize;
        let mut file = vec![8u8, 0, 1, 0, rec as u8, 0, 3, 0];
        let mut r = vec![0u8; rec];
        r[..9].copy_from_slice(b"Games.cbh");
        r[132..136].copy_from_slice(&(block.len() as u32).to_le_bytes());
        r[136..140].copy_from_slice(&u32::MAX.to_le_bytes());
        file.extend(r);
        file.extend(&block);
        let cbv = d.join("Bomb.cbv");
        std::fs::write(&cbv, &file).unwrap();
        assert!(unpacked(&cbv).unwrap_err().to_string().contains("far larger"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    struct CountWriter<'a>(&'a mut u64);
    impl Write for CountWriter<'_> {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            *self.0 += b.len() as u64;
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A database packed into an archive (stored blocks, a picture in a folder of its own beside
    /// it) opens as the database: the table, a game, Find's line; the archive is left as it was.
    #[test]
    fn an_archive_opens_as_its_database() {
        let d = std::env::temp_dir().join(format!("coxswain-cbv-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut moves = super::super::cbh::tests::RUY.to_vec();
        moves.push(255);
        let cbh = super::super::cbh::tests::base(&d, "Games", &moves);
        let mut files: Vec<(String, Vec<u8>)> = vec![("Games.cbh\\pic.bmp".into(), vec![1, 2, 3])];
        for ext in ["cbh", "cbg", "cba", "cbp", "cbt"] {
            files.push((format!("Games.{ext}"), std::fs::read(cbh.with_extension(ext)).unwrap()));
        }
        let rec = 173usize;
        let mut out = vec![8u8, 0, files.len() as u8, 0, rec as u8, 0, 3, 0];
        let streams: Vec<Vec<u8>> = files
            .iter()
            .map(|(_, b)| {
                let mut s = vec![];
                for c in b.chunks(1000) {
                    s.extend(((c.len() + 1) as u16).to_le_bytes());
                    s.extend([0, 0, 0]);
                    s.extend(c);
                }
                s
            })
            .collect();
        let mut at = 8 + files.len() * rec;
        for ((name, b), st) in files.iter().zip(&streams) {
            let mut r = vec![0u8; rec];
            r[..name.len()].copy_from_slice(name.as_bytes());
            r[128..132].copy_from_slice(&(at as u32).to_le_bytes());
            r[132..136].copy_from_slice(&(st.len() as u32).to_le_bytes());
            r[136..140].copy_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend(r);
            at += st.len();
        }
        out.extend(streams.concat());
        let cbv = d.join("Games.cbv");
        std::fs::write(&cbv, &out).unwrap();
        let lib = super::super::Library::open(&cbv).unwrap();
        while !lib.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(lib.row(0).unwrap().white, "Morphy, Paul");
        assert_eq!(lib.game(0).unwrap().main_line().len(), 5);
        assert!(super::super::games::search_text(&cbv, 1 << 20).unwrap().contains("Morphy, Paul"));
        assert_eq!(std::fs::read(&cbv).unwrap(), out);
        // Cut short: an error, and nothing half unpacked is used.
        let cut = d.join("Cut.cbv");
        std::fs::write(&cut, &out[..out.len() - 10]).unwrap();
        assert!(unpacked(&cut).is_err());
        // Five newer archives do not take the files of one still read: the oldest unused go.
        let mine = unpacked(&cbv).unwrap();
        let others: Vec<PathBuf> = (0..5).map(|i| d.join(format!("Other{i}.cbv"))).collect();
        let theirs: Vec<PathBuf> = others
            .iter()
            .map(|o| {
                std::fs::write(o, &out).unwrap();
                unpacked(o).unwrap()
            })
            .collect();
        assert!(mine.is_file(), "in use, kept");
        assert_eq!(lib.game(0).unwrap().main_line().len(), 5);
        assert!(!theirs[0].exists() && theirs[4].is_file(), "the oldest unused went");
        // Read no more: its files go when the next archive comes.
        drop(lib);
        std::fs::write(&others[0], [&out[..], &[0]].concat()).unwrap();
        unpacked(&others[0]).unwrap();
        assert!(!mine.exists(), "no longer in use, evicted");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Every game of a real archive (`COXSWAIN_CBV=/path/to/x.cbv`) read to its end. No real
    /// archive is in the repository.
    #[test]
    #[ignore]
    fn a_real_archive_reads_whole() {
        let p = PathBuf::from(std::env::var("COXSWAIN_CBV").unwrap());
        let cbh = unpacked(&p).unwrap();
        let (mut games, mut cut) = (0, vec![]);
        super::super::cbh::scan(&cbh, &|| false, &|_| {}, |g| {
            let text = super::super::cbh::pgn(&cbh, g.record).unwrap();
            games += 1;
            if text.contains(&crate::t!("chess.cbh_cut")) {
                cut.push(g.record);
            }
            true
        })
        .unwrap();
        eprintln!("{games} games, cut: {cut:?}");
        assert!(games > 0 && cut.is_empty());
    }
}

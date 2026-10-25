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
//! Every byte is untrusted: lengths are checked against the file before use, a member is
//! unpacked to no more than its stated length and `MAX_MEMBER`, and a damaged one is an error.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The largest member unpacked: the biggest real databases' `.cbg` files run to a few GB.
// ponytail: whole members in memory; stream them to disk when archives past 4 GB turn up.
const MAX_MEMBER: u64 = 4 << 30;
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

/// A member's bytes, unpacked from its blocks `b`, `size` long.
fn unpack(b: &[u8], size: u64) -> io::Result<Vec<u8>> {
    if size > MAX_MEMBER {
        return Err(bad("member too large"));
    }
    let mut out = Vec::with_capacity(size.min(1 << 26) as usize);
    let mut i = 0;
    while i + 4 <= b.len() {
        let n = u16::from_le_bytes([b[i], b[i + 1]]) as usize;
        let payload = b.get(i + 4..i + 4 + n).ok_or_else(|| bad("block past the member"))?;
        i += 4 + n;
        let Some((&mode, data)) = payload.split_first() else { continue };
        match mode {
            0 => out.extend_from_slice(data),
            1 => lz(data, &mut out)?,
            2 => out.extend(huffman(data)?),
            3 => lz(&huffman(data)?, &mut out)?,
            _ => return Err(bad("block mode")),
        }
        if out.len() as u64 > size {
            return Err(bad("member longer than stated"));
        }
    }
    if out.len() as u64 != size {
        return Err(bad("member shorter than stated"));
    }
    Ok(out)
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
fn lz(d: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
    let mut i = 0;
    let byte = |i: usize| d.get(i).copied().ok_or_else(|| bad("lz past the end"));
    while i < d.len() {
        if i + 1 == d.len() {
            out.push(d[i]);
            break;
        }
        let control = u16::from_le_bytes([d[i], d[i + 1]]);
        i += 2;
        for k in (0..16).rev() {
            if i >= d.len() {
                break;
            }
            if control >> k & 1 == 0 {
                out.push(d[i]);
                i += 1;
                continue;
            }
            let tag = byte(i)?;
            let (high, low) = ((tag >> 4) as usize, (tag & 15) as usize);
            match high {
                0 => {
                    let b = byte(i + 1)?;
                    out.extend(std::iter::repeat_n(b, low + 3));
                    i += 2;
                }
                1 => {
                    let (b1, b2) = (byte(i + 1)? as usize, byte(i + 2)?);
                    out.extend(std::iter::repeat_n(b2, low + (b1 << 4) + 0x13));
                    i += 3;
                }
                _ => {
                    let b1 = byte(i + 1)? as usize;
                    let back = (b1 << 4) + low + 3;
                    let n = if high == 2 { byte(i + 2)? as usize + 0x10 } else { high };
                    i += if high == 2 { 3 } else { 2 };
                    let from = out.len().checked_sub(back).ok_or_else(|| bad("lz distance"))?;
                    for k in 0..n {
                        out.push(out[from + k]);
                    }
                }
            }
            if out.len() as u64 > MAX_MEMBER {
                return Err(bad("member too large"));
            }
        }
    }
    Ok(())
}

/// Archives unpacked in this run: by path, size and time, the `.cbh` file in the cache.
type Unpacked = (PathBuf, u64, Option<std::time::SystemTime>, PathBuf);
static UNPACKED: Mutex<Vec<Unpacked>> = Mutex::new(Vec::new());

/// The `.cbh` file of the database archive `cbv` holds, unpacked into this run's folder in the
/// cache (with the files beside it its games need) when it is not there yet as the archive is
/// now. Only one database, the archive's first `.cbh`; its pictures and the rest are left.
pub fn unpacked(cbv: &Path) -> io::Result<PathBuf> {
    let meta = std::fs::metadata(cbv)?;
    let (size, time) = (meta.len(), meta.modified().ok());
    let mut done = UNPACKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = done.iter().find(|d| d.0 == cbv && d.1 == size && d.2 == time && d.3.is_file()) {
        return Ok(d.3.clone());
    }
    let mut f = File::open(cbv)?;
    let list = members(&mut f)?;
    // Members at the root only (`name.ext`), never a folder's (`name.ext\file`).
    let root = |m: &Member| !m.name.contains(['\\', '/']);
    let cbh = list.iter().find(|m| root(m) && m.name.to_ascii_lowercase().ends_with(".cbh")).ok_or_else(|| bad("no .cbh in it"))?;
    let stem = cbh.name[..cbh.name.len() - 4].to_string();
    let dir = crate::archive::run_folder("cbv", false)?.join(format!("{:016x}", {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (cbv, size, time).hash(&mut h);
        h.finish()
    }));
    crate::fs::private_dir(&dir)?;
    let mut wanted: HashMap<String, &Member> = HashMap::new();
    for m in list.iter().filter(|m| root(m)) {
        if let Some((s, ext)) = m.name.rsplit_once('.')
            && s.eq_ignore_ascii_case(&stem)
            && NEEDED.contains(&ext.to_ascii_lowercase().as_str())
        {
            wanted.insert(ext.to_ascii_lowercase(), m);
        }
    }
    let written = wanted.iter().try_for_each(|(ext, m)| {
        f.seek(SeekFrom::Start(m.at))?;
        let mut packed = vec![];
        (&mut f).take(m.packed).read_to_end(&mut packed)?;
        let bytes = unpack(&packed, m.size)?;
        std::fs::write(dir.join(format!("db.{ext}")), bytes)
    });
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    let out = dir.join("db.cbh");
    // The four last unpacked are kept; an older one's folder goes (the search store reading
    // many archives keeps no more than that in the cache).
    while done.len() >= KEPT {
        let old = done.remove(0);
        if let Some(d) = old.3.parent() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    done.push((cbv.to_path_buf(), size, time, out.clone()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut out = vec![];
        lz(&lz_data, &mut out).unwrap();
        assert_eq!(out, want);
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
        assert_eq!(unpack(&member, all.len() as u64).unwrap(), all);
        assert!(unpack(&member, all.len() as u64 + 1).is_err(), "shorter than stated");
        // A table that is no prefix code, a distance before the start, junk: errors, no panic.
        assert!(huffman(&[0, 4, 0x10]).is_err());
        assert!(lz(&[0x00, 0x80, 0x30, 0x05], &mut vec![]).is_err());
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
            let _ = unpack(&junk, 1000);
            let _ = huffman(&junk);
            let _ = lz(&junk, &mut vec![]);
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

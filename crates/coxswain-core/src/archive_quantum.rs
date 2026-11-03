//! The Quantum decoder of a cabinet folder (`archive_cab`): an arithmetic coder over adaptive
//! models, with LZ77 matches into a window of 2^10 to 2^21 bytes. Each 32 KB block restarts the
//! arithmetic coder; the models and the window go on through the whole folder.

use std::io;

/// What a block unpacks to, at the most.
pub(crate) const BLOCK: usize = 32768;

struct Model {
    /// The symbols with their running cumulative frequencies, most frequent first; one more
    /// entry at the end with frequency 0.
    syms: Vec<(u16, u16)>,
    shifts_left: u8,
}

impl Model {
    fn new(start: u16, n: u16) -> Model {
        Model { syms: (0..=n).map(|i| (start + i, n - i)).collect(), shifts_left: 4 }
    }

    /// Counts the symbol at `i` (and all before it) one more time, and rescales when the total
    /// is large.
    fn bump(&mut self, i: usize) {
        for s in &mut self.syms[..=i] {
            s.1 += 8;
        }
        if self.syms[0].1 > 3800 {
            self.update();
        }
    }

    fn update(&mut self) {
        let n = self.syms.len() - 1;
        self.shifts_left -= 1;
        if self.shifts_left != 0 {
            for i in (0..n).rev() {
                let next = self.syms[i + 1].1;
                let s = &mut self.syms[i].1;
                *s >>= 1;
                if *s <= next {
                    *s = next + 1;
                }
            }
        } else {
            self.shifts_left = 50;
            for i in 0..n {
                let next = self.syms[i + 1].1;
                let s = &mut self.syms[i].1;
                *s = (*s - next + 1) >> 1;
            }
            // An in-place selection sort by frequency, the way the format's makers did it:
            // equal frequencies keep an order that the other sorts would not.
            for i in 0..n.saturating_sub(1) {
                for j in i + 1..n {
                    if self.syms[i].1 < self.syms[j].1 {
                        self.syms.swap(i, j);
                    }
                }
            }
            for i in (0..n).rev() {
                self.syms[i].1 += self.syms[i + 1].1;
            }
        }
    }
}

const LENGTH_BASE: [u32; 27] = [0, 1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 18, 22, 26, 30, 38, 46, 54, 62, 78, 94, 110, 126, 158, 190, 222, 254];
const LENGTH_EXTRA: [u8; 27] = [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

/// The distance a position slot starts at and the bits that follow it.
fn slot(s: usize) -> (u32, u32) {
    if s < 4 { (s as u32, 0) } else { ((2 + (s as u32 & 1)) << (s / 2 - 1), s as u32 / 2 - 1) }
}

pub(crate) struct Quantum {
    window: Vec<u8>,
    at: usize,
    literal: [Model; 4],
    pos3: Model,
    pos4: Model,
    pos_long: Model,
    len_long: Model,
    selector: Model,
}

impl Quantum {
    /// A decoder for a window of 2^`bits` bytes (10 to 21).
    pub(crate) fn new(bits: u32) -> Quantum {
        let slots = (bits * 2) as u16;
        Quantum {
            window: vec![0; 1 << bits],
            at: 0,
            literal: [Model::new(0, 64), Model::new(0x40, 64), Model::new(0x80, 64), Model::new(0xc0, 64)],
            pos3: Model::new(0, slots.min(24)),
            pos4: Model::new(0, slots.min(36)),
            pos_long: Model::new(0, slots),
            len_long: Model::new(0, 27),
            selector: Model::new(0, 7),
        }
    }

    pub(crate) fn window_len(&self) -> usize {
        self.window.len()
    }

    /// The block `data` unpacked to `out_len` bytes (at most `BLOCK`), appended to `out`.
    /// `last`: the folder's last block, which may end in the middle of a match its maker had in
    /// mind; in any other a match over the end is a broken block.
    pub(crate) fn block(&mut self, data: &[u8], out_len: usize, last: bool, out: &mut Vec<u8>) -> io::Result<()> {
        if out_len > BLOCK {
            return Err(bad());
        }
        let mut c = Coder { data, bit: 0, h: 0xffff, l: 0, c: 0 };
        c.c = c.bits(16) as u16;
        let mask = self.window.len() - 1;
        let start = out.len();
        while out.len() - start < out_len {
            let left = out_len - (out.len() - start);
            let sel = c.symbol(&mut self.selector);
            let (len, dist) = match sel {
                0..=3 => {
                    let b = c.symbol(&mut self.literal[sel as usize]) as u8;
                    self.window[self.at] = b;
                    self.at = (self.at + 1) & mask;
                    out.push(b);
                    continue;
                }
                4 | 5 => {
                    let m = if sel == 4 { &mut self.pos3 } else { &mut self.pos4 };
                    let (base, extra) = slot(c.symbol(m) as usize);
                    (sel as usize - 1, base + c.bits(extra) + 1)
                }
                6 => {
                    let l = c.symbol(&mut self.len_long) as usize;
                    let len = LENGTH_BASE[l] + c.bits(u32::from(LENGTH_EXTRA[l])) + 5;
                    let (base, extra) = slot(c.symbol(&mut self.pos_long) as usize);
                    (len as usize, base + c.bits(extra) + 1)
                }
                _ => return Err(bad()),
            };
            if len > left && !last {
                return Err(bad_at("a match past the end of its block", out.len() - start, len, dist));
            }
            let len = len.min(left);
            if dist as usize > self.window.len() {
                return Err(bad_at("a distance past the window", out.len() - start, len, dist));
            }
            for _ in 0..len {
                let b = self.window[(self.at + self.window.len() - dist as usize) & mask];
                self.window[self.at] = b;
                self.at = (self.at + 1) & mask;
                out.push(b);
            }
        }
        Ok(())
    }
}

fn bad_at(what: &str, at: usize, len: usize, dist: u32) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{what} (at {at}, {len} by {dist})"))
}

fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "a broken Quantum block")
}

/// The arithmetic decoder over one block's bytes, most significant bit first; past the end it
/// reads zeros.
struct Coder<'a> {
    data: &'a [u8],
    bit: usize,
    h: u16,
    l: u16,
    c: u16,
}

impl Coder<'_> {
    fn bits(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            let byte = self.data.get(self.bit / 8).copied().unwrap_or(0);
            v = v << 1 | u32::from(byte >> (7 - self.bit % 8) & 1);
            self.bit += 1;
        }
        v
    }

    fn symbol(&mut self, m: &mut Model) -> u16 {
        let n = m.syms.len() - 1;
        let total = u32::from(m.syms[0].1);
        let range = u32::from(self.h.wrapping_sub(self.l)) + 1;
        let f = (u32::from(self.c.wrapping_sub(self.l)).wrapping_add(1).wrapping_mul(total).wrapping_sub(1) / range) & 0xffff;
        let mut i = 1;
        while i < n && u32::from(m.syms[i].1) > f {
            i += 1;
        }
        let sym = m.syms[i - 1].0;
        let range = u32::from(self.h.wrapping_sub(self.l)) + 1;
        let (hi, lo) = (u32::from(m.syms[i - 1].1), u32::from(m.syms[i].1));
        let l = u32::from(self.l);
        self.h = (l + hi * range / total).wrapping_sub(1) as u16;
        self.l = (l + lo * range / total) as u16;
        m.bump(i - 1);
        loop {
            if (self.l ^ self.h) & 0x8000 != 0 {
                if self.l & 0x4000 != 0 && self.h & 0x4000 == 0 {
                    self.c ^= 0x4000;
                    self.l &= 0x3fff;
                    self.h |= 0x4000;
                } else {
                    break;
                }
            }
            self.l <<= 1;
            self.h = self.h << 1 | 1;
            self.c = self.c << 1 | self.bits(1) as u16;
        }
        sym
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    struct Enc {
        l: u16,
        h: u16,
        pending: usize,
        shifts: usize,
        arith: Vec<bool>,
        raws: Vec<(usize, u32, u32)>,
    }

    impl Enc {
        fn symbol(&mut self, m: &mut Model, sym: u16) {
            let i = m.syms.iter().position(|s| s.0 == sym).unwrap();
            let total = u32::from(m.syms[0].1);
            let range = u32::from(self.h - self.l) + 1;
            let (hi, lo) = (u32::from(m.syms[i].1), u32::from(m.syms[i + 1].1));
            let l = u32::from(self.l);
            self.h = (l + hi * range / total - 1) as u16;
            self.l = (l + lo * range / total) as u16;
            m.bump(i);
            loop {
                if (self.l ^ self.h) & 0x8000 != 0 {
                    if self.l & 0x4000 != 0 && self.h & 0x4000 == 0 {
                        self.pending += 1;
                        self.l &= 0x3fff;
                        self.h |= 0x4000;
                    } else {
                        break;
                    }
                } else {
                    self.put(self.l & 0x8000 != 0);
                }
                self.l <<= 1;
                self.h = self.h << 1 | 1;
                self.shifts += 1;
            }
        }

        fn put(&mut self, b: bool) {
            self.arith.push(b);
            self.arith.extend(std::iter::repeat_n(!b, self.pending));
            self.pending = 0;
        }

        /// Raw bits sit before the arithmetic bit the decoder reads next.
        fn raw(&mut self, v: u32, n: u32) {
            self.raws.push((16 + self.shifts, v, n));
        }
    }

    /// `data` packed as one block by a decoder's twin `q` (its models and window go on).
    pub(crate) fn pack(q: &mut Quantum, data: &[u8]) -> Vec<u8> {
        let mut e = Enc { l: 0, h: 0xffff, pending: 0, shifts: 0, arith: vec![], raws: vec![] };
        let mask = q.window.len() - 1;
        let mut i = 0;
        while i < data.len() {
            // The longest match among the last 300 bytes of the window.
            let mut best = (0, 0);
            for d in 1..=300usize.min(q.window.len()) {
                let mut n = 0;
                while n < (data.len() - i).min(40) && q.window[(q.at + q.window.len() - d + n % d) & mask] == data[i + n] {
                    n += 1;
                }
                if n > best.0 {
                    best = (n, d);
                }
            }
            let (n, d) = best;
            // The slot a distance falls in.
            let slot_of = |dist: usize| (0..42).rev().find(|&s| slot(s).0 as usize <= dist - 1).unwrap();
            if n >= 3 {
                let s = slot_of(d);
                let (base, extra) = slot(s);
                if n <= 4 {
                    e.symbol(&mut q.selector, n as u16 + 1);
                    e.symbol(if n == 3 { &mut q.pos3 } else { &mut q.pos4 }, s as u16);
                } else {
                    e.symbol(&mut q.selector, 6);
                    let l = (0..27).rev().find(|&l| LENGTH_BASE[l] as usize + 5 <= n).unwrap();
                    e.symbol(&mut q.len_long, l as u16);
                    e.raw(n as u32 - 5 - LENGTH_BASE[l], u32::from(LENGTH_EXTRA[l]));
                    e.symbol(&mut q.pos_long, s as u16);
                }
                e.raw(d as u32 - 1 - base, extra);
                for k in 0..n {
                    q.window[q.at] = data[i + k];
                    q.at = (q.at + 1) & mask;
                }
                i += n;
            } else {
                let b = data[i];
                e.symbol(&mut q.selector, u16::from(b >> 6));
                e.symbol(&mut q.literal[usize::from(b >> 6)], u16::from(b));
                q.window[q.at] = b;
                q.at = (q.at + 1) & mask;
                i += 1;
            }
        }
        // Finish: two bits tell the quarter the value lies in.
        e.pending += 1;
        let top = e.l >= 0x4000;
        e.put(top);
        while e.arith.len() < 16 + e.shifts {
            e.arith.push(false);
        }
        let mut bits = vec![];
        for k in 0..=e.arith.len() {
            for &(_, v, n) in e.raws.iter().filter(|r| r.0 == k) {
                bits.extend((0..n).rev().map(|b| v >> b & 1 == 1));
            }
            bits.extend(e.arith.get(k));
        }
        bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, &b)| a | u8::from(b) << (7 - i))).collect()
    }

    #[test]
    fn blocks_the_encoder_made_come_back() {
        for bits in [10, 16, 21] {
            let (mut enc, mut dec) = (Quantum::new(bits), Quantum::new(bits));
            let text: Vec<u8> = b"the quick brown fox jumps over the lazy dog; "
                .iter()
                .cycle()
                .take(3000)
                .chain((0..2000u32).map(|i| (i * 7 % 251) as u8).collect::<Vec<_>>().iter())
                .copied()
                .collect();
            // Two blocks: the models and the window stay between them.
            for part in [&text[..2500], &text[2500..]] {
                let packed = pack(&mut enc, part);
                let mut out = vec![];
                dec.block(&packed, part.len(), false, &mut out).unwrap();
                assert_eq!(out, part, "window of 2^{bits}");
            }
        }
    }

    #[test]
    fn a_match_over_the_end_of_a_block_is_refused_unless_it_is_the_last() {
        let data = vec![b'a'; 100];
        for (last, ok) in [(false, false), (true, true)] {
            let packed = pack(&mut Quantum::new(12), &data);
            let mut out = vec![];
            let r = Quantum::new(12).block(&packed, 50, last, &mut out);
            assert_eq!(r.is_ok(), ok, "last: {last}");
            if ok {
                assert_eq!(out, data[..50]);
            }
        }
    }

    #[test]
    fn nonsense_never_runs_away() {
        let mut q = Quantum::new(12);
        for seed in 0..200u32 {
            let junk: Vec<u8> = (0..300u32).map(|i| (i.wrapping_mul(seed + 3) >> 3) as u8 ^ seed as u8).collect();
            let mut out = vec![];
            let _ = q.block(&junk, BLOCK, false, &mut out);
            assert!(out.len() <= BLOCK);
        }
    }
}

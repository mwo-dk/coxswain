//! Two files compared byte by byte, at the same place in both (as `cmp` does): a screen of hex
//! rows read when it is asked for, the next difference looked for from where the cursor is, and
//! the bytes that differ counted on a thread, a megabyte at a time, until the session stops.

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Bytes a row shows.
pub const WIDTH: u64 = 16;
const CHUNK: usize = 1 << 20;

/// One row: its offset and the bytes each side has there (fewer, or none, past its end).
#[derive(Clone, Debug, Serialize)]
pub struct HexRow {
    pub offset: u64,
    pub left: Vec<u8>,
    pub right: Vec<u8>,
}

/// How far the count is.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Count {
    /// Bytes that differ so far (a byte only one side has counts too).
    pub differ: u64,
    /// The first that differs, once one is found.
    pub first: Option<u64>,
    /// Bytes compared so far, of the longer side.
    pub done: u64,
    pub finished: bool,
}

pub struct Bytes {
    paths: [PathBuf; 2],
    pub len: [u64; 2],
    differ: Arc<AtomicU64>,
    first: Arc<AtomicU64>,
    done: Arc<AtomicU64>,
    finished: Arc<AtomicBool>,
}

fn read_at(f: &mut std::fs::File, at: u64, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(n);
    f.seek(SeekFrom::Start(at))?;
    f.take(n as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

impl Bytes {
    pub fn open(a: &Path, b: &Path) -> std::io::Result<Bytes> {
        let len = [std::fs::metadata(a)?.len(), std::fs::metadata(b)?.len()];
        Ok(Bytes {
            paths: [a.to_path_buf(), b.to_path_buf()],
            len,
            differ: Arc::default(),
            first: Arc::new(AtomicU64::new(u64::MAX)),
            done: Arc::default(),
            finished: Arc::default(),
        })
    }

    /// Start counting the bytes that differ, on a thread that ends when `stop` is set.
    pub fn count(&self, stop: Arc<AtomicBool>) {
        let (paths, len) = (self.paths.clone(), self.len);
        let (differ, first, done, finished) = (self.differ.clone(), self.first.clone(), self.done.clone(), self.finished.clone());
        std::thread::spawn(move || {
            let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(&paths[0]), std::fs::File::open(&paths[1])) else { return };
            let short = len[0].min(len[1]);
            let mut at = 0u64;
            while at < short && !stop.load(Ordering::Relaxed) {
                let n = CHUNK.min((short - at) as usize);
                let (Ok(x), Ok(y)) = (read_at(&mut fa, at, n), read_at(&mut fb, at, n)) else { return };
                let mut d = 0;
                for (i, (p, q)) in x.iter().zip(&y).enumerate() {
                    if p != q {
                        if d == 0 {
                            first.fetch_min(at + i as u64, Ordering::Relaxed);
                        }
                        d += 1;
                    }
                }
                differ.fetch_add(d, Ordering::Relaxed);
                at += n as u64;
                done.store(at, Ordering::Relaxed);
            }
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if len[0] != len[1] {
                first.fetch_min(short, Ordering::Relaxed);
                differ.fetch_add(len[0].max(len[1]) - short, Ordering::Relaxed);
            }
            done.store(len[0].max(len[1]), Ordering::Relaxed);
            finished.store(true, Ordering::Relaxed);
        });
    }

    pub fn counted(&self) -> Count {
        let first = self.first.load(Ordering::Relaxed);
        Count {
            differ: self.differ.load(Ordering::Relaxed),
            first: (first != u64::MAX).then_some(first),
            done: self.done.load(Ordering::Relaxed),
            finished: self.finished.load(Ordering::Relaxed),
        }
    }

    /// Rows in all.
    pub fn rows_total(&self) -> u64 {
        self.len[0].max(self.len[1]).div_ceil(WIDTH)
    }

    /// Rows `from..from + n`.
    pub fn rows(&self, from: u64, n: usize) -> std::io::Result<Vec<HexRow>> {
        let at = from * WIDTH;
        let want = n * WIDTH as usize;
        let mut sides = [vec![], vec![]];
        for (s, p) in sides.iter_mut().zip(&self.paths) {
            *s = read_at(&mut std::fs::File::open(p)?, at, want)?;
        }
        let rows = (0..n as u64).map(|r| {
            let cut = |v: &Vec<u8>| v.get((r * WIDTH) as usize..).map(|t| t[..t.len().min(WIDTH as usize)].to_vec()).unwrap_or_default();
            HexRow { offset: at + r * WIDTH, left: cut(&sides[0]), right: cut(&sides[1]) }
        });
        Ok(rows.take_while(|r| !(r.left.is_empty() && r.right.is_empty())).collect())
    }

    /// The row of the next byte that differs after row `row` (before it, `back`), read a
    /// megabyte at a time until one is found or `stop` is set.
    pub fn next(&self, row: u64, back: bool, stop: &AtomicBool) -> std::io::Result<Option<u64>> {
        let (mut fa, mut fb) = (std::fs::File::open(&self.paths[0])?, std::fs::File::open(&self.paths[1])?);
        let short = self.len[0].min(self.len[1]);
        let long = self.len[0].max(self.len[1]);
        if !back {
            let mut at = (row + 1) * WIDTH;
            while at < short && !stop.load(Ordering::Relaxed) {
                let n = CHUNK.min((short - at) as usize);
                let (x, y) = (read_at(&mut fa, at, n)?, read_at(&mut fb, at, n)?);
                if let Some(i) = x.iter().zip(&y).position(|(p, q)| p != q) {
                    return Ok(Some((at + i as u64) / WIDTH));
                }
                at += n as u64;
            }
            // Past the end of the shorter side every byte differs.
            let r = (short / WIDTH).max(row + 1);
            return Ok((long > short && r * WIDTH < long && !stop.load(Ordering::Relaxed)).then_some(r));
        }
        let mut end = (row * WIDTH).min(long);
        if end > short {
            // Past the shorter side every byte differs: the row before is one.
            return Ok(row.checked_sub(1));
        }
        while end > 0 && !stop.load(Ordering::Relaxed) {
            let n = CHUNK.min(end as usize);
            let at = end - n as u64;
            let (x, y) = (read_at(&mut fa, at, n)?, read_at(&mut fb, at, n)?);
            if let Some(i) = x.iter().zip(&y).rposition(|(p, q)| p != q) {
                return Ok(Some((at + i as u64) / WIDTH));
            }
            end = at;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_counts_and_the_next_difference() {
        let d = super::super::tests::tempfile_dir("bytes");
        let (a, b) = (d.join("a"), d.join("b"));
        let mut x = vec![0u8; 100];
        std::fs::write(&a, &x).unwrap();
        x[40] = 1;
        x.extend([7, 7]);
        std::fs::write(&b, &x).unwrap();
        let bytes = Bytes::open(&a, &b).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        bytes.count(stop.clone());
        while !bytes.counted().finished {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let c = bytes.counted();
        assert_eq!((c.differ, c.first), (3, Some(40)));
        assert_eq!(bytes.rows_total(), 7);
        let rows = bytes.rows(6, 4).unwrap();
        assert_eq!((rows.len(), rows[0].left.len(), rows[0].right.len()), (1, 4, 6));
        assert_eq!(bytes.next(0, false, &stop).unwrap(), Some(2));
        assert_eq!(bytes.next(2, false, &stop).unwrap(), Some(6));
        assert_eq!(bytes.next(6, true, &stop).unwrap(), Some(2));
        assert_eq!(bytes.next(2, true, &stop).unwrap(), None);
        std::fs::remove_dir_all(d).unwrap();
    }
}

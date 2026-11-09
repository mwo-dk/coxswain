//! A file inside an archive read as it is unpacked, never copied to the disk: the stream behind
//! the log view (`logs`) and the games (`chess`) of an entry.
//!
//! `EntryReader` is the entry's bytes in order, unpacked on a thread that waits when the reader
//! does not keep up (a few chunks are in flight, no more). `EntryFile` reads at any place, as a
//! file would: a few blocks are kept, and a place before the one the unpacking is at starts the
//! unpacking again, from the entry's start or, where the archive keeps places to restart from
//! (`archive::stream_at`: a tar), from the one before it (the way a packed PGN file is read,
//! `chess::games`).
//! Memory is bounded by the blocks and chunks, whatever the entry's size.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{Arc, Mutex};

/// Bytes handed from the unpacking thread to the reader at a time.
const CHUNK: usize = 64 * 1024;
/// Chunks in flight at most.
const IN_FLIGHT: usize = 4;
/// Bytes of a block of `EntryFile`, and how many blocks it keeps.
pub const BLOCK: u64 = 1 << 20;
pub const BLOCKS: usize = 4;

/// The bytes of the file `path` inside an archive, in order.
pub struct EntryReader {
    rx: Receiver<io::Result<Vec<u8>>>,
    chunk: Vec<u8>,
    at: usize,
}

impl EntryReader {
    /// Start unpacking `path`. Waits for the first bytes, so a file that cannot be read so
    /// (locked, not there) fails here.
    pub fn open(path: &Path) -> io::Result<EntryReader> {
        EntryReader::open_at(path, 0, Arc::default(), false)
    }

    /// `open`, also for a locked file of a 7z whose password was given (`archive::stream_in`):
    /// a wrong one fails somewhere in the read, at the end at the latest.
    pub fn open_known(path: &Path) -> io::Result<EntryReader> {
        EntryReader::open_at(path, 0, Arc::default(), true)
    }

    /// `open`, from byte `offset` of the entry on; `unpacked` counts the bytes unpacked.
    fn open_at(path: &Path, offset: u64, unpacked: Arc<AtomicU64>, known: bool) -> io::Result<EntryReader> {
        let (tx, rx) = sync_channel(IN_FLIGHT);
        let path = path.to_path_buf();
        std::thread::Builder::new().name("archive-stream".into()).spawn(move || {
            let sent = tx.clone();
            let done = crate::archive::stream_at(&path, offset, move |from, before| {
                unpacked.fetch_add(before, Ordering::Relaxed);
                loop {
                    let mut buf = vec![0u8; CHUNK];
                    let n = from.read(&mut buf)?;
                    if n == 0 {
                        return Ok(());
                    }
                    buf.truncate(n);
                    unpacked.fetch_add(n as u64, Ordering::Relaxed);
                    // The reader is gone: the unpacking stops.
                    tx.send(Ok(buf)).map_err(|_| io::Error::new(io::ErrorKind::Interrupted, "closed"))?;
                }
            }, known);
            if let Err(e) = done {
                let _ = sent.send(Err(e));
            }
        })?;
        let mut r = EntryReader { rx, chunk: vec![], at: 0 };
        r.fill()?;
        Ok(r)
    }

    /// The next chunk, when the one held is used up; an empty one at the end.
    fn fill(&mut self) -> io::Result<()> {
        if self.at >= self.chunk.len() {
            self.chunk = self.rx.recv().unwrap_or_else(|_| Ok(vec![]))?;
            self.at = 0;
        }
        Ok(())
    }
}

impl Read for EntryReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.fill()?;
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Where the unpacking is, and the blocks kept (the last used last).
struct Cursor {
    reader: Option<(EntryReader, u64)>,
    blocks: Vec<(u64, Vec<u8>)>,
}

/// A file inside an archive that can be read at any place; `try_clone` gives another cursor of
/// its own (a thread finding lines, one showing them), so they do not move each other.
pub struct EntryFile {
    path: PathBuf,
    size: u64,
    cursor: Mutex<Cursor>,
    /// Times the unpacking was started: once for a read in order, more for going back.
    opened: Arc<AtomicUsize>,
    /// Bytes unpacked in all, those skipped to reach a place included.
    unpacked: Arc<AtomicU64>,
}

impl EntryFile {
    pub fn open(path: &Path) -> io::Result<EntryFile> {
        let size = crate::archive::entry_size(path)?;
        let f = EntryFile { path: path.to_path_buf(), size, cursor: Mutex::new(Cursor { reader: None, blocks: vec![] }), opened: Arc::default(), unpacked: Arc::default() };
        // A file that cannot be read so fails now, not at the first line.
        f.read_at(&mut [0u8; 1], 0)?;
        Ok(f)
    }

    pub fn try_clone(&self) -> io::Result<EntryFile> {
        Ok(EntryFile { path: self.path.clone(), size: self.size, cursor: Mutex::new(Cursor { reader: None, blocks: vec![] }), opened: self.opened.clone(), unpacked: self.unpacked.clone() })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The size the archive lists.
    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// How many times the unpacking was started, over this file and its clones.
    pub fn unpackings(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// Bytes unpacked over this file and its clones: a read in order unpacks about the entry's
    /// size, going back far unpacks more only by the stretch from the place restarted at.
    pub fn unpacked(&self) -> u64 {
        self.unpacked.load(Ordering::Relaxed)
    }

    /// Bytes this cursor holds now.
    pub fn held(&self) -> usize {
        self.cursor.lock().unwrap_or_else(|e| e.into_inner()).blocks.iter().map(|(_, b)| b.len()).sum()
    }

    /// Where block `n` (bytes `n*BLOCK..`) is in `c.blocks`, unpacking up to it when it is not
    /// kept; the block is then the last one.
    fn load(&self, c: &mut Cursor, n: u64) -> io::Result<usize> {
        if let Some(i) = c.blocks.iter().position(|(k, _)| *k == n) {
            let b = c.blocks.remove(i);
            c.blocks.push(b);
            return Ok(c.blocks.len() - 1);
        }
        let want = n * BLOCK;
        // The blocks just before `n` are kept on the way, so paging back one block at a time
        // unpacks once per few blocks, not once per block.
        let back = n.saturating_sub(BLOCKS as u64 - 1);
        // Going back (or no unpacking yet): from the blocks kept before `n`, where the archive
        // can start there (a tar), else from the entry's start.
        if c.reader.as_ref().is_none_or(|(_, at)| *at > want) {
            c.reader = Some((EntryReader::open_at(&self.path, back * BLOCK, self.unpacked.clone(), false)?, back * BLOCK));
            self.opened.fetch_add(1, Ordering::Relaxed);
        }
        let first = back.max(c.reader.as_ref().map_or(0, |(_, at)| at.div_ceil(BLOCK)));
        let first = first.min(n);
        for k in first..=n {
            let (r, at) = c.reader.as_mut().expect("opened above");
            let skipped = io::copy(&mut r.take(k * BLOCK - (*at).min(k * BLOCK)), &mut io::sink())?;
            let mut data = Vec::with_capacity(BLOCK as usize);
            let got = r.take(BLOCK).read_to_end(&mut data)?;
            // The bytes taken: an entry that ends before its listed size leaves `at` at its end,
            // so a read past it later does not start the unpacking again (#907).
            *at += skipped + got as u64;
            c.blocks.retain(|(j, _)| *j != k);
            if c.blocks.len() >= BLOCKS {
                c.blocks.remove(0);
            }
            c.blocks.push((k, data));
        }
        Ok(c.blocks.len() - 1)
    }

    /// Read into `buf` from byte `at`, as far as the entry goes: the bytes read.
    pub fn read_at(&self, buf: &mut [u8], at: u64) -> io::Result<usize> {
        let mut c = self.cursor.lock().unwrap_or_else(|e| e.into_inner());
        let mut done = 0;
        while done < buf.len() {
            let place = at + done as u64;
            let i = self.load(&mut c, place / BLOCK)?;
            let block = &c.blocks[i].1;
            let from = (place % BLOCK) as usize;
            if from >= block.len() {
                break;
            }
            let n = (block.len() - from).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&block[from..from + n]);
            done += n;
        }
        Ok(done)
    }
}

impl Drop for EntryFile {
    fn drop(&mut self) {
        // The unpacking thread ends when its reader is dropped.
        self.cursor.lock().unwrap_or_else(|e| e.into_inner()).reader = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip holding `lines` lines of a log, made here, and its folder.
    fn zip_of(name: &str, lines: usize) -> (PathBuf, PathBuf, String) {
        let d = crate::pictures::tests::folder(name);
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let text: String = (0..lines).map(|i| format!("2026-11-03 INFO line {i} of the log\n")).collect();
        std::fs::write(src.join("big.log"), &text).unwrap();
        let a = d.join("logs.zip");
        crate::archive::create(&a, &[src.join("big.log")]).unwrap();
        (d, a, text)
    }

    /// #907: past the entry's real end (an archive whose listed size is more than it holds)
    /// nothing is read, and the unpacking is not started again for it.
    #[test]
    fn a_read_past_the_end_does_not_unpack_again() {
        let (d, a, text) = zip_of("entry-file-past-end", 70_000);
        assert!(text.len() as u64 > 2 * BLOCK && (text.len() as u64) < 3 * BLOCK);
        let f = EntryFile::open(&a.join("big.log")).unwrap();
        let mut buf = vec![0u8; 100];
        assert_eq!(f.read_at(&mut buf, 8 * BLOCK).unwrap(), 0);
        assert_eq!(f.read_at(&mut buf, 4 * BLOCK).unwrap(), 0);
        assert_eq!(f.read_at(&mut buf, 3 * BLOCK).unwrap(), 0);
        assert_eq!(f.unpackings(), 1);
        drop(f);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_entry_is_read_in_order_with_a_few_blocks_in_memory() {
        // Over eight blocks: far more than the cursor keeps.
        let (d, a, text) = zip_of("entry-file-order", 600_000);
        assert!(text.len() as u64 > 8 * BLOCK);
        let f = EntryFile::open(&a.join("big.log")).unwrap();
        assert_eq!(f.len(), text.len() as u64);
        let (mut got, mut at, mut buf) = (Vec::new(), 0u64, vec![0u8; 256 * 1024]);
        loop {
            let n = f.read_at(&mut buf, at).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
            at += n as u64;
            assert!(f.held() <= BLOCKS * BLOCK as usize, "blocks kept stay few");
        }
        assert!(got == text.as_bytes());
        assert_eq!(f.unpackings(), 1, "a read in order unpacks the entry once");
        // Going back starts again from the entry's start, and reads the same.
        let mut again = vec![0u8; 100];
        f.read_at(&mut again, 5).unwrap();
        assert_eq!(again, &text.as_bytes()[5..105]);
        assert_eq!(f.unpackings(), 2);
        // A clone has a cursor of its own.
        let g = f.try_clone().unwrap();
        g.read_at(&mut again, 3 * BLOCK).unwrap();
        assert_eq!(again, &text.as_bytes()[3 * BLOCK as usize..][..100]);
        assert_eq!(f.unpackings(), 3);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn every_kind_of_archive_streams_an_entry_and_stops_early() {
        let d = crate::pictures::tests::folder("entry-file-kinds");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let text: String = (0..300_000).map(|i| format!("line {i} of the log\n")).collect();
        std::fs::write(src.join("app.log"), &text).unwrap();
        for name in ["logs.zip", "logs.tar.gz", "logs.7z", "logs.tar"] {
            let a = d.join(name);
            crate::archive::create(&a, &[src.join("app.log")]).unwrap();
            let inside = a.join("app.log");
            let mut all = vec![];
            EntryReader::open(&inside).unwrap().read_to_end(&mut all).unwrap();
            assert!(all == text.as_bytes(), "{name}");
            // Dropped after a few bytes: the unpacking stops, nothing is left running.
            let mut r = EntryReader::open(&inside).unwrap();
            let mut few = [0u8; 9];
            r.read_exact(&mut few).unwrap();
            assert_eq!(&few, b"line 0 of");
            drop(r);
            let f = EntryFile::open(&inside).unwrap();
            let mut buf = vec![0u8; 20];
            f.read_at(&mut buf, 3 * BLOCK + 7).unwrap();
            assert_eq!(buf, &text.as_bytes()[3 * BLOCK as usize + 7..][..20], "{name}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Going back far in a big entry restarts near the place, where the archive keeps places
    /// to restart from (a tar), and from the entry's start where it does not (a zip); counted
    /// in bytes unpacked, never in time.
    #[test]
    fn going_back_restarts_from_a_kept_place_in_a_tar_and_from_the_start_in_a_zip() {
        let d = crate::pictures::tests::folder("entry-file-seek");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let text: String = (0..1_200_000).map(|i| format!("2026-11-03 INFO line {i} of the log {}\n", i % 977)).collect();
        assert!(text.len() as u64 > 40 * BLOCK);
        std::fs::write(src.join("big.log"), &text).unwrap();
        let size = text.len() as u64;
        for (name, smaller) in [("logs.tar.gz", true), ("logs.tar", true), ("logs.zip", false)] {
            let a = d.join(name);
            crate::archive::create(&a, &[src.join("big.log")]).unwrap();
            // Listed once: a gzip keeps its restart places then.
            crate::archive::list(&a, 100).unwrap();
            let f = EntryFile::open(&a.join("big.log")).unwrap();
            let mut buf = vec![0u8; 100];
            let far = size - 3 * BLOCK;
            f.read_at(&mut buf, far).unwrap();
            assert_eq!(buf, &text.as_bytes()[far as usize..][..100], "{name}");
            let to_far = f.unpacked();
            // Back to a line far above: the reader is past it, so the entry is opened again.
            f.read_at(&mut buf, 30 * BLOCK + 5).unwrap();
            assert_eq!(buf, &text.as_bytes()[30 * BLOCK as usize + 5..][..100], "{name}");
            let back = f.unpacked() - to_far;
            if smaller {
                assert!(to_far >= far, "{name}: forward is read on, as before");
                assert!(back < 12 * BLOCK, "{name}: {back} bytes unpacked to go back");
            } else {
                assert!(to_far >= far, "{name}");
                assert!(back >= 27 * BLOCK, "{name}");
            }
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_missing_entry_fails_at_open() {
        let (d, a, _) = zip_of("entry-file-missing", 10);
        assert!(EntryFile::open(&a.join("none.log")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}

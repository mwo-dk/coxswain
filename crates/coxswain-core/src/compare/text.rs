//! Two texts compared line by line. Each side is read once, a line at a time, into where its
//! lines start and a hash of each (16 bytes a line; nothing else of the file is kept); the
//! hashes are compared with a histogram diff (`similar`), after the lines both start and end
//! with are set aside, with a time limit past which it settles for a longer but still right
//! answer. What a screen shows is read from the files again when it is asked for (`rows`), and
//! the changed words of a changed line are worked out only for the lines on the screen.
//!
//! Limits: `MOST_LINES` lines a side (past them the rest is left out, and `cut` says so); a
//! line longer than `LONGEST` bytes shows its start only. A document is compared by the text
//! its reader takes out of it (`extract`), held in memory, and is never written.
//!
//! Editing is a change copied from one side to the other (`take`), undone one at a time, and
//! written by `save` in the file's own encoding and line endings (`edit::save`), for files of
//! at most `edit::MOST` bytes.

use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::Kind;
use crate::edit::{Encoding, SaveError, Stamp};

/// Lines read of each side, at most: 16 bytes each, 160 MB a side.
pub const MOST_LINES: usize = 10_000_000;
/// The most of one line shown.
pub const LONGEST: usize = 64 * 1024;
/// How long the diff may look for the shortest answer before it settles.
const PATIENCE: Duration = Duration::from_secs(5);
/// Lines longer than this have no changed words worked out: the whole line is marked.
const WORDS_MOST: usize = 4000;

/// What is set aside when lines are compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Spaces and tabs anywhere in a line.
    pub whitespace: bool,
    /// Capitals and small letters.
    pub case: bool,
}

/// Side by side, or one under the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Split,
    Inline,
}

/// The left side (0) or the right (1).
pub type Side = usize;

/// Where one side's lines come from.
enum Source {
    File(PathBuf),
    /// A document's text, or a UTF-16 file's, as UTF-8.
    Text(String),
}

/// One side, read.
struct Lines {
    source: Source,
    /// Where each line starts, and after the last where the text ends.
    starts: Vec<u64>,
    hashes: Vec<u64>,
    /// Decoded as UTF-8 (else Windows-1252).
    utf8: bool,
    /// What saving writes it in, and its line endings.
    encoding: Encoding,
    crlf: bool,
    /// Its last line ends in a line break.
    eol_at_end: bool,
    stamp: Option<Stamp>,
    /// Lines past `MOST_LINES` were left out.
    cut: bool,
}

impl Lines {
    fn len(&self) -> usize {
        self.hashes.len()
    }

    /// The whole bytes of lines `a..b`, line breaks and all, exactly as the source has them.
    fn raw(&self, a: usize, b: usize) -> std::io::Result<Vec<u8>> {
        let (s, e) = (self.starts[a], self.starts[b]);
        match &self.source {
            Source::Text(t) => Ok(t.as_bytes()[s as usize..e as usize].to_vec()),
            Source::File(p) => {
                let mut f = std::fs::File::open(p)?;
                f.seek(SeekFrom::Start(s))?;
                let mut buf = vec![0; (e - s) as usize];
                f.read_exact(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// One line's bytes as text with nothing lost: strict UTF-8, or Windows-1252 byte for byte.
    fn decode(&self, line: &[u8]) -> Option<String> {
        match (&self.source, self.encoding) {
            (Source::File(_), Encoding::Windows1252) => Some(crate::view::cp1252(line)),
            _ => String::from_utf8(line.to_vec()).ok(),
        }
    }

    /// Lines `a..b`, without their line breaks, each at most `LONGEST` bytes.
    fn read(&self, a: usize, b: usize) -> Vec<String> {
        let b = b.min(self.len());
        if a >= b {
            return vec![];
        }
        let span = |i: usize| (self.starts[i], self.starts[i + 1]);
        let tidy = |raw: &[u8]| {
            let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            crate::view::decode_line(&raw[..raw.len().min(LONGEST)], self.utf8)
        };
        match &self.source {
            Source::Text(t) => (a..b).map(|i| span(i)).map(|(s, e)| tidy(&t.as_bytes()[s as usize..e as usize])).collect(),
            Source::File(p) => {
                let Ok(mut f) = std::fs::File::open(p) else { return vec![String::new(); b - a] };
                let mut out = Vec::with_capacity(b - a);
                let mut buf = vec![];
                for i in a..b {
                    let (s, e) = span(i);
                    buf.clear();
                    let take = (e - s).min(LONGEST as u64 + 2);
                    if f.seek(SeekFrom::Start(s)).is_ok() {
                        let _ = (&mut f).take(take).read_to_end(&mut buf);
                    }
                    out.push(tidy(&buf));
                }
                out
            }
        }
    }
}

/// The hash a line is compared by, `options` set aside, its line break left out.
fn hash_line(line: &[u8], options: Options) -> u64 {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if !options.case && !options.whitespace {
        line.hash(&mut h);
        return h.finish();
    }
    let lower;
    let line: &[u8] = if options.case {
        if line.is_ascii() {
            lower = line.to_ascii_lowercase();
        } else {
            lower = String::from_utf8_lossy(line).to_lowercase().into_bytes();
        }
        &lower
    } else {
        line
    };
    for &b in line {
        if !(options.whitespace && b.is_ascii_whitespace()) {
            h.write_u8(b);
        }
    }
    h.finish()
}

/// Lines of `text` (from `from` on): where each starts, and its hash.
fn index(r: &mut impl BufRead, from: u64, options: Options, read: &AtomicU64, stop: &AtomicBool) -> Result<(Vec<u64>, Vec<u64>, bool, bool, bool), String> {
    let (mut starts, mut hashes) = (vec![from], vec![]);
    let (mut at, mut line, mut crlf, mut cut) = (from, Vec::new(), 0usize, false);
    let mut last_eol = true;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(crate::t!("compare.stopped"));
        }
        line.clear();
        // A line longer than `LONGEST` is hashed by its start, and its rest is passed over.
        let n = r.by_ref().take(LONGEST as u64 * 4).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let mut len = n as u64;
        if line.last() != Some(&b'\n') {
            if n as u64 == LONGEST as u64 * 4 {
                // The rest of a very long line.
                loop {
                    let buf = r.fill_buf().map_err(|e| e.to_string())?;
                    if buf.is_empty() {
                        break;
                    }
                    let (k, end) = match memchr::memchr(b'\n', buf) {
                        Some(i) => (i + 1, true),
                        None => (buf.len(), false),
                    };
                    r.consume(k);
                    len += k as u64;
                    if end {
                        line.push(b'\n');
                        break;
                    }
                }
            }
            last_eol = line.last() == Some(&b'\n');
        }
        if line.ends_with(b"\r\n") {
            crlf += 1;
        }
        if hashes.len() == MOST_LINES {
            cut = true;
            break;
        }
        hashes.push(hash_line(&line, options));
        at += len;
        starts.push(at);
        read.fetch_add(len, Ordering::Relaxed);
    }
    let crlf = crlf > 0 && crlf * 2 >= hashes.len();
    Ok((starts, hashes, crlf, last_eol, cut))
}

/// One side read: a text file as it is, a UTF-16 one as UTF-8, a document by its text.
fn lines_of(path: &Path, document: bool, options: Options, read: &AtomicU64, stop: &AtomicBool) -> Result<Lines, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = Some(crate::edit::Stamp::of(&meta));
    let mut head = vec![0u8; 64 * 1024];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| e.to_string())?;
    head.truncate(n);
    let utf16 = head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]);
    let text = if document && !super::looks_like_text(path) {
        let t = crate::extract::text_of(path, meta.len(), 256 << 20).unwrap_or_default();
        Some((t, Encoding::Utf8))
    } else if utf16 {
        // UTF-16 is rare and small: read whole, as the editor reads it.
        let doc = crate::edit::open(path)?;
        Some((doc.text, doc.encoding))
    } else {
        None
    };
    if let Some((t, encoding)) = text {
        read.fetch_add(meta.len(), Ordering::Relaxed);
        let (starts, hashes, crlf, eol, cut) = index(&mut t.as_bytes(), 0, options, &AtomicU64::new(0), stop)?;
        return Ok(Lines { source: Source::Text(t), starts, hashes, utf8: true, encoding, crlf, eol_at_end: eol, stamp: stamp.filter(|_| !document), cut });
    }
    let bom = head.starts_with(&[0xef, 0xbb, 0xbf]);
    let utf8 = crate::view::is_utf8(&head[if bom { 3 } else { 0 }..]);
    let mut f = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path).map_err(|e| e.to_string())?);
    let from = if bom { 3 } else { 0 };
    f.seek(SeekFrom::Start(from)).map_err(|e| e.to_string())?;
    let (starts, hashes, crlf, eol, cut) = index(&mut f, from, options, read, stop)?;
    let encoding = match (bom, utf8) {
        (true, _) => Encoding::Utf8Bom,
        (false, true) => Encoding::Utf8,
        (false, false) => Encoding::Windows1252,
    };
    Ok(Lines { source: Source::File(path.to_path_buf()), starts, hashes, utf8: utf8 || bom, encoding, crlf, eol_at_end: eol, stamp, cut })
}

/// A stretch of both sides: the same lines, or a change (a hunk).
#[derive(Clone, Debug)]
struct Seg {
    /// Where it starts on each side, and how many lines it has there.
    at: [usize; 2],
    len: [usize; 2],
    /// A change, by its number among the changes.
    hunk: Option<usize>,
    /// A change copied from that side to the other.
    taken: Option<Side>,
}

impl Seg {
    /// Lines it shows, side by side or one under the other.
    fn rows(&self, view: View) -> usize {
        match (self.hunk, self.taken, view) {
            (None, ..) => self.len[0],
            (Some(_), Some(from), _) => self.len[from],
            (Some(_), None, View::Split) => self.len[0].max(self.len[1]),
            (Some(_), None, View::Inline) => self.len[0] + self.len[1],
        }
    }
    /// Lines it has on `side` now, a copied change counted.
    fn now(&self, side: Side) -> usize {
        self.taken.map_or(self.len[side], |from| self.len[from])
    }
}

/// How a row of the compare differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Same,
    /// A line changed: both sides, its changed words marked.
    Changed,
    /// Only on the left.
    Removed,
    /// Only on the right.
    Added,
    /// A change copied to the other side, not yet saved.
    Taken,
}

/// One side of a row: its line number (from 1, as the side has it now), its text, and the
/// byte ranges of it that changed.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Cell {
    pub no: usize,
    pub text: String,
    pub marks: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub tag: Tag,
    /// The change it belongs to.
    pub hunk: Option<usize>,
    pub left: Option<Cell>,
    pub right: Option<Cell>,
}

/// What the title says of both sides.
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub changes: usize,
    pub lines: [usize; 2],
    /// Lines only on each side, of the changes.
    pub removed: usize,
    pub added: usize,
    pub encoding: [&'static str; 2],
    pub crlf: [bool; 2],
    /// Lines past `MOST_LINES` left out on a side.
    pub cut: bool,
    /// Each side can be changed and saved.
    pub editable: [bool; 2],
    /// Each side has changes not saved.
    pub dirty: [bool; 2],
    pub kind: Kind,
}

/// Two texts compared.
pub struct Compare {
    sides: [Lines; 2],
    paths: [PathBuf; 2],
    segs: Vec<Seg>,
    /// The first row of each stretch, side by side and one under the other, and the rows.
    firsts: [Vec<usize>; 2],
    /// Changes copied, the last first undone.
    taken: Vec<usize>,
    dirty: [bool; 2],
    kind: Kind,
    editable: [bool; 2],
}

/// The changed words of `a` against `b`: byte ranges of each.
pub fn words(a: &str, b: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    if a.len() > WORDS_MOST || b.len() > WORDS_MOST {
        return (vec![(0, a.len())], vec![(0, b.len())]);
    }
    let diff = similar::TextDiff::configure().algorithm(Algorithm::Patience).timeout(Duration::from_millis(50)).diff_words(a, b);
    let (mut ma, mut mb) = (vec![], vec![]);
    let (mut ia, mut ib) = (0usize, 0usize);
    let add = |v: &mut Vec<(usize, usize)>, s: usize, e: usize| match v.last_mut() {
        Some(last) if last.1 == s => last.1 = e,
        _ => v.push((s, e)),
    };
    for c in diff.iter_all_changes() {
        let n = c.value().len();
        match c.tag() {
            similar::ChangeTag::Equal => {
                ia += n;
                ib += n;
            }
            similar::ChangeTag::Delete => {
                if !c.value().trim().is_empty() {
                    add(&mut ma, ia, ia + n);
                }
                ia += n;
            }
            similar::ChangeTag::Insert => {
                if !c.value().trim().is_empty() {
                    add(&mut mb, ib, ib + n);
                }
                ib += n;
            }
        }
    }
    (ma, mb)
}

impl Compare {
    /// `a` and `b` read and compared; `stop` ends it early.
    pub fn open(a: &Path, b: &Path, kind: Kind, options: Options, read: &AtomicU64, stop: &AtomicBool) -> Result<Compare, String> {
        let document = kind == Kind::Document;
        let sides = [lines_of(a, document, options, read, stop)?, lines_of(b, document, options, read, stop)?];
        let (ha, hb) = (&sides[0].hashes, &sides[1].hashes);
        let pre = ha.iter().zip(hb).take_while(|(x, y)| x == y).count();
        let post = ha[pre..].iter().rev().zip(hb[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
        let ops = similar::capture_diff_slices_deadline(Algorithm::Histogram, &ha[pre..ha.len() - post], &hb[pre..hb.len() - post], Some(Instant::now() + PATIENCE));
        let mut segs: Vec<Seg> = vec![];
        let mut push = |at: [usize; 2], len: [usize; 2], same: bool| {
            if len == [0, 0] {
                return;
            }
            match segs.last_mut() {
                // Changes next to each other are one change.
                Some(last) if !same && last.hunk.is_some() => {
                    last.len[0] += len[0];
                    last.len[1] += len[1];
                }
                _ => segs.push(Seg { at, len, hunk: (!same).then_some(0), taken: None }),
            }
        };
        push([0, 0], [pre, pre], true);
        for op in &ops {
            let (o, n) = (op.old_range(), op.new_range());
            push([o.start + pre, n.start + pre], [o.len(), n.len()], matches!(op, DiffOp::Equal { .. }));
        }
        push([ha.len() - post, hb.len() - post], [post, post], true);
        let mut h = 0;
        for s in segs.iter_mut().filter(|s| s.hunk.is_some()) {
            s.hunk = Some(h);
            h += 1;
        }
        let small = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.len() <= crate::edit::MOST);
        let editable = [!document && small(a) && !sides[0].cut, !document && small(b) && !sides[1].cut];
        let mut c = Compare { sides, paths: [a.to_path_buf(), b.to_path_buf()], segs, firsts: [vec![], vec![]], taken: vec![], dirty: [false; 2], kind, editable };
        c.count();
        Ok(c)
    }

    /// The first row of each stretch worked out again.
    fn count(&mut self) {
        for (v, view) in [View::Split, View::Inline].into_iter().enumerate() {
            let mut at = 0;
            let mut firsts = Vec::with_capacity(self.segs.len() + 1);
            for s in &self.segs {
                firsts.push(at);
                at += s.rows(view);
            }
            firsts.push(at);
            self.firsts[v] = firsts;
        }
    }

    /// Rows in all.
    pub fn total(&self, view: View) -> usize {
        *self.firsts[view as usize].last().unwrap_or(&0)
    }

    pub fn summary(&self) -> Summary {
        let hunks = self.segs.iter().filter(|s| s.hunk.is_some() && s.taken.is_none());
        let (removed, added) = hunks.fold((0, 0), |(r, a), s| (r + s.len[0], a + s.len[1]));
        Summary {
            changes: self.segs.iter().filter(|s| s.hunk.is_some() && s.taken.is_none()).count(),
            lines: [self.sides[0].len(), self.sides[1].len()],
            removed,
            added,
            encoding: [self.sides[0].encoding.name(), self.sides[1].encoding.name()],
            crlf: [self.sides[0].crlf, self.sides[1].crlf],
            cut: self.sides.iter().any(|s| s.cut),
            editable: self.editable,
            dirty: self.dirty,
            kind: self.kind,
        }
    }

    /// The stretch row `row` is in.
    fn seg_of(&self, view: View, row: usize) -> usize {
        self.firsts[view as usize].partition_point(|&f| f <= row).saturating_sub(1).min(self.segs.len().saturating_sub(1))
    }

    /// Line numbers each side has now at the start of stretch `k` (changes copied counted).
    fn numbers_at(&self, k: usize) -> [usize; 2] {
        let mut n = [0, 0];
        for s in &self.segs[..k] {
            n[0] += s.now(0);
            n[1] += s.now(1);
        }
        n
    }

    /// Rows `from..from + n`.
    pub fn rows(&self, view: View, from: usize, n: usize) -> Vec<Row> {
        let mut out = vec![];
        if from >= self.total(view) {
            return out;
        }
        let mut k = self.seg_of(view, from);
        let mut nums = self.numbers_at(k);
        let mut skip = from - self.firsts[view as usize][k];
        while out.len() < n && k < self.segs.len() {
            let s = &self.segs[k];
            let want = (s.rows(view) - skip).min(n - out.len());
            self.seg_rows(view, s, nums, skip, want, &mut out);
            nums[0] += s.now(0);
            nums[1] += s.now(1);
            skip = 0;
            k += 1;
        }
        out
    }

    /// Rows `skip..skip + want` of stretch `s`, whose lines are numbered from `nums` + 1.
    fn seg_rows(&self, view: View, s: &Seg, nums: [usize; 2], skip: usize, want: usize, out: &mut Vec<Row>) {
        let cell = |no: usize, text: String, marks| Cell { no: no + 1, text, marks };
        match (s.hunk, s.taken) {
            (None, _) | (Some(_), Some(_)) => {
                let from = s.taken.unwrap_or(0);
                let lines = self.sides[from].read(s.at[from] + skip, s.at[from] + skip + want);
                let other = if s.taken.is_none() { self.sides[1].read(s.at[1] + skip, s.at[1] + skip + want) } else { lines.clone() };
                for (i, (l, r)) in lines.into_iter().zip(other).enumerate() {
                    out.push(Row {
                        tag: if s.taken.is_some() { Tag::Taken } else { Tag::Same },
                        hunk: s.hunk,
                        left: Some(cell(nums[0] + skip + i, l, vec![])),
                        right: Some(cell(nums[1] + skip + i, r, vec![])),
                    });
                }
            }
            (Some(h), None) => {
                let [la, lb] = s.len;
                // Only lines that have a partner get their words compared.
                let pair = |i: usize| -> (Option<String>, Option<String>) {
                    let l = (i < la).then(|| self.sides[0].read(s.at[0] + i, s.at[0] + i + 1).pop().unwrap_or_default());
                    let r = (i < lb).then(|| self.sides[1].read(s.at[1] + i, s.at[1] + i + 1).pop().unwrap_or_default());
                    (l, r)
                };
                let marked = |l: &Option<String>, r: &Option<String>| match (l, r) {
                    (Some(l), Some(r)) => words(l, r),
                    (Some(l), None) => (vec![(0, l.len())], vec![]),
                    (None, Some(r)) => (vec![], vec![(0, r.len())]),
                    _ => (vec![], vec![]),
                };
                for row in skip..skip + want {
                    match view {
                        View::Split => {
                            let (l, r) = pair(row);
                            let (ml, mr) = marked(&l, &r);
                            let tag = match (&l, &r) {
                                (Some(_), Some(_)) => Tag::Changed,
                                (Some(_), None) => Tag::Removed,
                                _ => Tag::Added,
                            };
                            out.push(Row { tag, hunk: Some(h), left: l.map(|t| cell(nums[0] + row, t, ml)), right: r.map(|t| cell(nums[1] + row, t, mr)) });
                        }
                        View::Inline => {
                            // The old lines, then the new, each marked against its partner.
                            let (i, old) = if row < la { (row, true) } else { (row - la, false) };
                            let (l, r) = pair(i);
                            let (ml, mr) = marked(&l, &r);
                            out.push(if old {
                                Row { tag: Tag::Removed, hunk: Some(h), left: l.map(|t| cell(nums[0] + i, t, ml)), right: None }
                            } else {
                                Row { tag: Tag::Added, hunk: Some(h), left: None, right: r.map(|t| cell(nums[1] + i, t, mr)) }
                            });
                        }
                    }
                }
            }
        }
    }

    /// The first row of the change after (or before, `back`) row `row`, none past the last.
    pub fn next(&self, view: View, row: usize, back: bool) -> Option<usize> {
        let firsts = &self.firsts[view as usize];
        let starts = self.segs.iter().enumerate().filter(|(_, s)| s.hunk.is_some() && s.taken.is_none()).map(|(k, _)| firsts[k]);
        if back { starts.filter(|&r| r < row).last() } else { starts.into_iter().find(|&r| r > row) }
    }

    /// The change row `row` is in, if it is in one not copied yet.
    pub fn hunk_at(&self, view: View, row: usize) -> Option<usize> {
        let s = &self.segs[self.seg_of(view, row)];
        s.hunk.filter(|_| s.taken.is_none())
    }

    /// The change `hunk` copied from side `from` to the other; false when that side cannot be
    /// changed.
    pub fn take(&mut self, hunk: usize, from: Side) -> bool {
        let to = from ^ 1;
        if !self.editable[to] {
            return false;
        }
        let Some(k) = self.segs.iter().position(|s| s.hunk == Some(hunk) && s.taken.is_none()) else { return false };
        self.segs[k].taken = Some(from);
        self.taken.push(k);
        self.dirty[to] = true;
        self.count();
        true
    }

    /// The last change copied, put back; its first row.
    pub fn undo(&mut self, view: View) -> Option<usize> {
        let k = self.taken.pop()?;
        let from = self.segs[k].taken.take()?;
        self.dirty[from ^ 1] = self.taken.iter().any(|&j| self.segs[j].taken == Some(from));
        self.count();
        Some(self.firsts[view as usize][k])
    }

    /// The text side `side` has now, from the whole bytes of each line (never the cut, shown
    /// ones); an error when a side it takes lines from cannot be read as it was, or changed on
    /// the disk since (where its lines start is known no more).
    fn text(&self, side: Side) -> Result<String, SaveError> {
        let mut text = String::new();
        let mut eol = self.sides[side].eol_at_end;
        for s in &self.segs {
            let from = s.taken.unwrap_or(side);
            let (a, b) = (s.at[from], s.at[from] + s.len[from]);
            if a == b {
                continue;
            }
            let l = &self.sides[from];
            if from != side && l.stamp.is_some_and(|st| crate::edit::changed(&self.paths[from], &st)) {
                return Err(SaveError::Io(crate::t!("compare.cannot_save")));
            }
            let raw = l.raw(a, b).map_err(|e| SaveError::Io(e.to_string()))?;
            let base = l.starts[a];
            for i in a..b {
                let line = &raw[(l.starts[i] - base) as usize..(l.starts[i + 1] - base) as usize];
                let line = line.strip_suffix(b"\n").unwrap_or(line);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                text.push_str(&l.decode(line).ok_or_else(|| SaveError::Io(crate::t!("compare.cannot_save")))?);
                text.push('\n');
            }
            eol = if b == l.len() { l.eol_at_end } else { true };
        }
        if !eol {
            text.pop();
        }
        Ok(text)
    }

    /// The sides with changes written, each in its encoding and line endings; one changed on the
    /// disk since it was read is not written unless `force`. A side written is no longer as it
    /// was read: the apps compare the two files again after a save.
    pub fn save(&mut self, force: bool) -> Result<(), SaveError> {
        for side in 0..2 {
            if !self.dirty[side] {
                continue;
            }
            let l = &self.sides[side];
            if !force && l.stamp.is_some_and(|st| crate::edit::changed(&self.paths[side], &st)) {
                return Err(SaveError::Changed);
            }
            let text = self.text(side)?;
            crate::edit::save(&self.paths[side], &text, l.encoding, l.crlf, if force { None } else { l.stamp.as_ref() })?;
            self.dirty[side] = false;
        }
        Ok(())
    }

    /// Lines `a..b` of side `side` as the file has them (for colouring code from a little above).
    pub fn lines(&self, side: Side, a: usize, b: usize) -> Vec<String> {
        self.sides[side].read(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(a: &str, b: &str, options: Options) -> (Compare, PathBuf) {
        let d = super::super::tests::tempfile_dir(&format!("text-{}", a.len() * 31 + b.len()));
        let (pa, pb) = (d.join("a.txt"), d.join("b.txt"));
        std::fs::write(&pa, a).unwrap();
        std::fs::write(&pb, b).unwrap();
        (Compare::open(&pa, &pb, Kind::Text, options, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap(), d)
    }

    #[test]
    fn rows_side_by_side_and_inline() {
        let (c, d) = compare("one\ntwo\nthree\nfour\n", "one\nTWO 2\nthree\nfour\nfive\n", Options::default());
        let s = c.summary();
        assert_eq!((s.changes, s.removed, s.added), (2, 1, 2));
        let rows = c.rows(View::Split, 0, 100);
        assert_eq!(rows.iter().map(|r| r.tag).collect::<Vec<_>>(), [Tag::Same, Tag::Changed, Tag::Same, Tag::Same, Tag::Added]);
        assert_eq!(rows[4].right.as_ref().map(|c| (c.no, c.text.as_str())), Some((5, "five")));
        assert!(rows[4].left.is_none());
        let inline = c.rows(View::Inline, 1, 2);
        assert_eq!(inline[0].left.as_ref().unwrap().text, "two");
        assert_eq!(inline[1].right.as_ref().unwrap().text, "TWO 2");
        assert_eq!(c.total(View::Inline), 6);
        assert_eq!(c.next(View::Split, 0, false), Some(1));
        assert_eq!(c.next(View::Split, 1, false), Some(4));
        assert_eq!(c.next(View::Split, 4, true), Some(1));
        assert_eq!(c.next(View::Split, 4, false), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn options_set_spaces_and_case_aside() {
        let (c, d) = compare("a  b\nC\n", "a b\nc\n", Options { whitespace: true, case: true });
        assert_eq!(c.summary().changes, 0);
        std::fs::remove_dir_all(d).unwrap();
        let (c, d) = compare("a  b\nC\n", "a b\nc\n", Options::default());
        assert_eq!(c.summary().changes, 1);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn changed_words_are_marked() {
        let (a, b) = words("let x = 1;", "let y = 1;");
        assert_eq!((a, b), (vec![(4, 5)], vec![(4, 5)]));
    }

    #[test]
    fn a_change_copied_saved_and_undone() {
        let (mut c, d) = compare("one\r\ntwo\r\nthree\r\n", "one\nTWO\nthree\nfour", Options::default());
        assert_eq!(c.summary().changes, 2);
        let first = c.hunk_at(View::Split, 1).unwrap();
        assert!(c.take(first, 0));
        assert_eq!(c.summary().changes, 1);
        assert_eq!(c.rows(View::Split, 1, 1)[0].tag, Tag::Taken);
        assert_eq!(c.undo(View::Split), Some(1));
        assert!(c.take(first, 0));
        c.save(false).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "one\ntwo\nthree\nfour");
        // A side written is read again before anything more is taken from it.
        assert!(c.take(c.hunk_at(View::Split, 3).unwrap(), 1));
        assert!(matches!(c.save(false), Err(SaveError::Io(_))));
        let mut c = Compare::open(&d.join("a.txt"), &d.join("b.txt"), Kind::Text, Options::default(), &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        // The other way, onto the CRLF file: its line endings stay.
        let last = c.hunk_at(View::Split, 3).unwrap();
        assert!(c.take(last, 1));
        c.save(false).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "one\r\ntwo\r\nthree\r\nfour");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn saving_keeps_long_lines_and_windows_1252_whole() {
        let long = "x".repeat(LONGEST * 2);
        let left = format!("{long}\nsame\n");
        let d = super::super::tests::tempfile_dir("long");
        let (pa, pb) = (d.join("a.txt"), d.join("b.txt"));
        std::fs::write(&pa, &left).unwrap();
        // Windows-1252: é is one byte, and a "\xc3\xa9" that would read as UTF-8 é stays two.
        std::fs::write(&pb, b"caf\xe9 \xc3\xa9\nsame\n").unwrap();
        let mut c = Compare::open(&pa, &pb, Kind::Text, Options::default(), &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        assert!(c.take(0, 0));
        c.save(false).unwrap();
        assert_eq!(std::fs::read_to_string(&pb).unwrap(), left);
        std::fs::write(&pb, b"caf\xe9 \xc3\xa9\nsame\n").unwrap();
        let mut c = Compare::open(&pa, &pb, Kind::Text, Options::default(), &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        assert!(c.take(0, 1));
        c.save(false).unwrap();
        assert_eq!(std::fs::read(&pa).unwrap(), b"caf\xe9 \xc3\xa9\nsame\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_side_gone_or_changed_is_never_saved_from() {
        let (mut c, d) = compare("a\nz\n", "b\nz\n", Options::default());
        assert!(c.take(0, 0));
        std::fs::remove_file(d.join("a.txt")).unwrap();
        assert!(matches!(c.save(false), Err(SaveError::Io(_))));
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "b\nz\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_changed_meanwhile_is_not_saved_over() {
        let (mut c, d) = compare("a\n", "b\n", Options::default());
        assert!(c.take(0, 0));
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(d.join("b.txt"), "bb\n").unwrap();
        assert_eq!(c.save(false), Err(SaveError::Changed));
        c.save(true).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "a\n");
        std::fs::remove_dir_all(d).unwrap();
    }
}

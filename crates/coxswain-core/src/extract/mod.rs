//! The text of a file, by its kind, for the search store.
//!
//! One module per family of formats, each with `pub fn text(path, max) -> Option<String>`:
//! the file's text, or `None` when it has none or cannot be read. `text_of` picks the module
//! by the file's extension; anything else is read as plain text.
//!
//! The files come from anywhere, so a reader must hold for any bytes at all:
//! - never panic, never loop without end, never read or build more than its limits allow;
//! - no network, no other programs, nothing written anywhere.
//!
//! The exceptions are `installed`: programs the user has installed (OCR, LibreOffice), run
//! at low priority with a time limit, for what no reader here can get; `tika`, Apache Tika
//! in a container without network, when the user turns it on; and calibre (`crate::ebook`),
//! installed or in a container without network, when the user turns e-books on.
//!
//! `text_of` also catches a panic from a reader (they lean on other people's parsers) and
//! cuts the text at `MAX_TEXT`.

use std::io::Read;
use std::path::Path;

pub mod book;
pub mod diagram;
pub mod installed;
pub mod mail;
pub mod msg;
pub mod notebook;
pub mod pdf;
pub mod plain;
pub mod rtf;
pub mod sheet;
pub mod slides;
pub mod tika;
pub mod word;

/// The most text kept of one file. A book is about 1 MB; past this it is a data dump.
pub const MAX_TEXT: usize = 4 * 1024 * 1024;
/// The most one entry of a zip may unpack to. Office files are zips, and a zip can claim a
/// few bytes and unpack to gigabytes.
pub const MAX_ENTRY: u64 = 64 * 1024 * 1024;

type Reader = fn(&Path, u64) -> Option<String>;

/// The reader for a file name, by its extension. `None`: read it as plain text.
fn reader(path: &Path) -> Option<Reader> {
    // A PGN file, packed too, or a PDN file: a line per game, its players, event, opening and
    // result.
    if matches!(crate::chess::format(path), Some(crate::chess::Format::Pgn | crate::chess::Format::Pdn | crate::chess::Format::Kif | crate::chess::Format::Csa | crate::chess::Format::Pbn)) {
        return Some(|path, _| crate::chess::games::search_text(path, MAX_TEXT));
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => |path, max| pdf::text(path, max).or_else(|| installed::scanned_pdf(path)),
        "docx" | "docm" | "dotx" | "odt" | "ott" => word::text,
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => sheet::text,
        "pptx" | "ppsx" | "potx" | "odp" => slides::text,
        "epub" | "html" | "htm" | "xhtml" => book::text,
        "ipynb" | "drawio" | "dio" => notebook::text,
        "eml" | "mbox" => mail::text,
        "msg" => msg::text,
        "dbf" => crate::dbf::search_text,
        "sgf" => |path, _| crate::go::search_text(path, MAX_TEXT),
        "rtf" => rtf::text,
        // Diagrams as text: their words, and a sentence per arrow.
        "mmd" | "mermaid" => |path, max| plain::text(path, max).map(diagram::mermaid),
        "dot" | "gv" => |path, max| plain::text(path, max).map(diagram::graphviz),
        "puml" | "plantuml" | "pu" | "iuml" | "wsd" => {
            |path, max| plain::text(path, max).map(diagram::plantuml)
        }
        "md" | "markdown" | "mdx" => |path, max| plain::text(path, max).map(diagram::markdown),
        e if installed::PICTURES.contains(&e) => installed::picture,
        // LibreOffice first, else Apache Tika when it is turned on.
        e if installed::OFFICE.contains(&e) => {
            |path, max| installed::office(path, max).or_else(|| tika::text(path, max))
        }
        e if tika::FORMATS.contains(&e) => tika::text,
        e if installed::POSTSCRIPT.contains(&e) => installed::postscript,
        // E-books calibre reads, when `[preview] ebooks` is on (`ebook`).
        e if crate::ebook::FORMATS.contains(&e) => crate::ebook::text,
        _ => return None,
    })
}

/// Whether the file at `path` is read line by line (`plain::tidied`), whatever its size: text,
/// code, CSV and logs, the kinds no reader of its own reads.
pub(crate) fn streams(path: &Path) -> bool {
    reader(path).is_none()
}

/// The text of the file at `path`, `size` bytes long, if it has any and is no larger than
/// `max`. Runs of blank space become one space or one line break; code keeps its indentation
/// and a blank line between its parts. None for a file only in the cloud, unless the settings
/// read those: reading it would download it.
pub fn text_of(path: &Path, size: u64, max: u64) -> Option<String> {
    text_up_to(path, size, max, MAX_TEXT)
}

/// The most text *Everything* (`long_files`) keeps of a file read line by line. Search by
/// words keeps a file's text in one row, read and written whole.
// ponytail: one row per file; past this, text in parts (rows per part) if anyone needs more.
pub const EVERYTHING_TEXT: usize = 256 << 20;

/// `text_of`, keeping up to `cap` bytes of the text of a file read line by line (text, code,
/// CSV, logs); every other reader stops at `MAX_TEXT` as ever.
pub fn text_up_to(path: &Path, size: u64, max: u64, cap: usize) -> Option<String> {
    // A PGN file of any size: its reader stops after a line per game for MAX_TEXT bytes.
    let pgn = matches!(crate::chess::format(path), Some(crate::chess::Format::Pgn | crate::chess::Format::Pdn | crate::chess::Format::Kif | crate::chess::Format::Csa | crate::chess::Format::Pbn));
    // Plain text, code, CSV and logs of any size: read line by line, they stop at MAX_TEXT.
    let streamed = pgn || streams(path);
    if size == 0 || (size > max && !streamed) || crate::cloud::keep_out(path) {
        return None;
    }
    let code = crate::code::is_code(&path.to_string_lossy());
    let text = match reader(path) {
        Some(read) => {
            let text = std::panic::catch_unwind(|| read(path, max)).ok().flatten()?;
            if code { tidy_code(&text) } else { tidy(&text) }
        }
        // Read as it is tidied, line by line: no more of the file than the text kept needs.
        None => plain::tidied_up_to(path, code, cap)?,
    };
    (!text.trim().is_empty()).then_some(text)
}

/// Lines of text, tidied as `tidy` (or `tidy_code`) tidies them, each handed to `f` as it is
/// made, without its line break: an empty one for a blank line kept in code.
pub(crate) struct TidyLines<'f> {
    t: Tidy,
    first: bool,
    f: &'f mut dyn FnMut(&str),
}

impl<'f> TidyLines<'f> {
    pub fn new(code: bool, f: &'f mut dyn FnMut(&str)) -> TidyLines<'f> {
        TidyLines {
            t: Tidy::whole(code),
            first: true,
            f,
        }
    }

    pub fn line(&mut self, l: &str) {
        self.t.push(l);
        // What was added: the line, after one line break (two before a blank line in code)
        // unless it is the first.
        let piece = self.t.drain();
        if piece.is_empty() {
            return;
        }
        let line = piece.trim_start_matches('\n');
        let breaks = piece.len() - line.len();
        if !self.first {
            for _ in 1..breaks {
                (self.f)("");
            }
        }
        self.first = false;
        (self.f)(line);
    }
}

/// Whether the file is a document whose passages can be made from all of it (`whole_lines`):
/// Word and OpenDocument text, workbooks of XML in a zip and presentations.
pub(crate) fn whole_document(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "docx" | "docm" | "dotx" | "odt" | "ott" | "xlsx" | "xlsm" | "ods" | "pptx" | "ppsx" | "potx" | "odp"
    )
}

/// The most XML unpacked of one document for its passages (`whole_lines`), read as it is
/// unpacked: no part of it is held.
const WHOLE_ROOM: u64 = 4 << 30;

/// Each line of the whole text of such a document (`whole_document`), tidied as `text_of`
/// tidies it, to `f` as it is read: not cut at `MAX_TEXT`, and never held whole. The same lines
/// as `text_of` gives, for a document with less text than that. `None` when the file cannot
/// be read, or a reader gave up.
pub(crate) fn whole_lines(path: &Path, f: &mut dyn FnMut(&str)) -> Option<()> {
    if !whole_document(path) || crate::cloud::keep_out(path) {
        return None;
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let slides = matches!(ext.as_str(), "pptx" | "ppsx" | "potx" | "odp");
    let mut lines = TidyLines::new(false, f);
    let mut sink = |chunk: &str| {
        let chunk = if slides { slides::clean(chunk.to_string()) } else { chunk.to_string() };
        chunk.lines().for_each(|l| lines.line(l));
    };
    let mut out = Text::to(&mut sink);
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match ext.as_str() {
        "xlsx" | "xlsm" | "ods" => sheet::read_into(path, u64::MAX, &mut out),
        _ if slides => slides::read_into(path, u64::MAX, WHOLE_ROOM, &mut out),
        _ => word::read_into(path, u64::MAX, &mut out),
    }))
    .ok()
    .flatten();
    out.end();
    read
}

/// Code as `tidy` leaves text, but with each line's indentation and one blank line where it
/// has some.
pub fn tidy_code(text: &str) -> String {
    tidied(text.lines(), true)
}

/// One space between words, one line break between lines, no more than `MAX_TEXT` bytes, and
/// the text as `seen` leaves it.
pub fn tidy(text: &str) -> String {
    tidied(text.lines(), false)
}

fn tidied<'a>(lines: impl Iterator<Item = &'a str>, code: bool) -> String {
    let mut t = Tidy::new(code);
    for l in lines {
        if !t.push(l) {
            break;
        }
    }
    t.out
}

/// A text kept whole (*Everything*, `long_files`) cut back to what is kept of a file, as
/// reading it again would cut it: tidying tidied text changes nothing. `None` when it is no
/// longer than that.
pub(crate) fn cut_back(body: &str, code: bool) -> Option<String> {
    (body.len() > MAX_TEXT).then(|| tidied(body.lines(), code))
}

/// `tidy` (or `tidy_code`) a line at a time, for text read as it comes: the same text in, the
/// same out, whether whole or line by line.
pub(crate) struct Tidy {
    pub out: String,
    line: String,
    blank: bool,
    code: bool,
    /// The most bytes of text (`MAX_TEXT`), and how many there are so far: `out` may be
    /// taken away as it grows (`drain`).
    cap: usize,
    len: usize,
}

impl Tidy {
    pub fn new(code: bool) -> Tidy {
        Tidy {
            out: String::new(),
            line: String::new(),
            blank: false,
            code,
            cap: MAX_TEXT,
            len: 0,
        }
    }

    /// No limit: for text that is handed on as it is made (`drain`), never kept whole.
    pub fn whole(code: bool) -> Tidy {
        Tidy::with_cap(code, usize::MAX)
    }

    /// At most `cap` bytes of text.
    pub fn with_cap(code: bool, cap: usize) -> Tidy {
        Tidy {
            cap,
            ..Tidy::new(code)
        }
    }

    /// The text made since the last call, taken out.
    pub fn drain(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    /// Adds a line (without its line break); `false` once the text is full.
    pub fn push(&mut self, l: &str) -> bool {
        let before = self.out.len();
        let go = self.push_to(l);
        self.len += self.out.len() - before;
        go
    }

    fn push_to(&mut self, l: &str) -> bool {
        let (out, len, cap) = (&mut self.out, self.len, self.cap);
        if self.code {
            let line = seen(l);
            let line = line.trim_end();
            if line.is_empty() {
                self.blank = len > 0;
                return true;
            }
            let start = if len == 0 {
                ""
            } else if self.blank {
                "\n\n"
            } else {
                "\n"
            };
            self.blank = false;
            if len + start.len() + line.len() > cap {
                return false;
            }
            out.push_str(start);
            out.push_str(line);
            return true;
        }
        let line = &mut self.line;
        line.clear();
        for word in seen(l).split_whitespace() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if line.is_empty() {
            return true;
        }
        let mut len = len;
        if len > 0 {
            out.push('\n');
            len += 1;
        }
        // The line that does not fit is cut at a word, and the text ends there.
        if len + line.len() > cap {
            let room = start(line, cap.saturating_sub(len));
            out.push_str(&room[..room.rfind(' ').unwrap_or(room.len())]);
            return false;
        }
        out.push_str(line);
        true
    }
}

/// The bytes as text, as far as they are UTF-8. A part of an office file is UTF-8 through and
/// through; what a hostile file puts after a byte that is not is left out, and nothing is
/// copied: a byte that is no UTF-8 would take three as a replacement character.
pub fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes)
        .unwrap_or_else(|e| std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default())
}

/// Whether bytes that are not UTF-8 are UTF-8 with a byte or two broken, as where a file was cut
/// off or a letter was pasted in from elsewhere. In any other encoding nearly every letter
/// beyond ASCII is broken UTF-8. A sample says which, for the whole may be 64 MB and broken
/// bytes are slow to look at.
pub(crate) fn nearly_utf8(bytes: &[u8]) -> bool {
    let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 16)]);
    let broken = sample.matches(char::REPLACEMENT_CHARACTER).count();
    let whole = sample.chars().filter(|c| !c.is_ascii()).count() - broken;
    whole > broken
}

/// A line without what stands in it and is no part of what it says, copied only when it has
/// some. A control character that is no blank space becomes a space: the store marks the words
/// it found with two of them, a terminal takes others for commands, and one between two words
/// parts them. The soft hyphen, the zero width space, the word joiner and a byte order mark
/// inside the text go: they say where a word may be broken at the end of a line or may not,
/// and left in, they end the word for the store, and "bud-get" is not found as "budget".
fn seen(line: &str) -> std::borrow::Cow<'_, str> {
    let hidden = |c: char| matches!(c, '\u{AD}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}');
    let control = |c: char| c.is_control() && !c.is_whitespace();
    if !line.contains(|c: char| hidden(c) || control(c)) {
        return line.into();
    }
    line.chars()
        .filter(|c| !hidden(*c))
        .map(|c| if control(c) { ' ' } else { c })
        .collect::<String>()
        .into()
}

/// The start of a text: no more than `most` bytes of it, and no part of a character.
fn start(text: &str, most: usize) -> &str {
    &text[..text.floor_char_boundary(most)]
}

/// Adds to `out` as much of `text` as keeps it within `most` bytes.
pub(crate) fn push_within(out: &mut String, text: &str, most: usize) {
    out.push_str(start(text, most.saturating_sub(out.len())));
}

/// `push_within` for a reader's `Text`.
pub(crate) fn keep_within(out: &mut Text, text: &str, most: usize) {
    out.push_str(start(text, most.saturating_sub(out.len())));
}

// ---------------------------------------------------------------- for the readers

/// The value of the element's attribute of that name, whatever its prefix. A malformed
/// attribute is passed over, and a value that cannot be normalised is given as it is.
fn attr(element: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    let found = element
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)?;
    Some(
        found
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .unwrap_or(found.value.clone())
            .into_owned(),
    )
}

/// The zip at `path`, if it is one and no larger than `max`, read through `Metered`.
pub(super) fn open_zip(path: &Path, max: u64) -> Option<zip::ZipArchive<Metered>> {
    let file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size > max {
        return None;
    }
    zip::ZipArchive::new(Metered {
        file,
        left: size.saturating_mul(8).saturating_add(1 << 20),
    })
    .ok()
}

/// A file that gives no more than `left` bytes, all told. zip looks from the end of the file
/// for the record that says where its directory is, and when the directory is broken, tries
/// the next such record before it, and the next, each time reading as far as the record
/// points: a file of nothing but such records costs it the square of its length, an hour for
/// 20 MB. A document is read once through, and a few times its length is more than enough.
pub(super) struct Metered {
    file: std::fs::File,
    left: u64,
}

impl Read for Metered {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::other("read more than a document needs"));
        }
        let most = usize::try_from(self.left)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = self.file.read(&mut buf[..most])?;
        self.left -= n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Metered {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(pos)
    }
}

/// Text as a reader makes it: kept, as far as the reader keeps it (`MAX_TEXT`), or handed on
/// to a sink a few lines at a time and never held whole (`Text::to`), for the passages of a
/// whole document (`whole_lines`). A reader stops when its text is `MAX_TEXT` long: `len` is
/// what is held, so a reader whose text goes to a sink reads the whole file.
pub(crate) struct Text<'a> {
    held: String,
    last: Option<char>,
    sink: Option<&'a mut dyn FnMut(&str)>,
    /// The time spent in the sink (a model making the vectors of each batch of passages):
    /// not the reader's, so it does not count against a reader's time limit.
    pub waited: std::time::Duration,
}

impl std::ops::Deref for Text<'_> {
    type Target = str;

    /// The text held: all of it, unless it goes to a sink.
    fn deref(&self) -> &str {
        &self.held
    }
}

impl<'a> Text<'a> {
    pub fn new() -> Text<'a> {
        Text {
            held: String::new(),
            last: None,
            sink: None,
            waited: std::time::Duration::ZERO,
        }
    }

    /// Text handed to `sink` as it is made, whole lines at a time, the last at `end`.
    pub fn to(sink: &'a mut dyn FnMut(&str)) -> Text<'a> {
        Text {
            sink: Some(sink),
            ..Text::new()
        }
    }

    /// Whether the text goes to a sink.
    pub fn whole(&self) -> bool {
        self.sink.is_some()
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.last.is_none()
    }

    pub fn ends_with(&self, c: char) -> bool {
        self.last == Some(c)
    }

    pub fn push(&mut self, c: char) {
        self.push_str(c.encode_utf8(&mut [0; 4]));
    }

    pub fn extend(&mut self, chars: impl IntoIterator<Item = char>) {
        self.push_str(&chars.into_iter().collect::<String>());
    }

    pub fn push_str(&mut self, text: &str) {
        let Some(last) = text.chars().next_back() else {
            return;
        };
        self.held.push_str(text);
        self.last = Some(last);
        // Whole lines go to the sink, the last line break looked for only in what came.
        if self.held.len() >= 1 << 16
            && let Some(sink) = self.sink.as_mut()
            && let Some(at) = text.rfind('\n')
        {
            let at = self.held.len() - text.len() + at;
            let began = std::time::Instant::now();
            sink(&self.held[..=at]);
            self.waited += began.elapsed();
            self.held.drain(..=at);
        }
    }

    /// The text kept, or what is left of it for the sink, handed to it.
    pub fn end(mut self) -> String {
        if let Some(sink) = self.sink.as_mut() {
            sink(&self.held);
            self.held.clear();
        }
        self.held
    }
}

/// Bytes read from `inner` as far as they are UTF-8, as `utf8` cuts them, a piece at a time:
/// what a part of a zip is parsed through as it is unpacked, so no part is held whole. A
/// character cut off at the end is left out, as is all after a byte that is no UTF-8.
pub(crate) struct Utf8<R> {
    inner: R,
    buf: Box<[u8]>,
    /// The bytes not yet handed on, `at..valid`, and those after them, `valid..filled`: the
    /// start of a character whose rest is still to come.
    at: usize,
    valid: usize,
    filled: usize,
    /// How many bytes came from `inner`.
    pub read: u64,
    /// Whether a byte that is no UTF-8 (or a character cut off at the end) ended it, one was a
    /// zero byte, and `inner` failed.
    pub broken: bool,
    pub zero: bool,
    pub failed: bool,
}

impl<R: Read> Utf8<R> {
    pub fn new(inner: R) -> Utf8<R> {
        Utf8 {
            inner,
            // Small pieces: a zip that breaks off loses what it unpacked in the read that failed.
            buf: vec![0; 8 << 10].into_boxed_slice(),
            at: 0,
            valid: 0,
            filled: 0,
            read: 0,
            broken: false,
            zero: false,
            failed: false,
        }
    }

    /// Reads the rest of `inner` without keeping it, so that `read` counts all of it, `broken`
    /// and `zero` say what all of it holds, and a fault at its end (a zip checks a part's
    /// checksum there) is in `failed`.
    pub fn finish(mut self) -> Utf8<R> {
        use std::io::BufRead;
        while let Ok(n) = self.fill_buf().map(<[u8]>::len) {
            if n == 0 {
                break;
            }
            self.consume(n);
        }
        while !self.failed {
            match self.inner.read(&mut self.buf) {
                Ok(0) => break,
                Ok(n) => self.read += n as u64,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => self.failed = true,
            }
        }
        self
    }
}

impl<R: Read> Read for Utf8<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = {
            let have = std::io::BufRead::fill_buf(self)?;
            let n = have.len().min(out.len());
            out[..n].copy_from_slice(&have[..n]);
            n
        };
        std::io::BufRead::consume(self, n);
        Ok(n)
    }
}

impl<R: Read> std::io::BufRead for Utf8<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        while self.at == self.valid && !self.broken {
            // The start of a character moves to the front, and more is read after it.
            self.buf.copy_within(self.valid..self.filled, 0);
            (self.at, self.valid, self.filled) = (0, 0, self.filled - self.valid);
            let n = match self.inner.read(&mut self.buf[self.filled..]) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.failed = true;
                    return Err(e);
                }
            };
            if n == 0 {
                // A character cut off at the end: no UTF-8 either.
                self.broken |= self.filled > 0;
                break;
            }
            self.read += n as u64;
            let fresh = self.filled;
            self.filled += n;
            match std::str::from_utf8(&self.buf[..self.filled]) {
                Ok(_) => self.valid = self.filled,
                Err(e) => {
                    self.valid = e.valid_up_to();
                    self.broken = e.error_len().is_some();
                }
            }
            self.zero |= self.buf[fresh.min(self.valid)..self.valid].contains(&0);
        }
        Ok(&self.buf[self.at..self.valid])
    }

    fn consume(&mut self, n: usize) {
        self.at = (self.at + n).min(self.valid);
    }
}

/// "slide10.xml" after "slide9.xml": digits compare as numbers.
fn natural(name: &str) -> Vec<(String, u64)> {
    let mut parts = vec![];
    let mut chars = name.chars().peekable();
    while chars.peek().is_some() {
        let text: String = std::iter::from_fn(|| chars.next_if(|c| !c.is_ascii_digit())).collect();
        let digits: String = std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_digit())).collect();
        parts.push((text, digits.parse().unwrap_or(u64::MAX)));
    }
    parts
}

fn unpack(entry: impl Read) -> Option<Vec<u8>> {
    let mut bytes = vec![];
    entry.take(MAX_ENTRY + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_ENTRY).then_some(bytes)
}

/// The text inside an XML document, tags dropped and entities resolved. An element whose
/// name (without its prefix, and in whatever case) is in `lines` ends a line where it
/// closes; one in `skip` is left out with all inside it. What is left of a broken document
/// is still returned. No more than `MAX_TEXT` bytes are built: one text may be as long as
/// the document, which may be `MAX_ENTRY`.
pub fn xml_text(xml: &[u8], lines: &[&str], skip: &[&str]) -> String {
    let mut out = Text::new();
    tags_dropped(utf8(xml).as_bytes(), lines, skip, false, &mut out);
    out.end()
}

/// `xml_text` of a document as it is read, `Utf8` cutting it as `utf8` cuts bytes: never more
/// of it in memory than one piece between two tags.
pub(crate) fn xml_text_of(xml: impl std::io::BufRead, lines: &[&str], skip: &[&str]) -> String {
    let mut out = Text::new();
    tags_dropped(xml, lines, skip, false, &mut out);
    out.end()
}

/// `xml_text_of` onto `out`.
pub(crate) fn xml_text_onto(xml: impl std::io::BufRead, lines: &[&str], skip: &[&str], out: &mut Text) {
    tags_dropped(xml, lines, skip, false, out)
}

/// The text of a web page, as `xml_text` reads it, but an element in `lines` ends a line
/// where it opens as well: a page closes no `<br>`, and often no `<td>` or `<li>`.
pub fn page_text(xml: &[u8], lines: &[&str], skip: &[&str]) -> String {
    let mut out = Text::new();
    tags_dropped(utf8(xml).as_bytes(), lines, skip, true, &mut out);
    out.end()
}

/// The text of UTF-8 `xml`, read as it comes.
fn tags_dropped(xml: impl std::io::BufRead, lines: &[&str], skip: &[&str], opens: bool, out: &mut Text) {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().check_end_names = false;
    let (mut skipping, mut buf) = (0usize, vec![]);
    let named = |name: &str, among: &[&str]| among.iter().any(|one| one.eq_ignore_ascii_case(name));
    while out.len() < MAX_TEXT {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if skipping > 0 || named(e.local_name().as_ref(), skip) => {
                skipping += 1
            }
            Ok(Event::Start(e)) if opens && named(e.local_name().as_ref(), lines) => out.push('\n'),
            Ok(Event::End(e)) => {
                if skipping > 0 {
                    skipping -= 1;
                } else if named(e.local_name().as_ref(), lines) {
                    out.push('\n');
                }
            }
            Ok(Event::Empty(e)) if skipping == 0 && named(e.local_name().as_ref(), lines) => {
                out.push('\n')
            }
            Ok(Event::Text(t)) if skipping == 0 => {
                out.push_str(start(&t.into_inner(), MAX_TEXT.saturating_sub(out.len())))
            }
            Ok(Event::CData(t)) if skipping == 0 => {
                out.push_str(start(&t.into_inner(), MAX_TEXT.saturating_sub(out.len())))
            }
            // &amp; and &#233; arrive on their own. An entity the document made up is dropped:
            // none is ever looked up or unfolded, so none can grow without end.
            Ok(Event::GeneralRef(r)) if skipping == 0 => match r.resolve_char_ref() {
                Ok(Some(c)) => out.push(c),
                _ => {
                    let name = r.into_inner();
                    match quick_xml::escape::resolve_xml_entity(&name) {
                        Some(known) => out.push_str(known),
                        None if &*name == "nbsp" => out.push(' '),
                        // A web page names the characters of HTML, &eacute;, which XML has not.
                        None if opens => {
                            out.extend(book::reference(&format!("&{name};")).map(|(c, _)| c))
                        }
                        None => {}
                    }
                }
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// A folder for a test's files, empty.
    pub fn folder(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("coxswain-extract-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A zip file with these entries, as office files are.
    pub fn zip_file(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A Word, OpenDocument, workbook or presentation file gives `whole_lines` the lines of
    /// `text_of` when it has less text than is kept, and all its text when it has more: the
    /// same start, and the end too.
    #[test]
    fn whole_lines_are_the_text_of_a_document_and_go_past_what_is_kept() {
        let d = folder("whole-lines");
        let docx = |p: &str| format!("<w:document xmlns:w=\"w\"><w:body>{p}</w:body></w:document>");
        let odt = |p: &str| format!("<office:document-content xmlns:office=\"o\" xmlns:text=\"t\"><office:body><office:text>{p}</office:text></office:body></office:document-content>");
        let pptx = |p: &str| format!("<p:sld xmlns:p=\"p\" xmlns:a=\"a\"><p:cSld><p:spTree><p:sp><p:txBody>{p}</p:txBody></p:sp></p:spTree></p:cSld></p:sld>");
        let xlsx = |p: &str| format!("<worksheet><sheetData>{p}</sheetData></worksheet>");
        let rels = r#"<Relationships><Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let book = r#"<workbook><sheets><sheet name="Fuel" r:id="rId1"/></sheets></workbook>"#;
        let write = |name: &str, n: usize| {
            let line = |i: usize| format!("crew {i} checks the  valve and\tthe fuel line, harbour");
            let paras: String = (0..n).map(|i| match name {
                "a.docx" => format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", line(i)),
                "a.odt" => format!("<text:p>{}</text:p>", line(i)),
                "a.pptx" => format!("<a:p><a:r><a:t>{}</a:t></a:r></a:p>", line(i)),
                _ => format!("<row><c t=\"inlineStr\"><is><t>{}</t></is></c><c><v>{i}</v></c></row>", line(i)),
            }).collect();
            let p = d.join(name);
            match name {
                "a.docx" => zip_file(&p, &[("word/document.xml", docx(&paras).as_bytes())]),
                "a.odt" => zip_file(&p, &[("content.xml", odt(&paras).as_bytes())]),
                "a.pptx" => zip_file(&p, &[("ppt/slides/slide1.xml", pptx(&paras).as_bytes())]),
                _ => zip_file(&p, &[("xl/workbook.xml", book.as_bytes()), ("xl/_rels/workbook.xml.rels", rels.as_bytes()), ("xl/worksheets/sheet1.xml", xlsx(&paras).as_bytes())]),
            }
            p
        };
        for name in ["a.docx", "a.odt", "a.pptx", "a.xlsx"] {
            for n in [3usize, 120_000] {
                let p = write(name, n);
                let text = text_of(&p, std::fs::metadata(&p).unwrap().len(), u64::MAX).unwrap();
                let mut lines = vec![];
                whole_lines(&p, &mut |l| lines.push(l.to_string())).unwrap();
                let whole = lines.join("\n");
                if n == 3 {
                    assert_eq!(whole, text, "{name}: less than is kept, the same text");
                } else {
                    assert!(text.len() <= MAX_TEXT && whole.len() > MAX_TEXT + MAX_TEXT / 2, "{name}: {} {}", text.len(), whole.len());
                    assert!(whole.starts_with(&text[..text.len() - 200]), "{name}: the same start");
                    assert!(whole.contains(&format!("crew {} checks", n - 1)), "{name}: to the end");
                }
            }
        }
        assert!(whole_lines(&d.join("a.docx").with_extension("pdf"), &mut |_| {}).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_file_of_zip_endings_is_read_within_its_limits() {
        let d = folder("endings");
        // The record at the end of a zip that says where its directory is, over and over: each
        // says the directory is at the start, where there is none.
        let ending = b"PK\x05\x06\0\0\0\0\x01\0\x01\0\x2e\0\0\0\0\0\0\0\0\0";
        let bytes = ending.repeat(512 * 1024 / ending.len());
        for name in [
            "endings.docx",
            "endings.pptx",
            "endings.epub",
            "endings.xlsx",
        ] {
            std::fs::write(d.join(name), &bytes).unwrap();
            let started = std::time::Instant::now();
            assert_eq!(text_of(&d.join(name), bytes.len() as u64, u64::MAX), None);
            assert!(
                started.elapsed() < crate::test_limit(std::time::Duration::from_secs(5)),
                "{name}: {:?}",
                started.elapsed()
            );
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn extract_tidies_cuts_and_survives() {
        assert_eq!(
            tidy_code("\n\nfn a() {\n\t  x(1,  2);   \n\n\n}\n\n"),
            "fn a() {\n\t  x(1,  2);\n\n}",
            "code keeps its indentation and one blank line"
        );
        assert_eq!(tidy("  one   two \n\n\n three\t\n"), "one two\nthree");
        let long = "é".repeat(MAX_TEXT);
        let cut = tidy(&long);
        assert!(cut.len() <= MAX_TEXT && cut.len() > MAX_TEXT - 4 && cut.chars().all(|c| c == 'é'));
        // The store's marks, a command for a terminal and a bell part words as a space does, and blank space of any kind is a space.
        assert_eq!(
            tidy("be\u{1}fore\u{2} \u{1b}[31mred\u{7}\tand\u{c}green \u{1} end"),
            "be fore [31mred and green end"
        );
        assert_eq!(
            tidy("bud\u{AD}get, bud\u{200B}get, bud\u{FEFF}get and bud\u{2060}get"),
            "budget, budget, budget and budget"
        );
        assert_eq!(tidy_code("\tfn\u{1}a()\u{AD}  "), "\tfn a()");
        // The line that does not fit begins on a line of its own and is cut at a word: no two words are joined at the end.
        let cut = tidy(&format!(
            "{}\nslut\nfærge diesel {}",
            "x".repeat(MAX_TEXT - 20),
            "y".repeat(40)
        ));
        assert!(
            cut.ends_with("x\nslut\nfærge diesel") && cut.len() <= MAX_TEXT,
            "{}",
            &cut[cut.len() - 40..]
        );
        // One text longer than what is kept is cut where it is read, not after it was built.
        let built = xml_text(format!("<a>{long}</a>").as_bytes(), &[], &[]);
        assert!(
            built.len() <= MAX_TEXT
                && built.len() > MAX_TEXT - 4
                && built.chars().all(|c| c == 'é'),
            "{} bytes",
            built.len()
        );

        assert_eq!(xml_text(b"<w:p><w:r><w:t>Fuel &amp; fire</w:t></w:r></w:p><w:p><w:t>Go</w:t><w:br/>now</w:p>", &["p", "br"], &[]), "Fuel & fire\nGo\nnow\n");
        // A web page ends a line where a cell or a break begins, since it need not close them; and it writes the names as it likes.
        assert_eq!(
            xml_text(
                b"<table><tr><td>budget<br>Fuel<td>Bro</table>",
                &["td", "br"],
                &[]
            ),
            "budgetFuelBro"
        );
        assert_eq!(
            page_text(
                b"<TABLE><tr><td>budget<br>Fuel<td>Bro<BR>Tunnel</table>",
                &["td", "br"],
                &[]
            ),
            "\nbudget\nFuel\nBro\nTunnel"
        );
        assert_eq!(
            xml_text(b"<a>keep<STYLE>p { x }</STYLE>this</a>", &[], &["style"]),
            "keepthis"
        );
        // A byte that is no UTF-8 ends the reading, and costs no copy of the document.
        assert_eq!(
            xml_text(b"<a>caf\xc3\xa9 and\xff then</a>", &[], &[]),
            "café and"
        );
        assert_eq!(utf8(b"\xffabc"), "");
        assert_eq!(
            xml_text(
                b"<a>keep<style>p { x }</style><b>this</b></a>",
                &[],
                &["style"]
            ),
            "keepthis"
        );
        assert_eq!(
            xml_text(b"<a>cut <b>short", &[], &[]),
            "cut short",
            "what is left of a broken document"
        );
        assert_eq!(
            xml_text(
                "<a>caf&#233; &#x41;&lt;&nbsp;b&made-up;</a>".as_bytes(),
                &[],
                &[]
            ),
            "café A< b"
        );
        let laughs = b"<!DOCTYPE l [<!ENTITY a \"ha\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]><l>&b;&b;&b;</l>";
        assert_eq!(
            xml_text(laughs, &[], &[]),
            "",
            "entities of the document's own are never unfolded"
        );

        let d = folder("shared");
        // A reader that panics costs that one file its text, nothing more.
        assert_eq!(
            std::panic::catch_unwind(|| -> Option<String> { panic!("a parser gave up") })
                .ok()
                .flatten(),
            None
        );
        std::fs::write(d.join("empty.pdf"), b"").unwrap();
        assert_eq!(text_of(&d.join("empty.pdf"), 0, 100), None);
        std::fs::write(d.join("notes.txt"), "Book   the ferry.\n\n").unwrap();
        assert_eq!(
            text_of(&d.join("notes.txt"), 20, 100).as_deref(),
            Some("Book the ferry.")
        );
        assert_eq!(
            text_of(&d.join("notes.txt"), 20, 10).as_deref(),
            Some("Book the ferry."),
            "plain text of any size: read line by line, it stops at MAX_TEXT"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}

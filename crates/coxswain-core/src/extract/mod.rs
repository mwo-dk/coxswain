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
    if matches!(crate::chess::format(path), Some(crate::chess::Format::Pgn | crate::chess::Format::Pdn | crate::chess::Format::Kif | crate::chess::Format::Csa)) {
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

/// The text of the file at `path`, `size` bytes long, if it has any and is no larger than
/// `max`. Runs of blank space become one space or one line break; code keeps its indentation
/// and a blank line between its parts. None for a file only in the cloud, unless the settings
/// read those: reading it would download it.
pub fn text_of(path: &Path, size: u64, max: u64) -> Option<String> {
    // A PGN file of any size: its reader stops after a line per game for MAX_TEXT bytes.
    let pgn = matches!(crate::chess::format(path), Some(crate::chess::Format::Pgn | crate::chess::Format::Pdn | crate::chess::Format::Kif | crate::chess::Format::Csa));
    // Plain text, code, CSV and logs of any size: read line by line, they stop at MAX_TEXT.
    let streamed = pgn || reader(path).is_none();
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
        None => plain::tidied(path, code)?,
    };
    (!text.trim().is_empty()).then_some(text)
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

/// `tidy` (or `tidy_code`) a line at a time, for text read as it comes: the same text in, the
/// same out, whether whole or line by line.
pub(crate) struct Tidy {
    pub out: String,
    line: String,
    blank: bool,
    code: bool,
}

impl Tidy {
    pub fn new(code: bool) -> Tidy {
        Tidy {
            out: String::new(),
            line: String::new(),
            blank: false,
            code,
        }
    }

    /// Adds a line (without its line break); `false` once the text is full.
    pub fn push(&mut self, l: &str) -> bool {
        let out = &mut self.out;
        if self.code {
            let line = seen(l);
            let line = line.trim_end();
            if line.is_empty() {
                self.blank = !out.is_empty();
                return true;
            }
            let start = if out.is_empty() {
                ""
            } else if self.blank {
                "\n\n"
            } else {
                "\n"
            };
            self.blank = false;
            if out.len() + start.len() + line.len() > MAX_TEXT {
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
        if !out.is_empty() {
            out.push('\n');
        }
        // The line that does not fit is cut at a word, and the text ends there.
        if out.len() + line.len() > MAX_TEXT {
            let room = start(line, MAX_TEXT.saturating_sub(out.len()));
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
fn push_within(out: &mut String, text: &str, most: usize) {
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
    tags_dropped(xml, lines, skip, false)
}

/// The text of a web page, as `xml_text` reads it, but an element in `lines` ends a line
/// where it opens as well: a page closes no `<br>`, and often no `<td>` or `<li>`.
pub fn page_text(xml: &[u8], lines: &[&str], skip: &[&str]) -> String {
    tags_dropped(xml, lines, skip, true)
}

fn tags_dropped(xml: &[u8], lines: &[&str], skip: &[&str], opens: bool) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(utf8(xml));
    reader.config_mut().check_end_names = false;
    let (mut out, mut skipping) = (String::new(), 0usize);
    let named = |name: &str, among: &[&str]| among.iter().any(|one| one.eq_ignore_ascii_case(name));
    while out.len() < MAX_TEXT {
        match reader.read_event() {
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
                out.push_str(start(&t.into_inner(), MAX_TEXT - out.len()))
            }
            Ok(Event::CData(t)) if skipping == 0 => {
                out.push_str(start(&t.into_inner(), MAX_TEXT - out.len()))
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
    out
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

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
//! The one exception is `installed`: programs the user has installed (OCR, LibreOffice), run
//! at low priority with a time limit, for what no reader here can get.
//!
//! `text_of` also catches a panic from a reader (they lean on other people's parsers) and
//! cuts the text at `MAX_TEXT`.

use std::io::Read;
use std::path::Path;

pub mod book;
pub mod installed;
pub mod mail;
pub mod notebook;
pub mod pdf;
pub mod plain;
pub mod rtf;
pub mod sheet;
pub mod slides;
pub mod word;

/// The most text kept of one file. A book is about 1 MB; past this it is a data dump.
pub const MAX_TEXT: usize = 4 * 1024 * 1024;
/// The most one entry of a zip may unpack to. Office files are zips, and a zip can claim a
/// few bytes and unpack to gigabytes.
pub const MAX_ENTRY: u64 = 64 * 1024 * 1024;

type Reader = fn(&Path, u64) -> Option<String>;

/// The reader for a file name, by its extension. `None`: read it as plain text.
fn reader(path: &Path) -> Option<Reader> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => |path, max| pdf::text(path, max).or_else(|| installed::scanned_pdf(path)),
        "docx" | "docm" | "dotx" | "odt" | "ott" => word::text,
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => sheet::text,
        "pptx" | "ppsx" | "potx" | "odp" => slides::text,
        "epub" | "html" | "htm" | "xhtml" => book::text,
        "ipynb" | "drawio" | "dio" => notebook::text,
        "eml" | "mbox" => mail::text,
        "rtf" => rtf::text,
        e if installed::PICTURES.contains(&e) => installed::picture,
        e if installed::OFFICE.contains(&e) => installed::office,
        _ => return None,
    })
}

/// The text of the file at `path`, `size` bytes long, if it has any and is no larger than
/// `max`. Runs of blank space become one space or one line break.
pub fn text_of(path: &Path, size: u64, max: u64) -> Option<String> {
    if size == 0 || size > max {
        return None;
    }
    let text = match reader(path) {
        Some(read) => std::panic::catch_unwind(|| read(path, max)).ok().flatten()?,
        None => plain::text(path, max)?,
    };
    let text = tidy(&text);
    (!text.is_empty()).then_some(text)
}

/// One space between words, one line break between lines, no more than `MAX_TEXT` bytes, and
/// nothing of what `unseen` names.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_TEXT));
    for line in text.lines().map(|l| l.replace(unseen, "").split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()) {
        if !out.is_empty() {
            out.push('\n');
        }
        // The line that does not fit is cut at a word, and the text ends there.
        if out.len() + line.len() > MAX_TEXT {
            let room = start(&line, MAX_TEXT.saturating_sub(out.len()));
            out.push_str(&room[..room.rfind(' ').unwrap_or(room.len())]);
            break;
        }
        out.push_str(&line);
    }
    out
}

/// The bytes as text, as far as they are UTF-8. A part of an office file is UTF-8 through and
/// through; what a hostile file puts after a byte that is not is left out, and nothing is
/// copied: a byte that is no UTF-8 would take three as a replacement character.
pub fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or_else(|e| std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default())
}

/// What stands in a text and is no part of what it says. A control character that is no blank
/// space: the store marks the words it found with two of them, and a terminal takes others for
/// commands. And the soft hyphen, the zero width space and the word joiner, which say where a
/// word may be broken at the end of a line or may not: left in, they end the word for the
/// store, and "bud-get" is not found as "budget".
fn unseen(c: char) -> bool {
    c.is_control() && !c.is_whitespace() || matches!(c, '\u{AD}' | '\u{200B}' | '\u{2060}')
}

/// The start of a text: no more than `most` bytes of it, and no part of a character.
fn start(text: &str, most: usize) -> &str {
    let mut end = most.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// ---------------------------------------------------------------- for the readers

/// One entry of a zip file, unpacked, or `None` when it is missing or larger than
/// `MAX_ENTRY`.
pub fn zip_entry(path: &Path, name: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let entry = zip.by_name(name).ok()?;
    unpack(entry)
}

/// The entries of a zip file whose names `wanted` picks, in the order of their names, each
/// unpacked. Entries larger than `MAX_ENTRY` are left out; at most `most` entries are read.
pub fn zip_entries(path: &Path, most: usize, wanted: impl Fn(&str) -> bool) -> Vec<(String, Vec<u8>)> {
    let Some(mut zip) = std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()) else { return vec![] };
    let mut names: Vec<String> = zip.file_names().filter(|n| wanted(n)).map(String::from).collect();
    names.sort_by(|a, b| natural(a).cmp(&natural(b)));
    names.truncate(most);
    names.into_iter().filter_map(|n| Some((n.clone(), unpack(zip.by_name(&n).ok()?)?))).collect()
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
            Ok(Event::Start(e)) if skipping > 0 || named(e.local_name().as_ref(), skip) => skipping += 1,
            Ok(Event::Start(e)) if opens && named(e.local_name().as_ref(), lines) => out.push('\n'),
            Ok(Event::End(e)) => {
                if skipping > 0 {
                    skipping -= 1;
                } else if named(e.local_name().as_ref(), lines) {
                    out.push('\n');
                }
            }
            Ok(Event::Empty(e)) if skipping == 0 && named(e.local_name().as_ref(), lines) => out.push('\n'),
            Ok(Event::Text(t)) if skipping == 0 => out.push_str(start(&t.into_inner(), MAX_TEXT - out.len())),
            Ok(Event::CData(t)) if skipping == 0 => out.push_str(start(&t.into_inner(), MAX_TEXT - out.len())),
            // &amp; and &#233; arrive on their own. An entity the document made up is dropped:
            // none is ever looked up or unfolded, so none can grow without end.
            Ok(Event::GeneralRef(r)) if skipping == 0 => match r.resolve_char_ref() {
                Ok(Some(c)) => out.push(c),
                _ => out.push_str(match &*r.into_inner() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    "nbsp" => " ",
                    _ => "",
                }),
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
        let d = std::env::temp_dir().join(format!("coxswain-extract-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A zip file with these entries, as office files are.
    pub fn zip_file(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_tidies_cuts_and_survives() {
        assert_eq!(tidy("  one   two \n\n\n three\t\n"), "one two\nthree");
        let long = "é".repeat(MAX_TEXT);
        let cut = tidy(&long);
        assert!(cut.len() <= MAX_TEXT && cut.len() > MAX_TEXT - 4 && cut.chars().all(|c| c == 'é'));
        // The store's marks, a command for a terminal and a bell are taken out, and blank space of any kind is a space.
        assert_eq!(tidy("be\u{1}fore\u{2} \u{1b}[31mred\u{7}\tand\u{c}green \u{1} end"), "before [31mred and green end");
        assert_eq!(tidy("bud\u{AD}get, bud\u{200B}get and bud\u{2060}get"), "budget, budget and budget");
        // The line that does not fit begins on a line of its own and is cut at a word: no two words are joined at the end.
        let cut = tidy(&format!("{}\nslut\nfærge diesel {}", "x".repeat(MAX_TEXT - 20), "y".repeat(40)));
        assert!(cut.ends_with("x\nslut\nfærge diesel") && cut.len() <= MAX_TEXT, "{}", &cut[cut.len() - 40..]);
        // One text longer than what is kept is cut where it is read, not after it was built.
        let built = xml_text(format!("<a>{long}</a>").as_bytes(), &[], &[]);
        assert!(built.len() <= MAX_TEXT && built.len() > MAX_TEXT - 4 && built.chars().all(|c| c == 'é'), "{} bytes", built.len());

        assert_eq!(xml_text(b"<w:p><w:r><w:t>Fuel &amp; fire</w:t></w:r></w:p><w:p><w:t>Go</w:t><w:br/>now</w:p>", &["p", "br"], &[]), "Fuel & fire\nGo\nnow\n");
        // A web page ends a line where a cell or a break begins, since it need not close them; and it writes the names as it likes.
        assert_eq!(xml_text(b"<table><tr><td>budget<br>Fuel<td>Bro</table>", &["td", "br"], &[]), "budgetFuelBro");
        assert_eq!(page_text(b"<TABLE><tr><td>budget<br>Fuel<td>Bro<BR>Tunnel</table>", &["td", "br"], &[]), "\nbudget\nFuel\nBro\nTunnel");
        assert_eq!(xml_text(b"<a>keep<STYLE>p { x }</STYLE>this</a>", &[], &["style"]), "keepthis");
        // A byte that is no UTF-8 ends the reading, and costs no copy of the document.
        assert_eq!(xml_text(b"<a>caf\xc3\xa9 and\xff then</a>", &[], &[]), "café and");
        assert_eq!(utf8(b"\xffabc"), "");
        assert_eq!(xml_text(b"<a>keep<style>p { x }</style><b>this</b></a>", &[], &["style"]), "keepthis");
        assert_eq!(xml_text(b"<a>cut <b>short", &[], &[]), "cut short", "what is left of a broken document");
        assert_eq!(xml_text("<a>caf&#233; &#x41;&lt;&nbsp;b&made-up;</a>".as_bytes(), &[], &[]), "café A< b");
        let laughs = b"<!DOCTYPE l [<!ENTITY a \"ha\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]><l>&b;&b;&b;</l>";
        assert_eq!(xml_text(laughs, &[], &[]), "", "entities of the document's own are never unfolded");

        let d = folder("shared");
        zip_file(&d.join("f.zip"), &[("ppt/slides/slide10.xml", b"ten"), ("ppt/slides/slide9.xml", b"nine"), ("other.xml", b"no")]);
        assert_eq!(zip_entry(&d.join("f.zip"), "other.xml").as_deref(), Some(&b"no"[..]));
        assert_eq!(zip_entry(&d.join("f.zip"), "missing.xml"), None);
        let slides = zip_entries(&d.join("f.zip"), 10, |n| n.starts_with("ppt/slides/"));
        assert_eq!(slides.iter().map(|(_, b)| b.as_slice()).collect::<Vec<_>>(), [&b"nine"[..], &b"ten"[..]]);
        std::fs::write(d.join("broken.zip"), b"PK\x03\x04 not a zip").unwrap();
        assert!(zip_entries(&d.join("broken.zip"), 10, |_| true).is_empty());

        // A reader that panics costs that one file its text, nothing more.
        assert_eq!(std::panic::catch_unwind(|| -> Option<String> { panic!("a parser gave up") }).ok().flatten(), None);
        std::fs::write(d.join("empty.pdf"), b"").unwrap();
        assert_eq!(text_of(&d.join("empty.pdf"), 0, 100), None);
        std::fs::write(d.join("notes.txt"), "Book   the ferry.\n\n").unwrap();
        assert_eq!(text_of(&d.join("notes.txt"), 20, 100).as_deref(), Some("Book the ferry."));
        assert_eq!(text_of(&d.join("notes.txt"), 20, 10), None, "larger than allowed");
        let _ = std::fs::remove_dir_all(d);
    }
}

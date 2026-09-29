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
//! `text_of` also catches a panic from a reader (they lean on other people's parsers) and
//! cuts the text at `MAX_TEXT`.

use std::io::Read;
use std::path::Path;

pub mod book;
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
        "pdf" => pdf::text,
        "docx" | "docm" | "dotx" | "odt" | "ott" => word::text,
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => sheet::text,
        "pptx" | "ppsx" | "potx" | "odp" => slides::text,
        "epub" | "html" | "htm" | "xhtml" => book::text,
        "ipynb" | "drawio" | "dio" => notebook::text,
        "eml" | "mbox" => mail::text,
        "rtf" => rtf::text,
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

/// One space between words, one line break between lines, no more than `MAX_TEXT` bytes.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_TEXT));
    for line in text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()) {
        if out.len() + line.len() + 1 > MAX_TEXT {
            let mut room = MAX_TEXT.saturating_sub(out.len() + 1).min(line.len());
            while !line.is_char_boundary(room) {
                room -= 1;
            }
            if room > 0 {
                out.push_str(&line[..room]);
            }
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }
    out
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
/// name (without its prefix) is in `lines` ends a line; one in `skip` is left out with all
/// inside it. What is left of a broken document is still returned.
pub fn xml_text(xml: &[u8], lines: &[&str], skip: &[&str]) -> String {
    use quick_xml::events::Event;
    let xml = String::from_utf8_lossy(xml);
    let mut reader = quick_xml::Reader::from_str(&xml);
    reader.config_mut().check_end_names = false;
    let (mut out, mut skipping) = (String::new(), 0usize);
    let named = |name: &str, among: &[&str]| among.contains(&name);
    while out.len() <= MAX_TEXT {
        match reader.read_event() {
            Ok(Event::Start(e)) if skipping > 0 || named(e.local_name().as_ref(), skip) => skipping += 1,
            Ok(Event::End(e)) => {
                if skipping > 0 {
                    skipping -= 1;
                } else if named(e.local_name().as_ref(), lines) {
                    out.push('\n');
                }
            }
            Ok(Event::Empty(e)) if skipping == 0 && named(e.local_name().as_ref(), lines) => out.push('\n'),
            Ok(Event::Text(t)) if skipping == 0 => out.push_str(&t.into_inner()),
            Ok(Event::CData(t)) if skipping == 0 => out.push_str(&t.into_inner()),
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

        assert_eq!(xml_text(b"<w:p><w:r><w:t>Fuel &amp; fire</w:t></w:r></w:p><w:p><w:t>Go</w:t><w:br/>now</w:p>", &["p", "br"], &[]), "Fuel & fire\nGo\nnow\n");
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

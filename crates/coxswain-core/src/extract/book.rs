//! Books and web pages: EPUB, and HTML as it is found on a disk.
//!
//! A page is read by a small scanner of its own and not as XML. Real pages leave attributes
//! unquoted and `<p>` and `<li>` open, name their letters by entities that XML does not know,
//! and are not always UTF-8. The scanner goes through the page once, from its start to its
//! end, and keeps what is between the tags: it builds no tree, so nothing in it can nest.
//!
//! The chapters of a book are read in the same way. They are XHTML, which has the `&eacute;`
//! that `xml_text` drops, and many were made by a converter that left a `&` or a `<` bare,
//! where `xml_text` stops.
//!
//! An EPUB is a zip: `META-INF/container.xml` names the package file, whose spine lists the
//! chapters in reading order and whose manifest says which file each of them is.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use encoding_rs::{Encoding, ISO_2022_JP, UTF_8, WINDOWS_1252};

use super::{MAX_ENTRY, MAX_TEXT};

/// Elements that are part of a line: one may begin or end inside a word, as the runs of a
/// word processor do, so nothing is put between them. Every other element, known or not,
/// ends the line, for two words that became one are both lost to a search.
const INLINE: [&str; 35] = [
    "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "cite", "code", "data", "del", "dfn", "em", "font", "i", "ins", "kbd", "mark", "nobr", "q", "rb", "ruby",
    "s", "samp", "small", "span", "strike", "strong", "sub", "sup", "time", "tt", "u", "var", "wbr",
];
/// Elements whose inside is no text of the page.
const LEFT_OUT: [&str; 3] = ["script", "style", "template"];
/// Elements whose inside is text as it is written, tags and all.
const WRITTEN: [&str; 2] = ["textarea", "title"];
/// The most files of a book's manifest and names in its spine that are kept. A book has
/// hundreds of chapters at the most; past this it is a data dump, or made to be trouble.
const MOST: usize = 5000;

/// The encoding a page declares, in a `<meta>` tag or as XML does, among its first bytes.
static DECLARED: LazyLock<regex::bytes::Regex> =
    LazyLock::new(|| regex::bytes::Regex::new(r#"(?i-u)(?:<meta[^>]*?charset|<\?xml[^>]*?encoding)\s*=\s*["']?\s*([a-z0-9_:.-]+)"#).expect("valid"));
/// A comment or a script, where a declaration is no declaration.
static HIDDEN: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"(?is-u)<!--.*?-->|<script.*?</script").expect("valid"));

/// The text of a book or a page: a book when the name of the file ends in `.epub`.
pub fn text(path: &Path, max: u64) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > max {
        return None;
    }
    let mut out = String::new();
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub")) {
        book(path, &mut out)?;
    } else {
        let mut bytes = vec![];
        std::fs::File::open(path).ok()?.take(max).read_to_end(&mut bytes).ok()?;
        page(&decoded(&bytes)?, &mut out, true);
    }
    // A soft hyphen says where a word may be broken at the end of a line, and a byte order
    // mark inside a text was pasted there. A search takes either for the end of the word.
    out.retain(|c| !matches!(c, '\u{ad}' | '\u{feff}'));
    (!out.trim().is_empty()).then_some(out)
}

// ---------------------------------------------------------------- a book

/// The text of an EPUB, onto `out`: its title and authors, then its chapters in reading order.
fn book(path: &Path, out: &mut String) -> Option<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    // The files of the zip by their names as a link would give them, for a package that
    // writes `Text\Ch1.xhtml` or `text/ch1.xhtml` for `Text/Ch1.xhtml`.
    let names: HashMap<String, String> = zip.file_names().map(|name| (key(name), name.to_string())).collect();
    // A file of the zip, no more than `most` bytes of it: what it says it holds is looked at
    // before it is unpacked, and one byte more than `most` says that it lied.
    let mut file = |name: &str, most: usize| {
        let entry = zip.by_name(names.get(&key(name))?).ok()?;
        (entry.size() <= most as u64).then(|| super::unpack(entry.take(most as u64 + 1)))?
    };
    let container = file("META-INF/container.xml", 1 << 20)?;
    let package = Pieces::of(&decoded(&container)?).find_map(|piece| match piece {
        Piece::Open(name, inside, _) if local(name) == "rootfile" => attribute(inside, "full-path"),
        _ => None,
    })?;
    let package = named(&[], &package);
    let folder: Vec<&str> = package.rsplit_once('/').map_or(vec![], |(folder, _)| folder.split('/').collect());
    let chapters = contents(&decoded(&file(&package, 8 << 20)?)?, out);
    // All the chapters together may unpack to what one entry may. Every name in the spine
    // costs 4096 bytes at the least, found or not, and a file is read once however many
    // names lead to it: so a book that names one large chapter a thousand times is not read
    // a thousand times, nor is an empty one opened a million times.
    let (mut room, mut read) = (MAX_ENTRY as usize, HashSet::new());
    for chapter in chapters {
        let Some(left) = room.checked_sub(4096).filter(|_| out.len() <= MAX_TEXT) else { break };
        room = left;
        let name = named(&folder, &chapter);
        if !read.insert(key(&name)) {
            continue;
        }
        let Some(bytes) = file(&name, room) else { continue };
        let Some(left) = room.checked_sub(bytes.len()) else { break };
        room = left;
        if let Some(chapter) = decoded(&bytes) {
            page(&chapter, out, false);
            out.push('\n');
        }
    }
    Some(())
}

/// What the package file of a book says: its title and its authors, put onto `out` as the
/// first lines, and the files of its chapters in reading order, as the package names them,
/// each once.
fn contents(package: &str, out: &mut String) -> Vec<String> {
    let (mut titles, mut authors, mut line) = (String::new(), String::new(), None);
    let (mut files, mut spine): (HashMap<String, String>, Vec<String>) = (HashMap::new(), vec![]);
    for piece in Pieces::of(package) {
        match piece {
            Piece::Open(name, inside, empty) => match local(name) {
                "item" if files.len() < MOST => {
                    // A page of a book may also be a picture, which has no text.
                    let kind = attribute(inside, "media-type").unwrap_or_default();
                    let text = kind.is_empty() || kind.contains("html") || kind.contains("xml");
                    if let (true, Some(id), Some(file)) = (text, attribute(inside, "id"), attribute(inside, "href")) {
                        files.insert(id, file);
                    }
                }
                "itemref" if spine.len() < MOST => spine.extend(attribute(inside, "idref")),
                "title" if !empty => line = Some(&mut titles),
                "creator" if !empty => line = Some(&mut authors),
                _ => line = None,
            },
            Piece::Text(text) => line.iter_mut().for_each(|line| unescape(text, line)),
            Piece::Literal(text) => line.iter_mut().for_each(|line| line.push_str(cut(text, (MAX_TEXT + 1).saturating_sub(line.len())))),
            Piece::Close(_) => line.take().into_iter().for_each(|line| line.push('\n')),
        }
    }
    for text in [titles, authors] {
        out.push_str(cut(&text, (MAX_TEXT + 1).saturating_sub(out.len())));
    }
    spine.iter().filter_map(|id| files.remove(id)).collect()
}

/// The name in the zip of a file that the package names: from the folder of the package,
/// with `%20` for a space, and perhaps with the `#place` in the file at its end.
fn named(folder: &[&str], link: &str) -> String {
    let link = link.split('#').next().unwrap_or_default();
    let mut parts: Vec<&str> = if link.starts_with('/') { vec![] } else { folder.to_vec() };
    for part in link.split('/') {
        match part {
            "" | "." => {}
            ".." => drop(parts.pop()),
            part => parts.push(part),
        }
    }
    let name = parts.join("/");
    let (bytes, mut plain, mut at) = (name.as_bytes(), vec![], 0);
    let digit = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
    while at < bytes.len() {
        match (bytes[at], digit(at + 1), digit(at + 2)) {
            (b'%', Some(high), Some(low)) => {
                plain.push((high * 16 + low) as u8);
                at += 3;
            }
            (byte, ..) => {
                plain.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&plain).into_owned()
}

/// A name as it is looked up: in one case, with `/` for the `\` that Windows writes.
fn key(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

// ---------------------------------------------------------------- a page

/// The bytes of a page as text: in the encoding of its byte order mark; as UTF-8 when they are
/// that, or nearly, whatever the page says; else in the encoding the page declares; else in
/// Windows-1252, as a browser does. `None` for bytes that are no text.
fn decoded(bytes: &[u8]) -> Option<Cow<'_, str>> {
    // UTF-32 begins with the mark of UTF-16 and would be read as it, a zero between letters.
    if bytes.starts_with(b"\xff\xfe\0\0") || bytes.starts_with(b"\0\0\xfe\xff") {
        return None;
    }
    if let Some((encoding, mark)) = Encoding::for_bom(bytes) {
        let rest = &bytes[mark..];
        return Some(if encoding == UTF_8 { String::from_utf8_lossy(rest) } else { encoding.decode_without_bom_handling(rest).0 });
    }
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    let declared = DECLARED.captures(&HIDDEN.replace_all(head, &b""[..])).and_then(|found| Encoding::for_label(&found[1]));
    // ISO-2022-JP writes Japanese in the bytes of ASCII, so it is read as it says before
    // the bytes are tried as UTF-8, which they always are.
    if declared == Some(ISO_2022_JP) {
        return Some(ISO_2022_JP.decode_without_bom_handling(bytes).0);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.into());
    }
    // UTF-8 with a byte or two broken, as where a file was cut off or a letter was pasted in
    // from elsewhere, is still UTF-8, whatever the page declares. In any other encoding
    // nearly every letter beyond ASCII is broken UTF-8. A sample says which, for the whole
    // may be 64 MB and broken bytes are slow to look at.
    let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 16)]);
    let broken = sample.matches(char::REPLACEMENT_CHARACTER).count();
    let whole = sample.chars().filter(|c| !c.is_ascii()).count() - broken;
    if whole > broken {
        return Some(String::from_utf8_lossy(bytes));
    }
    // A page that says UTF-16 in letters of one byte each is not UTF-16, and a label that
    // encoding_rs answers with its replacement encoding would be one U+FFFD for the whole
    // page: neither is compatible with ASCII, and the page is read as a browser reads it.
    Some(declared.filter(|e| e.is_ascii_compatible()).unwrap_or(WINDOWS_1252).decode_without_bom_handling(bytes).0)
}

/// The text of a page, onto `out`: what is between its tags, with a line for each block. The
/// `<title>` of the page is text when `titled`; a chapter's is the book's, once per chapter.
fn page(page: &str, out: &mut String, titled: bool) {
    let mut pieces = Pieces::of(page);
    while let Some(piece) = pieces.next() {
        if out.len() > MAX_TEXT {
            break;
        }
        match piece {
            // Blank space around a text lays the page out and says nothing: one space of it
            // is kept, so that the room for text is filled with words and not with it.
            Piece::Text(text) => {
                let inner = text.trim();
                if text.starts_with(char::is_whitespace) && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
                unescape(inner, out);
                if text.ends_with(char::is_whitespace) && !inner.is_empty() && out.len() <= MAX_TEXT {
                    out.push(' ');
                }
            }
            Piece::Literal(text) => out.push_str(cut(text, MAX_TEXT + 1 - out.len())),
            Piece::Open(name, _, false) if among(local(name), &LEFT_OUT) => {
                pieces.leave_out(name);
            }
            Piece::Open(name, _, false) if among(local(name), &WRITTEN) => {
                let written = pieces.leave_out(name);
                if titled || !local(name).eq_ignore_ascii_case("title") {
                    unescape(written, out);
                }
            }
            // A picture stands between words, and what it says stands where it does, as words
            // of their own. Only a letter alone is the first letter of a chapter, drawn large,
            // and the word goes on after it.
            Piece::Open(name, inside, _) if local(name).eq_ignore_ascii_case("img") => {
                let says = attributes(inside, 0, "alt").2.unwrap_or_default();
                let between = if says.chars().count() == 1 { "" } else { " " };
                *out += between;
                unescape(says, out);
                *out += between;
            }
            // The mark of a footnote is written against the word before it and, where the
            // note is, against its first word. It is a word of its own.
            Piece::Open(name, _, false) if among(local(name), &["a", "sup"]) => {
                if let Some(mark) = pieces.mark(name) {
                    if out.ends_with(char::is_alphanumeric) {
                        out.push(' ');
                    }
                    unescape(mark, out);
                    out.push(' ');
                }
            }
            Piece::Open(name, ..) | Piece::Close(name) => match local(name) {
                name if among(name, &INLINE) => {}
                name if among(name, &["td", "th"]) => out.push('\t'),
                _ if out.ends_with('\n') => {}
                _ => out.push('\n'),
            },
        }
    }
}

/// A piece of a page.
enum Piece<'a> {
    /// Text as it is written, with its `&amp;`.
    Text(&'a str),
    /// Text that means what it says, from a `<![CDATA[` section.
    Literal(&'a str),
    /// An opening tag: its name, its attributes as they are written, and whether it is also
    /// its own end, as `<br/>` is.
    Open(&'a str, &'a str, bool),
    /// A closing tag: its name.
    Close(&'a str),
}

/// A page, piece by piece. Every piece begins where the one before it ended and is at least
/// one byte long, so the pieces end when the page does.
struct Pieces<'a> {
    page: &'a str,
    at: usize,
}

impl<'a> Pieces<'a> {
    fn of(page: &'a str) -> Pieces<'a> {
        Pieces { page, at: 0 }
    }

    /// Leaves out what follows, up to the closing tag of `name`, and gives it as it is
    /// written. A browser leaves out the rest of the page when a script is never closed, and
    /// so is it here.
    fn leave_out(&mut self, name: &str) -> &'a str {
        let (rest, name) = (&self.page.as_bytes()[self.at..], name.as_bytes());
        // `</script>` closes a script; `</scripted>` does not.
        let closes = |at: usize| {
            rest[at..].starts_with(b"</")
                && rest[at + 2..].get(..name.len()).is_some_and(|written| written.eq_ignore_ascii_case(name))
                && rest.get(at + 2 + name.len()).is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>'))
        };
        let end = memchr::memchr_iter(b'<', rest).find(|&at| closes(at)).unwrap_or(rest.len());
        let written = &self.page[self.at..self.at + end];
        self.at += end;
        written
    }

    /// The text inside the element `name` that begins here, when it is the mark of a footnote:
    /// a number, a small roman numeral (of i, v and x, for "mix" is a word) or something in
    /// brackets, with nothing but elements of a line around it. The element is then gone
    /// through; else nothing is.
    fn mark(&mut self, name: &str) -> Option<&'a str> {
        let (mut peek, mut text) = (Pieces { page: self.page, at: self.at }, None);
        for _ in 0..8 {
            match peek.next()? {
                Piece::Text(written) if text.is_none() => text = Some(written.trim()),
                Piece::Close(inner) if inner.eq_ignore_ascii_case(name) => {
                    let text = text?;
                    let (number, roman) = (text.bytes().all(|b| b.is_ascii_digit()), text.bytes().all(|b| b"ivx".contains(&b)));
                    let mark = (1..=8).contains(&text.len()) && (number || roman || text.starts_with('[') && text.ends_with(']'));
                    if mark {
                        self.at = peek.at;
                    }
                    return mark.then_some(text);
                }
                Piece::Open(inner, ..) | Piece::Close(inner) if among(local(inner), &INLINE) => {}
                _ => return None,
            }
        }
        None
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        loop {
            let rest = &self.page[self.at..];
            let bytes = rest.as_bytes();
            if bytes.first() != Some(&b'<') {
                let end = memchr::memchr(b'<', bytes).unwrap_or(bytes.len());
                self.at += end;
                return (end > 0).then(|| Piece::Text(&rest[..end]));
            }
            let letter = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_alphabetic);
            // Where what began at the `<` is over: past the first `end`, or where the page
            // ends when there is none.
            let past = |from: usize, end: &str| rest[from..].find(end).map_or(rest.len(), |at| from + at + end.len());
            if rest.starts_with("<!--") {
                // A comment ends at `-->`, or at `--!>`, which a browser takes for the same.
                let end = past(2, "-->");
                self.at += rest[2..end].find("--!>").map_or(end, |at| 2 + at + 4);
            } else if let Some(text) = rest.strip_prefix("<![CDATA[") {
                self.at += past(9, "]]>");
                return Some(Piece::Literal(text.split("]]>").next().unwrap_or_default()));
            } else if letter(1) || (bytes.get(1) == Some(&b'/') && letter(2)) {
                let closing = bytes[1] == b'/';
                let name = 1 + usize::from(closing);
                let named = bytes[name..].iter().position(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/')).map_or(bytes.len(), |at| name + at);
                let (end, empty, _) = attributes(rest, named, "");
                self.at += end;
                let name = &rest[name..named];
                return Some(if closing { Piece::Close(name) } else { Piece::Open(name, &rest[named..end], empty) });
            } else if bytes.get(1).is_some_and(|b| matches!(b, b'!' | b'?' | b'/')) {
                // What kind of document this is, an instruction to a program, a tag with no name.
                let mut end = past(1, ">");
                // The kind of document, first in the page, may declare entities of its own
                // between `[` and `]`, with a `>` in each.
                if bytes[1] == b'!' && self.page[..self.at].trim().is_empty()
                    && let Some(open) = rest[..end].find('[')
                {
                    end = past(past(open, "]"), ">");
                }
                self.at += end;
            } else {
                self.at += 1;
                return Some(Piece::Text("<"));
            }
        }
    }
}

/// Goes through the attributes of a tag, which begin at `at`, to the `>` that ends the tag:
/// where the tag is over, whether it is also its own end, and the value of the attribute
/// `wanted` as it is written, if the tag has it. A `>` inside quotes ends no tag, a quote
/// begins a value only after a `=`, and a `/` at the end of an unquoted value is the value's.
fn attributes<'a>(tag: &'a str, mut at: usize, wanted: &str) -> (usize, bool, Option<&'a str>) {
    let (bytes, mut found, mut empty) = (tag.as_bytes(), None, false);
    let blank = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_whitespace);
    while at < bytes.len() && bytes[at] != b'>' {
        if blank(at) || bytes[at] == b'/' {
            empty = bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'>');
            at += 1;
            continue;
        }
        let name = at;
        at += 1;
        while at < bytes.len() && !blank(at) && !matches!(bytes[at], b'=' | b'>' | b'/') {
            at += 1;
        }
        let name = &tag[name..at];
        while blank(at) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while blank(at) {
            at += 1;
        }
        let value = at;
        let value = match bytes.get(at) {
            Some(quote @ (b'"' | b'\'')) => {
                let end = memchr::memchr(*quote, &bytes[at + 1..]).map_or(bytes.len(), |end| at + 1 + end);
                at = (end + 1).min(bytes.len());
                &tag[value + 1..end]
            }
            _ => {
                while at < bytes.len() && !blank(at) && bytes[at] != b'>' {
                    at += 1;
                }
                &tag[value..at]
            }
        };
        if found.is_none() && name.eq_ignore_ascii_case(wanted) {
            found = Some(value);
        }
    }
    ((at + 1).min(bytes.len()), empty, found)
}

/// The value of an attribute of a tag, from the tag's inside.
fn attribute(inside: &str, name: &str) -> Option<String> {
    let mut value = String::new();
    unescape(attributes(inside, 0, name).2?, &mut value);
    Some(value)
}

/// A name without its prefix: `title` for `dc:title`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn among(name: &str, names: &[&str]) -> bool {
    names.iter().any(|one| one.eq_ignore_ascii_case(name))
}

/// The start of `text`, no more than `most` bytes of it.
fn cut(text: &str, most: usize) -> &str {
    let mut end = most.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Text as it is written in a page, onto `out` as it is meant: `é` for `&eacute;`. No more of
/// it than fills `out` to `MAX_TEXT`; what is meant is never longer than what is written.
fn unescape(text: &str, out: &mut String) {
    let mut rest = cut(text, (MAX_TEXT + 1).saturating_sub(out.len()));
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let (character, length) = reference(&rest[at..]).unwrap_or(('&', 1));
        out.push(character);
        rest = &rest[at + length..];
    }
    out.push_str(rest);
}

/// The character that the start of `text` stands for, and how many bytes of `text` say so:
/// `&#233;`, `&#xe9;` or `&eacute;`. `None` when the `&` is only a `&`, as in AT&T.
fn reference(text: &str) -> Option<(char, usize)> {
    let body = &text[1..];
    let closed = |length: usize| length + usize::from(text[length..].starts_with(';'));
    if let Some(number) = body.strip_prefix('#') {
        let (digits, radix) = number.strip_prefix(['x', 'X']).map_or((number, 10), |digits| (digits, 16));
        // Eight digits after any zeros in front: a number of more digits is no character.
        let zeros = digits.bytes().take_while(|b| *b == b'0').count();
        let length = zeros + digits[zeros..].bytes().take(8).take_while(|b| (*b as char).is_digit(radix)).count();
        let character = match u32::from_str_radix(&digits[..length], radix).ok()? {
            // Pages written on Windows number the dashes and quotes of Windows-1252.
            code @ 0x80..=0x9f => WINDOWS_1252.decode_without_bom_handling(&[code as u8]).0.chars().next()?,
            code => char::from_u32(code).filter(|c| *c != '\0').unwrap_or(char::REPLACEMENT_CHARACTER),
        };
        return Some((character, closed(text.len() - digits.len() + length)));
    }
    let length = body.bytes().take(32).take_while(u8::is_ascii_alphanumeric).count();
    let character = ENTITIES[ENTITIES.binary_search_by_key(&&body[..length], |(name, _)| name).ok()?].1;
    // Only the oldest names, those of Latin-1, also hold without their `;`.
    (text[1 + length..].starts_with(';') || (character as u32) < 256).then_some((character, closed(1 + length)))
}

/// The characters that have a name in HTML 4, in the order of their names. HTML 5 added two
/// thousand more, nearly all of them signs of mathematics, which stay as they are written.
#[rustfmt::skip]
const ENTITIES: [(&str, char); 253] = [
    ("AElig", 'Æ'), ("Aacute", 'Á'), ("Acirc", 'Â'), ("Agrave", 'À'), ("Alpha", 'Α'), ("Aring", 'Å'), ("Atilde", 'Ã'), ("Auml", 'Ä'), ("Beta", 'Β'),
    ("Ccedil", 'Ç'), ("Chi", 'Χ'), ("Dagger", '‡'), ("Delta", 'Δ'), ("ETH", 'Ð'), ("Eacute", 'É'), ("Ecirc", 'Ê'), ("Egrave", 'È'), ("Epsilon", 'Ε'),
    ("Eta", 'Η'), ("Euml", 'Ë'), ("Gamma", 'Γ'), ("Iacute", 'Í'), ("Icirc", 'Î'), ("Igrave", 'Ì'), ("Iota", 'Ι'), ("Iuml", 'Ï'), ("Kappa", 'Κ'),
    ("Lambda", 'Λ'), ("Mu", 'Μ'), ("Ntilde", 'Ñ'), ("Nu", 'Ν'), ("OElig", 'Œ'), ("Oacute", 'Ó'), ("Ocirc", 'Ô'), ("Ograve", 'Ò'), ("Omega", 'Ω'),
    ("Omicron", 'Ο'), ("Oslash", 'Ø'), ("Otilde", 'Õ'), ("Ouml", 'Ö'), ("Phi", 'Φ'), ("Pi", 'Π'), ("Prime", '″'), ("Psi", 'Ψ'), ("Rho", 'Ρ'),
    ("Scaron", 'Š'), ("Sigma", 'Σ'), ("THORN", 'Þ'), ("Tau", 'Τ'), ("Theta", 'Θ'), ("Uacute", 'Ú'), ("Ucirc", 'Û'), ("Ugrave", 'Ù'), ("Upsilon", 'Υ'),
    ("Uuml", 'Ü'), ("Xi", 'Ξ'), ("Yacute", 'Ý'), ("Yuml", 'Ÿ'), ("Zeta", 'Ζ'), ("aacute", 'á'), ("acirc", 'â'), ("acute", '´'), ("aelig", 'æ'),
    ("agrave", 'à'), ("alefsym", 'ℵ'), ("alpha", 'α'), ("amp", '&'), ("and", '∧'), ("ang", '∠'), ("apos", '\''), ("aring", 'å'), ("asymp", '≈'),
    ("atilde", 'ã'), ("auml", 'ä'), ("bdquo", '„'), ("beta", 'β'), ("brvbar", '¦'), ("bull", '•'), ("cap", '∩'), ("ccedil", 'ç'), ("cedil", '¸'),
    ("cent", '¢'), ("chi", 'χ'), ("circ", 'ˆ'), ("clubs", '♣'), ("cong", '≅'), ("copy", '©'), ("crarr", '↵'), ("cup", '∪'), ("curren", '¤'), ("dArr", '⇓'),
    ("dagger", '†'), ("darr", '↓'), ("deg", '°'), ("delta", 'δ'), ("diams", '♦'), ("divide", '÷'), ("eacute", 'é'), ("ecirc", 'ê'), ("egrave", 'è'),
    ("empty", '∅'), ("emsp", '\u{2003}'), ("ensp", '\u{2002}'), ("epsilon", 'ε'), ("equiv", '≡'), ("eta", 'η'), ("eth", 'ð'), ("euml", 'ë'), ("euro", '€'),
    ("exist", '∃'), ("fnof", 'ƒ'), ("forall", '∀'), ("frac12", '½'), ("frac14", '¼'), ("frac34", '¾'), ("frasl", '⁄'), ("gamma", 'γ'), ("ge", '≥'),
    ("gt", '>'), ("hArr", '⇔'), ("harr", '↔'), ("hearts", '♥'), ("hellip", '…'), ("iacute", 'í'), ("icirc", 'î'), ("iexcl", '¡'), ("igrave", 'ì'),
    ("image", 'ℑ'), ("infin", '∞'), ("int", '∫'), ("iota", 'ι'), ("iquest", '¿'), ("isin", '∈'), ("iuml", 'ï'), ("kappa", 'κ'), ("lArr", '⇐'),
    ("lambda", 'λ'), ("lang", '〈'), ("laquo", '«'), ("larr", '←'), ("lceil", '⌈'), ("ldquo", '“'), ("le", '≤'), ("lfloor", '⌊'), ("lowast", '∗'),
    ("loz", '◊'), ("lrm", '\u{200e}'), ("lsaquo", '‹'), ("lsquo", '‘'), ("lt", '<'), ("macr", '¯'), ("mdash", '—'), ("micro", 'µ'), ("middot", '·'),
    ("minus", '−'), ("mu", 'μ'), ("nabla", '∇'), ("nbsp", '\u{a0}'), ("ndash", '–'), ("ne", '≠'), ("ni", '∋'), ("not", '¬'), ("notin", '∉'), ("nsub", '⊄'),
    ("ntilde", 'ñ'), ("nu", 'ν'), ("oacute", 'ó'), ("ocirc", 'ô'), ("oelig", 'œ'), ("ograve", 'ò'), ("oline", '‾'), ("omega", 'ω'), ("omicron", 'ο'),
    ("oplus", '⊕'), ("or", '∨'), ("ordf", 'ª'), ("ordm", 'º'), ("oslash", 'ø'), ("otilde", 'õ'), ("otimes", '⊗'), ("ouml", 'ö'), ("para", '¶'),
    ("part", '∂'), ("permil", '‰'), ("perp", '⊥'), ("phi", 'φ'), ("pi", 'π'), ("piv", 'ϖ'), ("plusmn", '±'), ("pound", '£'), ("prime", '′'), ("prod", '∏'),
    ("prop", '∝'), ("psi", 'ψ'), ("quot", '"'), ("rArr", '⇒'), ("radic", '√'), ("rang", '〉'), ("raquo", '»'), ("rarr", '→'), ("rceil", '⌉'), ("rdquo", '”'),
    ("real", 'ℜ'), ("reg", '®'), ("rfloor", '⌋'), ("rho", 'ρ'), ("rlm", '\u{200f}'), ("rsaquo", '›'), ("rsquo", '’'), ("sbquo", '‚'), ("scaron", 'š'),
    ("sdot", '⋅'), ("sect", '§'), ("shy", '\u{ad}'), ("sigma", 'σ'), ("sigmaf", 'ς'), ("sim", '∼'), ("spades", '♠'), ("sub", '⊂'), ("sube", '⊆'),
    ("sum", '∑'), ("sup", '⊃'), ("sup1", '¹'), ("sup2", '²'), ("sup3", '³'), ("supe", '⊇'), ("szlig", 'ß'), ("tau", 'τ'), ("there4", '∴'), ("theta", 'θ'),
    ("thetasym", 'ϑ'), ("thinsp", '\u{2009}'), ("thorn", 'þ'), ("tilde", '˜'), ("times", '×'), ("trade", '™'), ("uArr", '⇑'), ("uacute", 'ú'),
    ("uarr", '↑'), ("ucirc", 'û'), ("ugrave", 'ù'), ("uml", '¨'), ("upsih", 'ϒ'), ("upsilon", 'υ'), ("uuml", 'ü'), ("weierp", '℘'), ("xi", 'ξ'),
    ("yacute", 'ý'), ("yen", '¥'), ("yuml", 'ÿ'), ("zeta", 'ζ'), ("zwj", '\u{200d}'), ("zwnj", '\u{200c}'),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::{folder, zip_file};
    use crate::extract::text_of;

    /// What the store gets of a file with this name and these bytes.
    fn read(folder: &Path, name: &str, bytes: &[u8]) -> Option<String> {
        std::fs::write(folder.join(name), bytes).unwrap();
        text_of(&folder.join(name), bytes.len() as u64, 1 << 20)
    }

    /// Numbers without order, the same ones in every run.
    fn numbers(mut state: u64) -> impl FnMut() -> usize {
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 16) as usize
        }
    }

    const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
        <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    const PACKAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
        <package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
        <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
            <dc:identifier id="id">urn:uuid:1</dc:identifier>
            <dc:creator>Søren Ågård</dc:creator>
            <dc:title>Rejsen til fyret &amp; hjem</dc:title>
            <dc:creator id="nobody"/>
            <dc:language>da</dc:language>
        </metadata>
        <manifest>
            <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
            <item id="a" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
            <item id="b" href="text/../text/b.xhtml#top" media-type="application/xhtml+xml"/>
            <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
            <item id="gone" href="text/gone.xhtml" media-type="application/xhtml+xml"/>
        </manifest>
        <spine><itemref idref="cover"/><itemref idref="b"/><itemref idref="gone"/><itemref idref="a"/><itemref idref="b"/></spine>
        </package>"#;
    const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Rejsen til fyret</title>
        <link rel="stylesheet" href="s.css"/><script src="a.js"/></head><body><h1>Fyrtårnet</h1><p>Caf&eacute;en l&aring; ved havet.</p></body></html>"#;

    #[test]
    fn page_keeps_a_word_whole_and_ends_a_line_at_a_block() {
        let d = folder("page-lines");
        let page = "<!DOCTYPE html><html><head><title>Flight seven</title></head>\n<body><h1>Fuel</h1><p>Un<b>believ</b>able <i>costs</i>\n\
                    <p>Second<br>line<ul><li>one<li>two</ul><table><tr><td>Hour</td><th>Price</th></tr><tr><td>1</td><td>20</td></tr></table>\
                    <DIV>Upper</DIV>lower<my-box>own</my-box>H<sub>2</sub>O</body></html>";
        let text = "Flight seven\nFuel\nUnbelievable costs\nSecond\nline\none\ntwo\nHour Price\n1 20\nUpper\nlower\nown\nH2O";
        assert_eq!(read(&d, "a.html", page.as_bytes()).as_deref(), Some(text));
        assert_eq!(read(&d, "b.XHTML", page.as_bytes()).as_deref(), Some(text));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_keeps_a_word_whole_across_runs_and_parts_words_at_a_picture() {
        let d = folder("page-neighbours");
        // The runs of a word processor, as LibreOffice and OnlyOffice write them: a word in two.
        let page = "<p><i>zero-term</i><font face=\"x\"><i>inated</i></font> <span style=a>librarie</span><span style=a>s</span> \
                    <a href=a>Home</a><img src=bar.gif><a href=b>Products</a><img src=bar.gif alt=\"\"><a href=c>Contact</a> the<img src=x>end</p>\
                    <p><span class=large>O</span><span class=small>nce</span> upon <img src=w.png alt=W>ITH no \
                    <img alt='A lighthouse &amp; a rock' src=l.jpg>end<img src=none.png></p>";
        let text = "zero-terminated libraries Home Products Contact the end\nOnce upon WITH no A lighthouse & a rock end";
        assert_eq!(read(&d, "a.htm", page.as_bytes()).as_deref(), Some(text));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_sets_the_mark_of_a_footnote_apart_from_the_words_around_it() {
        let d = folder("page-footnotes");
        // As LibreOffice writes footnotes and endnotes, and Wikipedia its references.
        let page = "<p>The cargo<a class=\"sdfootnoteanc\" href=\"#n1\"><sup>1</sup></a> was heavy<sup><a href=\"#c2\">[2]</a></sup>, \
                    said <a href=x>mix</a>.</p>\
                    <div><p><a class=\"sdfootnotesym\" name=\"n1\">1</a>Footnoteword about the cargo.</p><p><a name=\"e1\">i</a>Endnoteword.</p></div>";
        let text = "The cargo 1 was heavy [2] , said mix.\n1 Footnoteword about the cargo.\ni Endnoteword.";
        assert_eq!(read(&d, "a.html", page.as_bytes()).as_deref(), Some(text));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_leaves_out_scripts_styles_and_comments() {
        let d = folder("page-left-out");
        let page = "<?xml version=\"1.0\"?><!DOCTYPE html><p>one<!-- two --> three<script>if (a < b) { document.write(\"<p>four</p>\") }</SCRIPT> five\
                    <style>p > a { }</style><template><p>six</p></template><a href=\"x>y\" title='a > b'>seven</a> <script src=\"a.js\"/>eight \
                    <![CDATA[nine & <ten>]]> 3 < 4 <!-->eleven";
        assert_eq!(read(&d, "a.html", page.as_bytes()).as_deref(), Some("one three\nfive\nseven\neight nine & <ten> 3 < 4 eleven"));
        assert_eq!(read(&d, "open.html", b"<p>kept</p><script>let lost = 1;").as_deref(), Some("kept"), "a script that is never closed");
        assert_eq!(read(&d, "only.html", b"<html><head><style>p { }</style></head><body><!-- nothing --></body></html>"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_reads_what_a_browser_shows_where_a_scanner_would_stop_early_or_late() {
        let d = folder("page-browser");
        let read = |name: &str, bytes: &[u8]| read(&d, name, bytes);
        assert_eq!(read("title.html", b"<title>The <script> element</title><p>All the text</p>").as_deref(), Some("The <script> element\nAll the text"));
        let area = b"<p>before</p><textarea>write <style> here</textarea><p>after</p>";
        assert_eq!(read("area.html", area).as_deref(), Some("before\nwrite <style> here\nafter"));
        assert_eq!(read("bang.html", b"<p>one</p><!-- a --!><p>two</p><!-- b --><p>three</p>").as_deref(), Some("one\ntwo\nthree"));
        assert_eq!(read("zeros.html", b"<p>A&#0000000065;B &#x000000041;</p>").as_deref(), Some("AAB A"));
        assert_eq!(read("prefix.html", b"<script>var a = \"</scripted>\"; var secretcode = 1;</script><p>kept</p>").as_deref(), Some("kept"));
        assert_eq!(read("slash.html", b"<script src=https://example.org/lib/>var leaked = 1;</script><p>kept</p>").as_deref(), Some("kept"));
        assert_eq!(read("subset.html", b"<!DOCTYPE html [<!ENTITY auth \"Jane\">]><p>&auth; wrote</p>").as_deref(), Some("&auth; wrote"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_resolves_the_names_and_numbers_of_characters() {
        let d = folder("page-characters");
        let page = "<p>bl&aring;b&aelig;rgr&oslash;d p&aring; caf&eacute;&nbsp;Sm&#248;r &#xe9;n &#X41; &lt;b&gt; &amp; AT&T &copy 2024 &#150; &unknown; \
                    fish&chips &#0; &#x110000; &Eacute;cole &euro;5 &hellip; exam&shy;ple un\u{ad}til bom&#xFEFF;inside &#</p>";
        let text = "blåbærgrød på café Smør én A <b> & AT&T © 2024 – &unknown; fish&chips \u{fffd} \u{fffd} École €5 … example until bominside &#";
        assert_eq!(read(&d, "a.html", page.as_bytes()).as_deref(), Some(text));
        assert!(ENTITIES.windows(2).all(|pair| pair[0].0 < pair[1].0), "a name is found by halving, so the names are in order");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_is_read_in_its_own_encoding() {
        let d = folder("page-encodings");
        let read = |name: &str, bytes: &[u8]| read(&d, name, bytes);
        let declared = b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"><p>bl\xe5b\xe6rgr\xf8d \x96 caf\xe9</p>";
        assert_eq!(read("declared.html", declared).as_deref(), Some("blåbærgrød – café"));
        assert_eq!(read("hebrew.html", b"<META CHARSET=windows-1255><p>\xf9\xec\xe5\xed</p>").as_deref(), Some("שלום"));
        let as_xml = b"<?xml version=\"1.0\" encoding=\"GBK\"?><html><body><p>\xc4\xe3\xba\xc3</p></body></html>";
        assert_eq!(read("chinese.xhtml", as_xml).as_deref(), Some("你好"));
        assert_eq!(read("silent.html", b"<p>r\xf8dgr\xf8d med fl\xf8de</p>").as_deref(), Some("rødgrød med fløde"), "nothing declared: as a browser reads it");
        assert_eq!(read("wrong.html", "<meta charset=\"iso-8859-1\"><p>Århus שלום</p>".as_bytes()).as_deref(), Some("Århus שלום"), "UTF-8 whatever it says");
        assert_eq!(read("cut.html", &"<p>blåbærgrød og rødgrød".as_bytes()[..27]).as_deref(), Some("blåbærgrød og rødgr\u{fffd}"), "cut inside a letter");
        let stale = ["<meta charset=\"iso-8859-1\">".as_bytes(), "<p>Blåbærgrød på caféen i Århus</p>".repeat(20).as_bytes(), b"<p>na\xefve</p>"].concat();
        assert!(read("stale.html", &stale).is_some_and(|text| text.contains("Blåbærgrød") && text.ends_with("na\u{fffd}ve")), "UTF-8 with one byte pasted in");
        assert_eq!(read("false.html", b"<meta charset=utf-16><p>caf\xe9</p>").as_deref(), Some("café"), "UTF-16 has no letters of one byte");
        let japanese = b"<meta charset=\"iso-2022-jp\"><p>\x1b$B$3$l$OF|K\\8l$N%Z!<%8$G$9\x1b(B</p>";
        assert_eq!(read("japanese.html", japanese).as_deref(), Some("これは日本語のページです"), "seven bits, so valid UTF-8 as well");
        assert_eq!(read("hz.html", b"<meta charset=\"hz-gb-2312\"><p>hello world caf\xe9 more words</p>").as_deref(), Some("hello world café more words"));
        let hidden = b"<!-- <meta charset=\"koi8-r\"> was wrong --><script>s = '<meta charset=\"koi8-r\">'</script>\
                       <meta charset=\"windows-1251\"><p>\xcf\xf0\xe8\xe2\xe5\xf2</p>";
        assert_eq!(read("hidden.html", hidden).as_deref(), Some("Привет"), "a declaration in a comment or a script is none");
        for bytes in [u16::to_le_bytes, u16::to_be_bytes] {
            let wide: Vec<u8> = "\u{feff}<p>Blåbær 你好</p>".encode_utf16().flat_map(bytes).collect();
            assert_eq!(read("wide.html", &wide).as_deref(), Some("Blåbær 你好"));
        }
        let wider: Vec<u8> = "\u{feff}<p>Hello wide world</p>".chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
        assert_eq!(read("wider.html", &wider), None, "UTF-32 is not read as UTF-16");
        assert_eq!(read("picture.html", b"<p>GIF89a\x00\x01\x02</p>"), None, "a zero byte is in no page");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn book_gives_title_and_author_then_the_chapters_in_reading_order() {
        let d = folder("book");
        let entries: [(&str, &[u8]); 7] = [
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", PACKAGE.as_bytes()),
            ("OEBPS/nav.xhtml", b"<html><body><nav>Not a chapter</nav></body></html>"),
            ("OEBPS/cover.jpg", b"\xff\xd8\xff\xe0 no text"),
            ("OEBPS/text/chapter 1.xhtml", "<html><body><p>שלום og 你好</p></body></html>".as_bytes()),
            ("OEBPS/text/b.xhtml", CHAPTER.as_bytes()),
        ];
        zip_file(&d.join("a.epub"), &entries);
        let text = text_of(&d.join("a.epub"), 1, 1 << 20);
        // The title of a chapter's page is the book's, and a chapter the spine names twice is read once.
        assert_eq!(text.as_deref(), Some("Rejsen til fyret & hjem\nSøren Ågård\nFyrtårnet\nCaféen lå ved havet.\nשלום og 你好"));

        // As a program that does not know XML writes it: nothing quoted, nothing closed.
        let entries: [(&str, &[u8]); 3] = [
            ("META-INF/container.xml", b"<container><rootfiles><rootfile full-path=package.opf>"),
            ("package.opf", b"<package><metadata><dc:title>Kort</dc:title><manifest><item href=one.html id=one><spine><itemref idref=one>"),
            ("one.html", b"<p>Eneste kapitel"),
        ];
        zip_file(&d.join("b.EPUB"), &entries);
        assert_eq!(text_of(&d.join("b.EPUB"), 1, 1 << 20).as_deref(), Some("Kort\nEneste kapitel"));

        // Names as they are written by hand: a slash in front, a space encoded, another case,
        // a backslash, and a chapter that is XML of no named kind.
        let entries: [(&str, &[u8]); 3] = [
            ("META-INF/container.xml", b"<rootfile full-path=\"/Book%20One/content.opf\"/>"),
            ("Book One/content.opf", b"<dc:title>Case</dc:title><item id=a href=\"Text\\Ch1.xhtml\" media-type=\"application/xml\"/><itemref idref=\"a\"/>"),
            ("book one/text/ch1.xhtml", b"<p>Found anyway</p>"),
        ];
        zip_file(&d.join("c.epub"), &entries);
        assert_eq!(text_of(&d.join("c.epub"), 1, 1 << 20).as_deref(), Some("Case\nFound anyway"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn book_reads_a_file_once_however_many_names_of_the_spine_lead_to_it() {
        let d = folder("book-spine");
        // One long name of a missing file, twenty thousand times in the spine: it was copied
        // once per name and looked for once per name, and a longer one filled the memory.
        let long = "x".repeat(100_000) + ".html";
        let package = format!(
            "<dc:title>Book</dc:title><item id='c' href='c.html'/><item id='d' href='./c.html'/><item id='m' href='{long}'/>\
             <spine><itemref idref='c'/><itemref idref='d'/>{}</spine>",
            "<itemref idref='m'/><itemref idref='c'/>".repeat(20_000)
        );
        let entries = [("META-INF/container.xml", "<rootfile full-path='p.opf'/>"), ("p.opf", &package), ("c.html", "<p>Oncewords</p>")];
        zip_file(&d.join("a.epub"), &entries.map(|(name, text)| (name, text.as_bytes())));
        let start = std::time::Instant::now();
        assert_eq!(text_of(&d.join("a.epub"), 1, 1 << 24).as_deref(), Some("Book\nOncewords"));
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn book_and_page_hold_for_cut_off_random_and_empty_files() {
        let d = folder("book-broken");
        let mut next = numbers(0x2545f4914f6cdd1d);
        // `text` itself is asked, for `text_of` would catch a panic and hide it.
        for name in ["empty.html", "empty.xhtml", "empty.epub"] {
            std::fs::write(d.join(name), b"").unwrap();
            assert_eq!(text(&d.join(name), 100), None);
        }
        assert_eq!((text(&d.join("missing.html"), 100), text(&d.join("missing.epub"), 100)), (None, None));
        std::fs::write(d.join("large.html"), b"<p>Too much</p>").unwrap();
        assert_eq!(text(&d.join("large.html"), 5), None, "larger than allowed");

        for _ in 0..200 {
            let bytes: Vec<u8> = (0..next() % 3000).map(|_| next() as u8).collect();
            for name in ["random.html", "random.epub"] {
                std::fs::write(d.join(name), &bytes).unwrap();
                let _ = text(&d.join(name), 1 << 20);
            }
            let entries: [(&str, &[u8]); 3] = [("META-INF/container.xml", b"<rootfile full-path='p.opf'/>"), ("p.opf", &bytes), ("c.html", &bytes)];
            zip_file(&d.join("inside.epub"), &entries);
            let _ = text(&d.join("inside.epub"), 1 << 20);
        }

        // Every piece of what pages are made of, in no order.
        let bits = ["<", ">", "/", "!", "--", "[", "]", "?", "&", "#", ";", "x", "\"", "'", "=", " ", "\n", "a", "p", "é", "中", "9", "%", ":", ".."];
        let words =
            ["script", "style", "template", "img alt", "![CDATA[", "td", "amp", "eacute", "item", "itemref", "href", "id", "idref", "title", "sup", "textarea"];
        for _ in 0..20_000 {
            let soup: String = (0..next() % 40).map(|_| if next() % 3 == 1 { words[next() % words.len()] } else { bits[next() % bits.len()] }).collect();
            let mut out = String::new();
            page(&soup, &mut out, true);
            assert!(out.len() <= soup.len(), "what is meant is never longer than what is written: {soup:?} gave {out:?}");
            let (chapters, name) = (contents(&soup, &mut out), named(&[&soup], &soup));
            assert!(chapters.len() <= soup.len() && name.len() <= 2 * soup.len() + 1);
        }

        let whole = "<html><head><meta charset=\"utf-8\"><title>Fyret</title><style>p { }</style></head><body><p class=\"første\">Blåbær &amp; rødgrød \
                     på <a href='x'>caféen</a> שלום 你好</p><script>let a = \"</p>\";</script><!-- note --><p>Slut</p></body></html>";
        for cut in 0..=whole.len() {
            let mut out = String::new();
            page(&decoded(&whole.as_bytes()[..cut]).unwrap(), &mut out, true);
            assert!(cut < whole.len() || super::super::tidy(&out) == "Fyret\nBlåbær & rødgrød på caféen שלום 你好\nSlut");
        }

        let entries = [("META-INF/container.xml", CONTAINER), ("OEBPS/content.opf", PACKAGE), ("OEBPS/text/b.xhtml", CHAPTER)];
        zip_file(&d.join("whole.epub"), &entries.map(|(name, text)| (name, text.as_bytes())));
        let whole = std::fs::read(d.join("whole.epub")).unwrap();
        assert!(text(&d.join("whole.epub"), 1 << 20).is_some_and(|text| text.contains("Fyrtårnet")));
        // A zip lists its entries at its end, so a book that was cut off has no text or little.
        for cut in (0..whole.len()).step_by(3) {
            std::fs::write(d.join("cut.epub"), &whole[..cut]).unwrap();
            assert!(text(&d.join("cut.epub"), 1 << 20).is_none_or(|text| text.len() < 200));
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn book_and_page_keep_no_more_text_than_allowed() {
        let d = folder("book-large");
        let mut out = String::new();
        page(&"lighthouse ".repeat(MAX_TEXT / 8), &mut out, true);
        assert!(out.len() > MAX_TEXT / 2 && out.len() <= MAX_TEXT + 1, "one text without an end");
        let chapter = "<p>Fyret st&aring;r p&aring; klippen h&oslash;jt over havet.</p>\n".repeat(MAX_TEXT / 128);
        let mut out = String::new();
        page(&chapter.repeat(5), &mut out, true);
        assert!(out.len() > MAX_TEXT / 2 && out.len() <= MAX_TEXT + 1, "many small texts");

        let package = "<package><manifest><item id='c' href='c.html'/></manifest><spine>".to_string() + &"<itemref idref='c'/>".repeat(20_000);
        let entries = [("META-INF/container.xml", "<rootfile full-path='p.opf'/>"), ("p.opf", &package), ("c.html", &chapter)];
        zip_file(&d.join("again.epub"), &entries.map(|(name, text)| (name, text.as_bytes())));
        std::fs::write(d.join("c.html"), &chapter).unwrap();
        assert_eq!(text_of(&d.join("again.epub"), 1, 1 << 20), text_of(&d.join("c.html"), 1, 1 << 24), "one chapter many times is one chapter");

        // A title of many literal sections, and an author as long: together no more than the limit.
        let section = format!("<![CDATA[{}]]>", "t".repeat(MAX_TEXT / 4)).repeat(3);
        let package = format!("<dc:title>{section}</dc:title><dc:creator>{section}</dc:creator>");
        let entries = [("META-INF/container.xml", "<rootfile full-path='p.opf'/>"), ("p.opf", &package)];
        zip_file(&d.join("titled.epub"), &entries.map(|(name, text)| (name, text.as_bytes())));
        assert!(text(&d.join("titled.epub"), 1 << 24).is_some_and(|text| text.len() > MAX_TEXT / 2 && text.len() <= MAX_TEXT + 2), "a long title");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn page_counts_its_words_and_not_its_layout_toward_the_limit() {
        // A table written one cell to a line, each indented: more blank space than the limit, fewer words.
        let mut page_text = String::from("<table>\n");
        for row in 0..40_000 {
            page_text += &format!("    <tr>\n{0}<td>row{row}cell1</td>\n{0}<td>row{row}cell2</td>\n{0}<td>row{row}cell3</td>\n    </tr>\n", " ".repeat(60));
        }
        page_text += "</table><p>lastrowword</p>";
        assert!(page_text.len() > MAX_TEXT * 2);
        let mut out = String::new();
        page(&page_text, &mut out, true);
        assert!(out.len() <= MAX_TEXT + 1 && out.contains("row39999cell3") && out.ends_with("lastrowword\n"), "{} bytes", out.len());
    }
}

//! Rich Text Format: groups in braces, control words after a backslash, and the text between
//! them. Word, WordPad, TextEdit and LibreOffice write it.
//!
//! Read by hand, in one pass and with a stack of its own. The `rtf-parser` crate was tried on
//! the same files and left: it gives no text at all for a file that is cut off, joins one
//! paragraph to the next, takes the bytes of a picture for text, and panics on a character
//! written in two halves. Only the code pages come from a crate, `encoding_rs`.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use encoding_rs::Encoding;

use super::MAX_TEXT;

/// A mail as Outlook writes it opens a group for every span of its HTML and closes them at the
/// end, some thousands deep; every other writer stays below a hundred. A group that is open
/// costs a few bytes and nothing else, so many are allowed. A file that goes deeper is read up
/// to there.
const MAX_DEPTH: usize = 100_000;
/// Word names about seventy fonts. The code pages of more than these are not kept.
const MAX_FONTS: usize = 4096;
/// A document has a header and a footer for each of its sections. More texts of them than
/// these are not remembered, and the ones that follow are given as often as they come.
const MAX_HEADERS: usize = 1024;
/// A header, a footer or the instruction of a field is a few lines at most. A longer one is not
/// looked into: the header is given as often as it comes, the instruction shows nothing. Groups
/// of a hostile file that stand inside each other would else have their text read once for each.
const MAX_LOOKED: usize = 4096;
/// As many bytes of text are taken at a time. Where a byte is a character no more of them wait
/// to be decoded, because a letter of Thai is three bytes of text and is to be counted as three.
const MAX_RUN: usize = 64 * 1024;

/// Groups without text of the document: the tables of colours, styles and lists, what is known
/// about the file, pictures, embedded objects, and the copy of a shape for a reader that knows
/// none (Word draws the shape a second time there, its text included). The table of fonts is
/// read for its code pages. The properties of a shape and the instruction of a field are left
/// out too, but for the text they show, and are seen to where their words are read.
const LEFT_OUT: &[&[u8]] = &[b"colortbl", b"stylesheet", b"info", b"pict", b"object", b"listtable", b"listoverridetable", b"shprslt"];

/// A group that begins with `\*` is one a reader may pass over, and all of them are left out
/// but these: LibreOffice writes notes and comments that way, it and Word keep the text of a
/// text box inside the description of its shape (Word 95 inside a drawing object) and the runs
/// of a formula behind a star, a field may show a part of its instruction, and a report
/// generator was seen to put the cells of a nested row among that row's properties.
const KEPT: &[&[u8]] = &[b"footnote", b"annotation", b"shpinst", b"do", b"fldinst", b"moMath", b"moMathPara", b"nesttableprops"];

pub fn text(path: &Path, max: u64) -> Option<String> {
    // A pipe is no file of bytes, and opening one waits for a writer that may never come.
    if !path.is_file() {
        return None;
    }
    let mut rtf = vec![];
    std::fs::File::open(path).ok()?.take(max).read_to_end(&mut rtf).ok()?;
    let text = read(&rtf)?;
    (!text.trim().is_empty()).then_some(text)
}

/// What a group holds: text of the document, the table of fonts, or nothing that is kept.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Fonts,
    LeftOut,
}

/// What is done where a group closes. Nothing but its braces parts the text of such a group
/// from the text around it, and two words must not become one.
#[derive(Clone, Copy)]
enum End {
    Nothing,
    /// A space after the mark of a list item or a run of a formula, a line break after a note,
    /// a comment or a text box.
    Write(char),
    /// A header or a footer, whose text began here. A document has one for the first, the left
    /// and the right pages, and again for every section: the same text is given once.
    Header(usize),
    /// The instruction of a field, whose text began here. What the field shows of it is kept.
    Instruction(usize),
}

/// What a group hands down to the groups inside it, and gets back when they close.
#[derive(Clone, Copy)]
struct Group {
    kind: Kind,
    /// The font in use: its character set is the code page of the bytes of text.
    font: Option<i32>,
    /// How many bytes after `\uN` repeat that character for a reader without Unicode.
    repeat: usize,
    /// This one is the group's own, and is not handed down.
    end: End,
}

/// The text so far. Bytes wait until the next thing that is not a byte, because a character
/// of Japanese, Chinese or Korean is two of them.
struct Text {
    out: String,
    bytes: Vec<u8>,
    page: &'static Encoding,
    /// The first half of a character outside the first 65 536, which `\uN` writes in two.
    high: Option<u16>,
}

impl Text {
    fn len(&self) -> usize {
        self.out.len() + self.bytes.len()
    }

    fn bytes(&mut self, bytes: &[u8], page: &'static Encoding) {
        if page != self.page {
            self.decode();
            self.page = page;
        }
        self.bytes.extend_from_slice(bytes);
        self.high = None;
        if self.bytes.len() >= MAX_RUN && page.is_single_byte() {
            self.decode();
        }
    }

    /// How long the text is, the bytes that waited included.
    fn end(&mut self) -> usize {
        self.decode();
        self.out.len()
    }

    fn decode(&mut self) {
        if !self.bytes.is_empty() {
            self.out.push_str(&self.page.decode_without_bom_handling(&self.bytes).0);
            self.bytes.clear();
        }
    }

    fn char(&mut self, c: char) {
        self.decode();
        self.out.push(c);
        self.high = None;
    }

    /// One unit of UTF-16. A first half waits for its second, and no longer than the next
    /// thing written: halves that are not side by side are not one character.
    fn unit(&mut self, unit: u16) {
        self.decode();
        match (self.high.take(), unit) {
            (Some(high), 0xDC00..=0xDFFF) => self.out.extend(char::decode_utf16([high, unit]).flatten()),
            (_, 0xD800..=0xDBFF) => self.high = Some(unit),
            _ => self.out.extend(char::from_u32(unit.into())),
        }
    }
}

/// The text of a document, or of as much of it as is there. `None` when it is not one.
fn read(rtf: &[u8]) -> Option<String> {
    if !rtf.starts_with(b"{\\rtf") {
        return None;
    }
    let mut text = Text { out: String::new(), bytes: vec![], page: encoding_rs::WINDOWS_1252, high: None };
    let mut group = Group { kind: Kind::Text, font: None, repeat: 1, end: End::Nothing };
    let mut outer: Vec<Group> = vec![];
    // The code page of the document, its default font, and the code page of each font.
    let (mut ansi, mut default_font, mut fonts) = (encoding_rs::WINDOWS_1252, None, HashMap::<i32, &'static Encoding>::new());
    // The headers and footers given so far, and whether the property of a shape that was named
    // last is the text written in the shape.
    let (mut headers, mut written) = (HashSet::<String>::new(), false);
    // The font the table of fonts is describing, whether nothing but blank space has been read
    // of the group, whether it began with `\*`, and how many bytes that repeat a `\uN` are
    // still to be passed over. A control word is not counted as one of those: a writer that
    // leaves them out would lose the next `\uN`.
    let (mut described, mut first, mut starred, mut skip) = (0, false, false, 0usize);
    let mut at = 0;

    while at < rtf.len() && text.len() <= MAX_TEXT {
        let byte = rtf[at];
        at += 1;
        match byte {
            b'{' if outer.len() == MAX_DEPTH => break,
            b'{' => {
                outer.push(group);
                group.end = End::Nothing;
                (first, starred, skip) = (true, false, 0);
            }
            b'}' => {
                match group.end {
                    End::Nothing => {}
                    End::Write(c) => text.char(c),
                    End::Header(from) => {
                        text.decode();
                        let said = Some(&text.out[from..]).filter(|said| said.len() <= MAX_LOOKED).map(|said| said.trim());
                        if said.is_some_and(|said| headers.contains(said)) {
                            text.out.truncate(from);
                        } else if let Some(said) = said.filter(|_| headers.len() < MAX_HEADERS) {
                            headers.insert(said.to_string());
                        }
                        text.char('\n');
                    }
                    End::Instruction(from) => {
                        text.decode();
                        let shown = shown(&text.out[from..]).to_string();
                        text.out.replace_range(from.., &shown);
                    }
                }
                // The last group to close is the document itself: what follows it is not.
                match outer.pop() {
                    Some(back) if !outer.is_empty() => group = back,
                    _ => break,
                }
                (first, starred, skip) = (false, false, 0);
            }
            b'\\' => {
                let Some(&next) = rtf.get(at) else { break };
                let begins = std::mem::take(&mut first);
                if !next.is_ascii_alphabetic() {
                    // A symbol: one character after the backslash, and two digits more after a quote.
                    at += 1;
                    let digits = if next == b'\'' { rtf[at..].iter().take(2).take_while(|b| b.is_ascii_hexdigit()).count() } else { 0 };
                    let hex = std::str::from_utf8(&rtf[at..at + digits]).ok().filter(|h| h.len() == 2).and_then(|h| u8::from_str_radix(h, 16).ok());
                    at += digits;
                    if next == b'*' {
                        // The mark of a group to pass over, where it begins the group. StarOffice writes
                        // `\*\cs12` in the middle of a run, for that one word to be passed over.
                        starred = begins;
                    } else if skip > 0 {
                        skip -= 1;
                    } else if group.kind == Kind::Text {
                        let page = page_of(group.font.or(default_font), &fonts, ansi);
                        match next {
                            b'\'' => text.bytes(hex.as_slice(), page),
                            // A brace or a backslash of the text, or the second byte of a character.
                            b'\\' | b'{' | b'}' => text.bytes(&[next], page),
                            b'~' => text.char('\u{a0}'),
                            b'_' => text.char('-'),
                            // TextEdit ends a paragraph with a backslash at the end of the line.
                            b'\r' | b'\n' => text.char('\n'),
                            // `\-` is a place where a word may be broken, and is no part of it.
                            _ => {}
                        }
                    }
                    continue;
                }

                // A word: letters, then a number if it has one, then one space that ends it.
                let letters = rtf[at..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
                let word = &rtf[at..at + letters];
                at += letters;
                let signed = usize::from(rtf.get(at) == Some(&b'-') && rtf.get(at + 1).is_some_and(u8::is_ascii_digit));
                let digits = signed + rtf[at + signed..].iter().take_while(|b| b.is_ascii_digit()).count();
                let number: Option<i32> = std::str::from_utf8(&rtf[at..at + digits]).ok().and_then(|n| n.parse().ok());
                at += digits;
                at += usize::from(rtf.get(at) == Some(&b' '));

                if std::mem::take(&mut starred) && !KEPT.contains(&word) {
                    group.kind = Kind::LeftOut;
                }
                if word == b"bin" {
                    // Bytes as they are, a picture mostly: braces in them mean nothing.
                    at = at.saturating_add(number.map_or(0, |n| n.max(0) as usize)).min(rtf.len());
                } else if group.kind == Kind::Fonts {
                    let page = match word {
                        b"fcharset" => number.and_then(charset_page),
                        b"cpg" => number,
                        _ => None,
                    };
                    if word == b"f" {
                        described = number.unwrap_or(0);
                    } else if let Some(page) = page.and_then(code_page).filter(|_| fonts.len() < MAX_FONTS) {
                        fonts.insert(described, page);
                    }
                } else if group.kind == Kind::Text {
                    match word {
                        b"fonttbl" => group.kind = Kind::Fonts,
                        w if LEFT_OUT.contains(&w) => group.kind = Kind::LeftOut,
                        // The properties of a part of a formula, `\mfPr` and the like. Some have words for values.
                        w if w.starts_with(b"m") && w.ends_with(b"Pr") => group.kind = Kind::LeftOut,
                        // A property of a shape is its name and its value. LibreOffice writes the text
                        // typed into a drawn shape as one, and Word the text of WordArt.
                        b"sn" => (group.kind, written) = (Kind::LeftOut, rtf[at..].starts_with(b"gtextUNICODE}")),
                        b"sv" if !written => group.kind = Kind::LeftOut,
                        b"fldinst" => group.end = End::Instruction(text.end()),
                        b"header" | b"headerl" | b"headerr" | b"headerf" | b"footer" | b"footerl" | b"footerr" | b"footerf" => {
                            text.char('\n');
                            group.end = End::Header(text.end());
                        }
                        b"ansicpg" => ansi = number.and_then(code_page).unwrap_or(ansi),
                        b"mac" => ansi = encoding_rs::MACINTOSH,
                        b"deff" => default_font = number,
                        b"f" => group.font = number,
                        b"plain" => group.font = None,
                        b"uc" => group.repeat = number.map_or(1, |n| n.max(0) as usize),
                        b"u" => {
                            // Past 32 767 the number is written as a negative one.
                            if let Some(unit) = number {
                                text.unit(unit as u16);
                                skip = group.repeat;
                            }
                        }
                        b"par" | b"line" | b"row" | b"nestrow" | b"page" | b"sect" | b"column" | b"softline" | b"softpage" | b"softcol" => text.char('\n'),
                        // A note, a comment or a text box stands in the middle of a line, and
                        // nothing but its group parts it from the words before and after.
                        b"footnote" | b"annotation" | b"shptxt" | b"dptxbxtext" | b"sv" => {
                            text.char('\n');
                            group.end = End::Write('\n');
                        }
                        // The mark of a list item and a run of a formula stand in a line, as a word of it.
                        b"listtext" | b"pntext" | b"mr" => {
                            text.char(' ');
                            group.end = End::Write(' ');
                        }
                        b"tab" | b"cell" | b"nestcell" => text.char('\t'),
                        b"emspace" | b"enspace" | b"qmspace" => text.char(' '),
                        b"emdash" => text.char('—'),
                        b"endash" => text.char('–'),
                        b"bullet" => text.char('•'),
                        b"lquote" => text.char('‘'),
                        b"rquote" => text.char('’'),
                        b"ldblquote" => text.char('“'),
                        b"rdblquote" => text.char('”'),
                        _ => {}
                    }
                }
            }
            // The line breaks of the file are not those of the document.
            b'\r' | b'\n' | 0 => {}
            _ => {
                // Text as it is, up to the next thing that is not, and no more than there is room for.
                let most = (MAX_TEXT - text.len()).min(rtf.len() - at).min(MAX_RUN);
                let run = at - 1..at + rtf[at..at + most].iter().take_while(|b| !matches!(b, b'{' | b'}' | b'\\' | b'\r' | b'\n' | 0)).count();
                at = run.end;
                first = first && rtf[run.clone()].iter().all(u8::is_ascii_whitespace);
                starred = false;
                let skipped = skip.min(run.len());
                skip -= skipped;
                if group.kind == Kind::Text && skipped < run.len() {
                    text.bytes(&rtf[run.start + skipped..run.end], page_of(group.font.or(default_font), &fonts, ansi));
                }
            }
        }
    }
    // No more text than is kept. A document that ends inside the instruction of a field gives none of that.
    let open = outer.iter().chain([&group]).find_map(|g| if let End::Instruction(from) = g.end { Some(from) } else { None });
    let mut most = text.end().min(MAX_TEXT).min(open.unwrap_or(MAX_TEXT));
    while !text.out.is_char_boundary(most) {
        most -= 1;
    }
    // A zero is no character of any text, and would end it for a reader that takes it as the end.
    // A soft hyphen is a place where a word may be broken and no part of it, but parts it for
    // the index of the store. A character of the private area, which a symbol of Wingdings or
    // Symbol is, that index takes for a letter of the word beside it.
    let kept = text.out[..most].chars().filter(|c| !matches!(c, '\0' | '\u{ad}'));
    Some(kept.map(|c| if matches!(c, '\u{e000}'..='\u{f8ff}') { ' ' } else { c }).collect())
}

/// What a field shows of its instruction, for the kinds that show text and have no result: the
/// word under a phonetic guide, `EQ \o(\s\up 11(reading),word)`, and the text of a place holder,
/// `MACROBUTTON macro text`.
fn shown(instruction: &str) -> &str {
    let looked = if instruction.len() <= MAX_LOOKED { instruction.trim() } else { "" };
    let (kind, rest) = looked.split_once(char::is_whitespace).unwrap_or_default();
    match kind {
        "EQ" if rest.contains("\\o") => rest.rsplit_once(',').map_or("", |(_, word)| word.strip_suffix(')').unwrap_or(word)),
        "MACROBUTTON" => rest.trim_start().split_once(char::is_whitespace).map_or("", |(_, text)| text),
        _ => "",
    }
}

/// The code page of the bytes of text: that of the font in use, or else that of the document.
fn page_of(font: Option<i32>, fonts: &HashMap<i32, &'static Encoding>, ansi: &'static Encoding) -> &'static Encoding {
    font.and_then(|f| fonts.get(&f).copied()).unwrap_or(ansi)
}

/// The code page of a font's character set, as the specification of the format lists them.
/// The ones that say nothing (default, symbol) and the ones of DOS are left to the document's.
fn charset_page(charset: i32) -> Option<i32> {
    Some(match charset {
        0 => 1252,
        77 => 10000,
        78 | 128 => 932,
        79 | 129 => 949,
        80 | 134 => 936,
        81 | 136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        _ => return None,
    })
}

fn code_page(number: i32) -> Option<&'static Encoding> {
    use encoding_rs::*;
    Some(match number {
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1252 => WINDOWS_1252,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        10000 => MACINTOSH,
        65001 => UTF_8,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::folder;
    use crate::extract::{text_of, tidy};

    /// A document of these tests as it is in a file. They write `^u` where a document has a
    /// backslash and a `u`: followed by four digits, that is an escape to many of the tools that
    /// pass source text along, and they would leave one character in its place.
    fn document(written: &str) -> Vec<u8> {
        written.replace("^u", "\\u").into_bytes()
    }

    /// What the store gets for a document: what the reader gives, tidied.
    fn of(rtf: &str) -> String {
        tidy(&read(&document(rtf)).unwrap_or_default())
    }

    /// Whether the index of the store, made as the store makes it, finds each of the words in the text.
    fn found<const N: usize>(text: &str, words: [&str; N]) -> [bool; N] {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE VIRTUAL TABLE text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');").unwrap();
        db.execute("INSERT INTO text(body) VALUES (?1)", [text]).unwrap();
        words.map(|w| db.query_row("SELECT count(*) FROM text WHERE text MATCH ?1", [format!("\"{w}\"")], |r| r.get::<_, i64>(0)).unwrap() == 1)
    }

    /// A document as LibreOffice writes it, cut down from one it wrote.
    const REPORT: &str = r"{\rtf1\ansi\deff3\adeflang1025
{\fonttbl{\f0\froman\fprq2\fcharset0 Times New Roman;}{\f3\froman\fprq2\fcharset0 Liberation Serif{\*\falt Times New Roman};}}
{\colortbl;\red0\green0\blue0;\red0\green0\blue255;}
{\stylesheet{\s0\snext0\nowidctlpar\loch\f3\fs24\lang45065 Normal;}
{\s1\sbasedon24\snext21\rtlch\afs48\ab\ltrch\fs48\b heading 1;}
}{\*\listtable{\list\listtemplateid1
{\listlevel\levelnfc255\leveljc0\levelstartat1\levelfollow2{\leveltext \'01\'06;}{\levelnumbers\'01;}\fi0\li0}\listid3}
}{\listoverridetable{\listoverride\listid1\listoverridecount0\ls1}}{\*\generator LibreOffice/26.8.0.3$Linux_X86_64}{\info{\upr{\title Hidden title}
{\*\ud{\title Hidden title}}}{\keywords hiddenkeyword}{\creatim\yr2026\mo9\dy29\hr18\min48}}{\*\userprops{\propname }\proptype30{\staticval }}
\hyphauto1\viewscale100\paperh16838\paperw11906\sectd\sbknone\pgwsxn11906\pghsxn16838
{\*\ftnsep\chftnsep}\pgndec\pard\plain \s1\rtlch\afs48\ab\ltrch\fs48\b\sb240\sa283\ltrpar{
Flight seven report}
\par \pard\plain \s21\sb0\sa283\ltrpar{
First paragraph about fuel.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
Danish letters: ^u230\'e6 ^u248\'f8 ^u229\'e5 and bl^u229\'e5b^u230\'e6rgr^u248\'f8d p^u229\'e5 ^u248\'f8en.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
A caf^u233\'e9 at 5 ^u8364\'80 ^u8212\'97 ^u8220\'93quoted^u8221\'94 ^u8230\'85 done.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
Hebrew: }{\ltrch\rtlch
^u1513\'3f^u1500\'3f^u1493\'3f^u1501\'3f}{
 }{
and Chinese: }{\loch\hich\dbch
^u20013\'3f^u25991\'3f }{
and Korean: ^u-10916\'3f^u-21139\'3f^u-14924\'3f.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
A rocket: \uc0 ^u-10179\uc1 ^u-8576\'3f lifts.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
Line one\line line two, a tab\tab stop, }{\b
bo}{
ld in one word.}
\par \pard\plain \s21\sb0\sa283\ltrpar{
Braces \{ and \} and a backslash \\ in text, 10\~km, fuel\-tank, twenty\_one.}
\par \pard\plain \s21\sb0\sa283{\listtext\pard\plain  ^u8226\'95\tab}\ilvl0\ls1 \fi-283\li0\ltrpar{
First bullet}
\par \trowd\trql\ltrrow\clbrdrt\brdrdb\brdrw5\brdrcf15\cellx640\clbrdrt\brdrdb\cellx1226\pard\plain \s26\qc\intbl\ltrpar{
Item}\cell\pard\plain \s26\qc\intbl\ltrpar{
Cost}\cell\row\pard \trowd\trql\ltrrow\cellx640\cellx1226\pard\plain \s23\intbl\ltrpar{
Fuel}\cell\pard\plain \s23\intbl\itap2\ltrpar{
Inner one}\nestcell\pard\plain \s23\intbl\itap2\ltrpar{
Inner two}\nestcell{\*\nesttableprops\trowd\trql\ltrrow\cellx2410\cellx4820\nestrow}{\nonesttables\par}\pard\plain \s23\intbl\ltrpar{
1200}\cell\row\pard \pard\plain \s21\sb0\sa283\ltrpar{
After the table.}
\par \pard\plain \s21\sb0\sa283\pagebb\ltrpar{
The first page ends here.\page The second page begins here.}
\par }";

    const REPORT_TEXT: [&str; 16] = [
        "Flight seven report",
        "First paragraph about fuel.",
        "Danish letters: æ ø å and blåbærgrød på øen.",
        "A café at 5 € — “quoted” … done.",
        "Hebrew: שלום and Chinese: 中文 and Korean: 한국어.",
        "A rocket: 🚀 lifts.",
        "Line one",
        "line two, a tab stop, bold in one word.",
        "Braces { and } and a backslash \\ in text, 10 km, fueltank, twenty-one.",
        "• First bullet",
        "Item Cost",
        "Fuel Inner one Inner two",
        "1200",
        "After the table.",
        "The first page ends here.",
        "The second page begins here.",
    ];

    #[test]
    fn rtf_gives_lines_cells_and_characters_in_order() {
        let d = folder("rtf-report");
        std::fs::write(d.join("Report.RTF"), document(REPORT)).unwrap();
        let text = text_of(&d.join("Report.RTF"), document(REPORT).len() as u64, 1 << 20).unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), REPORT_TEXT);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn rtf_reads_what_word_and_textedit_write() {
        // Word: fonts named before every run, in the middle of a sentence too; letters as bytes
        // of the code page with no `\u` beside them; tables of its own, all behind a star.
        let word = r#"{\rtf1\adeflang1025\ansi\ansicpg1252\uc1\adeff40\deff0\stshfdbch31506\stshfloch31506\deflang1033{\fonttbl{\f0\fbidi \froman\fcharset0
\fprq2{\*\panose 02020603050405020304}Times New Roman;}
{\f11\fbidi \fmodern\fcharset128\fprq1{\*\panose 02020609040205080304}MS Mincho{\*\falt \'82\'6c\'82\'72 \'96\'be\'92\'a9};}{\f40\fbidi \fswiss\fcharset0
\fprq2{\*\panose 020b0604030504040204}Tahoma;}}
{\colortbl;\red0\green0\blue0;}{\*\defchp \fs22 }{\stylesheet{\ql \li0\ri0 \fs22 \snext0 Normal;}{\*\cs10 \additive Default Paragraph Font;}}
{\*\listtable{\list\listtemplateid1{\listlevel\levelnfc23{\leveltext\'01^u-3913 ?;}{\levelnumbers;}\f3}{\listname ;}\listid1}}{\*\rsidtbl \rsid1048627}
{\*\generator Microsoft Word 15.0;}
{\info{\author Hidden Author}{\operator Hidden Operator}{\nofwords1435}}{\*\xmlnstbl {\xmlns1 http://schemas.microsoft.com/office/word/2003/wordml}}
\paperw12240\paperh15840{\*\themedata 504b030414000600080000002100}{\*\latentstyles\lsdstimax371{\lsdlockedexcept \lsdqformat1 Normal;}}
\pard\plain \ltrpar\s27\ql \li0\ri0\sb120\sa120 \rtlch\fcs1 \ab\af40\afs28 \ltrch\fcs0 \b\fs28\lang1033\loch\af40\hich\af40\dbch\af11 {\rtlch\fcs1 \af40
\afs19 \ltrch\fcs0 \fs19\dbch\af13\insrsid1048627 \hich\af40\dbch\af13\loch\f40 FUEL LOG OF FLIGHT SEVEN
\par }\pard\plain \ltrpar\s29\ql \li0 \rtlch\fcs1 \af40 \ltrch\fcs0 \fs19\loch\af40\hich\af40\dbch\af11 {\rtlch\fcs1 \ab0\af40 \ltrch\fcs0 \b0\dbch\af13
\insrsid1048627 \hich\af40\dbch\af13\loch\f40
These terms apply to the }{\rtlch\fcs1 \ab0\af40 \ltrch\fcs0 \b0\dbch\af13\insrsid5320404 \hich\af40\dbch\af13\loch\f40 rocket named above}{\rtlch
\fcs1 \af40 \ltrch\fcs0 \dbch\af13\insrsid1048627 \hich\af40\dbch\af13\loch\f40 . They also apply \hich\af40\dbch\af13\loch\f40 to its caf\'e9
\par {\listtext\pard\plain\ltrpar \s34 \rtlch\fcs1 \af0\afs19 \ltrch\fcs0 \f3\fs19\insrsid1048627 \loch\af3\dbch\af13\hich\f3 \'b7\tab}}\pard\plain \ltrpar
\s34 {\rtlch\fcs1 \af40 \ltrch\fcs0 \insrsid1048627 \hich\af40\dbch\af13\loch\f40 see }{\field\fldedit{\*\fldinst {\rtlch\fcs1 \af40 \ltrch
\fcs0 \insrsid10571001 \hich\af40\dbch\af11\loch\f40  HYPERLINK "http://example.org/hidden" }}{\fldrslt {\rtlch\fcs1 \af40 \ltrch\fcs0 \cs32\ul\cf2
\insrsid4673298 \hich\af40\dbch\af13\loch\f40 the source}
}}\sectd \ltrsect {\rtlch\fcs1 \af40 \ltrch\fcs0 \insrsid4673298 .
\par }}"#;
        assert_eq!(of(word), "FUEL LOG OF FLIGHT SEVEN\nThese terms apply to the rocket named above. They also apply to its café\n· see the source.");

        // TextEdit: a paragraph ends with a backslash at the end of the line, and the fonts
        // stand in their table without a group each.
        let textedit = r"{\rtf1\ansi\ansicpg1252\cocoartf2513
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fmodern\fcharset0 Courier-Bold;\f1\fmodern\fcharset0 Courier;}
{\colortbl;\red255\green255\blue255;\red0\green0\blue0;}
{\*\expandedcolortbl;;\cssrgb\c0\c0\c0;}
{\info
{\author Hidden}}\paperw11900\paperh16840\margl1440\margr1440\vieww25000\viewh16500\viewkind1\viewscale110
\deftab720
\pard\pardeftab720\partightenfactor0

\f0\b\fs26 \cf2 Licences of the parts\
\pard\pardeftab720\partightenfactor0

\f1\b0 \cf2 \
Les parties ont exig\'e9 que le pr\'e9sent contrat soit r\'e9dig\'e9 en anglais.\
The parts are \'a9 2012, provided \'93AS IS\'94.}";
        assert_eq!(
            of(textedit),
            "Licences of the parts\nLes parties ont exigé que le présent contrat soit rédigé en anglais.\nThe parts are © 2012, provided “AS IS”."
        );

        // The smallest: no code page, no character set, a space after a word that belongs to it.
        assert_eq!(
            of(r"{\rtf1\ansi{\fonttbl{\f0 Helvetica;}}\f0\fs32 \b Memo\b0\par\fs24 Booked for 6 October.\par Bring bl\'e5b\'e6r.\par}"),
            "Memo\nBooked for 6 October.\nBring blåbær."
        );
    }

    #[test]
    fn rtf_leaves_out_what_is_not_text_and_keeps_notes_comments_and_text_boxes() {
        let document = r#"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\header\pard\plain Headertext Rocket Works\par}{\footer\pard Footertext page {\field
{\*\fldinst  PAGE }{\fldrslt 2}}\par}
\pard\plain Body with a note{{\super \chftn{\*\footnote \chftn\pard\plain {Notetext from LibreOffice.}}}} after it.\par
Word writes it{\super \chftn {\footnote \pard\plain {\super \chftn }{Notetext from Word.}}} without a star.\par
Comment anchor{{\*\atnid }{\*\atnauthor Hidden Reviewer}\chatn{\*\annotation{\*\atndate -2015295488}\ltrpar{Commenttext is here.}}} continues.\par
Before the frame{\shp{\*\shpinst\shpleft3402\shpright6237{\sp{\sn shapeType}{\sv 202}}{\sp{\sn hiddenproperty}{\sv hiddenvalue}}{\shptxt\pard
\plain {Boxtext inside a frame.}\par \pard}}{\shprslt{\*\do\dobxcolumn{\dptxbxtext Hiddencopy of the box}}}}and after it.\par
{\pict{\*\picprop{\sp{\sn wzName}{\sv Hiddenpicturename}}}\picw40\pich40\pngblip
89504e470d0a1a0a0000000d49484452}Paragraph after a picture.\par
A {\field{\*\fldinst HYPERLINK "https://example.org/hiddentarget" }{\fldrslt {\ul visible link}}} and {\field
{\fldinst HYPERLINK "https://example.org/hiddenagain"}{\fldrslt another}}.\par
Bookmark {\*\bkmkstart hiddenbookmark}marked{\*\bkmkend hiddenbookmark} text.\par
{\object\objemb{\*\objclass Hiddenclass}{\*\objdata 0105000002000000}{\result Hiddenresult}}After an object.\par
{\*\unheardof Hiddenbecause {\b it is} starred}{\unheardof Kept because it is not}.\par}"#;
        let lines = [
            "Headertext Rocket Works",
            "Footertext page 2",
            "Body with a note",
            "Notetext from LibreOffice.",
            "after it.",
            "Word writes it",
            "Notetext from Word.",
            "without a star.",
            "Comment anchor",
            "Commenttext is here.",
            "continues.",
            "Before the frame",
            "Boxtext inside a frame.",
            "and after it.",
            "Paragraph after a picture.",
            "A visible link and another.",
            "Bookmark marked text.",
            "After an object.",
            "Kept because it is not.",
        ];
        assert_eq!(of(document).lines().collect::<Vec<_>>(), lines);

        // The bytes after `\bin` are a picture as it is: braces and backslashes in them are not the document's.
        let picture = [&b"{\\rtf1 Before{\\pict\\wmetafile8\\bin8 "[..], b"}{\\\x00\xff}{}", b"} and after the picture.\\par}"].concat();
        assert_eq!(read(&picture).as_deref(), Some("Before and after the picture.\n"));

        let d = folder("rtf-nothing");
        std::fs::write(d.join("picture.rtf"), r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\info{\title Hidden}}{\pict\pngblip 89504e47}}").unwrap();
        assert_eq!(text(&d.join("picture.rtf"), 1000), None, "a document without text has none");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn rtf_reads_bytes_in_the_code_page_of_the_font_or_else_of_the_document() {
        // WordPad on a Western machine: Russian in a font of the Russian character set.
        let wordpad = r"{\rtf1\ansi\ansicpg1252\deff0\deflang1033{\fonttbl{\f0\fnil\fcharset204 Arial;}{\f1\fnil\fcharset0 Arial;}}
\viewkind4\uc1\pard\lang1049\f0\fs20 \'cf\'f0\'e8\'e2\'e5\'f2 \'ec\'e8\'f0\lang1033\f1  and caf\'e9\par}";
        assert_eq!(of(wordpad), "Привет мир and café");
        // A Russian machine: the code page of the document, and a font that names none.
        assert_eq!(of(r"{\rtf1\ansi\ansicpg1251\deff0{\fonttbl{\f0\fswiss Arial;}}\f0 \'cf\'f0\'e8\'e2\'e5\'f2\par}"), "Привет");
        assert_eq!(of(r"{\rtf1\ansi\ansicpg1251 \'cf\'f0\'e8\'e2\'e5\'f2\par}"), "Привет", "no table of fonts at all");

        // The languages Coxswain speaks that code page 1252 does not write.
        assert_eq!(
            of(r"{\rtf1\ansi\ansicpg1252{\fonttbl{\f0\fcharset0 Arial;}{\f1\fcharset177 Arial;}}\f0 Hebrew: \rtlch\f1 \'f9\'ec\'e5\'ed\ltrch\f0  end\par}"),
            "Hebrew: שלום end"
        );
        assert_eq!(of(r"{\rtf1\ansi\ansicpg1252{\fonttbl{\f0\fcharset186 Arial Baltic;}}\f0 \'ed\'eemi\'edis \'e0\'feuolas\par}"), "ķīmiķis ąžuolas");
        assert_eq!(of(r"{\rtf1\ansi\ansicpg1257 \'ed\'eemi\'edis \'e0\'feuolas\par}"), "ķīmiķis ąžuolas");
        let two_fonts = r"{\rtf1\ansi{\fonttbl{\f0\fcharset238 Arial CE;}{\f1\fcharset161 Arial Greek;}}
\f0 Za\'bf\'f3\'b3\'e6 g\'ea\'9cl\'b9 \f1 \'ea\'e1\'eb\'e7\'ec\'dd\'f1\'e1\par}";
        assert_eq!(of(two_fonts), "Zażółć gęślą καλημέρα");

        // Two bytes to a character. The second one may be a letter, a brace or a backslash.
        assert_eq!(
            of(r"{\rtf1\ansi{\fonttbl{\f0\fcharset0 Arial;}{\f1\fnil\fcharset134 SimSun;}}\f0 Chinese: \f1 \'d6\'d0\'ce\'c4\f0  end\par}"),
            "Chinese: 中文 end"
        );
        assert_eq!(of(r"{\rtf1\ansi{\fonttbl{\f0\fmodern\fcharset128 MS Mincho;}}\f0 \'93\'fa\'96\{\'8c\'ea \'83A\'83\\\'83r\par}"), "日本語 アソビ");
        assert_eq!(of(r"{\rtf1\ansi\ansicpg949 \'c7\'d1\'b1\'b9\'be\'ee\par}"), "한국어");
        let textedit = r"{\rtf1\ansi\ansicpg1252\cocoartf2513{\fonttbl\f0\fswiss\fcharset0 Helvetica;\f1\fnil\fcharset128 HiraginoSans-W3;}
\f0\fs24 \cf0 Hello \f1 \'93\'fa\'96\'7b\'8c\'ea
\f0  caf\'e9}";
        assert_eq!(of(textedit), "Hello 日本語 café", "a table of fonts without a group for each");

        // A group gives the font back when it closes, and `\plain` goes back to the default one.
        let fonts = r"{\rtf1\ansi\deff0{\fonttbl{\f0\fcharset0 Arial;}{\f1\fcharset204 Arial;}}\f1 \'f0\'e0\'e7 {\f0 caf\'e9} \'e4\'e2\'e0 \plain caf\'e9\par}";
        assert_eq!(of(fonts), "раз café два café");
        assert_eq!(
            of(r"{\rtf1\ansi\deff1{\fonttbl{\f0\fcharset0 Arial;}{\f1\fcharset204 Arial;}}\'f0\'e0\'e7\par}"),
            "раз",
            "the default font, when none is named"
        );
        assert_eq!(of(r"{\rtf1\ansi{\fonttbl{\f0\fnil\cpg1251 Arial;}}\f0 \'f0\'e0\'e7\par}"), "раз", "a font may name its code page itself");
        assert_eq!(of(r"{\rtf1\mac{\fonttbl{\f0\fswiss Helvetica;}}\f0 caf\'8e p\'8c \'bfen\par}"), "café på øen");
        assert_eq!(of(r"{\rtf1\ansi\ansicpg437 caf\'e9\par}"), "café", "a code page the reader does not have is read as 1252");

        // `\uN` is followed by as many bytes as `\uc` says, for a reader without Unicode.
        assert_eq!(of(r"{\rtf1\ansi\ansicpg932\uc2 ^u26085\'93\'fa^u26412\'96\'7b end\par}"), "日本 end");
        assert_eq!(of(r"{\rtf1\ansi\uc0 ^u1513^u1500^u1493^u1501  end\par}"), "שלום end");
        assert_eq!(of(r"{\rtf1\ansi\uc1 {\uc2 ^u228\'e4\'e4}^u228\'e4\'e4\par}"), "äää", "a group gives the count back when it closes");
        assert_eq!(
            of(r"{\rtf1\ansi\uc1 rocket ^u-10179^u-8576? and ^u-10179?^u-8576? end\par}"),
            "rocket 🚀 and 🚀 end",
            "with the bytes left out after the first half, and without"
        );
    }

    #[test]
    fn rtf_survives_cut_off_random_and_empty_files() {
        let d = folder("rtf-broken");
        std::fs::write(d.join("empty.rtf"), b"").unwrap();
        assert_eq!(text(&d.join("empty.rtf"), 1000), None);
        assert_eq!(text(&d.join("missing.rtf"), 1000), None);
        for other in [&b"Plain text with the wrong name.\n"[..], b"PK\x03\x04 a zip", b"{\\rt", b"{", b" {\\rtf1 late}", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"] {
            std::fs::write(d.join("other.rtf"), other).unwrap();
            assert_eq!(text(&d.join("other.rtf"), 1000), None, "not a document: {other:?}");
        }
        let _ = std::fs::remove_dir_all(d);

        // A document cut off anywhere gives the text before the cut. Its last character may be
        // another than the document's: a number cut short is a number too.
        let (report, whole) = (document(REPORT), REPORT_TEXT.join("\n"));
        for cut in 0..report.len() {
            let text = tidy(&read(&report[..cut]).unwrap_or_default());
            assert!(text.len() <= whole.len(), "cut at {cut}");
            let sure = text.len().saturating_sub(4);
            assert!((0..=sure).rev().find(|&n| text.is_char_boundary(n)).is_some_and(|n| whole.starts_with(&text[..n])), "cut at {cut}: {text:?}");
        }
        assert_eq!(of(&REPORT[..REPORT.find("Cost").unwrap()]).lines().last(), Some("Item"));

        for (broken, text) in [
            (r"{\rtf1 never closed", "never closed"),
            (r"{\rtf1 closed} after the end", "closed"),
            (r"{\rtf1 closed more than opened}}}} after", "closed more than opened"),
            (r"{\rtf1 ends in a backslash\", "ends in a backslash"),
            (r"{\rtf1 ends in a quote\'", "ends in a quote"),
            (r"{\rtf1 ends in half a byte\'e", "ends in half a byte"),
            (r"{\rtf1 no byte\'zz at all}", "no bytezz at all"),
            (r"{\rtf1 ends in a word\pa", "ends in a word"),
            (r"{\rtf1 ends in a number\u-", "ends in a number-"),
            (r"{\rtf1{\fonttbl{\f0\fcharset204 cut in the table of fonts", ""),
            (r"{\rtf1 a number too large^u99999999999999999999? for a character}", "a number too large? for a character"),
            (r"{\rtf1 one half ^u-10179? alone, the other ^u-8576? too}", "one half alone, the other too"),
            (r"{\rtf1 no zero ^u0?\'00 in the text}", "no zero in the text"),
            (r"{\rtf1 \bin-5 fewer than no bytes}", "fewer than no bytes"),
            (r"{\rtf1 more bytes \bin99999999999999999999 than a number holds}", "more bytes than a number holds"),
            (r"{\rtf1 more bytes \bin2000000000 than there are}", "more bytes"),
            (r"{\rtf1\uc-3 ^u65? fewer than none}", "A? fewer than none"),
            (r"{\rtf1^uc2000000000 ^u65 all that follows is passed over}", "A"),
            (r"{\rtf1 a star \* and text, a star {\*} and nothing, a font \f99999 unheard of}", "a star and text, a star and nothing, a font unheard of"),
        ] {
            assert_eq!(of(broken), text, "{broken}");
        }

        // The same bytes every time: a test that fails must fail again.
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let marks = b"{{}}\\\\\\''**abfu0123456789 -;\r\n~_\xe6\x93";
        let words = [&b"\\u"[..], b"\\uc", b"\\bin", b"\\f", b"\\fonttbl", b"\\fcharset", b"\\ansicpg", b"\\*\\footnote", b"\\pict", b"\\par", b"\\'"];
        for round in 0..3000 {
            let mut bytes = b"{\\rtf1".to_vec();
            for _ in 0..random() % 400 {
                match round % 3 {
                    0 => bytes.push(random() as u8),
                    1 => bytes.push(marks[random() as usize % marks.len()]),
                    _ if random() % 3 == 0 => bytes.extend_from_slice(words[random() as usize % words.len()]),
                    _ => bytes.push(marks[random() as usize % marks.len()]),
                }
            }
            let text = read(&bytes).expect("it begins as a document");
            assert!(text.len() <= 3 * bytes.len(), "no more than three bytes of text for a byte of the file");
            assert_eq!(read(&bytes[1..]), None);
        }
    }

    #[test]
    fn rtf_stops_at_the_depth_the_text_and_the_size_it_is_allowed() {
        let nested = |depth: usize| format!("{{\\rtf1 before{} inside{} after}}", "{".repeat(depth), "}".repeat(depth));
        assert_eq!(of(&nested(MAX_DEPTH - 1)), "before inside after");
        assert_eq!(of(&nested(MAX_DEPTH)), "before", "read up to the group that is too deep");
        // A mail as Outlook writes it: a group for every span of its HTML, closed at the end.
        let mail = format!("{{\\rtf1\\ansi\\fromhtml1 Re:{} Chinese Patent Application{} Dear Sirs\\par}}", "{\\lang1033 ".repeat(2714), "}".repeat(2714));
        assert_eq!(of(&mail), "Re: Chinese Patent Application Dear Sirs");
        assert_eq!(read(format!("{{\\rtf1{}", "{".repeat(1_000_000)).as_bytes()).as_deref(), Some(""));
        assert_eq!(read(format!("{{\\rtf1{}", "{}".repeat(1_000_000)).as_bytes()).as_deref(), Some(""));
        let fonts: String = (0..MAX_FONTS + 10).map(|n| format!("{{\\f{n}\\fcharset204 Arial;}}")).collect();
        let many = format!("{{\\rtf1\\ansi{{\\fonttbl{fonts}}}\\f7 \\'f0\\'e0\\'e7 \\f{} \\'f0\\'e0\\'e7\\par}}", MAX_FONTS + 5);
        assert_eq!(of(&many), "раз ðàç", "the fonts past the last one kept are read in the code page of the document");

        // More text than is kept: the reader stops there, in a run of text or between two, and
        // gives what is kept and no more.
        let run = format!("{{\\rtf1 {}last}}", "word ".repeat(MAX_TEXT / 5 + 1000));
        let text = read(run.as_bytes()).unwrap();
        assert!(text.len() == MAX_TEXT && !text.contains("last"), "{}", text.len());
        let lines = format!("{{\\rtf1 {}last}}", "word\\par ".repeat(MAX_TEXT / 5 + 1000));
        let text = read(lines.as_bytes()).unwrap();
        assert!(text.len() == MAX_TEXT && !text.contains("last"), "{}", text.len());
        // A letter of Thai is one byte in the file and three in the text, written by its number or as it is.
        let thai = format!("{{\\rtf1\\ansi\\ansicpg874 {}last}}", "\\'ca".repeat(MAX_TEXT + 1000));
        let text = read(thai.as_bytes()).unwrap();
        assert!(text.len() <= MAX_TEXT && text.len() > MAX_TEXT - 3 && text.chars().all(|c| c == 'ส'), "{}", text.len());
        let mut thai = b"{\\rtf1\\ansi\\ansicpg874 ".to_vec();
        thai.resize(5 << 20, 0xca);
        let text = read(&thai).unwrap();
        assert!(text.len() <= MAX_TEXT && text.len() > MAX_TEXT - 3 && text.chars().all(|c| c == 'ส'), "{}", text.len());
        // Two bytes to a character, and the text is cut between two characters.
        let chinese = format!("{{\\rtf1\\ansi\\ansicpg936 a{}last}}", "\\'d6\\'d0".repeat(MAX_TEXT / 3 + 1000));
        let text = read(chinese.as_bytes()).unwrap();
        assert!(text.len() <= MAX_TEXT && text.len() > MAX_TEXT - 3 && text.chars().skip(1).all(|c| c == '中'), "{}", text.len());

        // A header and an instruction longer than is looked into: the one is given as often as
        // it comes, the other shows nothing.
        let long = "word ".repeat(MAX_LOOKED / 5 + 1);
        assert_eq!(of(&format!("{{\\rtf1{{\\header {long}}}{{\\header {long}}}{{\\header short}}{{\\header short}}}}")).lines().count(), 3);
        assert_eq!(of(&format!("{{\\rtf1 Name: {{\\field{{\\*\\fldinst MACROBUTTON None {long}}}}} end}}")), "Name: end");
        // Inside each other, a thousand deep, each is looked into for what is its own.
        let headers = format!("{{\\rtf1 before{}{} after}}", "{\\header x".repeat(1000), "}".repeat(1000));
        assert_eq!(of(&headers).lines().count(), 1002);
        let buttons = format!("{{\\rtf1 before {}{} after}}", "{\\fldinst MACROBUTTON a b".repeat(1000), "}".repeat(1000));
        assert_eq!(of(&buttons), format!("before {} after", "b".repeat(1000)));

        let d = folder("rtf-limits");
        std::fs::write(d.join("long.rtf"), &lines).unwrap();
        let text = text_of(&d.join("long.rtf"), lines.len() as u64, 64 << 20).unwrap();
        assert!(text.len() <= MAX_TEXT && text.len() > MAX_TEXT - 8 && text.lines().all(|l| l == "word" || "word".starts_with(l)));
        // A file that grew after it was measured is read as far as it was allowed to be.
        std::fs::write(d.join("grown.rtf"), r"{\rtf1 first words and then the last ones}").unwrap();
        assert_eq!(text_of(&d.join("grown.rtf"), 43, 18), None);
        assert_eq!(super::text(&d.join("grown.rtf"), 18).as_deref(), Some("first words"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn rtf_parts_a_note_a_comment_a_text_box_and_a_list_mark_from_the_words_beside_them() {
        // LibreOffice ends the text of a comment and of a note without `\par`, and OnlyOffice the
        // last paragraph of a text box: only the group parts their last word from the next one.
        let comment = r"{\rtf1\ansi {Total cost is }{{\*\atnid }{\*\atnauthor Reviewer}\chatn{\*\annotation{\*\atndate 1206454912}\ltrpar{check this number}}}{1200 euro.}\par}";
        assert_eq!(of(comment), "Total cost is\ncheck this number\n1200 euro.");
        assert_eq!(found(&of(comment), ["number", "1200", "number1200"]), [true, true, false]);
        let note = r"{\rtf1\ansi \pard\plain {{\super ${\*\footnote\ftnalt $\pard\plain {\tab Titolo uno}}}}{Capitolo primo}\par}";
        assert_eq!(of(note), "$\n$ Titolo uno\nCapitolo primo");
        let word = r"{\rtf1\ansi See the manual{\super \chftn {\footnote \pard\plain {\super \chftn }{of 2004}}}and the mark.\par}";
        assert_eq!(of(word), "See the manual\nof 2004\nand the mark.");
        let chart = r"{\rtf1\ansi {\shp{\*\shpinst\shpleft0{\sp{\sn shapeType}{\sv 1}}{\shptxt \pard\plain{\b FULL NAME}\b\par\pard\plain{Position}\fs18}}}{\b ORGANIZATIONAL CHART}\par}";
        assert_eq!(of(chart), "FULL NAME\nPosition\nORGANIZATIONAL CHART");
        assert_eq!(of(r"{\rtf1\ansi{\header\pard Headword}{\footer\pard Footword}Bodyword\par}"), "Headword\nFootword\nBodyword");
        // The mark of a list item stays on the line of its item.
        assert_eq!(of(r"{\rtf1\ansi{\listtext\pard\plain 1}Introduction\par{\pntext\pard\plain 2.1}Background\par}"), "1 Introduction\n2.1 Background");
    }

    #[test]
    fn rtf_keeps_the_run_when_a_star_stands_in_the_middle_of_its_group() {
        // StarOffice and OpenOffice.org name the style of a run so, for an old reader to pass over that one word.
        let link = r#"{\rtf1\ansi See {\field{\*\fldinst HYPERLINK "http://example.org/" }{\fldrslt \*\cs12\cf2\ul\ulc0 Geschichte}} and {\i0\b0\*\cs7\cf0 the quick brown fox}.\par}"#;
        assert_eq!(of(link), "See Geschichte and the quick brown fox.");
        assert_eq!(of("{\\rtf1 one{ \r\n\\*\\unheardof hidden} two{\\b\\*\\unheardof  three}{text \\*\\unheardof  four}\\par}"), "one two threetext four");
    }

    #[test]
    fn rtf_keeps_the_text_a_field_shows_of_its_instruction_and_nothing_else_of_it() {
        // A word with its reading above it, as LibreOffice and Word write it: the result is empty.
        let ruby = r#"{\rtf1\ansi Ruby: {\field{\*\fldinst { EQ \\* jc0 \\* "Font:Noto Serif CJK SC" \\* hps12 \\o(\\s\\up 11(^u12392 ?^u12358 ?),^u26481 ?^u20140 ?)}}{\fldrslt {}}} city.\par}"#;
        assert_eq!(of(ruby), "Ruby: 東京 city.");
        let inside = r#"{\rtf1\ansi\uc2 {\field{\*\fldinst  EQ \\* jc4 \\* "Font:^u26032\'b7\'73, ^u39636\'c5\'e9" \\* hps10 \\o\\ar(\\s\\up 10(^u12365\'c6\'b1),^u39740\'b0\'ad^u38272\'aa\'f9{
)}{\fldrslt }}^u12398\'c6\'ce^u26041\'a4\'e8\par}"#;
        assert_eq!(of(inside), "鬼門の方", "no space is put into a sentence that has none");
        // A place holder, 'click here and type'.
        assert_eq!(of(r"{\rtf1\ansi Name: {\field{\*\fldinst MACROBUTTON  None Placeholderword and more}} end\par}"), "Name: Placeholderword and more end");
        assert_eq!(of(r"{\rtf1\ansi Name: {\field{\*\fldinst {\b MACROBUTTON NoMacro [Click }{\i here]}}{\fldrslt }} end\par}"), "Name: [Click here] end");

        assert_eq!(of(r#"{\rtf1\ansi Page {\field{\*\fldinst  PAGE }{\fldrslt 2}}, {\field{\*\fldinst EQ \\f(1,2)}}{\field{\fldinst MACROBUTTON}} end\par}"#), "Page 2, end");
        for cut in [
            r#"{\rtf1\ansi See {\field{\*\fldinst HYPERLINK "http://example.org/hidden"#,
            r#"{\rtf1\ansi See {\field{\*\fldinst {\b HYPERLINK "http://example.org/hidden" }{\i \'e9"#,
            r"{\rtf1\ansi See {\field{\*\fldinst MACROBUTTON None Place",
        ] {
            assert_eq!(of(cut), "See", "a document cut off inside an instruction: {cut}");
        }
    }

    #[test]
    fn rtf_reads_the_text_written_in_a_drawn_shape_and_no_other_property_of_it() {
        // LibreOffice writes the text of a rectangle or an ellipse so, and Word the text of WordArt.
        let shape = r"{\rtf1\ansi Shape: {\field{\*\fldinst SHAPE }{\fldrslt{\shp{\*\shpinst\shpleft0{\sp{\sn shapeType}{\sv 3}}{\sp{\sn wzName}{\sv Shape1}}{\sp{\sn gtextUNICODE}{\sv Pump station}}{\sp{\sn gtextFont}{\sv Liberation Serif}}}}}}follows.\par}";
        assert_eq!(of(shape), "Shape:\nPump station\nfollows.");
        assert_eq!(of(r"{\rtf1\ansi{\sp{\sn gtextUNICODEs}{\sv hidden}}{\sp{\sv hidden}}{\sp{\sn gtextUNICODE}{\sn wzName}{\sv hidden}} nothing\par}"), "nothing");
    }

    #[test]
    fn rtf_reads_the_runs_of_a_formula_and_not_its_properties() {
        // LibreOffice and Word write the runs behind a star, OnlyOffice without one.
        let formula = r"{\rtf1\ansi Formula: {\mmath {\*\moMath {\mr Energy}{\mr =}{\mr mass}{\msSup {\me {\mr c}}{\msup {\mr 2}}}}{\mmathPict {\*\shppict{\pict\pngblip 89504e47}}}} follows.\par}";
        assert_eq!(of(formula), "Formula: Energy = mass c 2 follows.");
        let word = r"{\rtf1\ansi{\fonttbl{\f34\fcharset0 Cambria Math;}{\f635\fcharset161 Cambria Math Greek;}}{\mmath{\*\moMathPara {\*\moMath {\i\f34 {\mr\mscr0\msty2 A=}}{\i\f635
{\mr\mscr0\msty2 \'f0}}{\mf{\mfPr{\mtype noBar}{\mctrlPr\f34 }}{\mnum{\mr n}}{\mden{\mr k}}}{\mnary{\mnaryPr{\mchr ^u8721 ?}{\mlimLoc undOvr}{\msubHide on}}{\me{\mr x}}}}}}\par}";
        assert_eq!(of(word), "A= π n k x");
        assert_eq!(of(r"{\rtf1\ansi Formula: {\mmath {\moMath {\mr Energy}{\mr =}{\mr mass}}} follows.\par}"), "Formula: Energy = mass follows.");
    }

    #[test]
    fn rtf_reads_a_text_box_as_word_95_drew_it_and_not_the_copy_of_a_later_one() {
        let drawn = r"{\rtf1\ansi\ansicpg1252\deff0 Before{\*\do\dobxpage\dobypara\dodhgt8192\dptxbx\dptxbxmar0{\dptxbxtext\ltrpar\f8\fs50\cf1 textbox without border}\dpx929\dpy340\dplinehollow0}and after\par}";
        assert_eq!(of(drawn), "Before\ntextbox without border\nand after");
        let copied = r"{\rtf1\ansi{\shp{\*\shpinst{\shptxt\pard Boxtext\par}}{\shprslt{\*\do\dobxcolumn\dptxbx{\dptxbxtext\pard Boxtext\par}}}}\par}";
        assert_eq!(of(copied), "Boxtext");
    }

    #[test]
    fn rtf_lets_the_index_find_a_word_beside_a_symbol_and_a_word_with_a_soft_hyphen() {
        // A tick of Wingdings and a degree sign of Symbol are characters of the private area, letters to the index.
        let symbol = r"{\rtf1\ansi{\fonttbl{\f0\fcharset0 Arial;}{\f6\fcharset2 Wingdings;}}\f0 {\f6 ^u-3844\'fc}Done and 20{\f6 ^u-3920\'b0}C\par}";
        assert_eq!(of(symbol), "Done and 20 C");
        assert_eq!(found(&of(symbol), ["and", "Done", "20"]), [true, true, true]);
        // A soft hyphen is no part of its word, however it is written: pandoc writes it by its number.
        let soft = r"{\rtf1\ansi fuel^u173 ?tank and bl\'e5b\'e6r\'adgr\'f8d and soft\-hyphen\par}";
        assert_eq!(of(soft), "fueltank and blåbærgrød and softhyphen");
        assert_eq!(found(&of(soft), ["fueltank", "blåbærgrød", "softhyphen"]), [true, true, true]);
    }

    #[test]
    fn rtf_reads_the_cells_a_writer_puts_inside_the_properties_of_a_nested_row() {
        let nested = r"{\rtf1\ansi\ansicpg1251 \trowd\cellx5027\intbl\itap2{\*\nesttableprops\trowd\cellx968\itap2{\'c3\'f3\'eb\'fc\'ea\'ee}\nestcell\nestrow}\itap1 after\cell\row\pard\par}";
        assert_eq!(of(nested), "Гулько\nafter");
    }

    #[test]
    fn rtf_gives_the_same_header_or_footer_once() {
        // One for the first, the left and the right pages each, and all of them again for the next section.
        let pages = r"{\rtf1\ansi{\headerf \pard Rocket Works\par}{\headerl \pard Rocket Works\par}{\headerr \pard Rocket {\b Works}\par}{\footerr \pard Page {\field{\*\fldinst PAGE}{\fldrslt 1}}\par}
\pard Body\par\sect\sectd{\header \pard Rocket Works\par}{\footer \pard Page {\field{\*\fldinst PAGE}{\fldrslt 1}}}{\footerf \pard Rocket Works, first page\par}\pard Rocket Works\par\headery720 more\par}";
        assert_eq!(of(pages), "Rocket Works\nPage 1\nBody\nRocket Works, first page\nRocket Works\nmore");
        // No more of them are remembered than there is room for; the ones that follow are given as they come.
        let many: String = (0..MAX_HEADERS + 2).map(|n| format!("{{\\header h{n}}}{{\\header h{n}}}")).collect();
        let text = of(&format!("{{\\rtf1 {many}{{\\header h1}}{{\\header h{}}}}}", MAX_HEADERS + 1));
        assert_eq!(text.lines().count(), MAX_HEADERS + 2 * 2 + 1, "{:?}", text.lines().rev().take(6).collect::<Vec<_>>());
    }

    #[test]
    fn rtf_ends_a_line_at_a_column_break_and_at_a_soft_break() {
        let breaks = r"{\rtf1\ansi\cols2 The first column ends\column here, a line\softline breaks, a page\softpage ends and a column\softcol too.\par}";
        assert_eq!(of(breaks), "The first column ends\nhere, a line\nbreaks, a page\nends and a column\ntoo.");
    }

    /// The store hands over regular files only. Asked for a pipe all the same, the reader must not
    /// wait for someone to write into it.
    #[cfg(unix)]
    #[test]
    fn rtf_gives_nothing_for_a_named_pipe_and_does_not_wait_for_it() {
        let d = folder("rtf-pipe");
        let pipe = d.join("pipe.rtf");
        assert!(std::process::Command::new("mkfifo").arg(&pipe).status().unwrap().success());
        let (done, waiting) = std::sync::mpsc::channel();
        let asked = pipe.clone();
        std::thread::spawn(move || done.send(text(&asked, 1000)));
        let answer = waiting.recv_timeout(std::time::Duration::from_secs(3));
        if answer.is_err() {
            // The reader waits. A writer that opens the pipe and says nothing lets it go.
            drop(std::fs::OpenOptions::new().write(true).open(&pipe));
        }
        let _ = std::fs::remove_dir_all(d);
        assert_eq!(answer, Ok(None));
    }
}

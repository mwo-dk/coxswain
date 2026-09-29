//! Word and OpenDocument text: .docx .docm .dotx (Office Open XML) and .odt .ott
//! (OpenDocument). Both are zip files of XML parts, and a few of the parts hold the text.
//!
//! The body is read first, then headers, footers, notes and comments: when the text is cut
//! at `MAX_TEXT`, it is the body that stays. The parts are unpacked one at a time, so no more
//! than one of them is in memory.
//!
//! The shared `xml_text` does not do here. A Word file has numbers between its tags that place
//! a picture, a table is to give one line for each row, and a text box, a note or a comment
//! stands in the middle of a paragraph without being part of its sentence.

use std::collections::HashSet;
use std::io::{Read, Seek};
use std::path::Path;

use quick_xml::events::BytesStart;
use quick_xml::name::QName;

use super::MAX_TEXT;

/// The most parts read of one file. Every section of a document can have headers and footers
/// of its own, and they mostly say the same.
const MAX_PARTS: usize = 100;

/// The most XML unpacked and parsed of one file, all parts together. A part can be all tags
/// and no text, and a tag costs the parser as much as a word: the time it takes to read a file
/// must not be the choice of who made the file. A document with this much XML has more text
/// than is kept, well before its end.
const MAX_XML: u64 = 16 * 1024 * 1024;

type Zip = zip::ZipArchive<Metered>;

/// How a format keeps its text. An element is known by its name without the prefix, `w:p` by
/// `p`, as the prefix is the writer's choice. A name given with its prefix is that element
/// only: `dc:date` is when a comment was made, `text:date` is a date in the text.
struct Format {
    /// The parts that hold text, in the order they are read, without ".xml" and the number
    /// before it.
    parts: &'static [&'static str],
    /// Text counts inside these only.
    text: &'static [&'static str],
    /// These start a line and end one. Both, as what stands in the middle of a paragraph has
    /// paragraphs of its own, and "Before", "In the box", "after" are not one word.
    lines: &'static [&'static str],
    /// What these put into the text where they stand.
    signs: &'static [(&'static str, &'static str)],
    /// A cell of a table, which ends a word, and a row of one, which ends a line.
    cell: &'static str,
    row: &'static str,
    /// These are left out with all inside them.
    skip: &'static [&'static str],
}

/// Text is in `w:t`, and in `m:t` and `a:t` of formulas and drawings. Deleted text and field
/// codes are in elements of their own (`w:delText`, `w:instrText`), so they are never text.
/// A field to fill in (`w:sdt`) ends a line as a paragraph does: two of them side by side
/// are two answers, with nothing between them in the file. So does a run of a formula
/// (`m:r`): what stands over and under the line of a fraction has nothing between it.
static WORD: Format = Format {
    parts: &["word/document", "word/header", "word/footer", "word/footnotes", "word/endnotes", "word/comments"],
    text: &["t"],
    lines: &["p", "br", "cr", "sdt", "m:r"],
    signs: &[("tab", " "), ("ptab", " "), ("noBreakHyphen", "-")],
    cell: "tc",
    row: "tr",
    // Text that was moved away is read where it went. A text box is written twice, and the
    // second is a copy for older programs.
    skip: &["moveFrom", "Fallback"],
};

/// Text is in paragraphs and headings, and in the title and the description of a picture.
/// Headers and footers are in styles.xml.
static OPEN: Format = Format {
    parts: &["content", "styles"],
    text: &["p", "h", "title", "desc"],
    lines: &["p", "h", "title", "desc", "line-break"],
    signs: &[("tab", " "), ("s", " ")],
    cell: "table-cell",
    row: "table-row",
    // Deleted text, the number of a note, who made a comment and when (the initials have one
    // name in the standard and another in the files LibreOffice wrote before it), a picture
    // kept as text inside the document, and the code of a script set into the text.
    skip: &["tracked-changes", "note-citation", "dc:creator", "dc:date", "creator-initials", "sender-initials", "binary-data", "script"],
};

/// The text of a Word or OpenDocument file: its body, then its headers, footers, notes and
/// comments. `None` when the file is no such document, has no text, or is larger than `max`.
pub fn text(path: &Path, max: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size > max {
        return None;
    }
    let mut zip = zip::ZipArchive::new(Metered { file, left: size.saturating_mul(8).saturating_add(1 << 20) }).ok()?;
    // An OpenDocument file with a password keeps its parts under their names, as ciphertext.
    if unpack(&mut zip, "META-INF/manifest.xml", 1 << 20).is_some_and(|manifest| String::from_utf8_lossy(&manifest).contains("encryption-data")) {
        return None;
    }
    let mut parts: Vec<(usize, &Format, String)> = zip.file_names().filter_map(|name| part(name).map(|(at, format)| (at, format, name.to_string()))).collect();
    parts.sort_by_cached_key(|(at, _, name)| (*at, super::natural(name)));
    let (mut out, mut room) = (String::new(), MAX_XML);
    for (_, format, name) in parts.iter().take(MAX_PARTS) {
        if out.len() >= MAX_TEXT || room == 0 {
            break;
        }
        let Some(xml) = unpack(&mut zip, name, room) else { continue };
        room -= xml.len() as u64;
        let mut text = String::new();
        read(&xml, format, &mut text);
        // The styles of an OpenDocument file hold a header and a footer for every kind of page,
        // used or not, and a watermark is in all ten of them: each line of them once.
        if name == "styles.xml" {
            let mut seen = HashSet::new();
            text = text.lines().filter(|line| seen.insert(*line)).collect::<Vec<_>>().join("\n");
        }
        keep(&mut out, &text);
        keep(&mut out, "\n");
    }
    (!out.trim().is_empty()).then_some(out)
}

/// A file that gives no more than `left` bytes, all told. zip looks from the end of the file
/// for the record that says where its directory is, and when the directory is broken, tries
/// the next such record before it, and the next, each time reading as far as the record
/// points: a file of nothing but such records costs it the square of its length, an hour for
/// 20 MB. A document is read once through, and a few times its length is more than enough.
struct Metered {
    file: std::fs::File,
    left: u64,
}

impl Read for Metered {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::other("read more than a document needs"));
        }
        let most = usize::try_from(self.left).unwrap_or(usize::MAX).min(buf.len());
        let n = self.file.read(&mut buf[..most])?;
        self.left -= n as u64;
        Ok(n)
    }
}

impl Seek for Metered {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(pos)
    }
}

/// One part of the zip, unpacked, no more than `room` bytes of it. A part cut off there is
/// read as far as it goes.
fn unpack(zip: &mut Zip, name: &str, room: u64) -> Option<Vec<u8>> {
    let mut bytes = vec![];
    zip.by_name(name).ok()?.take(room).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Where a part of the zip comes in the order of reading, and its format. `None` for a part
/// that holds no text.
fn part(name: &str) -> Option<(usize, &'static Format)> {
    let kind = name.strip_suffix(".xml")?.trim_end_matches(|c: char| c.is_ascii_digit());
    [&WORD, &OPEN].into_iter().find_map(|format| Some((format.parts.iter().position(|part| *part == kind)?, format)))
}

/// Whether the element is one of `names`.
fn among(names: &[&str], name: QName) -> bool {
    names.iter().any(|n| *n == name.as_ref() || *n == name.local_name().as_ref())
}

/// The value of the element's attribute named `key` (without its prefix), if it has one.
fn attribute(element: &BytesStart, key: &str) -> Option<String> {
    let found = element.attributes().flatten().find(|a| a.key.local_name().as_ref() == key)?;
    Some(found.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned())
}

/// Adds `text` to `out`, as much of it as `MAX_TEXT` leaves room for.
fn keep(out: &mut String, text: &str) {
    let mut room = MAX_TEXT.saturating_sub(out.len()).min(text.len());
    while !text.is_char_boundary(room) {
        room -= 1;
    }
    out.push_str(&text[..room]);
}

/// Adds the text of one part to `out`. What is left of a broken part is still read.
fn read(xml: &[u8], format: &Format, out: &mut String) {
    use quick_xml::events::Event;
    let xml = String::from_utf8_lossy(xml);
    let mut reader = quick_xml::Reader::from_str(&xml);
    // An end without a start, or an "&" on its own, is no reason to stop reading.
    let config = reader.config_mut();
    (config.check_end_names, config.allow_unmatched_ends, config.allow_dangling_amp) = (false, true, true);
    // How deep the reader is in what is left out, in elements of text and in cells of tables.
    // Counted, not called: a document nested a million deep costs no more than a flat one.
    let (mut skipping, mut inside, mut cells) = (0usize, 0usize, 0usize);
    // How many ends of lines are to be left out: a drop cap is a paragraph of its own, of the
    // first letters of the paragraph after it, so it ends no line and the next starts none.
    let mut joins = 0usize;
    // Inside the list of a drop-down field: which entry is chosen, and how many were seen.
    let mut list: Option<(usize, usize)> = None;
    while out.len() < MAX_TEXT {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        let (element, name, opens, closes) = match &event {
            Event::Start(e) => (Some(e), e.name(), true, false),
            Event::Empty(e) => (Some(e), e.name(), true, true),
            Event::End(e) => (None, e.name(), false, true),
            _ if skipping > 0 || inside == 0 => continue,
            Event::Text(t) => {
                keep(out, t);
                continue;
            }
            Event::CData(t) => {
                keep(out, t);
                continue;
            }
            // &amp; and &#233; arrive on their own. An entity the document made up is dropped:
            // none is ever looked up or unfolded, so none can grow without end.
            Event::GeneralRef(r) => {
                match r.resolve_char_ref() {
                    Ok(Some(c)) => keep(out, c.encode_utf8(&mut [0; 4])),
                    _ => keep(out, quick_xml::escape::resolve_xml_entity(r).unwrap_or("")),
                }
                continue;
            }
            _ => continue,
        };
        if skipping > 0 || among(format.skip, name) {
            match (opens, closes) {
                (true, false) => skipping += 1,
                (false, true) => skipping = skipping.saturating_sub(1),
                _ => {}
            }
            continue;
        }
        let (text, cell) = (among(format.text, name) as usize, among(&[format.cell], name) as usize);
        if among(format.lines, name) {
            if joins > 0 {
                joins -= 1;
            } else {
                // Inside a cell a line ends as a word does: the row is the line.
                keep(out, if cells > 0 { " " } else { "\n" });
            }
        }
        if let Some(element) = element {
            if let Some((_, sign)) = format.signs.iter().find(|(element, _)| among(&[element], name)) {
                keep(out, sign);
            }
            match name.local_name().as_ref() {
                "framePr" if attribute(element, "dropCap").is_some() => joins = 2,
                // A watermark, and WordArt, is a shape whose words are an attribute.
                "textpath" => {
                    if let Some(words) = attribute(element, "string") {
                        keep(out, &format!("\n{words}\n"));
                    }
                }
                // A drop-down field of the older kind: its entries are attributes, and the
                // page shows the chosen one.
                "ddList" if !closes => list = Some((0, 0)),
                "result" if list.is_some() => list = attribute(element, "val").and_then(|v| v.parse().ok()).map(|chosen| (chosen, 0)),
                "listEntry" => {
                    if let Some((chosen, seen)) = list.as_mut() {
                        if chosen == seen {
                            keep(out, &format!(" {} ", attribute(element, "val").unwrap_or_default()));
                        }
                        *seen += 1;
                    }
                }
                _ => {}
            }
            if !closes {
                inside += text;
                cells += cell;
            }
        }
        if closes {
            if !opens {
                inside = inside.saturating_sub(text);
                cells = cells.saturating_sub(cell);
                if name.local_name().as_ref() == "ddList" {
                    list = None;
                }
            }
            if cell > 0 {
                keep(out, " ");
            }
            if among(&[format.row], name) {
                keep(out, "\n");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::{folder, zip_file};
    use crate::extract::tidy;

    /// The main part of a Word file with this body.
    fn word(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
             xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">\
             <w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>"
        )
    }

    /// The main part of an OpenDocument file with this body.
    fn open(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
             xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">\
             <office:body><office:text>{body}</office:text></office:body></office:document-content>"
        )
    }

    /// A paragraph of a Word file.
    fn p(text: &str) -> String {
        format!("<w:p><w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr><w:r><w:rPr></w:rPr><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
    }

    /// What the reader makes of a file with these parts, as the store gets it.
    fn read_file(name: &str, parts: &[(&str, &str)]) -> Option<String> {
        let d = folder(name);
        let parts: Vec<(&str, &[u8])> = parts.iter().map(|(name, xml)| (*name, xml.as_bytes())).collect();
        zip_file(&d.join(name), &parts);
        let text = text(&d.join(name), u64::MAX).map(|t| tidy(&t));
        let _ = std::fs::remove_dir_all(d);
        text
    }

    #[test]
    fn word_gives_the_body_then_headers_footers_notes_and_comments() {
        let part = |root: &str, text: &str| format!("<w:{root} xmlns:w=\"w\">{}</w:{root}>", p(text));
        let text = read_file(
            "parts.docx",
            &[
                ("word/comments.xml", &part("comments", "A comment")),
                ("word/commentsExtended.xml", &part("commentsEx", "Not a part with text")),
                ("word/endnotes.xml", &part("endnotes", "An endnote")),
                ("word/footer1.xml", &part("ftr", "A footer")),
                ("word/footnotes.xml", &part("footnotes", "A footnote")),
                ("word/header10.xml", &part("hdr", "Header ten")),
                ("word/header2.xml", &part("hdr", "Header two")),
                ("word/styles.xml", &part("styles", "A style")),
                ("word/glossary/document.xml", &part("glossaryDocument", "A building block")),
                ("docProps/core.xml", "<cp:coreProperties><dc:title>A title</dc:title></cp:coreProperties>"),
                ("word/document.xml", &word(&p("The body"))),
            ],
        );
        assert_eq!(text.as_deref(), Some("The body\nHeader two\nHeader ten\nA footer\nA footnote\nAn endnote\nA comment"));
        // The main part has a number in its name when the document was saved on the web.
        assert_eq!(read_file("second.docm", &[("word/document2.xml", &word(&p("The body")))]).as_deref(), Some("The body"));
    }

    #[test]
    fn word_keeps_a_word_whole_and_two_words_apart() {
        let body = [
            "<w:p><w:r><w:t xml:space=\"preserve\">A word with </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>\
             <w:proofErr w:type=\"spellStart\"/><w:r><w:t>ness</w:t></w:r><w:bookmarkStart w:id=\"0\" w:name=\"here\"/>\
             <w:r><w:lastRenderedPageBreak/><w:t xml:space=\"preserve\"> inside.</w:t></w:r></w:p>",
            &p("Blåbærgrød på æblet, a café, שלום and 你好世界."),
            &p("Fuel &amp; fire &lt;&#233;&#x41;&gt; &made-up;<![CDATA[<kept>]]> R & D"),
            "<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"720\"/></w:tabs></w:pPr><w:r><w:t>Tab</w:t><w:tab/><w:t>separated, line</w:t>\
             <w:br/><w:t>broken, page</w:t><w:br w:type=\"page\"/><w:t>broken, non</w:t><w:noBreakHyphen/><w:t>breaking, hy</w:t>\
             <w:softHyphen/><w:t>phen.</w:t></w:r></w:p>",
            "<w:p><w:hyperlink r:id=\"rId2\"><w:r><w:t>Linked</w:t></w:r></w:hyperlink><w:r><w:t xml:space=\"preserve\"> words.</w:t></w:r></w:p>",
            // Two fields to fill in, side by side as LibreOffice writes them.
            "<w:p><w:sdt><w:sdtPr><w:alias w:val=\"Not text\"/><w:text/></w:sdtPr><w:sdtContent><w:r><w:t>Postal code</w:t></w:r></w:sdtContent>\
             </w:sdt><w:r><w:rPr></w:rPr></w:r><w:sdt><w:sdtPr><w:text/></w:sdtPr><w:sdtContent><w:r><w:t>City</w:t></w:r></w:sdtContent></w:sdt></w:p>",
        ]
        .concat();
        let text = read_file("words.docx", &[("word/document.xml", &word(&body))]).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "A word with boldness inside.");
        assert_eq!(lines[1], "Blåbærgrød på æblet, a café, שלום and 你好世界.");
        assert_eq!(lines[2], "Fuel & fire <éA> <kept> R & D");
        assert_eq!(lines[3..], ["Tab separated, line", "broken, page", "broken, non-breaking, hyphen.", "Linked words.", "Postal code", "City"]);
    }

    #[test]
    fn word_keeps_a_text_box_apart_from_the_paragraph_it_stands_in() {
        // As LibreOffice and Word write it: the box twice, and numbers that place it.
        let body = "<w:p><w:r><w:t>Before</w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wp:anchor>\
                    <wp:positionH relativeFrom=\"column\"><wp:posOffset>123456</wp:posOffset></wp:positionH>\
                    <wp:positionV><wp:align>center</wp:align></wp:positionV><wp:docPr id=\"1\" name=\"Frame1\"/>\
                    <a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>In the box.</w:t></w:r></w:p></w:txbxContent>\
                    </wps:txbx></wps:wsp></a:graphicData></a:graphic>\
                    <wp14:sizeRelH relativeFrom=\"margin\"><wp14:pctWidth>40000</wp14:pctWidth></wp14:sizeRelH></wp:anchor></w:drawing></mc:Choice>\
                    <mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>In the box.</w:t></w:r></w:p></w:txbxContent>\
                    </v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t>after</w:t></w:r></w:p>";
        assert_eq!(read_file("box.docx", &[("word/document.xml", &word(body))]).as_deref(), Some("Before\nIn the box.\nafter"));
    }

    #[test]
    fn word_leaves_out_deleted_and_moved_text_and_field_codes() {
        let body = "<w:p><w:r><w:t>Kept</w:t></w:r><w:del w:id=\"0\" w:author=\"Someone\"><w:r><w:delText>Deleted</w:delText></w:r></w:del>\
                    <w:ins w:id=\"1\"><w:r><w:t xml:space=\"preserve\"> and added</w:t></w:r></w:ins></w:p>\
                    <w:p><w:moveFrom w:id=\"2\"><w:r><w:t>Moved</w:t></w:r></w:moveFrom>\
                    <w:r><w:rPr><w:moveFrom w:id=\"3\"/></w:rPr><w:t>Stayed</w:t></w:r></w:p>\
                    <w:p><w:moveTo w:id=\"4\"><w:r><w:t>Moved</w:t></w:r></w:moveTo></w:p>\
                    <w:p><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
                    <w:r><w:instrText xml:space=\"preserve\"> PAGE \\* MERGEFORMAT </w:instrText></w:r>\
                    <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>2</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";
        assert_eq!(read_file("changes.docx", &[("word/document.xml", &word(body))]).as_deref(), Some("Kept and added\nStayed\nMoved\nPage 2"));
    }

    #[test]
    fn a_table_gives_a_line_for_each_row_and_a_word_for_each_cell() {
        let cell = |text: &[&str]| format!("<w:tc><w:tcPr><w:tcW w:w=\"3212\"/></w:tcPr>{}</w:tc>", text.iter().map(|t| p(t)).collect::<String>());
        let row = |cells: &[String]| format!("<w:tr><w:trPr></w:trPr>{}</w:tr>", cells.concat());
        let table = format!(
            "{}<w:tbl><w:tblGrid><w:gridCol w:w=\"3212\"/></w:tblGrid>{}{}</w:tbl>{}",
            p("Before"),
            row(&[cell(&["Item"]), cell(&["Cost"]), cell(&["Note"])]),
            row(&[cell(&["Fuel"]), cell(&["1200"]), cell(&["Largest", "Two paragraphs"])]),
            p("After")
        );
        let lines = "Before\nItem Cost Note\nFuel 1200 Largest Two paragraphs\nAfter";
        assert_eq!(read_file("table.docx", &[("word/document.xml", &word(&table))]).as_deref(), Some(lines));

        let paragraphs = |text: &[&str]| text.iter().map(|t| format!("<text:p>{t}</text:p>")).collect::<String>();
        let cell = |text: &[&str]| format!("<table:table-cell office:value-type=\"string\">{}</table:table-cell>", paragraphs(text));
        let row = |cells: &[String]| format!("<table:table-row>{}</table:table-row>", cells.concat());
        let table = format!(
            "<text:p>Before</text:p><table:table table:name=\"Costs\"><table:table-column table:number-columns-repeated=\"3\"/>\
             {}{}</table:table><text:p>After</text:p>",
            row(&[cell(&["Item"]), cell(&["Cost"]), cell(&["Note"])]),
            row(&[cell(&["Fuel"]), cell(&["1200"]), cell(&["Largest", "Two paragraphs"])]),
        );
        assert_eq!(read_file("table.odt", &[("content.xml", &open(&table))]).as_deref(), Some(lines));
    }

    #[test]
    fn opendocument_gives_the_body_then_headers_and_footers() {
        let body = "<text:sequence-decls><text:sequence-decl text:name=\"Table\"/></text:sequence-decls>\
                    <text:table-of-content><text:table-of-content-source>\
                    <text:index-title-template>Not text</text:index-title-template></text:table-of-content-source>\
                    <text:index-body><text:p>Rocket debrief<text:tab/>1</text:p></text:index-body></text:table-of-content>\
                    <text:h text:outline-level=\"1\">Rocket debrief</text:h>\
                    <text:p>A word with <text:span text:style-name=\"T1\">bold</text:span>ness<text:bookmark text:name=\"here\"/>\
                    <text:soft-page-break/> inside.</text:p>\
                    <text:p>Blåbærgrød på æblet, a café, שלום and 你好世界.</text:p>\
                    <text:p>Tab<text:tab/>separated, line<text:line-break/>broken, three<text:s text:c=\"3\"/>spaces, one<text:s/>space.</text:p>\
                    <text:p>A <text:a xlink:href=\"https://example.com/\">linked</text:a> word, written \
                    <text:date text:date-value=\"2026-09-29\">29 September</text:date>.</text:p>\
                    <text:list><text:list-item><text:p>First of a list</text:p></text:list-item>\
                    <text:list-item><text:p>Second of a list</text:p></text:list-item></text:list>";
        let styles = "<office:document-styles><office:styles><text:notes-configuration>\
                      <text:note-continuation-notice-forward>Not text</text:note-continuation-notice-forward>\
                      </text:notes-configuration></office:styles><office:master-styles><style:master-page style:name=\"Standard\">\
                      <style:header><text:p>A header</text:p></style:header>\
                      <style:footer><text:p>Page <text:page-number text:select-page=\"current\">2</text:page-number></text:p></style:footer>\
                      </style:master-page></office:master-styles></office:document-styles>";
        let text = read_file(
            "parts.ott",
            &[
                ("mimetype", "application/vnd.oasis.opendocument.text-template"),
                ("styles.xml", styles),
                ("meta.xml", "<office:document-meta><office:meta><dc:title>A title</dc:title></office:meta></office:document-meta>"),
                ("Object 1/content.xml", &open("<text:p>A document inside the document</text:p>")),
                ("content.xml", &open(body)),
            ],
        );
        let lines = [
            "Rocket debrief 1",
            "Rocket debrief",
            "A word with boldness inside.",
            "Blåbærgrød på æblet, a café, שלום and 你好世界.",
            "Tab separated, line",
            "broken, three spaces, one space.",
            "A linked word, written 29 September.",
            "First of a list",
            "Second of a list",
            "A header",
            "Page 2",
        ];
        assert_eq!(text.unwrap().lines().collect::<Vec<_>>(), lines);
    }

    #[test]
    fn opendocument_keeps_notes_comments_and_frames_apart_and_leaves_out_deleted_text() {
        let body = "<text:tracked-changes><text:changed-region text:id=\"ct1\"><text:deletion><office:change-info>\
                    <dc:creator>Who deleted</dc:creator><dc:date>2026-09-01T10:00:00</dc:date></office:change-info>\
                    <text:p>Deleted</text:p></text:deletion></text:changed-region></text:tracked-changes>\
                    <text:p>Kept<text:change text:change-id=\"ct1\"/> after it.</text:p>\
                    <text:p>Noted<text:note text:id=\"ftn1\" text:note-class=\"footnote\"><text:note-citation>1</text:note-citation>\
                    <text:note-body><text:p>A footnote.</text:p></text:note-body></text:note> and on.</text:p>\
                    <text:p>Remarked<office:annotation><dc:creator>Who remarked</dc:creator><dc:date>2026-09-02T11:00:00</dc:date>\
                    <meta:creator-initials>WR</meta:creator-initials><text:p>A comment.</text:p></office:annotation> and on.</text:p>\
                    <text:p>Before<draw:frame draw:name=\"Frame1\" text:anchor-type=\"as-char\"><draw:text-box>\
                    <text:p>In the box.</text:p></draw:text-box></draw:frame>after.</text:p>\
                    <text:p>Shown<draw:frame draw:name=\"Image1\"><draw:image>\
                    <office:binary-data>iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB</office:binary-data></draw:image>\
                    <svg:title>A rocket</svg:title><svg:desc>On the pad</svg:desc></draw:frame>here.</text:p>";
        let text = read_file("apart.odt", &[("content.xml", &open(body))]).unwrap();
        let lines = [
            "Kept after it.",
            "Noted",
            "A footnote.",
            "and on.",
            "Remarked",
            "A comment.",
            "and on.",
            "Before",
            "In the box.",
            "after.",
            "Shown",
            "A rocket",
            "On the pad",
            "here.",
        ];
        assert_eq!(text.lines().collect::<Vec<_>>(), lines);
    }

    #[test]
    fn a_file_that_is_no_document_gives_none() {
        let d = folder("none");
        let read = |name: &str, bytes: &[u8]| {
            std::fs::write(d.join(name), bytes).unwrap();
            text(&d.join(name), u64::MAX)
        };
        assert_eq!(read("empty.docx", b""), None);
        assert_eq!(read("text.odt", b"Plain text with the name of a document."), None);
        assert_eq!(read("start.docx", b"PK\x03\x04 and no more of a zip"), None);
        // An older Word file, and one with a password, is no zip at all.
        assert_eq!(read("older.docx", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1\0\0\0\0\0\0\0\0"), None);
        assert_eq!(text(&d.join("missing.docx"), u64::MAX), None);
        assert_eq!(text(&d, u64::MAX), None, "a folder");

        zip_file(&d.join("other.docx"), &[("xl/workbook.xml", b"<workbook><t>Not this</t></workbook>"), ("word/media/document.png", b"<w:t>Nor this</w:t>")]);
        assert_eq!(text(&d.join("other.docx"), u64::MAX), None, "a zip without the parts of a document");
        zip_file(&d.join("tags.docx"), &[("word/document.xml", word("<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr></w:p>").as_bytes())]);
        assert_eq!(text(&d.join("tags.docx"), u64::MAX), None, "a document without text");

        zip_file(&d.join("large.docx"), &[("word/document.xml", word(&p("The body")).as_bytes())]);
        let size = std::fs::metadata(d.join("large.docx")).unwrap().len();
        assert_eq!(text(&d.join("large.docx"), size - 1), None, "larger than allowed");
        assert!(text(&d.join("large.docx"), size).is_some());
        let _ = std::fs::remove_dir_all(d);
    }

    /// Bytes that look like chance and are the same every time.
    fn noise(length: usize, mut seed: u32) -> Vec<u8> {
        let next = |seed: &mut u32| {
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (*seed >> 24) as u8
        };
        (0..length).map(|_| next(&mut seed)).collect()
    }

    #[test]
    fn a_broken_file_gives_none_or_what_is_left_and_never_a_panic() {
        let d = folder("broken");
        let path = d.join("broken.docx");
        for seed in 0..50 {
            std::fs::write(&path, noise(3000, seed)).unwrap();
            assert_eq!(text(&path, u64::MAX), None, "chance, seed {seed}");
            // Chance behind the first bytes of a zip, and as the parts of one.
            std::fs::write(&path, [&b"PK\x03\x04\x14\0\0\0\x08\0"[..], &noise(3000, seed)].concat()).unwrap();
            assert_eq!(text(&path, u64::MAX), None, "chance in a zip, seed {seed}");
            let parts = [noise(3000, seed), noise(3000, seed + 100), noise(3000, seed + 200)];
            zip_file(&path, &[("word/document.xml", &parts[0]), ("content.xml", &parts[1]), ("styles.xml", &parts[2])]);
            let _ = text(&path, u64::MAX);
        }

        // A file cut off anywhere, and one with a byte changed anywhere.
        let body = word(&[p("The first paragraph."), p("The second, of a café &amp; 你好.")].concat());
        zip_file(&path, &[("word/document.xml", body.as_bytes()), ("word/footnotes.xml", body.as_bytes())]);
        let whole = std::fs::read(&path).unwrap();
        assert!(text(&path, u64::MAX).is_some());
        for cut in (0..whole.len()).step_by(3) {
            std::fs::write(&path, &whole[..cut]).unwrap();
            let _ = text(&path, u64::MAX);
        }
        for at in (0..whole.len()).step_by(3) {
            let mut changed = whole.clone();
            changed[at] = !changed[at];
            std::fs::write(&path, changed).unwrap();
            let _ = text(&path, u64::MAX);
        }
        let _ = std::fs::remove_dir_all(d);

        // The same for the parts themselves, at every byte.
        let content = open(
            "<text:p>The first<text:s/>paragraph.</text:p><table:table><table:table-row><table:table-cell>\
             <text:p>A caf&#233;</text:p></table:table-cell></table:table-row></table:table>",
        );
        for (xml, format) in [(body.as_bytes(), &WORD), (content.as_bytes(), &OPEN)] {
            for at in 0..xml.len() {
                read(&xml[..at], format, &mut String::new());
                for with in [b'<', b'>', b'&', b'/', b'"', 0, 0xFF] {
                    let mut changed = xml.to_vec();
                    changed[at] = with;
                    read(&changed, format, &mut String::new());
                }
            }
        }
        let mut left = String::new();
        read(&body.as_bytes()[..body.find("paragraph").unwrap()], &WORD, &mut left);
        assert_eq!(tidy(&left), "The first", "what is left of a part that is cut off");

        // An end without a start counts nothing below zero, and the text after it is read.
        let mut out = String::new();
        read(b"</w:tc></w:t></w:moveFrom></w:p><w:p><w:t>Still read</w:t>", &WORD, &mut out);
        assert_eq!(tidy(&out), "Still read");
    }

    #[test]
    fn a_document_nested_deep_or_full_of_entities_is_read_within_the_limits() {
        let deep = 200_000;
        let nested = format!("{}<w:t>At the bottom</w:t>{}", "<w:tbl><w:tr><w:tc><w:p>".repeat(deep), "</w:p></w:tc></w:tr></w:tbl>".repeat(deep));
        let mut out = String::new();
        read(nested.as_bytes(), &WORD, &mut out);
        assert_eq!(tidy(&out), "At the bottom");

        let laughs = "<!DOCTYPE l [<!ENTITY a \"ha\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\">]><w:p><w:r><w:t>Said &b;&b;&b; once</w:t></w:r></w:p>";
        let mut out = String::new();
        read(laughs.as_bytes(), &WORD, &mut out);
        assert_eq!(tidy(&out), "Said once", "entities of the document's own are never unfolded");

        // More text than is kept: the body fills it, and the part after it is not read.
        let long = "é".repeat(MAX_TEXT / 2 - 100);
        let body = word(&[p("The start"), p(&long), p(&long), p(&long)].concat());
        let text = read_file("long.docx", &[("word/document.xml", &body), ("word/header1.xml", &p("A header"))]).unwrap();
        assert!(text.len() <= MAX_TEXT && text.len() > MAX_TEXT - 4, "{} bytes", text.len());
        assert!(text.starts_with("The start\né") && text.ends_with('é'));
    }

    #[test]
    fn word_joins_a_drop_cap_to_the_paragraph_it_starts() {
        // As Word and OnlyOffice write it: the first letters in a paragraph of their own.
        let body = "<w:p><w:pPr><w:keepNext/><w:framePr w:dropCap=\"drop\" w:lines=\"3\" w:hSpace=\"113\" w:wrap=\"around\" w:hAnchor=\"text\" \
                    w:vAnchor=\"text\"/><w:spacing w:after=\"0\" w:line=\"720\" w:lineRule=\"exact\"/></w:pPr><w:r><w:rPr><w:sz w:val=\"72\"/></w:rPr>\
                    <w:t xml:space=\"preserve\">Y</w:t></w:r></w:p><w:p><w:r><w:t xml:space=\"preserve\">esterday the ascent was nominal.</w:t></w:r></w:p>\
                    <w:p><w:pPr><w:framePr w:w=\"2000\" w:hAnchor=\"page\"/></w:pPr><w:r><w:t>A frame that is no drop cap</w:t></w:r></w:p>";
        let text = read_file("dropcap.docx", &[("word/document.xml", &word(body))]);
        assert_eq!(text.as_deref(), Some("Yesterday the ascent was nominal.\nA frame that is no drop cap"));
    }

    #[test]
    fn word_keeps_the_words_of_a_watermark_and_the_chosen_entry_of_a_drop_down() {
        // A watermark, as LibreOffice and Word write it: a shape whose words are an attribute.
        let header = "<w:hdr xmlns:w=\"w\" xmlns:v=\"v\"><w:p><w:r><w:pict><v:shape id=\"PowerPlusWaterMarkObject\" type=\"_x0000_t136\">\
                      <v:path textpathok=\"t\"/><v:textpath on=\"t\" fitshape=\"t\" string=\"Strictly &amp; confidential\" \
                      style=\"font-family:&quot;Liberation Sans&quot;;font-size:1pt\" trim=\"t\"/></v:shape></w:pict></w:r></w:p></w:hdr>";
        // A drop-down field of the older kind, as a file that has been .doc keeps it.
        let body = "<w:p><w:r><w:t xml:space=\"preserve\">Choose </w:t></w:r><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData><w:name w:val=\"Gas\"/>\
                    <w:enabled/><w:ddList><w:result w:val=\"1\"/><w:listEntry w:val=\"Oxygen\"/><w:listEntry w:val=\"Nitrogen\"/>\
                    <w:listEntry w:val=\"Argon\"/></w:ddList></w:ffData></w:fldChar></w:r><w:r><w:instrText xml:space=\"preserve\"> FORMDROPDOWN </w:instrText>\
                    </w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r><w:r><w:t xml:space=\"preserve\"> now.</w:t></w:r></w:p>\
                    <w:p><w:r><w:fldChar w:fldCharType=\"begin\"><w:ffData><w:ddList><w:listEntry w:val=\"First\"/><w:listEntry w:val=\"Second\"/></w:ddList>\
                    </w:ffData></w:fldChar></w:r><w:r><w:t xml:space=\"preserve\"> is chosen when none is.</w:t></w:r></w:p>";
        let text = read_file("fields.docx", &[("word/document.xml", &word(body)), ("word/header1.xml", header)]);
        assert_eq!(text.as_deref(), Some("Choose Nitrogen now.\nFirst is chosen when none is.\nStrictly & confidential"));
    }

    #[test]
    fn word_keeps_the_parts_of_a_formula_apart() {
        // As LibreOffice exports "velocity = {distance} over {elapsed}".
        let body = "<w:p><w:r><w:t xml:space=\"preserve\">Velocity is </w:t></w:r>\
                    <m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t xml:space=\"preserve\">velocity</m:t></m:r>\
                    <m:r><m:t xml:space=\"preserve\">=</m:t></m:r><m:f><m:num><m:r><m:t xml:space=\"preserve\">distance</m:t></m:r></m:num>\
                    <m:den><m:r><m:t xml:space=\"preserve\">elapsed</m:t></m:r></m:den></m:f></m:oMath></w:p>";
        let text = read_file("formula.docx", &[("word/document.xml", &word(body))]).unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), ["Velocity is", "velocity", "=", "distance", "elapsed"]);
    }

    #[test]
    fn opendocument_leaves_out_the_initials_of_a_comment_and_the_code_of_a_script() {
        // A comment as LibreOffice writes it for ODF 1.2 Extended, and a script set into the text.
        let body = "<text:p>Vibration<office:annotation loext:resolved=\"false\"><dc:creator>Bo Jensen</dc:creator><dc:date>2026-09-02T11:00:00</dc:date>\
                    <loext:sender-initials>BJ</loext:sender-initials><text:p>Recheck the accelerometer.</text:p></office:annotation> peaked at liftoff.</text:p>\
                    <text:p>Beforehand<text:script script:language=\"JavaScript\">var scriptsecret = 42;</text:script> and afterwards stand apart.</text:p>\
                    <text:p>A long com\u{AD}pound noun.</text:p>";
        let text = read_file("comment.odt", &[("content.xml", &open(body))]).unwrap();
        let lines = ["Vibration", "Recheck the accelerometer.", "peaked at liftoff.", "Beforehand and afterwards stand apart.", "A long compound noun."];
        assert_eq!(text.lines().collect::<Vec<_>>(), lines);
        // The soft hyphen as OnlyOffice writes it into a Word file: the word stays whole there too.
        assert_eq!(read_file("soft.docx", &[("word/document.xml", &word(&p("A long com\u{AD}pound noun.")))]).as_deref(), Some("A long compound noun."));
    }

    #[test]
    fn opendocument_reads_the_same_header_once_and_a_locked_file_not_at_all() {
        let page = |name: &str, header: &str| {
            format!("<style:master-page style:name=\"{name}\"><style:header><text:p>{header}</text:p></style:header></style:master-page>")
        };
        let styles = format!(
            "<office:document-styles><office:master-styles>{}{}{}{}</office:master-styles></office:document-styles>",
            page("Standard", "Strictly confidential"),
            page("First_20_Page", "Strictly confidential"),
            page("Envelope", "Strictly confidential"),
            page("Index", "The index")
        );
        let text = read_file("watermark.odt", &[("content.xml", &open("<text:p>Body under a watermark.</text:p>")), ("styles.xml", &styles)]);
        assert_eq!(text.as_deref(), Some("Body under a watermark.\nStrictly confidential\nThe index"));

        // A file with a password keeps its parts under their names, as ciphertext, and says so
        // in its manifest. Its content.xml here is readable, so that only the manifest counts.
        let manifest = "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">\
                        <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\" manifest:size=\"3456\">\
                        <manifest:encryption-data manifest:checksum-type=\"SHA1/1K\" manifest:checksum=\"abc=\"><manifest:algorithm \
                        manifest:algorithm-name=\"http://www.w3.org/2001/04/xmlenc#aes256-cbc\" manifest:initialisation-vector=\"abc=\"/>\
                        </manifest:encryption-data></manifest:file-entry></manifest:manifest>";
        let content = open("<text:p>Would be ciphertext.</text:p>");
        assert_eq!(read_file("locked.odt", &[("META-INF/manifest.xml", manifest), ("content.xml", &content)]), None);
        let plain = manifest.replace("<manifest:encryption-data", "<manifest:other").replace("</manifest:encryption-data>", "</manifest:other>");
        assert_eq!(read_file("open.odt", &[("META-INF/manifest.xml", &plain), ("content.xml", &content)]).as_deref(), Some("Would be ciphertext."));
    }

    #[test]
    fn a_file_of_zip_endings_or_of_tags_alone_is_read_within_its_limits() {
        let d = folder("bounds");
        // The record at the end of a zip that says where its directory is, over and over: each
        // says the directory is at the start, where there is none.
        let ending = b"PK\x05\x06\0\0\0\0\x01\0\x01\0\x2e\0\0\0\0\0\0\0\0\0";
        std::fs::write(d.join("endings.docx"), ending.repeat(512 * 1024 / ending.len())).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(text(&d.join("endings.docx"), u64::MAX), None);
        assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
        // What zip reads of a file is metered, whatever it looks for.
        std::fs::write(d.join("short.docx"), b"0123456789").unwrap();
        let mut metered = Metered { file: std::fs::File::open(d.join("short.docx")).unwrap(), left: 7 };
        let mut read = String::new();
        assert!(metered.read_to_string(&mut read).is_err());
        assert_eq!(read, "0123456");

        // A part that is all tags: the parser's work is bounded, not only the text it gives.
        let tag = format!("<w:x w:val=\"{}\"/>", "a".repeat(1000));
        let tags = tag.repeat(MAX_XML as usize / tag.len() + 1);
        let body = word(&format!("{tags}{}", p("Behind the limit")));
        zip_file(&d.join("tags.docx"), &[("word/document.xml", body.as_bytes()), ("word/header1.xml", p("A header").as_bytes())]);
        assert_eq!(text(&d.join("tags.docx"), u64::MAX), None, "neither the text behind the limit nor the part after it");
        let body = word(&format!("{}{}", tag.repeat(1000), p("Within the limit")));
        zip_file(&d.join("fewer.docx"), &[("word/document.xml", body.as_bytes()), ("word/header1.xml", p("A header").as_bytes())]);
        assert_eq!(text(&d.join("fewer.docx"), u64::MAX).map(|t| tidy(&t)).as_deref(), Some("Within the limit\nA header"));
        let _ = std::fs::remove_dir_all(d);
    }
}

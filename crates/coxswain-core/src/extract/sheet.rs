//! Spreadsheets: `.xlsx` and `.xlsm` (XML in a zip), `.xlsb` (records in a zip), `.xls`
//! (records in a compound file) and `.ods` (XML in a zip).
//!
//! Every sheet gives its name on a line and then its rows, the cells of a row separated by a
//! tab. A formula gives the value it had when the file was saved. A number is written as a
//! person reads it: 1840, 12.5%, 2026-03-14. A cell without a value gives nothing. The notes
//! on the cells follow the rows.
//!
//! A sheet can claim a million rows in a few bytes, so no count that a file states is
//! believed. Text is made only of cells that are written out in the file, a cell or a row
//! that is said to repeat is read once, and the reading stops at `MAX_TEXT`.
//!
//! The kind of file is told by its first bytes and not by its name: a `.xls` is often a
//! `.xlsx`, or a web page with a table, that was given the name.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use super::{MAX_ENTRY, MAX_TEXT, page_text, start, utf8, xml_text};

/// The most sheets read of one workbook. Each is unpacked on its own.
const MOST_SHEETS: usize = 256;
/// The most cell formats kept of one workbook. Excel itself stops at 65,490.
const MOST_FORMATS: usize = 1 << 16;
/// The most shared strings kept of one workbook: more than fill `MAX_TEXT` with short words.
const MOST_STRINGS: usize = 1 << 20;
/// The first bytes of a compound file.
const COMPOUND: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// What Excel writes in a cell whose formula went wrong. Nothing to search for.
const ERRORS: [&str; 7] = ["#N/A", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#NULL!"];
/// The elements of a web page that end a line: a page closes no `<br>`, and often no `<td>`.
const PAGE: [&str; 20] = ["tr", "td", "th", "br", "p", "div", "title", "h1", "h2", "h3", "h4", "h5", "h6", "caption", "li", "dt", "dd", "option", "pre", "hr"];
/// The elements of the XML that Excel 2003 wrote that end a line, and those that are its
/// settings, styles and properties, with no cell in them.
const XML_2003: [&str; 6] = ["Worksheet", "Table", "Row", "Cell", "Data", "Comment"];
const XML_2003_SKIP: [&str; 7] = ["DocumentProperties", "OfficeDocumentSettings", "ExcelWorkbook", "Styles", "Names", "WorksheetOptions", "AutoFilter"];

pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut head = vec![];
    std::fs::File::open(path).ok()?.take(8).read_to_end(&mut head).ok()?;
    let text = if head.starts_with(b"PK") {
        let mut zip = Zip { archive: zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?, left: 4 * MAX_ENTRY };
        match zip.part("content.xml") {
            Some(content) => ods(&content),
            None => excel(&mut zip)?,
        }
    } else if head == COMPOUND {
        let mut file = vec![];
        std::fs::File::open(path).ok()?.take(max).read_to_end(&mut file).ok()?;
        xls(&file)?
    } else {
        // Programs on the web give the name `.xls` to a page with a table, to XML and to
        // text with tabs or commas in it.
        let mut bytes = vec![];
        std::fs::File::open(path).ok()?.take(max).read_to_end(&mut bytes).ok()?;
        let mut text = decoded(bytes)?;
        match text.trim_start_matches('\u{feff}').trim_start().starts_with('<') {
            true if text.contains("urn:schemas-microsoft-com:office:spreadsheet") => xml_text(text.as_bytes(), &XML_2003, &XML_2003_SKIP),
            true => page_text(text.as_bytes(), &PAGE, &["style", "script"]),
            false => {
                text.truncate(start(&text, MAX_TEXT).len());
                text
            }
        }
    };
    (!text.trim().is_empty()).then_some(text)
}

/// Text as a program on the web writes it: UTF-8, UTF-16 with a mark at its start, or
/// Windows-1252 when it is neither, as exports for Denmark and its neighbours often are. A
/// page that is UTF-8 but for a stray byte is read as UTF-8 still. A zero byte near the
/// start says that it is no text at all.
fn decoded(bytes: Vec<u8>) -> Option<String> {
    let wide = |bytes: &[u8], unit: fn([u8; 2]) -> u16| {
        let units = bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]]));
        Some(char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect())
    };
    match bytes.get(..2) {
        Some([0xFF, 0xFE]) => return wide(&bytes[2..], u16::from_le_bytes),
        Some([0xFE, 0xFF]) => return wide(&bytes[2..], u16::from_be_bytes),
        _ => {}
    }
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return None;
    }
    let fault = match String::from_utf8(bytes) {
        Ok(text) => return Some(text),
        Err(fault) => fault,
    };
    let valid = fault.utf8_error().valid_up_to();
    let bytes = fault.into_bytes();
    if !bytes[..valid].is_ascii() {
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    // Windows-1252 is Latin-1 but for the 32 characters that Latin-1 leaves for controls.
    const OWN: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š',
        '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    Some(bytes.iter().map(|b| if (0x80..0xA0).contains(b) { OWN[*b as usize - 0x80] } else { *b as char }).collect())
}

// ---------------------------------------------------------------- the text, as it grows

/// Starts a line, unless the text is at the start of one.
fn line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Adds text to what is built, as far as `MAX_TEXT`: no cell, string, name or note is kept
/// longer than all the text may be, however long a file makes one.
fn add(built: &mut String, text: &str) {
    built.push_str(start(text, MAX_TEXT.saturating_sub(built.len())));
}

/// Adds a cell to the line, after a tab when it is not the first. A line break inside a cell
/// becomes a space, so that a row stays a line.
fn cell(out: &mut String, text: &str) {
    let text = start(text, MAX_TEXT.saturating_sub(out.len()));
    if text.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\t');
    }
    out.extend(text.chars().map(|c| if c.is_control() { ' ' } else { c }));
}

/// A text on a line of its own: the name of a sheet, a note on a cell.
fn alone(out: &mut String, text: &str) {
    line(out);
    cell(out, text);
    line(out);
}

// ---------------------------------------------------------------- numbers and dates

/// What the format of a cell makes of its number.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Number,
    Percent,
    Date,
    /// A span of time, which may be longer than a day: 255:10:10.
    Span,
}

/// What a format makes of a number: by the code it is written in when it has one, else by its
/// number among the formats that Excel is born with. A code is one of a date when it has
/// more letters of days, weeks, months, years, hours and seconds than it has places for
/// digits, or when it is minutes and seconds and nothing else: M:S.00.
fn kind(id: u32, code: Option<&str>) -> Kind {
    let Some(code) = code else {
        return match id {
            9 | 10 => Kind::Percent,
            14..=22 | 27..=36 | 45..=47 | 50..=58 => Kind::Date,
            _ => Kind::Number,
        };
    };
    let (mut letters, mut digits, mut percent, mut chars) = (0, 0, false, code.chars());
    // Whether the pattern is minutes, seconds and their fractions, and nothing else.
    let mut clock = true;
    while let Some(c) = chars.next() {
        match c {
            // Text in quotes, a colour or a currency in brackets and a character that is
            // written as it is are no part of the pattern. Hours, minutes or seconds in
            // brackets are counted on beyond a day.
            '"' => drop(chars.by_ref().find(|c| *c == '"')),
            '[' => {
                let inside: String = chars.by_ref().take_while(|c| *c != ']').collect();
                if !inside.is_empty() && ["h", "m", "s"].iter().any(|unit| inside.to_lowercase().replace(unit, "").is_empty()) {
                    return Kind::Span;
                }
            }
            '\\' | '_' | '*' => drop(chars.next()),
            'm' | 's' | 'M' | 'S' => letters += 1,
            'd' | 'y' | 'h' | 'w' | 'D' | 'Y' | 'H' | 'W' => (letters, clock) = (letters + 1, false),
            '0' => digits += 1,
            '#' | '?' => (digits, clock) = (digits + 1, false),
            '%' => (percent, clock) = (true, false),
            ':' | '.' => {}
            _ => clock = false,
        }
    }
    if letters > digits || letters > 0 && clock {
        Kind::Date
    } else if percent {
        Kind::Percent
    } else {
        Kind::Number
    }
}

/// The kind of every cell format, from the number format each one names.
fn kinds(codes: &HashMap<u32, Kind>, formats: &[u32]) -> Vec<Kind> {
    formats.iter().map(|id| codes.get(id).copied().unwrap_or_else(|| kind(*id, None))).collect()
}

/// A number as a person reads it: 1840 and not 1840.0, 0.3 and not 0.30000000000000004.
/// A spreadsheet shows fifteen digits; what lies beyond them is the noise of its arithmetic.
fn number(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let rounded: f64 = format!("{value:.14e}").parse().unwrap_or(value);
    if rounded != 0.0 && !(1e-9..1e15).contains(&rounded.abs()) { format!("{rounded:e}") } else { rounded.to_string() }
}

/// The date that Excel counts as so many days, as 2026-03-14, with the time of day when the
/// number has a part of a day, or the time alone when it is less than a day. `None` when
/// the number is no date that Excel knows.
fn date(days: f64, from_1904: bool) -> Option<String> {
    if !(0.0..2_958_466.0).contains(&days) {
        return None;
    }
    let seconds = (days * 86_400.0).round() as i64;
    let (mut day, time) = (seconds / 86_400, seconds % 86_400);
    let clock = format!("{:02}:{:02}:{:02}", time / 3600, time / 60 % 60, time % 60);
    if day == 0 {
        return Some(clock);
    }
    // Day 1 is 1 January 1900, and day 60 is a 29 February 1900 that never was; or day 0 is
    // 1 January 1904, in workbooks that began on a Macintosh.
    day += if from_1904 { 1462 } else { (day < 60) as i64 };
    // The days are counted from 1 March of the year 0, where a cycle of 400 years begins.
    let day = day - 25_569 + 719_468;
    let (era, of_era) = (day.div_euclid(146_097), day.rem_euclid(146_097));
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * of_year + 2) / 153;
    let (d, m) = (of_year - (153 * month + 2) / 5 + 1, if month < 10 { month + 3 } else { month - 9 });
    let y = year_of_era + era * 400 + (m <= 2) as i64;
    Some(if time == 0 { format!("{y:04}-{m:02}-{d:02}") } else { format!("{y:04}-{m:02}-{d:02} {clock}") })
}

/// What the sheets of a workbook of Excel share.
#[derive(Default)]
struct Book {
    /// The strings, which a cell names by their place.
    strings: Vec<String>,
    /// The kind of each cell format, which a cell names by its place.
    kinds: Vec<Kind>,
    /// Whether day 0 is in 1904.
    from_1904: bool,
}

impl Book {
    fn string(&self, at: usize) -> String {
        self.strings.get(at).cloned().unwrap_or_default()
    }

    /// A number as its cell shows it.
    fn number(&self, value: f64, format: usize) -> String {
        match self.kinds.get(format) {
            Some(Kind::Date) => date(value, self.from_1904).unwrap_or_else(|| number(value)),
            Some(Kind::Span) if value.abs() < 1e9 => {
                let seconds = (value.abs() * 86_400.0).round() as i64;
                format!("{}{}:{:02}:{:02}", if value < 0.0 { "-" } else { "" }, seconds / 3600, seconds / 60 % 60, seconds % 60)
            }
            Some(Kind::Percent) => number(value * 100.0) + "%",
            _ => number(value),
        }
    }
}

/// Keeps a shared string, and says whether there is room for more: a table of strings is
/// kept no longer than the part it came from may be. It is not cut where the text is: the
/// strings of the first sheet may lie at the end of a long table, when the sheet was added
/// last and moved to the front.
fn keep(strings: &mut Vec<String>, kept: &mut usize, text: String) -> bool {
    *kept += text.len();
    strings.push(text);
    *kept <= MAX_ENTRY as usize && strings.len() < MOST_STRINGS
}

// ---------------------------------------------------------------- bytes

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..)?.get(..2)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..)?.get(..4)?.try_into().ok()?))
}

fn real(bytes: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(bytes.get(at..)?.get(..8)?.try_into().ok()?))
}

/// A number that Excel packs in four bytes: a whole number or the upper half of a real one,
/// either of them to be divided by a hundred when the lowest bit says so.
fn packed(bytes: &[u8], at: usize) -> Option<f64> {
    let bits = le32(bytes, at)?;
    let value = if bits & 2 != 0 { ((bits as i32) >> 2) as f64 } else { f64::from_bits(((bits & !3) as u64) << 32) };
    Some(if bits & 1 != 0 { value / 100.0 } else { value })
}

/// Text in UTF-16, the lower byte of a character first, and no more of it than `MAX_TEXT`.
fn utf16(bytes: &[u8]) -> String {
    let units = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    let mut text = String::new();
    for c in char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)) {
        if text.len() >= MAX_TEXT {
            break;
        }
        text.push(c);
    }
    text
}

// ---------------------------------------------------------------- XML

/// A part of an XML document.
enum Part<'a> {
    /// An element opens. One that is empty opens and closes.
    Open(&'a BytesStart<'a>),
    /// An element closes: its name, without the prefix.
    Close(&'a str),
    /// Text, its entities resolved.
    Text(&'a str),
}

/// Walks an XML document part by part, until `each` says `false`. What is left of a broken
/// document is walked, as far as it is UTF-8. An entity that the document made up is
/// dropped, as in `xml_text`.
fn walk(xml: &[u8], mut each: impl FnMut(Part) -> bool) {
    let mut reader = quick_xml::Reader::from_str(utf8(xml));
    reader.config_mut().check_end_names = false;
    loop {
        let more = match reader.read_event() {
            Ok(Event::Start(e)) => each(Part::Open(&e)),
            Ok(Event::Empty(e)) => each(Part::Open(&e)) && each(Part::Close(e.local_name().as_ref())),
            Ok(Event::End(e)) => each(Part::Close(e.local_name().as_ref())),
            Ok(Event::Text(t)) => each(Part::Text(&t.into_inner())),
            Ok(Event::CData(t)) => each(Part::Text(&t.into_inner())),
            Ok(Event::GeneralRef(r)) => {
                let mut one = [0u8; 4];
                each(Part::Text(match r.resolve_char_ref() {
                    Ok(Some(c)) => c.encode_utf8(&mut one),
                    _ => match &*r.into_inner() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    },
                }))
            }
            Ok(Event::Eof) | Err(_) => false,
            Ok(_) => true,
        };
        if !more {
            break;
        }
    }
}

/// The value of the attribute of that name, whatever its prefix.
fn attr(element: &BytesStart, name: &str) -> Option<String> {
    let found = element.attributes().with_checks(false).map_while(Result::ok).find(|a| a.key.local_name().as_ref() == name)?;
    Some(found.normalized_value(XmlVersion::Implicit1_0).unwrap_or(found.value.clone()).into_owned())
}

// ---------------------------------------------------------------- .xlsx, .xlsm and .xlsb

/// The zip of a spreadsheet, and how many bytes may still be unpacked of it.
struct Zip {
    archive: zip::ZipArchive<std::fs::File>,
    left: u64,
}

impl Zip {
    /// A part by its name, or the first `MAX_ENTRY` bytes of it, and what was read of it
    /// before a fault in the zip. The sheet of a large workbook unpacks to more than that, and
    /// its first rows are as much text as is kept; `zip_entry` would leave all of it out.
    ///
    /// Some writers call a part by a name with other capitals than they gave it, and some
    /// name the parts with the backslashes of Windows.
    fn part(&mut self, name: &str) -> Option<Vec<u8>> {
        let named = self.archive.file_names().find(|n| n.replace('\\', "/").eq_ignore_ascii_case(name))?.to_string();
        let (mut bytes, most) = (vec![], MAX_ENTRY.min(self.left));
        let _ = self.archive.by_name(&named).ok()?.take(most).read_to_end(&mut bytes);
        self.left -= bytes.len() as u64;
        // Where a part is cut, it is cut between two characters.
        if bytes.len() as u64 == most {
            bytes.truncate(bytes.iter().rposition(|byte| *byte < 0x80).map_or(0, |at| at + 1));
        }
        Some(bytes)
    }
}

/// The relations of a part: of each its name, its type and the part it leads to, which lies
/// in `folder` unless it says where else.
fn relations(xml: &[u8], folder: &str) -> Vec<[String; 3]> {
    let mut all = vec![];
    walk(xml, |part| {
        if let Part::Open(e) = part
            && e.local_name().as_ref() == "Relationship"
        {
            let target = attr(e, "Target").unwrap_or_default();
            let name = target.strip_prefix('/').map_or_else(|| format!("{folder}{target}"), String::from);
            all.push([attr(e, "Id").unwrap_or_default(), attr(e, "Type").unwrap_or_default(), name]);
        }
        all.len() < 4 * MOST_SHEETS
    });
    all
}

/// A workbook of Excel since 2007: parts in a zip, written as XML or as records. The
/// relations of the file say where the workbook lies, the workbook names its sheets, and its
/// own relations say in which part each of them lies.
fn excel(zip: &mut Zip) -> Option<String> {
    let whole = relations(&zip.part("_rels/.rels").unwrap_or_default(), "");
    let name = whole.iter().find(|r| r[1].ends_with("/officeDocument")).map_or("xl/workbook.xml", |r| &r[2]);
    let workbook = zip.part(name)?;
    let (folder, file) = name.split_at(name.rfind('/').map_or(0, |at| at + 1));
    let parts = relations(&zip.part(&format!("{folder}_rels/{file}.rels")).unwrap_or_default(), folder);
    let mut of_type = |kind: &str| parts.iter().find(|p| p[1].ends_with(kind)).and_then(|p| zip.part(&p[2])).unwrap_or_default();
    let (strings, styles, records) = (of_type("/sharedStrings"), of_type("/styles"), name.ends_with(".bin"));

    let (mut sheets, book) = match records {
        true => {
            let (sheets, from_1904) = xlsb_sheets(&workbook);
            (sheets, Book { strings: xlsb_strings(&strings), kinds: xlsb_kinds(&styles), from_1904 })
        }
        false => {
            let (sheets, from_1904) = xlsx_sheets(&workbook);
            (sheets, Book { strings: xlsx_strings(&strings), kinds: xlsx_kinds(&styles), from_1904 })
        }
    };
    drop((workbook, strings, styles));
    // A workbook whose list of sheets cannot be read still has the sheets among its relations,
    // without their names.
    if sheets.is_empty() {
        sheets = parts.iter().filter(|p| p[1].ends_with("/worksheet")).map(|p| (String::new(), p[0].clone())).take(MOST_SHEETS).collect();
    }

    let mut out = String::new();
    for (name, id) in sheets {
        if out.len() > MAX_TEXT {
            return Some(out);
        }
        alone(&mut out, &name);
        match parts.iter().find(|p| p[0] == id).and_then(|p| zip.part(&p[2])) {
            Some(sheet) if records => xlsb_sheet(&sheet, &book, &mut out),
            Some(sheet) => xlsx_sheet(&sheet, &book, &mut out),
            None => {}
        }
    }
    // The notes on the cells, after the sheets. Who wrote them and where they are shown is
    // left out.
    let notes = format!("{folder}comments").to_lowercase();
    let names = zip.archive.file_names().map(|n| n.replace('\\', "/")).filter(|n| n.to_lowercase().starts_with(&notes));
    for name in names.take(MOST_SHEETS).collect::<Vec<_>>() {
        if out.len() > MAX_TEXT {
            break;
        }
        line(&mut out);
        match (zip.part(&name), name.ends_with(".bin")) {
            (Some(notes), true) => xlsb_records(&notes, |id, data| {
                if id == 0x27D {
                    alone(&mut out, &xlsb_string(data, 1).unwrap_or_default().0);
                }
                out.len() <= MAX_TEXT
            }),
            (Some(notes), false) => out.push_str(&xml_text(&notes, &["comment"], &["authors", "rPh", "AlternateContent"])),
            (None, _) => {}
        }
    }
    Some(out)
}

/// Text as Excel writes it in XML, where a character that XML has no place for is written
/// as `_x000D_`: its number, in four digits of sixteen.
fn unescaped(text: String) -> String {
    if !text.contains("_x") {
        return text;
    }
    let (mut out, mut rest) = (String::new(), text.as_str());
    while let Some(at) = rest.find("_x") {
        let digits = rest.get(at + 2..at + 6).filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()) && rest.as_bytes().get(at + 6) == Some(&b'_'));
        match digits.and_then(|digits| char::from_u32(u32::from_str_radix(digits, 16).ok()?)) {
            Some(character) => {
                out.push_str(&rest[..at]);
                out.push(character);
                rest = &rest[at + 7..];
            }
            None => {
                out.push_str(&rest[..at + 2]);
                rest = &rest[at + 2..];
            }
        }
    }
    out + rest
}

/// The sheets of a workbook in XML, each with its name and the name of its relation, and
/// whether day 0 is in 1904.
fn xlsx_sheets(workbook: &[u8]) -> (Vec<(String, String)>, bool) {
    let (mut sheets, mut from_1904) = (vec![], false);
    walk(workbook, |part| {
        if let Part::Open(e) = part {
            match e.local_name().as_ref() {
                "workbookPr" => from_1904 = attr(e, "date1904").is_some_and(|v| v == "1" || v == "true"),
                "sheet" => sheets.push((attr(e, "name").unwrap_or_default(), attr(e, "id").unwrap_or_default())),
                _ => {}
            }
        }
        sheets.len() < MOST_SHEETS
    });
    (sheets, from_1904)
}

/// The shared strings in XML. A string may lie in runs, which are read as one; how it is
/// said aloud, which Japanese has beside the text, is left out.
fn xlsx_strings(xml: &[u8]) -> Vec<String> {
    let (mut strings, mut kept, mut one) = (vec![], 0, String::new());
    let (mut reading, mut aloud) = (false, false);
    walk(xml, |part| {
        match part {
            Part::Open(e) => match e.local_name().as_ref() {
                "si" => one.clear(),
                "t" => reading = true,
                "rPh" => aloud = true,
                _ => {}
            },
            Part::Close("t") => reading = false,
            Part::Close("rPh") => aloud = false,
            Part::Close("si") => return keep(&mut strings, &mut kept, unescaped(std::mem::take(&mut one))),
            Part::Text(text) if reading && !aloud => add(&mut one, text),
            _ => {}
        }
        true
    });
    strings
}

/// The kind of every cell format in the styles in XML.
fn xlsx_kinds(xml: &[u8]) -> Vec<Kind> {
    let (mut codes, mut formats, mut inside) = (HashMap::new(), vec![], false);
    let id = |e: &BytesStart| attr(e, "numFmtId").and_then(|id| id.parse::<u32>().ok()).unwrap_or(0);
    walk(xml, |part| {
        match part {
            Part::Open(e) => match e.local_name().as_ref() {
                "numFmt" if codes.len() < MOST_FORMATS => drop(codes.insert(id(e), kind(id(e), attr(e, "formatCode").as_deref()))),
                "cellXfs" => inside = true,
                "xf" if inside => formats.push(id(e)),
                _ => {}
            },
            Part::Close("cellXfs") => return false,
            _ => {}
        }
        formats.len() < MOST_FORMATS
    });
    kinds(&codes, &formats)
}

/// The rows of a sheet in XML.
fn xlsx_sheet(xml: &[u8], book: &Book, out: &mut String) {
    let (mut sort, mut format, mut value) = (String::new(), 0, String::new());
    let (mut reading, mut aloud) = (false, false);
    walk(xml, |part| {
        match part {
            Part::Open(e) => match e.local_name().as_ref() {
                "row" => line(out),
                "c" => {
                    sort = attr(e, "t").unwrap_or_default();
                    format = attr(e, "s").and_then(|s| s.parse().ok()).unwrap_or(0);
                    value.clear();
                }
                // The value of the cell, or the text of a string that is written in the cell.
                "v" | "t" => reading = true,
                "rPh" => aloud = true,
                _ => {}
            },
            Part::Close("v" | "t") => reading = false,
            Part::Close("rPh") => aloud = false,
            Part::Close("c") => {
                let shown = match sort.as_str() {
                    "s" => value.trim().parse().map(|at| book.string(at)).unwrap_or_default(),
                    "str" | "inlineStr" => unescaped(std::mem::take(&mut value)),
                    "b" => String::from(if value.trim() == "1" { "TRUE" } else { "FALSE" }),
                    "d" => value.replace('T', " "),
                    // What went wrong in a formula is nothing to search for.
                    "e" => String::new(),
                    _ => value.trim().parse().map(|v| book.number(v, format)).unwrap_or_else(|_| std::mem::take(&mut value)),
                };
                value.clear();
                cell(out, &shown);
            }
            Part::Text(text) if reading && !aloud => add(&mut value, text),
            _ => {}
        }
        out.len() <= MAX_TEXT
    });
}

/// Walks the records of a part of a `.xlsb`, until `each` says `false` or a record does not
/// fit in the part. A record is its number, its length and that many bytes; both numbers are
/// written seven bits to a byte, the eighth saying that another byte follows.
fn xlsb_records(part: &[u8], mut each: impl FnMut(u32, &[u8]) -> bool) {
    let packed = |at: &mut usize, most: usize| {
        let mut value = 0;
        for place in 0..most {
            let byte = *part.get(*at)?;
            *at += 1;
            value |= ((byte & 0x7F) as u32) << (7 * place);
            if byte & 0x80 == 0 {
                break;
            }
        }
        Some(value)
    };
    let mut at = 0;
    while let (Some(id), Some(length)) = (packed(&mut at, 2), packed(&mut at, 4))
        && let Some(data) = part.get(at..).and_then(|rest| rest.get(..length as usize))
        && each(id, data)
    {
        at += data.len();
    }
}

/// A string in a record of a `.xlsb`, and where it ends: the number of its characters in
/// four bytes, then the characters in UTF-16. A string that is not there has all bits set
/// for a number.
fn xlsb_string(data: &[u8], at: usize) -> Option<(String, usize)> {
    let count = le32(data, at)?;
    if count == u32::MAX {
        return Some((String::new(), at + 4));
    }
    let bytes = data.get(at + 4..)?.get(..(count as usize).checked_mul(2)?)?;
    Some((utf16(bytes), at + 4 + bytes.len()))
}

fn xlsb_sheets(workbook: &[u8]) -> (Vec<(String, String)>, bool) {
    let (mut sheets, mut from_1904) = (vec![], false);
    xlsb_records(workbook, |id, data| {
        match id {
            0x99 => from_1904 = data.first().is_some_and(|flags| flags & 1 != 0),
            // A sheet: how it is shown and its number, four bytes each, then the name of its
            // relation and its own.
            0x9C => {
                if let Some((relation, at)) = xlsb_string(data, 8)
                    && let Some((name, _)) = xlsb_string(data, at)
                {
                    sheets.push((name, relation));
                }
            }
            _ => {}
        }
        sheets.len() < MOST_SHEETS
    });
    (sheets, from_1904)
}

fn xlsb_strings(part: &[u8]) -> Vec<String> {
    let (mut strings, mut kept) = (vec![], 0);
    // A byte of flags comes before the string. One that cannot be read still takes its place.
    xlsb_records(part, |id, data| id != 0x13 || keep(&mut strings, &mut kept, xlsb_string(data, 1).unwrap_or_default().0));
    strings
}

fn xlsb_kinds(part: &[u8]) -> Vec<Kind> {
    let (mut codes, mut formats, mut inside) = (HashMap::new(), vec![], false);
    xlsb_records(part, |id, data| {
        match id {
            0x2C if codes.len() < MOST_FORMATS => {
                if let (Some(id), Some((code, _))) = (le16(data, 0), xlsb_string(data, 2)) {
                    codes.insert(id as u32, kind(id as u32, Some(&code)));
                }
            }
            // The formats of the cells lie between these two; those of the styles, which are
            // the same records, lie before them.
            0x269 => inside = true,
            0x26A => return false,
            0x2F if inside => formats.push(le16(data, 2).unwrap_or(0) as u32),
            _ => {}
        }
        formats.len() < MOST_FORMATS
    });
    kinds(&codes, &formats)
}

/// The rows of a sheet in records. A cell begins with its column in four bytes and its
/// format in three and a byte of flags; its value follows.
fn xlsb_sheet(part: &[u8], book: &Book, out: &mut String) {
    xlsb_records(part, |id, data| {
        let format = le32(data, 4).map_or(0, |f| f & 0xFF_FFFF) as usize;
        let shown = match id {
            0x00 => {
                line(out);
                None
            }
            0x02 => packed(data, 8).map(|v| book.number(v, format)),
            0x04 | 0x0A => data.get(8).map(|v| String::from(if *v != 0 { "TRUE" } else { "FALSE" })),
            0x05 | 0x09 => real(data, 8).map(|v| book.number(v, format)),
            0x06 | 0x08 => xlsb_string(data, 8).map(|s| s.0),
            0x07 => le32(data, 8).map(|at| book.string(at as usize)),
            0x3E => xlsb_string(data, 9).map(|s| s.0),
            // The rows end here; what follows is how the sheet is shown and printed.
            0x92 => return false,
            _ => None,
        };
        cell(out, &shown.unwrap_or_default());
        out.len() <= MAX_TEXT
    });
}

// ---------------------------------------------------------------- .ods

/// An OpenDocument spreadsheet: tables of rows of cells. A cell has the text it shows, and a
/// number or a date has its value beside it, which is read instead: the text follows the
/// language of the one who wrote it (1.840,00 and 14/03/26), the value does not.
fn ods(xml: &[u8]) -> String {
    let (mut out, mut value, mut text, mut note) = (String::new(), None, String::new(), String::new());
    let (mut paragraphs, mut noting, mut signed, mut linked) = (0usize, false, false, false);
    walk(xml, |part| {
        let written = if noting { &mut note } else { &mut text };
        match part {
            // A table named after another file, with a `#`, is a copy of the cells that a
            // formula reads from that file: the workbook never shows them.
            Part::Close("table") if linked => linked = false,
            _ if linked => {}
            Part::Open(e) => match e.local_name().as_ref() {
                "table" => match attr(e, "name").unwrap_or_default() {
                    name if name.starts_with('\'') && name.contains("'#") => linked = true,
                    name => alone(&mut out, &name),
                },
                "table-row" => line(&mut out),
                "table-cell" | "covered-table-cell" => {
                    value = ods_value(e);
                    text.clear();
                    note.clear();
                }
                // A note on the cell. Who wrote it and when is left out.
                "annotation" => noting = true,
                "creator" | "date" => signed = true,
                "p" | "h" => {
                    paragraphs += 1;
                    if !written.is_empty() {
                        written.push(' ');
                    }
                }
                "s" | "tab" | "line-break" => written.push(' '),
                _ => {}
            },
            Part::Close("table-cell" | "covered-table-cell") => {
                // OnlyOffice writes what went wrong in a formula as a string.
                if !ERRORS.contains(&text.trim()) {
                    cell(&mut out, value.as_deref().unwrap_or(&text));
                }
                cell(&mut out, &note);
                (value, noting, signed, paragraphs) = (None, false, false, 0);
                text.clear();
                note.clear();
            }
            Part::Close("annotation") => noting = false,
            Part::Close("creator" | "date") => signed = false,
            Part::Close("p" | "h") => paragraphs = paragraphs.saturating_sub(1),
            Part::Text(t) if paragraphs > 0 && !signed => add(written, t),
            _ => {}
        }
        out.len() <= MAX_TEXT
    });
    out
}

/// The value of a cell that is a number or a date, as it is read.
fn ods_value(cell: &BytesStart) -> Option<String> {
    let value = || attr(cell, "value")?.parse::<f64>().ok();
    // What went wrong in a formula is nothing to search for. LibreOffice says that it went
    // wrong in a type of its own, beside the one that the standard has.
    if cell.attributes().with_checks(false).map_while(Result::ok).any(|a| a.key.local_name().as_ref() == "value-type" && a.value == "error") {
        return Some(String::new());
    }
    match attr(cell, "value-type")?.as_str() {
        "float" | "currency" => Some(number(value()?)),
        "percentage" => Some(number(value()? * 100.0) + "%"),
        "date" => Some(attr(cell, "date-value")?.replace('T', " ")),
        "boolean" => Some(attr(cell, "boolean-value")?.to_uppercase()),
        _ => None,
    }
}

// ---------------------------------------------------------------- .xls

/// A workbook of Excel before 2007: records, each its number and its length in two bytes
/// and then that many bytes. First come those of the workbook, with the names of the sheets
/// and where each begins, the formats and the shared strings; then those of each sheet.
fn xls(file: &[u8]) -> Option<String> {
    let mut stream = workbook(file)?;
    // A workbook whose structure is protected is encrypted, with a password that Excel keeps
    // for the ones nobody gave. The record that says so lies among the first.
    let mut at = 0;
    while let (Some(id), Some(length)) = (le16(&stream, at), le16(&stream, at + 2))
        && id != 0x000A
    {
        if id == 0x002F {
            stream = unlocked(&stream, at + 4)?;
            break;
        }
        at += 4 + length as usize;
    }
    let record = |at: usize| {
        let rest = stream.get(at..)?;
        let data = rest.get(4..)?.get(..le16(rest, 2)? as usize)?;
        Some((le16(rest, 0)?, data, at + 4 + data.len()))
    };

    let (mut codes, mut formats, mut sheets, mut shared) = (HashMap::new(), vec![], vec![], vec![]);
    let (mut unicode, mut from_1904, mut at, mut last) = (true, false, 0, 0);
    while let Some((id, data, next)) = record(at) {
        match id {
            // Since 1997 text is Unicode; before, it was in the characters of the writer's country.
            0x0809 => unicode = le16(data, 0) != Some(0x0500),
            0x0022 => from_1904 = le16(data, 0) == Some(1),
            0x041E if codes.len() < MOST_FORMATS => {
                if let (Some(id), Some(code)) = (le16(data, 0), xls_string(data, 2, if unicode { 2 } else { 1 }, unicode)) {
                    codes.insert(id as u32, kind(id as u32, Some(&code)));
                }
            }
            0x00E0 if formats.len() < MOST_FORMATS => formats.push(le16(data, 2).unwrap_or(0) as u32),
            // A sheet: where it begins, how it is shown, what it is, and its name.
            0x0085 if sheets.len() < MOST_SHEETS => {
                sheets.push((le32(data, 0).unwrap_or(u32::MAX) as usize, data.get(5).copied(), xls_string(data, 6, 1, unicode).unwrap_or_default()));
            }
            // The shared strings run on over the records that follow them.
            0x00FC => shared.push(data),
            0x003C if last == 0x00FC => shared.push(data),
            0x000A => break,
            _ => {}
        }
        if id != 0x003C {
            last = id;
        }
        at = next;
    }
    let book = Book { strings: xls_strings(&shared), kinds: kinds(&codes, &formats), from_1904 };

    // The sheets of a workbook lie one after the other, so all of them are no longer than the
    // stream. A file that points every sheet at the same records is stopped by that.
    let (mut out, mut left) = (String::new(), stream.len());
    for (start, sort, name) in sheets {
        alone(&mut out, &name);
        // A chart or a macro has a sheet of its own, with no cells.
        if sort != Some(0) {
            continue;
        }
        let (mut at, mut depth, mut row, mut note) = (start, 0, None, 0);
        while let Some((id, data, next)) = record(at)
            && out.len() <= MAX_TEXT
            && left > 0
        {
            (at, left) = (next, left.saturating_sub(4 + data.len()));
            match id {
                // A chart in a sheet has records of its own, from a beginning to an end.
                0x0809 => depth += 1,
                0x000A if depth <= 1 => break,
                0x000A => depth -= 1,
                // The text of a note on a cell, or of a box that is drawn on the sheet: this
                // record says how long it is, the next one has it after a byte of flags.
                0x01B6 => note = le16(data, 10).unwrap_or(0) as usize,
                0x003C if note > 0 => {
                    let wide = data.first().is_some_and(|flags| flags & 1 != 0);
                    alone(&mut out, &xls_characters(data.get(1..).unwrap_or_default(), note, wide));
                    (note, row) = (0, None);
                }
                _ if depth == 1 => xls_cell(id, data, &book, unicode, &mut row, &mut out),
                _ => {}
            }
        }
    }
    Some(out)
}

/// The cell of a record, if it is one, added to its row. A cell begins with its row, its
/// column and its format, two bytes each; its value follows.
fn xls_cell(id: u16, data: &[u8], book: &Book, unicode: bool, row: &mut Option<u16>, out: &mut String) {
    let format = le16(data, 4).unwrap_or(0) as usize;
    let truth = |value: &u8| String::from(if *value != 0 { "TRUE" } else { "FALSE" });
    let shown = match id {
        0x00FD => le32(data, 6).map(|at| book.string(at as usize)),
        0x0204 | 0x00D6 => xls_string(data, 6, 2, unicode),
        0x0203 => real(data, 6).map(|v| book.number(v, format)),
        0x027E => packed(data, 6).map(|v| book.number(v, format)),
        0x0205 if data.get(7) == Some(&0) => data.get(6).map(truth),
        // What a formula gave: a number, or, where a number cannot be, the sort of value it is.
        // A string lies in the record that follows and takes the place of the formula.
        0x0006 if le16(data, 12) != Some(0xFFFF) => real(data, 6).map(|v| book.number(v, format)),
        0x0006 if data.get(6) == Some(&1) => data.get(8).map(truth),
        0x0207 => return cell(out, &xls_string(data, 0, 2, unicode).unwrap_or_default()),
        // Before 1997 a note on a cell lay in a record like a cell, without a format.
        0x001C if !unicode => xls_string(data, 4, 2, unicode),
        0x0006 | 0x00BD => None,
        _ => return,
    };
    if le16(data, 0) != *row {
        line(out);
        *row = le16(data, 0);
    }
    cell(out, &shown.unwrap_or_default());
    // Numbers side by side in a row: the first column, a format and a number for each column,
    // the last column.
    if id == 0x00BD {
        for at in (4..data.len().saturating_sub(7)).step_by(6) {
            if let (Some(format), Some(value)) = (le16(data, at), packed(data, at + 2)) {
                cell(out, &book.number(value, format as usize));
            }
        }
    }
}

/// So many characters of a `.xls`, or as many as there are. One byte to a character is the
/// first 256 characters of Unicode since 1997; before, it was the characters of the writer's
/// country, which for the west of Europe are nearly the same.
fn xls_characters(bytes: &[u8], count: usize, wide: bool) -> String {
    let bytes = &bytes[..bytes.len().min(count.saturating_mul(if wide { 2 } else { 1 }))];
    if wide { utf16(bytes) } else { bytes.iter().map(|b| *b as char).collect() }
}

/// A string in a record of a `.xls`: the number of its characters in `width` bytes, then,
/// since 1997, a byte of flags that says whether a character is one byte or two and what
/// else lies between the flags and the characters. What a record cuts off is left out.
fn xls_string(data: &[u8], at: usize, width: usize, unicode: bool) -> Option<String> {
    let count = if width == 1 { *data.get(at)? as usize } else { le16(data, at)? as usize };
    let (mut at, mut wide) = (at + width, false);
    if unicode {
        let flags = *data.get(at)?;
        wide = flags & 1 != 0;
        at += 1 + if flags & 8 != 0 { 2 } else { 0 } + if flags & 4 != 0 { 4 } else { 0 };
    }
    Some(xls_characters(data.get(at..)?, count, wide))
}

/// The shared strings of a `.xls`, from the records they run over. A string that a record
/// cuts goes on in the next after a byte that says anew whether a character is one byte or
/// two; what follows the characters (their formatting, how they are said aloud) goes on
/// without one.
fn xls_strings(records: &[&[u8]]) -> Vec<String> {
    let (mut strings, mut kept) = (vec![], 0);
    // The table begins with two counts of four bytes, which are not believed.
    let (mut record, mut at) = (0, 8);
    // The next bytes of a header, which no record cuts.
    let header = |record: &mut usize, at: &mut usize, width: usize| {
        if *at >= records.get(*record)?.len() {
            (*record, *at) = (*record + 1, 0);
        }
        let bytes = records.get(*record)?.get(*at..)?.get(..width)?;
        *at += width;
        Some(bytes.iter().rev().fold(0usize, |value, byte| value << 8 | *byte as usize))
    };
    while let Some(mut count) = header(&mut record, &mut at, 2) {
        let Some(flags) = header(&mut record, &mut at, 1) else { break };
        let Some(runs) = (if flags & 8 != 0 { header(&mut record, &mut at, 2) } else { Some(0) }) else { break };
        let Some(more) = (if flags & 4 != 0 { header(&mut record, &mut at, 4) } else { Some(0) }) else { break };

        let (mut text, mut wide) = (String::new(), flags & 1 != 0);
        while count > 0 {
            let Some(here) = records.get(record) else { break };
            let width = if wide { 2 } else { 1 };
            let fit = count.min(here.len().saturating_sub(at) / width);
            if fit == 0 {
                let Some(flags) = records.get(record + 1).and_then(|next| next.first()) else { break };
                (record, at, wide) = (record + 1, 1, flags & 1 != 0);
                continue;
            }
            text.push_str(&xls_characters(&here[at..], fit, wide));
            (at, count) = (at + fit * width, count - fit);
        }
        if count > 0 || !keep(&mut strings, &mut kept, text) {
            break;
        }
        let mut skip = runs * 4 + more;
        while skip > 0 {
            let Some(here) = records.get(record) else { break };
            let step = skip.min(here.len().saturating_sub(at));
            (at, skip) = (at + step, skip - step);
            if skip > 0 {
                (record, at) = (record + 1, 0);
            }
        }
    }
    strings
}

/// The stream with its records decrypted, when it is encrypted with RC4 and the password that
/// Excel uses when nobody gave one, `VelvetSweatshop`: a workbook whose structure is
/// protected is written that way, and opens everywhere without a question. `None` when the
/// password is another, or the cipher is another: that workbook is locked, and has no text
/// to read. `filepass` is where the record that says so begins, after its header.
///
/// The cipher is keyed from the password and the salt of the record, anew for every 1024
/// bytes of the stream. The headers of the records are in the clear, as are a few records
/// and the first four bytes of a sheet's entry in the list of sheets; the cipher counts on
/// over them all the same.
fn unlocked(stream: &[u8], filepass: usize) -> Option<Vec<u8>> {
    let header = stream.get(filepass..)?;
    if header.get(..6)? != [1, 0, 1, 0, 1, 0] {
        return None;
    }
    let (salt, verifier, hash) = (header.get(6..22)?, header.get(22..38)?, header.get(38..54)?);
    let password: Vec<u8> = "VelvetSweatshop".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let key = md5(&[&md5(&password)[..5], salt].concat().repeat(16));
    let block = |number: usize| Rc4::new(&md5(&[&key[..5], &(number as u32).to_le_bytes()].concat()));
    let mut first = block(0);
    let verifier: Vec<u8> = verifier.iter().chain(hash).map(|byte| byte ^ first.next()).collect();
    if md5(&verifier[..16])[..] != verifier[16..] {
        return None;
    }

    let (mut out, mut at, mut cipher) = (stream.to_vec(), 0, block(0));
    while let (Some(id), Some(length)) = (le16(&out, at), le16(&out, at + 2)) {
        let (from, to) = (at + 4, (at + 4 + length as usize).min(out.len()));
        let clear = matches!(id, 0x0809 | 0x002F | 0x00E1 | 0x0138 | 0x0194 | 0x0195 | 0x0196);
        for (position, byte) in out.iter_mut().enumerate().take(to).skip(at) {
            if position % 1024 == 0 {
                cipher = block(position / 1024);
            }
            let mask = cipher.next();
            if position >= from && !clear && !(id == 0x0085 && position < from + 4) {
                *byte ^= mask;
            }
        }
        at = to;
    }
    Some(out)
}

/// The stream cipher RC4: a permutation of the bytes, stirred by the key and then by itself.
struct Rc4 {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    fn new(key: &[u8]) -> Rc4 {
        let mut state: [u8; 256] = std::array::from_fn(|at| at as u8);
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, j as usize);
        }
        Rc4 { state, i: 0, j: 0 }
    }

    fn next(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[self.i as usize]);
        self.state.swap(self.i as usize, self.j as usize);
        self.state[self.state[self.i as usize].wrapping_add(self.state[self.j as usize]) as usize]
    }
}

/// The MD5 of a message, as RFC 1321 lays it out: it keys the cipher of a locked workbook,
/// and is no good for anything else any more.
fn md5(message: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    const SINES: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&(message.len() as u64).wrapping_mul(8).to_le_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in padded.chunks_exact(64) {
        let words: Vec<u32> = chunk.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (mix, word) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), 7 * i % 16),
            };
            let turned = a.wrapping_add(mix).wrapping_add(SINES[i]).wrapping_add(words[word]).rotate_left(SHIFTS[i / 16 * 4 + i % 4]);
            (a, d, c, b) = (d, c, b, b.wrapping_add(turned));
        }
        for (sum, part) in state.iter_mut().zip([a, b, c, d]) {
            *sum = sum.wrapping_add(part);
        }
    }
    let mut digest = [0u8; 16];
    for (place, word) in state.iter().enumerate() {
        digest[place * 4..place * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    digest
}

/// The stream with the workbook in a compound file: `Workbook`, or `Book` before 1997.
///
/// A compound file is a small file system. It is cut in sectors, a table says for each
/// sector which one follows it in its stream, and a directory names the streams with their
/// first sector and their length. A stream of less than 4096 bytes lies in small sectors
/// in a stream of its own, with a table of its own. Whatever the file says, nothing is read
/// that is not in it, and no stream is followed for longer than the file is.
fn workbook(file: &[u8]) -> Option<Vec<u8>> {
    /// The bytes of a stream: its pieces in the order of the table, `length` bytes at most.
    fn stream<'a>(first: u32, table: &[u32], length: usize, piece: impl Fn(u32) -> Option<&'a [u8]>) -> Vec<u8> {
        let (mut bytes, mut at) = (vec![], first);
        while bytes.len() < length
            && let Some(more) = piece(at).filter(|more| !more.is_empty())
        {
            bytes.extend_from_slice(more);
            at = table.get(at as usize).copied().unwrap_or(u32::MAX);
        }
        bytes.truncate(length);
        bytes
    }
    let numbers = |bytes: &[u8]| bytes.chunks_exact(4).map(|n| u32::from_le_bytes([n[0], n[1], n[2], n[3]])).collect::<Vec<_>>();

    let (shift, small_shift) = (le16(file, 30)?, le16(file, 32)?);
    if !file.starts_with(&COMPOUND) || !(9..=12).contains(&shift) || small_shift > shift {
        return None;
    }
    let (size, small) = (1usize << shift, 1usize << small_shift);
    // The first sector lies after the header, which is as long as a sector. The last may be
    // cut short.
    let sector = |at: u32| file.get((at as usize).checked_add(1)?.checked_mul(size)?..).map(|rest| &rest[..rest.len().min(size)]);

    // The sectors that hold the table: 109 of them are named in the header, the rest in
    // sectors of their own, each of which ends with the number of the next. No more of them
    // are read than would hold a table of all the sectors the file has room for.
    let most = file.len() / size / (size / 4) + 1;
    let mut holders: Vec<u32> = numbers(file.get(76..512)?);
    let mut next = le32(file, 68)?;
    while holders.len() < most
        && let Some(more) = sector(next).map(numbers)
        && let Some((following, named)) = more.split_last()
    {
        holders.extend_from_slice(named);
        next = *following;
    }
    holders.truncate(most);
    let table: Vec<u32> = holders.iter().filter_map(|at| sector(*at)).flat_map(numbers).collect();

    // Of the directory, 128 bytes name a stream: the name in UTF-16 and its length in bytes
    // with the zero that ends it, what it is (2 is a stream), its first sector and its length.
    // ponytail: the first stream of the name is taken, wherever it lies in the tree of the
    // directory. A workbook that lies inside another as an object could come first; follow
    // the tree from the root if that is ever seen.
    let directory = stream(le32(file, 48)?, &table, file.len(), sector);
    let name_of = |entry: &[u8]| utf16(&entry[..(le16(entry, 64).unwrap_or(0) as usize).saturating_sub(2).min(64)]);
    let named = |name: &str| directory.chunks_exact(128).find(|entry| entry[66] == 2 && name_of(entry).eq_ignore_ascii_case(name));
    let found = named("Workbook").or_else(|| named("Book"))?;
    let (first, length) = (le32(found, 116)?, (le32(found, 120)? as usize).min(file.len()));
    if length >= le32(file, 56)? as usize {
        return Some(stream(first, &table, length, sector));
    }
    // The first name in the directory is the root, and its stream is the one of the small sectors.
    let root = directory.get(..128)?;
    let smalls = stream(le32(root, 116)?, &table, (le32(root, 120)? as usize).min(file.len()), sector);
    let small_table = numbers(&stream(le32(file, 60)?, &table, file.len(), sector));
    Some(stream(first, &small_table, length, |at| smalls.get((at as usize).checked_mul(small)?..).map(|rest| &rest[..rest.len().min(small)])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::{folder, zip_file};
    use crate::extract::text_of;

    const RELATION: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    /// What every workbook of the tests says, whatever its format.
    const READ: &str = "Budget\nPost\tBeløb\nCafé München\t1840\t2026-03-14\t12.5%\nשלום\t你好世界\tTRUE\n\
        I  alt\t1852.5\t0.3\t07:45:10\nRejse & æøå\nÆrøskøbing\t3";

    /// A zip file with parts of text.
    fn zip_of(path: &Path, parts: &[(&str, &str)]) {
        zip_file(path, &parts.iter().map(|(name, part)| (*name, part.as_bytes())).collect::<Vec<_>>());
    }

    /// The relations of a file or of a workbook: the type and the part of each.
    fn relations_of(all: &[(&str, &str)]) -> String {
        let one = |(at, (kind, part)): (usize, &(&str, &str))| format!("<Relationship Id=\"rId{}\" Type=\"{RELATION}/{kind}\" Target=\"{part}\"/>", at + 1);
        format!("<Relationships>{}</Relationships>", all.iter().enumerate().map(one).collect::<String>())
    }

    /// The relations of the workbooks of the tests: two sheets, the strings and the styles.
    fn relations_of_book() -> String {
        let sheets = [("worksheet", "worksheets/sheet1.xml"), ("worksheet", "/xl/worksheets/sheet2.xml")];
        relations_of(&[sheets[0], sheets[1], ("sharedStrings", "sharedStrings.xml"), ("styles", "styles.xml")])
    }

    fn xlsx(path: &Path) {
        let workbook = "<workbook xmlns:r=\"r\"><workbookPr date1904=\"false\"/><sheets>\
            <sheet name=\"Budget\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Rejse &amp; æøå\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>";
        let strings = "<sst><si><t>Post</t></si><si><t>Beløb</t></si>\
            <si><r><t>Caf&#233; </t></r><r><t>München</t></r><rPh><t>kafe</t></rPh></si>\
            <si><t>שלום</t></si><si><t>你好世界</t></si><si><t>I_x000D__x000A_alt</t></si><si><t>Ærøskøbing</t></si></sst>";
        let styles = r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="dd\/mm\/yyyy"/><numFmt numFmtId="165" formatCode="hh:mm:ss"/></numFmts>
            <cellStyleXfs><xf numFmtId="14"/></cellStyleXfs>
            <cellXfs><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="10"/><xf numFmtId="165"/></cellXfs></styleSheet>"#;
        let budget = r#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>1840</v></c><c r="C2" s="1"><v>46095</v></c><c r="D2" s="2"><v>0.125</v></c></row>
            <row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3" t="s"><v>4</v></c><c r="C3" s="1"/><c r="D3" t="b"><v>1</v></c></row>
            <row r="4"><c r="A4" t="s"><v>5</v></c><c r="B4"><f>SUM(B2:B3)</f><v>1852.5</v></c><c r="C4"><f>0.1+0.2</f><v>0.30000000000000004</v></c>
                <c r="D4" t="e"><f>1/0</f><v>#DIV/0!</v></c><c r="E4" s="3"><v>0.3230324074</v></c></row>
            </sheetData></worksheet>"#;
        // Two cells in the last row of a sheet, in its last columns.
        let journey = r#"<worksheet><sheetData><row r="1048576">
            <c r="XFC1048576" t="inlineStr"><is><t>Ærøskøbing</t></is></c><c r="XFD1048576"><v>3</v></c></row></sheetData></worksheet>"#;
        let parts = [
            ("_rels/.rels", &*relations_of(&[("officeDocument", "xl/workbook.xml")])),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", &*relations_of_book()),
            ("xl/sharedStrings.xml", strings),
            ("xl/styles.xml", styles),
            ("xl/worksheets/sheet1.xml", budget),
            ("xl/worksheets/sheet2.xml", journey),
        ];
        zip_of(path, &parts);
    }

    /// A record of a `.xlsb`.
    fn record(id: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = if id < 0x80 { vec![id as u8] } else { vec![id as u8 | 0x80, (id >> 7) as u8] };
        let mut length = data.len();
        while length >= 0x80 {
            bytes.push(length as u8 | 0x80);
            length >>= 7;
        }
        bytes.push(length as u8);
        [&bytes[..], data].concat()
    }

    /// The characters of a text in UTF-16, the lower byte first.
    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A string as a `.xlsb` writes it.
    fn counted(text: &str) -> Vec<u8> {
        [&(text.encode_utf16().count() as u32).to_le_bytes()[..], &wide(text)].concat()
    }

    /// A cell of a `.xlsb`: its column, its format and its value.
    fn cell_of(id: u32, column: u32, format: u32, value: &[u8]) -> Vec<u8> {
        record(id, &[&column.to_le_bytes()[..], &format.to_le_bytes(), value].concat())
    }

    /// A whole number as Excel packs it in four bytes.
    fn whole(value: i32) -> [u8; 4] {
        ((value << 2) as u32 | 2).to_le_bytes()
    }

    fn xlsb(path: &Path) {
        let sheet = |id: u32, relation: &str, name: &str| record(0x9C, &[&[0u8; 4][..], &id.to_le_bytes(), &counted(relation), &counted(name)].concat());
        let sheets = [sheet(1, "rId1", "Budget"), sheet(2, "rId2", "Rejse & æøå")].concat();
        let workbook = [record(0x83, &[]), record(0x99, &[0; 8]), record(0x8F, &[]), sheets, record(0x90, &[]), record(0x84, &[])].concat();
        let string = |text: &str| record(0x13, &[&[0u8][..], &counted(text)].concat());
        let strings = ["Post", "Beløb", "Café München", "שלום", "你好世界", "I\r\nalt"].map(string).concat();
        let format = |id: u16, code: &str| record(0x2C, &[&id.to_le_bytes()[..], &counted(code)].concat());
        let look = |format: u16| record(0x2F, &[&[0u8, 0][..], &format.to_le_bytes(), &[0; 12]].concat());
        let styles = [
            [record(0x267, &[]), format(164, "dd\\/mm\\/yyyy"), format(165, "hh:mm:ss"), record(0x268, &[])].concat(),
            [record(0x272, &[]), look(14), record(0x273, &[])].concat(),
            [record(0x269, &[]), look(0), look(164), look(10), look(165), record(0x26A, &[])].concat(),
        ];
        let row = |at: u32| record(0x00, &[&at.to_le_bytes()[..], &[0; 13]].concat());
        let shared = |column: u32, at: u32| cell_of(0x07, column, 0, &at.to_le_bytes());
        let budget = [
            [record(0x81, &[]), record(0x91, &[])].concat(),
            [row(0), shared(0, 0), shared(1, 1)].concat(),
            [row(1), shared(0, 2), cell_of(0x02, 1, 0, &whole(1840)), cell_of(0x02, 2, 1, &whole(46095))].concat(),
            cell_of(0x05, 3, 2, &0.125f64.to_le_bytes()),
            [row(2), shared(0, 3), cell_of(0x06, 1, 0, &counted("你好世界")), cell_of(0x01, 2, 1, &[]), cell_of(0x04, 3, 0, &[1])].concat(),
            // What formulas gave: two numbers, with the formulas behind them, and a fault.
            [row(3), shared(0, 5), cell_of(0x09, 1, 0, &[&1852.5f64.to_le_bytes()[..], &[0; 8]].concat())].concat(),
            [cell_of(0x09, 2, 0, &(0.1f64 + 0.2).to_le_bytes()), cell_of(0x0B, 3, 0, &[7]), cell_of(0x05, 4, 3, &0.3230324074f64.to_le_bytes())].concat(),
            [record(0x92, &[]), cell_of(0x06, 0, 0, &counted("after the rows")), record(0x82, &[])].concat(),
        ];
        let town = cell_of(0x08, 16_382, 0, &[&counted("Ærøskøbing")[..], &[0; 8]].concat());
        let journey = [record(0x91, &[]), row(1_048_575), town, cell_of(0x02, 16_383, 0, &whole(3)), record(0x92, &[])].concat();
        let (whole, relations) = (relations_of(&[("officeDocument", "xl/workbook.bin")]), relations_of_book().replace(".xml", ".bin"));
        let parts: [(&str, &[u8]); 7] = [
            ("_rels/.rels", whole.as_bytes()),
            ("xl/workbook.bin", &workbook),
            ("xl/_rels/workbook.bin.rels", relations.as_bytes()),
            ("xl/sharedStrings.bin", &strings),
            ("xl/styles.bin", &styles.concat()),
            ("xl/worksheets/sheet1.bin", &budget.concat()),
            ("xl/worksheets/sheet2.bin", &journey),
        ];
        zip_file(path, &parts);
    }

    /// A cell of an `.ods`: what says its value, and the text it shows.
    fn shown(value: &str, text: &str) -> String {
        format!("<table:table-cell {value}><text:p>{text}</text:p></table:table-cell>")
    }

    /// A row of an `.ods` that is said so many times.
    fn times(repeated: u32, cells: &[String]) -> String {
        format!("<table:table-row table:number-rows-repeated=\"{repeated}\">{}</table:table-row>", cells.concat())
    }

    fn ods_file(path: &Path) {
        let text = |text: &str| shown("office:value-type=\"string\"", text);
        let number = |kind: &str, value: &str, text: &str| shown(&format!("office:value-type=\"{kind}\" office:value=\"{value}\""), text);
        let empty = |repeated: u32| format!("<table:table-cell table:number-columns-repeated=\"{repeated}\"/>");
        let budget = [
            times(1, &[text("Post"), text("Beløb")]),
            // What a cell shows is written in the language of its writer. Its value is not.
            times(1, &[
                text("Café<text:s/><text:span>Mün</text:span>chen"),
                number("currency", "1840", "kr. 1.840,00"),
                shown("office:value-type=\"date\" office:date-value=\"2026-03-14\"", "14/03/26"),
                number("percentage", "0.125", "12,5 %"),
            ]),
            times(1, &[text("שלום"), text("你好世界"), empty(16_000), shown("office:value-type=\"boolean\" office:boolean-value=\"true\"", "SAND")]),
            times(1, &[
                text("I<text:line-break/></text:p><text:p>alt"),
                number("float\" table:formula=\"of:=SUM([.B2:.B3])", "1852.5", "1.852,5"),
                number("float\" table:formula=\"of:=0.1+0.2", "0.30000000000000004", "0,3"),
                shown("table:formula=\"of:=1/0\" office:value-type=\"string\" calcext:value-type=\"error\"", "#DIV/0!"),
                shown("office:value-type=\"time\" office:time-value=\"PT07H45M10S\"", "07:45:10"),
            ]),
            // The rest of the sheet, empty: a million rows of sixteen thousand cells.
            times(1_048_570, &[empty(16_384)]),
        ];
        // A row that is said a million times, of two cells that are said eight thousand times.
        let town = shown("table:number-columns-repeated=\"8000\" office:value-type=\"string\"", "Ærøskøbing");
        let nights = shown("table:number-columns-repeated=\"8000\" office:value-type=\"float\" office:value=\"3\"", "3");
        let content = format!(
            "<office:document-content xmlns:office=\"o\" xmlns:table=\"t\" xmlns:text=\"x\" xmlns:number=\"n\" xmlns:calcext=\"c\">\
             <office:automatic-styles><number:date-style><number:text>no text of a cell</number:text></number:date-style></office:automatic-styles>\
             <office:body><office:spreadsheet><table:table table:name=\"Budget\">{}</table:table>\
             <table:table table:name=\"Rejse &amp; æøå\">{}</table:table></office:spreadsheet></office:body></office:document-content>",
            budget.concat(),
            times(1_048_576, &[town, nights])
        );
        zip_of(path, &[("mimetype", "application/vnd.oasis.opendocument.spreadsheet"), ("content.xml", &content)]);
    }

    /// A record of a `.xls`.
    fn biff(id: u16, data: &[u8]) -> Vec<u8> {
        [&id.to_le_bytes()[..], &(data.len() as u16).to_le_bytes(), data].concat()
    }

    /// A string as a `.xls` writes it since 1997, its length in `width` bytes: one byte to a
    /// character when all its characters allow it.
    fn marked(text: &str, width: usize) -> Vec<u8> {
        let narrow = text.chars().all(|c| (c as u32) < 256);
        let characters = if narrow { text.chars().map(|c| c as u8).collect() } else { wide(text) };
        [&(text.encode_utf16().count() as u16).to_le_bytes()[..width], &[!narrow as u8], &characters].concat()
    }

    /// A cell of a `.xls`: its row, its column, its format and its value.
    fn at(id: u16, row: u16, column: u16, format: u16, value: &[u8]) -> Vec<u8> {
        biff(id, &[&row.to_le_bytes()[..], &column.to_le_bytes(), &format.to_le_bytes(), value].concat())
    }

    /// The record that begins the workbook (5), a sheet (16) or a chart (32) of a `.xls`
    /// since 1997, and the one that ends it.
    fn begin(sort: u8) -> Vec<u8> {
        biff(0x0809, &[0, 6, sort, 0])
    }
    const END: [u8; 4] = [0x0A, 0, 0, 0];

    /// A sheet in the list of the sheets of a `.xls`: where it begins, what it is, its name.
    fn listed(start: usize, sort: u8, name: &str) -> Vec<u8> {
        biff(0x0085, &[&(start as u32).to_le_bytes()[..], &[0, sort], &marked(name, 1)].concat())
    }

    /// The stream of a workbook as Excel wrote it from 1997 to 2003.
    fn xls_stream() -> Vec<u8> {
        let shared = |row: u16, column: u16, at_: u32| at(0x00FD, row, column, 0, &at_.to_le_bytes());
        // Two numbers side by side, from the second column to the third: 1852.5 and 0.3.
        let side_by_side = [&[3u8, 0, 1, 0, 0, 0][..], &(185_250u32 << 2 | 3).to_le_bytes(), &[0, 0], &(30u32 << 2 | 3).to_le_bytes(), &[2, 0]].concat();
        let budget = [
            begin(16),
            [shared(0, 0, 0), shared(0, 1, 1)].concat(),
            [shared(1, 0, 2), at(0x027E, 1, 1, 0, &whole(1840)), at(0x027E, 1, 2, 1, &whole(46095)), at(0x0203, 1, 3, 2, &0.125f64.to_le_bytes())].concat(),
            [shared(2, 0, 3), at(0x0204, 2, 1, 0, &marked("你好世界", 2)), at(0x0201, 2, 2, 1, &[]), at(0x0205, 2, 3, 0, &[1, 0])].concat(),
            // A chart in the sheet, with a number of its own that is no cell.
            [begin(32), at(0x0203, 9, 9, 0, &99f64.to_le_bytes()), END.to_vec()].concat(),
            // A formula that gave a string, which follows it.
            [at(0x0006, 3, 0, 0, &[0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0]), biff(0x0207, &marked("I\r\nalt", 2))].concat(),
            [biff(0x00BD, &side_by_side), at(0x0205, 3, 3, 0, &[7, 1]), at(0x0203, 3, 4, 3, &0.3230324074f64.to_le_bytes())].concat(),
            END.to_vec(),
        ]
        .concat();
        let journey = [begin(16), at(0x0204, 65_535, 254, 0, &marked("Ærøskøbing", 2)), at(0x027E, 65_535, 255, 0, &whole(3)), END.to_vec()].concat();

        let format = |id: u16, code: &str| biff(0x041E, &[&id.to_le_bytes()[..], &marked(code, 2)].concat());
        let look = |format: u16| biff(0x00E0, &[&[0u8, 0][..], &format.to_le_bytes(), &[0; 16]].concat());
        // The shared strings, the third of them cut in two by the end of the record: its first
        // five characters have one byte each, the seven that follow have two. The fourth has
        // formatting and how it is said aloud behind it, four bytes each.
        let strings = [
            biff(0x00FC, &[&[5u8, 0, 0, 0, 4, 0, 0, 0][..], &marked("Post", 2), &marked("Beløb", 2), &[12, 0, 0], b"Caf\xe9 "].concat()),
            biff(0x003C, &[&[1u8][..], &wide("München"), &[4, 0, 13, 1, 0, 4, 0, 0, 0], &wide("שלום"), &[0; 4], b"said"].concat()),
        ];
        let workbook = |first: usize, second: usize| {
            [
                [begin(5), biff(0x0022, &[0, 0]), format(164, "dd\\/mm\\/yyyy"), format(165, "hh:mm:ss")].concat(),
                [look(0), look(164), look(10), look(165)].concat(),
                [listed(first, 0, "Budget"), listed(0, 2, "A chart"), listed(second, 0, "Rejse & æøå")].concat(),
                [strings.concat(), END.to_vec()].concat(),
            ]
            .concat()
        };
        let before = workbook(0, 0).len();
        [workbook(before, before + budget.len()), budget, journey].concat()
    }

    /// A compound file with one stream, which lies in small sectors when it is less than
    /// 4096 bytes, as in the files of Excel: the directory in the first sector, the table of
    /// the small sectors in the second, then the bytes, and last the table of the sectors.
    fn compound(name: &str, stream: &[u8]) -> Vec<u8> {
        const LAST: u32 = 0xFFFF_FFFE;
        let in_small = stream.len() < 4096;
        let sectors = stream.len().div_ceil(512).max(1) as u32;
        // A sector of the table has the numbers of 128 sectors, its own among them. The header
        // has room for the names of 109 of them.
        let tables = (2 + sectors).div_ceil(127);
        assert!(tables <= 109);
        let chain = |from: u32, count: u32| (0..count).map(move |at| if at + 1 == count { LAST } else { from + at + 1 });

        let mut header = vec![0xFFu8; 512];
        header[..76].fill(0);
        header[..8].copy_from_slice(&COMPOUND);
        for (at, value) in [(24, 0x3Eu16), (26, 3), (28, 0xFFFE), (30, 9), (32, 6)] {
            header[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        let named = (0..tables).map(|table| (76 + 4 * table as usize, 2 + sectors + table));
        for (at, value) in [(44, tables), (48, 0), (56, 4096), (60, 1), (64, 1), (68, LAST), (72, 0)].into_iter().chain(named) {
            header[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        let entry = |name: &str, sort: u8, first: u32, length: usize| {
            let mut entry = vec![0u8; 128];
            entry[..2 * name.len()].copy_from_slice(&wide(name));
            entry[64..66].copy_from_slice(&(2 * name.len() as u16 + 2).to_le_bytes());
            entry[66] = sort;
            entry[116..120].copy_from_slice(&first.to_le_bytes());
            entry[120..124].copy_from_slice(&(length as u32).to_le_bytes());
            entry
        };
        let mut directory = match in_small {
            true => [entry("Root Entry", 5, 2, stream.len().next_multiple_of(64)), entry(name, 2, 0, stream.len())].concat(),
            false => [entry("Root Entry", 5, LAST, 0), entry(name, 2, 2, stream.len())].concat(),
        };
        directory.resize(512, 0);
        let mut small_table: Vec<u32> = if in_small { chain(0, stream.len().div_ceil(64) as u32).collect() } else { vec![] };
        small_table.resize(128, u32::MAX);
        let mut table: Vec<u32> = [LAST, LAST].into_iter().chain(chain(2, sectors)).chain((0..tables).map(|_| 0xFFFF_FFFD)).collect();
        table.resize(tables as usize * 128, u32::MAX);

        let numbers = |table: &[u32]| table.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<u8>>();
        let mut bytes = stream.to_vec();
        bytes.resize(sectors as usize * 512, 0);
        [header, directory, numbers(&small_table), bytes, numbers(&table)].concat()
    }

    /// Bytes that look like chance and are the same every time.
    fn chance(count: usize, mut seed: u32) -> Vec<u8> {
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        (0..count).map(|_| next()).collect()
    }

    #[test]
    fn sheet_reads_names_rows_numbers_and_dates_of_every_format() {
        let d = folder("sheet-reads");
        xlsx(&d.join("budget.xlsx"));
        xlsb(&d.join("budget.xlsb"));
        ods_file(&d.join("budget.ods"));
        std::fs::write(d.join("budget.xls"), compound("Workbook", &xls_stream())).unwrap();
        for name in ["budget.xlsx", "budget.xlsb", "budget.ods"] {
            assert_eq!(text(&d.join(name), 1 << 20).as_deref(), Some(READ), "{name}");
        }
        // The chart of the `.xls` has a sheet of its own, which has a name and no cells.
        assert_eq!(text(&d.join("budget.xls"), 1 << 20), Some(READ.replace("Rejse", "A chart\nRejse")));

        // The name says nothing: a file is read as what it is.
        std::fs::copy(d.join("budget.xlsx"), d.join("macros.xlsm")).unwrap();
        std::fs::copy(d.join("budget.xlsx"), d.join("renamed.xls")).unwrap();
        assert_eq!(text(&d.join("macros.xlsm"), 1 << 20).as_deref(), Some(READ));
        assert_eq!(text(&d.join("renamed.xls"), 1 << 20).as_deref(), Some(READ));

        // As the store asks for it, and finds its words.
        let size = std::fs::metadata(d.join("budget.xls")).unwrap().len();
        let tidied = text_of(&d.join("budget.xls"), size, 1 << 20).unwrap();
        assert!(tidied.starts_with("Budget\nPost Beløb\nCafé München 1840 2026-03-14 12.5%\n") && tidied.contains("\nI alt 1852.5 0.3 07:45:10\n"), "{tidied}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_reads_a_long_workbook_of_the_years_before_1997_and_the_notes_on_cells() {
        let d = folder("sheet-old");
        // More than 4096 bytes lie in sectors of 512. The text is in the characters of the
        // west of Europe, one byte each, and no byte of flags comes before it.
        let begin_old = |sort: u8| biff(0x0809, &[0, 5, sort, 0, 0, 0, 0, 0]);
        let narrow = |text: &str| [&(text.chars().count() as u16).to_le_bytes()[..], &text.chars().map(|c| c as u8).collect::<Vec<_>>()].concat();
        let label = |row: u16, text: String| at(0x0204, row, 0, 0, &narrow(&text));
        let rows: Vec<u8> = (0..400).flat_map(|row| label(row, format!("Færge {row}"))).collect();
        let note = biff(0x001C, &[&[9u8, 0, 0, 0][..], &narrow("Husk årer")].concat());
        let list = biff(0x0085, &[&[32u8, 0, 0, 0, 0, 0, 5][..], b"\xc6r\xf8 1"].concat());
        let stream = [begin_old(5), list, END.to_vec(), begin_old(16), rows, note, END.to_vec()].concat();
        assert!(stream.len() > 4096 && stream[32..36] == [0x09, 0x08, 8, 0], "the sheet begins where the list says");
        std::fs::write(d.join("old.xls"), compound("Book", &stream)).unwrap();
        let read = text(&d.join("old.xls"), 1 << 20).unwrap();
        assert!(read.starts_with("Ærø 1\nFærge 0\nFærge 1\n") && read.ends_with("Færge 399\nHusk årer"), "{read}");

        // A workbook that is locked with a password has no text to read.
        let locked = [begin(5), biff(0x002F, &[0; 6]), listed(0, 0, "Secret"), END.to_vec()].concat();
        std::fs::write(d.join("locked.xls"), compound("Workbook", &locked)).unwrap();
        assert_eq!(text(&d.join("locked.xls"), 1 << 20), None);

        // One whose structure is protected is encrypted with the password that Excel keeps for
        // that, and is read: here one of more than a block of the cipher, with the sheet's
        // entry in the list of sheets, which is half in the clear, past the first block.
        let salt = [7u8; 16];
        let password: Vec<u8> = "VelvetSweatshop".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let key = md5(&[&md5(&password)[..5], &salt[..]].concat().repeat(16));
        let mut cipher = Rc4::new(&md5(&[&key[..5], &[0, 0, 0, 0]].concat()));
        let verifier = [9u8; 16];
        let sealed: Vec<u8> = verifier.iter().chain(&md5(&verifier)).map(|byte| byte ^ cipher.next()).collect();
        let filepass = biff(0x002F, &[&[1, 0, 1, 0, 1, 0][..], &salt, &sealed].concat());
        let formats: Vec<u8> = (0..60).flat_map(|_| biff(0x00E0, &[0; 20])).collect();
        let globals = |start: usize| [begin(5), filepass.clone(), formats.clone(), listed(start, 0, "Timeseddel"), END.to_vec()].concat();
        let rows = [at(0x0204, 0, 0, 0, &marked("Udfyld kun de gule felter", 2)), at(0x027E, 1, 0, 0, &whole(1840))].concat();
        let plain = [globals(globals(0).len()), begin(16), rows, END.to_vec()].concat();
        assert!(plain.len() > 1024 && globals(0).len() > 1024);
        // The cipher is its own inverse: unlocking the plain stream is locking it.
        let encrypted = unlocked(&plain, 12).unwrap();
        assert!(encrypted[..12] == plain[..12] && encrypted != plain);
        std::fs::write(d.join("protected.xls"), compound("Workbook", &encrypted)).unwrap();
        assert_eq!(text(&d.join("protected.xls"), 1 << 20).as_deref(), Some("Timeseddel\nUdfyld kun de gule felter\n1840"));
        let mut other = encrypted.clone();
        other[20] ^= 1;
        std::fs::write(d.join("other-password.xls"), compound("Workbook", &other)).unwrap();
        assert_eq!(text(&d.join("other-password.xls"), 1 << 20), None);
        let hex = |digest: [u8; 16]| digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!((hex(md5(&[b'a'; 56])), hex(md5(&[b'a'; 64]))), ("3b0c8ac703f828b04c6c197006d17218".into(), "014842d480b571495a4a0363793f7367".into()));

        // The notes of a workbook since 1997: a record that says how long the text is, and
        // the text in the next, here in a stream that is named in capitals.
        let note = [biff(0x01B6, &[&[0u8; 10][..], &[10, 0], &[0; 6]].concat()), biff(0x003C, b"\x00Husk visum"), biff(0x003C, &[0; 16])].concat();
        let noted = [begin(5), listed(28, 0, "Visa"), END.to_vec(), begin(16), at(0x027E, 0, 0, 0, &whole(4)), note, END.to_vec()].concat();
        std::fs::write(d.join("notes.xls"), compound("WORKBOOK", &noted)).unwrap();
        assert_eq!(text(&d.join("notes.xls"), 1 << 20).as_deref(), Some("Visa\n4\nHusk visum\n"));

        let notes = "<comments><authors><author>mwo</author></authors><commentList>\
            <comment ref=\"A1\"><text><r><t>Husk </t></r><r><t>visum</t></r></text><mc:AlternateContent><xdr:col>1</xdr:col></mc:AlternateContent></comment>\
            <comment ref=\"B2\"><text><t>Spørg Åse</t></text></comment></commentList></comments>";
        let workbook = "<workbook><sheets><sheet name=\"Visa\" r:id=\"rId1\"/></sheets></workbook>";
        zip_of(&d.join("notes.xlsx"), &[("xl/workbook.xml", workbook), ("xl/comments1.xml", notes)]);
        assert_eq!(text(&d.join("notes.xlsx"), 1 << 20).as_deref(), Some("Visa\nHusk visum\nSpørg Åse\n"));

        let text_of_note = [record(0x27B, &[0; 36]), record(0x27D, &[&[0u8][..], &counted("Husk visum")].concat()), record(0x27C, &[])].concat();
        let sheets = record(0x9C, &[&[0u8; 8][..], &counted("rId1"), &counted("Visa")].concat());
        let whole = relations_of(&[("officeDocument", "xl/workbook.bin")]);
        zip_file(&d.join("notes.xlsb"), &[("_rels/.rels", whole.as_bytes()), ("xl/workbook.bin", &sheets), ("xl/comments1.bin", &text_of_note)]);
        assert_eq!(text(&d.join("notes.xlsb"), 1 << 20).as_deref(), Some("Visa\nHusk visum\n"));

        let signed = "<dc:creator>mwo</dc:creator><dc:date>2026-09-29T00:00:00</dc:date>";
        let note = format!("<office:annotation>{signed}<text:p>Husk</text:p><text:p>visum</text:p></office:annotation>");
        let cells = [shown("", &format!("{note}</text:p><text:p>北京")), shown("office:value-type=\"float\" office:value=\"4\"", "4")];
        let table = format!("<table:table table:name=\"Visa\">{}</table:table>", times(1, &cells));
        let content = format!("<d xmlns:office=\"o\" xmlns:table=\"t\" xmlns:text=\"x\" xmlns:dc=\"d\">{table}</d>");
        zip_of(&d.join("notes.ods"), &[("content.xml", &content)]);
        assert_eq!(text(&d.join("notes.ods"), 1 << 20).as_deref(), Some("Visa\n北京\tHusk visum\t4"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_finds_the_parts_of_a_workbook_where_its_writer_put_them() {
        let d = folder("sheet-parts");
        // The workbook in a folder of its own choice, parts that are called by other capitals
        // than they have, and the backslashes of Windows in their names.
        let whole = relations_of(&[("officeDocument", "/Book/Workbook.xml")]);
        let relations = relations_of(&[("worksheet", "worksheets/sheet1.xml"), ("worksheet", "/book/Sheets/Two.xml"), ("sharedStrings", "Strings.xml")]);
        let sheet = |of: usize| format!("<worksheet><sheetData><row><c t=\"s\"><v>{of}</v></c><c><v>{of}</v></c></row></sheetData></worksheet>");
        let parts = [
            ("_rels\\.rels", whole.as_str()),
            ("book\\workbook.xml", "<workbook><sheets><sheet name=\"One\" r:id=\"rId1\"/><sheet name=\"Two\" r:id=\"rId2\"/></sheets></workbook>"),
            ("book\\_rels\\workbook.xml.rels", relations.as_str()),
            ("BOOK/STRINGS.XML", "<sst><si><t>Færge</t></si><si><t>Bro</t></si></sst>"),
            ("book\\worksheets\\sheet1.xml", &sheet(0)),
            ("book/sheets/two.xml", &sheet(1)),
        ];
        zip_of(&d.join("parts.xlsx"), &parts);
        assert_eq!(text(&d.join("parts.xlsx"), 1 << 20).as_deref(), Some("One\nFærge\t0\nTwo\nBro\t1"));

        // A workbook whose list of sheets cannot be read: the sheets, without their names.
        let parts = [("xl/workbook.xml", "<workbook/>"), ("xl/_rels/workbook.xml.rels", &*relations_of_book()), ("xl/worksheets/sheet1.xml", &sheet(7))];
        zip_of(&d.join("nameless.xlsx"), &parts);
        assert_eq!(text(&d.join("nameless.xlsx"), 1 << 20).as_deref(), Some("7\n"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_reads_a_web_page_or_text_that_is_named_as_a_workbook() {
        let d = folder("sheet-named");
        let table = "<table><tr><td>Færge</td><td>1840</td></tr><tr><td>Bro</td></tr></table>";
        let page = format!("\u{feff}<html><head><style>td {{ x }}</style></head><body>{table}</body></html>");
        std::fs::write(d.join("page.xls"), page).unwrap();
        assert_eq!(text_of(&d.join("page.xls"), 100, 1 << 20).as_deref(), Some("Færge\n1840\nBro"));
        std::fs::write(d.join("tabs.xls"), "Færge\t1840\nBro\t12\n").unwrap();
        assert_eq!(text_of(&d.join("tabs.xls"), 100, 1 << 20).as_deref(), Some("Færge 1840\nBro 12"));
        zip_of(&d.join("other.xlsx"), &[("word/document.xml", "<w:t>No workbook</w:t>")]);
        assert_eq!(text(&d.join("other.xlsx"), 1 << 20), None);

        // A page as programs write them: breaks and cells that are never closed, names in
        // capitals, headings and captions right before the table, a list.
        let page = "<HTML><BODY><h2>Kontoudtog marts</h2><table><caption>Bevægelser</caption><tr><th>Dato<th>Tekst<tr><TD>Bro<BR>Tunnel<td>1840\
            </table><ul><li>første<li>anden</ul><P>Slut</P><DIV>Efter</DIV></BODY></HTML>";
        std::fs::write(d.join("loose.xls"), page).unwrap();
        let read = text_of(&d.join("loose.xls"), 100, 1 << 20).unwrap();
        assert_eq!(read, "Kontoudtog marts\nBevægelser\nDato\nTekst\nBro\nTunnel\n1840\nførste\nanden\nSlut\nEfter");

        // The XML of Excel 2003: its settings and properties are no text, and a cell and its
        // note are apart.
        let xml = "<?xml version=\"1.0\"?><Workbook xmlns=\"urn:schemas-microsoft-com:office:spreadsheet\" xmlns:ss=\"urn:schemas-microsoft-com:office:spreadsheet\">\
            <DocumentProperties><Author>Michael</Author></DocumentProperties><ExcelWorkbook><WindowHeight>9000</WindowHeight><ProtectStructure>False</ProtectStructure></ExcelWorkbook>\
            <Styles><Style ss:ID=\"Default\"><Interior ss:Color=\"#0000ee\"/></Style></Styles><Names><NamedRange ss:Name=\"Post\"/></Names>\
            <Worksheet ss:Name=\"Regnskab\"><Table><Row><Cell><Data ss:Type=\"String\">Post</Data></Cell><Cell><Data ss:Type=\"Number\">1840.5</Data>\
            <Comment ss:Author=\"Michael\"><ss:Data>Michael:<B>Husk moms</B></ss:Data></Comment></Cell></Row></Table><WorksheetOptions><Selected/></WorksheetOptions></Worksheet></Workbook>";
        std::fs::write(d.join("2003.xls"), xml).unwrap();
        assert_eq!(text_of(&d.join("2003.xls"), 100, 1 << 20).as_deref(), Some("Post\n1840.5\nMichael:Husk moms"));

        // Text in UTF-16 with a mark at its start, in Windows-1252, and in UTF-8 but for a byte.
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Dato\tBeløb\r\n14-03-2026\t250".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(d.join("utf16.xls"), utf16).unwrap();
        assert_eq!(text_of(&d.join("utf16.xls"), 100, 1 << 20).as_deref(), Some("Dato Beløb\n14-03-2026 250"));
        std::fs::write(d.join("latin.xls"), b"Dato;Bel\xf8b\r\nF\xe6rge til \xc6r\xf8;\x93250\x94 \x80").unwrap();
        assert_eq!(text_of(&d.join("latin.xls"), 100, 1 << 20).as_deref(), Some("Dato;Beløb\nFærge til Ærø;“250” €"));
        std::fs::write(d.join("stray.xls"), ["Færge til Ærø\nBro".as_bytes(), &[0xFF], b" og"].concat()).unwrap();
        assert_eq!(text_of(&d.join("stray.xls"), 100, 1 << 20).as_deref(), Some("Færge til Ærø\nBro\u{fffd} og"));
        std::fs::write(d.join("binary.xls"), b"MZ\0\0\0\0program").unwrap();
        assert_eq!(text_of(&d.join("binary.xls"), 100, 1 << 20), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_leaves_out_of_an_ods_what_the_workbook_never_shows() {
        let d = folder("sheet-ods-hidden");
        // The cells of another file that a formula reads, which LibreOffice keeps in a table
        // named after the file; and what went wrong in a formula, which OnlyOffice writes as
        // a string.
        let cached = format!("<table:table table:name=\"'file:///tmp/made.ods'#Regnskab\"><table:table-source xlink:href=\"made.ods\"/>{}</table:table>", times(1, &[shown("", "Marstal")]));
        let shown_cells = [shown("office:value-type=\"string\"", "Brændstof"), shown("office:value-type=\"string\" office:string-value=\"#N/A\"", "#N/A"), shown("", "#REF!")];
        let own = format!("<table:table table:name=\"Ark\">{}</table:table>", times(1, &shown_cells));
        let content = format!("<d xmlns:office=\"o\" xmlns:table=\"t\" xmlns:text=\"x\" xmlns:xlink=\"l\">{cached}{own}</d>");
        zip_of(&d.join("linked.ods"), &[("content.xml", &content)]);
        assert_eq!(text(&d.join("linked.ods"), 1 << 20).as_deref(), Some("Ark\nBrændstof"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_writes_numbers_and_dates_as_they_are_read() {
        let numbers = [(1840.0, "1840"), (12.5, "12.5"), (0.1 + 0.2, "0.3"), (-4.2e-5, "-0.000042"), (0.07 * 100.0, "7")];
        for (value, read) in numbers.into_iter().chain([(1e21, "1e21"), (2.5e-12, "2.5e-12"), (0.0, "0"), (f64::NAN, ""), (f64::INFINITY, "")]) {
            assert_eq!(number(value), read);
        }
        let dates = [(46095.0, "2026-03-14"), (46294.3958333333, "2026-09-29 09:30:00"), (0.3230324074, "07:45:10"), (36526.0, "2000-01-01")];
        for (days, read) in dates.into_iter().chain([(1.0, "1900-01-01"), (59.0, "1900-02-28"), (61.0, "1900-03-01"), (2_958_465.0, "9999-12-31")]) {
            assert_eq!(date(days, false).as_deref(), Some(read));
        }
        assert_eq!(date(44633.0, true).as_deref(), Some("2026-03-14"), "counted from 1904");
        assert_eq!((date(-1.0, false), date(3e6, false), date(f64::NAN, false)), (None, None, None));

        let dates = ["dd\\/mm\\/yyyy", "[$-406]d. mmmm yyyy", "hh:mm:ss", "[Red]h:mm", "m/d/yy h:mm", "YYYY-MM-DD", "mm:ss.0"];
        let numbers = ["General", "0.00", "#,##0.00\\ \"kr. d.\"", "[Red]-#,##0", "0.00E+00", "_-* #,##0.00\\ [$kr.-406]_-", "@", "#,##0\\ _d", "#,##0.00 USD"];
        assert!(dates.iter().all(|code| kind(0, Some(code)) == Kind::Date) && numbers.iter().all(|code| kind(14, Some(code)) == Kind::Number));
        assert_eq!([kind(0, Some("0.0%")), kind(9, None), kind(14, None), kind(22, None)], [Kind::Percent, Kind::Percent, Kind::Date, Kind::Date]);
        assert_eq!([kind(0, None), kind(49, None), kind(0, Some("[$-406]0"))], [Kind::Number; 3]);
        assert_eq!([kind(0, Some("[hh]:mm:ss")), kind(0, Some("[m]"))], [Kind::Span; 2]);
        // A date shown as its week, and a time of minutes and seconds alone.
        assert_eq!([kind(0, Some("WW")), kind(0, Some("\"第\"WW\"週\"")), kind(0, Some("[$-412]M:S.00")), kind(0, Some("mm:ss"))], [Kind::Date; 4]);
        assert_eq!([kind(0, Some("0.00 \"m\"")), kind(0, Some("#,##0.00 [$MS-406]"))], [Kind::Number; 2]);
        let book = Book { kinds: vec![Kind::Span], ..Book::default() };
        assert_eq!((book.number(10.6320601852, 0), book.number(-0.25, 0)), ("255:10:10".into(), "-6:00:00".into()));

        let packed_of = |bits: u32| packed(&bits.to_le_bytes(), 0);
        assert_eq!([packed_of(1840 << 2 | 2), packed_of(185_250 << 2 | 3), packed_of((-3i32 << 2 | 2) as u32)], [Some(1840.0), Some(1852.5), Some(-3.0)]);
        assert_eq!(packed_of((12.5f64.to_bits() >> 32) as u32), Some(12.5));
        assert_eq!(unescaped("a_x000D__x000A_b_x41_x0041__xZZZZ__x".into()), "a\r\nb_x41A_xZZZZ__x");
    }

    #[test]
    fn sheet_survives_cut_off_changed_random_and_empty_files() {
        let d = folder("sheet-survives");
        xlsx(&d.join("whole.xlsx"));
        xlsb(&d.join("whole.xlsb"));
        ods_file(&d.join("whole.ods"));
        std::fs::write(d.join("whole.xls"), compound("Workbook", &xls_stream())).unwrap();
        let read = |name: &str, bytes: &[u8]| {
            std::fs::write(d.join(name), bytes).unwrap();
            text(&d.join(name), 1 << 20)
        };

        // Cut off at every length, and with every byte changed in turn, and with chance for a
        // tail, which a table or a count may point into. Whatever comes of it, it comes.
        let whole = std::fs::read(d.join("whole.xls")).unwrap();
        for at in 0..whole.len() {
            let mut changed = whole.clone();
            changed[at] = changed[at].wrapping_add(1 + at as u8 % 255);
            let _ = (xls(&whole[..at]), xls(&changed), xls(&[&whole[..at], &chance(whole.len() - at, at as u32)[..]].concat()));
        }
        // The same of the files that are zips, at every fifth byte: they are read from the disk.
        for name in ["whole.xlsx", "whole.xlsb", "whole.ods", "whole.xls"] {
            let whole = std::fs::read(d.join(name)).unwrap();
            for at in (0..whole.len()).step_by(5) {
                read("cut.xls", &whole[..at]);
                let mut changed = whole.clone();
                changed[at] = changed[at].wrapping_add(1 + at as u8 % 255);
                read("changed.xls", &changed);
                read("tail.xls", &[&whole[..at], &chance(whole.len() - at, at as u32)[..]].concat());
            }
        }

        // The readers of the parts, which in a file lie behind the zip: on bytes of real parts
        // and on bytes of chance, with some of them changed.
        let book = Book { strings: vec!["one".into(); 8], kinds: vec![Kind::Number, Kind::Date, Kind::Percent, Kind::Span], from_1904: false };
        let parts = [std::fs::read(d.join("whole.xlsb")).unwrap(), std::fs::read(d.join("whole.xlsx")).unwrap(), xls_stream(), chance(4096, 7)];
        for (part, seed) in parts.iter().flat_map(|part| (0..200).map(move |seed| (part, seed))) {
            let mut bytes = part.clone();
            for pair in chance(16, seed).chunks(2) {
                let at = pair[0] as usize * 251 % bytes.len();
                bytes[at] = pair[1];
            }
            let mut out = String::new();
            xlsb_sheet(&bytes, &book, &mut out);
            xlsx_sheet(&bytes, &book, &mut out);
            let _ = (xlsb_sheets(&bytes), xlsb_strings(&bytes), xlsb_kinds(&bytes), xlsx_sheets(&bytes), xlsx_strings(&bytes), xlsx_kinds(&bytes));
            let _ = (ods(&bytes), relations(&bytes, ""), xls_strings(&[&bytes[..bytes.len() / 2], &bytes[bytes.len() / 2..]]));
            let _ = (workbook(&bytes), xls(&compound("Workbook", &bytes)));
        }

        for seed in 0..200 {
            assert_eq!(read("chance.xlsx", &chance(3000, seed)), None);
            read("chance.xlsx", &[&b"PK\x03\x04"[..], &chance(3000, seed)].concat());
            read("chance.xls", &[&COMPOUND[..], &chance(3000, seed)].concat());
            // A header that is right, and chance for tables and directory.
            read("chance.xls", &[&compound("Workbook", b"")[..512], &chance(3000, seed)[..]].concat());
        }
        assert_eq!(read("empty.xlsx", b""), None);
        assert_eq!(read("empty.xls", &COMPOUND), None);
        assert_eq!(read("empty.ods", b"PK"), None);
        assert_eq!(text(&d.join("missing.xlsx"), 1 << 20), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sheet_stops_at_its_limits_whatever_a_file_claims() {
        let d = folder("sheet-limits");
        let started = std::time::Instant::now();
        let long = "Færgen sejler til Ærø ".repeat(40);
        let full = |read: &str| read.len() > MAX_TEXT && read.len() < MAX_TEXT + 2 * long.len();

        // More text than is kept: the reading stops, in every format.
        let rows = format!("<row><c t=\"inlineStr\"><is><t>{long}</t></is></c><c><v>1840</v></c></row>").repeat(6000);
        let mut out = String::new();
        xlsx_sheet(format!("<worksheet><sheetData>{rows}</sheetData></worksheet>").as_bytes(), &Book::default(), &mut out);
        assert!(full(&out), "{}", out.len());
        let rows = [record(0, &[0; 4]), cell_of(0x06, 0, 0, &counted(&long))].concat().repeat(6000);
        let mut out = String::new();
        xlsb_sheet(&rows, &Book::default(), &mut out);
        assert!(full(&out), "{}", out.len());
        let rows = times(1, &[shown("", &long)]).repeat(6000);
        assert!(full(&ods(format!("<d xmlns:table=\"t\" xmlns:text=\"x\"><table:table>{rows}</table:table></d>").as_bytes())));
        let rows: Vec<u8> = (0..6000).flat_map(|row| at(0x0204, row, 0, 0, &marked(&long, 2))).collect();
        let stream = [begin(5), listed(28, 0, "Long"), END.to_vec(), begin(16), rows, END.to_vec()].concat();
        assert!(stream[28..32] == [0x09, 0x08, 4, 0] && full(&xls(&compound("Workbook", &stream)).unwrap()));

        // A zip that unpacks to more than is unpacked of it is read as far as that, and no
        // further: of a large workbook the first rows are read, and the last are not.
        let row = format!("<row><c t=\"inlineStr\"><is><t>{long}</t></is></c></row>");
        let sheet = format!("<worksheet><sheetData>{}</sheetData></worksheet>", row.repeat(10));
        let workbook = "<workbook><sheets><sheet name=\"Long\" r:id=\"rId1\"/><sheet name=\"Next\" r:id=\"rId2\"/></sheets></workbook>";
        let relations = relations_of_book();
        let sheets = [("xl/worksheets/sheet1.xml", &*sheet), ("xl/worksheets/sheet2.xml", &*sheet)];
        zip_of(&d.join("large.xlsx"), &[("xl/workbook.xml", workbook), ("xl/_rels/workbook.xml.rels", &relations), sheets[0], sheets[1]]);
        assert_eq!(text(&d.join("large.xlsx"), 1 << 20).unwrap().lines().count(), 22);
        let zip = |left: usize| Zip { archive: zip::ZipArchive::new(std::fs::File::open(d.join("large.xlsx")).unwrap()).unwrap(), left: left as u64 };
        let read = excel(&mut zip(workbook.len() + relations.len() + 3 * row.len() + 60));
        assert_eq!(read, Some(format!("Long\n{long}\n{long}\n{long}\nNext\n")), "cut in the fourth row");
        let mut cut = zip("<worksheet><sheetData><row><c t=\"inlineStr\"><is><t>Fæ".len() - 1);
        assert_eq!(cut.part("xl/worksheets/sheet1.xml").unwrap().last(), Some(&b'F'), "cut between two characters, not in the æ");

        // A table of strings longer than the text that is kept is kept whole: the cells of the
        // first sheet may name its last strings. It ends at the size of a part, or at a count.
        assert!(6000 * long.len() > MAX_TEXT);
        assert_eq!(xlsx_strings(format!("<sst>{}</sst>", format!("<si><t>{long}</t></si>").repeat(6000)).as_bytes()).len(), 6000);
        assert_eq!(xlsb_strings(&record(0x13, &[&[0u8][..], &counted(&long)].concat()).repeat(6000)).len(), 6000);
        assert_eq!(xls_strings(&[&[&[0u8; 8][..], &marked(&long, 2).repeat(6000)].concat()]).len(), 6000);
        let (mut strings, mut bytes) = (vec![], MAX_ENTRY as usize - 1);
        assert_eq!((0..10).take_while(|_| keep(&mut strings, &mut bytes, "æ".into())).count(), 0);
        let (mut strings, mut bytes) = (vec![], 0);
        assert_eq!((0..MOST_STRINGS + 10).take_while(|_| keep(&mut strings, &mut bytes, String::new())).count(), MOST_STRINGS - 1);

        // One string, cell, name or note longer than all the text is kept as far as that, in
        // every format, and no more of the text is built than that and a line break.
        let one = xlsx_strings(format!("<sst><si>{}</si><si><t>next</t></si></sst>", format!("<t>{long}</t>").repeat(6000)).as_bytes());
        assert!(one.len() == 2 && one[0].len() == MAX_TEXT && one[1] == "next");
        let huge = "a".repeat(MAX_TEXT + 100);
        let mut out = String::new();
        let rows = format!("<worksheet><sheetData><row><c t=\"inlineStr\"><is><t>{huge}</t></is></c></row><row><c><v>1</v></c></row></sheetData></worksheet>");
        xlsx_sheet(rows.as_bytes(), &Book::default(), &mut out);
        assert_eq!(out.len(), MAX_TEXT + 1, "{}", out.len());
        let mut out = String::new();
        xlsb_sheet(&[cell_of(0x06, 0, 0, &counted(&huge)), record(0, &[0; 4]), cell_of(0x02, 0, 0, &whole(1))].concat(), &Book::default(), &mut out);
        assert_eq!(out.len(), MAX_TEXT + 1, "{}", out.len());
        let table = format!("<d xmlns:table=\"t\" xmlns:text=\"x\"><table:table table:name=\"{huge}\">{}</table:table></d>", times(1, &[shown("", &huge)]));
        assert_eq!(ods(table.as_bytes()).len(), MAX_TEXT + 1);
        assert_eq!(utf16(&wide(&huge)).len(), MAX_TEXT);
        // Bytes that are no UTF-8 are read up to the first of them, and never copied.
        let mut out = String::new();
        xlsx_sheet(&[&b"<worksheet><sheetData><row><c><v>1</v></c></row><row><c t=\"inlineStr\"><is><t>x"[..], &[0xFF; 100_000][..]].concat(), &Book::default(), &mut out);
        assert_eq!(out, "1\n");

        // A string that claims more characters than the file has, and a record that claims
        // more bytes.
        assert_eq!(xlsb_string(&[0xFF, 0xFF, 0xFF, 0x7F, b'a', 0], 0), None);
        assert_eq!(xls_string(&[0xFF, 0xFF, 1, b'a', 0, b'b'], 0, 2, true).as_deref(), Some("a"));
        let mut out = String::new();
        xlsb_sheet(&[0x06, 0xFF, 0xFF, 0xFF, 0x7F, 0, 0], &Book::default(), &mut out);
        assert_eq!(out, "");

        // More sheets than are read, none of which is in the file.
        let sheets: String = (0..10_000).map(|sheet| format!("<sheet name=\"Sheet {sheet}\" r:id=\"rId{sheet}\"/>")).collect();
        zip_of(&d.join("sheets.xlsx"), &[("xl/workbook.xml", &format!("<workbook><sheets>{sheets}</sheets></workbook>"))]);
        assert_eq!(text(&d.join("sheets.xlsx"), 1 << 20).unwrap().lines().count(), MOST_SHEETS);

        // A compound file whose table sends every sector to the same one, in a circle.
        let mut circle = compound("Workbook", &xls_stream().repeat(3));
        assert!(xls(&circle).is_some_and(|read| read.starts_with("Budget\nPost\tBeløb\n")));
        let table = circle.len() - 512;
        circle[table..].copy_from_slice(&2u32.to_le_bytes().repeat(128));
        assert!(xls(&circle).is_some_and(|read| read.starts_with("Budget\nPost\tBeløb\n")), "the first sector of the stream, over and over");
        // One that names the same sector of its table a hundred times, to have a table of
        // more sectors than the file has.
        let mut named = compound("Workbook", &xls_stream());
        let first = named[76..80].to_vec();
        named[44..48].copy_from_slice(&109u32.to_le_bytes());
        named[76..512].copy_from_slice(&first.repeat(109));
        assert!(xls(&named).is_some_and(|read| read.starts_with("Budget\nPost\tBeløb\n")));
        // And a workbook whose every sheet begins at the first record, and none of which ends.
        let stream = [begin(5), listed(0, 0, "Again").repeat(MOST_SHEETS), END.to_vec(), biff(0x0001, &[]).repeat(500_000)].concat();
        assert_eq!(xls(&compound("Workbook", &stream)).unwrap().lines().count(), MOST_SHEETS);

        // A debug build in a busy virtual machine, beside the other tests, has taken 40 seconds (NetBSD).
        assert!(started.elapsed() < std::time::Duration::from_secs(90), "{:?}", started.elapsed());
        let _ = std::fs::remove_dir_all(d);
    }
}

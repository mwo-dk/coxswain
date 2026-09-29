//! Jupyter notebooks (.ipynb) and draw.io diagrams (.drawio, .dio).
//!
//! A notebook is JSON: its cells in their order, each followed by what it printed when it
//! ran. It is read as it streams by and only the wanted text is kept, because a large
//! notebook is mostly pictures, and a hostile one is mostly brackets.
//!
//! A diagram is XML: the names of its pages and the labels of its shapes, which may be HTML.
//! A page is either XML as it is, or packed: base64 of deflated, percent-encoded XML.

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::{MAX_ENTRY, MAX_TEXT};

/// The most text kept of what one cell printed: the log of a long run must not push the cells after it out of the text.
const MAX_OUTPUT: usize = 16 * 1024;

pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut bytes = vec![];
    std::fs::File::open(path).ok()?.take(max.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max {
        return None;
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let text = match bytes.iter().find(|b| !b.is_ascii_whitespace())? {
        b'{' => notebook(bytes),
        b'<' => diagram(&String::from_utf8_lossy(bytes)),
        _ => return None,
    };
    // What a program printed holds backspaces and worse, and the store marks the words it found with two such characters.
    let text = text.replace(|c: char| c.is_control() && c != '\n', " ");
    (!text.trim().is_empty()).then_some(text)
}

/// Adds to `out` as much of `text` as keeps it within `most` bytes.
fn push(out: &mut String, text: &str, most: usize) {
    let mut room = most.saturating_sub(out.len()).min(text.len());
    while !text.is_char_boundary(room) {
        room -= 1;
    }
    out.push_str(&text[..room]);
}

/// Ends the line that `out` is in, if it is in one.
fn end_line(out: &mut String, most: usize) {
    if !out.ends_with('\n') {
        push(out, "\n", most);
    }
}

// ---------------------------------------------------------------- notebooks

/// What has no words and takes no room where the text is shown. First what programs print for a terminal: colours and cursor movements,
/// the choice of a character set as `tput sgr0` makes it, and the title of the window or the address of a link. So "\x1b[31mfire" is the
/// word "fire". Then a picture that is written into the text as a data URI: one of 3 MB would fill the text and push out every cell after it.
/// No part of the pattern looks past the next escape character or past the letters of the picture, so the time grows with the length only.
static UNSEEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b(\[[0-?]*[ -/]*[@-~]|[()*+].|\][^\x07\x1b]*(\x07|\x1b\\))|data:[A-Za-z0-9/.+;=-]*;base64,[A-Za-z0-9+/=]+").unwrap()
});

/// The text of a notebook. One that is cut off or broken gives its text up to that place.
fn notebook(json: &[u8]) -> String {
    let mut out = String::new();
    let _ = Walk { at: At::Book, out: &mut out, most: MAX_TEXT }.deserialize(&mut serde_json::Deserializer::from_slice(json));
    out
}

/// Where in the notebook the reader is, which decides what a key means there.
#[derive(Clone, Copy, PartialEq)]
enum At {
    /// The notebook itself, or one worksheet of an old notebook (format 3), which has the same "cells".
    Book,
    Cell,
    Output,
    /// The "data" of an output: the same result in several forms, of which only "text/plain" is text.
    Data,
    /// A string, or a list of strings that are the lines of one text, with their line ends or without.
    Text,
}

/// Reads one value of the notebook and adds the text in it to `out`, never past `most` bytes. Nothing else is kept.
/// How deep values may nest is for serde_json to say: it gives up at 128 levels.
struct Walk<'a> {
    at: At,
    out: &'a mut String,
    most: usize,
}

impl<'de> DeserializeSeed<'de> for Walk<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, from: D) -> Result<(), D::Error> {
        // Text is asked for as bytes, which serde_json hands over as they are. Asked for text, it refuses one half of a character pair
        // written as an escape, which JavaScript writes, and the reading would end there.
        if self.at == At::Text { from.deserialize_bytes(self) } else { from.deserialize_any(self) }
    }
}

impl<'de> Visitor<'de> for Walk<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("text, a list or an object")
    }

    /// Text where a list or an object belongs says nothing.
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_bytes<E>(self, text: &[u8]) -> Result<(), E> {
        push(self.out, &UNSEEN.replace_all(&String::from_utf8_lossy(text), ""), self.most);
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<(), A::Error> {
        while list.next_element_seed(Walk { at: self.at, out: &mut *self.out, most: self.most })?.is_some() {
            // Jupyter leaves the line end on each line of a text. Its second format and some other programs take it off.
            if self.at == At::Text {
                end_line(self.out, self.most);
            }
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        // Jupyter writes the keys of a cell in the order of the alphabet, so "outputs" comes before "source":
        // what a cell printed waits here until the cell has been read, and is kept when the file ends in the middle of it.
        let mut printed = String::new();
        let read = (|| {
            while let Some(key) = map.next_key::<String>()? {
                let at = match (self.at, key.as_str()) {
                    (At::Book, "worksheets") => At::Book,
                    (At::Book, "cells") => At::Cell,
                    // The text of a cell is its "source", in the third format the "input" of a code cell, in the first its "code" or "text".
                    (At::Cell, "source" | "input" | "code" | "text") => At::Text,
                    (At::Cell, "outputs") => At::Output,
                    (At::Output, "text" | "ename" | "evalue") => At::Text,
                    (At::Output, "data") => At::Data,
                    (At::Data, "text/plain") => At::Text,
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                        continue;
                    }
                };
                if at == At::Output {
                    map.next_value_seed(Walk { at, out: &mut printed, most: MAX_OUTPUT })?;
                } else {
                    map.next_value_seed(Walk { at, out: &mut *self.out, most: self.most })?;
                }
                if at == At::Text {
                    end_line(self.out, self.most);
                }
            }
            Ok(())
        })();
        if !printed.is_empty() {
            push(self.out, &printed, self.most);
            // What was printed may have been cut in the middle of a line, and the next cell must not go on in that line.
            end_line(self.out, self.most);
        }
        read
    }
}

// ---------------------------------------------------------------- diagrams

/// A tag or an entity of HTML. A tag ends at the next < or >, an entity at the first character that is none of its name:
/// no search looks past the place where the next one begins, so the time grows with the length only.
static MARKUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[A-Za-z][^<>]*>|&(#[0-9]+|#[xX][0-9A-Fa-f]+|[A-Za-z]+);").unwrap());
/// The tags that stand inside a word, as in <b>R</b>ocket: they are dropped.
const WITHIN: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "big", "cite", "code", "del", "dfn", "em", "font", "i", "ins", "kbd", "mark", "q", "s", "samp", "small", "span", "strike",
    "strong", "sub", "sup", "tt", "u", "var", "wbr",
];
/// The tags that end a line. Any other name between < and > is no tag of a label and is kept, as in List<String>.
const BREAKS: &[&str] = &[
    "blockquote", "br", "caption", "center", "dd", "div", "dl", "dt", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "img", "li", "ol", "p", "pre", "table", "tbody",
    "td", "tfoot", "th", "thead", "tr", "ul",
];

/// HTML as plain words, read as a browser reads it: what is no tag and no entity is text, a lone & or < too.
fn words(html: &str) -> String {
    // A comment says nothing to the reader of the label. Comments are taken out here, where the end of each is looked for once and
    // the looking ends with the first that has none. As a part of the pattern, every comment that was begun and never ended
    // sent the search to the end of the label, and a label of such comments took hours.
    let (mut kept, mut rest) = (String::with_capacity(html.len()), html);
    while let Some((before, comment)) = rest.split_once("<!--") {
        let Some((_, after)) = comment.split_once("-->") else { break };
        kept.push_str(before);
        rest = after;
    }
    kept.push_str(rest);

    // The pattern finds the markup and its name is read here: asked for the name too, the search takes three times as long, and a
    // page that unpacks to all it may is then read in seven seconds.
    let among = |tags: &[&str], tag: &str| tags.iter().any(|known| known.eq_ignore_ascii_case(tag));
    let (mut plain, mut from) = (String::with_capacity(kept.len()), 0);
    for found in MARKUP.find_iter(&kept) {
        plain.push_str(&kept[from..found.start()]);
        from = found.end();
        let markup = found.as_str();
        let tag = markup.trim_start_matches(['<', '/']).split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or_default();
        match markup.strip_prefix('&').map(|entity| character(entity.trim_end_matches(';'))) {
            Some(Some(character)) => plain.push(character),
            None if among(WITHIN, tag) => {}
            None if among(BREAKS, tag) => plain.push('\n'),
            // What is no tag of a label and no entity that is known stays as it is written.
            _ => plain.push_str(markup),
        }
    }
    plain.push_str(&kept[from..]);
    plain
}

/// The character an entity stands for: those a browser writes when it saves a label, and the numbered ones.
fn character(entity: &str) -> Option<char> {
    Some(match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16),
                None => number.parse(),
            };
            char::from_u32(code.ok()?)?
        }
    })
}

/// The text of a diagram. All its packed pages together may unpack to `MAX_ENTRY` bytes.
fn diagram(xml: &str) -> String {
    let (mut out, mut room) = (String::new(), MAX_ENTRY);
    labels(xml, &mut out, Some(&mut room));
    out
}

/// Adds the names of the pages and the labels of the shapes to `out`. `room` is how many bytes packed pages may still
/// unpack to; it is `None` inside a page that was packed, where nothing is unpacked again.
fn labels(xml: &str, out: &mut String, mut room: Option<&mut u64>) {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    // Whether a page has just begun: text that follows at once is the page, packed.
    let mut begun = false;
    // The label of an object waits for the mxCell inside the object, which has the style.
    let mut waiting = None;
    while out.len() < MAX_TEXT {
        let page = std::mem::take(&mut begun);
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                // What an attribute says, without the XML that it is kept in.
                let said = |name: &str| e.try_get_attribute(name).ok().flatten().map(|attribute| words(&attribute.value));
                let (label, object) = match e.local_name().as_ref() {
                    "diagram" => (said("name"), None),
                    "mxCell" => (said("value"), None),
                    "object" | "UserObject" => (None, said("label")),
                    _ => continue,
                };
                begun = e.local_name().as_ref() == "diagram";
                // draw.io shows a label as HTML when the style says one of these two. Any other label is text as it stands, and read
                // as HTML it would lose the Table of List<Table>.
                let style = e.try_get_attribute("style").ok().flatten();
                let html = style.is_some_and(|style| style.value.split(';').any(|part| part == "html=1" || part == "whiteSpace=wrap"));
                for label in std::mem::replace(&mut waiting, object).into_iter().chain(label) {
                    push(out, &if html { words(&label) } else { label }, MAX_TEXT);
                    push(out, "\n", MAX_TEXT);
                }
            }
            Ok(Event::Text(packed)) if page => {
                if let Some(room) = room.as_deref_mut() {
                    labels(&unpack(&packed.into_inner(), room), out, None);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // The mxCell never came, and the label is kept as it stands: that loses no word.
    push(out, &waiting.map(|label| label + "\n").unwrap_or_default(), MAX_TEXT);
}

/// A packed page as the XML it is, no more of it than `room` bytes, which shrinks by what was used.
/// A page that is broken gives what could be unpacked of it, and text that is no packed page gives nothing.
fn unpack(page: &str, room: &mut u64) -> String {
    let mut bytes = vec![];
    let _ = flate2::read::DeflateDecoder::new(&base64(page)[..]).take(*room).read_to_end(&mut bytes);
    *room = room.saturating_sub(bytes.len() as u64);

    // %C3%A6 is the two bytes of "æ". The bytes are moved down in place: a page may be large, and its text is never longer than what it is made from.
    let (mut to, mut from) = (0, 0);
    while from < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
        let (byte, step) = match (bytes[from], hex(from + 1), hex(from + 2)) {
            (b'%', Some(high), Some(low)) => ((high * 16 + low) as u8, 3),
            (byte, ..) => (byte, 1),
        };
        bytes[to] = byte;
        (to, from) = (to + 1, from + step);
    }
    bytes.truncate(to);
    String::from_utf8(bytes).unwrap_or_else(|broken| String::from_utf8_lossy(broken.as_bytes()).into_owned())
}

/// The bytes that base64 text stands for, up to the first character that is not of its alphabet. Blank space is passed over.
fn base64(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let six = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => break,
        };
        bits = bits << 6 | six as u32;
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push((bits >> held) as u8);
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::folder;
    use crate::extract::{text_of, tidy};
    use std::io::Write;

    /// A notebook as Jupyter writes it: the keys in the order of the alphabet, the text in lines.
    const NOTEBOOK: &str = r##"{
 "cells": [
  {"cell_type": "markdown", "metadata": {}, "source": [
    "# Flight 7\n", "\n", "Blåbærgrød på en ø, café, שלום, 火箭\n", "| Stage | Thrust |\n", "|---|---|\n", "| First | 7600 kN |"]},
  {"cell_type": "code", "execution_count": 1, "metadata": {}, "outputs": [
    {"name": "stdout", "output_type": "stream", "text": ["liftoff\n", "at 12:04\n"]},
    {"data": {"image/png": "iVBORw0KGgoAAAANSUhEUg==", "text/html": ["<b>markup</b>"], "text/plain": ["   stage  thrust\n", "0  first    7600"]},
     "execution_count": 1, "metadata": {}, "output_type": "execute_result"},
    {"ename": "ZeroDivisionError", "evalue": "division by zero", "output_type": "error", "traceback": ["\u001b[0;31mTraceback\u001b[0m"]}
   ], "source": "print('liftoff')"},
  {"attachments": {"plot.png": {"image/png": "iVBORw0KGgoAAAANSUhEUg=="}}, "cell_type": "raw", "metadata": {}, "source": ["The end"]}
 ],
 "metadata": {"kernelspec": {"display_name": "Python 3", "name": "python3"}, "title": "Not a cell"},
 "nbformat": 4,
 "nbformat_minor": 5
}"##;

    /// A diagram as draw.io writes it when it does not pack the pages.
    const DIAGRAM: &str = r##"<mxfile host="app.diagrams.net" version="24.7.17" pages="2">
  <diagram name="Første side" id="p1">
    <mxGraphModel dx="1422" dy="794" grid="1">
      <root>
        <mxCell id="0"/>
        <mxCell id="1" parent="0"/>
        <mxCell id="2" value="&lt;b&gt;Rocket&lt;/b&gt;&lt;br&gt;stage&amp;nbsp;1" style="rounded=1;html=1;" vertex="1" parent="1">
          <mxGeometry x="40" y="40" width="120" height="60" as="geometry"/>
        </mxCell>
        <mxCell id="3" value="&lt;div&gt;Blåbærgrød på en ø&lt;/div&gt;&lt;div&gt;caf&amp;#233; שלום 火箭&lt;/div&gt;"
                style="html=1;" vertex="1" parent="1"/>
        <mxCell id="4" value="&lt;table&gt;&lt;tr&gt;&lt;td&gt;Stage&lt;/td&gt;&lt;td&gt;Thrust&lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;"
                style="html=1;" vertex="1" parent="1"/>
        <mxCell id="5" style="edgeStyle=none;html=1;" edge="1" parent="1" source="2" target="3"/>
        <object label="Fuel &amp;amp; &lt;i&gt;fire&lt;/i&gt;" owner="Ann" id="6"><mxCell style="html=1;" vertex="1" parent="1"/></object>
        <UserObject label="Manual" link="https://example.com" id="7"><mxCell style="html=1;" vertex="1" parent="1"/></UserObject>
      </root>
    </mxGraphModel>
  </diagram>
  <diagram name="Page &amp; two" id="p2">
    <mxGraphModel><root>
      <mxCell id="0"/>
      <mxCell id="2" value="items: List&lt;String&gt; &amp; R&amp;D&#xa;second line" style="rounded=0;" vertex="1" parent="0"/>
    </root></mxGraphModel>
  </diagram>
</mxfile>"##;

    /// A page with one shape, "<b>Blåbær</b><br>café 火箭", packed by another program the way draw.io packs: by Python's urllib, zlib and base64.
    const PACKED: &str = "dVBBDsIgEHzN3CnEqkdKqycfQS1ak61tEE39vVuwNTF6IAwzs7PsQplu3Hs7tIe+cQRVQRnf9yGhbjSOCFJcGqgSUgo+kLs/qoyqeFi6u8RA5sRZRc3g\
                          PIGCYBT0qo5X7mcDhy6epcjPzNGeon/L6ajW2GTQxQR0BZ36iFt40rtrGzoepcxiVvyR88GNScwSNVjvruHHVAw+C+DH135e";

    /// The text of a file with this name and these bytes, as the search store gets it.
    fn read(name: &str, bytes: &[u8]) -> Option<String> {
        let d = folder(&format!("notebook-{name}"));
        std::fs::write(d.join(name), bytes).unwrap();
        let text = text_of(&d.join(name), bytes.len() as u64, 1 << 24);
        let _ = std::fs::remove_dir_all(d);
        text
    }

    /// Text with all but letters and digits percent-encoded, which is the first step of packing a page.
    fn escaped(text: &str) -> String {
        text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    }

    /// Bytes deflated and then written as base64, which is the rest of packing a page.
    fn deflated(bytes: &[u8]) -> String {
        let mut deflate = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
        deflate.write_all(bytes).unwrap();
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::new();
        for three in deflate.finish().unwrap().chunks(3) {
            let bits = three.iter().fold(0u32, |bits, b| bits << 8 | *b as u32) << (8 * (3 - three.len()));
            let sixes = (0..4).map(|n| alphabet[(bits >> (18 - 6 * n)) as usize & 63] as char);
            text.extend(sixes.take(three.len() + 1).chain(std::iter::repeat_n('=', 3 - three.len())));
        }
        text
    }

    /// Bytes that look random and are the same in every run.
    fn noise(length: usize, seed: u64) -> Vec<u8> {
        let mut bits = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let next = || {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            (bits >> 32) as u8
        };
        std::iter::repeat_with(next).take(length).collect()
    }

    #[test]
    fn notebook_gives_its_cells_in_order_each_followed_by_what_it_printed() {
        let text = read("flight.ipynb", NOTEBOOK.as_bytes()).unwrap();
        let cells = "# Flight 7\nBlåbærgrød på en ø, café, שלום, 火箭\n| Stage | Thrust |\n|---|---|\n| First | 7600 kN |\nprint('liftoff')\n";
        let printed = "liftoff\nat 12:04\nstage thrust\n0 first 7600\nZeroDivisionError\ndivision by zero\n";
        assert_eq!(text, format!("{cells}{printed}The end"), "no picture, no HTML of a result, no traceback, nothing about the notebook itself");

        let marked = format!("\u{feff}{NOTEBOOK}");
        assert_eq!(read("marked.IPYNB", marked.as_bytes()), Some(text), "a byte order mark is passed over");
    }

    #[test]
    fn old_notebook_gives_the_cells_of_every_worksheet() {
        let old = r##"{"metadata": {"name": "Old flight"}, "nbformat": 3, "nbformat_minor": 0, "worksheets": [
            {"cells": [
                {"cell_type": "heading", "level": 1, "metadata": {}, "source": ["Første ark"]},
                {"cell_type": "code", "collapsed": false, "input": ["print 2 + 2\n", "x = 'old'"], "language": "python", "metadata": {}, "outputs": [
                    {"output_type": "stream", "stream": "stdout", "text": ["4\n"]},
                    {"output_type": "pyout", "prompt_number": 1, "png": "iVBORw0KGgoAAAANSUhEUg==", "html": ["<b>markup</b>"], "text": ["'old result'"]},
                    {"ename": "NameError", "evalue": "name 'y' is not defined", "output_type": "pyerr", "traceback": ["Traceback"]}
                ], "prompt_number": 1}
            ], "metadata": {}},
            {"cells": [{"cell_type": "markdown", "metadata": {}, "source": "Second worksheet"}], "metadata": {}}
        ]}"##;
        let text = read("old.ipynb", old.as_bytes()).unwrap();
        assert_eq!(text, "Første ark\nprint 2 + 2\nx = 'old'\n4\n'old result'\nNameError\nname 'y' is not defined\nSecond worksheet");
    }

    #[test]
    fn notebook_keeps_a_bounded_part_of_what_a_cell_printed_and_none_of_its_colours() {
        let long = r#""epoch 1 loss 0.5\n","#.repeat(100_000);
        let colours = r#""\u001b[31mfire\u001b[0m and \u001b[1;32msmo\u001b[0mke\u0008\u0001\u0002 gone\r100%""#;
        let book = format!(
            r#"{{"cells": [
                {{"cell_type": "code", "outputs": [{{"output_type": "stream", "text": [{long} "done"]}}], "source": "train()"}},
                {{"cell_type": "code", "outputs": [{{"output_type": "stream", "text": {colours}}}], "source": "after the log"}}
            ]}}"#
        );
        let text = notebook(book.as_bytes());
        assert!(text.starts_with("train()\nepoch 1 loss 0.5\n"));
        assert!(text.len() < MAX_OUTPUT + 100, "{} bytes are kept of nearly 2 MB", text.len());
        assert!(text.contains("\nafter the log\nfire and smoke"), "the cell after the long one begins a line, and words are whole without their colours");

        let text = read("colours.ipynb", book.as_bytes()).unwrap();
        assert!(text.ends_with("\nafter the log\nfire and smoke gone 100%"), "no character is left that the store or a terminal gives a meaning");
    }

    #[test]
    fn diagram_gives_the_names_of_its_pages_and_its_labels_as_plain_words() {
        let first = "Første side\nRocket\nstage 1\nBlåbærgrød på en ø\ncafé שלום 火箭\nStage\nThrust\nFuel & fire\nManual\n";
        let second = "Page & two\nitems: List<String> & R&D\nsecond line";
        assert_eq!(read("rocket.drawio", DIAGRAM.as_bytes()), Some(format!("{first}{second}")));

        let bare = r#"<?xml version="1.0" encoding="UTF-8"?><mxGraphModel><root><mxCell id="2" value="No file around it" vertex="1"/></root></mxGraphModel>"#;
        assert_eq!(read("bare.dio", bare.as_bytes()).as_deref(), Some("No file around it"));
    }

    #[test]
    fn packed_diagram_gives_what_it_gives_unpacked() {
        let file = format!(r#"<mxfile><diagram id="p1" name="Packed page">{PACKED}</diagram><diagram name="Second">{PACKED}</diagram></mxfile>"#);
        assert_eq!(read("packed.drawio", file.as_bytes()).as_deref(), Some("Packed page\nBlåbær\ncafé 火箭\nSecond\nBlåbær\ncafé 火箭"));

        // The whole diagram, each page packed here, with the line breaks that some programs put around a packed page.
        let mut pages = DIAGRAM.split("<mxGraphModel").skip(1).map(|page| format!("<mxGraphModel{}", page.split("</diagram>").next().unwrap()));
        let file = format!(
            "<mxfile>\n<diagram name=\"Første side\">\n{}\n</diagram>\n<diagram name=\"Page &amp; two\">{}</diagram>\n</mxfile>",
            deflated(escaped(&pages.next().unwrap()).as_bytes()),
            deflated(escaped(&pages.next().unwrap()).as_bytes())
        );
        assert_eq!(read("packed.dio", file.as_bytes()), read("plain.drawio", DIAGRAM.as_bytes()));
    }

    #[test]
    fn label_is_read_as_a_browser_reads_it() {
        assert_eq!(words("<b>Rock</b>et <FONT color=\"#f00\">fu</FONT>el"), "Rocket fuel", "a tag inside a word does not part it");
        assert_eq!(words("<p>Hello</p><p>World</p>one<br>two<BR/>three<hr>"), "\nHello\n\nWorld\none\ntwo\nthree\n", "paragraphs are never joined");
        assert_eq!(words("<ul><li>one</li><li>two</li></ul>").split_whitespace().collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(words("caf&#233; &#xE9;&#Xe9; &lt;tag&gt; &amp;amp; a&nbsp;b &quot;q&quot; &apos;"), "café éé <tag> &amp; a b \"q\" '");
        let text = "R&D, a < b > c, x<y, 1 & 2; &made-up; &unknown; &#99999999999; &#xD800; &; Map<Key, Value> and <<use>>";
        assert_eq!(words(text), text, "what is no tag and no entity of HTML is text");
        assert_eq!(words("kept<!-- a note\n<b>in</b> it -->too <!-- never closed"), "kepttoo <!-- never closed");
        assert_eq!(words("<b never closed"), "<b never closed");
        assert_eq!(words(""), "");
    }

    #[test]
    fn label_is_html_only_when_the_style_of_its_shape_says_so() {
        let shape = |label: &str, style: &str| diagram(&format!(r#"<root><mxCell id="2" value="{label}" style="{style}" vertex="1"/></root>"#));
        assert_eq!(shape("+tables: List&lt;Table&gt; rows", "text;html=0;"), "+tables: List<Table> rows\n");
        assert_eq!(shape("+isBefore(): start&lt;b.start and end&gt;after", "text;align=left;"), "+isBefore(): start<b.start and end>after\n");
        assert_eq!(shape("&lt;&lt;table&gt;&gt; &lt;div&gt; a&amp;nbsp;b&#10;next", ""), "<<table>> <div> a&nbsp;b\nnext\n", "no word is taken for a tag");
        // The same members as draw.io writes them into a label that is HTML.
        assert_eq!(shape("+tables: List&amp;lt;Table&amp;gt; rows&lt;br&gt;+marks", "text;html=1;"), "+tables: List<Table> rows\n+marks\n");
        assert_eq!(shape("&lt;b&gt;R&lt;/b&gt;ocket", "rounded=1;whiteSpace=wrap;"), "Rocket\n", "draw.io shows a label that wraps as HTML");
        assert_eq!(shape("&lt;b&gt;R&lt;/b&gt;ocket", "xhtml=1;html=10;nowhiteSpace=wrap"), "<b>R</b>ocket\n");

        // The label of an object is written as the style of the mxCell inside it says.
        let object = |style: &str| diagram(&format!(r#"<root><object label="Map&lt;i, q&gt; of" id="2"><mxCell style="{style}" vertex="1"/></object></root>"#));
        assert_eq!(object("text;"), "Map<i, q> of\n");
        assert_eq!(object("html=1;"), "Map of\n");
        let lonely = r#"<UserObject label="&lt;i&gt;one&lt;/i&gt;"/><mxCell value="&lt;i&gt;two&lt;/i&gt;" style="html=1"/><object label="&lt;p&gt;end"/>"#;
        assert_eq!(diagram(lonely), "one\ntwo\n<p>end\n", "an object with no mxCell inside it keeps its label");
    }

    #[test]
    fn label_of_comments_begun_and_never_ended_is_read_as_fast_as_any_other() {
        // As draw.io keeps "<!--&a;" in a label that is HTML. Read with a pattern that looks for the end of each comment, a label of this
        // length took 25 seconds in the build that the tests run in, and one of twice the length four times as long. It takes a tenth
        // of a second now, a quarter when the machine is busy with the other tests: the limit is far from both.
        let limit = std::time::Duration::from_secs(5);
        let page = format!(r#"<mxGraphModel><root><mxCell id="2" value="{}" style="html=1;"/></root></mxGraphModel>"#, "&lt;!--&amp;a;".repeat(20_000));
        let start = std::time::Instant::now();
        let text = diagram(&page);
        assert!(start.elapsed() < limit, "it took {:?}", start.elapsed());
        assert_eq!(text, format!("{}\n", "<!--&a;".repeat(20_000)));

        let packed = format!(r#"<mxfile><diagram name="P">{}</diagram></mxfile>"#, deflated(escaped(&page).as_bytes()));
        let start = std::time::Instant::now();
        assert_eq!(diagram(&packed), format!("P\n{text}"));
        assert!(start.elapsed() < limit, "packed, it took {:?}", start.elapsed());
    }

    #[test]
    fn picture_written_into_a_cell_is_left_out_and_the_cells_after_it_are_read() {
        // 5 MB each, which is more than the text that is kept of a file.
        let picture = "iVBORw0KGgo+/".repeat(400_000);
        let book = format!(
            r##"{{"cells": [
                {{"cell_type": "markdown", "source": ["# Setup\n", "![setup.png](data:image/png;base64,{picture}==) and\n", "below"]}},
                {{"cell_type": "markdown", "source": "<img src=\"data:image/svg+xml;charset=utf-8;base64,{picture}\"> after"}},
                {{"cell_type": "code", "source": "address = 'data:image/png;base64,' + picture"}}
            ]}}"##
        );
        let text = "# Setup\n![setup.png]() and\nbelow\n<img src=\"\"> after\naddress = 'data:image/png;base64,' + picture\n";
        assert_eq!(notebook(book.as_bytes()), text, "the words data and base64 are words as any other");
    }

    #[test]
    fn lines_of_a_cell_are_never_joined() {
        // The second format of notebooks, and some programs to this day, write the lines without their line ends.
        let book = br#"{"nbformat": 2, "worksheets": [{"cells": [
            {"cell_type": "markdown", "source": ["budget", "", "Fuel"]},
            {"cell_type": "code", "input": ["a = 1", "b = 2"], "outputs": [
                {"output_type": "stream", "text": ["one", "two"]},
                {"output_type": "pyout", "text": ["with its\n", "line end"]}
            ]}
        ]}]}"#;
        assert_eq!(notebook(book), "budget\nFuel\na = 1\nb = 2\none\ntwo\nwith its\nline end\n");
    }

    #[test]
    fn first_format_of_notebooks_gives_its_code_and_its_text() {
        let book = br##"{"cells": [{"cell_type": "text", "text": "# Oldest"}, {"cell_type": "code", "code": "a = 10", "prompt_number": 1}], "nbformat": 1}"##;
        assert_eq!(notebook(book), "# Oldest\na = 10\n");
    }

    #[test]
    fn what_a_terminal_does_not_show_is_no_part_of_a_word() {
        let printed = |text: &str| notebook(format!(r#"{{"cells": [{{"outputs": [{{"output_type": "stream", "text": "{text}"}}]}}]}}"#).as_bytes());
        assert_eq!(printed(r"\u001b[1mbold\u001b(B\u001b[m word, as tput sgr0 ends it"), "bold word, as tput sgr0 ends it\n");
        assert_eq!(printed(r"\u001b)0\u001b*A\u001b+Bsets"), "sets\n");
        assert_eq!(printed(r"see \u001b]8;;https://example.com/a\u001b\\the li\u001b]8;;\u0007nk\u001b]0;title\u0007 here"), "see the link here\n");
        assert_eq!(printed(r"\u001b]8;;never ended \u001b[1mbold"), "\u{1b}]8;;never ended bold\n", "what is begun and never ended is text");
        assert_eq!(printed(r"cut off \u001b("), "cut off \u{1b}(\n");
    }

    #[test]
    fn half_a_character_pair_does_not_end_the_reading() {
        // JavaScript writes and reads one half of a pair as an escape, Python reads it.
        let pair = [r"\ud83d", r"\ude80"].concat();
        let book = format!(r#"{{"cells": [{{"outputs": [{{"text": ["rocket \ud83d"]}}], "source": "one \ud83d\ud83d \udc00 {pair}"}}, {{"source": "two"}}]}}"#);
        let text = notebook(book.as_bytes());
        let words: Vec<_> = text.split(|c: char| !c.is_alphanumeric() && c != '\u{1f680}').filter(|word| !word.is_empty()).collect();
        assert_eq!(words, ["one", "\u{1f680}", "rocket", "two"]);
    }

    #[test]
    fn packed_page_is_base64_of_deflated_percent_encoded_text() {
        assert_eq!(base64("SGVsbG8="), b"Hello");
        assert_eq!(base64("SGVs\nbG8gd2\r\n 9ybGQ"), b"Hello world", "line breaks are passed over, and the padding may be missing");
        assert_eq!(base64("+/+/"), [0xfb, 0xff, 0xbf]);
        assert_eq!(base64("SGVsbG8=IGFnYWlu"), b"Hello", "it ends where the padding begins");
        assert_eq!(base64("<mxGraphModel>"), b"");
        assert_eq!(base64(""), b"");

        let mut room = 1000;
        assert_eq!(unpack(&deflated(escaped("Blåbær 100% <b>").as_bytes()), &mut room), "Blåbær 100% <b>");
        assert_eq!(room, 1000 - "Bl%C3%A5b%C3%A6r%20100%25%20%3Cb%3E".len() as u64);
        assert_eq!(unpack(&deflated(b"100% %4 %zz %C3 %41%"), &mut room), "100% %4 %zz \u{fffd} A%", "what is no percent-encoding stays as it is");
        assert_eq!(unpack("not packed at all", &mut room), "");
        assert_eq!(unpack(&PACKED[..PACKED.len() / 2], &mut room).get(..20), Some("<mxGraphModel><root>"), "what could be unpacked of a page that is cut off");
    }

    #[test]
    fn broken_files_give_none_or_some_text_and_never_a_panic() {
        // These go to the reader itself: `text_of` would hide a panic.
        let d = folder("notebook-broken");
        let file = |name: &str, bytes: &[u8]| {
            std::fs::write(d.join(name), bytes).unwrap();
            text(&d.join(name), 1 << 20)
        };
        assert_eq!(file("empty.ipynb", b""), None);
        assert_eq!(file("empty.drawio", b""), None);
        assert_eq!(file("blank.ipynb", b" \n\t\n"), None);
        assert_eq!(file("no-cells.ipynb", b"{\"nbformat\": 4}"), None);
        assert_eq!(file("no-shapes.drawio", b"<mxfile><diagram name=\"\"><mxGraphModel/></diagram></mxfile>"), None);
        assert_eq!(file("notes.ipynb", b"Notes about the notebook"), None, "neither JSON nor XML");
        assert_eq!(text(&d.join("not-there.ipynb"), 1 << 20), None);
        assert_eq!(text(&d, 1 << 20), None, "a folder");
        std::fs::write(d.join("large.ipynb"), NOTEBOOK).unwrap();
        assert_eq!(text(&d.join("large.ipynb"), NOTEBOOK.len() as u64 - 1), None, "larger than allowed");
        assert!(text(&d.join("large.ipynb"), NOTEBOOK.len() as u64).is_some());
        assert!(text(&d.join("large.ipynb"), u64::MAX).is_some());
        for seed in 0..50 {
            let random = noise(3000, seed);
            assert_eq!(file("random.ipynb", &random), None);
            assert_eq!(file("random.drawio", &[b"{\"cells\":[", &random[..]].concat()), None);
            assert_eq!(file("random.dio", &[b"<mxfile><diagram>", &random[..]].concat()), None);
        }
        let _ = std::fs::remove_dir_all(d);

        // Cut off at every byte, also in the middle of a letter, a tag or an entity: what is read is what the whole file has.
        let whole = notebook(NOTEBOOK.as_bytes());
        for cut in 0..NOTEBOOK.len() {
            let text = notebook(&NOTEBOOK.as_bytes()[..cut]);
            assert!(text.lines().all(|line| whole.lines().any(|l| l == line)), "cut at {cut}: {text:?}");
        }
        let cut = NOTEBOOK.find("\"traceback\"").unwrap();
        let text = tidy(&notebook(&NOTEBOOK.as_bytes()[..cut]));
        assert!(
            text.ends_with("| First | 7600 kN |\nliftoff\nat 12:04\nstage thrust\n0 first 7600\nZeroDivisionError\ndivision by zero"),
            "what a cell printed is kept when the cell is cut off"
        );
        let whole = diagram(DIAGRAM);
        for cut in 0..DIAGRAM.len() {
            let text = diagram(&String::from_utf8_lossy(&DIAGRAM.as_bytes()[..cut]));
            // An object that is cut off from its mxCell gives its label as it stands: nothing says that it is HTML.
            assert!(whole.starts_with(text.trim_end_matches("Fuel &amp; <i>fire</i>\n")), "cut at {cut}: {text:?}");
        }
        for cut in 0..PACKED.len() {
            let text = diagram(&format!("<mxfile><diagram name=\"P\">{}", &PACKED[..cut]));
            assert!(text == "P\n" || text == "P\nBlåbær\ncafé 火箭\n", "cut at {cut}: {text:?}");
        }

        // Spoiled in three places, a thousand ways each.
        for seed in 0..1000 {
            let spoiled = |document: &str| {
                let mut bytes = document.as_bytes().to_vec();
                for (n, place) in noise(12, seed).chunks(4).enumerate() {
                    let at = u32::from_le_bytes([place[0], place[1], place[2], 0]) as usize % bytes.len();
                    match (seed as usize + n) % 3 {
                        0 => bytes[at] = place[3],
                        1 => bytes.insert(at, place[3]),
                        _ => drop(bytes.remove(at)),
                    }
                }
                String::from_utf8_lossy(&bytes).into_owned()
            };
            notebook(spoiled(NOTEBOOK).as_bytes());
            diagram(&spoiled(DIAGRAM));
            diagram(&format!("<mxfile><diagram>{}</diagram></mxfile>", spoiled(PACKED)));
            words(&String::from_utf8_lossy(&noise(200, seed)));
        }
    }

    #[test]
    fn nothing_is_nested_or_unpacked_or_kept_without_end() {
        // serde_json gives up at 128 levels, long before the stack does.
        let lists = r#"{"cells":[{"source":"#.to_string() + &"[".repeat(1_000_000);
        let objects = r#"{"cells":[{"metadata":"#.to_string() + &r#"{"a":"#.repeat(500_000);
        for deep in [lists, objects, r#"{"worksheets":["#.repeat(200_000)] {
            assert_eq!(notebook(deep.as_bytes()), "");
        }
        assert_eq!(diagram(&"<mxCell value=\"deep\">".repeat(3)), "deep\ndeep\ndeep\n", "elements in elements are read one after the other");
        assert_eq!(diagram(&"<object label=\"deep\"><a>".repeat(200_000)).len(), 200_000 * 5);

        // A page that is packed inside a packed page stays packed.
        let inner = format!("<mxfile><diagram name=\"Inner\">{PACKED}</diagram></mxfile>");
        assert_eq!(diagram(&format!("<mxfile><diagram name=\"Outer\">{}</diagram></mxfile>", deflated(escaped(&inner).as_bytes()))), "Outer\nInner\n");

        // A page unpacks to no more than there is room for, and the pages of a file share the room.
        let mut room = 58;
        assert_eq!(unpack(PACKED, &mut room), "<mxGraphModel><root><mxCell id=\"0\"/>");
        assert_eq!(room, 0);
        assert_eq!(unpack(PACKED, &mut room), "");
        let page = escaped(&format!("<mxGraphModel><root>{}</root></mxGraphModel>", "<mxCell value=\"a\"/>".repeat(100_000)));
        let file = format!("<mxfile>{}</mxfile>", format!("<diagram>{}</diagram>", deflated(page.as_bytes())).repeat(3));
        let (mut out, mut room) = (String::new(), page.len() as u64 * 3 / 2);
        labels(&file, &mut out, Some(&mut room));
        assert_eq!((room, out.len()), (0, 150_000 * 2), "the first page is read whole, the second to its middle, the third not at all");

        // No more text is kept than the store takes.
        let long = format!(r#"{{"cells":[{{"source":[{}"end"]}}]}}"#, r#""Blåbær og fløde\n","#.repeat(300_000));
        assert!((MAX_TEXT - 3..=MAX_TEXT).contains(&notebook(long.as_bytes()).len()));
        let long = format!("<mxfile><diagram name=\"Long\"><mxGraphModel>{}", "<mxCell value=\"Blåbær og fløde\"/>".repeat(300_000));
        assert!((MAX_TEXT - 3..=MAX_TEXT).contains(&diagram(&long).len()));
        let long = format!("<mxCell value=\"{}\"/>", "é".repeat(MAX_TEXT));
        assert_eq!(diagram(&long).len(), MAX_TEXT);
    }
}

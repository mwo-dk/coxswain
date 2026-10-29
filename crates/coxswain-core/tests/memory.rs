//! Peak memory and time of the readers and the archives on large and deeply nested inputs,
//! ignored by default. Linux only (peak RSS from `/proc`):
//!
//! ```sh
//! COXSWAIN_BENCH_DIR=… cargo test --release -p coxswain-core --test memory -- --ignored --nocapture
//! ```
//!
//! `memory_table` makes the inputs under `$COXSWAIN_BENCH_DIR/memory` (kept for the next run;
//! about 6 GB), then measures each case in a process of its own, so one case's memory is not
//! another's, and prints a Markdown table. That process gets a home of its own in the same
//! folder: a peek is made in its cache, never the user's. The 2 GB compressed tars and the 7z
//! need `gzip`, `xz`, `zstd` and `7z` installed; a case whose tool is missing is left out.
//! `COXSWAIN_BENCH_ONLY=pdf,tar` measures only the cases whose names hold one of those words;
//! `COXSWAIN_BENCH_INPUTS=games-2m-flat.7z` makes only the inputs named (the others' cases are
//! left out unless made before).
//!
//! Readers are given no size limit (`max` = `u64::MAX`): what is measured is the reader, not
//! `[search] text_max_size`, which today keeps files over 20 MB out.

// `w.word().to_string()` in a format: each word borrows the generator, so it is copied first.
#![allow(clippy::to_string_in_format_args)]

use coxswain_core::{archive, extract, meaning};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const MB: u64 = 1 << 20;

// ---------------------------------------------------------------- made-up words

/// Words that look like a language's: a few thousand of them, two to ten letters, drawn by a
/// fixed sequence, so the text packs as text does (gzip to about a third), not as zeros.
struct Words {
    state: u64,
    vocab: Vec<String>,
}

impl Words {
    fn new(seed: u64) -> Words {
        let mut w = Words {
            state: seed | 1,
            vocab: vec![],
        };
        let letters = b"etaoinshrdlucmfwypvbgkqjxz";
        for _ in 0..4000 {
            let len = 2 + (w.next() % 9) as usize;
            // Common letters more often, as in English.
            let word: String = (0..len)
                .map(|_| letters[((w.next() % 26) * (w.next() % 26) / 26) as usize] as char)
                .collect();
            w.vocab.push(word);
        }
        w
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn word(&mut self) -> &str {
        // Zipf-ish: low indices far more often.
        let r = self.next() % 4000;
        let i = (r * r / 4000) as usize;
        &self.vocab[i]
    }

    /// A line of about `n` words.
    fn line(&mut self, n: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            if i > 0 {
                s.push(' ');
            }
            let w = self.word().to_string();
            s.push_str(&w);
        }
        s
    }
}

/// `bytes` of lines of text, written as made.
fn write_text(
    to: &mut impl Write,
    bytes: u64,
    seed: u64,
    line: impl Fn(&mut Words, u64) -> String,
) -> io::Result<()> {
    let mut w = Words::new(seed);
    let (mut n, mut i) = (0u64, 0u64);
    while n < bytes {
        let mut l = line(&mut w, i);
        l.push('\n');
        to.write_all(l.as_bytes())?;
        n += l.len() as u64;
        i += 1;
    }
    Ok(())
}

/// Bytes no packer can make smaller: a picture's or a packed stream's.
fn noise(bytes: usize, seed: u64) -> Vec<u8> {
    let mut w = Words {
        state: seed | 1,
        vocab: vec![],
    };
    let mut v = Vec::with_capacity(bytes + 8);
    while v.len() < bytes {
        v.extend_from_slice(&w.next().to_le_bytes());
    }
    v.truncate(bytes);
    v
}

/// An XML part of about `bytes`: `wrap(text)` per paragraph of 40 words, between `head` and
/// `tail`.
fn xml_part(
    bytes: u64,
    seed: u64,
    head: &str,
    tail: &str,
    wrap: impl Fn(&str) -> String,
) -> Vec<u8> {
    let mut out = head.as_bytes().to_vec();
    let mut w = Words::new(seed);
    while (out.len() as u64) < bytes {
        out.extend_from_slice(wrap(&w.line(40)).as_bytes());
    }
    out.extend_from_slice(tail.as_bytes());
    out
}

fn zip_of(path: &Path, parts: &[(&str, &[u8])]) -> io::Result<()> {
    let mut z = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(path)?));
    for (name, bytes) in parts {
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(if *name == "mimetype" {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            })
            .large_file(bytes.len() as u64 > u32::MAX as u64 / 2);
        z.start_file(*name, o).map_err(io::Error::other)?;
        z.write_all(bytes)?;
    }
    z.finish().map_err(io::Error::other)?;
    Ok(())
}

// ---------------------------------------------------------------- the inputs

fn dir() -> PathBuf {
    let d = std::env::var_os("COXSWAIN_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.worktrees/bench-scratch")
        })
        .join("memory");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `name` in the folder, made by `make` (into a part file, renamed when done) unless it is there.
fn input(name: &str, make: impl FnOnce(&Path) -> io::Result<()>) -> Option<PathBuf> {
    let at = dir().join(name);
    if at.exists() {
        return Some(at);
    }
    // `COXSWAIN_BENCH_INPUTS=games-2m-flat.7z,…`: only those are made.
    if std::env::var("COXSWAIN_BENCH_INPUTS").is_ok_and(|only| !only.split(',').any(|o| o == name)) {
        return None;
    }
    let part = dir().join(format!("{name}.part"));
    let _ = std::fs::remove_file(&part);
    let t = Instant::now();
    match make(&part).and_then(|()| std::fs::rename(&part, &at)) {
        Ok(()) => {
            eprintln!("made {name} in {:.0} s", t.elapsed().as_secs_f64());
            Some(at)
        }
        Err(e) => {
            eprintln!("{name} left out: {e}");
            let _ = std::fs::remove_file(&part);
            None
        }
    }
}

fn run(cmd: &mut Command) -> io::Result<()> {
    let s = cmd.status()?;
    s.success()
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("{cmd:?}: {s}")))
}

/// A PDF of `pages` pages, each with `text` bytes of words in a packed content stream and,
/// when `picture` is not 0, a grey picture of that many bytes that does not pack.
fn pdf(to: &Path, pages: usize, text: usize, picture: usize) -> io::Result<()> {
    let mut f = Counted(BufWriter::new(std::fs::File::create(to)?), 0);
    let mut offsets = vec![0u64; 4 + pages * 3];
    let obj = |f: &mut Counted, n: usize, body: &[u8], offsets: &mut Vec<u64>| -> io::Result<()> {
        offsets[n] = f.1;
        f.write_all(format!("{n} 0 obj\n").as_bytes())?;
        f.write_all(body)?;
        f.write_all(b"\nendobj\n")
    };
    f.write_all(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")?;
    let page = |i: usize| 4 + i * 3;
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", page(i))).collect();
    obj(
        &mut f,
        1,
        b"<< /Type /Catalog /Pages 2 0 R >>",
        &mut offsets,
    )?;
    obj(
        &mut f,
        2,
        format!("<< /Type /Pages /Count {pages} /Kids [{kids}] >>").as_bytes(),
        &mut offsets,
    )?;
    obj(
        &mut f,
        3,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        &mut offsets,
    )?;
    let mut w = Words::new(7);
    let side = (picture as f64).sqrt() as usize;
    for i in 0..pages {
        let mut content = String::from("BT /F1 10 Tf 50 780 Td 12 TL\n");
        while content.len() < text {
            content.push_str(&format!("({}) '\n", w.line(12)));
        }
        content.push_str("ET\n");
        if side > 0 {
            content.push_str(&format!("q 200 0 0 200 300 50 cm /Im{i} Do Q\n"));
        }
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::fast());
        z.write_all(content.as_bytes())?;
        let packed = z.finish()?;
        let res = if side > 0 {
            format!(
                "<< /Font << /F1 3 0 R >> /XObject << /Im{i} {} 0 R >> >>",
                page(i) + 2
            )
        } else {
            "<< /Font << /F1 3 0 R >> >>".to_string()
        };
        obj(
            &mut f,
            page(i),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources {res} /Contents {} 0 R >>", page(i) + 1).as_bytes(),
            &mut offsets,
        )?;
        let mut stream = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            packed.len()
        )
        .into_bytes();
        stream.extend_from_slice(&packed);
        stream.extend_from_slice(b"\nendstream");
        obj(&mut f, page(i) + 1, &stream, &mut offsets)?;
        if side > 0 {
            let px = noise(side * side, i as u64 + 1);
            let mut stream = format!(
                "<< /Type /XObject /Subtype /Image /Width {side} /Height {side} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                px.len()
            )
            .into_bytes();
            stream.extend_from_slice(&px);
            stream.extend_from_slice(b"\nendstream");
            obj(&mut f, page(i) + 2, &stream, &mut offsets)?;
        } else {
            obj(&mut f, page(i) + 2, b"null", &mut offsets)?;
        }
    }
    let xref = f.1;
    let n = offsets.len();
    f.write_all(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes())?;
    for o in &offsets[1..] {
        f.write_all(format!("{o:010} 00000 n \n").as_bytes())?;
    }
    f.write_all(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    )?;
    f.0.flush()
}

struct Counted(BufWriter<std::fs::File>, u64);

impl Write for Counted {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.0.write(b)?;
        self.1 += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

const PART: u64 = 60 * MB;

fn docx(to: &Path, part: u64) -> io::Result<()> {
    let body = xml_part(
        part,
        11,
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
        "</w:body></w:document>",
        |t| format!("<w:p><w:r><w:t>{t}</w:t></w:r></w:p>"),
    );
    zip_of(to, &[
        ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("word/document.xml", &body),
    ])
}

fn xlsx(to: &Path) -> io::Result<()> {
    let strings = 200_000;
    let mut w = Words::new(13);
    let mut ss = format!(
        r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{strings}" uniqueCount="{strings}">"#
    );
    for _ in 0..strings {
        ss.push_str(&format!("<si><t>{}</t></si>", w.line(6)));
    }
    ss.push_str("</sst>");
    let mut sheet = String::from(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    let mut r = 1;
    while (sheet.len() as u64) < PART {
        sheet.push_str(&format!(r#"<row r="{r}">"#));
        for c in 0..8u8 {
            let col = (b'A' + c) as char;
            if c % 2 == 0 {
                sheet.push_str(&format!(
                    r#"<c r="{col}{r}" t="s"><v>{}</v></c>"#,
                    w.next() % strings
                ));
            } else {
                sheet.push_str(&format!(
                    r#"<c r="{col}{r}"><v>{}</v></c>"#,
                    w.next() % 100_000
                ));
            }
        }
        sheet.push_str("</row>");
        r += 1;
    }
    sheet.push_str("</sheetData></worksheet>");
    zip_of(to, &[
        ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#),
        ("xl/workbook.xml", br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#),
        ("xl/sharedStrings.xml", ss.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

/// A record of a `.xls`: its number, its length and its data.
fn biff(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend(id.to_le_bytes());
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
}

/// A `.xls` of about `PART` bytes: 200,000 shared strings, then sheets of 65,536 rows of four
/// strings and four numbers each, as many as fill it; in a compound file (`cfb`).
fn xls(to: &Path) -> io::Result<()> {
    let strings = 200_000u32;
    let mut w = Words::new(31);
    // The shared strings, each whole in one record; a record full goes on in the next.
    let (mut sst, mut records) = (vec![], vec![]);
    sst.extend(strings.to_le_bytes());
    sst.extend(strings.to_le_bytes());
    for _ in 0..strings {
        let line = w.line(6);
        let line = &line[..line.len().min(200)];
        if sst.len() + 3 + line.len() > 8224 {
            records.push(std::mem::take(&mut sst));
        }
        sst.extend((line.len() as u16).to_le_bytes());
        sst.push(0);
        sst.extend(line.as_bytes());
    }
    records.push(sst);
    let sheet = |w: &mut Words| {
        let mut out = vec![];
        biff(&mut out, 0x0809, &[0, 6, 16, 0]);
        for row in 0..=u16::MAX {
            for column in 0..8u16 {
                let mut data = [&row.to_le_bytes()[..], &column.to_le_bytes(), &[0, 0]].concat();
                if column % 2 == 0 {
                    data.extend((w.next() as u32 % strings).to_le_bytes());
                    biff(&mut out, 0x00FD, &data);
                } else {
                    data.extend(((w.next() as u32 % 100_000) << 2 | 2).to_le_bytes());
                    biff(&mut out, 0x027E, &data);
                }
            }
        }
        biff(&mut out, 0x000A, &[]);
        out
    };
    let mut sheets = vec![];
    let mut size = 0;
    while size < PART as usize {
        let s = sheet(&mut w);
        size += s.len();
        sheets.push(s);
    }
    let globals = |starts: &[usize]| {
        let mut out = vec![];
        biff(&mut out, 0x0809, &[0, 6, 5, 0]);
        for (n, start) in starts.iter().enumerate() {
            let name = format!("Data {}", n + 1);
            let data = [&(*start as u32).to_le_bytes()[..], &[0, 0], &[name.len() as u8, 0], name.as_bytes()].concat();
            biff(&mut out, 0x0085, &data);
        }
        for (n, r) in records.iter().enumerate() {
            biff(&mut out, if n == 0 { 0x00FC } else { 0x003C }, r);
        }
        biff(&mut out, 0x000A, &[]);
        out
    };
    let head = globals(&vec![0; sheets.len()]).len();
    let starts: Vec<usize> = sheets
        .iter()
        .scan(head, |at, s| {
            let start = *at;
            *at += s.len();
            Some(start)
        })
        .collect();
    let mut file = cfb::CompoundFile::create(std::fs::File::create(to)?)?;
    let mut stream = file.create_stream("Workbook")?;
    stream.write_all(&globals(&starts))?;
    for s in &sheets {
        stream.write_all(s)?;
    }
    drop(stream);
    file.flush()
}

/// A record of a `.xlsb`: its number and length seven bits to a byte, then its data.
fn brt(out: &mut Vec<u8>, id: u32, data: &[u8]) {
    let mut id = id;
    loop {
        let more = id >= 0x80;
        out.push(id as u8 & 0x7F | if more { 0x80 } else { 0 });
        id >>= 7;
        if !more {
            break;
        }
    }
    let mut length = data.len();
    while length >= 0x80 {
        out.push(length as u8 | 0x80);
        length >>= 7;
    }
    out.push(length as u8);
    out.extend(data);
}

/// A `.xlsb` with a sheet of about `PART` bytes in records and 200,000 shared strings.
fn xlsb(to: &Path) -> io::Result<()> {
    let strings = 200_000u32;
    let mut w = Words::new(37);
    let counted = |text: &str| {
        let units: Vec<u16> = text.encode_utf16().collect();
        [&(units.len() as u32).to_le_bytes()[..], &units.iter().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>()].concat()
    };
    let mut sst = vec![];
    for _ in 0..strings {
        brt(&mut sst, 0x13, &[&[0u8][..], &counted(&w.line(6))].concat());
    }
    let mut sheet = vec![];
    let mut row = 0u32;
    while (sheet.len() as u64) < PART {
        brt(&mut sheet, 0x00, &[&row.to_le_bytes()[..], &[0; 21]].concat());
        for column in 0..8u32 {
            let head = [&column.to_le_bytes()[..], &[0, 0, 0, 0]].concat();
            if column % 2 == 0 {
                brt(&mut sheet, 0x07, &[&head[..], &(w.next() as u32 % strings).to_le_bytes()].concat());
            } else {
                brt(&mut sheet, 0x02, &[&head[..], &((w.next() as u32 % 100_000) << 2 | 2).to_le_bytes()].concat());
            }
        }
        row += 1;
    }
    let mut book = vec![];
    brt(&mut book, 0x83, &[]);
    brt(&mut book, 0x8F, &[]);
    brt(&mut book, 0x9C, &[&[0u8; 4][..], &1u32.to_le_bytes(), &counted("rId1"), &counted("Data")].concat());
    brt(&mut book, 0x90, &[]);
    brt(&mut book, 0x84, &[]);
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    zip_of(to, &[
        ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.bin" ContentType="application/vnd.ms-excel.sheet.binary.macroEnabled.main"/><Override PartName="/xl/worksheets/sheet1.bin" ContentType="application/vnd.ms-excel.worksheet"/><Override PartName="/xl/sharedStrings.bin" ContentType="application/vnd.ms-excel.sharedStrings"/></Types>"#),
        ("_rels/.rels", format!(r#"<Relationships><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.bin"/></Relationships>"#).as_bytes()),
        ("xl/workbook.bin", &book),
        ("xl/_rels/workbook.bin.rels", format!(r#"<Relationships><Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.bin"/><Relationship Id="rId2" Type="{rel}/sharedStrings" Target="sharedStrings.bin"/></Relationships>"#).as_bytes()),
        ("xl/sharedStrings.bin", &sst),
        ("xl/worksheets/sheet1.bin", &sheet),
    ])
}

fn pptx(to: &Path) -> io::Result<()> {
    let slide = xml_part(
        PART,
        17,
        r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody>"#,
        "</p:txBody></p:sp></p:spTree></p:cSld></p:sld>",
        |t| format!("<a:p><a:r><a:t>{t}</a:t></a:r></a:p>"),
    );
    zip_of(to, &[
        ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#),
        ("ppt/presentation.xml", br#"<?xml version="1.0"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#),
        ("ppt/slides/slide1.xml", &slide),
    ])
}

fn odt(to: &Path) -> io::Result<()> {
    let content = xml_part(
        PART,
        19,
        r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text>"#,
        "</office:text></office:body></office:document-content>",
        |t| format!("<text:p>{t}</text:p>"),
    );
    zip_of(
        to,
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", &content),
        ],
    )
}

fn epub(to: &Path) -> io::Result<()> {
    let chapter = xml_part(
        PART,
        23,
        r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head><body>"#,
        "</body></html>",
        |t| format!("<p>{t}</p>"),
    );
    zip_of(to, &[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("OEBPS/content.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>A long book</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
        ("OEBPS/c1.xhtml", &chapter),
    ])
}

/// A page saved with its picture inside it: a tag of 30 MB between two paragraphs.
fn picture_inside(to: &Path) -> io::Result<()> {
    let mut page = b"<!doctype html><html><body><p>Before the picture</p><img alt=\"A chart\" src=\"data:image/png;base64,".to_vec();
    page.extend(std::iter::repeat_n(b'A', 30 * MB as usize));
    page.extend_from_slice(b"\"><p>After the picture</p></body></html>");
    std::fs::write(to, page)
}

fn html(to: &Path) -> io::Result<()> {
    std::fs::write(
        to,
        xml_part(
            PART,
            29,
            "<!doctype html><html><head><title>Long</title></head><body>",
            "</body></html>",
            |t| format!("<p>{t}</p>\n"),
        ),
    )
}

fn plain(to: &Path, bytes: u64, kind: &str) -> io::Result<()> {
    let mut f = BufWriter::new(std::fs::File::create(to)?);
    match kind {
        "log" => write_text(&mut f, bytes, 31, |w, i| {
            format!(
                "2026-10-21T{:02}:{:02}:{:02}.{:03}Z INFO worker-{} {}",
                i / 3_600_000 % 24,
                i / 60_000 % 60,
                i / 1000 % 60,
                i % 1000,
                i % 16,
                w.line(10)
            )
        })?,
        "csv" => {
            f.write_all(b"id,name,city,amount,note\n")?;
            write_text(&mut f, bytes, 37, |w, i| {
                format!(
                    "{i},{},{},{}.{:02},\"{}\"",
                    w.word().to_string(),
                    w.word().to_string(),
                    w.next() % 10_000,
                    w.next() % 100,
                    w.line(8)
                )
            })?
        }
        _ => write_text(&mut f, bytes, 41, |w, _| w.line(14))?,
    }
    f.flush()
}

/// A tar of `files` files of `each` bytes of text, then a small `last.txt`: the file at its end.
fn tar_of(to: &Path, files: usize, each: u64) -> io::Result<()> {
    let mut b = tar::Builder::new(BufWriter::new(std::fs::File::create(to)?));
    for i in 0..files {
        let mut data = vec![];
        write_text(&mut data, each, 100 + i as u64, |w, _| w.line(14))?;
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_760_000_000);
        b.append_data(&mut h, format!("data/part-{i:03}.txt"), &data[..])?;
    }
    let last = b"the last file of the archive\n";
    let mut h = tar::Header::new_gnu();
    h.set_size(last.len() as u64);
    h.set_mode(0o644);
    b.append_data(&mut h, "data/last.txt", &last[..])?;
    b.into_inner()?.flush()
}

/// `src` packed by `tool` (which writes it to its standard output) into `to`.
fn packed(src: &Path, to: &Path, tool: &[&str]) -> io::Result<()> {
    let out = std::fs::File::create(to)?;
    run(Command::new(tool[0]).args(&tool[1..]).arg(src).stdout(out))
}

/// A `docker save` tar: `manifest.json`, a config, and `layers` layer tars of about `each`
/// bytes (gzipped with `gz`), the last holding `etc/last.txt` after its data.
fn image(to: &Path, layers: usize, each: u64, gz: bool) -> io::Result<()> {
    let mut b = tar::Builder::new(BufWriter::new(std::fs::File::create(to)?));
    let mut names = vec![];
    for l in 0..layers {
        let mut layer = tar::Builder::new(vec![]);
        let mut data = vec![];
        write_text(&mut data, each, 200 + l as u64, |w, _| w.line(14))?;
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        layer.append_data(&mut h, format!("usr/share/layer{l}/data.txt"), &data[..])?;
        if l + 1 == layers {
            let mut h = tar::Header::new_gnu();
            h.set_size(5);
            h.set_mode(0o644);
            layer.append_data(&mut h, "etc/last.txt", &b"last\n"[..])?;
        }
        let mut bytes = layer.into_inner()?;
        if gz {
            let mut w = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
            w.write_all(&bytes)?;
            bytes = w.finish()?;
        }
        let name = format!("{:064x}/layer.tar", l + 1);
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        b.append_data(&mut h, &name, &bytes[..])?;
        names.push(format!("\"{name}\""));
    }
    let config = br#"{"architecture":"amd64","os":"linux","config":{},"rootfs":{"type":"layers","diff_ids":[]}}"#;
    let mut h = tar::Header::new_gnu();
    h.set_size(config.len() as u64);
    h.set_mode(0o644);
    b.append_data(&mut h, "config.json", &config[..])?;
    let manifest = format!(
        r#"[{{"Config":"config.json","RepoTags":["localhost/bench:1"],"Layers":[{}]}}]"#,
        names.join(",")
    );
    let mut h = tar::Header::new_gnu();
    h.set_size(manifest.len() as u64);
    h.set_mode(0o644);
    b.append_data(&mut h, "manifest.json", manifest.as_bytes())?;
    b.into_inner()?.flush()
}

/// A zip holding `inner` (stored as it is, as zips in zips often are) and a small note.
fn zip_around(to: &Path, name: &str, inner: &Path) -> io::Result<()> {
    let mut z = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(to)?));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    z.start_file("readme.txt", o).map_err(io::Error::other)?;
    z.write_all(b"a zip in a zip\n")?;
    z.start_file(name, o).map_err(io::Error::other)?;
    io::copy(&mut std::fs::File::open(inner)?, &mut z)?;
    z.finish().map_err(io::Error::other)?;
    Ok(())
}

/// A zip of one text file of `bytes` (deflated).
fn zip_big(to: &Path, name: &str, bytes: u64) -> io::Result<()> {
    let mut z = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(to)?));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1))
        .large_file(true);
    z.start_file(name, o).map_err(io::Error::other)?;
    write_text(&mut z, bytes, 43, |w, _| w.line(14))?;
    z.finish().map_err(io::Error::other)?;
    Ok(())
}

/// A zip of `n` small text files in folders of 1,000, deflated: a listing as long as a photo
/// library's or a source tree's.
fn zip_many(to: &Path, n: usize) -> io::Result<()> {
    let mut z = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(to)?));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));
    let mut w = Words::new(47);
    for i in 0..n {
        z.start_file(format!("d{:03}/f{i:06}.txt", i / 1000), o)
            .map_err(io::Error::other)?;
        z.write_all(w.line(12).as_bytes())?;
    }
    z.finish().map_err(io::Error::other)?;
    Ok(())
}

/// A 7z of `n` small Go games, in solid blocks of 10,000, as a collection of a few hundred
/// thousand games is packed: a list of contents of over 64 MB.
fn seven_many(to: &Path, n: usize) -> io::Result<()> {
    let mut w = sevenz_rust2::ArchiveWriter::create(to).map_err(io::Error::other)?;
    let mut words = Words::new(61);
    for start in (0..n).step_by(10_000) {
        let (mut entries, mut readers) = (vec![], vec![]);
        for i in start..(start + 10_000).min(n) {
            let name = if i + 1 == n {
                "kgs-19-2019-games/last.sgf".to_string()
            } else {
                format!("kgs-19-2019-games/{:02}/{}-vs-{}-{i:07}.sgf", i % 12, words.word().to_string(), words.word().to_string())
            };
            entries.push(sevenz_rust2::ArchiveEntry::new_file(&name));
            let game = format!("(;GM[1]FF[4]SZ[19]PB[{}]PW[{}];B[pd];W[dp];B[pp];W[dd])\n", words.word().to_string(), words.word().to_string());
            readers.push(sevenz_rust2::SourceReader::from(io::Cursor::new(game.into_bytes())));
        }
        w.push_archive_entries(entries, readers).map_err(io::Error::other)?;
    }
    w.finish().map_err(io::Error::other)?;
    Ok(())
}

/// A 7z of `n` small Go games all in one folder, `3d`, in solid blocks of 10,000: as the
/// largest folders of a public Go dataset are (two million games in one).
fn seven_flat(to: &Path, n: usize) -> io::Result<()> {
    let mut w = sevenz_rust2::ArchiveWriter::create(to).map_err(io::Error::other)?;
    let mut words = Words::new(67);
    for start in (0..n).step_by(10_000) {
        let (mut entries, mut readers) = (vec![], vec![]);
        for i in start..(start + 10_000).min(n) {
            let name = if i + 1 == n {
                "3d/last.sgf".to_string()
            } else {
                format!("3d/2019-{:02}-{:02}-{i:07}.sgf", i % 12 + 1, i % 28 + 1)
            };
            entries.push(sevenz_rust2::ArchiveEntry::new_file(&name));
            let game = format!("(;GM[1]FF[4]SZ[19]PB[{}]PW[{}];B[pd];W[dp])\n", words.word().to_string(), words.word().to_string());
            readers.push(sevenz_rust2::SourceReader::from(io::Cursor::new(game.into_bytes())));
        }
        w.push_archive_entries(entries, readers).map_err(io::Error::other)?;
    }
    w.finish().map_err(io::Error::other)?;
    Ok(())
}

/// A folder for search to read: a zip of 200 text files of 1 MB, a tar.gz of the same, and a
/// zip of one 100 MB text file.
fn search_folder(to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    let mut z = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(to.join("notes.zip"))?));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));
    let mut b = tar::Builder::new(vec![]);
    for i in 0..200 {
        let mut data = vec![];
        write_text(&mut data, MB, 500 + i, |w, _| w.line(14))?;
        z.start_file(format!("notes/n{i:03}.txt"), o).map_err(io::Error::other)?;
        z.write_all(&data)?;
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        b.append_data(&mut h, format!("notes/n{i:03}.txt"), &data[..])?;
    }
    z.finish().map_err(io::Error::other)?;
    let mut gz = flate2::write::GzEncoder::new(
        BufWriter::new(std::fs::File::create(to.join("notes.tar.gz"))?),
        flate2::Compression::fast(),
    );
    gz.write_all(&b.into_inner()?)?;
    gz.finish()?.flush()?;
    zip_big(&to.join("big.zip"), "big.txt", 100 * MB)
}

// ---------------------------------------------------------------- the cases

#[derive(Clone, Copy)]
enum Do {
    /// `extract::text_of` with no size limit, then `meaning::passages` of the text.
    Read,
    /// `meaning::passages_streamed` of the whole file (`extract::whole_lines`), as search by
    /// meaning makes the passages of a document with more text than is kept.
    Whole,
    /// `archive::list`, the whole archive.
    List,
    /// `archive::listing` of a folder inside, as a panel shows it, then of the archive's top
    /// and that folder again (as browsing goes in, up and in); with what stays in memory once
    /// the listings are dropped (the archive's list kept for the next look).
    Folder(&'static str),
    /// `archive::copy_out` of an entry into a fresh folder.
    CopyOut(&'static str),
    /// `archive::peek` of an entry: the copy the preview pane and F3 show.
    Peek(&'static str),
    /// The games table of a PGN file (`chess::games::Library`), read to its end.
    Games,
    /// The games list of an SGF collection (`go::Collection`).
    Sgf,
    /// `store::scan` of a folder, archives read inside: their files' text in the store.
    Search,
}

struct Case {
    name: String,
    input: PathBuf,
    what: Do,
}

fn cases() -> Vec<Case> {
    let mut out = vec![];
    let mut add = |name: &str, input: Option<PathBuf>, what: Do| {
        if let Some(input) = input {
            out.push(Case {
                name: name.into(),
                input,
                what,
            });
        }
    };
    // Readers.
    add(
        "pdf 20 MB, text only (2,000 pages)",
        input("text-20.pdf", |p| pdf(p, 2000, 24_000, 0)),
        Do::Read,
    );
    add(
        "pdf 150 MB, text and pictures (1,000 pages)",
        input("pictures-150.pdf", |p| pdf(p, 1000, 3000, 150_000)),
        Do::Read,
    );
    add(
        "docx, 60 MB document.xml",
        input("huge-part.docx", |p| docx(p, PART)),
        Do::Read,
    );
    add(
        "docx, 200 MB document.xml",
        input("over-cap.docx", |p| docx(p, 200 * MB)),
        Do::Read,
    );
    add(
        "xlsx, 60 MB sheet, 200,000 strings",
        input("huge-sheet.xlsx", xlsx),
        Do::Read,
    );
    add(
        "xls, 60 MB in sheets, 200,000 strings",
        input("huge-sheets.xls", xls),
        Do::Read,
    );
    add(
        "xlsb, 60 MB sheet, 200,000 strings",
        input("huge-sheet.xlsb", xlsb),
        Do::Read,
    );
    add(
        "xls, the same, passages from the whole file",
        input("huge-sheets.xls", xls),
        Do::Whole,
    );
    add(
        "xlsb, the same, passages from the whole file",
        input("huge-sheet.xlsb", xlsb),
        Do::Whole,
    );
    add(
        "pptx, 60 MB slide",
        input("huge-slide.pptx", pptx),
        Do::Read,
    );
    add(
        "odt, 60 MB content.xml",
        input("huge-content.odt", odt),
        Do::Read,
    );
    add(
        "epub, 60 MB chapter",
        input("huge-chapter.epub", epub),
        Do::Read,
    );
    add("html, 60 MB", input("huge.html", html), Do::Read);
    add(
        "epub, the same, passages from the whole file",
        input("huge-chapter.epub", epub),
        Do::Whole,
    );
    add(
        "html, the same, passages from the whole file",
        input("huge.html", html),
        Do::Whole,
    );
    add(
        "html, a 30 MB picture inside a tag",
        input("picture-inside.html", picture_inside),
        Do::Read,
    );
    add(
        "log 300 MB",
        input("big.log", |p| plain(p, 300 * MB, "log")),
        Do::Read,
    );
    add(
        "csv 300 MB",
        input("big.csv", |p| plain(p, 300 * MB, "csv")),
        Do::Read,
    );
    add(
        "txt 300 MB",
        input("big.txt", |p| plain(p, 300 * MB, "txt")),
        Do::Read,
    );
    // Archives.
    let big = input("big-entry.zip", |p| zip_big(p, "big.txt", 500 * MB));
    add("zip, 500 MB entry: list", big.clone(), Do::List);
    add("zip, 500 MB entry: peek", big.clone(), Do::Peek("big.txt"));
    add("zip, 500 MB entry: copy out", big, Do::CopyOut("big.txt"));
    let inner = input("nest-3.zip", |p| zip_big(p, "inner.txt", 200 * MB));
    let mid = inner.and_then(|i| input("nest-2.zip", |p| zip_around(p, "nest-3.zip", &i)));
    let outer = mid.and_then(|m| input("nest-1.zip", |p| zip_around(p, "nest-2.zip", &m)));
    add(
        "zip in zip in zip (inner 200 MB): list outer",
        outer.clone(),
        Do::List,
    );
    add(
        "zip in zip in zip: peek the 2nd level",
        outer.clone(),
        Do::Peek("nest-2.zip"),
    );
    add(
        "zip in zip in zip: copy out the 2nd level",
        outer,
        Do::CopyOut("nest-2.zip"),
    );
    let tar = input("big.tar", |p| tar_of(p, 20, 100 * MB));
    add("tar 2 GB: list", tar.clone(), Do::List);
    add(
        "tar 2 GB: peek the last entry",
        tar.clone(),
        Do::Peek("data/last.txt"),
    );
    for (ending, tool) in [
        ("gz", &["gzip", "-1", "-c"][..]),
        ("xz", &["xz", "-1", "-T3", "-c"][..]),
        ("zst", &["zstd", "-3", "-T3", "-q", "-c"][..]),
    ] {
        let p = tar
            .as_ref()
            .and_then(|t| input(&format!("big.tar.{ending}"), |p| packed(t, p, tool)));
        add(&format!("tar.{ending} 2 GB: list"), p.clone(), Do::List);
        add(
            &format!("tar.{ending} 2 GB: peek the last entry"),
            p.clone(),
            Do::Peek("data/last.txt"),
        );
    }
    // Each case is a process of its own with the same cache: a list keeps what the peeks
    // after it use.
    for (file, what, gz) in [("image-10-layers.tar", "", false), ("image-10-gz-layers.tar", "gzip ", true)] {
        let img = input(file, |p| image(p, 10, 50 * MB, gz));
        add(
            &format!("container image, 10 {what}layers of 50 MB: list"),
            img.clone(),
            Do::List,
        );
        add(
            &format!("container image, {what}layers: peek a file at the end of the last layer"),
            img.clone(),
            Do::Peek("etc/last.txt"),
        );
        add(
            &format!("container image, {what}layers: copy out the last layer's 50 MB file"),
            img.clone(),
            Do::CopyOut("usr/share/layer9/data.txt"),
        );
        add(&format!("container image, {what}layers: list again, in a new run"), img, Do::List);
    }
    let seven = tar.as_ref().and_then(|_| {
        input("solid.7z", |p| {
            let src = dir().join("solid-src");
            let _ = std::fs::remove_dir_all(&src);
            std::fs::create_dir_all(&src)?;
            for i in 0..30 {
                let mut f =
                    BufWriter::new(std::fs::File::create(src.join(format!("part-{i:02}.txt")))?);
                write_text(&mut f, 10 * MB, 300 + i, |w, _| w.line(14))?;
                f.flush()?;
            }
            std::fs::write(src.join("zz-last.txt"), b"last\n")?;
            // 7z wants its own ending; it is renamed when done.
            let to = p.with_extension("7z");
            let r = run(Command::new("7z")
                .args(["a", "-t7z", "-ms=on", "-mx=3", "-mmt=3", "-bd", "-bso0"])
                .arg(&to)
                .arg(src.join(".")));
            let _ = std::fs::remove_dir_all(&src);
            r.and_then(|()| std::fs::rename(&to, p))
        })
    });
    add(
        "7z solid, 300 MB in 31 files: list",
        seven.clone(),
        Do::List,
    );
    add(
        "7z solid: peek the last entry",
        seven,
        Do::Peek("zz-last.txt"),
    );
    let many = input("many-200k.zip", |p| zip_many(p, 200_000));
    add("zip, 200,000 entries: list", many.clone(), Do::List);
    add(
        "zip, 200,000 entries: peek the last entry",
        many,
        Do::Peek("d199/f199999.txt"),
    );
    let flat = input("games-2m-flat.7z", |p| seven_flat(p, 2_000_000));
    add("7z, 2,000,000 games in one folder: list the folder", flat.clone(), Do::Folder("3d"));
    add("7z, 2,000,000 games in one folder: peek the last entry", flat, Do::Peek("3d/last.sgf"));
    let games = input("games-900k.7z", |p| seven_many(p, 900_000));
    add("7z, 900,000 games, a 90 MB list: list", games.clone(), Do::List);
    add(
        "7z, 900,000 games: peek the last entry",
        games,
        Do::Peek("kgs-19-2019-games/last.sgf"),
    );
    let bz = tar
        .as_ref()
        .and_then(|t| input("big.tar.bz2", |p| packed(t, p, &["bzip2", "-1", "-c"])));
    add("tar.bz2 2 GB: list", bz.clone(), Do::List);
    add("tar.bz2 2 GB: peek the last entry", bz, Do::Peek("data/last.txt"));
    let inner = input("inner.tar", |p| tar_of(p, 5, 40 * MB))
        .and_then(|t| input("inner.tar.gz", |p| packed(&t, p, &["gzip", "-1", "-c"])));
    let outer = inner.and_then(|i| {
        input("outer.tar", |p| {
            let mut b = tar::Builder::new(BufWriter::new(std::fs::File::create(p)?));
            b.append_path_with_name(&i, "inner.tar.gz")?;
            b.into_inner()?.flush()
        })
    });
    let outer = outer.and_then(|o| input("outer.tar.gz", |p| packed(&o, p, &["gzip", "-1", "-c"])));
    add(
        "tar.gz in tar.gz (inner 200 MB): peek an entry of the inner",
        outer,
        Do::Peek("inner.tar.gz/data/last.txt"),
    );
    add(
        "search inside archives: zip and tar.gz of 200 × 1 MB, zip of 100 MB",
        input("search-archives", search_folder),
        Do::Search,
    );
    // Game files.
    let pgn_file = input("games-2g.pgn", |p| pgn(p, 7_000_000));
    add(
        "pgn 2 GB, 7 million games: games table",
        pgn_file.clone(),
        Do::Games,
    );
    add("pgn 2 GB: search text", pgn_file, Do::Read);
    let sgf_file = input("games-300m.sgf", |p| sgf(p, 300 * MB));
    add(
        "sgf 300 MB collection: games list",
        sgf_file.clone(),
        Do::Sgf,
    );
    add("sgf 300 MB: search text", sgf_file, Do::Read);
    out
}

/// A PGN collection of `games` games of about 400 bytes: real openings, made-up players.
fn pgn(to: &Path, games: usize) -> io::Result<()> {
    const LINES: [&str; 4] = [
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7",
        "1. d4 Nf6 2. c4 e6 3. Nc3 Bb4 4. e3 O-O 5. Bd3 d5 6. Nf3 c5 7. O-O Nc6 8. a3 Bxc3 9. bxc3 dxc4 10. Bxc4 Qc7",
        "1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 6. Be3 e5 7. Nb3 Be6 8. f3 Be7 9. Qd2 O-O 10. O-O-O Nbd7",
        "1. c4 e5 2. Nc3 Nf6 3. Nf3 Nc6 4. g3 d5 5. cxd5 Nxd5 6. Bg2 Nb6 7. O-O Be7 8. d3 O-O 9. a3 Be6 10. b4 f6",
    ];
    let mut f = BufWriter::new(std::fs::File::create(to)?);
    let mut w = Words::new(51);
    for i in 0..games {
        // Players from a pool of 50,000, as a real collection repeats them.
        let player = |w: &mut Words| {
            let i = (w.next() % 50_000) as usize;
            format!("{} {}", w.vocab[i % 4000], w.vocab[i / 4000 % 4000])
        };
        let (white, black) = (player(&mut w), player(&mut w));
        let result = ["1-0", "0-1", "1/2-1/2"][i % 3];
        writeln!(
            f,
            "[Event \"{} open\"]\n[Site \"{}\"]\n[Date \"{}.{:02}.{:02}\"]\n[Round \"{}\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n[WhiteElo \"{}\"]\n[BlackElo \"{}\"]\n\n{} {result}\n",
            w.word().to_string(),
            w.word().to_string(),
            1950 + i % 75,
            1 + i % 12,
            1 + i % 28,
            1 + i % 9,
            1800 + w.next() % 900,
            1800 + w.next() % 900,
            LINES[i % 4],
        )?;
    }
    f.flush()
}

/// An SGF collection of about `bytes`: 19×19 games of 150 moves each.
fn sgf(to: &Path, bytes: u64) -> io::Result<()> {
    let mut f = BufWriter::new(std::fs::File::create(to)?);
    let mut w = Words::new(53);
    let (mut n, mut i) = (0u64, 0u64);
    while n < bytes {
        let mut g = format!(
            "(;GM[1]FF[4]SZ[19]KM[6.5]PB[{}]PW[{}]EV[{} cup]DT[2020-01-{:02}]RE[{}]",
            w.word().to_string(),
            w.word().to_string(),
            w.word().to_string(),
            1 + i % 28,
            ["B+R", "W+3.5", "B+0.5"][(i % 3) as usize]
        );
        for m in 0..150 {
            let c = |v: u64| (b'a' + (v % 19) as u8) as char;
            g.push_str(&format!(
                ";{}[{}{}]",
                if m % 2 == 0 { 'B' } else { 'W' },
                c(w.next()),
                c(w.next())
            ));
        }
        g.push_str(")\n");
        f.write_all(g.as_bytes())?;
        n += g.len() as u64;
        i += 1;
    }
    f.flush()
}

// ---------------------------------------------------------------- measuring

fn status(key: &str) -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

/// Runs in the process of its own: one case, a line `ROW|…` for the table.
#[test]
#[ignore]
fn memory_case() {
    let Some(name) = std::env::var_os("COXSWAIN_BENCH_CASE") else {
        return;
    };
    let case = cases()
        .into_iter()
        .find(|c| *c.name == *name)
        .expect("a case of that name");
    // A folder's size is its files'.
    let size = match std::fs::read_dir(&case.input) {
        Ok(d) => d.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum(),
        Err(_) => std::fs::metadata(&case.input).unwrap().len(),
    };
    // The peak so far is forgotten: from here on it is this case's.
    std::fs::write("/proc/self/clear_refs", "5").expect("Linux 4.0 or later");
    let base = status("VmRSS:");
    let t = Instant::now();
    let result = match case.what {
        Do::Read => {
            let text = extract::text_of(&case.input, size, u64::MAX);
            let read = t.elapsed().as_secs_f64();
            let n = text.as_ref().map_or(0, String::len);
            let passages = text.as_deref().map_or(0, |t| {
                meaning::passages(t, &case.input.to_string_lossy()).len()
            });
            format!(
                "{:.0} KB of text, {passages} passages (read {read:.2} s)",
                n as f64 / 1024.0
            )
        }
        Do::Whole => {
            let path = case.input.to_string_lossy();
            let (mut n, mut lines) = (0usize, 0usize);
            let passages = meaning::passages_streamed(&path, &mut |f| {
                extract::whole_lines(&case.input, &mut |l| {
                    n += l.len();
                    lines += 1;
                    f(l)
                }) == Some(true)
            });
            format!(
                "{:.0} MB of text read twice, {} passages",
                n as f64 / 2.0 / 1e6,
                passages.map_or(0, |p| p.len())
            )
        }
        Do::List => match archive::list(&case.input, usize::MAX) {
            Ok((entries, _)) => format!("{} entries", entries.len()),
            Err(e) => format!("error: {e}"),
        },
        Do::Folder(inner) => match archive::listing(&case.input, inner) {
            Ok((entries, _)) => {
                let n = entries.len();
                let first = t.elapsed().as_secs_f64();
                drop(entries);
                let again = Instant::now();
                let top = archive::listing(&case.input, "").map_or(0, |(e, _)| e.len());
                let back = archive::listing(&case.input, inner).map_or(0, |(e, _)| e.len());
                let again = again.elapsed().as_secs_f64();
                // What is still held once the panels let go: freed memory handed back first.
                #[cfg(all(target_os = "linux", target_env = "gnu"))]
                // SAFETY: only hands freed memory back to the system.
                unsafe {
                    libc::malloc_trim(0);
                }
                let kept = (status("VmRSS:") - base).max(0.0);
                format!("{n} entries ({first:.2} s; top {top} and back {back} in {again:.2} s), {kept:.0} MB kept after")
            }
            Err(e) => format!("error: {e}"),
        },
        Do::CopyOut(inner) => {
            let to = dir().join("out");
            let _ = std::fs::remove_dir_all(&to);
            std::fs::create_dir_all(&to).unwrap();
            let r = match archive::copy_out(&case.input, inner, &to, None, false) {
                Ok(p) => format!(
                    "{:.0} MB out",
                    std::fs::metadata(p).map_or(0, |m| m.len()) as f64 / 1e6
                ),
                Err(e) => format!("error: {e}"),
            };
            let _ = std::fs::remove_dir_all(&to);
            r
        }
        Do::Games => {
            let lib = coxswain_core::chess::games::Library::open(&case.input).unwrap();
            let mut first = None;
            loop {
                let p = lib.progress();
                if first.is_none() && p.games >= 1000 {
                    first = Some(t.elapsed().as_secs_f64());
                }
                if p.done {
                    break format!(
                        "{} games{} (first 1,000 after {:.2} s)",
                        p.games,
                        if p.capped { ", capped" } else { "" },
                        first.unwrap_or_default()
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        Do::Sgf => match coxswain_core::go::Collection::open(&case.input) {
            Ok(c) => {
                c.wait(usize::MAX);
                let p = c.progress();
                format!("{} games{}", p.games, if p.capped { ", capped" } else { "" })
            }
            Err(e) => format!("error: {e}"),
        },
        Do::Search => {
            use coxswain_core::store::{self, Store};
            let db = dir().join("search-archives.db");
            let _ = std::fs::remove_file(&db);
            let cfg = coxswain_core::config::SearchConfig {
                text_roots: vec![case.input.clone()],
                archives: true,
                text_max_size: u64::MAX,
                ..Default::default()
            };
            let store = Store::open(&db).unwrap();
            store.hurry.store(true, std::sync::atomic::Ordering::Relaxed);
            store::scan(&store, &cfg, &std::sync::atomic::AtomicBool::new(false)).unwrap();
            let r = format!("{} texts", store.texts());
            drop(store);
            let _ = std::fs::remove_file(&db);
            r
        }
        Do::Peek(inner) => match archive::peek(&case.input.join(inner)) {
            Ok(p) => format!(
                "{:.0} MB copy",
                std::fs::metadata(p).map_or(0, |m| m.len()) as f64 / 1e6
            ),
            Err(e) => format!("refused: {e}"),
        },
    };
    let secs = t.elapsed().as_secs_f64();
    let peak = status("VmHWM:");
    println!(
        "\nROW|{}|{:.0}|{:.2}|{:.0}|{}",
        case.name,
        size as f64 / 1e6,
        secs,
        (peak - base).max(0.0),
        result
    );
}

/// Makes the inputs, measures each case in a process of its own and prints the table.
#[test]
#[ignore]
fn memory_table() {
    if std::env::var_os("COXSWAIN_BENCH_CASE").is_some() {
        return;
    }
    let only: Vec<String> = std::env::var("COXSWAIN_BENCH_ONLY")
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let home = dir().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut rows = vec![];
    for case in cases() {
        if !only.is_empty() && !only.iter().any(|o| case.name.contains(o.as_str())) {
            continue;
        }
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "memory_case",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("COXSWAIN_BENCH_CASE", &case.name)
            .env("COXSWAIN_BENCH_DIR", dir().parent().unwrap())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let row = text
            .lines()
            .find_map(|l| l.split_once("ROW|").map(|(_, r)| r))
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}|||| failed: {}", case.name, out.status));
        eprintln!("{row}");
        rows.push(row);
    }
    println!("\n| Case | Input MB | Seconds | Peak RSS MB | Result |\n|---|--:|--:|--:|---|");
    for r in rows {
        println!("| {} |", r.replace('|', " | "));
    }
}

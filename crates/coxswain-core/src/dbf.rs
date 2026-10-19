//! dBase, FoxPro and Clipper tables (`.dbf`, a shapefile's attributes too): the fields with
//! their types, and the records, as both apps' table views show them (`tables`) and as search
//! reads them. The text's code page comes from the header's language byte, or from the
//! shapefile's `.cpg` beside it; memo fields (kept in `.dbt` or `.fpt` files) are named, not read.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use encoding_rs::Encoding;

/// The most fields a table may have: dBase's limit is 255, FoxPro's 255, dBase 7's 1024.
const MAX_FIELDS: usize = 2048;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    /// dBase's letter: C text, N and F numbers, D date, L yes/no, M memo, and the rest.
    pub kind: char,
    pub len: usize,
    pub decimals: u8,
}

/// A table's header: what each record holds and where the records are.
#[derive(Debug)]
pub struct Table {
    pub version: u8,
    pub records: u64,
    pub fields: Vec<Field>,
    /// The date of its last change, as dBase keeps it (`YYYY-MM-DD`).
    pub updated: String,
    /// The code page its text is read in, by name.
    pub code_page: &'static str,
    start: u64,
    record_len: usize,
    decode: Decode,
}

#[derive(Debug, Clone, Copy)]
enum Decode {
    Utf8OrAnsi,
    Dos(&'static str),
    Other(&'static Encoding),
}

/// Code page 437's characters from 0x80, then code page 850's: `encoding_rs` has neither.
const CP437: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";
const CP850: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";

/// The code page of a language driver byte (dBase's and Visual FoxPro's ids).
fn code_page(id: u8) -> (&'static str, Decode) {
    use encoding_rs::*;
    let other = |name, e| (name, Decode::Other(e));
    match id {
        0x01 => ("437", Decode::Dos(CP437)),
        // DOS code pages without a table here read as 850, their nearest.
        0x02 | 0x64 | 0x66 | 0x67 | 0x6a | 0x6b => ("850", Decode::Dos(CP850)),
        0x03 | 0x57 | 0x58 | 0x59 => other("1252", WINDOWS_1252),
        0x04 => other("Mac Roman", MACINTOSH),
        0x65 | 0x26 => other("866", IBM866),
        0x7b | 0x13 => other("932", SHIFT_JIS),
        0x7a | 0x4d => other("936", GBK),
        0x79 | 0x4e => other("949", EUC_KR),
        0x78 | 0x4f => other("950", BIG5),
        0x7c | 0x50 => other("874", WINDOWS_874),
        0x7d => other("1255", WINDOWS_1255),
        0x7e => other("1256", WINDOWS_1256),
        0xc8 => other("1250", WINDOWS_1250),
        0xc9 => other("1251", WINDOWS_1251),
        0xca => other("1254", WINDOWS_1254),
        0xcb => other("1253", WINDOWS_1253),
        0x96 => other("Mac Cyrillic", X_MAC_CYRILLIC),
        _ => ("UTF-8 / 1252", Decode::Utf8OrAnsi),
    }
}

fn text(bytes: &[u8], how: Decode) -> String {
    let s = match how {
        Decode::Utf8OrAnsi => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
        },
        Decode::Dos(high) if bytes.is_ascii() => {
            let _ = high;
            String::from_utf8_lossy(bytes).into_owned()
        }
        Decode::Dos(high) => {
            let high: Vec<char> = high.chars().collect();
            bytes.iter().map(|&b| if b < 0x80 { b as char } else { high[usize::from(b - 0x80)] }).collect()
        }
        Decode::Other(e) => e.decode_without_bom_handling(bytes).0.into_owned(),
    };
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// The table's header, `None` for a file that is no dBase table.
pub fn open(path: &Path) -> Option<(File, Table)> {
    if crate::cloud::unasked(path) {
        return None;
    }
    let mut f = File::open(path).ok()?;
    let mut h = [0u8; 32];
    f.read_exact(&mut h).ok()?;
    // The versions dBase, FoxPro and Clipper wrote: the low bits 2 to 5 (dBase II to 7).
    let version = h[0];
    if !matches!(version, 0x02 | 0x03 | 0x04 | 0x05 | 0x30 | 0x31 | 0x32 | 0x43 | 0x63 | 0x83 | 0x8b | 0x8c | 0xcb | 0xe5 | 0xf5 | 0xfb) {
        return None;
    }
    let records = u32::from_le_bytes([h[4], h[5], h[6], h[7]]).into();
    let start: u64 = u16::from_le_bytes([h[8], h[9]]).into();
    let record_len: usize = u16::from_le_bytes([h[10], h[11]]).into();
    let (month, day) = (h[2], h[3]);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || start < 33 {
        return None;
    }
    let updated = format!("{}-{month:02}-{day:02}", 1900 + u32::from(h[1]));
    // dBase 7 has 48-byte field descriptors after a header of 68.
    let (desc, first) = if version & 0x07 == 0x04 { (48, 68) } else { (32, 32) };
    let mut head = vec![0u8; (start as usize).saturating_sub(first)];
    f.seek(SeekFrom::Start(first as u64)).ok()?;
    f.read_exact(&mut head).ok()?;
    let mut fields = vec![];
    for d in head.chunks_exact(desc).take(MAX_FIELDS) {
        if d[0] == 0x0d {
            break;
        }
        let (name_len, kind_at, len_at) = if desc == 48 { (32, 32, 33) } else { (11, 11, 16) };
        let name = String::from_utf8_lossy(d[..name_len].split(|c| *c == 0).next().unwrap_or_default()).trim().to_string();
        // A character field's length goes on in the decimals byte in Clipper and FoxPro.
        let kind = d[kind_at] as char;
        let mut len = usize::from(d[len_at]);
        if kind == 'C' && desc == 32 {
            len += usize::from(d[17]) << 8;
        }
        fields.push(Field { name, kind, len, decimals: if kind == 'C' { 0 } else { d[len_at + 1] } });
    }
    let sum: usize = 1 + fields.iter().map(|f| f.len).sum::<usize>();
    // A record holds the deleted mark and every field: one that cannot is no table.
    if fields.is_empty() || record_len == 0 || sum > record_len {
        return None;
    }
    // A shapefile names its code page in a `.cpg` file beside it.
    let cpg_path = path.with_extension("cpg");
    // A regular file only (never a pipe that would wait), and only its first bytes: a label is short.
    let cpg = (!crate::cloud::unasked(&cpg_path)
        && std::fs::metadata(&cpg_path).is_ok_and(|m| m.is_file()))
    .then(|| {
        use std::io::Read;
        let mut label = Vec::new();
        File::open(&cpg_path).and_then(|f| f.take(64).read_to_end(&mut label)).ok()?;
        Encoding::for_label(String::from_utf8_lossy(&label).trim().as_bytes())
    })
    .flatten();
    let (code_page, decode) = match cpg {
        Some(e) => (e.name(), Decode::Other(e)),
        None => code_page(h[29]),
    };
    Some((f, Table { version, records, fields, updated, code_page, start, record_len, decode }))
}

impl Table {
    /// The fields as "name C(20)" and the like, for the schema line.
    pub fn schema(&self) -> String {
        self.fields
            .iter()
            .map(|f| match f.kind {
                'N' | 'F' if f.decimals > 0 => format!("{} {}({},{})", f.name, f.kind, f.len, f.decimals),
                k => format!("{} {k}({})", f.name, f.len),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// One field of a record as text.
    fn value(&self, f: &Field, raw: &[u8]) -> String {
        let le = |n: usize| raw.get(..n).map(|b| b.iter().rev().fold(0u64, |v, x| (v << 8) | u64::from(*x)));
        match f.kind {
            'D' if raw.len() == 8 && raw.iter().all(u8::is_ascii_digit) => {
                let s = String::from_utf8_lossy(raw);
                format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..])
            }
            'L' => match raw.first() {
                Some(b'T' | b't' | b'Y' | b'y') => "✓".into(),
                Some(b'F' | b'f' | b'N' | b'n') => "✗".into(),
                _ => String::new(),
            },
            // Visual FoxPro's binary ones.
            'I' if raw.len() == 4 => (le(4).unwrap_or(0) as u32 as i32).to_string(),
            'Y' if raw.len() == 8 => format!("{:.4}", le(8).unwrap_or(0) as i64 as f64 / 10_000.0),
            'B' | 'O' if raw.len() == 8 => f64::from_bits(le(8).unwrap_or(0)).to_string(),
            'T' | '@' if raw.len() == 8 => {
                let day = le(4).unwrap_or(0) as i64;
                let ms = (le(8).unwrap_or(0) >> 32) as i64;
                // Julian day 2440588 is 1970-01-01.
                let secs = (day - 2_440_588) * 86_400 + ms / 1000;
                chrono::DateTime::from_timestamp(secs, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
            }
            'M' | 'G' | 'P' | 'B' => {
                let t = text(raw, Decode::Utf8OrAnsi);
                let binary = raw.len() == 4 && le(4).unwrap_or(0) != 0;
                if t.trim_start_matches('0').is_empty() && !binary { String::new() } else { crate::t!("dbf.memo") }
            }
            _ => text(raw, self.decode),
        }
    }

    /// Up to `max` records from the first, without those marked deleted: each its values.
    pub fn rows(&self, f: &mut File, max: usize) -> Vec<Vec<String>> {
        let mut out = vec![];
        self.each(f, |row| {
            out.push(row);
            out.len() < max
        });
        out
    }

    /// Each record that is not marked deleted, from the first, to `take`, until it says stop.
    pub fn each(&self, f: &mut File, mut take: impl FnMut(Vec<String>) -> bool) {
        if self.record_len == 0 || f.seek(SeekFrom::Start(self.start)).is_err() {
            return;
        }
        let mut r = std::io::BufReader::new(f.take(self.records.saturating_mul(self.record_len as u64)));
        let mut rec = vec![0u8; self.record_len];
        while r.read_exact(&mut rec).is_ok() {
            if rec.first() == Some(&b'*') {
                continue;
            }
            let mut at = 1;
            let row = self
                .fields
                .iter()
                .map(|fd| {
                    let raw = &rec[at.min(rec.len())..(at + fd.len).min(rec.len())];
                    at += fd.len;
                    self.value(fd, raw)
                })
                .collect();
            if !take(row) {
                break;
            }
        }
    }
}

/// The table's text for search: its field names, then each record's values, a line each,
/// until `MAX_TEXT`.
pub fn search_text(path: &Path, _max: u64) -> Option<String> {
    let (mut f, t) = open(path)?;
    let mut out = t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(" ");
    // Record by record, until the text is as long as the store keeps.
    t.each(&mut f, |row| {
        out.push('\n');
        out.push_str(&row.join(" "));
        out.len() < crate::extract::MAX_TEXT
    });
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A dBase III table with a name, a number, a date and a yes/no, in code page 850.
    pub fn table(rows: &[(&[u8], &str, &str, bool)]) -> Vec<u8> {
        let fields: [(&str, u8, u8, u8); 4] = [("NAME", b'C', 10, 0), ("MASS", b'N', 8, 2), ("LAUNCHED", b'D', 8, 0), ("FLOWN", b'L', 1, 0)];
        let record_len = 1 + 10 + 8 + 8 + 1;
        let start = 32 + 32 * fields.len() + 1;
        let mut b = vec![0u8; 32];
        b[0] = 0x03;
        b[1..4].copy_from_slice(&[126, 10, 19]);
        b[4..8].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        b[8..10].copy_from_slice(&(start as u16).to_le_bytes());
        b[10..12].copy_from_slice(&(record_len as u16).to_le_bytes());
        b[29] = 0x02;
        for (n, k, l, d) in fields {
            let mut desc = [0u8; 32];
            desc[..n.len()].copy_from_slice(n.as_bytes());
            desc[11] = k;
            desc[16] = l;
            desc[17] = d;
            b.extend(desc);
        }
        b.push(0x0d);
        for (i, (name, mass, date, flown)) in rows.iter().enumerate() {
            b.push(if i == 1 { b'*' } else { b' ' });
            let mut n = name.to_vec();
            n.resize(10, b' ');
            b.extend(n);
            b.extend(format!("{mass:>8}").as_bytes());
            b.extend(date.as_bytes());
            b.push(if *flown { b'T' } else { b'F' });
        }
        b.push(0x1a);
        b
    }

    #[test]
    fn dbf_fields_records_and_code_page() {
        let d = crate::pictures::tests::folder("dbf");
        let p = d.join("fleet.dbf");
        // "Ørsted" in code page 850 is 0x9D then "rsted".
        std::fs::write(&p, table(&[(b"\x9drsted", "12.50", "19690716", true), (b"gone", "1.00", "19700101", false), (b"Vostok", "4.73", "19610412", false)])).unwrap();
        let (mut f, t) = open(&p).unwrap();
        assert_eq!((t.records, t.fields.len(), t.code_page, t.updated.as_str()), (3, 4, "850", "2026-10-19"));
        assert_eq!(t.schema(), "NAME C(10), MASS N(8,2), LAUNCHED D(8), FLOWN L(1)");
        let rows = t.rows(&mut f, 10);
        assert_eq!(rows, [vec!["Ørsted", "12.50", "1969-07-16", "✓"], vec!["Vostok", "4.73", "1961-04-12", "✗"]], "the deleted one left out");
        let text = search_text(&p, u64::MAX).unwrap();
        assert!(text.starts_with("NAME MASS LAUNCHED FLOWN") && text.contains("Vostok"));
        // A shapefile's .cpg wins over the header's byte.
        std::fs::write(d.join("fleet.cpg"), "UTF-8").unwrap();
        assert_eq!(open(&p).unwrap().1.code_page, "UTF-8");
        // A .cpg only in the cloud is not read (that would download it): the header's byte.
        crate::cloud::pretend(&d.join("fleet.cpg"), true);
        assert_eq!(open(&p).unwrap().1.code_page, "850");
        crate::cloud::pretend(&d.join("fleet.cpg"), false);
        assert!(open(&d.join("fleet.cpg")).is_none(), "text is no table");
        // Fields of no length and a record of none: no table (it would have no deleted mark).
        let mut empty = table(&[(b"Ada", "1.00", "20261019", true)]);
        empty[10..12].copy_from_slice(&0u16.to_le_bytes());
        for d in 0..4 {
            empty[32 + d * 32 + 16] = 0;
        }
        std::fs::write(&p, &empty).unwrap();
        assert!(open(&p).is_none());
        // Search's text stops at its limit, however many records there are.
        let many: Vec<(&[u8], &str, &str, bool)> = vec![(b"Vostok", "4.73", "19610412", true); 200_000];
        std::fs::write(&p, table(&many)).unwrap();
        let text = search_text(&p, u64::MAX).unwrap();
        assert!(text.len() < crate::extract::MAX_TEXT + 100, "{}", text.len());
        let good = table(&[(b"Ada", "1.00", "20261019", true)]);
        crate::pictures::tests::hostile(&good, |b| {
            std::fs::write(&p, b).unwrap();
            if let Some((mut f, t)) = open(&p) {
                let _ = t.rows(&mut f, 10);
            }
        });
        std::fs::remove_dir_all(d).unwrap();
    }
}

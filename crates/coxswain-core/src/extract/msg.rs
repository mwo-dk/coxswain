//! Outlook messages (`.msg`): a compound file (OLE) with a stream for each property. Its
//! subject, who it is from and to, its date, its body (plain text, else the HTML made plain,
//! else the compressed RTF unpacked and read), and its attachments' names and sizes.

use std::io::Read;
use std::path::Path;

use super::MAX_TEXT;
use super::mail::Summary;

/// The most bytes read of one property.
const MAX_PROPERTY: u64 = 16 << 20;
/// The most attachments listed.
const MAX_ATTACHMENTS: usize = 500;

type Cfb = cfb::CompoundFile<std::fs::File>;

/// A property's stream under `dir` ("" for the message), of the types it may have: Unicode
/// text (001F), 8-bit text (001E) or bytes (0102).
fn property(f: &mut Cfb, dir: &str, id: u16) -> Option<Vec<u8>> {
    for kind in ["001F", "001E", "0102"] {
        let path = format!("{dir}/__substg1.0_{id:04X}{kind}");
        if let Ok(s) = f.open_stream(&path) {
            let mut out = vec![];
            s.take(MAX_PROPERTY).read_to_end(&mut out).ok()?;
            return Some(if kind == "001F" { utf16(&out).into_bytes() } else { out });
        }
    }
    None
}

fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
}

fn string(f: &mut Cfb, dir: &str, id: u16) -> String {
    property(f, dir, id).map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default()
}

/// The time it was delivered, else sent, as seconds since 1970: from the fixed-size
/// properties' stream (a FILETIME, 100 ns since 1601).
fn when(f: &mut Cfb) -> Option<i64> {
    let mut b = vec![];
    f.open_stream("/__properties_version1.0").ok()?.take(1 << 20).read_to_end(&mut b).ok()?;
    // The message's own stream has a header of 32 bytes, then entries of 16.
    let find = |tag: u32| {
        b.get(32..)?.chunks_exact(16).find(|e| u32::from_le_bytes([e[0], e[1], e[2], e[3]]) == tag).map(|e| u64::from_le_bytes(e[8..16].try_into().unwrap_or_default()))
    };
    let t = find(0x0e06_0040).or_else(|| find(0x0039_0040))?;
    Some((t / 10_000_000) as i64 - 11_644_473_600)
}

/// Compressed RTF (MS-OXRTFCP): LZ77 over a dictionary of 4096 bytes that starts with these.
const RTF_START: &[u8; 207] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

/// The RTF of a compressed RTF property, no more than `MAX_PROPERTY` of it.
pub(crate) fn unpack_rtf(b: &[u8]) -> Option<Vec<u8>> {
    let raw = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    let kind = b.get(8..12)?;
    let data = &b[16.min(b.len())..];
    if kind == b"MELA" {
        return Some(data[..data.len().min(raw)].to_vec());
    }
    if kind != b"LZFu" {
        return None;
    }
    let want = raw.min(MAX_PROPERTY as usize);
    let mut dict = [0u8; 4096];
    dict[..207].copy_from_slice(RTF_START);
    let (mut write, mut at, mut out) = (207usize, 0usize, Vec::with_capacity(want.min(1 << 20)));
    'all: while at < data.len() && out.len() < want {
        let control = data[at];
        at += 1;
        for bit in 0..8 {
            if out.len() >= want || at >= data.len() {
                break 'all;
            }
            if control & (1 << bit) == 0 {
                let c = data[at];
                at += 1;
                out.push(c);
                dict[write] = c;
                write = (write + 1) % 4096;
            } else {
                let Some(r) = data.get(at..at + 2) else { break 'all };
                at += 2;
                let r = u16::from_be_bytes([r[0], r[1]]) as usize;
                let (offset, len) = (r >> 4, (r & 0xf) + 2);
                if offset == write {
                    break 'all;
                }
                for i in 0..len {
                    let c = dict[(offset + i) % 4096];
                    out.push(c);
                    dict[write] = c;
                    write = (write + 1) % 4096;
                }
            }
        }
    }
    Some(out)
}

/// The message in the file, `None` for a file that is no Outlook message (or one only in the
/// cloud: reading it would download it).
pub fn summary(path: &Path) -> Option<Summary> {
    if crate::cloud::unasked(path) {
        return None;
    }
    // `cfb` checks the file's tables; a panic in it is a file that cannot be read.
    std::panic::catch_unwind(|| read(path)).ok().flatten()
}

fn read(path: &Path) -> Option<Summary> {
    let mut f = cfb::CompoundFile::open(std::fs::File::open(path).ok()?).ok()?;
    let subject = string(&mut f, "", 0x0037);
    let name = string(&mut f, "", 0x0c1a);
    let address = [0x5d01, 0x0c1f].into_iter().map(|id| string(&mut f, "", id)).find(|a| a.contains('@')).unwrap_or_default();
    let from = match (name.is_empty(), address.is_empty()) {
        (false, false) if name != address => format!("{name} <{address}>"),
        (true, _) => address,
        _ => name,
    };
    let to = string(&mut f, "", 0x0e04);
    let date = when(&mut f).and_then(|s| chrono::DateTime::from_timestamp(s, 0)).map(|d| d.to_rfc2822()).unwrap_or_default();
    let body = property(&mut f, "", 0x1000).map(|b| String::from_utf8_lossy(&b).into_owned()).filter(|t| !t.trim().is_empty());
    let text = body
        .or_else(|| property(&mut f, "", 0x1013).map(|h| super::mail::plain(&String::from_utf8_lossy(&h))))
        .or_else(|| property(&mut f, "", 0x1009).and_then(|r| unpack_rtf(&r)).and_then(|r| super::rtf::read(&r)))
        .unwrap_or_default();
    let mut attachments = vec![];
    let dirs: Vec<String> = f
        .read_root_storage()
        .filter(|e| e.is_storage() && e.name().starts_with("__attach_version1.0_"))
        .map(|e| format!("/{}", e.name()))
        .take(MAX_ATTACHMENTS)
        .collect();
    for d in dirs {
        let name = [0x3707, 0x3704, 0x3001].into_iter().map(|id| string(&mut f, &d, id)).find(|n| !n.is_empty());
        let size = f.entry(format!("{d}/__substg1.0_37010102")).map(|e| e.len() as usize).unwrap_or(0);
        attachments.push((name, size));
    }
    if subject.is_empty() && from.is_empty() && text.is_empty() && attachments.is_empty() {
        return None;
    }
    let text: String = text.chars().take(MAX_TEXT).collect();
    Some(Summary { subject, from, to, date, text, attachments })
}

/// The message's text for search: its headers, its body and its attachments' names.
pub fn text(path: &Path, _max: u64) -> Option<String> {
    let m = summary(path)?;
    let names = m.attachments.iter().filter_map(|(n, _)| n.clone()).collect::<Vec<_>>().join("\n");
    Some([m.subject, m.from, m.to, m.date, m.text, names].join("\n"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// An Outlook message as Outlook writes one, with the properties that are read.
    pub fn message(path: &Path) {
        let mut f = cfb::CompoundFile::create(std::fs::File::create(path).unwrap()).unwrap();
        let mut put = |p: &str, b: &[u8]| f.create_stream(p).unwrap().write_all(b).unwrap();
        put("/__substg1.0_0037001F", &utf16le("Launch window"));
        put("/__substg1.0_0C1A001F", &utf16le("Ada Lovelace"));
        put("/__substg1.0_5D01001F", &utf16le("ada@example.com"));
        put("/__substg1.0_0E04001F", &utf16le("Mission Control"));
        put("/__substg1.0_1000001F", &utf16le("Go for launch at 09:00."));
        let mut props = vec![0u8; 32];
        props.extend(0x0e06_0040u32.to_le_bytes());
        props.extend(6u32.to_le_bytes());
        // 2026-10-19 08:00:00 UTC as a FILETIME.
        props.extend(((1_792_396_800u64 + 11_644_473_600) * 10_000_000).to_le_bytes());
        put("/__properties_version1.0", &props);
        f.create_storage("/__attach_version1.0_#00000000").unwrap();
        let mut put = |p: &str, b: &[u8]| f.create_stream(p).unwrap().write_all(b).unwrap();
        put("/__attach_version1.0_#00000000/__substg1.0_3707001F", &utf16le("trajectory.pdf"));
        put("/__attach_version1.0_#00000000/__substg1.0_37010102", &[7u8; 1234]);
        f.flush().unwrap();
    }

    #[test]
    fn outlook_message_and_its_attachment() {
        let d = crate::pictures::tests::folder("msg");
        let p = d.join("launch.msg");
        message(&p);
        let m = summary(&p).unwrap();
        assert_eq!((m.subject.as_str(), m.from.as_str(), m.to.as_str()), ("Launch window", "Ada Lovelace <ada@example.com>", "Mission Control"));
        assert!(m.date.contains("19 Oct 2026"), "{}", m.date);
        assert_eq!(m.text, "Go for launch at 09:00.");
        assert_eq!(m.attachments, [(Some("trajectory.pdf".to_string()), 1234)]);
        assert!(text(&p, u64::MAX).unwrap().contains("trajectory.pdf"));
        let bytes = std::fs::read(&p).unwrap();
        crate::pictures::tests::hostile(&bytes, |b| {
            std::fs::write(&p, b).unwrap();
            let _ = summary(&p);
        });
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn compressed_rtf_unpacks() {
        // The example of MS-OXRTFCP 3.1.1: "{\rtf1\ansi\ansicpg1252\pard hello world}\r\n".
        let b: [u8; 61] = [
            0x2d, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xf1, 0xc5, 0xc7, 0xa7, 0x03, 0x00, 0x0a, 0x00, 0x72, 0x63, 0x70, 0x67,
            0x31, 0x32, 0x35, 0x42, 0x32, 0x0a, 0xf3, 0x20, 0x68, 0x65, 0x6c, 0x09, 0x00, 0x20, 0x62, 0x77, 0x05, 0xb0, 0x6c, 0x64, 0x7d, 0x0a, 0x80, 0x0f,
            0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let rtf = unpack_rtf(&b).unwrap();
        assert_eq!(String::from_utf8_lossy(&rtf), "{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n");
        crate::pictures::tests::hostile(&b, |b| {
            let _ = unpack_rtf(b);
        });
    }
}

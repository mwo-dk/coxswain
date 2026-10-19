//! Tracker music (`.mod`, `.xm`, `.s3m`, `.it`): what a module holds, read from its headers,
//! for both apps' facts. Nothing is played. A `.mod` is a module only when its content says
//! so (a tag at byte 1080): Modula-2 source has the same ending and stays text.

use std::io::Read;
use std::path::Path;

use crate::{t, tn};

/// The most bytes read of a module: headers and instrument names, past the sample data of
/// an XM, which comes between them.
const MAX_READ: u64 = 64 << 20;
/// The most instrument or sample names listed.
const MAX_NAMES: usize = 256;

/// What a module holds.
#[derive(Debug, Default, PartialEq)]
pub struct Module {
    pub format: String,
    pub title: String,
    pub channels: usize,
    pub patterns: usize,
    pub instruments: usize,
    pub samples: usize,
    /// Entries in its order list: how many patterns are played, one after the other.
    pub positions: usize,
    pub tracker: String,
    /// The instruments' or samples' names that are not empty: often a message from its maker.
    pub names: Vec<String>,
}

/// An ending a module has. No file is read.
pub fn named(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["mod", "xm", "s3m", "it"].contains(&e.to_ascii_lowercase().as_str()))
}

/// The module at `path`, `None` for a file that is none (or only in the cloud: reading it would
/// download it).
pub fn read(path: &Path) -> Option<Module> {
    if !named(path) || crate::cloud::unasked(path) {
        return None;
    }
    let mut b = vec![];
    std::fs::File::open(path).ok()?.take(MAX_READ).read_to_end(&mut b).ok()?;
    parse(&b)
}

/// The module in these bytes, of any of the four kinds, by their content.
pub fn parse(b: &[u8]) -> Option<Module> {
    if b.starts_with(b"Extended Module: ") {
        xm(b)
    } else if b.get(44..48) == Some(b"SCRM") {
        s3m(b)
    } else if b.starts_with(b"IMPM") {
        it(b)
    } else {
        protracker(b)
    }
}

/// A name of fixed length: up to its first NUL, the characters a terminal could take for
/// commands left out, trimmed. Old trackers wrote code page 437, which is ASCII for names.
fn name(b: &[u8]) -> String {
    let b = b.split(|c| *c == 0).next().unwrap_or_default();
    b.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { ' ' }).collect::<String>().trim().to_string()
}

fn u16le(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?).into())
}

fn u32le(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

fn named_list(names: impl Iterator<Item = String>) -> Vec<String> {
    names.filter(|n| !n.is_empty()).take(MAX_NAMES).collect()
}

/// ProTracker and its kin: 31 samples, and a tag at 1080 that says how many channels.
fn protracker(b: &[u8]) -> Option<Module> {
    let tag = b.get(1080..1084)?;
    let channels = match tag {
        b"M.K." | b"M!K!" | b"M&K!" | b"N.T." | b"FLT4" | b"4CHN" => 4,
        b"FLT8" | b"CD81" | b"OKTA" | b"OCTA" | b"8CHN" => 8,
        [d, b'C', b'H', b'N'] if d.is_ascii_digit() => usize::from(d - b'0'),
        [a, b, b'C', b'H' | b'N'] if a.is_ascii_digit() && b.is_ascii_digit() => usize::from((a - b'0') * 10 + (b - b'0')),
        [b'T', b'D', b'Z', d] if d.is_ascii_digit() => usize::from(d - b'0'),
        _ => return None,
    };
    let positions = usize::from(*b.get(950)?).min(128);
    let orders = b.get(952..1080)?;
    let patterns = orders.iter().map(|p| usize::from(*p)).max().unwrap_or(0) + 1;
    let samples = (0..31).filter(|i| u16::from_be_bytes([b[20 + i * 30 + 22], b[20 + i * 30 + 23]]) > 0).count();
    Some(Module {
        format: format!("ProTracker MOD ({})", String::from_utf8_lossy(tag)),
        title: name(&b[..20]),
        channels,
        patterns,
        instruments: 0,
        samples,
        positions,
        tracker: String::new(),
        names: named_list((0..31).map(|i| name(&b[20 + i * 30..20 + i * 30 + 22]))),
    })
}

/// FastTracker 2: the header, then each pattern, then each instrument with its samples.
fn xm(b: &[u8]) -> Option<Module> {
    let header = u32le(b, 60)?;
    let positions = u16le(b, 64)?;
    let channels = u16le(b, 68)?;
    let patterns = u16le(b, 70)?;
    let instruments = u16le(b, 72)?;
    let mut at = 60usize.checked_add(header)?;
    for _ in 0..patterns.min(256) {
        let len = u32le(b, at)?;
        let packed = u16le(b, at + 7)?;
        at = at.checked_add(len)?.checked_add(packed)?;
    }
    let (mut names, mut samples) = (vec![], 0);
    for _ in 0..instruments.min(256) {
        let Some(size) = u32le(b, at) else { break };
        let Some(raw) = b.get(at + 4..at + 26) else { break };
        names.push(name(raw));
        let n = u16le(b, at + 27).unwrap_or(0);
        let sample_header = if n > 0 { u32le(b, at + 29).unwrap_or(40) } else { 40 };
        samples += n;
        let mut next = at.checked_add(size.max(29))?;
        let mut data = 0usize;
        for s in 0..n.min(16) {
            let h = next.checked_add(s * sample_header)?;
            data = data.saturating_add(u32le(b, h).unwrap_or(0));
            if let Some(raw) = b.get(h + 18..h + 40) {
                names.push(name(raw));
            }
        }
        next = next.checked_add(n.min(16) * sample_header)?.checked_add(data)?;
        at = next;
    }
    Some(Module {
        format: "FastTracker 2 XM".into(),
        title: name(b.get(17..37)?),
        channels,
        patterns,
        instruments,
        samples,
        positions,
        tracker: name(b.get(38..58)?),
        names: named_list(names.into_iter()),
    })
}

/// Scream Tracker 3: a header, its order list, and a pointer to each instrument (sample).
fn s3m(b: &[u8]) -> Option<Module> {
    let orders = u16le(b, 32)?;
    let ins = u16le(b, 34)?;
    let patterns = u16le(b, 36)?;
    let channels = b.get(64..96)?.iter().filter(|c| **c < 16).count();
    let list = b.get(96..96 + orders)?;
    let positions = list.iter().filter(|o| **o < 254).count();
    let names = (0..ins.min(MAX_NAMES)).filter_map(|i| {
        let at = u16le(b, 96 + orders + i * 2)? * 16;
        Some(name(b.get(at + 48..at + 76)?))
    });
    let version = u16le(b, 40).unwrap_or(0);
    Some(Module {
        format: "Scream Tracker 3 S3M".into(),
        title: name(&b[..28]),
        channels,
        patterns,
        instruments: 0,
        samples: ins,
        positions,
        tracker: tracker_version(version),
        names: named_list(names),
    })
}

/// Who made an S3M or an IT, from the version it carries.
fn tracker_version(v: usize) -> String {
    let (who, n) = (v >> 12, v & 0xfff);
    let number = format!("{}.{:02x}", n >> 8, n & 0xff);
    match who {
        1 => format!("Scream Tracker {number}"),
        2 => format!("Imago Orpheus {number}"),
        3 => format!("Impulse Tracker {number}"),
        4 => "Schism Tracker".into(),
        5 => "OpenMPT".into(),
        _ => String::new(),
    }
}

/// Impulse Tracker: a header, its order list, then pointers to instruments, samples, patterns.
fn it(b: &[u8]) -> Option<Module> {
    let orders = u16le(b, 0x20)?;
    let ins = u16le(b, 0x22)?;
    let smp = u16le(b, 0x24)?;
    let patterns = u16le(b, 0x26)?;
    let flags = u16le(b, 0x2c)?;
    let channels = b.get(0x40..0x80)?.iter().filter(|p| **p & 0x80 == 0).count();
    let list = b.get(0xc0..0xc0 + orders)?;
    let positions = list.iter().filter(|o| **o < 254).count();
    let pointers = 0xc0 + orders;
    let at = |i: usize| u32le(b, pointers + i * 4);
    // With instruments their names are listed, else those of the samples.
    let uses_instruments = flags & 4 != 0 && ins > 0;
    let names: Vec<String> = if uses_instruments {
        (0..ins.min(MAX_NAMES)).filter_map(|i| Some(name(b.get(at(i)? + 0x20..at(i)? + 0x3a)?))).collect()
    } else {
        (0..smp.min(MAX_NAMES)).filter_map(|i| Some(name(b.get(at(ins + i)? + 0x14..at(ins + i)? + 0x2e)?))).collect()
    };
    Some(Module {
        format: "Impulse Tracker IT".into(),
        title: name(b.get(4..30)?),
        channels,
        patterns,
        instruments: if uses_instruments { ins } else { 0 },
        samples: smp,
        positions,
        tracker: tracker_version(u16le(b, 0x28).unwrap_or(0)),
        names: named_list(names.into_iter()),
    })
}

/// A module's facts as (label, value) lines.
pub fn facts(m: &Module) -> Vec<(String, String)> {
    let mut out = vec![(t!("facts.format"), m.format.clone())];
    if !m.title.is_empty() {
        out.push((t!("facts.title"), m.title.clone()));
    }
    out.push((t!("facts.channel_count"), m.channels.to_string()));
    out.push((t!("facts.patterns"), m.patterns.to_string()));
    if m.instruments > 0 {
        out.push((t!("facts.instruments"), m.instruments.to_string()));
    }
    out.push((t!("facts.samples"), m.samples.to_string()));
    out.push((t!("facts.length"), tn!("facts.positions", m.positions as u64)));
    if !m.tracker.is_empty() {
        out.push((t!("facts.made_with"), m.tracker.clone()));
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ProTracker module of four channels, titled, with two named samples and 3 positions.
    pub fn protracker_bytes() -> Vec<u8> {
        let mut b = vec![0u8; 1084 + 1024];
        b[..6].copy_from_slice(b"launch");
        for (i, n) in ["kick", "greetings to all"].iter().enumerate() {
            b[20 + i * 30..20 + i * 30 + n.len()].copy_from_slice(n.as_bytes());
            b[20 + i * 30 + 22..20 + i * 30 + 24].copy_from_slice(&100u16.to_be_bytes());
        }
        b[950] = 3;
        b[952..955].copy_from_slice(&[0, 1, 0]);
        b[1080..1084].copy_from_slice(b"M.K.");
        b
    }

    #[test]
    fn modules_of_each_kind() {
        let m = parse(&protracker_bytes()).unwrap();
        assert_eq!((m.title.as_str(), m.channels, m.patterns, m.samples, m.positions), ("launch", 4, 2, 2, 3));
        assert_eq!(m.names, ["kick", "greetings to all"]);
        // Modula-2 source is no module.
        assert!(parse(b"MODULE Hello;\nBEGIN\nEND Hello.\n").is_none());

        let mut xm = b"Extended Module: Orbit".to_vec();
        xm.resize(37, 0);
        xm.push(0x1a);
        xm.extend(b"FastTracker v2.00   ");
        xm.extend(0x0104u16.to_le_bytes());
        xm.extend(276u32.to_le_bytes());
        xm.extend(2u16.to_le_bytes());
        xm.extend(0u16.to_le_bytes());
        xm.extend(8u16.to_le_bytes());
        xm.extend(1u16.to_le_bytes());
        xm.extend(1u16.to_le_bytes());
        xm.resize(60 + 276, 0);
        // One empty pattern, then one instrument without samples.
        xm.extend(9u32.to_le_bytes());
        xm.extend([0, 64, 0, 0, 0]);
        let mut ins = 29u32.to_le_bytes().to_vec();
        ins.extend(b"lead synth");
        ins.resize(4 + 22, 0);
        ins.extend([0, 0, 0]);
        xm.extend(ins);
        let m = parse(&xm).unwrap();
        assert_eq!((m.title.as_str(), m.channels, m.patterns, m.instruments, m.tracker.as_str()), ("Orbit", 8, 1, 1, "FastTracker v2.00"));
        assert_eq!(m.names, ["lead synth"]);

        let mut s3m = b"Space".to_vec();
        s3m.resize(28, 0);
        s3m.extend([0x1a, 16, 0, 0]);
        s3m.extend(3u16.to_le_bytes());
        s3m.extend(1u16.to_le_bytes());
        s3m.extend(1u16.to_le_bytes());
        s3m.extend([0, 0]);
        s3m.extend(0x1320u16.to_le_bytes());
        s3m.resize(44, 0);
        s3m.extend(b"SCRM");
        s3m.resize(64, 0);
        s3m.extend([0, 1, 2, 3]);
        s3m.extend([255; 28]);
        s3m.extend([0, 255, 254]);
        // The instrument at paragraph 7 (byte 112).
        s3m.extend(7u16.to_le_bytes());
        s3m.resize(112 + 48, 0);
        s3m.extend(b"bass drum");
        s3m.resize(112 + 80, 0);
        let m = parse(&s3m).unwrap();
        assert_eq!((m.channels, m.positions, m.samples, m.tracker.as_str()), (4, 1, 1, "Scream Tracker 3.20"));
        assert_eq!(m.names, ["bass drum"]);
        assert_eq!(facts(&m).len(), 7);

        for b in [protracker_bytes(), xm, s3m] {
            crate::pictures::tests::hostile(&b, |b| {
                let _ = parse(b);
            });
        }
    }
}

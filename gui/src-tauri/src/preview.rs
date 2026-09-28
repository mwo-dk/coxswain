//! Previews that need Rust: a file's git diff, SQLite databases, EPUB books, and facts for
//! the preview pane (photo EXIF, audio tags, what an executable was built for).

use bosum_core::{t, tn};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

type Res<T> = Result<T, String>;

// ---------------------------------------------------------------- git diff

/// The file's changes against HEAD (staged and unstaged), or `None` when there are none.
#[tauri::command]
pub async fn git_diff(path: PathBuf) -> Res<Option<String>> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Ok(None) };
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "--no-color", "--no-ext-diff", "HEAD", "--"])
        .arg(name)
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok((out.status.success() && !text.trim().is_empty()).then(|| text.chars().take(512 * 1024).collect()))
}

// ---------------------------------------------------------------- SQLite

#[derive(Serialize)]
pub struct DbTable {
    name: String,
    kind: String,
    /// `None` for views, or when counting took too long.
    rows: Option<u64>,
    sql: String,
}

/// Tables and views with their row counts and schema. Opened read-only; counting stops after
/// about a second per table on huge databases.
#[tauri::command]
pub async fn sqlite_info(path: PathBuf) -> Res<Vec<DbTable>> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT name, type, coalesce(sql, '') FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .map_err(|e| e.to_string())?;
    let mut tables: Vec<DbTable> = stmt
        .query_map([], |r| Ok(DbTable { name: r.get(0)?, kind: r.get(1)?, rows: None, sql: r.get(2)? }))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    for t in tables.iter_mut().filter(|t| t.kind == "table") {
        let start = std::time::Instant::now();
        // Without the hook, counting simply has no time limit.
        let _ = db.progress_handler(10_000, Some(move || start.elapsed().as_millis() > 1000));
        let q = format!("SELECT count(*) FROM \"{}\"", t.name.replace('"', "\"\""));
        t.rows = db.query_row(&q, [], |r| r.get::<_, i64>(0)).ok().map(|n| n as u64);
    }
    Ok(tables)
}

// ---------------------------------------------------------------- EPUB

#[derive(Serialize)]
pub struct Book {
    title: String,
    /// The first chapter with real text, as XHTML body content. The UI sanitizes it.
    html: String,
}

#[tauri::command]
pub async fn epub_preview(path: PathBuf) -> Res<Book> {
    let mut z = zip::ZipArchive::new(BufReader::new(File::open(&path).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    let mut read = |name: &str| -> Res<String> {
        let mut s = String::new();
        z.by_name(name).map_err(|e| format!("{name}: {e}"))?.take(4 << 20).read_to_string(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    };
    let attr = |tag: &str, name: &str| -> Option<String> {
        regex::Regex::new(&format!(r#"\b{name}\s*=\s*["']([^"']*)["']"#)).ok()?.captures(tag).map(|c| c[1].to_string())
    };
    let container = read("META-INF/container.xml")?;
    let opf_path = regex::Regex::new(r"<rootfile\b[^>]*>").unwrap().find(&container).and_then(|m| attr(m.as_str(), "full-path")).ok_or_else(|| t!("err.epub_no_rootfile"))?;
    let opf = read(&opf_path)?;
    let base = Path::new(&opf_path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let title = regex::Regex::new(r"(?s)<dc:title[^>]*>(.*?)</dc:title>").unwrap().captures(&opf).map(|c| c[1].trim().to_string()).unwrap_or_default();
    let items: Vec<(String, String)> = regex::Regex::new(r"<item\b[^>]*>")
        .unwrap()
        .find_iter(&opf)
        .filter_map(|m| Some((attr(m.as_str(), "id")?, attr(m.as_str(), "href")?)))
        .collect();
    let body = regex::Regex::new(r"(?s)<body[^>]*>(.*)</body>").unwrap();
    let tags = regex::Regex::new(r"(?s)<[^>]*>").unwrap();
    for m in regex::Regex::new(r"<itemref\b[^>]*>").unwrap().find_iter(&opf) {
        let Some(id) = attr(m.as_str(), "idref") else { continue };
        let Some((_, href)) = items.iter().find(|(i, _)| *i == id) else { continue };
        let file = if base.is_empty() { href.clone() } else { format!("{base}/{href}") };
        let Ok(doc) = read(&file) else { continue };
        let html = body.captures(&doc).map_or(doc.clone(), |c| c[1].to_string());
        // Skip covers and title pages: the first document with some real text wins.
        if tags.replace_all(&html, "").split_whitespace().count() > 60 {
            return Ok(Book { title, html });
        }
    }
    Err(t!("err.no_readable_chapter"))
}

// ---------------------------------------------------------------- certificates

#[derive(Serialize)]
pub struct Cert {
    subject: String,
    issuer: String,
    not_before: String,
    not_after: String,
    /// Seconds until expiry; negative once expired.
    expires_in: i64,
    names: Vec<String>,
    serial: String,
    is_ca: bool,
}

/// Every certificate in a PEM file (a chain has several), or the one in a DER file.
#[tauri::command]
pub async fn cert_info(path: PathBuf) -> Res<Vec<Cert>> {
    use x509_parser::prelude::*;
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let now = ASN1Time::now().timestamp();
    let describe = |c: &X509Certificate| {
        let names = c
            .subject_alternative_name()
            .ok()
            .flatten()
            .map(|san| {
                san.value
                    .general_names
                    .iter()
                    .filter_map(|n| match n {
                        GeneralName::DNSName(d) => Some(d.to_string()),
                        GeneralName::RFC822Name(m) => Some(m.to_string()),
                        GeneralName::IPAddress(ip) if ip.len() == 4 => Some(ip.iter().map(u8::to_string).collect::<Vec<_>>().join(".")),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Cert {
            subject: c.subject().to_string(),
            issuer: c.issuer().to_string(),
            not_before: c.validity().not_before.to_string(),
            not_after: c.validity().not_after.to_string(),
            expires_in: c.validity().not_after.timestamp() - now,
            names,
            serial: c.raw_serial_as_string(),
            is_ca: c.is_ca(),
        }
    };
    if data.starts_with(b"-----BEGIN") || data.windows(10).any(|w| w == b"-----BEGIN") {
        let mut out = vec![];
        for pem in Pem::iter_from_buffer(&data).flatten() {
            if pem.label == "CERTIFICATE" {
                if let Ok(c) = pem.parse_x509() {
                    out.push(describe(&c));
                }
            }
        }
        return if out.is_empty() { Err(t!("err.no_certificate")) } else { Ok(out) };
    }
    let (_, c) = parse_x509_certificate(&data).map_err(|e| e.to_string())?;
    Ok(vec![describe(&c)])
}

// ---------------------------------------------------------------- e-mail

#[derive(Serialize)]
pub struct Mail {
    subject: String,
    from: String,
    to: String,
    date: String,
    text: String,
    attachments: Vec<(String, usize)>,
}

#[tauri::command]
pub async fn mail_preview(path: PathBuf) -> Res<Mail> {
    use mail_parser::{Address, MessageParser, MimeHeaders};
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let m = MessageParser::default().parse(&data[..]).ok_or_else(|| t!("err.not_email"))?;
    let who = |a: Option<&Address>| {
        a.map(|a| {
            a.iter()
                .map(|x| match (&x.name, &x.address) {
                    (Some(n), Some(e)) => format!("{n} <{e}>"),
                    (None, Some(e)) => e.to_string(),
                    (Some(n), None) => n.to_string(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
    };
    Ok(Mail {
        subject: m.subject().unwrap_or("").to_string(),
        from: who(m.from()),
        to: who(m.to()),
        date: m.date().map(|d| d.to_rfc822()).unwrap_or_default(),
        text: m.body_text(0).map(|t| t.chars().take(200_000).collect()).unwrap_or_default(),
        attachments: m.attachments().map(|a| (a.attachment_name().map_or_else(|| t!("facts.unnamed"), str::to_string), a.contents().len())).collect(),
    })
}

// ---------------------------------------------------------------- property lists

/// A macOS property list (binary or XML) as XML text.
#[tauri::command]
pub async fn plist_xml(path: PathBuf) -> Res<String> {
    let v = plist::Value::from_file(&path).map_err(|e| e.to_string())?;
    let mut out = vec![];
    v.to_writer_xml(&mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

// ---------------------------------------------------------------- facts

/// Label and value pairs for the preview pane: EXIF for photos, tags and format for audio,
/// target and kind for executables. Empty for anything else.
#[tauri::command]
pub async fn file_facts(path: PathBuf) -> Vec<(String, String)> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "tif" | "tiff" | "heic" | "heif" | "png" | "webp" | "avif" | "dng" => exif_facts(&path),
        "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wav" | "aiff" | "wv" | "ape" => audio_facts(&path),
        _ => binary_facts(&path),
    }
}

fn exif_facts(path: &Path) -> Vec<(String, String)> {
    use exif::{In, Tag};
    let Ok(f) = File::open(path) else { return vec![] };
    let Ok(ex) = exif::Reader::new().read_from_container(&mut BufReader::new(f)) else { return vec![] };
    let get = |t: Tag| ex.get_field(t, In::PRIMARY).map(|f| f.display_value().with_unit(&ex).to_string().trim_matches('"').to_string());
    let mut out: Vec<(String, String)> = [
        (t!("facts.camera"), [get(Tag::Make), get(Tag::Model)].into_iter().flatten().collect::<Vec<_>>().join(" ")),
        (t!("facts.lens"), get(Tag::LensModel).unwrap_or_default()),
        (t!("facts.taken"), get(Tag::DateTimeOriginal).unwrap_or_default()),
        (t!("facts.exposure"), get(Tag::ExposureTime).unwrap_or_default()),
        (t!("facts.aperture"), get(Tag::FNumber).unwrap_or_default()),
        (t!("facts.iso"), get(Tag::PhotographicSensitivity).unwrap_or_default()),
        (t!("facts.focal_length"), get(Tag::FocalLength).unwrap_or_default()),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
        .collect();
    // GPS as decimal degrees, which maps and search boxes accept.
    let deg = |t: Tag, r: Tag| {
        let v = ex.get_field(t, In::PRIMARY)?;
        let exif::Value::Rational(ref p) = v.value else { return None };
        let d = p.first()?.to_f64() + p.get(1)?.to_f64() / 60.0 + p.get(2)?.to_f64() / 3600.0;
        let neg = ex.get_field(r, In::PRIMARY).is_some_and(|f| matches!(f.display_value().to_string().as_str(), "S" | "W"));
        Some(if neg { -d } else { d })
    };
    if let (Some(lat), Some(lon)) = (deg(Tag::GPSLatitude, Tag::GPSLatitudeRef), deg(Tag::GPSLongitude, Tag::GPSLongitudeRef)) {
        out.push((t!("facts.location"), format!("{lat:.5}, {lon:.5}")));
    }
    out
}

fn audio_facts(path: &Path) -> Vec<(String, String)> {
    use lofty::prelude::*;
    let Ok(file) = lofty::read_from_path(path) else { return vec![] };
    let mut out = vec![];
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        let mut add = |k: &str, v: Option<String>| {
            if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
                out.push((t!(k), v));
            }
        };
        add("facts.title", tag.title().map(|s| s.to_string()));
        add("facts.artist", tag.artist().map(|s| s.to_string()));
        add("facts.album", tag.album().map(|s| s.to_string()));
        add("facts.year", tag.date().map(|d| d.year.to_string()));
        add("facts.track", tag.track().map(|n| n.to_string()));
        add("facts.genre", tag.genre().map(|s| s.to_string()));
    }
    let p = file.properties();
    let secs = p.duration().as_secs();
    out.push((t!("facts.length"), format!("{}:{:02}", secs / 60, secs % 60)));
    if let Some(b) = p.audio_bitrate() {
        out.push((t!("facts.bitrate"), format!("{b} kbps")));
    }
    if let (Some(r), Some(c)) = (p.sample_rate(), p.channels()) {
        let channels = match c {
            1 => t!("facts.mono"),
            2 => t!("facts.stereo"),
            _ => tn!("facts.channels", c),
        };
        out.push((t!("facts.format"), t!("facts.sample_format", "rate" => format!("{:.1}", r as f64 / 1000.0), "channels" => channels)));
    }
    out
}

/// ELF, PE and Mach-O headers: which platform and CPU a binary is for, and what kind it is.
fn binary_facts(path: &Path) -> Vec<(String, String)> {
    let mut h = [0u8; 4096];
    let n = File::open(path).and_then(|mut f| f.read(&mut h)).unwrap_or(0);
    binary_kind(&h[..n]).map(|(fmt, cpu, kind)| vec![(t!("facts.executable"), format!("{fmt}, {cpu}")), (t!("facts.kind"), kind)]).unwrap_or_default()
}

fn binary_kind(h: &[u8]) -> Option<(String, String, String)> {
    let u16le = |o: usize| Some(u16::from_le_bytes(h.get(o..o + 2)?.try_into().ok()?));
    let u32le = |o: usize| Some(u32::from_le_bytes(h.get(o..o + 4)?.try_into().ok()?));
    if h.starts_with(b"\x7fELF") {
        let bits = if h.get(4) == Some(&2) { 64 } else { 32 };
        let (t, m) = if h.get(5) == Some(&1) {
            (u16le(16)?, u16le(18)?)
        } else {
            (u16::from_be_bytes(h.get(16..18)?.try_into().ok()?), u16::from_be_bytes(h.get(18..20)?.try_into().ok()?))
        };
        let cpu = match m {
            3 => "x86",
            62 => "x86-64",
            40 => "ARM",
            183 => "ARM64",
            243 => "RISC-V",
            8 => "MIPS",
            21 => "PowerPC 64",
            _ => "",
        };
        let kind = match t {
            1 => "object",
            2 => "program",
            3 => "program_or_shared",
            4 => "core_dump",
            _ => "other",
        };
        return Some((t!("facts.elf", "bits" => bits), cpu_name(cpu), kind_name(kind)));
    }
    if h.starts_with(b"MZ") {
        let pe = u32le(0x3c)? as usize;
        if h.get(pe..pe + 4)? != b"PE\0\0" {
            return Some((t!("facts.dos"), "x86".into(), kind_name("program")));
        }
        let cpu = match u16le(pe + 4)? {
            0x8664 => "x86-64",
            0x14c => "x86",
            0xaa64 => "ARM64",
            0x1c4 => "ARM",
            _ => "",
        };
        let dll = u16le(pe + 22)? & 0x2000 != 0;
        let gui = u16le(pe + 24 + 68)? == 2;
        let kind = if dll { "dll" } else if gui { "windowed" } else { "console" };
        return Some((t!("facts.windows_pe"), cpu_name(cpu), kind_name(kind)));
    }
    let magic = u32le(0)?;
    if magic == 0xfeedfacf || magic == 0xfeedface {
        let cpu = match u32le(4)? {
            0x0100_0007 => "x86-64",
            0x0100_000c => "ARM64",
            7 => "x86",
            _ => "",
        };
        let kind = match u32le(12)? {
            2 => "program",
            6 => "dylib",
            8 => "bundle",
            1 => "object",
            _ => "other",
        };
        return Some((t!("facts.macho"), cpu_name(cpu), kind_name(kind)));
    }
    // Universal ("fat") binaries are big-endian and list one slice per CPU.
    if h.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
        let n = u32::from_be_bytes(h.get(4..8)?.try_into().ok()?);
        if n > 0 && n < 10 {
            let cpus: Vec<String> = (0..n as usize)
                .filter_map(|i| {
                    let o = 8 + i * 20;
                    Some(match u32::from_be_bytes(h.get(o..o + 4)?.try_into().ok()?) {
                        0x0100_0007 => "x86-64",
                        0x0100_000c => "ARM64",
                        _ => "",
                    })
                    .map(cpu_name)
                })
                .collect();
            return Some((t!("facts.macos_universal"), cpus.join(" + "), kind_name("program_or_library")));
        }
    }
    None
}

/// A CPU name as shown; empty means one Bosum does not name.
fn cpu_name(cpu: &str) -> String {
    if cpu.is_empty() { t!("facts.other_cpu") } else { cpu.to_string() }
}

/// The text for a binary kind code: `facts.bin.<code>`.
fn kind_name(code: &str) -> String {
    t!(&format!("facts.bin.{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_kinds_from_headers() {
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[16] = 3;
        elf[18] = 62;
        assert_eq!(binary_kind(&elf).unwrap(), (t!("facts.elf", "bits" => 64), "x86-64".into(), t!("facts.bin.program_or_shared")));

        let mut pe = vec![0u8; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0xaa64u16.to_le_bytes());
        pe[0x80 + 24 + 68] = 3;
        assert_eq!(binary_kind(&pe).unwrap().2, t!("facts.bin.console"));
        assert_eq!(binary_kind(&pe).unwrap().1, "ARM64");

        let mut macho = vec![0u8; 32];
        macho[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
        macho[4..8].copy_from_slice(&0x0100_000cu32.to_le_bytes());
        macho[12] = 2;
        assert_eq!(binary_kind(&macho).unwrap(), (t!("facts.macho"), "ARM64".into(), t!("facts.bin.program")));

        assert!(binary_kind(b"hello world").is_none());
        // This test binary itself is a real executable for the host.
        let me = std::env::current_exe().unwrap();
        assert!(!binary_facts(&me).is_empty());
    }

    #[test]
    fn mail_and_plist() {
        let d = std::env::temp_dir().join(format!("bosum-test-mail-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.eml"), "From: Ada <ada@example.com>\r\nTo: bob@example.com\r\nSubject: Launch\r\nDate: Mon, 28 Sep 2026 10:00:00 +0000\r\n\r\nGo for launch.\r\n").unwrap();
        let m = tauri::async_runtime::block_on(mail_preview(d.join("m.eml"))).unwrap();
        assert_eq!((m.subject.as_str(), m.from.as_str(), m.to.as_str()), ("Launch", "Ada <ada@example.com>", "bob@example.com"));
        assert!(m.text.contains("Go for launch"));
        let mut dict = plist::Dictionary::new();
        dict.insert("Name".into(), "Bosum".into());
        plist::Value::Dictionary(dict).to_file_binary(d.join("i.plist")).unwrap();
        let xml = tauri::async_runtime::block_on(plist_xml(d.join("i.plist"))).unwrap();
        assert!(xml.contains("<key>Name</key>") && xml.contains("<string>Bosum</string>"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn sqlite_tables_and_counts() {
        let p = std::env::temp_dir().join(format!("bosum-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("CREATE TABLE launch(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO launch(name) VALUES ('a'), ('b'); CREATE VIEW v AS SELECT * FROM launch;").unwrap();
        drop(db);
        let t = tauri::async_runtime::block_on(sqlite_info(p.clone())).unwrap();
        assert_eq!((t[0].name.as_str(), t[0].rows), ("launch", Some(2)));
        assert_eq!((t[1].kind.as_str(), t[1].rows), ("view", None));
        std::fs::remove_file(p).unwrap();
    }
}

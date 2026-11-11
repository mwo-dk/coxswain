//! Previews that need Rust: a file's git diff, SQLite databases and dBase tables, EPUB books,
//! old pictures as PNG, a comic's pages, a tracker module's samples, and facts for the preview
//! pane (photo EXIF, audio tags, what an executable was built for, what an old file holds).

use coxswain_core::{t, tn};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

// ---------------------------------------------------------------- git diff

/// The file's changes against HEAD (staged and unstaged), or `None` when there are none. In a
/// history: what that commit changed in it. In a ZFS snapshot: how it differs from the file now.
#[tauri::command]
pub async fn git_diff(path: PathBuf) -> Res<Option<String>> {
    crate::here(&path)?;
    crate::blocking(move || {
        if let Some(at) = coxswain_core::history::split(&path).filter(|_| !path.exists()) {
            let text = coxswain_core::history::diff(&at).map_err(|e| e.to_string())?;
            return Ok((!text.trim().is_empty()).then_some(text));
        }
        // In a ZFS snapshot: what changed in it since, against the file now.
        if coxswain_core::zfs::in_snapshot(&path).is_some() {
            let text = coxswain_core::zfs::diff(&path).map_err(|e| e.to_string())?;
            return Ok((!text.trim().is_empty()).then_some(text));
        }
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Ok(None) };
        let mut c = coxswain_core::git::command(dir);
        // No external diff or textconv program from the repository's own config.
        c.args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--ignore-submodules=dirty", "HEAD", "--"]).arg(name);
        // Ended past its limit: a repository can name a FIFO git waits on.
        let Some(out) = coxswain_core::git::output(c) else { return Ok(None) };
        let text = String::from_utf8_lossy(&out.stdout);
        Ok((out.status.success() && !text.trim().is_empty()).then(|| text.chars().take(512 * 1024).collect()))
    })
    .await
}

// ---------------------------------------------------------------- SQLite

/// Tables and views with their row counts, schema and first rows (`tables::sqlite`), or a dBase
/// table's fields and first 200 records (`tables::dbf`), off the window's thread.
#[tauri::command]
pub async fn sqlite_info(path: PathBuf) -> Res<Vec<coxswain_core::tables::Table>> {
    crate::here(&path)?;
    let rows = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dbf")) { 200 } else { 5 };
    tauri::async_runtime::spawn_blocking(move || coxswain_core::tables::of(&path, rows)).await.map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------- old pictures, comics, modules

#[derive(Serialize)]
pub struct OldPicture {
    /// The PNG in the preview cache.
    file: PathBuf,
    pages: usize,
}

/// Page `page` of an old picture (TIFF, TGA, IFF, PCX, PSD, JPEG XL, …) as a PNG no wider or
/// taller than `side`, made once and kept in the preview cache (`pictures::png`). `None`: the
/// file is no such picture after all (a `.hdr` of another program), shown as text instead. A
/// thumbnail takes the preview's PNG when that was made; one picture is decoded at a time for
/// thumbnails and one for the preview, so a folder of big TIFFs does not decode them all at once.
#[tauri::command]
pub async fn old_picture(path: PathBuf, page: usize, side: u32) -> Res<Option<OldPicture>> {
    crate::here(&path)?;
    let locked = || coxswain_core::archive::locked_previews().map_err(|e| e.to_string());
    crate::blocking(move || picture_in(&crate::convert::previews_dir()?, &locked, &path, page, side)).await
}

/// `old_picture`, with the preview cache at `previews`, and `locked_dir` giving the run's own
/// folder a locked file's PNG goes to instead (`archive::locked_previews`, gone when the run
/// ends). Inside an archive the picture is unpacked once, and kept for its facts
/// (`asset::inside_file`); a tile (`side` up to 256) of one not reached alone is `None`, its icon. An error a copy of it on the disk would mend says so
/// (`pictures::NEEDS_COPY`).
fn picture_in(previews: &Path, locked_dir: &dyn Fn() -> Res<PathBuf>, path: &Path, page: usize, side: u32) -> Res<Option<OldPicture>> {
    static THUMBS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static PREVIEW: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let side = side.clamp(64, 4096);
    let split = coxswain_core::archive::split(path).filter(|_| !path.exists());
    // A locked file's preview is shown from the cache only once its password is proved again
    // (read to its checksum), so a picture once unlocked is not shown without it; a listing
    // that cannot be read (a 7z with locked names) counts as locked, and so does a file under a
    // locked one on the way (a picture in a zip inside a locked 7z).
    let locked = split.as_ref().is_some_and(|(archive, inner)| coxswain_core::archive::locked_on_the_way(archive, inner).unwrap_or(true));
    // A grid's tile of one not reached alone (further into a solid 7z's block, a tar's): its
    // icon, as `asset::thumb` gives, not all before it unpacked again for each tile; only a PNG
    // already kept is shown, and not a locked one's, which would be read again for its password.
    let icon = side <= 256 && split.is_some() && !coxswain_core::archive::reached_alone(path);
    if icon && locked {
        return Ok(None);
    }
    // Its PNG is never in the lasting cache: in this run's own folder, swept when the run ends.
    let own;
    let previews = if locked {
        own = locked_dir()?;
        own.as_path()
    } else {
        previews
    };
    let shown = |hit: OldPicture| -> Res<Option<OldPicture>> {
        if locked {
            coxswain_core::archive::head(path, 64).map_err(|e| e.to_string())?;
        }
        Ok(Some(hit))
    };
    let cached = |side: u32| -> Res<(PathBuf, PathBuf, Option<OldPicture>)> {
        let out = previews.join(crate::convert::cache_name(path, path, "picture", &format!("{page}-{side}"))?);
        coxswain_core::spill::used(&out);
        let (file, count) = (out.join("picture.png"), out.join("pages.txt"));
        let pages = std::fs::read_to_string(&count).ok().and_then(|s| s.trim().parse().ok());
        let hit = pages.filter(|_| file.is_file()).map(|pages| OldPicture { file: file.clone(), pages });
        Ok((out, file, hit))
    };
    if side <= 256
        && let (_, _, Some(hit)) = cached(2048)?
    {
        return shown(hit);
    }
    if let (_, _, Some(hit)) = cached(side)? {
        return shown(hit);
    }
    if icon {
        return Ok(None);
    }
    let _one = if side <= 256 { &THUMBS } else { &PREVIEW }.lock().unwrap_or_else(|e| e.into_inner());
    // Made meanwhile by the one before in line.
    let (out, file, hit) = cached(side)?;
    if let Some(hit) = hit {
        return shown(hit);
    }
    // Inside an archive: unpacked once here. A locked file is read again even when kept, which
    // proves its password; one over 64 MB (`asset::MAX_INSIDE`) is read from a copy.
    let bytes = match &split {
        Some((archive, _)) => match crate::asset::inside(path, archive, side > 256, locked) {
            Ok(Some(b)) => Some(b),
            Ok(None) => return Err(coxswain_core::pictures::needs_copy(t!("pictures.file_too_large", "size" => format!("{} MB", crate::asset::MAX_INSIDE >> 20)))),
            Err(e) if e.to_string() == coxswain_core::archive::LOCKED => return Err(e.to_string()),
            Err(e) => return Err(coxswain_core::pictures::needs_copy(e)),
        },
        None => None,
    };
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let Some(pages) = coxswain_core::pictures::png(path, bytes.as_deref().map(Vec::as_slice), page, side, &file)? else {
        let _ = std::fs::remove_dir(&out);
        return Ok(None);
    };
    let _ = std::fs::write(out.join("pages.txt"), pages.to_string());
    Ok(Some(OldPicture { file, pages }))
}

/// A comic's pages (`archive::pages`): the pictures in it, in the order of their names.
#[tauri::command]
pub async fn comic_pages(path: PathBuf) -> Res<Vec<String>> {
    crate::here(&path)?;
    crate::blocking(move || coxswain_core::archive::pages(&path).map_err(|e| e.to_string())).await
}

/// A tracker module's instrument and sample names; `None` for a file that is no module (a
/// `.mod` of Modula-2 source), shown as text instead.
#[tauri::command]
pub async fn module_names(path: PathBuf) -> Res<Option<Vec<String>>> {
    crate::here(&path)?;
    crate::blocking(move || Ok(coxswain_core::music::read(&path).map(|m| m.names))).await
}

// ---------------------------------------------------------------- EPUB

#[derive(Serialize)]
pub struct Book {
    title: String,
    /// The first chapter with real text, as XHTML body content. The UI sanitizes it.
    html: String,
}

/// The book's title and its first chapter with real text: covers and title pages are skipped.
#[tauri::command]
pub async fn epub_preview(path: PathBuf) -> Res<Book> {
    crate::here(&path)?;
    crate::blocking(move || match coxswain_core::extract::book::first_chapter(&path) {
        None => Err(t!("err.epub_no_rootfile")),
        Some((_, None)) => Err(t!("err.no_readable_chapter")),
        Some((title, Some(html))) => Ok(Book { title, html }),
    })
    .await
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
    crate::here(&path)?;
    crate::blocking(move || certs(&path)).await
}

fn certs(path: &Path) -> Res<Vec<Cert>> {
    use x509_parser::prelude::*;
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
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

/// The first message of a mail file, read as the search reads it (`extract::mail`).
#[tauri::command]
pub async fn mail_preview(path: PathBuf) -> Res<Mail> {
    crate::here(&path)?;
    crate::blocking(move || {
        let m = coxswain_core::extract::mail::summary(&path, 64 << 20).ok_or_else(|| t!("err.not_email"))?;
        Ok(Mail {
            subject: m.subject,
            from: m.from,
            to: m.to,
            date: m.date,
            text: m.text.chars().take(200_000).collect(),
            attachments: m.attachments.into_iter().map(|(name, size)| (name.unwrap_or_else(|| t!("facts.unnamed")), size)).collect(),
        })
    })
    .await
}

// ---------------------------------------------------------------- property lists

/// A macOS property list (binary or XML) as XML text.
#[tauri::command]
pub async fn plist_xml(path: PathBuf) -> Res<String> {
    crate::here(&path)?;
    crate::blocking(move || {
        let v = plist::Value::from_file(&path).map_err(|e| e.to_string())?;
        let mut out = vec![];
        v.to_writer_xml(&mut out).map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    })
    .await
}

// ---------------------------------------------------------------- facts

/// Label and value pairs for the preview pane: EXIF for photos, tags and format for audio,
/// target and kind for executables. Empty for anything else, and for a file only in the cloud:
/// reading it would download it.
#[tauri::command]
pub async fn file_facts(path: PathBuf, ctx: tauri::State<'_, crate::Ctx>) -> Result<Vec<(String, String)>, ()> {
    if coxswain_core::cloud::unasked(&path) {
        return Ok(vec![]);
    }
    let cfg = ctx.cfg().preview.clone();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        // An old picture, a module, a mailbox, a comic, a dBase table (`facts::of`), then the rest.
        let mut out = coxswain_core::facts::of(&path).unwrap_or_default();
        out.extend(match ext.as_str() {
            "jpg" | "jpeg" | "tif" | "tiff" | "heic" | "heif" | "png" | "webp" | "avif" | "dng" => exif_facts(&path),
            "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wav" | "aiff" | "wv" | "ape" => audio_facts(&path),
            // A video, or a song of a kind the tags reader does not know: ffprobe's facts.
            _ if out.is_empty() && coxswain_core::media::is_media(&path) => coxswain_core::media::facts(&cfg, &path),
            _ if out.is_empty() && coxswain_core::geo::is_vector(&path) => vec![],
            _ if out.is_empty() => binary_facts(&path),
            _ => vec![],
        });
        // Geodata's: a GPX file's tracks read here, the rest by GDAL; a GeoTIFF's after its photo facts.
        coxswain_core::geo::merge(&mut out, coxswain_core::geo::facts(&cfg, &path));
        out
    })
    .await
    .or(Ok(vec![]))
}

fn exif_facts(path: &Path) -> Vec<(String, String)> {
    use exif::{In, Tag};
    // Inside an archive: from the bytes the preview's picture was served from.
    let ex = match crate::asset::inside_file(path) {
        Some(bytes) => exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&bytes[..])),
        None => {
            let Ok(f) = File::open(path) else { return vec![] };
            exif::Reader::new().read_from_container(&mut BufReader::new(f))
        }
    };
    let Ok(ex) = ex else { return vec![] };
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
    // Inside an archive: from the bytes the player was served from.
    let file = match crate::asset::inside_file(path) {
        Some(bytes) => lofty::probe::Probe::new(std::io::Cursor::new(&bytes[..])).guess_file_type().ok().and_then(|p| p.read().ok()),
        None => lofty::read_from_path(path).ok(),
    };
    let Some(file) = file else { return vec![] };
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

/// A CPU name as shown; empty means one Coxswain does not name.
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

    /// A song inside a zip, previewed from memory, has its facts read from those bytes.
    #[test]
    fn a_song_inside_an_archive_has_its_facts() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-song-facts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Two seconds of 8 kHz mono 8-bit silence.
        let (rate, n) = (8000u32, 16000u32);
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + n).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend([1, 0, 1, 0]);
        wav.extend(rate.to_le_bytes());
        wav.extend(rate.to_le_bytes());
        wav.extend([1, 0, 8, 0]);
        wav.extend(b"data");
        wav.extend(n.to_le_bytes());
        wav.extend(std::iter::repeat_n(128u8, n as usize));
        let song = dir.join("tone.wav");
        std::fs::write(&song, &wav).unwrap();
        let zip = dir.join("music.zip");
        coxswain_core::archive::create(&zip, std::slice::from_ref(&song)).unwrap();
        let facts = audio_facts(&zip.join("tone.wav"));
        assert!(facts.iter().any(|(_, v)| v == "0:02"), "{facts:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

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

    /// No locked file in these: its own folder is never asked for.
    fn unused() -> Res<PathBuf> {
        Err("no locked file here".into())
    }

    #[test]
    fn old_pictures_are_made_once_and_thumbnails_take_the_preview() {
        let d = std::env::temp_dir().join(format!("coxswain-test-old-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (cache, pic, text) = (d.join("cache"), d.join("a.ppm"), d.join("scan.hdr"));
        std::fs::write(&pic, b"P6\n2 1\n255\n\xff\0\0\0\xff\0").unwrap();
        std::fs::write(&text, "ENVI\nsamples = 2\n").unwrap();
        assert!(picture_in(&cache, &unused, &text, 0, 2048).unwrap().is_none(), "another program's .hdr: text");
        let big = picture_in(&cache, &unused, &pic, 0, 2048).unwrap().unwrap();
        assert!(big.file.is_file() && big.pages == 1);
        let thumb = picture_in(&cache, &unused, &pic, 0, 256).unwrap().unwrap();
        assert_eq!(thumb.file, big.file, "the preview's PNG, not a second decode");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A TIFF inside a zip is unpacked once for its preview: its PNG made and its EXIF read from
    /// the same bytes. A spoiled one says why, with no call for a copy, which would fail alike.
    #[test]
    fn an_old_picture_inside_an_archive_is_unpacked_once() {
        let d = std::env::temp_dir().join(format!("coxswain-test-old-picture-once-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (tif, spoiled, zip) = (d.join("a.tif"), d.join("spoiled.tif"), d.join("pics.zip"));
        // One grey pixel, uncompressed: eight directory entries, then the pixel at 110.
        let mut t = b"II\x2a\x00\x08\x00\x00\x00\x08\x00".to_vec();
        for (tag, kind, v) in [(256u16, 3u16, 1u32), (257, 3, 1), (258, 3, 8), (259, 3, 1), (262, 3, 1), (273, 4, 110), (278, 3, 1), (279, 4, 1)] {
            t.extend([&tag.to_le_bytes()[..], &kind.to_le_bytes(), &1u32.to_le_bytes(), &v.to_le_bytes()].concat());
        }
        t.extend([0, 0, 0, 0, 0x80]);
        std::fs::write(&tif, t).unwrap();
        std::fs::write(&spoiled, b"II\x2a\x00\x08\x00\x00\x00\x00\x00").unwrap();
        coxswain_core::archive::create(&zip, &[tif, spoiled]).unwrap();
        let inner = zip.join("a.tif");
        // The one file kept unpacked is shared with the other tests' threads: one of them may
        // take its place between the two reads, so a few tries; a second unpacking every time fails.
        let once = (0..3).any(|i| {
            let before = coxswain_core::archive::heads();
            let made = picture_in(&d.join(format!("cache-{i}")), &unused, &inner, 0, 2048).unwrap().unwrap();
            assert!(made.file.is_file() && made.pages == 1);
            exif_facts(&inner);
            coxswain_core::archive::heads() - before == 1
        });
        assert!(once, "unpacked once for its picture and its facts");
        let e = picture_in(&d.join("cache"), &unused, &zip.join("spoiled.tif"), 0, 2048).err().unwrap();
        assert!(!e.starts_with(coxswain_core::pictures::NEEDS_COPY), "{e}");
        let js = format!("export const NEEDS_COPY = \"{}\";", coxswain_core::pictures::NEEDS_COPY);
        assert!(include_str!("../../src/lib.js").contains(&js), "the page knows the core's word");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Old pictures in a solid 7z: a tile of one further into the block is its icon, nothing
    /// unpacked, while its preview is still made (and then shown as its tile); the first of the
    /// block gets its tile.
    #[test]
    fn no_old_picture_tile_further_into_a_solid_block() {
        let d = std::env::temp_dir().join(format!("coxswain-test-old-picture-solid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let seven = d.join("pics.7z");
        // One grey pixel, uncompressed: eight directory entries, then the pixel at 110.
        let mut tif = b"II\x2a\x00\x08\x00\x00\x00\x08\x00".to_vec();
        for (tag, kind, v) in [(256u16, 3u16, 1u32), (257, 3, 1), (258, 3, 8), (259, 3, 1), (262, 3, 1), (273, 4, 110), (278, 3, 1), (279, 4, 1)] {
            tif.extend([&tag.to_le_bytes()[..], &kind.to_le_bytes(), &1u32.to_le_bytes(), &v.to_le_bytes()].concat());
        }
        tif.extend([0, 0, 0, 0, 0x80]);
        let names = ["a.tif", "b.tif", "c.tif"];
        let mut w = sevenz_rust2::ArchiveWriter::create(&seven).unwrap();
        let entries = names.iter().map(|n| {
            let mut e = sevenz_rust2::ArchiveEntry::new_file(n);
            e.size = tif.len() as u64;
            e
        });
        let readers = names.iter().map(|_| sevenz_rust2::SourceReader::new(std::io::Cursor::new(tif.clone()))).collect();
        w.push_archive_entries(entries.collect(), readers).unwrap();
        w.finish().unwrap();
        let cache = d.join("cache");
        let before = coxswain_core::archive::heads();
        assert!(picture_in(&cache, &unused, &seven.join("b.tif"), 0, 256).unwrap().is_none(), "b.tif: its icon");
        assert!(picture_in(&cache, &unused, &seven.join("c.tif"), 0, 256).unwrap().is_none(), "c.tif: its icon");
        assert_eq!(coxswain_core::archive::heads(), before, "nothing unpacked for an icon");
        let made = picture_in(&cache, &unused, &seven.join("c.tif"), 0, 2048).unwrap().unwrap();
        assert!(made.file.is_file() && made.pages == 1, "the preview still unpacks it");
        let before = coxswain_core::archive::heads();
        let kept = picture_in(&cache, &unused, &seven.join("c.tif"), 0, 256).unwrap().unwrap();
        assert_eq!((kept.file, coxswain_core::archive::heads()), (made.file, before), "once previewed, its tile takes that PNG, nothing unpacked");
        let tile = picture_in(&cache, &unused, &seven.join("a.tif"), 0, 256).unwrap().unwrap();
        assert!(tile.file.is_file(), "the first of the block: its tile");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// An old picture inside a locked zip: said as locked (the preview asks the password), not
    /// taken for text; with the password, its PNG made from memory in the run's own folder, never
    /// in the lasting preview cache, and not shown from there once the password is forgotten.
    #[test]
    fn an_old_picture_inside_a_locked_archive_asks_for_its_password() {
        let d = std::env::temp_dir().join(format!("coxswain-test-old-picture-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (cache, own, pic, zip) = (d.join("cache"), d.join("own"), d.join("a.ppm"), d.join("locked.zip"));
        let own_dir = || -> Res<PathBuf> { Ok(own.clone()) };
        std::fs::write(&pic, b"P6\n2 1\n255\n\xff\0\0\0\xff\0").unwrap();
        coxswain_core::archive::create_locked(&zip, std::slice::from_ref(&pic), Some("hunter2"), false).unwrap();
        coxswain_core::archive::forget(&zip);
        let inner = zip.join("a.ppm");
        assert_eq!(picture_in(&cache, &own_dir, &inner, 0, 2048).err().as_deref(), Some(coxswain_core::archive::LOCKED));
        coxswain_core::archive::remember(&zip, "hunter2");
        let made = picture_in(&cache, &own_dir, &inner, 0, 2048).unwrap().unwrap();
        assert!(made.file.is_file() && made.pages == 1);
        assert!(made.file.starts_with(&own), "in the run's own folder: {}", made.file.display());
        assert!(!cache.exists(), "nothing in the lasting preview cache");
        coxswain_core::archive::forget(&zip);
        // Its PNG is kept for this run, but not shown again without the password.
        assert_eq!(picture_in(&cache, &own_dir, &inner, 0, 2048).err().as_deref(), Some(coxswain_core::archive::LOCKED));
        assert_eq!(picture_in(&cache, &own_dir, &inner, 0, 256).err().as_deref(), Some(coxswain_core::archive::LOCKED));
        // A picture in an open zip inside a locked one: locked on the way, kept the same way.
        let (open, outer) = (d.join("open.zip"), d.join("outer.zip"));
        coxswain_core::archive::create(&open, std::slice::from_ref(&pic)).unwrap();
        coxswain_core::archive::create_locked(&outer, std::slice::from_ref(&open), Some("hunter2"), false).unwrap();
        let made = picture_in(&cache, &own_dir, &outer.join("open.zip").join("a.ppm"), 0, 2048).unwrap().unwrap();
        assert!(made.file.starts_with(&own) && !cache.exists(), "{}", made.file.display());
        coxswain_core::archive::forget(&outer);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn mail_and_plist() {
        let d = std::env::temp_dir().join(format!("coxswain-test-mail-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.eml"), "From: Ada <ada@example.com>\r\nTo: bob@example.com\r\nSubject: Launch\r\nDate: Mon, 28 Sep 2026 10:00:00 +0000\r\n\r\nGo for launch.\r\n").unwrap();
        let m = tauri::async_runtime::block_on(mail_preview(d.join("m.eml"))).unwrap();
        assert_eq!((m.subject.as_str(), m.from.as_str(), m.to.as_str()), ("Launch", "Ada <ada@example.com>", "bob@example.com"));
        assert!(m.text.contains("Go for launch"));
        let mut dict = plist::Dictionary::new();
        dict.insert("Name".into(), "Coxswain".into());
        plist::Value::Dictionary(dict).to_file_binary(d.join("i.plist")).unwrap();
        let xml = tauri::async_runtime::block_on(plist_xml(d.join("i.plist"))).unwrap();
        assert!(xml.contains("<key>Name</key>") && xml.contains("<string>Coxswain</string>"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn sqlite_tables_and_counts() {
        let p = std::env::temp_dir().join(format!("coxswain-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = rusqlite::Connection::open(&p).unwrap();
        db.execute_batch("CREATE TABLE launch(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO launch(name) VALUES ('a'), ('b'); CREATE VIEW v AS SELECT * FROM launch;").unwrap();
        drop(db);
        let t = tauri::async_runtime::block_on(sqlite_info(p.clone())).unwrap();
        assert_eq!((t[0].name.as_str(), t[0].rows), ("launch", Some(2)));
        assert_eq!((t[1].kind.as_str(), t[1].rows), ("view", None));
        assert_eq!(t[0].sample, [vec!["id", "name"], vec!["1", "a"], vec!["2", "b"]]);
        std::fs::remove_file(p).unwrap();
    }
}

//! Text that only an installed program can get, when it is installed:
//! - words in pictures, screenshots and scans: `tesseract` (OCR);
//! - scanned PDFs, which have pictures of pages instead of text: `pdftoppm` makes the pictures;
//! - older Office formats and drawings (.doc, .ppt, .pub, Visio …): LibreOffice makes a PDF,
//!   read as any PDF.
//!
//! The programs run at the lowest priority, with a time limit, into a folder of their own that
//! is removed afterwards. Photos from a camera are not read: they seldom have words, and there
//! are thousands of them.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::tools;

/// The programs, each with what it reads, for Settings.
pub const PROGRAMS: &[&str] = &["tesseract", "pdftoppm", "soffice"];

/// Pictures tesseract reads.
pub const PICTURES: &[&str] = &["png", "jpg", "jpeg", "jpe", "tif", "tiff", "webp", "bmp"];
/// Formats LibreOffice turns into a PDF and the built-in readers do not know.
pub const OFFICE: &[&str] = &["doc", "dot", "wps", "wpd", "pub", "ppt", "pps", "pot", "vsd", "vsdx", "odg", "sxw", "sxi", "sxd", "lwp", "pages", "key"];
/// Pages of a scanned PDF read, at most.
const MAX_PAGES: u32 = 30;
const TIMEOUT: Duration = Duration::from_secs(120);

/// Which of `PROGRAMS` are installed, looked for now: one installed while the helper runs is
/// used from the next scan, a few stats per file read.
pub fn found() -> Vec<(&'static str, Option<PathBuf>)> {
    PROGRAMS.iter().map(|p| (*p, tools::which(p))).collect()
}

fn program(name: &str) -> Option<PathBuf> {
    tools::which(name)
}

/// The extensions installed programs read now: they are read again when that changes.
pub fn extensions() -> Vec<&'static str> {
    let mut v = vec![];
    if program("tesseract").is_some() {
        v.extend(PICTURES);
        if program("pdftoppm").is_some() {
            v.push("pdf");
        }
    }
    if program("soffice").is_some() {
        v.extend(OFFICE);
    }
    v
}

/// A folder of its own for one run, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Option<Scratch> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("coxswain-extract-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        Some(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// tesseract's languages: English and the system's language when installed, else what is.
fn languages(tesseract: &Path) -> &'static str {
    static LANGS: OnceLock<String> = OnceLock::new();
    LANGS.get_or_init(|| {
        let out = tools::output(tesseract, &[OsStr::new("--list-langs")], Duration::from_secs(10), 64 * 1024).unwrap_or_default();
        let installed: Vec<String> = String::from_utf8_lossy(&out).lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && *l != "osd").map(str::to_string).collect();
        // BCP 47 to tesseract's ISO 639-2 names, for the languages Coxswain speaks and a few more.
        let own = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
        let code = match own.split(['-', '_']).next().unwrap_or("") {
            "da" => "dan",
            "sv" => "swe",
            "nb" | "no" => "nor",
            "fi" => "fin",
            "et" => "est",
            "lv" => "lav",
            "lt" => "lit",
            "de" => "deu",
            "fr" => "fra",
            "it" => "ita",
            "nl" => "nld",
            "es" => "spa",
            "ca" => "cat",
            "eu" => "eus",
            "he" | "iw" => "heb",
            "pl" => "pol",
            "pt" => "por",
            _ => "",
        };
        let chosen: Vec<&str> = ["eng", code].into_iter().filter(|c| installed.iter().any(|i| i == c)).collect();
        if chosen.is_empty() { installed.first().cloned().unwrap_or_else(|| "eng".into()) } else { chosen.join("+") }
    })
}

/// The words tesseract finds in a picture.
fn ocr(picture: &Path) -> Option<String> {
    let tesseract = program("tesseract")?;
    let args = [picture.as_os_str(), OsStr::new("stdout"), OsStr::new("-l"), OsStr::new(languages(&tesseract)), OsStr::new("--psm"), OsStr::new("3")];
    let out = tools::output(&tesseract, &args, TIMEOUT, super::MAX_TEXT as u64)?;
    let text = String::from_utf8_lossy(&out).into_owned();
    // A picture without words gives a few stray letters.
    (text.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3).count() >= 3).then_some(text)
}

/// The words in a picture, unless it is a photo from a camera.
pub fn picture(path: &Path, _max: u64) -> Option<String> {
    if camera_photo(path) {
        return None;
    }
    ocr(path)
}

/// The words on the pages of a PDF that has pictures of pages instead of text.
pub fn scanned_pdf(path: &Path) -> Option<String> {
    let (pdftoppm, _) = (program("pdftoppm")?, program("tesseract")?);
    let scratch = Scratch::new()?;
    let last = MAX_PAGES.to_string();
    let prefix = scratch.0.join("page");
    let args = [OsStr::new("-r"), OsStr::new("200"), OsStr::new("-gray"), OsStr::new("-png"), OsStr::new("-l"), OsStr::new(&last), path.as_os_str(), prefix.as_os_str()];
    tools::output(&pdftoppm, &args, TIMEOUT, 1024)?;
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&scratch.0).ok()?.flatten().map(|e| e.path()).collect();
    pages.sort();
    let text: Vec<String> = pages.iter().filter_map(|p| ocr(p)).collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

/// An older Office file or a drawing, made into a PDF by LibreOffice and read as one.
pub fn office(path: &Path, max: u64) -> Option<String> {
    let soffice = program("soffice")?;
    let scratch = Scratch::new()?;
    // A profile of its own, so neither a running LibreOffice nor a preview being made holds it.
    let profile = dirs::cache_dir()?.join("coxswain").join("libreoffice-index-profile");
    let url = format!("-env:UserInstallation=file:///{}", profile.to_string_lossy().trim_start_matches('/').replace('\\', "/"));
    let args = [OsStr::new(&url), OsStr::new("--headless"), OsStr::new("--norestore"), OsStr::new("--convert-to"), OsStr::new("pdf"), OsStr::new("--outdir"), scratch.0.as_os_str(), path.as_os_str()];
    tools::output(&soffice, &args, TIMEOUT, 64 * 1024)?;
    let pdf = std::fs::read_dir(&scratch.0).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "pdf"))?;
    super::pdf::text(&pdf, max.max(std::fs::metadata(&pdf).ok()?.len()))
}

/// A JPEG whose EXIF names the camera that took it.
fn camera_photo(path: &Path) -> bool {
    use std::io::Read;
    let mut head = vec![];
    if std::fs::File::open(path).and_then(|f| f.take(128 * 1024).read_to_end(&mut head)).is_err() || !head.starts_with(&[0xFF, 0xD8]) {
        return false;
    }
    let Some(at) = head.windows(6).position(|w| w == b"Exif\0\0") else { return false };
    has_make(&head[at + 6..])
}

/// Whether the TIFF structure in EXIF has the Make tag (0x010F) in its first directory.
fn has_make(tiff: &[u8]) -> bool {
    let big = match tiff.get(..2) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return false,
    };
    let u16_at = |i: usize| tiff.get(i..i + 2).map(|b| if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let u32_at = |i: usize| tiff.get(i..i + 4).map(|b| if big { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) });
    let Some(ifd) = u32_at(4).map(|o| o as usize) else { return false };
    let Some(n) = u16_at(ifd) else { return false };
    (0..n as usize).filter_map(|i| u16_at(ifd + 2 + i * 12)).any(|tag| tag == 0x010F)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG start with an EXIF block whose first directory has these tags.
    fn jpeg(tags: &[u16], big: bool) -> Vec<u8> {
        let (w16, w32): (fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) = if big { (u16::to_be_bytes, u32::to_be_bytes) } else { (u16::to_le_bytes, u32::to_le_bytes) };
        let mut tiff = if big { b"MM".to_vec() } else { b"II".to_vec() };
        tiff.extend(w16(42));
        tiff.extend(w32(8));
        tiff.extend(w16(tags.len() as u16));
        for t in tags {
            tiff.extend(w16(*t));
            tiff.extend([0u8; 10]);
        }
        [&[0xFF, 0xD8, 0xFF, 0xE1, 0, 0][..], b"Exif\0\0", &tiff].concat()
    }

    #[test]
    fn installed_programs_skip_camera_photos_only() {
        assert!(has_make(&jpeg(&[0x0100, 0x010F], false)[12..]));
        assert!(has_make(&jpeg(&[0x010F], true)[12..]));
        assert!(!has_make(&jpeg(&[0x0100, 0x0112], false)[12..]), "no Make: a screenshot or a scan");
        assert!(!has_make(b"II*\0\xff\xff\xff\xff"), "a directory past the end");
        assert!(!has_make(b""));
        let d = std::env::temp_dir().join(format!("coxswain-installed-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("photo.jpg"), jpeg(&[0x010F], false)).unwrap();
        std::fs::write(d.join("scan.jpg"), jpeg(&[0x0100], false)).unwrap();
        std::fs::write(d.join("shot.png"), b"\x89PNG").unwrap();
        assert!(camera_photo(&d.join("photo.jpg")));
        assert!(!camera_photo(&d.join("scan.jpg")) && !camera_photo(&d.join("shot.png")));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// With the programs installed (they are not on CI): a picture of words, a PDF that is only
    /// a picture of a page, and a Word 97 file. `magick` draws the picture.
    #[test]
    fn installed_programs_read_pictures_scans_and_old_office_files() {
        let (Some(magick), Some(fc), Some(_)) = (tools::which("magick"), tools::which("fc-match"), program("tesseract")) else { return };
        // ImageMagick's own default font may lack the letters: a font of the system's.
        let font = String::from_utf8(std::process::Command::new(fc).args(["-f", "%{file}", "sans"]).output().unwrap().stdout).unwrap();
        let d = std::env::temp_dir().join(format!("coxswain-installed-read-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let draw = |out: &str| {
            let ok = std::process::Command::new(&magick)
                .args(["-size", "1400x300", "xc:white", "-font", &font, "-fill", "black", "-pointsize", "64", "-annotate", "+40+160", "Rocket launch window opens"])
                .arg(d.join(out))
                .status()
                .unwrap();
            assert!(ok.success());
        };
        draw("shot.png");
        let text = picture(&d.join("shot.png"), u64::MAX).unwrap_or_default();
        assert!(text.contains("Rocket") && text.contains("window"), "{text:?}");

        if program("pdftoppm").is_some() {
            draw("scan.pdf");
            assert_eq!(super::super::pdf::text(&d.join("scan.pdf"), u64::MAX), None, "a picture, no text of its own");
            let text = super::super::text_of(&d.join("scan.pdf"), 1, u64::MAX).unwrap_or_default();
            assert!(text.contains("launch"), "{text:?}");
        }
        if program("soffice").is_some() {
            std::fs::write(d.join("memo.txt"), "Fuel budget for flight seven\n").unwrap();
            let s = std::process::Command::new(program("soffice").unwrap()).args(["--headless", "--convert-to", "doc", "--outdir"]).arg(&d).arg(d.join("memo.txt")).output().unwrap();
            if s.status.success() && d.join("memo.doc").exists() {
                let text = super::super::text_of(&d.join("memo.doc"), 1, u64::MAX).unwrap_or_default();
                assert!(text.contains("Fuel budget"), "{text:?}");
            }
        }
        std::fs::remove_dir_all(d).unwrap();
    }
}

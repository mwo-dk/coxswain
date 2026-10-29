//! Compare files (docs/files/compare-files.md), in both apps: two files side by side, or one
//! under the other. Text line by line (`text`), with the changed words of a changed line; a PDF,
//! Word or other document by the text its reader takes out of it; pictures by their pixels; any
//! other file byte by byte (`bytes`).
//!
//! A `Session` reads the two files on a thread of its own, so the window opens at once and Stop
//! (closing it) ends the reading: what the apps show is pulled from it a screen at a time.

pub mod aside;
pub mod bytes;
pub mod folders;
pub mod sync;
pub mod text;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// How two files are compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Line by line.
    Text,
    /// By the text their readers take out of them (PDF, Word, OpenDocument…), line by line.
    Document,
    /// Pixel by pixel.
    Picture,
    /// Byte by byte, at the same place in both.
    Binary,
}

/// Pictures the desktop app's web view shows by itself, besides those `pictures` decodes.
const WEB_PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// A picture both apps can compare by its pixels: one `pictures` decodes, or one of
/// `WEB_PICTURES`.
pub fn is_picture(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    WEB_PICTURES.contains(&ext.as_str()) || crate::pictures::named(path)
}

/// Documents compared by the text their reader takes out of them: only these, so a picture is
/// never read by OCR, nor anything else by an installed program, to be compared.
const DOCUMENTS: &[&str] = &[
    "pdf", "docx", "docm", "dotx", "odt", "ott", "xlsx", "xlsm", "xls", "xlsb", "ods", "pptx", "ppsx", "potx", "odp", "epub", "rtf", "msg",
];

fn is_document(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    DOCUMENTS.contains(&ext.as_str())
}

/// Text by its first 8 KB: no zero byte, or a UTF-16 byte order mark.
fn looks_like_text(path: &Path) -> bool {
    let mut head = vec![0u8; 8192];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    crate::view::decode(&head[..n]).is_some()
}

/// How `a` and `b` are compared: pictures when both are; text when both look like it; by their
/// text when each is text or a document (`DOCUMENTS`); else byte by byte.
pub fn kind(a: &Path, b: &Path) -> Kind {
    if is_picture(a) && is_picture(b) {
        return Kind::Picture;
    }
    let (ta, tb) = (looks_like_text(a), looks_like_text(b));
    if ta && tb {
        Kind::Text
    } else if (ta || is_document(a)) && (tb || is_document(b)) {
        Kind::Document
    } else {
        Kind::Binary
    }
}

// ---------------------------------------------------------------- pictures

/// One picture's size, as read.
#[derive(Clone, Debug, Serialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// Two pictures compared.
#[derive(Clone, Debug, Serialize)]
pub struct Pictures {
    pub left: Size,
    pub right: Size,
    /// Pixels that differ, when both are the same size.
    pub differ: Option<u64>,
}

fn decode_picture(path: &Path) -> Result<image::RgbaImage, String> {
    if crate::pictures::readable(path) {
        return crate::pictures::decode(path, 0).map(|p| p.image);
    }
    let open = || image::ImageReader::open(path).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string());
    // Its size first: the RGBA copy of a picture is 4 bytes a pixel, and no more than
    // `pictures::MAX_PIXELS` are decoded.
    let (w, h) = open()?.into_dimensions().map_err(|e| e.to_string())?;
    crate::pictures::check_size(w.into(), h.into())?;
    let mut r = open()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 << 20);
    limits.max_image_width = Some(w);
    limits.max_image_height = Some(h);
    r.limits(limits);
    // Other people's decoders: a panic in one is an error here.
    std::panic::catch_unwind(move || r.decode().map(|i| i.to_rgba8()).map_err(|e| e.to_string())).unwrap_or_else(|_| Err("panic".into()))
}

/// `a` and `b` compared pixel by pixel.
pub fn pictures(a: &Path, b: &Path) -> Result<Pictures, String> {
    let (pa, pb) = (decode_picture(a)?, decode_picture(b)?);
    let size = |p: &image::RgbaImage| Size { width: p.width(), height: p.height() };
    let differ = (pa.dimensions() == pb.dimensions()).then(|| pa.pixels().zip(pb.pixels()).filter(|(x, y)| x != y).count() as u64);
    Ok(Pictures { left: size(&pa), right: size(&pb), differ })
}

// ---------------------------------------------------------------- the session

/// What the reading thread has made.
pub enum Done {
    Text(text::Compare),
    Bytes(bytes::Bytes),
    Pictures(Pictures),
    Failed(String),
}

/// Two files being compared: read on a thread of its own, which stops when the session is
/// dropped (the window closed) or `stop` is called.
pub struct Session {
    pub kind: Kind,
    pub left: PathBuf,
    pub right: PathBuf,
    pub options: text::Options,
    /// Bytes read so far of both files, of `total`.
    pub read: Arc<AtomicU64>,
    pub total: u64,
    stop: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Done>>>,
}

impl Session {
    /// Start comparing `left` with `right`.
    pub fn open(left: &Path, right: &Path, options: text::Options) -> Session {
        let kind = kind(left, right);
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        let s = Session {
            kind,
            left: left.to_path_buf(),
            right: right.to_path_buf(),
            options,
            read: Arc::default(),
            total: size(left) + size(right),
            stop: Arc::default(),
            done: Arc::default(),
        };
        let (a, b, read, stop, done) = (s.left.clone(), s.right.clone(), s.read.clone(), s.stop.clone(), s.done.clone());
        std::thread::spawn(move || {
            let made = match kind {
                Kind::Text | Kind::Document => text::Compare::open(&a, &b, kind, options, &read, &stop).map(Done::Text),
                Kind::Binary => bytes::Bytes::open(&a, &b)
                    .map(|b| {
                        b.count(stop.clone());
                        Done::Bytes(b)
                    })
                    .map_err(|e| e.to_string()),
                Kind::Picture => pictures(&a, &b).map(Done::Pictures),
            };
            *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(made.unwrap_or_else(Done::Failed));
        });
        s
    }

    /// What was made, once it is (`None` while reading).
    pub fn done(&self) -> MutexGuard<'_, Option<Done>> {
        self.done.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The same two files compared again with other options (or after they were saved).
    pub fn again(&self, options: text::Options) -> Session {
        Session::open(&self.left, &self.right, options)
    }

    /// How far the reading is, in percent.
    pub fn percent(&self) -> usize {
        crate::settings::percent(self.read.load(Ordering::Relaxed) as usize, self.total.max(1) as usize)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_content_and_name() {
        let d = tempfile_dir("kinds");
        let w = |n: &str, b: &[u8]| {
            let p = d.join(n);
            std::fs::write(&p, b).unwrap();
            p
        };
        let (t1, t2, bin) = (w("a.rs", b"fn a() {}\n"), w("b.txt", b"hello\n"), w("c.bin", b"\0\x01\x02"));
        assert_eq!(kind(&t1, &t2), Kind::Text);
        assert_eq!(kind(&t1, &bin), Kind::Binary);
        let pdf = w("c.pdf", b"%PDF-1.4\n\0\0");
        assert_eq!(kind(&t1, &pdf), Kind::Document);
        let (p1, p2) = (d.join("x.png"), d.join("y.png"));
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255])).save(&p1).unwrap();
        let mut other = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        other.put_pixel(1, 1, image::Rgba([9, 9, 9, 255]));
        other.save(&p2).unwrap();
        assert_eq!(kind(&p1, &p2), Kind::Picture);
        let p = pictures(&p1, &p2).unwrap();
        assert_eq!((p.left.width, p.right.height, p.differ), (3, 2, Some(1)));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_picture_too_big_to_hold_is_refused_before_it_is_decoded() {
        let d = tempfile_dir("huge");
        // A PNG header claiming 100000 × 100000 pixels: 40 GB as RGBA.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        let crc = crc32(&png[12..]);
        png.extend(crc.to_be_bytes());
        let p = d.join("huge.png");
        std::fs::write(&p, &png).unwrap();
        assert!(pictures(&p, &p).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    pub(crate) fn tempfile_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-compare-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}

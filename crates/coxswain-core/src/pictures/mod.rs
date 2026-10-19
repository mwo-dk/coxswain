//! Old pictures, in both apps: TIFF (with its pages), TGA, Netpbm, DDS, Radiance HDR,
//! OpenEXR, QOI and farbfeld through the `image` crate, JPEG XL through `jxl-oxide`, and
//! Coxswain's own small readers of Amiga IFF (ILBM and PBM), PCX, XPM, XBM and Photoshop's
//! flattened picture. The desktop app's preview gets a PNG of one (`png`), cached with the
//! other previews; both apps show its facts (`facts`): its format, size in pixels, colours,
//! pages and layers. All pure Rust.
//!
//! The files come from anywhere, so every reader holds for any bytes: bounded reads
//! (`MAX_FILE`), no more pixels than `MAX_PIXELS`, no panic (`catch_unwind` over the crates'
//! own readers as well), and errors said as text.

mod iff;
pub(crate) mod pcx;
mod psd;
mod xpm;

use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageFormat, RgbaImage};

use crate::{t, tn};

/// The most bytes of a picture read into memory.
pub const MAX_FILE: u64 = 256 << 20;
/// The most pixels a picture may have to be shown: 64 million, 256 MB as RGBA.
pub const MAX_PIXELS: u64 = 64 << 20;
/// The most pages of a TIFF counted.
const MAX_PAGES: usize = 1000;

/// Which reader a picture has.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// One the `image` crate reads; a TIFF has pages.
    Lib(ImageFormat),
    Jxl,
    Iff,
    Pcx,
    Xpm,
    Xbm,
    Psd,
}

/// The endings, in small letters, and their readers.
const ENDINGS: &[(&str, Kind)] = &[
    ("tif", Kind::Lib(ImageFormat::Tiff)),
    ("tiff", Kind::Lib(ImageFormat::Tiff)),
    ("tga", Kind::Lib(ImageFormat::Tga)),
    ("ppm", Kind::Lib(ImageFormat::Pnm)),
    ("pgm", Kind::Lib(ImageFormat::Pnm)),
    ("pbm", Kind::Lib(ImageFormat::Pnm)),
    ("pnm", Kind::Lib(ImageFormat::Pnm)),
    ("pam", Kind::Lib(ImageFormat::Pnm)),
    ("dds", Kind::Lib(ImageFormat::Dds)),
    ("hdr", Kind::Lib(ImageFormat::Hdr)),
    ("exr", Kind::Lib(ImageFormat::OpenExr)),
    ("qoi", Kind::Lib(ImageFormat::Qoi)),
    ("ff", Kind::Lib(ImageFormat::Farbfeld)),
    ("jxl", Kind::Jxl),
    ("iff", Kind::Iff),
    ("lbm", Kind::Iff),
    ("ilbm", Kind::Iff),
    ("pcx", Kind::Pcx),
    ("xpm", Kind::Xpm),
    ("xbm", Kind::Xbm),
    ("psd", Kind::Psd),
    ("psb", Kind::Psd),
];

/// Every ending these pictures have, without the dot, for the desktop app's page.
pub fn endings() -> Vec<&'static str> {
    ENDINGS.iter().map(|(e, _)| *e).collect()
}

fn kind_by_name(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    ENDINGS.iter().find(|(e, _)| *e == ext).map(|(_, k)| *k)
}

/// A name one of these pictures has. No file is read.
pub fn named(path: &Path) -> bool {
    kind_by_name(path).is_some()
}

/// The file is one of these pictures: by its name, and its first bytes say so too (a `.hdr` of
/// another program is not). Only the first bytes are read.
pub fn readable(path: &Path) -> bool {
    kind(path).is_some()
}

/// A picture a web view shows as it is, or one of these: a page of a comic.
pub fn is_page(path: &Path) -> bool {
    const WEB: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif", "svg"];
    named(path) || path.extension().and_then(|e| e.to_str()).is_some_and(|e| WEB.contains(&e.to_ascii_lowercase().as_str()))
}

/// The reader for the file: by its name, and its first bytes say so too. A `.hdr` that is a
/// header of another kind (ENVI, Analyze) or an `.ff` that is no farbfeld is none.
fn kind(path: &Path) -> Option<Kind> {
    let k = kind_by_name(path)?;
    let mut head = [0u8; 64];
    let n = read_head(path, &mut head)?;
    sniff(k, &head[..n]).then_some(k)
}

fn read_head(path: &Path, head: &mut [u8]) -> Option<usize> {
    let mut f = File::open(path).ok()?;
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(m) => n += m,
            Err(_) => return None,
        }
    }
    Some(n)
}

/// Whether the first bytes are those of a picture of this kind (TGA has none to check).
fn sniff(k: Kind, h: &[u8]) -> bool {
    match k {
        Kind::Lib(ImageFormat::Tiff) => {
            h.starts_with(b"II*\0") || h.starts_with(b"MM\0*") || h.starts_with(b"II+\0") || h.starts_with(b"MM\0+")
        }
        Kind::Lib(ImageFormat::Tga) => h.len() >= 18,
        Kind::Lib(ImageFormat::Pnm) => h.len() > 2 && h[0] == b'P' && (b'1'..=b'7').contains(&h[1]),
        Kind::Lib(ImageFormat::Dds) => h.starts_with(b"DDS "),
        Kind::Lib(ImageFormat::Hdr) => h.starts_with(b"#?RADIANCE") || h.starts_with(b"#?RGBE"),
        Kind::Lib(ImageFormat::OpenExr) => h.starts_with(&[0x76, 0x2f, 0x31, 0x01]),
        Kind::Lib(ImageFormat::Qoi) => h.starts_with(b"qoif"),
        Kind::Lib(ImageFormat::Farbfeld) => h.starts_with(b"farbfeld"),
        Kind::Lib(_) => false,
        Kind::Jxl => h.starts_with(&[0xff, 0x0a]) || h.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n"),
        Kind::Iff => h.starts_with(b"FORM") && h.len() >= 12 && (&h[8..12] == b"ILBM" || &h[8..12] == b"PBM "),
        Kind::Pcx => h.len() >= 4 && h[0] == 0x0a && h[2] == 1 && [1, 2, 4, 8].contains(&h[3]),
        Kind::Xpm => xpm::is_xpm(h),
        Kind::Xbm => xpm::is_xbm(h),
        Kind::Psd => h.starts_with(b"8BPS"),
    }
}

/// What a picture says of itself, for the facts.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Info {
    pub format: &'static str,
    pub width: u64,
    pub height: u64,
    /// What its colours are, in words.
    pub colours: String,
    pub pages: Option<usize>,
    pub layers: Option<usize>,
}

/// A picture: its pixels and how many pages its file has.
#[derive(Debug)]
pub struct Picture {
    pub image: RgbaImage,
    pub pages: usize,
}

/// The format's name as shown.
fn format_name(k: Kind) -> &'static str {
    match k {
        Kind::Lib(ImageFormat::Tiff) => "TIFF",
        Kind::Lib(ImageFormat::Tga) => "Truevision TGA",
        Kind::Lib(ImageFormat::Pnm) => "Netpbm",
        Kind::Lib(ImageFormat::Dds) => "DirectDraw Surface (DDS)",
        Kind::Lib(ImageFormat::Hdr) => "Radiance HDR",
        Kind::Lib(ImageFormat::OpenExr) => "OpenEXR",
        Kind::Lib(ImageFormat::Qoi) => "QOI",
        Kind::Lib(ImageFormat::Farbfeld) => "farbfeld",
        Kind::Lib(_) => "",
        Kind::Jxl => "JPEG XL",
        Kind::Iff => "Amiga IFF",
        Kind::Pcx => "PCX",
        Kind::Xpm => "X PixMap (XPM)",
        Kind::Xbm => "X BitMap (XBM)",
        Kind::Psd => "Photoshop",
    }
}

/// "RGB, 8 bits per channel", "grey, 16 bits per channel".
pub(crate) fn colours(model: &str, bits: u32) -> String {
    t!("facts.colours_value", "model" => model, "bits" => bits)
}

pub(crate) fn grey() -> String {
    t!("facts.grey")
}

pub(crate) fn palette(n: usize) -> String {
    tn!("facts.palette", n as u64)
}

pub(crate) fn too_large(w: u64, h: u64) -> String {
    t!("pictures.too_large", "width" => w, "height" => h)
}

/// The pixels are no more than `MAX_PIXELS`, and none of the sides is 0.
pub(crate) fn check_size(w: u64, h: u64) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err(t!("pictures.unreadable", "why" => "0 × 0"));
    }
    if w.saturating_mul(h) > MAX_PIXELS {
        return Err(too_large(w, h));
    }
    Ok(())
}

fn limits() -> image::Limits {
    let mut l = image::Limits::default();
    // 512 MB: an 8-bit RGBA picture of `MAX_PIXELS` and its copy on the way.
    l.max_alloc = Some(512 << 20);
    l
}

/// The whole file, if it is no larger than `MAX_FILE`.
fn read_all(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = vec![];
    File::open(path)
        .and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(t!("pictures.file_too_large", "size" => format!("{} MB", MAX_FILE >> 20)));
    }
    Ok(bytes)
}

/// Page `page` (from 0) of the picture, and how many it has. A page past the last is the last.
pub fn decode(path: &Path, page: usize) -> Result<Picture, String> {
    let k = kind(path).ok_or_else(|| t!("pictures.not_this"))?;
    // The crates' readers are other people's code: a panic in one is an error here.
    std::panic::catch_unwind(|| decode_kind(path, k, page))
        .unwrap_or_else(|_| Err(t!("pictures.unreadable", "why" => "panic")))
}

fn decode_kind(path: &Path, k: Kind, page: usize) -> Result<Picture, String> {
    let unreadable = |e: &dyn std::fmt::Display| t!("pictures.unreadable", "why" => e);
    let one = |image: RgbaImage| Ok(Picture { image, pages: 1 });
    match k {
        Kind::Lib(ImageFormat::Tiff) => {
            let mut bytes = read_all(path)?;
            let ifds = tiff_pages(&bytes);
            let pages = ifds.len().max(1);
            // The header is made to point at the page asked for: `image` reads the first.
            if let Some(&at) = ifds.get(page.min(pages - 1)) {
                point_at(&mut bytes, at);
            }
            let d = image::codecs::tiff::TiffDecoder::new(Cursor::new(bytes)).map_err(|e| unreadable(&e))?;
            let image = lib(d)?;
            Ok(Picture { image, pages })
        }
        Kind::Lib(fmt) => {
            let mut r = image::ImageReader::with_format(BufReader::new(File::open(path).map_err(|e| e.to_string())?), fmt);
            r.limits(limits());
            let d = r.into_decoder().map_err(|e| unreadable(&e))?;
            one(lib(d)?)
        }
        Kind::Jxl => {
            let d = jxl_oxide::integration::JxlDecoder::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
                .map_err(|e| unreadable(&e))?;
            one(lib(d)?)
        }
        Kind::Iff => one(iff::decode(&read_all(path)?)?),
        Kind::Pcx => one(pcx::decode(&read_all(path)?)?),
        Kind::Xpm => one(xpm::decode_xpm(&read_all(path)?)?),
        Kind::Xbm => one(xpm::decode_xbm(&read_all(path)?)?),
        Kind::Psd => one(psd::decode(&read_all(path)?)?),
    }
}

/// A picture through one of the crates' decoders, its size checked first.
fn lib(mut d: impl ImageDecoder) -> Result<RgbaImage, String> {
    let (w, h) = d.dimensions();
    check_size(w.into(), h.into())?;
    d.set_limits(limits()).map_err(|e| t!("pictures.unreadable", "why" => e))?;
    let img = DynamicImage::from_decoder(d).map_err(|e| t!("pictures.unreadable", "why" => e))?;
    Ok(img.into_rgba8())
}

/// Where each page's directory (IFD) of a TIFF (or BigTIFF) starts, in their order: at most
/// `MAX_PAGES`, a loop or an offset past the end ends the list.
fn tiff_pages(b: &[u8]) -> Vec<u64> {
    tiff_pages_in(&mut Cursor::new(b), b.len() as u64)
}

/// `tiff_pages` of a file `len` bytes long, read with seeks: only the directories' heads.
fn tiff_pages_in(r: &mut (impl Read + std::io::Seek), len: u64) -> Vec<u64> {
    let r = std::cell::RefCell::new(r);
    let read = |o: u64, n: usize| -> Option<[u8; 8]> {
        let mut r = r.borrow_mut();
        r.seek(std::io::SeekFrom::Start(o)).ok()?;
        let mut b = [0u8; 8];
        r.read_exact(&mut b[..n]).ok()?;
        Some(b)
    };
    let le = read(0, 2).is_some_and(|b| &b[..2] == b"II");
    let u16_at = |o: u64| -> Option<u64> {
        let s = read(o, 2)?;
        Some(if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) } as u64)
    };
    let u32_at = |o: u64| -> Option<u64> {
        let s: [u8; 4] = read(o, 4)?[..4].try_into().ok()?;
        Some(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) } as u64)
    };
    let u64_at = |o: u64| -> Option<u64> {
        let s = read(o, 8)?;
        Some(if le { u64::from_le_bytes(s) } else { u64::from_be_bytes(s) })
    };
    let big = u16_at(2) == Some(43);
    let mut at = if big { u64_at(8) } else { u32_at(4) };
    let mut out = vec![];
    while let Some(o) = at.filter(|&o| o != 0 && o < len) {
        if out.len() >= MAX_PAGES || out.contains(&o) {
            break;
        }
        out.push(o);
        at = if big {
            u64_at(o).and_then(|n| u64_at(o.checked_add(8)?.checked_add(n.checked_mul(20)?)?))
        } else {
            u16_at(o).and_then(|n| u32_at(o + 2 + n * 12))
        };
    }
    out
}

/// The TIFF header's first directory set to the one at `at`.
fn point_at(b: &mut [u8], at: u64) {
    let le = b.starts_with(b"II");
    if b.get(2..4) == Some(if le { &[43, 0] } else { &[0, 43] }) {
        let v = if le { at.to_le_bytes() } else { at.to_be_bytes() };
        if let Some(s) = b.get_mut(8..16) {
            s.copy_from_slice(&v);
        }
    } else if let (Ok(at), Some(s)) = (u32::try_from(at), b.get_mut(4..8)) {
        s.copy_from_slice(&if le { at.to_le_bytes() } else { at.to_be_bytes() });
    }
}

/// The colours a crate's decoder says a picture has, in words.
fn lib_colours(c: image::ColorType) -> String {
    use image::ColorType as C;
    let bits = (c.bits_per_pixel() / u16::from(c.channel_count())) as u32;
    let model = match c {
        C::L8 | C::L16 => grey(),
        C::La8 | C::La16 => t!("facts.grey_alpha"),
        C::Rgb8 | C::Rgb16 | C::Rgb32F => "RGB".into(),
        _ => "RGBA".into(),
    };
    colours(&model, bits)
}

/// What a picture is, read from its header where it can be: the format, its size in pixels,
/// its colours, and its pages and layers. `None` for a file that is none of these pictures,
/// and for a file only in the cloud (reading it would download it).
pub(crate) fn info(path: &Path) -> Option<Info> {
    if crate::cloud::unasked(path) {
        return None;
    }
    let k = kind(path)?;
    std::panic::catch_unwind(|| info_kind(path, k)).ok().flatten()
}

fn info_kind(path: &Path, k: Kind) -> Option<Info> {
    let format = format_name(k);
    let from = |d: &dyn Fn() -> Option<((u32, u32), image::ColorType)>| {
        let ((w, h), c) = d()?;
        Some(Info { format, width: w.into(), height: h.into(), colours: lib_colours(c), ..Info::default() })
    };
    match k {
        Kind::Lib(ImageFormat::Tiff) => {
            // The header and the directories only, read with seeks: not the pictures.
            let mut f = BufReader::new(File::open(path).ok()?);
            let len = f.get_ref().metadata().ok()?.len();
            let pages = tiff_pages_in(&mut f, len).len();
            let mut i = from(&|| {
                let d = image::codecs::tiff::TiffDecoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
                Some((d.dimensions(), d.color_type()))
            })?;
            i.pages = Some(pages.max(1));
            Some(i)
        }
        Kind::Lib(fmt) => from(&|| {
            let r = image::ImageReader::with_format(BufReader::new(File::open(path).ok()?), fmt);
            let d = r.into_decoder().ok()?;
            Some((d.dimensions(), d.color_type()))
        }),
        Kind::Jxl => from(&|| {
            let d = jxl_oxide::integration::JxlDecoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
            Some((d.dimensions(), d.color_type()))
        }),
        Kind::Psd => psd::info(&mut BufReader::new(File::open(path).ok()?)),
        Kind::Iff | Kind::Pcx | Kind::Xpm | Kind::Xbm => {
            let bytes = read_all(path).ok()?;
            let mut i = match k {
                Kind::Iff => iff::info(&bytes),
                Kind::Pcx => pcx::info(&bytes),
                Kind::Xpm => xpm::info_xpm(&bytes),
                _ => xpm::info_xbm(&bytes),
            }?;
            i.format = if k == Kind::Iff { i.format } else { format };
            Some(i)
        }
    }
}

/// The facts of a picture as (label, value) lines: format, size, colours, pages, layers.
pub fn facts(path: &Path) -> Option<Vec<(String, String)>> {
    let i = info(path)?;
    let mut out = vec![
        (t!("facts.format"), i.format.to_string()),
        (t!("facts.dimensions"), t!("facts.dimensions_value", "width" => i.width, "height" => i.height)),
    ];
    if !i.colours.is_empty() {
        out.push((t!("facts.colours"), i.colours));
    }
    if let Some(p) = i.pages.filter(|&p| p > 1) {
        out.push((t!("facts.pages"), p.to_string()));
    }
    if let Some(l) = i.layers {
        out.push((t!("facts.layers"), l.to_string()));
    }
    Some(out)
}

/// Page `page` of the picture as a PNG at `to`, made no wider or taller than `side` pixels;
/// how many pages it has.
pub fn png(path: &Path, page: usize, side: u32, to: &Path) -> Result<usize, String> {
    let Picture { image, pages } = decode(path, page)?;
    let image = DynamicImage::ImageRgba8(image);
    let image = if image.width().max(image.height()) > side { image.thumbnail(side, side) } else { image };
    let tmp = to.with_extension("part");
    let w = std::io::BufWriter::new(File::create(&tmp).map_err(|e| e.to_string())?);
    let enc = image::codecs::png::PngEncoder::new_with_quality(w, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive);
    image.write_with_encoder(enc).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())?;
    Ok(pages)
}

/// PackBits (Apple's, and Amiga's ByteRun1): from `src` at `at` until `out` holds `want` bytes
/// more or `src` ends. A run that would pass `want` is cut there.
pub(crate) fn unpack_bits(src: &[u8], at: &mut usize, out: &mut Vec<u8>, want: usize) {
    let end = out.len() + want;
    while out.len() < end && *at < src.len() {
        let n = src[*at] as i8;
        *at += 1;
        if n >= 0 {
            let take = (n as usize + 1).min(end - out.len()).min(src.len() - *at);
            out.extend_from_slice(&src[*at..*at + take]);
            *at += n as usize + 1;
        } else if n != -128 {
            let Some(&b) = src.get(*at) else { break };
            *at += 1;
            let take = (1 - n as isize) as usize;
            out.resize(out.len() + take.min(end - out.len()), b);
        }
    }
    *at = (*at).min(src.len());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A folder of its own for a test.
    pub fn folder(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-pictures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Every way of cutting and spoiling `bytes`: the reader must not panic on any, nor take long.
    pub fn hostile(bytes: &[u8], read: impl Fn(&[u8])) {
        for cut in (0..bytes.len()).step_by((bytes.len() / 64).max(1)) {
            read(&bytes[..cut]);
        }
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..300 {
            let mut b = bytes.to_vec();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = (seed as usize) % b.len().max(1);
                if let Some(x) = b.get_mut(i) {
                    *x = (seed >> 32) as u8;
                }
            }
            read(&b);
        }
    }

    #[test]
    fn packbits_runs_and_literals_within_bounds() {
        let src = [2u8, 1, 2, 3, 0xfe, 9, 0x80, 0];
        let (mut at, mut out) = (0, vec![]);
        unpack_bits(&src, &mut at, &mut out, 100);
        assert_eq!(out, [1, 2, 3, 9, 9, 9]);
        let (mut at, mut out) = (0, vec![]);
        unpack_bits(&src, &mut at, &mut out, 4);
        assert_eq!(out, [1, 2, 3, 9], "never more than asked for");
        let (mut at, mut out) = (0, vec![]);
        unpack_bits(&[127, 1], &mut at, &mut out, 1000);
        assert_eq!(out, [1], "a literal run past the end stops there");
        assert_eq!(at, 2);
    }

    #[test]
    fn tiff_pages_are_found_and_shown_one_by_one() {
        let d = folder("tiff");
        // Two pages of 2×1 grey pixels, by hand: header, then each page's directory.
        let page = |at: u32, next: u32, value: u8| -> Vec<u8> {
            let mut v = vec![];
            let entry = |tag: u16, kind: u16, count: u32, val: u32| -> Vec<u8> {
                [tag.to_le_bytes().to_vec(), kind.to_le_bytes().to_vec(), count.to_le_bytes().to_vec(), val.to_le_bytes().to_vec()].concat()
            };
            let data = at + 2 + 8 * 12 + 4;
            v.extend(8u16.to_le_bytes());
            for e in [
                entry(256, 3, 1, 2),
                entry(257, 3, 1, 1),
                entry(258, 3, 1, 8),
                entry(259, 3, 1, 1),
                entry(262, 3, 1, 1),
                entry(273, 4, 1, data),
                entry(278, 3, 1, 1),
                entry(279, 4, 1, 2),
            ] {
                v.extend(e);
            }
            v.extend(next.to_le_bytes());
            v.extend([value, value]);
            v
        };
        let first = 8u32;
        let p1 = page(first, 0, 0);
        let second = first + p1.len() as u32;
        let mut bytes = b"II*\0".to_vec();
        bytes.extend(first.to_le_bytes());
        bytes.extend(page(first, second, 10));
        bytes.extend(page(second, 0, 200));
        let p = d.join("two.tif");
        std::fs::write(&p, &bytes).unwrap();
        assert_eq!(tiff_pages(&bytes).len(), 2);
        // The same with seeks, over the file: what facts reads, never the whole file.
        let mut f = std::fs::File::open(&p).unwrap();
        assert_eq!(tiff_pages_in(&mut f, bytes.len() as u64), tiff_pages(&bytes));
        let a = decode(&p, 0).unwrap();
        let b = decode(&p, 1).unwrap();
        assert_eq!((a.pages, a.image.dimensions(), a.image.get_pixel(0, 0)[0]), (2, (2, 1), 10));
        assert_eq!(b.image.get_pixel(1, 0)[0], 200, "the second page");
        assert_eq!(decode(&p, 9).unwrap().image.get_pixel(0, 0)[0], 200, "past the last: the last");
        let f = facts(&p).unwrap();
        assert!(f.iter().any(|(_, v)| v == "2"), "{f:?}");
        let out = d.join("out.png");
        assert_eq!(png(&p, 1, 2048, &out).unwrap(), 2);
        assert!(std::fs::read(&out).unwrap().starts_with(b"\x89PNG"));
        // A directory that points at itself ends the list.
        let mut looped = bytes.clone();
        let next_at = first as usize + 2 + 8 * 12;
        looped[next_at..next_at + 4].copy_from_slice(&first.to_le_bytes());
        assert_eq!(tiff_pages(&looped).len(), 1);
        hostile(&bytes, |b| {
            std::fs::write(&p, b).unwrap();
            let _ = decode(&p, 1);
            let _ = facts(&p);
        });
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn crate_formats_and_their_facts() {
        let d = folder("lib");
        let p = d.join("a.ppm");
        std::fs::write(&p, b"P6\n2 1\n255\n\xff\0\0\0\xff\0").unwrap();
        let pic = decode(&p, 0).unwrap();
        assert_eq!(pic.image.get_pixel(1, 0).0, [0, 255, 0, 255]);
        let f = facts(&p).unwrap();
        assert_eq!(f[0].1, "Netpbm");
        assert!(f[1].1.contains('2') && f[1].1.contains('1'));
        let q = d.join("a.qoi");
        let mut qoi = b"qoif".to_vec();
        qoi.extend(1u32.to_be_bytes());
        qoi.extend(1u32.to_be_bytes());
        qoi.extend([4, 0, 0xfe, 10, 20, 30, 0, 0, 0, 0, 0, 0, 0, 1]);
        std::fs::write(&q, &qoi).unwrap();
        assert_eq!(decode(&q, 0).unwrap().image.get_pixel(0, 0).0, [10, 20, 30, 255]);
        // A `.hdr` that is another program's header is no picture: shown as text.
        let h = d.join("scan.hdr");
        std::fs::write(&h, "ENVI\nsamples = 10\n").unwrap();
        assert!(facts(&h).is_none() && decode(&h, 0).is_err() && named(&h));
        assert!(limits().max_alloc <= Some(512 << 20), "no more than 512 MB for one picture");
        // Too many pixels are refused before they are made.
        let big = d.join("big.pgm");
        std::fs::write(&big, b"P5\n100000 100000\n255\n").unwrap();
        assert!(decode(&big, 0).unwrap_err().contains("100000"));
        hostile(&qoi, |b| {
            std::fs::write(&q, b).unwrap();
            let _ = decode(&q, 0);
        });
        std::fs::remove_dir_all(d).unwrap();
    }
}

//! Amiga IFF pictures: ILBM (bitplanes, with Extra Half-Brite, HAM6 and HAM8, and 24- and
//! 32-bit true colour) and Deluxe Paint's PBM (one byte a pixel), uncompressed or ByteRun1.

use image::RgbaImage;

use super::{Info, check_size, colours, grey, palette, unpack_bits};

/// What the chunks before the picture say.
struct Head {
    w: u32,
    h: u32,
    planes: u8,
    masking: u8,
    compressed: bool,
    transparent: u16,
    /// Deluxe Paint's chunky PBM rather than bitplanes.
    chunky: bool,
    cmap: Vec<[u8; 3]>,
    /// The Amiga's display mode (CAMG): HAM, Extra Half-Brite.
    mode: u32,
    body: Option<(usize, usize)>,
}

const HAM: u32 = 0x800;
const EHB: u32 = 0x80;

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn head(b: &[u8]) -> Option<Head> {
    if b.get(..4)? != b"FORM" {
        return None;
    }
    let chunky = b.get(8..12)? == b"PBM ";
    let mut h = Head { w: 0, h: 0, planes: 0, masking: 0, compressed: false, transparent: 0, chunky, cmap: vec![], mode: 0, body: None };
    let mut seen_bmhd = false;
    let mut at = 12usize;
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = be32(b, at + 4)? as usize;
        let start = at + 8;
        let end = start.saturating_add(len).min(b.len());
        let c = &b[start..end];
        match id {
            b"BMHD" if c.len() >= 20 => {
                h.w = be16(c, 0)?.into();
                h.h = be16(c, 2)?.into();
                h.planes = c[8];
                h.masking = c[9];
                h.compressed = c[10] == 1;
                h.transparent = be16(c, 12)?;
                seen_bmhd = true;
            }
            b"CMAP" => h.cmap = c.chunks_exact(3).take(256).map(|x| [x[0], x[1], x[2]]).collect(),
            b"CAMG" if c.len() >= 4 => h.mode = be32(c, 0)?,
            b"BODY" => {
                h.body = Some((start, end));
                break;
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        at = start.saturating_add(len).saturating_add(len & 1);
    }
    seen_bmhd.then_some(h)
}

/// Which kind of picture the header describes, in words.
fn describe(h: &Head) -> (String, &'static str) {
    let ham = h.mode & HAM != 0 && (h.planes == 6 || h.planes == 8);
    let ehb = !ham && h.mode & EHB != 0 && h.planes == 6;
    let format = if h.chunky { "Amiga IFF PBM (Deluxe Paint)" } else { "Amiga IFF ILBM" };
    let c = if ham {
        format!("HAM{}", h.planes)
    } else if ehb {
        format!("Extra Half-Brite, {}", palette(64))
    } else if h.planes >= 24 {
        colours(if h.planes == 32 { "RGBA" } else { "RGB" }, 8)
    } else if h.cmap.is_empty() {
        colours(&grey(), h.planes.into())
    } else {
        palette(1usize << h.planes.min(8))
    };
    (c, format)
}

pub(super) fn info(b: &[u8]) -> Option<Info> {
    let h = head(b)?;
    let (colours, format) = describe(&h);
    Some(Info { format, width: h.w.into(), height: h.h.into(), colours, ..Info::default() })
}

/// The palette as RGB: a CMAP written by old programs holds 4-bit values (0x0F), made 8-bit.
fn palette_of(h: &Head) -> Vec<[u8; 3]> {
    let n = 1usize << h.planes.min(8);
    if h.cmap.is_empty() {
        let top = (n - 1).max(1);
        return (0..n).map(|i| [(i * 255 / top) as u8; 3]).collect();
    }
    let four_bit = h.cmap.iter().flatten().all(|v| v & 0xf0 == 0);
    let mut p: Vec<[u8; 3]> = h.cmap.iter().map(|c| if four_bit { c.map(|v| v * 17) } else { *c }).collect();
    p.resize(n.max(p.len()), [0, 0, 0]);
    p
}

pub(super) fn decode(b: &[u8]) -> Result<RgbaImage, String> {
    let bad = |why: &str| crate::t!("pictures.unreadable", "why" => why);
    let h = head(b).ok_or_else(|| bad("no BMHD"))?;
    check_size(h.w.into(), h.h.into())?;
    let (start, end) = h.body.ok_or_else(|| bad("no BODY"))?;
    let body = &b[start..end];
    let (w, hgt) = (h.w as usize, h.h as usize);
    if !h.chunky && !(1..=8).contains(&h.planes) && h.planes != 24 && h.planes != 32 {
        return Err(bad(&format!("{} bitplanes", h.planes)));
    }
    // One row of each plane (and of the mask) has a whole number of 16-bit words.
    let row_bytes = if h.chunky { w + (w & 1) } else { w.div_ceil(16) * 2 };
    let planes_in_row = if h.chunky { 1 } else { h.planes as usize + usize::from(h.masking == 1) };
    let row_len = row_bytes * planes_in_row;
    let mut img = RgbaImage::new(h.w, h.h);
    let pal = palette_of(&h);
    let ham = !h.chunky && h.mode & HAM != 0 && (h.planes == 6 || h.planes == 8);
    let ehb = !h.chunky && !ham && h.mode & EHB != 0 && h.planes == 6;
    let (mut at, mut row) = (0usize, Vec::with_capacity(row_len));
    let mut indices = vec![0u32; w];
    for y in 0..hgt {
        row.clear();
        if h.compressed {
            unpack_bits(body, &mut at, &mut row, row_len);
        } else {
            let take = row_len.min(body.len().saturating_sub(at));
            row.extend_from_slice(&body[at..at + take]);
            at += take;
        }
        // A file that ends early shows what it has, the rest black.
        row.resize(row_len, 0);
        if h.chunky {
            for (x, i) in indices.iter_mut().enumerate() {
                *i = row[x].into();
            }
        } else {
            indices.fill(0);
            for p in 0..h.planes as usize {
                let plane = &row[p * row_bytes..(p + 1) * row_bytes];
                for (x, i) in indices.iter_mut().enumerate() {
                    if plane[x >> 3] & (0x80 >> (x & 7)) != 0 {
                        *i |= 1 << p;
                    }
                }
            }
        }
        // HAM starts each row from the background colour.
        let mut last = pal[0];
        for (x, &v) in indices.iter().enumerate() {
            let px = if h.planes >= 24 && !h.chunky {
                let alpha = if h.planes == 32 { (v >> 24) as u8 } else { 255 };
                [v as u8, (v >> 8) as u8, (v >> 16) as u8, alpha]
            } else if ham {
                let (bits, ctl) = if h.planes == 6 { (4, v >> 4) } else { (6, v >> 6) };
                let val = v & ((1 << bits) - 1);
                let wide = if bits == 4 { (val * 17) as u8 } else { ((val << 2) | (val >> 4)) as u8 };
                last = match ctl {
                    0 => pal[val as usize],
                    1 => [last[0], last[1], wide],
                    2 => [wide, last[1], last[2]],
                    _ => [last[0], wide, last[2]],
                };
                [last[0], last[1], last[2], 255]
            } else if ehb && v >= 32 {
                let c = pal[(v - 32) as usize];
                [c[0] / 2, c[1] / 2, c[2] / 2, 255]
            } else {
                let c = pal.get(v as usize).copied().unwrap_or_default();
                let clear = h.masking == 2 && v == u32::from(h.transparent);
                [c[0], c[1], c[2], if clear { 0 } else { 255 }]
            };
            img.put_pixel(x as u32, y as u32, image::Rgba(px));
        }
    }
    Ok(img)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An ILBM of `w`×`h` with `planes` bitplanes, a palette, a display mode and a BODY given
    /// as rows of plane bytes, ByteRun1 when `packed`.
    pub fn ilbm(w: u16, h: u16, planes: u8, cmap: &[[u8; 3]], mode: u32, body: &[u8], packed: bool) -> Vec<u8> {
        let chunk = |id: &[u8], data: &[u8]| -> Vec<u8> {
            let mut c = id.to_vec();
            c.extend((data.len() as u32).to_be_bytes());
            c.extend(data);
            if data.len() % 2 == 1 {
                c.push(0);
            }
            c
        };
        let mut bmhd = vec![];
        bmhd.extend(w.to_be_bytes());
        bmhd.extend(h.to_be_bytes());
        bmhd.extend([0, 0, 0, 0, planes, 0, u8::from(packed), 0, 0, 0, 10, 11]);
        bmhd.extend(w.to_be_bytes());
        bmhd.extend(h.to_be_bytes());
        let mut inner = b"ILBM".to_vec();
        inner.extend(chunk(b"BMHD", &bmhd));
        inner.extend(chunk(b"CMAP", &cmap.concat()));
        inner.extend(chunk(b"CAMG", &mode.to_be_bytes()));
        inner.extend(chunk(b"BODY", body));
        let mut out = b"FORM".to_vec();
        out.extend((inner.len() as u32).to_be_bytes());
        out.extend(inner);
        out
    }

    #[test]
    fn ilbm_bitplanes_ham_and_ehb() {
        // 2 planes, 16 pixels wide: plane 0 is 0xFF00, plane 1 is 0x0F0F.
        let pal = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let raw = [0xff, 0x00, 0x0f, 0x0f];
        let b = ilbm(16, 1, 2, &pal, 0, &raw, false);
        let img = decode(&b).unwrap();
        // x=0: plane0 1, plane1 0 → 1 (red); x=4: 1 and 1 → 3 (blue); x=12: 0 and 1 → 2.
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(4, 0).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(12, 0).0, [0, 255, 0, 255]);
        assert_eq!(img.get_pixel(8, 0).0, [0, 0, 0, 255]);
        // The same row packed with ByteRun1: a literal of 2, a run of 2 × 0x0F.
        let packed = [1, 0xff, 0x00, 0xff, 0x0f];
        assert_eq!(decode(&ilbm(16, 1, 2, &pal, 0, &packed, true)).unwrap(), img);
        let i = info(&b).unwrap();
        assert_eq!((i.width, i.height, i.format), (16, 1, "Amiga IFF ILBM"));

        // HAM6: pixel 0 palette 1 (red), pixel 1 "modify blue" to 15.
        let mut planes = [[0u8; 2]; 6];
        // Pixel 0: value 0b00_0001 → plane 0. Pixel 1: 0b01_1111 → planes 0-4.
        planes[0][0] = 0b1100_0000;
        for p in planes.iter_mut().take(4).skip(1) {
            p[0] = 0b0100_0000;
        }
        planes[4][0] = 0b0100_0000;
        let mut pal16 = vec![[0, 0, 0]; 16];
        pal16[1] = [255, 0, 0];
        let ham = decode(&ilbm(16, 1, 6, &pal16, HAM, &planes.concat(), false)).unwrap();
        assert_eq!(ham.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(ham.get_pixel(1, 0).0, [255, 0, 255, 255], "red kept, blue set");
        assert!(info(&ilbm(16, 1, 6, &pal16, HAM, &planes.concat(), false)).unwrap().colours.contains("HAM6"));

        // EHB: index 33 is colour 1 at half brightness.
        let mut planes = [[0u8; 2]; 6];
        planes[0][0] = 0x80;
        planes[5][0] = 0x80;
        let mut pal32 = vec![[0, 0, 0]; 32];
        pal32[1] = [200, 100, 50];
        let ehb = decode(&ilbm(16, 1, 6, &pal32, EHB, &planes.concat(), false)).unwrap();
        assert_eq!(ehb.get_pixel(0, 0).0, [100, 50, 25, 255]);
    }

    #[test]
    fn ilbm_holds_for_any_bytes() {
        let pal = [[0, 0, 0], [255, 255, 255]];
        let b = ilbm(40, 30, 1, &pal, 0, &[0x81; 40], true);
        super::super::tests::hostile(&b, |b| {
            let _ = decode(b);
            let _ = info(b);
        });
        // A huge size from a spoiled header is refused, not allocated.
        let big = ilbm(65535, 65535, 1, &pal, 0, &[], false);
        assert!(decode(&big).is_err());
    }
}

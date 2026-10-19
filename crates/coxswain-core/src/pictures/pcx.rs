//! ZSoft PCX: 1, 2, 4 and 8 bits a pixel in one plane, 16-colour EGA planes, and 24- and
//! 32-bit colour in three or four planes of 8 bits; run-length encoded or not.

use image::RgbaImage;

use super::{Info, check_size, colours, palette};

struct Head {
    w: u32,
    h: u32,
    bits: u8,
    planes: u8,
    line: usize,
    rle: bool,
}

fn head(b: &[u8]) -> Option<Head> {
    if b.len() < 128 || b[0] != 0x0a {
        return None;
    }
    let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let (x0, y0, x1, y1) = (u16le(4), u16le(6), u16le(8), u16le(10));
    if x1 < x0 || y1 < y0 {
        return None;
    }
    Some(Head {
        w: u32::from(x1 - x0) + 1,
        h: u32::from(y1 - y0) + 1,
        bits: b[3],
        planes: b[65],
        line: u16le(66).into(),
        rle: b[2] == 1,
    })
}

fn describe(h: &Head) -> String {
    match (h.bits, h.planes) {
        (8, 3) => colours("RGB", 8),
        (8, 4) => colours("RGBA", 8),
        (1, 1) => crate::t!("facts.black_white"),
        (b, p) => palette(1usize << (u32::from(b) * u32::from(p)).min(8)),
    }
}

pub(super) fn info(b: &[u8]) -> Option<Info> {
    let h = head(b)?;
    Some(Info { width: h.w.into(), height: h.h.into(), colours: describe(&h), ..Info::default() })
}

pub(super) fn decode(b: &[u8]) -> Result<RgbaImage, String> {
    let bad = |why: &str| crate::t!("pictures.unreadable", "why" => why);
    let h = head(b).ok_or_else(|| bad("header"))?;
    check_size(h.w.into(), h.h.into())?;
    let (w, hgt) = (h.w as usize, h.h as usize);
    let need = (w * usize::from(h.bits)).div_ceil(8);
    if !matches!((h.bits, h.planes), (1 | 2 | 4 | 8, 1) | (1, 2..=4) | (8, 3 | 4)) || h.line < need {
        return Err(bad(&format!("{} bits, {} planes", h.bits, h.planes)));
    }
    let scan = h.line * usize::from(h.planes);
    // The 16 colours of the header, and a VGA palette of 256 after the pixels (0x0C, then 768).
    let ega: Vec<[u8; 3]> = b[16..64].chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    let vga = (b.len() >= 128 + 769 && b[b.len() - 769] == 0x0c)
        .then(|| b[b.len() - 768..].chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>());
    let pal: Vec<[u8; 3]> = match (h.bits, h.planes) {
        (8, 1) => vga.unwrap_or_else(|| (0..256).map(|i| [i as u8; 3]).collect()),
        (1, 1) => vec![[0, 0, 0], [255, 255, 255]],
        _ => ega,
    };
    let data = &b[128..];
    let mut img = RgbaImage::new(h.w, h.h);
    let (mut at, mut row) = (0usize, Vec::with_capacity(scan));
    for y in 0..hgt {
        row.clear();
        while row.len() < scan && at < data.len() {
            let c = data[at];
            at += 1;
            if h.rle && c & 0xc0 == 0xc0 {
                let Some(&v) = data.get(at) else { break };
                at += 1;
                let n = usize::from(c & 0x3f).min(scan - row.len());
                row.resize(row.len() + n, v);
            } else {
                row.push(c);
            }
        }
        row.resize(scan, 0);
        let plane = |p: usize| &row[p * h.line..(p + 1) * h.line];
        for x in 0..w {
            let px = match (h.bits, h.planes) {
                (8, 3 | 4) => {
                    let a = if h.planes == 4 { plane(3)[x] } else { 255 };
                    [plane(0)[x], plane(1)[x], plane(2)[x], a]
                }
                (1, p) => {
                    let mut i = 0usize;
                    for k in 0..usize::from(p) {
                        if plane(k)[x >> 3] & (0x80 >> (x & 7)) != 0 {
                            i |= 1 << k;
                        }
                    }
                    let c = pal.get(i).copied().unwrap_or_default();
                    [c[0], c[1], c[2], 255]
                }
                (bits, _) => {
                    let bits = usize::from(bits);
                    let bit = x * bits;
                    let byte = plane(0)[bit >> 3];
                    let i = usize::from(byte >> (8 - bits - (bit & 7))) & ((1 << bits) - 1);
                    let c = pal.get(i).copied().unwrap_or_default();
                    [c[0], c[1], c[2], 255]
                }
            };
            img.put_pixel(x as u32, y as u32, image::Rgba(px));
        }
    }
    Ok(img)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An 8-bit PCX of `w`×`h` with a grey VGA palette, its pixels run-length encoded.
    pub fn pcx8(w: u16, h: u16, pixels: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0] = 0x0a;
        b[1] = 5;
        b[2] = 1;
        b[3] = 8;
        b[8..10].copy_from_slice(&(w - 1).to_le_bytes());
        b[10..12].copy_from_slice(&(h - 1).to_le_bytes());
        b[65] = 1;
        let line = w + (w & 1);
        b[66..68].copy_from_slice(&line.to_le_bytes());
        for row in pixels.chunks(w as usize) {
            let mut row = row.to_vec();
            row.resize(line as usize, 0);
            for v in row {
                if v >= 0xc0 {
                    b.push(0xc1);
                }
                b.push(v);
            }
        }
        b.push(0x0c);
        for i in 0..=255u8 {
            b.extend([i, i, 255 - i]);
        }
        b
    }

    #[test]
    fn pcx_eight_bits_and_planes() {
        let b = pcx8(3, 2, &[0, 200, 7, 1, 2, 255]);
        let img = decode(&b).unwrap();
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(img.get_pixel(1, 0).0, [200, 200, 55, 255], "from the VGA palette");
        assert_eq!(img.get_pixel(2, 1).0, [255, 255, 0, 255]);
        assert_eq!(info(&b).unwrap().width, 3);
        // 24-bit: three planes of one byte each, a run of 2 for the red plane.
        let mut rgb = b[..128].to_vec();
        rgb[65] = 3;
        rgb[8..10].copy_from_slice(&1u16.to_le_bytes());
        rgb[10..12].copy_from_slice(&0u16.to_le_bytes());
        rgb[66..68].copy_from_slice(&2u16.to_le_bytes());
        rgb.extend([0xc2, 9, 1, 2, 3, 4]);
        let img = decode(&rgb).unwrap();
        assert_eq!(img.get_pixel(1, 0).0, [9, 2, 4, 255]);
        super::super::tests::hostile(&b, |b| {
            let _ = decode(b);
            let _ = info(b);
        });
    }
}

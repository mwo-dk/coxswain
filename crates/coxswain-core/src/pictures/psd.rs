//! Photoshop (`.psd`, and `.psb` for large documents): the flattened picture Photoshop keeps
//! after the layers ("maximize compatibility"), raw or PackBits, in bitmap, grey, indexed,
//! RGB, CMYK, duotone or multichannel mode, 1, 8, 16 or 32 bits. Its layers are counted, not
//! drawn.

use std::io::{Read, Seek, SeekFrom};

use image::RgbaImage;

use super::{Info, check_size, colours, grey, palette, unpack_bits};

struct Head {
    big: bool,
    channels: usize,
    h: u32,
    w: u32,
    depth: u16,
    mode: u16,
}

fn head(b: &[u8]) -> Option<Head> {
    if b.len() < 26 || &b[..4] != b"8BPS" {
        return None;
    }
    let be16 = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
    let be32 = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    Some(Head { big: be16(4) == 2, channels: be16(12).into(), h: be32(14), w: be32(18), depth: be16(22), mode: be16(24) })
}

fn mode_name(h: &Head) -> String {
    let bits = u32::from(h.depth);
    match h.mode {
        0 => crate::t!("facts.black_white"),
        1 | 8 => colours(&grey(), bits),
        2 => palette(256),
        3 => colours(if h.channels > 3 { "RGBA" } else { "RGB" }, bits),
        4 => colours("CMYK", bits),
        7 => colours("Multichannel", bits),
        9 => colours("Lab", bits),
        m => format!("mode {m}"),
    }
}

/// Facts from the header and the lengths of the sections before the picture, read with seeks:
/// a big document's resources and layers are not read.
pub(super) fn info(r: &mut (impl Read + Seek)) -> Option<Info> {
    let mut b = [0u8; 26];
    r.read_exact(&mut b).ok()?;
    let h = head(&b)?;
    let colour_data = len32_of(r)?;
    r.seek(SeekFrom::Current(colour_data as i64)).ok()?;
    let resources = len32_of(r)?;
    r.seek(SeekFrom::Current(resources as i64)).ok()?;
    // The layer and mask section's length, then the layer info's, then the count of layers
    // (negative when the first alpha channel is the merged picture's transparency).
    let section = if h.big { len64_of(r)? } else { len32_of(r)? };
    let layers = if section == 0 {
        Some(0)
    } else {
        let info = if h.big { len64_of(r)? } else { len32_of(r)? };
        if info == 0 {
            Some(0)
        } else {
            let mut c = [0u8; 2];
            r.read_exact(&mut c).ok()?;
            Some(i16::from_be_bytes(c).unsigned_abs().into())
        }
    };
    Some(Info { format: "Photoshop", width: h.w.into(), height: h.h.into(), colours: mode_name(&h), layers, pages: None })
}

fn len32_of(r: &mut impl Read) -> Option<u64> {
    let mut x = [0u8; 4];
    r.read_exact(&mut x).ok()?;
    Some(u32::from_be_bytes(x).into())
}

fn len64_of(r: &mut impl Read) -> Option<u64> {
    let mut x = [0u8; 8];
    r.read_exact(&mut x).ok()?;
    Some(u64::from_be_bytes(x))
}

pub(super) fn decode(b: &[u8]) -> Result<RgbaImage, String> {
    let bad = |why: &str| crate::t!("pictures.unreadable", "why" => why);
    let h = head(b).ok_or_else(|| bad("header"))?;
    check_size(h.w.into(), h.h.into())?;
    if !(1..=56).contains(&h.channels) || ![1, 8, 16, 32].contains(&h.depth) {
        return Err(bad(&format!("{} channels of {} bits", h.channels, h.depth)));
    }
    if h.mode == 9 {
        return Err(bad("Lab"));
    }
    // The sections before the picture, each a length and its bytes.
    let mut at = 26usize;
    let take = |at: &mut usize, n: usize| -> Option<usize> {
        let v = b.get(*at..at.checked_add(n)?)?.iter().fold(0usize, |v, x| v.saturating_mul(256).saturating_add(usize::from(*x)));
        *at += n;
        Some(v)
    };
    let colour_len = take(&mut at, 4).ok_or_else(|| bad("colour data"))?;
    let colour_data = b.get(at..at.saturating_add(colour_len)).ok_or_else(|| bad("colour data"))?;
    at += colour_len;
    let res = take(&mut at, 4).ok_or_else(|| bad("resources"))?;
    at = at.saturating_add(res);
    let layers = take(&mut at, if h.big { 8 } else { 4 }).ok_or_else(|| bad("layers"))?;
    at = at.saturating_add(layers);
    let compression = take(&mut at, 2).ok_or_else(|| bad("picture"))?;
    let (w, hgt) = (h.w as usize, h.h as usize);
    let row = (w * usize::from(h.depth)).div_ceil(8);
    // Only the channels that make the picture are read: the first four at most.
    let wanted = h.channels.min(4);
    if (row * hgt).saturating_mul(wanted) > (super::MAX_PIXELS * 8) as usize {
        return Err(super::too_large(h.w.into(), h.h.into()));
    }
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(wanted);
    match compression {
        0 => {
            for c in 0..wanted {
                let start = at.saturating_add(c * row * hgt);
                let rest = b.get(start.min(b.len())..).unwrap_or_default();
                let mut p = rest[..rest.len().min(row * hgt)].to_vec();
                p.resize(row * hgt, 0);
                planes.push(p);
            }
        }
        1 => {
            // A count of bytes for every row of every channel, then the rows.
            let count = if h.big { 4 } else { 2 };
            let mut data = at.saturating_add(h.channels * hgt * count);
            for c in 0..wanted {
                let mut p = Vec::with_capacity(row * hgt);
                for y in 0..hgt {
                    let mut i = at + (c * hgt + y) * count;
                    let n = take(&mut i, count).unwrap_or(0);
                    let end = data.saturating_add(n).min(b.len());
                    let src = b.get(data.min(end)..end).unwrap_or_default();
                    let mut pos = 0;
                    unpack_bits(src, &mut pos, &mut p, row);
                    p.resize((y + 1) * row, 0);
                    data = end;
                }
                planes.push(p);
            }
        }
        _ => return Err(bad("ZIP")),
    }
    let sample = |p: &[u8], i: usize| -> u8 {
        match h.depth {
            16 => p[i * 2],
            32 => (f32::from_be_bytes([p[i * 4], p[i * 4 + 1], p[i * 4 + 2], p[i * 4 + 3]]).clamp(0.0, 1.0) * 255.0) as u8,
            1 => if p[i >> 3] & (0x80 >> (i & 7)) != 0 { 0 } else { 255 },
            _ => p[i],
        }
    };
    let mut img = RgbaImage::new(h.w, h.h);
    for y in 0..hgt {
        for x in 0..w {
            let at_row = |c: usize| &planes[c][y * row..(y + 1) * row];
            let s = |c: usize| sample(at_row(c), x);
            let px = match h.mode {
                3 if wanted >= 3 => [s(0), s(1), s(2), if wanted >= 4 { s(3) } else { 255 }],
                4 if wanted >= 4 => {
                    // Photoshop keeps CMYK as 255 less the ink.
                    let k = u16::from(s(3));
                    let mix = |v: u8| (u16::from(v) * k / 255) as u8;
                    [mix(s(0)), mix(s(1)), mix(s(2)), 255]
                }
                2 => {
                    let i = usize::from(s(0));
                    // The palette: 256 reds, then the greens, then the blues.
                    let c = |k: usize| colour_data.get(k * 256 + i).copied().unwrap_or(0);
                    [c(0), c(1), c(2), 255]
                }
                _ => {
                    let g = s(0);
                    let a = if wanted >= 2 && matches!(h.mode, 1 | 8) { s(1) } else { 255 };
                    [g, g, g, a]
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

    /// A PSD of `w`×`h` in RGB, 8 bits, with three layers, its picture raw or PackBits.
    pub fn psd(w: u32, h: u32, rgb: [u8; 3], packed: bool) -> Vec<u8> {
        let mut b = b"8BPS".to_vec();
        b.extend(1u16.to_be_bytes());
        b.extend([0; 6]);
        b.extend(3u16.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend(w.to_be_bytes());
        b.extend(8u16.to_be_bytes());
        b.extend(3u16.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend(4u32.to_be_bytes());
        b.extend([1, 2, 3, 4]);
        // Layer and mask section: its layer info holds a count of -3.
        b.extend(6u32.to_be_bytes());
        b.extend(2u32.to_be_bytes());
        b.extend((-3i16).to_be_bytes());
        b.extend(u16::from(packed).to_be_bytes());
        if packed {
            for _ in 0..3 * h {
                b.extend(2u16.to_be_bytes());
            }
            for c in rgb {
                for _ in 0..h {
                    b.extend([(1 - w as i32) as u8, c]);
                }
            }
        } else {
            for c in rgb {
                b.extend(std::iter::repeat_n(c, (w * h) as usize));
            }
        }
        b
    }

    #[test]
    fn psd_flattened_picture_and_layers() {
        for packed in [false, true] {
            let b = psd(4, 3, [10, 20, 30], packed);
            let img = decode(&b).unwrap();
            assert_eq!(img.dimensions(), (4, 3));
            assert_eq!(img.get_pixel(3, 2).0, [10, 20, 30, 255], "packed: {packed}");
            let i = info(&mut std::io::Cursor::new(&b)).unwrap();
            assert_eq!((i.width, i.height, i.layers), (4, 3, Some(3)));
            super::super::tests::hostile(&b, |b| {
                let _ = decode(b);
                let _ = info(&mut std::io::Cursor::new(b));
            });
        }
    }
}

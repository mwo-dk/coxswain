//! X11's pictures written as C source: XPM (version 3, colours by `#rgb` or by a few names,
//! `None` for clear) and XBM (bits, as X10's shorts or X11's chars).

use std::collections::HashMap;

use image::RgbaImage;

use super::{Info, check_size, palette};

/// The most colours an XPM may name.
const MAX_COLOURS: usize = 1 << 20;

pub(super) fn is_xpm(h: &[u8]) -> bool {
    let s = String::from_utf8_lossy(h);
    s.contains("XPM") || s.contains("static char")
}

pub(super) fn is_xbm(h: &[u8]) -> bool {
    h.trim_ascii_start().starts_with(b"#define")
}

/// The C strings of the file, in order, comments left out. Escapes are kept as they are:
/// XPM's strings have none that matter.
fn strings(b: &[u8]) -> Vec<&[u8]> {
    let (mut out, mut at) = (vec![], 0);
    while at < b.len() {
        match b[at] {
            b'/' if b.get(at + 1) == Some(&b'*') => {
                at = memchr::memmem::find(&b[at + 2..], b"*/").map_or(b.len(), |e| at + 2 + e + 2);
            }
            b'"' => {
                let start = at + 1;
                let mut end = start;
                while end < b.len() && b[end] != b'"' {
                    end += if b[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(b.len());
                out.push(&b[start..end]);
                at = end + 1;
            }
            _ => at += 1,
        }
    }
    out
}

/// Width, height, colours and characters a pixel, from XPM's first string.
fn values(s: &[u8]) -> Option<(u32, u32, usize, usize)> {
    let mut it = std::str::from_utf8(s).ok()?.split_ascii_whitespace().map(str::parse::<u64>);
    let (w, h, n, cpp) = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    (w <= u32::MAX.into() && h <= u32::MAX.into() && (1..=8).contains(&cpp) && n as usize <= MAX_COLOURS)
        .then_some((w as u32, h as u32, n as usize, cpp as usize))
}

pub(super) fn info_xpm(b: &[u8]) -> Option<Info> {
    let s = strings(b);
    let (w, h, n, _) = values(s.first()?)?;
    Some(Info { width: w.into(), height: h.into(), colours: palette(n), ..Info::default() })
}

/// A colour of an XPM: `#rgb` in 1 to 4 hex digits each, `None`, or one of X11's commonest names.
fn colour(v: &str) -> Option<[u8; 4]> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some([0, 0, 0, 0]);
    }
    if let Some(hex) = v.strip_prefix('#') {
        let d = hex.len() / 3;
        if d == 0 || d > 4 || hex.len() % 3 != 0 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let part = |i: usize| {
            let x = u32::from_str_radix(&hex[i * d..(i + 1) * d], 16).unwrap_or(0);
            (x * 255 / ((1 << (4 * d)) - 1)) as u8
        };
        return Some([part(0), part(1), part(2), 255]);
    }
    let named: &[(&str, [u8; 3])] = &[
        ("black", [0, 0, 0]),
        ("white", [255, 255, 255]),
        ("red", [255, 0, 0]),
        ("green", [0, 255, 0]),
        ("blue", [0, 0, 255]),
        ("yellow", [255, 255, 0]),
        ("cyan", [0, 255, 255]),
        ("magenta", [255, 0, 255]),
        ("gray", [190, 190, 190]),
        ("grey", [190, 190, 190]),
        ("orange", [255, 165, 0]),
        ("brown", [165, 42, 42]),
        ("navy", [0, 0, 128]),
    ];
    let lower = v.to_ascii_lowercase().replace(' ', "");
    // `gray50`, `grey75`: a grey of that percentage.
    if let Some(p) = lower.strip_prefix("gray").or_else(|| lower.strip_prefix("grey")).and_then(|p| p.parse::<u32>().ok()) {
        let g = (p.min(100) * 255 / 100) as u8;
        return Some([g, g, g, 255]);
    }
    named.iter().find(|(n, _)| *n == lower).map(|(_, c)| [c[0], c[1], c[2], 255]).or(Some([128, 128, 128, 255]))
}

pub(super) fn decode_xpm(b: &[u8]) -> Result<RgbaImage, String> {
    let bad = |why: &str| crate::t!("pictures.unreadable", "why" => why);
    let s = strings(b);
    let (w, h, n, cpp) = s.first().and_then(|v| values(v)).ok_or_else(|| bad("values"))?;
    check_size(w.into(), h.into())?;
    if s.len() < 1 + n {
        return Err(bad("colours"));
    }
    let mut map: HashMap<&[u8], [u8; 4]> = HashMap::with_capacity(n.min(4096));
    for line in &s[1..1 + n] {
        if line.len() < cpp {
            continue;
        }
        let (key, rest) = line.split_at(cpp);
        // Pairs of a context and a colour: the colour one (`c`) first, else grey, else mono.
        let words: Vec<&str> = std::str::from_utf8(rest).unwrap_or_default().split_ascii_whitespace().collect();
        let pick = |ctx: &str| {
            let at = words.iter().position(|w| *w == ctx)?;
            // A name may have spaces: up to the next context word.
            let end = words[at + 1..].iter().position(|w| ["c", "m", "g", "g4", "s"].contains(w)).map_or(words.len(), |e| at + 1 + e);
            colour(&words.get(at + 1..end)?.join(" "))
        };
        let c = pick("c").or_else(|| pick("g")).or_else(|| pick("g4")).or_else(|| pick("m")).unwrap_or([0, 0, 0, 255]);
        map.insert(key, c);
    }
    let mut img = RgbaImage::new(w, h);
    for (y, row) in s[1 + n..].iter().take(h as usize).enumerate() {
        for (x, px) in row.chunks_exact(cpp).take(w as usize).enumerate() {
            if let Some(c) = map.get(px) {
                img.put_pixel(x as u32, y as u32, image::Rgba(*c));
            }
        }
    }
    Ok(img)
}

/// XBM's width and height (`#define name_width 16`), and whether its bits are shorts (X10).
fn xbm_head(b: &[u8]) -> Option<(u32, u32, bool)> {
    let text = String::from_utf8_lossy(&b[..b.len().min(4096)]);
    let mut w = None;
    let mut h = None;
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        if it.next() != Some("#define") {
            continue;
        }
        let (name, value) = (it.next()?, it.next().and_then(|v| v.parse::<u32>().ok()));
        if name.ends_with("_width") {
            w = value;
        } else if name.ends_with("_height") {
            h = value;
        }
    }
    let short = text.split('{').next().is_some_and(|before| before.contains("short"));
    Some((w?, h?, short))
}

pub(super) fn info_xbm(b: &[u8]) -> Option<Info> {
    let (w, h, _) = xbm_head(b)?;
    Some(Info { width: w.into(), height: h.into(), colours: crate::t!("facts.black_white"), ..Info::default() })
}

pub(super) fn decode_xbm(b: &[u8]) -> Result<RgbaImage, String> {
    let bad = |why: &str| crate::t!("pictures.unreadable", "why" => why);
    let (w, h, short) = xbm_head(b).ok_or_else(|| bad("width and height"))?;
    check_size(w.into(), h.into())?;
    let start = memchr::memchr(b'{', b).ok_or_else(|| bad("bits"))? + 1;
    // Each value's bits, the first pixel in the lowest; a row starts on a whole value.
    let mut bytes = vec![];
    for word in b[start..].split(|c| *c == b',' || *c == b'}') {
        let word = std::str::from_utf8(word).unwrap_or_default().trim();
        let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) else { continue };
        let Ok(v) = u16::from_str_radix(hex, 16) else { continue };
        if short {
            bytes.extend(v.to_le_bytes());
        } else {
            bytes.push(v as u8);
        }
    }
    let unit = if short { 16 } else { 8 };
    let row = (w as usize).div_ceil(unit) * unit / 8;
    let mut img = RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    for y in 0..h as usize {
        for x in 0..w as usize {
            if bytes.get(y * row + x / 8).is_some_and(|v| v & (1 << (x % 8)) != 0) {
                img.put_pixel(x as u32, y as u32, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const XPM: &str = r##"/* XPM */
static char *rocket[] = {
/* width height colours chars */
"3 2 3 1",
"  c None",
". c #FF0000",
"X c gray50 m black",
" .X",
"X. "
};
"##;

    #[test]
    fn xpm_colours_and_clear() {
        let img = decode_xpm(XPM.as_bytes()).unwrap();
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(img.get_pixel(0, 0).0[3], 0, "None is clear");
        assert_eq!(img.get_pixel(1, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(2, 0).0, [127, 127, 127, 255]);
        assert_eq!(colour("#fff0").map(|c| c[3]), None, "three equal parts or none");
        assert_eq!(colour("#ffff00000000"), Some([255, 0, 0, 255]));
        assert!(is_xpm(XPM.as_bytes()));
        assert_eq!(info_xpm(XPM.as_bytes()).unwrap().height, 2);
        super::super::tests::hostile(XPM.as_bytes(), |b| {
            let _ = decode_xpm(b);
        });
    }

    #[test]
    fn xbm_bits_lowest_first() {
        let x = "#define dot_width 10\n#define dot_height 2\nstatic unsigned char dot_bits[] = {\n 0x01, 0x02, 0x00, 0x00 };\n";
        let img = decode_xbm(x.as_bytes()).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(9, 0).0, [0, 0, 0, 255], "the second byte's bit 1: x = 9");
        assert_eq!(img.get_pixel(1, 0).0, [255, 255, 255, 255]);
        let x10 = "#define a_width 2\n#define a_height 1\nstatic short a_bits[] = { 0x0002 };";
        assert_eq!(decode_xbm(x10.as_bytes()).unwrap().get_pixel(1, 0).0, [0, 0, 0, 255]);
        super::super::tests::hostile(x.as_bytes(), |b| {
            let _ = decode_xbm(b);
        });
    }
}

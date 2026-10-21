//! Plain text: no zero byte at its start, and UTF-8. Text, code, Markdown, logs,
//! configuration, and everything else that is written to be read as it is. UTF-8 with a stray
//! byte keeps all its text; text in another encoding keeps what comes before its first byte
//! that is no UTF-8.

use std::io::Read;
use std::path::Path;

pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8192];
    let n = f.read(&mut head).ok()?;
    if head[..n].contains(&0) {
        return None;
    }
    let mut bytes = head[..n].to_vec();
    f.take(max).read_to_end(&mut bytes).ok()?;
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) if super::nearly_utf8(e.as_bytes()) => {
            Some(String::from_utf8_lossy(e.as_bytes()).into_owned())
        }
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()
        }
    }
}

/// The most of one line read: a line longer than this (a file of one endless line) is cut
/// here. No line of text is this long; the text kept stops at `MAX_TEXT` long before.
const MAX_LINE: u64 = 64 << 20;

/// `text`, tidied (`super::tidy`, or `super::tidy_code` for `code`), read line by line as it
/// is tidied: no more of the file is read than the text kept needs, whatever the file's size.
/// The same text comes out as from `tidy(&text(path, …))`: a file of UTF-8 with a stray byte
/// keeps all its text, one in another encoding what comes before its first byte that is no
/// UTF-8 (decided by its first 64 KB, as `text` decides).
pub fn tidied(path: &Path, code: bool) -> Option<String> {
    let mut f = std::io::BufReader::with_capacity(1 << 16, std::fs::File::open(path).ok()?);
    let mut head = Vec::with_capacity(1 << 16);
    (&mut f).take(1 << 16).read_to_end(&mut head).ok()?;
    if head[..head.len().min(8192)].contains(&0) {
        return None;
    }
    let mut r = std::io::Cursor::new(head.clone()).chain(f);
    let mut t = super::Tidy::new(code);
    // How a byte that is no UTF-8 is taken: decided at the first such byte.
    let mut lossy = None;
    let mut line = vec![];
    loop {
        line.clear();
        let n = std::io::BufRead::read_until(&mut (&mut r).take(MAX_LINE), b'\n', &mut line).ok()?;
        if n == 0 {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        let go = match std::str::from_utf8(&line) {
            Ok(l) => t.push(l),
            Err(e) => match *lossy.get_or_insert_with(|| super::nearly_utf8(&head)) {
                true => t.push(&String::from_utf8_lossy(&line)),
                // The text ends where its first byte that is no UTF-8 is.
                false => {
                    t.push(std::str::from_utf8(&line[..e.valid_up_to()]).unwrap_or_default());
                    false
                }
            },
        };
        if !go {
            break;
        }
    }
    Some(t.out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stray_byte_costs_a_letter_and_another_encoding_what_follows_it() {
        let d = crate::extract::tests::folder("plain");
        let utf8 = [
            &"smørrebrød på øen ".repeat(20).into_bytes()[..],
            b"caf\xe9 ",
            b"after",
        ]
        .concat();
        std::fs::write(d.join("stray.txt"), &utf8).unwrap();
        let text = super::text(&d.join("stray.txt"), u64::MAX).unwrap();
        assert!(text.ends_with("caf\u{fffd} after"), "{text}");
        std::fs::write(d.join("latin.txt"), b"plain words, caf\xe9 and after").unwrap();
        assert_eq!(
            super::text(&d.join("latin.txt"), u64::MAX).as_deref(),
            Some("plain words, caf")
        );
        std::fs::write(d.join("zero.bin"), b"a\0b").unwrap();
        assert_eq!(super::text(&d.join("zero.bin"), u64::MAX), None);
        let _ = std::fs::remove_dir_all(d);
    }

    /// Read line by line, the text is the same as read whole and tidied: on text, code, broken
    /// and other encodings, line breaks of every kind, and files past `MAX_TEXT`.
    #[test]
    fn read_line_by_line_the_text_is_the_same_as_read_whole() {
        use crate::extract::{MAX_TEXT, tidy, tidy_code};
        let d = crate::extract::tests::folder("plain-lines");
        let mut x = 7u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let words = ["orbit", "fuel", "smørrebrød", "日本語", "\u{AD}soft", "tab\there", "x\u{7}y", "  ", "\u{FEFF}", "end."];
        let mut cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"one line, no break".to_vec(),
            b"crlf\r\nlines\r\n\r\nand a lone\rcarriage\r".to_vec(),
            b"\n\n\nblank start\n\n\n\nblank end\n\n".to_vec(),
            [&"smørrebrød ".repeat(50).into_bytes()[..], b"caf\xe9 after\nnext line"].concat(),
            b"plain words, caf\xe9 and after\nmore".to_vec(),
            b"latin \xe6\xf8\xe5 all through\n\xe6\xf8\xe5 again".to_vec(),
            [&b"a"[..], &vec![b'b'; MAX_TEXT + 10][..], b" c\nd"].concat(),
            b"cut at the end \xe2\x82".to_vec(),
        ];
        for n in [1_000usize, 200_000, MAX_TEXT / 7 + 3] {
            let mut t = vec![];
            for i in 0..n {
                let w = words[(next() % words.len() as u64) as usize];
                t.extend_from_slice(w.as_bytes());
                t.extend_from_slice(match next() % 9 {
                    0 => b"\n",
                    1 => b"\r\n",
                    2 => b"\n\n",
                    3 => b"    ",
                    _ => b" ",
                });
                if i % 5000 == 4999 {
                    t.push(0x80 + (next() % 64) as u8);
                }
            }
            cases.push(t);
        }
        for (i, bytes) in cases.iter().enumerate() {
            for (ext, code) in [("txt", false), ("rs", true)] {
                let p = d.join(format!("case{i}.{ext}"));
                std::fs::write(&p, bytes).unwrap();
                let whole = super::text(&p, u64::MAX).map(|t| if code { tidy_code(&t) } else { tidy(&t) });
                assert_eq!(super::tidied(&p, code), whole, "case {i} as .{ext}");
            }
        }
        let _ = std::fs::remove_dir_all(d);
    }
}

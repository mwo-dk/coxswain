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
}

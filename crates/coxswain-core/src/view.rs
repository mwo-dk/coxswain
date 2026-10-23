//! What both apps' viewers show of a file: its text decoded (UTF-8, UTF-16 with its byte order
//! mark, else Windows-1252), a hex dump when it is no text, and the theme colour each kind of token
//! of highlighted code takes, so code looks alike in the desktop app's preview and the
//! terminal app's viewer.

/// Bytes of a file as text: `None` when it is no text (a zero byte in its first 8 KB, outside
/// UTF-16). UTF-8 (a byte order mark dropped), UTF-16 by its mark, and UTF-8 with a few stray
/// bytes keep their letters; any other encoding is read as Windows-1252 (Latin-1 with the
/// curly quotes, dashes and € that Windows put at 0x80–0x9F), which every byte is.
/// A character cut off at the end (a file read only in part) costs only itself.
pub fn decode(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]).filter(|_| !bytes.starts_with(&[0xff, 0xfe, 0, 0])) {
        return Some(utf16(rest, u16::from_le_bytes));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return Some(utf16(rest, u16::from_be_bytes));
    }
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    Some(if is_utf8(bytes) { String::from_utf8_lossy(bytes).into_owned() } else { cp1252(bytes) })
}

/// Whether text is UTF-8: all of it, or but for a character cut off at its end, or with a stray
/// byte or two (`nearly_utf8`); else it is in another encoding.
pub fn is_utf8(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) if e.error_len().is_none() => true,
        Err(_) => crate::extract::nearly_utf8(bytes),
    }
}

/// One line of a file as text, for a viewer that reads a big file a line at a time: UTF-8
/// (stray bytes shown as �) when the file is (`utf8`, from `is_utf8` on its start), else
/// Windows-1252.
pub fn decode_line(bytes: &[u8], utf8: bool) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) if utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Err(_) => cp1252(bytes),
    }
}

/// Windows-1252's characters at 0x80–0x9F; the five it leaves out stay as they are.
const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

pub(crate) fn cp1252(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if (0x80..0xa0).contains(&b) { CP1252[(b - 0x80) as usize] } else { b as char }).collect()
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.chunks_exact(2).map(|c| unit([c[0], c[1]]));
    char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// A hex dump, 16 bytes a line with their offset and what of them is printable, of at most
/// `lines` lines: `00000010  48 65 6c 6c 6f …  Hello…`.
pub fn hex(bytes: &[u8], lines: usize) -> Vec<String> {
    bytes
        .chunks(16)
        .take(lines)
        .enumerate()
        .map(|(i, c)| {
            let h: String = c.iter().map(|b| format!("{b:02x} ")).collect();
            let a: String = c.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
            format!("{:08x}  {h:<48} {a}", i * 16)
        })
        .collect()
}

/// The kinds of token highlighted code is coloured by, each with the theme slot whose
/// foreground it takes: highlight.js's class (the desktop app; its preview's style sheet,
/// `gui/src/Preview.svelte`, says the same, and a test holds them together) and the TextMate
/// scopes that mean it (the terminal app's grammars), most particular first.
pub const TOKENS: &[(&str, &[&str], &str)] = &[
    ("comment", &["comment"], "hidden"),
    ("string", &["string", "constant.character"], "git_added"),
    ("attr", &["entity.other.attribute-name"], "git_added"),
    ("number", &["constant.numeric"], "search_hit"),
    ("literal", &["constant.language", "constant"], "search_hit"),
    ("title", &["entity.name.function", "entity.name.section", "entity.name.tag", "support.function"], "directory"),
    ("type", &["entity.name.type", "entity.name.class", "support.type", "support.class", "entity.other.inherited-class", "storage.type.primitive", "storage.type.numeric"], "git_modified"),
    ("keyword", &["keyword.operator.word", "keyword", "storage.modifier", "storage"], "symlink"),
    ("built_in", &["support", "variable.language"], "symlink"),
    ("meta", &["meta.preprocessor", "keyword.control.import", "meta.annotation"], "git_renamed"),
    ("addition", &["markup.inserted"], "git_added"),
    ("deletion", &["markup.deleted"], "git_deleted"),
];

/// Scopes left plain, as highlight.js leaves operators and punctuation.
const PLAIN: &[&str] = &["keyword.operator", "punctuation"];

/// The theme slot a TextMate scope (`keyword.control.rust`) is coloured with: the token whose
/// scope is the longest prefix of it; `None` for plain text.
pub fn slot_of_scope(scope: &str) -> Option<&'static str> {
    TOKENS
        .iter()
        .flat_map(|(_, scopes, slot)| scopes.iter().map(move |s| (*s, Some(*slot))))
        .chain(PLAIN.iter().map(|s| (*s, None)))
        .filter(|(s, _)| scope == *s || scope.starts_with(s) && scope.as_bytes().get(s.len()) == Some(&b'.'))
        .max_by_key(|(s, _)| s.len())
        .and_then(|(_, slot)| slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_read_in_its_encoding_and_binary_is_none() {
        assert_eq!(decode("smørrebrød".as_bytes()).as_deref(), Some("smørrebrød"));
        assert_eq!(decode(b"\xef\xbb\xbfhi").as_deref(), Some("hi"));
        assert_eq!(decode(b"caf\xe9 cr\xe8me").as_deref(), Some("café crème"), "Latin-1");
        assert_eq!(decode(b"\xff\xfeh\0i\0").as_deref(), Some("hi"), "UTF-16 LE");
        assert_eq!(decode(b"\xfe\xff\0h\0i").as_deref(), Some("hi"), "UTF-16 BE");
        assert_eq!(decode(b"MZ\0\0\x90").as_deref(), None);
        // Cut off in the middle of ø: the rest stays.
        assert_eq!(decode(&"brød".as_bytes()[..3]).as_deref(), Some("br\u{fffd}"));
        assert_eq!(decode_line(b"na\xefve", false), "naïve");
        assert_eq!(decode_line(b"na\xefve", true), "na\u{fffd}ve", "a stray byte in UTF-8");
        assert_eq!(decode(b"\x93quoted\x94 \x80 5").as_deref(), Some("“quoted” € 5"), "Windows-1252");
        assert!(!is_utf8(b"caf\xe9 au lait") && is_utf8("café".as_bytes()));
        assert_eq!(hex(b"Hello\n", 1), ["00000000  48 65 6c 6c 6f 0a                                Hello."]);
    }

    #[test]
    fn scopes_take_the_slot_of_their_longest_prefix() {
        assert_eq!(slot_of_scope("keyword.control.rust"), Some("symlink"));
        assert_eq!(slot_of_scope("keyword.control.import.python"), Some("git_renamed"));
        assert_eq!(slot_of_scope("comment.line.double-slash"), Some("hidden"));
        assert_eq!(slot_of_scope("constant.numeric.integer"), Some("search_hit"));
        assert_eq!(slot_of_scope("constant.character.escape"), Some("git_added"));
        assert_eq!(slot_of_scope("source.rust"), None);
        assert_eq!(slot_of_scope("storage.type.function.rust"), Some("symlink"));
        assert_eq!(slot_of_scope("keyword.operator.assignment"), None);
        assert_eq!(slot_of_scope("keyword.operator.word.and"), Some("symlink"));
        assert_eq!(slot_of_scope("stringy"), None);
    }

    /// The desktop app's style sheet colours each highlight.js class with the same slot.
    #[test]
    fn the_desktop_preview_colours_tokens_alike() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gui/src/Preview.svelte");
        // Outside the repository (the crate as published) there is no desktop app to compare.
        let Ok(css) = std::fs::read_to_string(path) else { return };
        for (class, _, slot) in TOKENS {
            let rule = format!(".hljs-{class})");
            let at = css.find(&rule).unwrap_or_else(|| panic!("no rule for .hljs-{class}"));
            let body = &css[at..at + css[at..].find('}').unwrap()];
            let var = format!("var(--{}-fg)", slot.replace('_', "-"));
            assert!(body.contains(&var), ".hljs-{class} should take {var}: {body}");
        }
    }
}

//! Mail: one message (`.eml`) or a mailbox of many (`.mbox`).
//!
//! Of each message: its subject, who it is from and to, its date, its body, and the names of
//! its attachments. The body is every part that is text, in the order of the parts, with HTML
//! made plain; where a message has its body in more than one form, it is the form that a mail
//! program shows. A message forwarded inside another is read the same way, after it.
//!
//! `mail-parser` reads the headers and undoes base64 and the character sets. It is handed the
//! few headers that are read here and no more than `FIELD` of each, since it takes time by the
//! square of the length of a header and memory of many times its size. The parts are taken
//! apart here and not by its `MessageParser::parse`, which follows messages inside messages
//! to any depth: a file of a hundred thousand of them overflows the stack, and that ends the
//! program, where a panic would have been caught. HTML is made plain here as well: its
//! `html_to_text` joins words that a tag parts and loses what follows a comment that ends in
//! three dashes.

use std::borrow::Cow;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use mail_parser::decoders::charsets::map::charset_decoder;
use mail_parser::decoders::html::add_html_token;
use mail_parser::parsers::MessageStream;
use mail_parser::{Addr, Address, MessageParser, MimeHeaders};
use memchr::{memchr, memmem};
use regex::bytes::{Captures, Regex};

use super::MAX_TEXT;

/// The most messages read of one mailbox.
const MAX_MESSAGES: usize = 100_000;
/// How deep parts inside parts and messages inside messages are followed. A message from a
/// mail program is three or four deep, and each time it is forwarded whole adds as many.
const MAX_DEPTH: usize = 16;
/// The headers that are read, in small letters, and the most that is read of one of them:
/// room for some hundreds of addresses, and a quarter of it for a header that says what a
/// part is, which is a line or three. `mail-parser` takes two milliseconds for a content
/// type of this size that is made to be slow, and a file has room for thousands of them.
const WANTED: [&str; 8] = ["subject", "from", "to", "cc", "date", "content-type", "content-transfer-encoding", "content-disposition"];
const FIELD: usize = 16 * 1024;
/// The kinds of text that are data and not words of the message.
const DATA: [&str; 4] = ["text/calendar", "text/csv", "text/vcard", "text/x-vcard"];

pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut bytes = vec![];
    std::fs::File::open(path).ok()?.take(max.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max {
        return None;
    }
    let mailbox = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mbox"));
    // A message of a mailbox has a share of the text and not all of it, so that one large
    // message does not hide every message after it.
    let room = if mailbox { MAX_TEXT / 16 } else { MAX_TEXT };
    let (mut out, mut rest) = (String::new(), &bytes[..]);
    for _ in 0..MAX_MESSAGES {
        if rest.is_empty() || out.len() >= MAX_TEXT {
            break;
        }
        let end = if mailbox { next_message(rest) } else { rest.len() };
        add(&mut out, &message(&rest[..end], 0, room), MAX_TEXT);
        rest = &rest[end..];
    }
    let out = shown(&out);
    (!out.trim().is_empty()).then_some(out)
}

/// Where the next message of a mailbox starts: at a line that begins with "From " and ends
/// as the lines between messages do, in a time and a year. A blank line before it is not
/// asked for, since mailboxes are joined by scripts as well, and a paragraph that begins
/// with "From " starts no message: the patches that git writes have no ">" before one.
fn next_message(bytes: &[u8]) -> usize {
    static START: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?m-u)^From .*\d\d:\d\d.*[ +-]\d{4}\s*$"));
    START.find_at(bytes, bytes.len().min(1)).map_or(bytes.len(), |line| line.start())
}

fn pattern(of: &str) -> Regex {
    Regex::new(of).expect("valid")
}

/// A line more in `to`, or as much of it as there is room for when `to` may hold `most` bytes.
fn add(to: &mut String, text: &str, most: usize) {
    let mut room = most.saturating_sub(to.len()).min(text.len());
    while !text.is_char_boundary(room) {
        room -= 1;
    }
    to.push_str(&text[..room]);
    to.push('\n');
}

/// What one message holds, gathered part by part. Each of the three may hold `room` bytes.
#[derive(Default)]
struct Found {
    /// The parts that are text, in their order.
    body: String,
    attachments: String,
    /// The messages forwarded inside this one, each read as this one is.
    inside: String,
}

/// The text of one message. Bytes without one of the headers that are read are not a message.
fn message(raw: &[u8], depth: usize, room: usize) -> String {
    // Blank lines before the headers are passed over, and the line that parts the messages
    // of a mailbox is not a header.
    let raw = raw.trim_ascii_start();
    let raw = if raw.starts_with(b"From ") { &raw[memchr(b'\n', raw).map_or(raw.len(), |at| at + 1)..] } else { raw };
    let (head, body) = split(raw);
    if head.is_empty() {
        return String::new();
    }
    let (head, mut found) = (headers(&head), Found::default());
    part(&head, body, depth, room, &mut found);
    [head.lines, found.body, found.attachments, found.inside].join("\n")
}

/// One part of a message: parts of its own, a message, a body, or an attachment.
fn part(head: &Head, body: &[u8], depth: usize, room: usize, found: &mut Found) {
    let kind = head.kind.as_str();
    if let (true, Some(boundary)) = (kind.starts_with("multipart/"), &head.boundary) {
        // Each part lies between two lines that begin with "--" and the boundary; the line
        // after the last part ends with "--" as well. A message that was cut off has no such
        // line, and its last part runs to the end.
        let mark = format!("--{boundary}").into_bytes();
        let ends_there = |at: usize| matches!(body.get(at + mark.len()), None | Some(b'\r' | b'\n' | b' ' | b'\t' | b'-'));
        let mut marks = memmem::find_iter(body, &mark).filter(|&at| (at == 0 || body[at - 1] == b'\n') && ends_there(at));
        // The form of the body that is chosen among the parts of an alternative, and how
        // many words it has.
        let (mut next, mut form) = (marks.next(), (String::new(), 0));
        while let Some(at) = next {
            let after = &body[at + mark.len()..];
            if after.starts_with(b"--") || depth >= MAX_DEPTH {
                break;
            }
            let start = at + mark.len() + memchr(b'\n', after).map_or(after.len(), |line_end| line_end + 1);
            next = marks.next();
            let (head, body) = split(body.get(start..next.unwrap_or(body.len())).unwrap_or_default());
            let mut one = Found::default();
            part(&headers(&head), body, depth + 1, room, &mut one);
            // The parts of an alternative are one body in several forms, and a mail program
            // shows the last it can, HTML after plain text. An earlier form is taken when it
            // says more than twice as much: the HTML of a newsletter may be one picture.
            // ponytail: twice is a guess; the forms could be compared word by word.
            let words = one.body.split_whitespace().count();
            if kind != "multipart/alternative" {
                add(&mut found.body, &one.body, room);
            } else if words > 0 && form.1 <= 2 * words {
                form = (one.body, words);
            }
            add(&mut found.attachments, &one.attachments, room);
            add(&mut found.inside, &one.inside, room);
        }
        return add(&mut found.body, &form.0, room);
    }
    if matches!(kind, "message/rfc822" | "message/global") {
        if depth < MAX_DEPTH {
            add(&mut found.inside, &message(&decoded(&head.encoding, body), depth + 1, room), room);
        }
    } else if !head.attached && !DATA.contains(&kind) && (kind.is_empty() || ["text/", "message/", "multipart/"].iter().any(|text| kind.starts_with(text))) {
        // Text of any kind is read, a patch or Markdown as it is. The report of a mail that
        // came back is text too, and so is a body that says it has parts and names no
        // boundary between them.
        let bytes = decoded(&head.encoding, body);
        let text = characters(&bytes[..bytes.len().min(MAX_TEXT)], head.charset.as_deref());
        add(&mut found.body, &if kind == "text/html" { plain(&text) } else { text }, room);
    }
    // A text that was sent along as a file is part of the body to many mail programs, and
    // has a name all the same.
    if let Some(name) = &head.name {
        add(&mut found.attachments, name, room);
    }
}

/// The headers of a message or of a part that are read, the first of each name and no more
/// than `FIELD` of it, and the body. The headers end at a blank line, which may hold spaces
/// or a line end that was translated twice, or at the first line that is no header.
fn split(raw: &[u8]) -> (Vec<u8>, &[u8]) {
    let (mut kept, mut seen, mut room, mut at) = (vec![], [false; WANTED.len()], 0, 0);
    while at < raw.len() {
        let end = memchr(b'\n', &raw[at..]).map_or(raw.len(), |line_end| at + line_end + 1);
        let line = &raw[at..end];
        if line.trim_ascii().is_empty() {
            at = end;
            break;
        }
        // A line that begins with blank space is the rest of the header before it.
        if !line.starts_with(b" ") && !line.starts_with(b"\t") {
            let name = memchr(b':', line).map(|colon| line[..colon].trim_ascii_end());
            let Some(name) = name.filter(|n| !n.is_empty() && n.iter().all(u8::is_ascii_graphic)) else { break };
            let wanted = WANTED.iter().position(|w| w.as_bytes().eq_ignore_ascii_case(name));
            let first = wanted.filter(|w| !std::mem::replace(&mut seen[*w], true));
            room = first.map_or(0, |w| if WANTED[w].starts_with("content-") { FIELD / 4 } else { FIELD });
        }
        let cut = &line[..line.len().min(room)];
        kept.extend_from_slice(cut);
        // A header that was cut, here or where the file ends, would lose its value without
        // a line end.
        if !cut.is_empty() && !cut.ends_with(b"\n") {
            kept.push(b'\n');
        }
        room -= cut.len();
        at = end;
    }
    (kept, &raw[at..])
}

/// What is read of the headers of a message or a part. `mail-parser`'s record of them takes
/// many times their size, so it is dropped before the body is read.
#[derive(Default)]
struct Head {
    /// The subject, who the message is from, to and copied to, and its date, a line each.
    lines: String,
    /// The content type in small letters, as "text/html"; empty when there is none.
    kind: String,
    boundary: Option<String>,
    charset: Option<String>,
    /// The content transfer encoding, in small letters.
    encoding: String,
    attached: bool,
    /// The name of the file that the part is.
    name: Option<String>,
}

/// The headers that `split` kept. Names and subjects are ASCII with encoded words by the
/// rules, but old mail programs wrote them as they were, in the character set of the body.
fn headers(kept: &[u8]) -> Head {
    let kept = [&words(kept)[..], b"\n"].concat();
    let head = parsed(&String::from_utf8_lossy(&kept));
    if std::str::from_utf8(&kept).is_ok() {
        return head;
    }
    let lines = kept.split_inclusive(|b| *b == b'\n').map(|line| characters(line, head.charset.as_deref()));
    parsed(&lines.collect::<String>())
}

fn parsed(head: &str) -> Head {
    let Some(mail) = MessageParser::default().parse_headers(head.as_bytes()) else { return Head::default() };
    let one = |a: &Addr| match (a.name(), a.address()) {
        (Some(name), Some(address)) => format!("{name} <{address}>"),
        (name, address) => name.or(address).unwrap_or_default().to_string(),
    };
    let all = |list: &[Addr]| list.iter().map(one).collect::<Vec<_>>().join(", ");
    // A group of addresses has a name, and it is often all that is said of who a mail went to.
    let who = |address: Option<&Address>| match address {
        Some(Address::List(list)) => all(list),
        Some(Address::Group(groups)) => {
            groups.iter().map(|g| format!("{}: {}", g.name.as_deref().unwrap_or_default(), all(&g.addresses))).collect::<Vec<_>>().join("; ")
        }
        None => String::new(),
    };
    let date = mail.date().filter(|d| d.is_valid()).map(|d| d.to_rfc822()).unwrap_or_default();
    let lines = [mail.subject().unwrap_or_default().to_string(), who(mail.from()), who(mail.to()), who(mail.cc()), date];
    let kind = mail.content_type();
    Head {
        lines: lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"),
        kind: kind.map(|k| format!("{}/{}", k.ctype(), k.subtype().unwrap_or_default()).to_ascii_lowercase()).unwrap_or_default(),
        boundary: kind.and_then(|k| k.attribute("boundary")).map(String::from),
        charset: kind.and_then(|k| k.attribute("charset")).map(String::from),
        encoding: mail.content_transfer_encoding().unwrap_or_default().trim().to_ascii_lowercase(),
        attached: mail.content_disposition().is_some_and(|d| d.is_attachment()),
        name: mail.attachment_name().map(String::from),
    }
}

/// The encoded words of the headers, made ready for `mail-parser`.
///
/// Two words of one kind with blank space between them are one text that was cut in two, and
/// some mail programs cut it in the middle of a character. `mail-parser` reads each word by
/// itself and cannot put that character together again, so the two are made one word here.
///
/// A "=?" that begins no word which ends is parted. `mail-parser` reads on from it to the
/// end of the header to find that out, and from the next one again: a header of nothing but
/// such beginnings takes minutes.
fn words(head: &[u8]) -> Cow<'_, [u8]> {
    static SEAM: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u)\?=\s+(=\?[^?\s]+\?[bqBQ]\?)"));
    static WORD: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u)(=\?[^?\s]+\?[bqBQ]\?[^?]*\?=)|=\?"));
    if memmem::find(head, b"=?").is_none() {
        return Cow::Borrowed(head);
    }
    let joined = SEAM.replace_all(head, |seam: &Captures| {
        // The word that ends here began a little before, 75 bytes at most by the rules. It
        // is of the same kind when it begins as the word that follows does, and it can be
        // joined to that one when it does not end in the "=" that fills base64 up.
        let (before, kind) = (&head[..seam.get(0).map_or(0, |s| s.start())], &seam[1]);
        let near = &before[before.len().saturating_sub(128)..];
        let word = memmem::rfind(near, b"=?").map_or(&[][..], |at| &near[at..]);
        let text = word.get(kind.len()..).filter(|_| word[..kind.len()].eq_ignore_ascii_case(kind));
        let joins = text.is_some_and(|t| !t.ends_with(b"=") && !t.iter().any(|b| b"? \t\r\n".contains(b)));
        if joins { vec![] } else { seam[0].to_vec() }
    });
    WORD.replace_all(&joined, |word: &Captures| word.get(1).map_or(&b"= ?"[..], |w| w.as_bytes()).to_vec()).into_owned().into()
}

/// A body as it was before it was sent as base64 or quoted-printable, and of a body that
/// was sent so no more than can become text. Base64 that breaks the rules is nothing to read.
fn decoded<'a>(encoding: &str, body: &'a [u8]) -> Cow<'a, [u8]> {
    let sent = &body[..body.len().min(2 * MAX_TEXT)];
    if encoding.starts_with("base64") {
        let (end, bytes) = MessageStream::new(sent).decode_base64_mime(b"");
        if end == usize::MAX { Cow::Borrowed(&[]) } else { bytes }
    } else if encoding.starts_with("quoted-printable") {
        quoted_printable(sent).into()
    } else {
        Cow::Borrowed(body)
    }
}

/// Quoted-printable undone: "=" and two digits of sixteen are a byte, and "=" at the end of
/// a line joins the line to the next. Any other "=" is left as it is, as mail programs leave
/// it, and so is the line end after a line of "=====" under a title.
fn quoted_printable(body: &[u8]) -> Vec<u8> {
    let (mut out, mut rest, mut after_one) = (Vec::with_capacity(body.len()), body, false);
    let digit = |b: &u8| (*b as char).to_digit(16);
    while let Some(at) = memchr(b'=', rest) {
        out.extend_from_slice(&rest[..at]);
        (rest, after_one) = (&rest[at + 1..], after_one && at == 0);
        match rest {
            [a, b, ..] if digit(a).is_some() && digit(b).is_some() => out.push((digit(a).unwrap_or(0) * 16 + digit(b).unwrap_or(0)) as u8),
            [b'\n', ..] | [b'\r', b'\n', ..] if !after_one => {}
            _ => {
                out.push(b'=');
                after_one = true;
                continue;
            }
        }
        (rest, after_one) = (&rest[if rest[0] == b'\n' { 1 } else { 2 }..], false);
    }
    out.extend_from_slice(rest);
    out
}

/// Bytes as text, by the character set the part names; `mail-parser` has a decoder for each
/// but UTF-8. When the part names none, or one that is not known, it is Windows-1252: mail
/// from before UTF-8 was written in it or in Latin-1, and often without saying so.
///
/// A line that is UTF-8 is read as UTF-8 whatever the part says. Mail from cron says
/// US-ASCII of what a job wrote in UTF-8, and in a character set of one byte to the
/// character next to no line of text is UTF-8 by chance. Line by line, so that one byte
/// that is wrong costs a line and not the message.
fn characters(bytes: &[u8], charset: Option<&str>) -> String {
    // UTF-7 ends a run of base64 at any character that is none, and "-" is only the one
    // that is dropped; `mail-parser` drops them all, and with them the space between words.
    static RUN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u)(\+[A-Za-z0-9+/]+)([^A-Za-z0-9+/-]|\z)"));
    let said = |names: &[&str]| charset.is_some_and(|c| names.iter().any(|name| c.to_ascii_lowercase().contains(name)));
    let named = charset.and_then(|c| charset_decoder(c.as_bytes()));
    // UTF-16 and UTF-7 are not ASCII with more characters added, and have no lines of bytes.
    if let (Some(decode), true) = (named, said(&["16", "ucs", "unicode", "utf-7", "utf7"])) {
        return decode(&if said(&["7"]) { RUN.replace_all(bytes, &b"$1-$2"[..]) } else { Cow::Borrowed(bytes) });
    }
    let lines = bytes.split_inclusive(|b| *b == b'\n').map(|line| match (std::str::from_utf8(line), named.or_else(|| charset_decoder(b"windows-1252"))) {
        (Ok(text), _) => text.to_string(),
        // Cut in the middle of its last character, it is UTF-8 all the same.
        (Err(e), Some(decode)) if !said(&["utf"]) && e.error_len().is_some() => decode(line),
        _ => String::from_utf8_lossy(line).into_owned(),
    });
    lines.collect()
}

/// HTML as plain text: the tags dropped, with a line break for each that is not part of a
/// line of text, the comments, style sheets, scripts and title left out, and the entities
/// resolved. It forgives what a browser forgives: a comment ends at the first "-->", "<"
/// begins a tag only before a letter, and ">" inside the quotes of an attribute ends none.
fn plain(html: &str) -> String {
    const INLINE: [&str; 32] = [
        "a", "abbr", "acronym", "b", "bdi", "bdo", "big", "cite", "code", "del", "dfn", "em", "font", "i", "ins", "kbd", "mark", "nobr", "q", "s", "samp",
        "small", "span", "strike", "strong", "sub", "sup", "time", "tt", "u", "var", "wbr",
    ];
    const HIDDEN: [&str; 4] = ["script", "style", "template", "title"];
    static MARKUP: LazyLock<Regex> = LazyLock::new(|| {
        // What opens also ends, at the end of the text when nowhere else. That a comment is
        // left open is then not found out by reading to the end, from every comment anew.
        let hidden = HIDDEN.map(|tag| format!(r"<{tag}(?:\s[^>]*)?>.*?(?:</{tag}\s*>|\z)")).join("|");
        let tag = r#"</?[a-z](?:=\s*"[^"]*"|=\s*'[^']*'|[^>])*(?:>|\z)"#;
        pattern(&format!(r"(?is-u)<!--+>|<!--.*?(?:-->|\z)|{hidden}|{tag}|<[!?/][^>]*(?:>|\z)|&#?[a-z0-9]{{1,32}};"))
    });
    let text = MARKUP.replace_all(html.as_bytes(), |found: &Captures| {
        let (found, mut text) = (&found[0], String::new());
        let letters = found[1..].iter().skip_while(|b| **b == b'/').take_while(|b| b.is_ascii_alphanumeric());
        let name: String = letters.map(|b| b.to_ascii_lowercase() as char).collect();
        if found.starts_with(b"&") {
            add_html_token(&mut text, found, false);
        } else if !name.is_empty() && !INLINE.contains(&name.as_str()) && !HIDDEN.contains(&name.as_str()) {
            text.push('\n');
        }
        text
    });
    String::from_utf8_lossy(&text).into_owned()
}

/// The text without what is not seen in it: the colours of a terminal, which mail from cron
/// and from build servers has, the soft hyphen and the space without width, which part a
/// word for the store, and the control characters, among them the zero byte and the two
/// that the store marks its finds with.
fn shown(text: &str) -> String {
    static COLOUR: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new("\x1b\\[[0-9;]*[A-Za-z]").expect("valid"));
    let text = COLOUR.replace_all(text, "");
    let seen = text.chars().filter(|c| !matches!(c, '\u{ad}' | '\u{200b}'));
    seen.map(|c| if c.is_control() && !matches!(c, '\n' | '\t') { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::folder;
    use super::super::{text_of, tidy};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The text of a file with these bytes, tidied as the store gets it. The reader is called
    /// as it is first: `text_of` would catch its panic and hide it. Tests run side by side,
    /// so every file has a folder of its own.
    fn read(name: &str, bytes: &[u8]) -> Option<String> {
        static FOLDERS: AtomicUsize = AtomicUsize::new(0);
        let path = folder(&format!("mail-{}", FOLDERS.fetch_add(1, Ordering::Relaxed))).join(name);
        std::fs::write(&path, bytes).unwrap();
        let text = super::text(&path, 32 << 20).map(|t| tidy(&t)).filter(|t| !t.is_empty());
        if bytes.len() < 10_000 {
            assert_eq!(text_of(&path, bytes.len() as u64, 32 << 20), text, "the store finds this reader by the name of the file");
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        text
    }

    /// The text as the reader gives it, before it is tidied and cut.
    fn untidied(name: &str, bytes: &[u8]) -> String {
        let path = folder(&format!("mail-untidied-{name}")).join(name);
        std::fs::write(&path, bytes).unwrap();
        let text = super::text(&path, 32 << 20).unwrap_or_default();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        text
    }

    fn base64(bytes: &[u8]) -> String {
        const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for (i, three) in bytes.chunks(3).enumerate() {
            let n = three.iter().enumerate().fold(0u32, |n, (at, b)| n | (*b as u32) << (16 - 8 * at));
            for at in 0..4 {
                out.push(if at <= three.len() { LETTERS[(n >> (18 - 6 * at)) as usize & 63] as char } else { '=' });
            }
            if i % 19 == 18 {
                out.push_str("\r\n");
            }
        }
        out
    }

    #[test]
    fn mail_gives_headers_then_body_then_attachment_names() {
        let mail = "Received: from mail.example.com by mx.example.dk; Tue, 15 Sep 2026 10:30:02 +0200\r\n\
            Subject: =?utf-8?B?Rmx5dm5pbmcgc3l2OiBicsOmbmRzdG9m?=\r\n =?utf-8?Q?_og_v=C3=A6gt?=\r\n\
            From: =?iso-8859-1?Q?S=F8ren_=C6r=F8?= <soren@example.dk>\r\n\
            To: =?utf-8?q?Zo=C3=AB_Caf=C3=A9?= <zoe@example.fr>,\r\n =?utf-8?b?5Zui6Zif?= <team@example.cn>, plain@example.com\r\n\
            Cc: =?utf-8?b?15PXldeT?= <david@example.il>\r\n\
            Date: Tue, 15 Sep 2026 10:30:00 +0200\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=\"=_outer\"\r\n\
            \r\n\
            This is a message in MIME format, says the preamble.\r\n\
            --=_outer\r\n\
            Content-Type: text/plain; charset=\"utf-8\"\r\n\
            Content-Transfer-Encoding: 8bit\r\n\
            \r\n\
            Kære alle\r\n\r\nRødgrød med fløde på åen, café crème, שלום and 火箭 发射.\r\n\
            --=_outer\r\n\
            Content-Type: application/pdf\r\n\
            Content-Transfer-Encoding: base64\r\n\
            Content-Disposition: attachment;\r\n filename*=utf-8''r%C3%B8dgr%C3%B8d%20med%20fl%C3%B8de.pdf\r\n\
            \r\n\
            JVBERi0xLjQKJSBhdHRhY2htZW50c2VjcmV0Cg==\r\n\
            --=_outer\r\n\
            Content-Type: text/plain; charset=\"utf-8\"\r\n\
            Content-Disposition: attachment; filename=\"notes.txt\"\r\n\
            \r\n\
            Words inside an attachment.\r\n\
            --=_outer\r\n\
            Content-Type: text/plain; charset=\"us-ascii\"; name=\"valve.log\"\r\n\
            Content-Disposition: inline; filename=\"valve.log\"\r\n\
            \r\n\
            Words of a text shown in the message.\r\n\
            --=_outer\r\n\
            Content-Type: image/png; name=\"=?utf-8?b?54Gr566tLnBuZw==?=\"\r\n\
            Content-Transfer-Encoding: base64\r\n\
            \r\n\
            iVBORw0KGgo=\r\n\
            --=_outer--\r\n\
            And an epilogue.\r\n";
        let text = "Flyvning syv: brændstof og vægt\n\
            Søren Ærø <soren@example.dk>\n\
            Zoë Café <zoe@example.fr>, 团队 <team@example.cn>, plain@example.com\n\
            דוד <david@example.il>\n\
            Tue, 15 Sep 2026 10:30:00 +0200\n\
            Kære alle\n\
            Rødgrød med fløde på åen, café crème, שלום and 火箭 发射.\n\
            Words of a text shown in the message.\n\
            rødgrød med fløde.pdf\n\
            notes.txt\n\
            valve.log\n\
            火箭.png";
        assert_eq!(read("flight.eml", mail.as_bytes()).as_deref(), Some(text));
        assert_eq!(read("unix.EML", mail.replace("\r\n", "\n").as_bytes()).as_deref(), Some(text), "line ends of either kind");
    }

    #[test]
    fn mail_without_mime_is_headers_and_a_body() {
        let mail = "From: ada@example.com\nSubject: Bench test\n\nThe valve opened late.\n\nFrom what we saw, it was the cold.\n";
        assert_eq!(
            read("old.eml", mail.as_bytes()).as_deref(),
            Some("Bench test\nada@example.com\nThe valve opened late.\nFrom what we saw, it was the cold.")
        );
        assert_eq!(read("headers.eml", b"Subject: Only a subject").as_deref(), Some("Only a subject"));
        assert_eq!(
            read("latin.eml", b"Subject: R\xf8dgr\xf8d\nFrom: S\xf8ren <soren@example.dk>\n\nBl\xe5b\xe6r\n").as_deref(),
            Some("Rødgrød\nSøren <soren@example.dk>\nBlåbær"),
            "Latin-1 as it is"
        );
        let saved = format!("From ada@example.com Tue Sep 15 10:30:00 2026\n{mail}");
        assert_eq!(read("saved.eml", saved.as_bytes()), read("old.eml", mail.as_bytes()), "a message saved with the line of a mailbox before it");
    }

    #[test]
    fn mail_body_in_two_forms_is_the_form_that_a_mail_program_shows() {
        let both = "Subject: Both\nContent-Type: multipart/alternative; boundary=alt\n\n\
            --alt\nContent-Type: text/plain\n\nThe plain body.\n\
            --alt\nContent-Type: multipart/related; boundary=rel\n\n\
            --rel\nContent-Type: text/html\n\n<p>The <b>marked up</b> body.</p>\n\
            --rel\nContent-Type: image/png\nContent-Disposition: inline; filename=logo.png\nContent-ID: <logo>\n\niVBORw0KGgo=\n\
            --rel--\n\
            --alt--\n";
        assert_eq!(read("both.eml", both.as_bytes()).as_deref(), Some("Both\nThe marked up body.\nlogo.png"));

        let empty_html = both.replace("The <b>marked up</b> body.", "&nbsp;");
        assert_eq!(read("empty.eml", empty_html.as_bytes()).as_deref(), Some("Both\nThe plain body.\nlogo.png"), "a part without words is no body");

        let forms = |plain: &str, html: &str| {
            let mail = format!(
                "Subject: News\nContent-Type: multipart/alternative; boundary=alt\n\n\
                --alt\nContent-Type: text/plain\n\n{plain}\n--alt\nContent-Type: text/html\n\n{html}\n\
                --alt\nContent-Type: text/calendar\n\nBEGIN:VCALENDAR\n--alt--\n"
            );
            read("forms.eml", mail.as_bytes())
        };
        assert_eq!(
            forms("Your mail program does not show HTML. Open this message in a browser.", "<h1>Engine test passed</h1><p>It ran for ninety seconds.</p>")
                .as_deref(),
            Some("News\nEngine test passed\nIt ran for ninety seconds."),
            "a plain part that only points at the HTML"
        );
        assert_eq!(
            forms("The launch window opens on Tuesday at noon, and the fuel is on its way.", "<img src=\"cid:news\"><a href=\"x\">View in browser</a>")
                .as_deref(),
            Some("News\nThe launch window opens on Tuesday at noon, and the fuel is on its way."),
            "HTML that is a picture of the words"
        );

        let html = "Subject: Newsletter\nContent-Type: TEXT/HTML; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\n\
            <!DOCTYPE html><html><head><title>Hidden</title><style>td { color: red }</style></head><body>=\n\
            <script>var hidden =3D 1 < 2;</script><!-- hidden too -->=\n\
            <div>First block</div><div>Second block</div>=\n\
            <p>R&oslash;dgr&oslash;d p&aring; &aring;en, caf&eacute; &amp; cr=C3=A8me &#1513;&#1500;&#1493;&#1501; &#x706B;&#x7BAD;</p>=\n\
            <table><tr><th>Stage</th><th>Fuel</th></tr><tr><td>One</td><td>1200&nbsp;kg</td></tr></table>=\n\
            <ul><li>Alpha</li><li>Beta</li></ul><h1>Heading</h1>Tail<br>after the break, <b>bold</b>ly joined</body></html>\n";
        let text = "Newsletter\nFirst block\nSecond block\nRødgrød på åen, café & crème שלום 火箭\nStage\nFuel\nOne\n1200 kg\nAlpha\nBeta\nHeading\n\
            Tail\nafter the break, boldly joined";
        assert_eq!(read("news.eml", html.as_bytes()).as_deref(), Some(text));
    }

    #[test]
    fn mail_in_other_character_sets_and_encodings_is_read() {
        let mail = |kind: &str, encoding: &str, body: &[u8]| {
            [format!("Subject: Test\nContent-Type: {kind}\nContent-Transfer-Encoding: {encoding}\n\n").as_bytes(), body].concat()
        };
        let body =
            |kind: &str, encoding: &str, body: &[u8]| read("set.eml", &mail(kind, encoding, body)).map(|t| t.trim_start_matches("Test").trim().to_string());

        assert_eq!(body("text/plain; charset=iso-8859-1", "8bit", b"R\xf8dgr\xf8d p\xe5 \xe5en, caf\xe9").as_deref(), Some("Rødgrød på åen, café"));
        assert_eq!(
            body("text/plain; charset=ISO-8859-1", "Quoted-Printable", b"R=F8dgr=F8d p=E5 =\n=E5en, caf=E9 og 2 + 2 =3D 4, se ?q=rocket").as_deref(),
            Some("Rødgrød på åen, café og 2 + 2 = 4, se ?q=rocket")
        );
        assert_eq!(body("text/plain; charset=windows-1252", "8bit", b"\x93p\xe5 \xe5en\x94 koster 5 \x80").as_deref(), Some("“på åen” koster 5 €"));
        assert_eq!(
            body("text/plain; charset=utf-8", "BASE64", base64("Rødgrød, שלום and 火箭".as_bytes()).as_bytes()).as_deref(),
            Some("Rødgrød, שלום and 火箭")
        );
        assert_eq!(body("text/plain; charset=windows-1255", "8bit", b"\xf9\xec\xe5\xed rocket").as_deref(), Some("שלום rocket"));
        assert_eq!(body("text/plain; charset=iso-8859-8", "8bit", b"\xf9\xec\xe5\xed rocket").as_deref(), Some("שלום rocket"));
        assert_eq!(
            body("text/plain; charset=utf-16", "base64", base64(b"\xff\xfeR\x00\xf8\x00d\x00 \x00k\x70\xad\x7b").as_bytes()).as_deref(),
            Some("Rød 火箭")
        );
        assert_eq!(body("text/plain; charset=koi8-r", "8bit", b"\xf2\xc1\xcb\xc5\xd4\xc1").as_deref(), Some("Ракета"));
        assert_eq!(body("text/html; charset=iso-8859-1", "quoted-printable", b"<p>Bl=E5b=E6r</p><p>gr&oslash;d</p>").as_deref(), Some("Blåbær\ngrød"));

        assert_eq!(body("text/plain", "8bit", "Rødgrød 火箭".as_bytes()).as_deref(), Some("Rødgrød 火箭"), "UTF-8 that is not declared");
        assert_eq!(body("text/plain", "8bit", b"R\xf8dgr\xf8d p\xe5 \xe5en").as_deref(), Some("Rødgrød på åen"), "Latin-1 that is not declared");
        assert_eq!(body("text/plain; charset=made-up", "8bit", b"R\xf8dgr\xf8d").as_deref(), Some("Rødgrød"), "a character set nobody knows");
        assert_eq!(
            body("text/plain; charset=UTF-8", "8bit", b"R\xc3\xb8dgr\xc3\xb8d \xff p\xc3\xa5").as_deref(),
            Some("Rødgrød \u{fffd} på"),
            "UTF-8 with a byte that is wrong stays UTF-8"
        );
        assert_eq!(read("bare.eml", b"Subject: Gammel post\n\nR\xf8dgr\xf8d\n").as_deref(), Some("Gammel post\nRødgrød"));
        assert_eq!(body("text/plain", "base64", b"not base64 at all, but words").as_deref(), Some(""), "base64 that breaks the rules is not read as text");
        let chinese = body("text/plain; charset=gb2312", "8bit", b"\xbb\xf0\xbc\xfd launch").unwrap();
        assert!(chinese.ends_with(" launch") && !chinese.contains("火箭"), "a character set of two bytes to the character is not read, the ASCII in it is");
    }

    #[test]
    fn mail_forwarded_inside_a_message_is_read_after_it() {
        let inner = "Subject: Valve logs\nFrom: Grace Hopper <grace@example.com>\nContent-Type: multipart/mixed; boundary=in\n\n\
            --in\n\nThe valve opened late.\n--in\nContent-Type: text/csv; name=valve.csv\n\n1,2,3\n--in--\n";
        let outer = |encoding: &str, inner: &str| {
            format!(
                "Subject: Fwd: Valve logs\nFrom: ada@example.com\nContent-Type: multipart/mixed; boundary=out\n\n\
                --out\nContent-Type: text/plain\n\nSee what Grace wrote.\n\
                --out\nContent-Type: message/rfc822\nContent-Transfer-Encoding: {encoding}\nContent-Disposition: attachment; filename=forwarded.eml\n\n{inner}\n\
                --out\nContent-Type: application/zip; name=logs.zip\n\nPK\n\
                --out--\n"
            )
        };
        let text = "Fwd: Valve logs\nada@example.com\nSee what Grace wrote.\nforwarded.eml\nlogs.zip\nValve logs\nGrace Hopper <grace@example.com>\nThe valve opened late.\nvalve.csv";
        assert_eq!(read("forward.eml", outer("7bit", inner).as_bytes()).as_deref(), Some(text));
        assert_eq!(read("forward64.eml", outer("base64", &base64(inner.as_bytes())).as_bytes()).as_deref(), Some(text));
    }

    #[test]
    fn mailbox_gives_every_message_in_its_order() {
        let mailbox = "From MAILER-DAEMON Tue Sep 29 16:56:06 2026\n\
            Subject: First in the box\nFrom: ada@example.com\n\n\
            Body of the first.\n\n>From here on it is about fuel.\nFrom time to time a line begins so in the middle of a paragraph.\n\n\
            From grace@example.com Tue Sep 29 16:57:00 2026\n\
            Subject: =?utf-8?b?QW5kZW4gaSBrYXNzZW46IMOmw7jDpQ==?=\nFrom: grace@example.com\nContent-Type: multipart/mixed; boundary=b\n\n\
            --b\nContent-Type: text/plain; charset=utf-8\n\nRødgrød i den anden.\n\
            --b\nContent-Type: application/pdf; name=second.pdf\nContent-Transfer-Encoding: base64\n\nJVBERi0xLjQK\n\
            --b--\n\n\
            From - Tue Sep 29 16:58:00 2026\n\
            Subject: Third\n\nThe third and last, 火箭.\n";
        let text = "First in the box\nada@example.com\nBody of the first.\n>From here on it is about fuel.\nFrom time to time a line begins so in the middle of a paragraph.\n\
            Anden i kassen: æøå\ngrace@example.com\nRødgrød i den anden.\nsecond.pdf\n\
            Third\nThe third and last, 火箭.";
        assert_eq!(read("box.mbox", mailbox.as_bytes()).as_deref(), Some(text));
        assert_eq!(read("box.MBOX", mailbox.replace('\n', "\r\n").as_bytes()).as_deref(), Some(text), "as a mail program on Windows leaves it");
        assert_eq!(read("one.mbox", b"Subject: No line before it\n\nStill a message.\n").as_deref(), Some("No line before it\nStill a message."));

        let many = "From a@example.com Tue Sep 29 16:56:06 2026\nSubject: One of many\n\nWords.\n\n".repeat(super::MAX_MESSAGES + 10);
        assert_eq!(read("many.mbox", many.as_bytes()).unwrap().matches("One of many").count(), super::MAX_MESSAGES, "no more messages than the limit");
    }

    #[test]
    fn mail_that_is_empty_cut_off_or_not_mail_never_panics() {
        assert_eq!(read("empty.eml", b""), None);
        assert_eq!(read("empty.mbox", b""), None);
        assert_eq!(read("blank.eml", b"\n\n\n"), None);
        assert_eq!(read("notes.eml", b"Just some notes, no headers.\n\nAnd a second paragraph: with a colon.\n"), None, "text that is not mail");
        assert_eq!(read("lines.mbox", &b"From \n\n".repeat(200_000)), None, "a mailbox of nothing but its lines");

        let path = folder("mail-large").join("large.eml");
        std::fs::write(&path, b"Subject: Too large\n\nbody\n").unwrap();
        assert_eq!(super::text(&path, 24), None, "larger than allowed");
        assert!(super::text(&path, 25).is_some() && super::text(&path, u64::MAX).is_some());
        assert_eq!(super::text(&path.with_file_name("missing.eml"), 100), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        // Every beginning of a message with parts, encodings and a message inside it.
        let whole = format!(
            "From a@example.com Tue Sep 29 16:56:06 2026\r\nSubject: =?utf-8?B?w6bDuMOl?=\r\nFrom: \"A (b\" <a@example.com>\r\nDate: Tue, 15 Sep 2026 10:30:00 +0200\r\n\
            Content-Type: multipart/mixed;\r\n boundary=\"x\"\r\n\r\n--x\r\nContent-Type: text/html; charset=iso-8859-1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n\
            <p>Bl=E5b=E6r &amp; gr&oslash;d</p>\r\n--x\r\nContent-Type: message/rfc822\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--x\r\n\
            Content-Disposition: attachment; filename*=utf-8''r%C3%B8d.pdf\r\n\r\nJVBERi0=\r\n--x--\r\n",
            base64(b"Subject: Inside\nContent-Type: text/plain; charset=utf-16\n\n\xff\xfeh\x00i\x00")
        );
        assert!(read("whole.eml", whole.as_bytes()).unwrap().ends_with("Blåbær & grød\nrød.pdf\nInside\nhi"));
        for end in 0..whole.len() {
            let cut = read("cut.eml", &whole.as_bytes()[..end]);
            assert_eq!(cut, read("cut.mbox", &whole.as_bytes()[..end]));
            assert!(cut.is_some() || end < whole.find("From: ").unwrap(), "there is text once the subject is whole: {end}");
        }

        // Bytes of no meaning, as they are and behind the headers of a message.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut noise = |len: usize| -> Vec<u8> {
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 32) as u8
            };
            (0..len).map(|_| next()).collect()
        };
        for round in 0..100 {
            let bytes = noise(round * 19);
            let _ = read("noise.eml", &bytes);
            let _ = read("noise.mbox", &[b"From \n", &bytes[..], b"\n\nFrom \n", &bytes[..]].concat());
            for kind in [
                "text/plain; charset=utf-16",
                "text/html; charset=shift_jis",
                "multipart/mixed; boundary=\"\"",
                "message/rfc822",
                "text/plain; charset=iso-2022-jp",
            ] {
                for encoding in ["8bit", "base64", "quoted-printable"] {
                    let _ = read(
                        "noise.eml",
                        &[format!("Subject: s\nContent-Type: {kind}\nContent-Transfer-Encoding: {encoding}\n\n--\n").as_bytes(), &bytes[..]].concat(),
                    );
                }
            }
        }
    }

    #[test]
    fn mail_nested_without_end_is_followed_to_a_depth_and_no_further() {
        let levels = 100_000;
        let messages = format!("Subject: outermost\n{}Subject: innermost\n\nwords\n", "Content-Type: message/rfc822\n\n".repeat(levels));
        assert_eq!(read("messages.eml", messages.as_bytes()).as_deref(), Some("outermost"));

        let hidden = format!("Subject: wrapper\nContent-Type: message/rfc822\nContent-Transfer-Encoding: base64\n\n{}", base64(messages.as_bytes()));
        assert_eq!(read("hidden.eml", hidden.as_bytes()).as_deref(), Some("wrapper\noutermost"));

        let parts: String = (0..levels).map(|i| format!("Content-Type: multipart/mixed; boundary=b{i}\n\n--b{i}\n")).collect();
        assert_eq!(read("parts.eml", format!("Subject: parts\n{parts}Content-Type: text/plain\n\ninnermost\n").as_bytes()).as_deref(), Some("parts"));

        let shallow: String = (0..super::MAX_DEPTH).map(|i| format!("Content-Type: multipart/mixed; boundary=b{i}\n\n--b{i}\n")).collect();
        assert_eq!(
            read("shallow.eml", format!("Subject: parts\n{shallow}Content-Type: text/plain\n\ninnermost\n").as_bytes()).as_deref(),
            Some("parts\ninnermost")
        );
    }

    #[test]
    fn mail_headers_made_to_be_slow_are_read_in_time() {
        let begun = std::time::Instant::now();
        // Encoded words that never end: `mail-parser` reads from each of them to the end of the header.
        let open = "=?ab?q?x ".repeat(44_444);
        let text = read("subject.eml", format!("Subject: {open}\n\nbody\n").as_bytes()).unwrap();
        assert!(text.starts_with("= ?ab?q?x = ?ab?q?x") && text.ends_with("\nbody"), "{}", &text[..40]);
        let _ = read("names.eml", format!("To: {open}\nContent-Type: text/plain; name=\"{open}\"\n\nbody\n").as_bytes());
        let first = "From a@example.com Tue Sep 29 16:56:06 2026\nSubject: one\n\nbody\n\n";
        let second = format!("{first}From b@example.com Tue Sep 29 16:57:06 2026\nSubject: {open}\n\nbody\n");
        assert!(read("second.mbox", second.as_bytes()).unwrap().starts_with("one\nbody\n= ?ab?q?x"));
        assert_eq!(
            read("words.eml", "Subject: Is 2+2=? or =?utf-8?q?fl=C3=B8de?= (=?ab?q?x\n\nbody\n".as_bytes()).as_deref(),
            Some("Is 2+2= ? or fløde (= ?ab?q?x\nbody"),
            "a word that ends is read beside those that do not"
        );

        // Parameters that continue one another: it joins them by copying all that it has joined so far.
        let pieces = "a*1=bbbbbb;".repeat(380_000);
        let names: String = (0..200_000).map(|i| format!("a{i}*1=x;")).collect();
        for parameters in [pieces, names] {
            let part = format!("--b\nContent-Type: text/plain; {parameters}\nContent-Disposition: inline; {parameters}\n\nbody\n");
            let mail = format!("Subject: s\nContent-Type: multipart/mixed; boundary=b\n\n{part}{part}--b--\n");
            assert_eq!(read("parameters.eml", mail.as_bytes()).as_deref(), Some("s\nbody\nbody"));
        }
        // Quadratic reading takes minutes; a debug build in a busy virtual machine takes up to ten seconds.
        let limit = std::time::Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 5 });
        assert!(begun.elapsed() < limit, "took {:?}", begun.elapsed());
    }

    #[test]
    fn mail_gives_no_more_text_than_is_kept_and_of_a_header_no_more_than_mail_has() {
        // Each of these bytes is three in UTF-8.
        let euro = [&b"Subject: s\nContent-Type: text/plain; charset=windows-1252\n\n"[..], &vec![0x80u8; 5 << 20]].concat();
        let text = untidied("euro.eml", &euro);
        assert!(text.len() <= super::MAX_TEXT + 16 && text.contains("€€€"), "{} bytes", text.len());

        // Headers of a megabyte at every level: `mail-parser` takes ninety times their size in memory.
        let levels: String = (0..15).map(|i| format!("Subject: s{i}\nTo: {}\nContent-Type: message/rfc822\n\n", "a@example.com, ".repeat(80_000))).collect();
        let text = untidied("nested.eml", (levels + "Subject: last\n\nbody\n").as_bytes());
        assert!(text.len() < 400_000 && text.contains("s14\na@example.com, a@example.com") && text.trim_end().ends_with("last\nbody"), "{} bytes", text.len());

        let first = format!("To: {}\nContent-Type: multipart/mixed; boundary=b\nSubject: s\n\n--b\n\nbody\n--b--\n", "a@example.com, ".repeat(80_000));
        assert!(read("first.eml", first.as_bytes()).unwrap().ends_with("a@example.com\nbody"), "what follows a header that is cut is read");
    }

    #[test]
    fn mail_body_is_every_part_that_is_text_in_the_order_of_the_parts() {
        let mail = "Subject: s\nContent-Type: multipart/mixed; boundary=b\n\n\
            --b\nContent-Type: text/html\n\n<p>The launch is on Tuesday.</p>\n\
            --b\nContent-Type: text/plain\nContent-Disposition: inline\n\n_______\nrockets mailing list\n\
            --b\nContent-Type: text/plain; name=valve.log\n\n10:30 open\n\
            --b\nContent-Type: text/x-patch; name=fix.patch\nContent-Disposition: inline; filename=fix.patch\n\n+open the valve earlier\n\
            --b\nContent-Type: text/markdown; charset=utf-8\n\n# Notes\n\
            --b\nContent-Type: text\n\nOld words.\n\
            --b\nContent-Type: text/calendar\n\nBEGIN:VCALENDAR\n\
            --b\nContent-Type: message/delivery-status\n\nDiagnostic-Code: smtp; 550 mailbox unavailable\n\
            --b\nContent-Type: text/rfc822-headers\n\nSubject: the mail that came back\n\
            --b--\n";
        let text = "s\nThe launch is on Tuesday.\n_______\nrockets mailing list\n10:30 open\n+open the valve earlier\n# Notes\nOld words.\n\
            Diagnostic-Code: smtp; 550 mailbox unavailable\nSubject: the mail that came back\nvalve.log\nfix.patch";
        assert_eq!(read("parts.eml", mail.as_bytes()).as_deref(), Some(text));
        assert_eq!(
            read("lost.eml", b"Subject: Lost\nContent-Type: multipart/mixed\n\nThe body of a mail that names no boundary.\n").as_deref(),
            Some("Lost\nThe body of a mail that names no boundary.")
        );
    }

    #[test]
    fn mail_html_keeps_words_apart_that_a_tag_parts_and_whole_that_a_tag_is_in() {
        let html = |body: &str| read("page.eml", format!("Subject: s\nContent-Type: text/html; charset=utf-8\n\n{body}").as_bytes());
        assert_eq!(
            html("<p><b>Important</b> notice</p><p><a href=\"x\">Click</a> here</p><div><br></div><div><i>Ada</i> Lovelace</div>").as_deref(),
            Some("s\nImportant notice\nClick here\nAda Lovelace")
        );
        assert_eq!(
            html("<SPAN>Dear</SPAN> Michael,<table><tr><td>One</td><td>Two</td></tr></table>H<sub>2</sub>O in the turbo&shy;pump<new-tag>new</new-tag>tag")
                .as_deref(),
            Some("s\nDear Michael,\nOne\nTwo\nH2O in the turbopump\nnew\ntag")
        );
    }

    #[test]
    fn mail_html_forgives_what_a_browser_forgives() {
        let html = |body: &str| read("page.eml", format!("Subject: s\nContent-Type: text/html\n\n{body}").as_bytes());
        assert_eq!(
            html("<p>Before.</p><!----><p>Between.</p><!------ rule ------><p>After.</p><!--- header ---><p>Last.</p><!-->x<!--->y<!-- > -->z").as_deref(),
            Some("s\nBefore.\nBetween.\nAfter.\nLast.\nxyz"),
            "a comment ends at the first two dashes and \">\""
        );
        assert_eq!(
            html("<html><head><title>Title words</title>\n<body>\n<p>Body words after a head left open.</p>\n</body></html>").as_deref(),
            Some("s\nBody words after a head left open.")
        );
        assert_eq!(html("<p>Pressure was < 5 bar & rising, 3<4.</p>").as_deref(), Some("s\nPressure was < 5 bar & rising, 3<4."));
        assert_eq!(html("<a href=\"x\" title=\"next > page\">Next</a> <a title='a > b'>page</a>").as_deref(), Some("s\nNext page"));
        assert_eq!(html("<a title=x\">One</a> <a title='y>Two</a> three").as_deref(), Some("s\nOne Two three"), "quotes that are left open");
        assert_eq!(html("<p>Words</p><a href=\"http://exam").as_deref(), Some("s\nWords"), "cut off inside a tag");
        assert_eq!(html("<p>Words</p><!-- cut off <p>inside a comment</p>").as_deref(), Some("s\nWords"));
    }

    #[test]
    fn mail_html_leaves_out_styles_scripts_and_titles_whatever_their_tags_hold() {
        let mail = "Subject: s\nContent-Type: text/html\n\n<head profile=\"x\"><title lang=en>Hidden title</title></head>\
            <body><style type=\"text/css\">p { color: red }</style><script type=\"application/ld+json\">{\"reservationNumber\": \"RXJ34P\"}</script>\
            <p>Shipped.</p><STYLE>\n.a > .b { }\n</STYLE >Sent.<titles>Kept.</titles></body>";
        assert_eq!(read("style.eml", mail.as_bytes()).as_deref(), Some("s\nShipped.\nSent.\nKept."));
    }

    #[test]
    fn mail_headers_end_at_a_line_that_is_blank_to_the_eye_or_is_no_header() {
        assert_eq!(
            read("twice.eml", b"Subject: s\r\r\nFrom: a@example.com\r\r\n\r\r\nbody words\r\r\n").as_deref(),
            Some("s\na@example.com\nbody words"),
            "line ends that were translated twice"
        );
        assert_eq!(
            read("space.eml", b"Subject: s\nFrom: a@example.com\n \t\nThe body after a line of blanks.\n").as_deref(),
            Some("s\na@example.com\nThe body after a line of blanks.")
        );
        assert_eq!(
            read("none.eml", b"Subject: s\nFrom: a@example.com\nThe body with no blank line before it.\nNote: still the body.\n").as_deref(),
            Some("s\na@example.com\nThe body with no blank line before it.\nNote: still the body.")
        );
        assert_eq!(read("blank.eml", b"\n\r\nSubject: s\n\nbody\n").as_deref(), Some("s\nbody"), "blank lines before the headers");
        assert_eq!(read("blank.mbox", b"\nFrom a@example.com Tue Sep 29 16:56:06 2026\nSubject: s\n\nbody\n").as_deref(), Some("s\nbody"));
        let parts = "Subject: s\nContent-Type: multipart/mixed; boundary=b\n\n--b\n\nNote: a part without headers begins with a blank line.\n--b--\n";
        assert_eq!(read("parts.eml", parts.as_bytes()).as_deref(), Some("s\nNote: a part without headers begins with a blank line."));
    }

    #[test]
    fn mail_in_quoted_printable_that_breaks_the_rules_is_decoded_where_it_keeps_them() {
        let mail = b"Subject: s\nContent-Type: text/plain; charset=iso-8859-1\nContent-Transfer-Encoding: Quoted-Printable (as it was)\n\n\
            Title\n=====\nR=F8dgr=F8d, a=b, 100=\n% and =\r\nmore =";
        assert_eq!(read("equals.eml", mail).as_deref(), Some("s\nTitle\n=====\nRødgrød, a=b, 100% and more ="));
    }

    #[test]
    fn mail_text_has_nothing_in_it_that_is_not_seen() {
        let mail = "Subject: Zero\nContent-Type: multipart/mixed; boundary=n\n\n\
            --n\nContent-Type: text/html\n\n<p>before&#0;after &#1;not found&#2; bud&shy;get</p>\n\
            --n\n\nbuild \x1b[1;31mfailed\x1b[0m to\0link\n\
            --n\nContent-Disposition: attachment; filename=\"a\0b.txt\"\n\nx\n--n--\n";
        assert_eq!(read("zero.eml", mail.as_bytes()).as_deref(), Some("Zero\nbefore after not found budget\nbuild failed to link\na b.txt"));
    }

    #[test]
    fn mail_names_of_groups_of_addresses_are_kept() {
        let mail = "Subject: Groups\nTo: Launch team: Grace Hopper <grace@example.com>, linus@example.com;, undisclosed-recipients:;\n\
            Cc: Crew: ada@example.com;\n\nbody\n";
        let text = "Groups\nLaunch team: Grace Hopper <grace@example.com>, linus@example.com; undisclosed-recipients:\nCrew: ada@example.com\nbody";
        assert_eq!(read("groups.eml", mail.as_bytes()).as_deref(), Some(text));
    }

    #[test]
    fn mailbox_messages_start_at_a_line_that_ends_in_a_time_and_a_year() {
        let patches = "From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001\nFrom: ada@example.com\nSubject: [PATCH 1/2] Valve\n\n\
            The valve opened late.\n\nFrom now on it opens earlier, says the turbopump.\n---\n valve.txt | 1 +\n\n\
            From 2222222222222222222222222222222222222222 Mon Sep 17 00:00:00 2001\nFrom: ada@example.com\nSubject: [PATCH 2/2] Pump\n\n\
            From 10:30 the pump runs.\n";
        let text = "[PATCH 1/2] Valve\nada@example.com\nThe valve opened late.\nFrom now on it opens earlier, says the turbopump.\n---\nvalve.txt | 1 +\n\
            [PATCH 2/2] Pump\nada@example.com\nFrom 10:30 the pump runs.";
        assert_eq!(read("patches.mbox", patches.as_bytes()).as_deref(), Some(text));

        let joined = "From a@example.com Tue Sep 29 16:56:06 2026\nSubject: One\n\nFirst body.\n\
            From b@example.com Tue Sep 29 16:57:06 +0200 2026\r\nSubject: =?utf-8?b?csO4ZGdyw7hk?=\n\nSecond body.\n\
            From - Tue Sep 29 16:58 2026 +0200\nSubject: Three\n\nThird body.\n";
        assert_eq!(
            read("joined.mbox", joined.as_bytes()).as_deref(),
            Some("One\nFirst body.\nrødgrød\nSecond body.\nThree\nThird body."),
            "no blank lines between the messages"
        );
    }

    #[test]
    fn mailbox_gives_each_message_a_share_of_the_text() {
        let large = format!("From a@example.com Tue Sep 29 16:56:06 2026\nSubject: Large\n\n{}\n", "a picture as letters\n".repeat(400_000));
        let mailbox = format!("{large}From b@example.com Tue Sep 29 16:57:06 2026\nSubject: After it\n\nThe words of the message after the large one.\n");
        let text = read("large.mbox", mailbox.as_bytes()).unwrap();
        assert!(text.starts_with("Large\na picture as letters\n") && text.ends_with("\nAfter it\nThe words of the message after the large one."));
        assert!(text.len() < super::MAX_TEXT / 8, "{} bytes", text.len());
        assert!(read("large.eml", large.as_bytes()).unwrap().len() > super::MAX_TEXT - 100, "a message in a file of its own has all the room");
    }

    #[test]
    fn mail_that_is_utf8_is_read_as_utf8_whatever_it_says() {
        let mail = |kind: &str, body: &[u8]| [format!("Subject: s\nContent-Type: {kind}\nContent-Transfer-Encoding: 8bit\n\n").as_bytes(), body].concat();
        let cron = mail("text/plain; charset=ANSI_X3.4-1968", "Rødgrød på åen\n".as_bytes());
        assert_eq!(read("cron.eml", &cron).as_deref(), Some("s\nRødgrød på åen"), "as mail from cron says");
        assert_eq!(read("latin.eml", &mail("text/plain; charset=iso-8859-1", "Blåbær 火箭\n".as_bytes())).as_deref(), Some("s\nBlåbær 火箭"));
        assert_eq!(
            read("mixed.eml", &mail("text/plain", b"R\xc3\xb8dgr\xc3\xb8d p\xc3\xa5 \xc3\xa5en\n> caf\xe9 cr\xe8me\n")).as_deref(),
            Some("s\nRødgrød på åen\n> café crème"),
            "one line that is not UTF-8 is that line alone"
        );
    }

    #[test]
    fn mail_headers_written_as_they_were_are_read_in_the_character_set_of_the_body() {
        let hebrew = b"Subject: \xf9\xec\xe5\xed\nContent-Type: text/plain; charset=windows-1255\n\n\xf9\xec\xe5\xed\n";
        assert_eq!(read("hebrew.eml", hebrew).as_deref(), Some("שלום\nשלום"));
        let mixed = b"Subject: R\xc3\xb8dgr\xc3\xb8d\nFrom: S\xf8ren <s@example.dk>\n\nbody\n";
        assert_eq!(read("mixed.eml", mixed).as_deref(), Some("Rødgrød\nSøren <s@example.dk>\nbody"), "each header by itself");
    }

    #[test]
    fn mail_in_utf7_keeps_the_character_that_ends_a_run() {
        let mail = "Subject: s\nContent-Type: text/plain; charset=utf-7\n\np+AOU +AOU-en og r+APg-dgr+APg-d+AOU";
        assert_eq!(read("utf7.eml", mail.as_bytes()).as_deref(), Some("s\npå åen og rødgrødå"));
    }

    #[test]
    fn mail_subject_cut_in_the_middle_of_a_character_is_whole() {
        let base64 = "Subject: =?utf-8?B?QnLD?=\n =?utf-8?B?pm5kc3RvZg==?=\n\nbody\n";
        assert_eq!(read("cut.eml", base64.as_bytes()).as_deref(), Some("Brændstof\nbody"));
        let quoted = "Subject: =?UTF-8?Q?Br=C3?= =?utf-8?q?=A6ndstof?= =?iso-8859-1?q?_p=E5_?= =?utf-8?b?w6U=?= =?utf-8?b?ZW4=?=\n\nbody\n";
        assert_eq!(read("cut.eml", quoted.as_bytes()).as_deref(), Some("Brændstof på åen\nbody"), "words of other kinds stay words of their own");
    }
}

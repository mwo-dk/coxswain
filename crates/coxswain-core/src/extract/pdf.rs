//! PDF: the text of the pages, in their order, each page ending a line.
//!
//! The pdf-extract crate works out the characters: it runs the drawing commands of a page,
//! follows the fonts from glyph codes to characters and says where each one lands. Neither it
//! nor lopdf, which it reads the file with, was written for files that are wrong. Left alone
//! they abort on a form that draws itself, never end on a page that is its own parent, unpack
//! whatever a stream says it holds, take hundreds of times the size of a file to hold what
//! they build of it, and panic on a font they do not like. So this module stands on three
//! sides of them:
//! - before: `heavy` weighs the objects of the file and `tables` looks through its tables
//!   before lopdf reads them, `rebuilt` makes a table anew when the file's own cannot be
//!   used, `at_load` is there for every object it loads, `calm` goes through the loaded
//!   document, takes out what they cannot take and measures what is left, and `affordable`
//!   says whether a page can be read within bounds;
//! - around: a thread with a stack of a known size, every page read on its own behind
//!   `catch_unwind`, the clock looked at between pages and between characters;
//! - after: `Words` makes words and lines of the characters and the places they were drawn at.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::{Duration, Instant};

use memchr::memmem;
use pdf_extract::content::{Content, Operation};
use pdf_extract::encryption::decrypt_object;
use pdf_extract::xref::XrefEntry;
use pdf_extract::{
    Dictionary, Document, EncryptionState, LoadOptions, MediaBox, Object, ObjectId, ObjectStream, OutputDev, OutputError, Reader, Stream, Transform,
};

use super::{MAX_ENTRY, MAX_TEXT};

/// The longest one document is read for. The pages read by then are kept.
const TIME: Duration = Duration::from_secs(3);
/// The most pages read of one document, the most annotations of one page, and the most
/// tables of objects one file may have: it gets one more each time it is saved as a change.
const MAX_PAGES: usize = 10_000;
const MAX_ON_TOP: usize = 1000;
const MAX_TABLES: usize = 256;
/// How deep forms may draw forms and pages hang under pages, and how many brackets a font
/// program may open: the parsers behind pdf-extract go one call deeper for each.
const DEPTH: usize = 64;
/// The stack of the thread that reads. The caller's may be a small one, and running out of
/// stack ends the program, not the reading.
const STACK: usize = 16 * 1024 * 1024;
/// The most a character map may unpack to, the most codes one may map, and the most of a
/// Type 1 font program that may be readable.
const MAX_MAP: u64 = 4 * 1024 * 1024;
const MAX_CODES: usize = 1 << 16;
const MAX_PROGRAM: usize = 256 * 1024;
/// The most widths of characters written out for all the fonts of a document. A run of them is
/// a few bytes in the file, however long it says it is.
const MAX_WIDTHS: usize = 1 << 18;
/// What lopdf builds of a file takes far more room than the file: 120 bytes for every number,
/// name or string of an object, five times that for a list, and five times for every operator
/// of a page's content. So what it is given is weighed first, in things of 120 bytes. The
/// objects of a file may weigh this much and a quarter of the file's length, and the content
/// of a page, with every form it draws as often as it draws it, `MAX_CONTENT`.
const MAX_THINGS: u64 = 1 << 18;
const MAX_CONTENT: u64 = 2 << 20;
/// What drawing a form costs before its first thing.
const DRAW: u64 = 8;
/// The encodings of simple fonts that pdf-extract knows by name.
const ENCODINGS: [&[u8]; 3] = [b"WinAnsiEncoding", b"MacRomanEncoding", b"MacExpertEncoding"];
/// The standard encoding where it differs from the one pdf-extract starts from, and the
/// built-in encoding of Symbol, each as the differences of a font are written: a code, then
/// the names of the characters from that code on.
const STANDARD: &str = "39 /quoteright 96 /quoteleft 161 /exclamdown /cent /sterling /fraction /yen /florin /section /currency /quotesingle \
    /quotedblleft /guillemotleft /guilsinglleft /guilsinglright /fi /fl 177 /endash /dagger /daggerdbl /periodcentered 182 /paragraph /bullet \
    /quotesinglbase /quotedblbase /quotedblright /guillemotright /ellipsis /perthousand 191 /questiondown 193 /grave /acute /circumflex /tilde \
    /macron /breve /dotaccent /dieresis 202 /ring /cedilla 205 /hungarumlaut /ogonek /caron /emdash 225 /AE 227 /ordfeminine 232 /Lslash /Oslash \
    /OE /ordmasculine 241 /ae 245 /dotlessi 248 /lslash /oslash /oe /germandbls";
const SYMBOL: &str = "32 /space /exclam /universal /numbersign /existential /percent /ampersand /suchthat /parenleft /parenright /asteriskmath \
    /plus /comma /minus /period /slash /zero /one /two /three /four /five /six /seven /eight /nine /colon /semicolon /less /equal /greater \
    /question /congruent /Alpha /Beta /Chi /Delta /Epsilon /Phi /Gamma /Eta /Iota /theta1 /Kappa /Lambda /Mu /Nu /Omicron /Pi /Theta /Rho /Sigma \
    /Tau /Upsilon /sigma1 /Omega /Xi /Psi /Zeta /bracketleft /therefore /bracketright /perpendicular /underscore /radicalex /alpha /beta /chi \
    /delta /epsilon /phi /gamma /eta /iota /phi1 /kappa /lambda /mu /nu /omicron /pi /theta /rho /sigma /tau /upsilon /omega1 /omega /xi /psi \
    /zeta /braceleft /bar /braceright /similar 160 /Euro /Upsilon1 /minute /lessequal /fraction /infinity /florin /club /diamond /heart /spade \
    /arrowboth /arrowleft /arrowup /arrowright /arrowdown /degree /plusminus /second /greaterequal /multiply /proportional /partialdiff /bullet \
    /divide /notequal /equivalence /approxequal /ellipsis /arrowvertex /arrowhorizex /carriagereturn /aleph /Ifraktur /Rfraktur /weierstrass \
    /circlemultiply /circleplus /emptyset /intersection /union /propersuperset /reflexsuperset /notsubset /propersubset /reflexsubset /element \
    /notelement /angle /gradient /registerserif /copyrightserif /trademarkserif /product /radical /dotmath /logicalnot /logicaland /logicalor \
    /arrowdblboth /arrowdblleft /arrowdblup /arrowdblright /arrowdbldown /lozenge /angleleft /registersans /copyrightsans /trademarksans \
    /summation /parenlefttp /parenleftex /parenleftbt /bracketlefttp /bracketleftex /bracketleftbt /bracelefttp /braceleftmid /braceleftbt \
    /braceex 241 /angleright /integral /integraltp /integralex /integralbt /parenrighttp /parenrightex /parenrightbt /bracketrighttp \
    /bracketrightex /bracketrightbt /bracerighttp /bracerightmid /bracerightbt";

/// The text of a PDF file, or `None`: no text in it (a scan), a password needed to open it,
/// not a PDF, larger than `max`.
pub fn text(path: &Path, max: u64) -> Option<String> {
    let mut bytes = vec![];
    std::fs::File::open(path).ok()?.take(max.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max || memmem::find(&bytes[..bytes.len().min(1024)], b"%PDF-").is_none() {
        return None;
    }
    // A panic of pdf-extract on a page that is wrong is caught below, and costs nothing but the
    // rest of that page. The hook would still print each one, into the log of whoever called.
    QUIET.call_once(|| {
        let loud = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            if std::thread::current().name() != Some(THREAD) {
                loud(panic);
            }
        }));
    });
    // The virtual machines in CI are slow and busy: the tests' files get four times as long there.
    let slow = cfg!(test) && std::env::var_os("COXSWAIN_SLOW_CI").is_some_and(|v| v == "1");
    let until = Instant::now() + if slow { TIME * 4 } else { TIME };
    let reader = std::thread::Builder::new().name(THREAD.into()).stack_size(STACK);
    std::thread::scope(|scope| reader.spawn_scoped(scope, || read(bytes, until)).ok()?.join().ok()?)
}

const THREAD: &str = "pdf";
static QUIET: std::sync::Once = std::sync::Once::new();

fn read(mut bytes: Vec<u8>, until: Instant) -> Option<String> {
    // Whatever stands before the header, a mail gateway's line or a byte order mark, is no
    // part of the file: the places its tables point at are counted from the header.
    if let Some(at) = memmem::find(&bytes[..bytes.len().min(1024)], b"%PDF-") {
        bytes.drain(..at);
    }
    let most = MAX_THINGS + bytes.len() as u64 / 4;
    if heavy(&bytes, most) {
        return None;
    }
    // A file whose tables cannot be read, or lead to no page, is read again with a table
    // made from the objects that are in it.
    let mut doc = loaded(&mut bytes, most, until);
    if doc.as_ref().is_none_or(|doc| doc.page_iter().next().is_none()) {
        bytes = rebuilt(&bytes)?;
        doc = loaded(&mut bytes, most, until);
    }
    let mut doc = doc?;
    drop(bytes);
    let known = calm(&mut doc, until);
    let pages: Vec<ObjectId> = doc.page_iter().take(MAX_PAGES).collect();
    // pdf-extract is asked for one page at a time and goes through the tree of pages to find
    // it, which for page 2000 of 2000 is all of them. It is given a tree with one page in it.
    // What a page inherits is not lost: that goes by the page's own /Parent. The tree, and
    // a page for what lies on top of a page, get numbers no object of the document has.
    let free = doc.objects.keys().next_back().map_or(0, |id| id.0).checked_add(2)?;
    let (tree, above) = ((free - 1, 0), (free, 0));
    doc.catalog_mut().ok()?.set("Pages", tree);

    let mut text = String::new();
    let mut costs = HashMap::new();
    'pages: for page in pages {
        // The page, then what lies on top of it, each as a page of its own.
        for on_top in std::iter::once(None).chain(on_top(&doc, page).into_iter().map(Some)) {
            if Instant::now() > until || text.len() > MAX_TEXT {
                break 'pages;
            }
            let page = on_top.map_or(page, |look| {
                doc.objects.insert(above, look.into());
                above
            });
            if !affordable(&mut doc, &known, &mut costs, page) {
                continue;
            }
            let mut one = Dictionary::new();
            one.set("Type", "Pages");
            one.set("Kids", vec![Object::Reference(page)]);
            doc.objects.insert(tree, one.into());
            let mut words = Words::new(&mut text, until);
            // A page pdf-extract gives up on keeps what was read of it.
            let _ = catch_unwind(AssertUnwindSafe(|| pdf_extract::output_doc_page(&doc, &mut words, 1)));
            words.close_line(true);
        }
    }
    (!text.trim().is_empty()).then_some(text)
}

/// The document loaded by lopdf, with its tables looked through first and its objects held
/// to `most` things, and decrypted when it takes no password.
fn loaded(bytes: &mut [u8], most: u64, until: Instant) -> Option<Document> {
    // lopdf loads an encrypted file another way, where nothing can be looked at before it is
    // unpacked and no clock is looked at. So it is not to see that the file is encrypted: the
    // word is spelled wrong for it, and the file is decrypted here, once it is loaded.
    let locked = tables(bytes)?;
    if let Some(word) = locked {
        bytes[word..word + LOCKED.len()].copy_from_slice(LOCKED);
    }
    LOAD.set(Some(Load { room: MAX_ENTRY, things: most, left: bytes.len() as u64, until, locked: locked.is_some() }));
    let options = LoadOptions { filter: Some(at_load), ..Default::default() };
    let mut doc = catch_unwind(|| Document::load_mem_with_options(bytes, options)).ok()?.ok()?;
    if locked.is_some() {
        unlock(&mut doc, most)?;
    }
    // A trailer made anew names no catalog: the catalog may be in an object stream, where no
    // look at the bytes of the file finds it. Among the objects loaded it is one of a kind.
    if doc.trailer.get(b"Root").is_err() {
        let named = |object: &Object, key: &[u8], name: &[u8]| object.as_dict().ok().and_then(|d| d.get(key).ok()).and_then(|t| t.as_name().ok()) == Some(name);
        if let Some(id) = doc.objects.iter().find(|(_, object)| named(object, b"Type", b"Catalog")).map(|(id, _)| *id) {
            doc.trailer.set("Root", Object::Reference(id));
        }
    }
    Some(doc)
}

/// The file with a table of its objects made anew at its end, as every other reader makes one
/// when the file's own cannot be read: where each `N G obj` stands, the last of each number.
/// What an object stream holds lopdf takes from the stream, listed or not. The trailer is the
/// file's own, with the tables it points to spelled wrong for lopdf, or one of the size alone.
/// `None`: no object to be found.
fn rebuilt(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut starts = std::collections::BTreeMap::new();
    for at in memmem::find_iter(bytes, b"obj") {
        // Back over the generation and the number, each after blank space: how long each is.
        let mut end = at;
        let mut back = |wanted: fn(&u8) -> bool| {
            let start = bytes[..end].iter().rposition(|c| !wanted(c)).map_or(0, |at| at + 1);
            let length = end - start;
            end = start;
            length
        };
        let lengths = [back(u8::is_ascii_whitespace), back(u8::is_ascii_digit), back(u8::is_ascii_whitespace), back(u8::is_ascii_digit)];
        if lengths.contains(&0) || lengths[3] > 10 || !end.checked_sub(1).is_none_or(|before| b" \t\r\n\0\x0c>])".contains(&bytes[before])) {
            continue;
        }
        let Ok(id) = std::str::from_utf8(&bytes[end..end + lengths[3]]).unwrap_or_default().parse::<u32>() else { continue };
        starts.insert(id, end);
    }
    let last = *starts.keys().next_back()?;
    // The file's own trailer, when it has one that can be read.
    let own = memmem::rfind_iter(bytes, b"trailer").find_map(|at| {
        let to = memmem::find(&bytes[at..], b"startxref").map_or(bytes.len(), |end| at + end);
        let mut text = bytes[at + 7..to].to_vec();
        for (word, spelled) in [(LOCKED, &b"/Encrypt"[..]), (b"/Prev", b"/Prex"), (b"/XRefStm", b"/XRefStx")] {
            for found in memmem::find_iter(&bytes[at + 7..to], word) {
                text[found..found + word.len()].copy_from_slice(spelled);
            }
        }
        matches!(object_at(&[b"1 0 obj", &text[..], b" endobj"].concat(), 0)?, Object::Dictionary(_)).then_some(text)
    });
    // lopdf insists on a size. The file's own, when it has one, comes after and counts.
    let own = own.and_then(|text| {
        let at = memmem::find(&text, b"<<")? + 2;
        Some([&text[..at], format!(" /Size {} ", last + 1).as_bytes(), &text[at..]].concat())
    });
    let trailer = own.unwrap_or_else(|| format!("<< /Size {} >>", last + 1).into_bytes());
    let mut file = bytes.to_vec();
    file.push(b'\n');
    let table = file.len();
    file.extend(b"xref\n");
    for (id, at) in starts {
        file.extend(format!("{id} 1\n{at:010} 00000 n \n").bytes());
    }
    file.extend([b"trailer\n", &trailer[..], format!("\nstartxref\n{table}\n%%EOF\n").as_bytes()].concat());
    Some(file)
}

/// What lies on top of a page, the fields of a form, notes, stamps: every annotation that has
/// a look of its own has it as a stream of content, and is given as a page with that content.
fn on_top(doc: &Document, page: ObjectId) -> Vec<Dictionary> {
    let annotations = doc.get_dictionary(page).ok().and_then(|page| held(doc, page, b"Annots")).and_then(|o| o.as_array().ok());
    // The fields of a form may count on the fonts that the form as a whole names.
    let shared = doc.catalog().ok().and_then(|c| dict_at(doc, c, b"AcroForm")).and_then(|form| form.get(b"DR").ok());
    let looks = annotations.into_iter().flatten().take(MAX_ON_TOP).filter_map(|annotation| {
        let annotation = doc.dereference(annotation).ok()?.1.as_dict().ok()?;
        let (id, look) = stream_at(doc, dict_at(doc, annotation, b"AP")?.get(b"N").ok()?)?;
        let mut page = Dictionary::new();
        page.set("Type", "Page");
        page.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        page.set("Resources", look.dict.get(b"Resources").ok().or(shared).cloned().unwrap_or(Dictionary::new().into()));
        page.set("Contents", id);
        Some(page)
    });
    looks.collect()
}

/// What a dictionary has under a key, a reference followed.
fn held<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    doc.dereference(dict.get(key).ok()?).ok().map(|(_, object)| object)
}

fn dict_at<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    held(doc, dict, key)?.as_dict().ok()
}

/// The stream an object is or refers to, and its number.
fn stream_at<'a>(doc: &'a Document, object: &'a Object) -> Option<(ObjectId, &'a Stream)> {
    match doc.dereference(object) {
        Ok((Some(id), Object::Stream(stream))) => Some((id, stream)),
        _ => None,
    }
}

fn is(stream: &Stream, key: &[u8], name: &[u8]) -> bool {
    stream.dict.get(key).and_then(Object::as_name).is_ok_and(|n| n == name)
}

// ---------------------------------------------------------------- before: the file loaded

/// How much lopdf builds of these bytes, in things: every token is one, a dictionary two, a
/// list five, and where the bytes are the `content` of a page every operator, which is a
/// token that starts with a letter, is five. Nothing is looked into, so a string of words
/// weighs as much as words do.
fn things(bytes: &[u8], content: bool) -> u64 {
    let (mut count, mut apart, mut named) = (0, true, false);
    for c in bytes {
        let (blank, delimiter) = (b" \t\n\r\0\x0c".contains(c), b"()<>[]{}/%".contains(c));
        // A token starts at an opening bracket, a slash, or a regular character after a blank
        // or a bracket. The second character of `<<` opens nothing, and neither does a name.
        if b"([{/".contains(c) || (*c == b'<' && !named) || (apart && !blank && !delimiter && !named) {
            count += match c {
                b'[' => 5,
                b'<' => 2,
                c if content && c.is_ascii_alphabetic() => 5,
                _ => 1,
            };
        }
        named = *c == b'/' || *c == b'<';
        apart = blank || delimiter;
    }
    count
}

/// What an object lopdf has built weighs, in things.
fn weighs(object: &Object) -> u64 {
    match object {
        Object::Array(items) => 5 + items.iter().map(weighs).sum::<u64>(),
        Object::Dictionary(dict) | Object::Stream(Stream { dict, .. }) => 2 + dict.iter().map(|(_, item)| weighs(item)).sum::<u64>(),
        _ => 1,
    }
}

/// What the object at the start of `bytes` weighs, in things, as far as `most`: the tokens of
/// the first thing there, and of all within it when it is a list or a dictionary. Strings and
/// comments are stepped over as lopdf steps over them, so that no bracket in one counts, and
/// every 120 bytes stepped over are a thing too: what is stepped over is held as well, and a
/// string that never closes must not be read to the end of the file for every `obj` before it.
fn weight(bytes: &[u8], most: u64) -> u64 {
    let (mut at, mut depth, mut count) = (0, 0usize, 0);
    while at < bytes.len() && count <= most {
        let rest = &bytes[at..];
        let ends = |wanted: fn(&u8) -> bool| rest.iter().position(wanted).unwrap_or(rest.len());
        let (mut counted, mut done) = (true, false);
        let step = match rest[0] {
            c if b" \t\n\r\0\x0c".contains(&c) => {
                counted = false;
                1
            }
            b'%' => {
                counted = false;
                ends(|c| *c == b'\n' || *c == b'\r')
            }
            b'(' => {
                let (mut open, mut n) = (0usize, 0);
                while let Some(c) = rest.get(n) {
                    n += 1;
                    match c {
                        b'\\' => n += 1,
                        b'(' => open += 1,
                        b')' => {
                            open -= 1;
                            if open == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                n
            }
            b'<' if rest.get(1) == Some(&b'<') => {
                depth += 1;
                count += 1;
                2
            }
            b'>' if rest.get(1) == Some(&b'>') => {
                (counted, depth) = (false, depth.saturating_sub(1));
                done = depth == 0;
                2
            }
            b'<' => ends(|c| *c == b'>') + 1,
            b'[' => {
                depth += 1;
                count += 4;
                1
            }
            b']' => {
                (counted, depth) = (false, depth.saturating_sub(1));
                done = depth == 0;
                1
            }
            c if b")>]{}".contains(&c) => {
                counted = false;
                1
            }
            _ => 1 + rest[1..].iter().position(|c| b" \t\n\r\0\x0c()<>[]{}/%".contains(c)).unwrap_or(rest.len() - 1),
        };
        at += step;
        count += counted as u64 + step as u64 / 120;
        if done || (counted && depth == 0) {
            break;
        }
    }
    count
}

/// Whether what lopdf would build of the file's objects, all together or any one of them, is
/// more than `most` things. Every object starts at `obj`, as a trailer does at `trailer`, and
/// wherever in the file that stands the table of objects may point.
fn heavy(bytes: &[u8], most: u64) -> bool {
    let starts = memmem::find_iter(bytes, b"obj").filter(|at| !bytes[..*at].ends_with(b"end")).map(|at| at + 3);
    let trailers = memmem::find_iter(bytes, b"trailer").map(|at| at + 7);
    let mut sum = 0;
    for at in starts.chain(trailers) {
        sum += weight(&bytes[at..], most);
        if sum > most {
            return true;
        }
    }
    false
}

/// `/Encrypt` as lopdf is given it, and an object stream under a name it does not know: one
/// it cannot unpack it throws away, and an encrypted one it cannot unpack.
const LOCKED: &[u8] = b"/Encrypx";
const HELD: &[u8] = b"ObjStmHeld";

/// The tables of the file, where its objects are listed, looked through before lopdf reads
/// them: of a table in a stream lopdf unpacks all the stream holds, makes room for fields as
/// wide as the table says they are, and counts out as many objects as it says there are.
/// `None`: not within bounds. Else where the newest table says `/Encrypt`, if it does.
fn tables(bytes: &[u8]) -> Option<Option<usize>> {
    // The newest is found from the end of the file, the others from one another.
    let tail = bytes.len().saturating_sub(512);
    let end = tail + memmem::rfind(&bytes[tail..], b"%%EOF")?;
    let word = end.checked_sub(25)?;
    let word = word + memmem::rfind(&bytes[word..end], b"startxref")?;
    let newest: usize = std::str::from_utf8(&bytes[word + 9..end]).ok()?.trim().parse().ok()?;

    let (mut starts, mut seen, mut locked) = (vec![newest], HashSet::new(), None);
    while let Some(start) = starts.pop() {
        let Some(table) = bytes.get(start..).filter(|_| seen.insert(start)) else { continue };
        if seen.len() > MAX_TABLES {
            return None;
        }
        // A table written as text has a dictionary after it, one in a stream is an object
        // with one. Both are read as lopdf reads them, by lopdf.
        let (object, named) = if table.starts_with(b"xref") {
            let Some(from) = memmem::find(table, b"trailer").map(|at| at + 7) else { continue };
            let to = memmem::find(&table[from..], b"startxref").map_or(table.len(), |at| from + at);
            (object_at(&[b"1 0 obj", &table[from..to], b" endobj"].concat(), 0)?, start + from..start + to)
        } else {
            (object_at(bytes, start)?, start..start + memmem::find(table, b"stream").unwrap_or(table.len()))
        };
        if let Object::Stream(stream) = &object {
            let rows = unpacked(stream, bytes.len() as u64)?.len();
            let widths: Vec<i64> = stream.dict.get(b"W").and_then(Object::as_array).ok()?.iter().filter_map(|width| width.as_i64().ok()).collect();
            let width = widths.iter().filter(|width| (0..=8).contains(*width)).sum::<i64>() as usize;
            if widths.iter().any(|width| !(0..=8).contains(width)) || width == 0 || rows / width > bytes.len() / 8 + 4096 {
                return None;
            }
        }
        let (Object::Dictionary(dict) | Object::Stream(Stream { dict, .. })) = &object else { return None };
        if start == newest && dict.has(b"Encrypt") {
            let word = named.start + memmem::find(&bytes[named.clone()], b"/Encrypt")?;
            locked = bytes.get(word + 8).is_some_and(|after| b" \t\r\n/<[(".contains(after)).then_some(word);
            locked?;
        }
        starts.extend([&b"Prev"[..], b"XRefStm"].iter().filter_map(|key| usize::try_from(dict.get(key).and_then(Object::as_i64).ok()?).ok()));
    }
    Some(locked)
}

/// The object that starts at a place in a file, as lopdf reads it.
fn object_at(bytes: &[u8], at: usize) -> Option<Object> {
    let mut numbers = bytes.get(at..)?.split(u8::is_ascii_whitespace).filter(|word| !word.is_empty()).map(|word| std::str::from_utf8(word).ok());
    let id = (numbers.next()??.parse().ok()?, numbers.next()??.parse().ok()?);
    let mut reader =
        Reader { buffer: bytes, document: Document::new(), encryption_state: None, raw_objects: Default::default(), password: None, strict: false };
    reader.document.reference_table.insert(id.0, XrefEntry::Normal { offset: at.try_into().ok()?, generation: id.1 });
    reader.get_object(id, &mut HashSet::new()).ok()
}

/// What the loading of a document is held to.
#[derive(Clone, Copy)]
struct Load {
    /// How much its object streams may still unpack to, and how many things they may still
    /// hold. What they hold is kept for as long as the document is, so it is all of them
    /// together that count.
    room: u64,
    things: u64,
    /// How many bytes of streams may still come. A file has no more than its own length of
    /// them, unless it lists the same one over and over.
    left: u64,
    until: Instant,
    locked: bool,
}

thread_local! {
    /// lopdf takes a plain function to call, and a function has nowhere else to keep this.
    static LOAD: Cell<Option<Load>> = const { Cell::new(None) };
}

/// Called by lopdf for every object it loads, before it unpacks an object stream.
fn at_load(id: (u32, u16), object: &mut Object) -> Option<((u32, u16), Object)> {
    let mut load = LOAD.get()?;
    let size = object.as_stream().map_or(0, |stream| stream.content.len() as u64);
    if Instant::now() > load.until || size > load.left {
        // Out of lopdf, back to `read`, and without a word: this is no fault of the program's.
        std::panic::resume_unwind(Box::new(()));
    }
    // What is given back is used for the objects inside an object stream only, and a stream is
    // never one of them. Nor is one so heavy that a copy of it would be a burden.
    let Object::Stream(stream) = object else { return Some((id, if weighs(object) > MAX_THINGS { Object::Null } else { object.clone() })) };
    load.left -= size;
    if is(stream, b"Subtype", b"Image") {
        hush(stream);
    } else if is(stream, b"Type", b"ObjStm") && load.locked {
        stream.dict.set("Type", Object::Name(HELD.to_vec()));
    } else if is(stream, b"Type", b"ObjStm") {
        match unpacked(stream, load.room).map(|bytes| (bytes.len() as u64, things(&bytes, false))) {
            Some((size, weight)) if weight <= load.things => (load.room, load.things) = (load.room - size, load.things - weight),
            _ => hush(stream),
        }
    }
    LOAD.set(Some(load));
    Some((id, Object::Null))
}

/// The document decrypted with no password, as lopdf does it, but for the object streams:
/// those are unpacked here, within bounds, holding no more than `most` things together.
/// `None`: it takes a password.
fn unlock(doc: &mut Document, most: u64) -> Option<()> {
    let named = doc.trailer.remove(&LOCKED[1..])?;
    let lock = named.as_reference().ok()?;
    doc.trailer.set("Encrypt", named);
    doc.authenticate_password("").ok()?;
    let state = EncryptionState::decode(doc, "").ok()?;
    let (mut room, mut things_left, mut inside) = (MAX_ENTRY, most, vec![]);
    for (id, object) in doc.objects.iter_mut().filter(|(id, _)| **id != lock) {
        let _ = decrypt_object(&state, *id, object);
        let Object::Stream(stream) = object else { continue };
        if is(stream, b"Type", HELD) {
            stream.dict.set("Type", "ObjStm");
            match unpacked(stream, room).map(|bytes| (bytes.len() as u64, things(&bytes, false))) {
                Some((size, weight)) if weight <= things_left => (room, things_left) = (room - size, things_left - weight),
                _ => hush(stream),
            }
            // An object is taken from the stream the table says it is in.
            let listed = |number: &u32| !matches!(doc.reference_table.get(*number), Some(XrefEntry::Compressed { container, .. }) if *container != id.0);
            inside.extend(ObjectStream::new(stream).map(|stream| stream.objects).unwrap_or_default().into_iter().filter(|((number, _), _)| listed(number)));
        }
    }
    for (id, object) in inside {
        doc.objects.entry(id).or_insert(object);
    }
    doc.trailer.remove(b"Encrypt");
    doc.objects.remove(&lock);
    Some(())
}

// ---------------------------------------------------------------- before: the document made safe

/// The stream with nothing in it. Not empty: lopdf reads an empty one from the file again.
fn hush(stream: &mut Stream) {
    stream.set_plain_content(b"\n".to_vec());
}

/// What lopdf gives, when it gives anything and does not panic.
fn of_lopdf<T, E>(ask: impl FnOnce() -> Result<T, E>) -> Option<T> {
    catch_unwind(AssertUnwindSafe(ask)).ok()?.ok()
}

/// What lopdf unpacks the stream to, when that is no more than `most` bytes.
fn unpacked(stream: &Stream, most: u64) -> Option<Cow<'_, [u8]>> {
    let Ok(filters) = stream.filters() else { return Some(Cow::Borrowed(&stream.content)) };
    let mut bytes = Cow::Borrowed(&stream.content[..]);
    for filter in filters {
        bytes = Cow::Owned(match filter {
            b"FlateDecode" => inflated(&bytes, most)?,
            // These two lopdf may unpack itself: they give four bytes for one at the most, and
            // 4096, the longest entry of the table, for a code of nine bits.
            b"ASCII85Decode" | b"LZWDecode" => {
                if bytes.len() as u64 * if filter == b"LZWDecode" { 4096 } else { 4 } > most {
                    return None;
                }
                let mut one = Stream::new(Dictionary::new(), bytes.into_owned());
                one.dict.set("Filter", Object::Name(filter.to_vec()));
                match of_lopdf(|| one.decompressed_content()) {
                    Some(bytes) => bytes,
                    None => return Some(Cow::Borrowed(&stream.content)),
                }
            }
            // A filter lopdf does not know. It gives up, and the stream is taken as it is.
            _ => return Some(Cow::Borrowed(&stream.content)),
        });
    }
    // A predictor changes the bytes, not how many there are: now lopdf may do all of it.
    if stream.dict.has(b"DecodeParms") {
        return Some(of_lopdf(|| stream.decompressed_content()).map_or(Cow::Borrowed(&stream.content[..]), Cow::Owned));
    }
    Some(bytes)
}

/// As lopdf inflates: zlib, and when that gives nothing, deflate from the third byte on.
fn inflated(packed: &[u8], most: u64) -> Option<Vec<u8>> {
    let mut out = vec![];
    let zlib = flate2::read::ZlibDecoder::new(packed).take(most + 1).read_to_end(&mut out);
    if zlib.is_err() && out.is_empty() && packed.len() > 2 {
        let _ = flate2::read::DeflateDecoder::new(&packed[2..]).take(most + 1).read_to_end(&mut out);
    }
    (out.len() as u64 <= most).then_some(out)
}

/// What is known of a stream once the document is calm.
struct Known {
    /// What lopdf builds of its content, in things, and the content with it.
    weight: u64,
    /// How many times its content says `Do`, which draws a form: another stream of content.
    draws: u64,
    /// The names in its content and how many times each is there, when it draws. A form drawn
    /// is named each time. `None`: too many names to keep, or names written with escapes.
    names: Option<HashMap<Vec<u8>, u64>>,
}

impl Known {
    fn of(content: &[u8]) -> Known {
        // As lopdf reads an operator: letters, and nothing of the kind before or after.
        let part = |at: Option<&u8>| at.is_some_and(|c| c.is_ascii_alphabetic() || b"*'\"".contains(c));
        let draws = memmem::find_iter(content, b"Do").filter(|at| !part(at.checked_sub(1).and_then(|i| content.get(i))) && !part(content.get(at + 2))).count();
        let mut names = (draws > 0 && !content.contains(&b'#')).then(HashMap::new);
        let mut rest = content;
        while let (Some(found), Some(at)) = (&mut names, memchr::memchr(b'/', rest)) {
            rest = &rest[at + 1..];
            let end = rest.iter().position(|c| c.is_ascii_whitespace() || b"\0()<>[]{}/%".contains(c)).unwrap_or(rest.len());
            *found.entry(rest[..end].to_vec()).or_default() += 1;
            if found.len() > 1024 {
                names = None;
            }
        }
        Known { weight: things(content, true).saturating_add(content.len() as u64 / 120), draws: draws as u64, names }
    }
}

/// The document made safe to hand to pdf-extract, and what is known of its streams after.
fn calm(doc: &mut Document, until: Instant) -> HashMap<ObjectId, Known> {
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    // Each object is taken out to be mended, so that the others can be looked at meanwhile.
    let mut mending = Mending { plain: HashMap::new(), widths: MAX_WIDTHS, until };
    for id in &ids {
        if let Some(mut object) = doc.objects.insert(*id, Object::Null) {
            mend(&mut object, doc, &mut mending, 2 * DEPTH);
            doc.objects.insert(*id, object);
        }
    }
    let mut plain = mending.plain;
    // Content written anew is kept unpacked, all of it together within bounds.
    let mut room = MAX_ENTRY;
    let drawn: HashSet<ObjectId> = doc.page_iter().take(MAX_PAGES).flat_map(|page| doc.get_page_contents(page)).collect();
    let mut known = HashMap::new();
    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get_mut(&id) else { continue };
        match plain.remove(&id) {
            Some((_, content)) if !content.is_empty() => stream.set_plain_content(content),
            _ if is(stream, b"Subtype", b"Image") => hush(stream),
            _ => {}
        }
        let content = if Instant::now() < until { unpacked(stream, MAX_ENTRY) } else { None };
        let Some(content) = content else {
            hush(stream);
            continue;
        };
        let as_is = Known::of(&content);
        let shown = as_is.weight <= MAX_CONTENT && (drawn.contains(&id) || is(stream, b"Subtype", b"Form"));
        match shown.then(|| rewritten(&content)).flatten().filter(|written| written.len() as u64 <= room) {
            Some(written) => {
                room -= written.len() as u64;
                known.insert(id, Known::of(&written));
                stream.set_plain_content(written);
            }
            None => drop(known.insert(id, as_is)),
        }
    }
    known
}

/// Content written anew for pdf-extract, when it has either of two things it passes over:
/// strings shown with `'` and `"`, written with the operators they are short for; and the
/// text a marked span says its glyphs stand for, `/Span << /ActualText (...) >> BDC ... EMC`,
/// as LibreOffice says an Arabic letter drawn as a base and a dot, and Word says a ligature.
/// pdf-extract hands back nothing of a span but its glyphs, and nothing else but paths: so
/// the text is written as a path, with `SPAN` as its start and the UTF-16 units of the text as
/// the points it goes to, filled before the glyphs of the span, and a path to `SPAN` and one up
/// filled after them. `Words` reads it back. `None` when the content has neither.
fn rewritten(content: &[u8]) -> Option<Vec<u8>> {
    // Either quote comes after the string it shows and ends there.
    let shows = |at: usize| at > 0 && b")> \t\r\n".contains(&content[at - 1]) && content.get(at + 1).is_none_or(u8::is_ascii_whitespace);
    if memchr::memchr2_iter(b'\'', b'"', content).all(|at| !shows(at)) && memmem::find(content, b"ActualText").is_none() {
        return None;
    }
    let op = |operator: &str, operands: Vec<Object>| Operation::new(operator, operands);
    let path = |points: Vec<f64>| {
        let goes = points.into_iter().map(|point| op("l", vec![point.into(), 0.0.into()]));
        [op("m", vec![SPAN.into(), 0.0.into()])].into_iter().chain(goes).chain([op("f", vec![])])
    };
    let mut written = vec![];
    // How many marked spans are open, and at what depth the one with the text was opened.
    let (mut open, mut spanned) = (0usize, None);
    for Operation { operator, mut operands } in of_lopdf(|| Content::decode(content))?.operations {
        match (operator.as_str(), operands.len()) {
            ("'", 1) => written.extend([op("T*", vec![]), op("Tj", operands)]),
            ("\"", 3) => {
                let string = operands.split_off(2);
                let spacing = operands.split_off(1);
                written.extend([op("Tc", spacing), op("Tw", operands), op("T*", vec![]), op("Tj", string)]);
            }
            ("BDC" | "BMC", _) => {
                open += 1;
                let said = operands.get(1).and_then(|o| o.as_dict().ok()).and_then(|span| span.get(b"ActualText").ok());
                if let (Some(Object::String(said, _)), None) = (said, spanned) {
                    spanned = Some(open);
                    written.extend(path(units(said).into_iter().map(f64::from).collect()));
                }
                written.push(op(&operator, operands));
            }
            ("EMC", _) => {
                if spanned == Some(open) {
                    spanned = None;
                    written.extend(path(vec![1.0]));
                }
                open = open.saturating_sub(1);
                written.push(op(&operator, operands));
            }
            // A picture within the content. lopdf reads it as a stream, and would write it as one.
            ("BI", _) => {}
            _ => written.push(op(&operator, operands)),
        }
    }
    of_lopdf(|| Content { operations: written }.encode())
}

/// A point no drawing goes to: where the path that carries the text of a span starts.
const SPAN: f64 = -65537.5;

/// The UTF-16 units of a string of a PDF: with a byte order mark it is UTF-16, and without one
/// each byte is a character, near enough.
fn units(string: &[u8]) -> Vec<u16> {
    match string.strip_prefix(b"\xfe\xff") {
        Some(utf16) => utf16.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect(),
        None => string.iter().map(|byte| *byte as u16).collect(),
    }
}

/// What a stream is put in the place of, by what a font names it as.
#[derive(Clone, Copy, PartialEq)]
enum Use {
    /// A map from codes to characters: written anew, flat, its ranges counted.
    Map,
    /// A map from bytes to codes, of a font of two-byte codes: taken to be the plain one.
    Codes,
    /// A Type 1 font program: what is readable of it, the part before `eexec`.
    Program,
    /// A font program or a colour profile that pdf-extract unpacks and does not look at.
    Nothing,
}

/// What the mending of a document has found, and what it is held to.
struct Mending {
    /// The streams to put something in the place of, with what, and as what they were first named.
    plain: HashMap<ObjectId, (Use, Vec<u8>)>,
    /// How many widths of characters may still be written out.
    widths: usize,
    until: Instant,
}

fn mend(object: &mut Object, doc: &Document, mending: &mut Mending, depth: usize) {
    if depth == 0 {
        return;
    }
    match object {
        Object::Array(items) => {
            if let [Object::Name(kind), profile, ..] = &items[..]
                && kind == b"ICCBased"
                && let Some((id, _)) = stream_at(doc, profile)
            {
                mending.plain.entry(id).or_insert((Use::Nothing, b"\n".to_vec()));
            }
            items.iter_mut().for_each(|item| mend(item, doc, mending, depth - 1));
        }
        Object::Dictionary(dict) | Object::Stream(Stream { dict, .. }) => {
            mend_font(dict, doc, mending);
            dict.iter_mut().for_each(|(_, item)| mend(item, doc, mending, depth - 1));
        }
        _ => {}
    }
}

/// A font, its descriptor or its encoding, changed to what pdf-extract reads without giving up.
/// Any other dictionary has none of these keys and is left as it is.
fn mend_font(dict: &mut Dictionary, doc: &Document, mending: &mut Mending) {
    let Mending { plain, widths, until } = mending;
    let until = *until;
    let name = |dict: &Dictionary, key: &[u8]| held(doc, dict, key).and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec);
    let kind = name(dict, b"Subtype").unwrap_or_default();

    // The streams it names.
    for (key, used) in [(&b"ToUnicode"[..], Use::Map), (b"FontFile", Use::Program), (b"FontFile2", Use::Nothing), (b"FontFile3", Use::Nothing)] {
        let Ok(named) = dict.get(key) else { continue };
        let keep = match stream_at(doc, named) {
            // The one kind of font program in FontFile3 that pdf-extract reads, with a parser that holds.
            Some((_, stream)) if key == b"FontFile3" && is(stream, b"Subtype", b"Type1C") => true,
            Some((id, stream)) => {
                let (as_what, content) = plain.entry(id).or_insert_with(|| (used, if Instant::now() < until { in_place_of(stream, used) } else { vec![] }));
                *as_what == used && !content.is_empty()
            }
            None => key != b"ToUnicode" || matches!(named, Object::Name(n) if n == b"Identity-H"),
        };
        if !keep {
            dict.remove(key);
        }
    }

    // The encoding: by a name pdf-extract does not know, in a stream, or missing.
    if name(dict, b"BaseEncoding").is_some_and(|base| !ENCODINGS.contains(&&base[..])) {
        dict.set("BaseEncoding", "WinAnsiEncoding");
    }
    let encoding = name(dict, b"Encoding");
    if kind == b"Type0" {
        match dict.get(b"Encoding").ok().and_then(|o| stream_at(doc, o)) {
            Some((id, _)) if plain.entry(id).or_insert((Use::Codes, CODES.to_vec())).0 == Use::Codes => {}
            _ if encoding.is_some_and(|e| e == b"Identity-H" || e == b"Identity-V") => {}
            _ => dict.set("Encoding", "Identity-H"),
        }
    } else if [&b"Type1"[..], b"MMType1", b"TrueType", b"Type3"].contains(&&kind[..]) {
        // Without an encoding pdf-extract gives up at the first character its map lacks. It
        // has one when the font is TrueType, or has a Type 1 program to take it from.
        let descriptor = dict_at(doc, dict, b"FontDescriptor");
        let flags = descriptor.and_then(|d| d.get(b"Flags").and_then(Object::as_i64).ok());
        let mapped = dict.has(b"ToUnicode") || descriptor.is_some_and(|d| d.has(b"FontFile3"));
        let lacks = !dict.has(b"Encoding") && (kind == b"Type3" || (mapped && kind != b"TrueType" && !descriptor.is_some_and(|d| d.has(b"FontFile"))));
        if lacks || encoding.is_some_and(|e| !ENCODINGS.contains(&&e[..])) {
            dict.set("Encoding", "WinAnsiEncoding");
        }
        // Differences from no base are differences from the font's built-in encoding, which
        // pdf-extract takes to be its own: "fi" at \256 is read as ®, and every width by it.
        // The built-in encoding of a font that is not symbolic is the standard one, and
        // Symbol has its own. The rest have theirs in their program, and are left alone.
        let own = dict_at(doc, dict, b"Encoding")
            .filter(|e| kind != b"Type3" && !e.has(b"BaseEncoding"))
            .map(|e| held(doc, e, b"Differences").and_then(|o| o.as_array().ok()).cloned().unwrap_or_default());
        if let Some(own) = own {
            let base = name(dict, b"BaseFont").unwrap_or_default();
            let symbolic = flags.map_or(base.ends_with(b"Symbol") || base.ends_with(b"Dingbats"), |flags| flags & 32 == 0);
            let built_in = match (symbolic, base.ends_with(b"Symbol")) {
                (false, _) => Some(STANDARD),
                (true, true) => Some(SYMBOL),
                _ => None,
            };
            if let Some(built_in) = built_in {
                let mut all: Vec<Object> = built_in
                    .split(' ')
                    .map(|word| word.strip_prefix('/').map_or_else(|| Object::Integer(word.parse().unwrap_or(0)), |name| Object::Name(name.into())))
                    .collect();
                all.extend(own);
                let mut encoding = Dictionary::new();
                encoding.set("Differences", all);
                dict.set("Encoding", encoding);
            }
        }
        // The widths and the last character have to agree.
        let listed = held(doc, dict, b"Widths").and_then(|o| o.as_array().ok()).map(Vec::len);
        if let (Ok(first), Some(listed)) = (dict.get(b"FirstChar").and_then(Object::as_i64), listed) {
            dict.set("LastChar", first.saturating_add(listed as i64 - 1));
        }
    } else if kind == b"CIDFontType0" || kind == b"CIDFontType2" {
        let written = held(doc, dict, b"W").and_then(|o| o.as_array().ok()).map(|given| widths_of(given, widths));
        if let Some(written) = written {
            dict.set("W", written);
        }
    }
}

/// The widths of a font of two-byte codes written anew, every run of them as a list. A run given
/// as its first code, its last and the one width of them all is read by pdf-extract as nothing.
/// The digits of everything a browser prints are such a run: with the default width in their
/// place, the words after a number land where they are not, and are cut in two.
fn widths_of(given: &[Object], room: &mut usize) -> Vec<Object> {
    let width = |object: &Object| object.as_float().ok().filter(|width| width.is_finite()).unwrap_or(0.0);
    let mut written = vec![];
    let mut at = 0;
    while let (Some(first), Some(next)) = (given.get(at).and_then(|first| first.as_i64().ok()), given.get(at + 1)) {
        let (run, step): (Vec<Object>, usize) = match (next, given.get(at + 2)) {
            (Object::Array(run), _) => (run.iter().take(*room).map(|each| width(each).into()).collect(), 2),
            (last, Some(all)) => {
                let long = last.as_i64().map_or(0, |last| last.saturating_sub(first).saturating_add(1).clamp(0, MAX_CODES as i64));
                (vec![width(all).into(); (long as usize).min(*room)], 3)
            }
            _ => break,
        };
        *room -= run.len();
        if !run.is_empty() && (0..MAX_CODES as i64).contains(&first) {
            written.extend([first.into(), run.into()]);
        }
        at += step;
    }
    written
}

/// Two-byte codes that stand for themselves.
const CODES: &[u8] = b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 begincidrange\n<0000> <FFFF> 0\nendcidrange\n";

/// What is put in the place of a stream a font names. Empty: it cannot be made safe.
fn in_place_of(stream: &Stream, used: Use) -> Vec<u8> {
    match used {
        Use::Map => unpacked(stream, MAX_MAP).map(|bytes| map_of(&bytes)).unwrap_or_default(),
        Use::Program => unpacked(stream, MAX_ENTRY).map(|bytes| program_of(&bytes)).unwrap_or_default(),
        Use::Codes => CODES.to_vec(),
        Use::Nothing => b"\n".to_vec(),
    }
}

/// The readable part of a Type 1 font program, where its encoding is. The parser it goes to
/// calls itself for every bracket opened: with more of them than a font has, nothing is given.
fn program_of(bytes: &[u8]) -> Vec<u8> {
    let Some(end) = memmem::find(&bytes[..bytes.len().min(MAX_PROGRAM)], b"eexec") else { return vec![] };
    let opened = bytes[..end].iter().filter(|c| b"([{<".contains(c)).count();
    if opened > DEPTH { vec![] } else { bytes[..end].to_vec() }
}

/// A map from codes to characters written anew: single codes and ranges of them, no more than
/// `MAX_CODES` in all, nothing nested, nothing but well-formed UTF-16. The parser pdf-extract
/// has for these calls itself for every bracket and counts out a range of any length.
fn map_of(bytes: &[u8]) -> Vec<u8> {
    enum Token<'a> {
        Hex(Vec<u8>),
        Open,
        Word(&'a [u8]),
    }
    let mut tokens = vec![];
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        let ends = |wanted: fn(&u8) -> bool| rest.iter().position(wanted).unwrap_or(rest.len());
        at += match rest[0] {
            b'<' if rest.get(1) == Some(&b'<') => 2,
            b'<' => {
                let end = ends(|c| *c == b'>');
                let mut digits: Vec<u8> = rest[1..end].iter().filter_map(|c| (*c as char).to_digit(16)).map(|d| d as u8).collect();
                if digits.len() % 2 == 1 {
                    digits.push(0);
                }
                tokens.push(Token::Hex(digits.chunks(2).map(|pair| pair[0] << 4 | pair[1]).collect()));
                end + 1
            }
            b'[' => {
                tokens.push(Token::Open);
                1
            }
            b'%' => ends(|c| *c == b'\n' || *c == b'\r'),
            // A string, as the name of the map is. None holds a code.
            b'(' => ends(|c| *c == b')') + 1,
            c if c.is_ascii_alphabetic() => {
                let end = ends(|c| !c.is_ascii_alphanumeric());
                tokens.push(Token::Word(&rest[..end]));
                end
            }
            _ => 1,
        };
    }

    // A code is one to four bytes, what it stands for is UTF-16.
    let code = |hex: &[u8]| (1..=4).contains(&hex.len()).then(|| hex.iter().fold(0u32, |code, byte| code << 8 | *byte as u32));
    let units = |hex: &[u8]| hex.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect::<Vec<u16>>();
    let (mut singles, mut spans) = (vec![], vec![]);
    let mut put = |code: u32, to: &[u16]| {
        if !to.is_empty() && String::from_utf16(to).is_ok() && singles.len() < MAX_CODES {
            singles.push(format!("<{code:08X}> <{}>\n", to.iter().map(|unit| format!("{unit:04X}")).collect::<String>()));
        }
    };
    let (mut single, mut ranges) = (false, false);
    let mut tokens = tokens.iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::Word(word) => (single, ranges) = (*word == b"beginbfchar", *word == b"beginbfrange"),
            Token::Hex(from) if single => {
                if let (Some(code), Some(Token::Hex(to))) = (code(from), tokens.next_if(|next| matches!(next, Token::Hex(_)))) {
                    put(code, &units(to));
                }
            }
            Token::Hex(from) if ranges => {
                let Some(Token::Hex(to)) = tokens.next_if(|next| matches!(next, Token::Hex(_))) else { continue };
                let (Some(from), Some(to)) = (code(from), code(to)) else { continue };
                let to = to.min(from.saturating_add(u16::MAX as u32));
                match tokens.next() {
                    // The last unit goes up by one with each code. A range of single units that
                    // stays within them is one pdf-extract counts out well, and may stay a range.
                    Some(Token::Hex(first)) => match units(first)[..] {
                        [first] if to >= from && first as u32 + (to - from) <= u16::MAX as u32 => spans.push((from, to, first)),
                        ref first => {
                            for code in from..=to {
                                let mut units = first.to_vec();
                                if let Some(last) = units.last_mut() {
                                    *last = last.wrapping_add((code - from) as u16);
                                }
                                put(code, &units);
                            }
                        }
                    },
                    Some(Token::Open) => {
                        for code in from..=to {
                            let Some(Token::Hex(to)) = tokens.next_if(|next| matches!(next, Token::Hex(_))) else { break };
                            put(code, &units(to));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let mut room = MAX_CODES.saturating_sub(singles.len());
    spans.retain(|(from, to, _)| {
        room = room.saturating_sub((to - from) as usize + 1);
        room > 0
    });
    let spans: Vec<String> = spans.iter().map(|(from, to, first)| format!("<{from:08X}> <{to:08X}> <{first:04X}>\n")).collect();
    format!("{} beginbfchar\n{}endbfchar\n{} beginbfrange\n{}endbfrange\n", singles.len(), singles.concat(), spans.len(), spans.concat()).into_bytes()
}

// ---------------------------------------------------------------- before: a page within bounds

/// What drawing has been found to cost, by the stream and the resources it draws with.
type Costs = HashMap<(ObjectId, usize), u64>;

/// Whether pdf-extract reads the page within bounds: what it inherits is found, and the
/// content it goes through, every form counted as many times as it is drawn, weighs no more
/// than `MAX_CONTENT` things. A page too dear for the forms it draws is read without them.
fn affordable(doc: &mut Document, known: &HashMap<ObjectId, Known>, costs: &mut Costs, page: ObjectId) -> bool {
    let Ok(mut dict) = doc.get_dictionary(page) else { return false };
    // pdf-extract goes up by /Parent, calling itself, for the resources and the size of the page.
    let mut resources = None;
    for level in 0.. {
        resources = resources.or(dict_at(doc, dict, b"Resources"));
        let Ok(parent) = dict.get(b"Parent").and_then(Object::as_reference).and_then(|id| doc.get_dictionary(id)) else { break };
        if level == DEPTH {
            return false;
        }
        dict = parent;
    }
    let contents: Vec<ObjectId> =
        doc.get_page_contents(page).iter().map(|id| doc.objects.get(id).and_then(|o| stream_at(doc, o)).map_or(*id, |(id, _)| id)).collect();
    let sum = |cost: &mut dyn FnMut(ObjectId) -> u64| contents.iter().fold(0u64, |sum, id| sum.saturating_add(cost(*id)));
    if sum(&mut |id| drawing(doc, known, costs, id, resources, DEPTH)) <= MAX_CONTENT {
        return true;
    }
    let alone = sum(&mut |id| known.get(&id).map_or(0, |k| k.weight.saturating_add(k.draws.saturating_mul(DRAW))));
    let drawn: Vec<ObjectId> = resources.into_iter().flat_map(|r| forms(doc, r)).map(|(_, id)| id).collect();
    for form in drawn {
        if let Some(Object::Stream(stream)) = doc.objects.get_mut(&form) {
            hush(stream);
        }
    }
    alone <= MAX_CONTENT
}

/// The forms of a resource dictionary, by name.
fn forms<'a>(doc: &'a Document, resources: &'a Dictionary) -> impl Iterator<Item = (&'a [u8], ObjectId)> {
    dict_at(doc, resources, b"XObject").into_iter().flat_map(|forms| forms.iter()).filter_map(|(name, form)| Some((&name[..], stream_at(doc, form)?.0)))
}

/// What pdf-extract goes through for this stream drawn once with these resources, in things,
/// or `u64::MAX`: it would never end, or go deeper than `DEPTH`.
fn drawing(doc: &Document, known: &HashMap<ObjectId, Known>, costs: &mut Costs, id: ObjectId, resources: Option<&Dictionary>, depth: usize) -> u64 {
    let Some(stream) = known.get(&id) else { return 0 };
    let (Some(resources), true) = (resources, stream.draws > 0) else { return stream.weight };
    // The same form with the same resources costs the same, and while it is being counted it
    // is one that draws itself.
    let key = (id, resources as *const Dictionary as usize);
    if let Some(cost) = costs.get(&key) {
        return *cost;
    }
    if depth == 0 {
        return u64::MAX;
    }
    costs.insert(key, u64::MAX);
    let (mut cost, mut dearest) = (stream.weight, DRAW);
    for (name, form) in forms(doc, resources) {
        let times = stream.names.as_ref().map_or(stream.draws, |names| names.get(name).copied().unwrap_or(0).min(stream.draws));
        if times > 0 {
            // A form has resources of its own, or draws with those it is drawn with.
            let own = doc.objects.get(&form).and_then(|o| o.as_stream().ok()).and_then(|form| dict_at(doc, &form.dict, b"Resources"));
            let one = DRAW.saturating_add(drawing(doc, known, costs, form, own.or(Some(resources)), depth - 1));
            cost = cost.saturating_add(one.saturating_mul(times));
            dearest = dearest.max(one);
        }
    }
    // Without the names, every `Do` is taken to draw the dearest.
    if stream.names.is_none() {
        cost = stream.weight.saturating_add(dearest.saturating_mul(stream.draws));
    }
    costs.insert(key, cost);
    cost
}

// ---------------------------------------------------------------- after: words and lines

/// Where a glyph was drawn.
struct Spot {
    at: (f64, f64),
    /// The way its line runs, of length one.
    way: (f64, f64),
    /// How far it reaches along its line.
    reach: f64,
    /// The size of its font.
    size: f64,
}

/// A marked span being read: the text its glyphs stand for, and how far along the line the
/// glyphs reach so far, from the first to the last, in a font of `size`.
struct Span {
    text: String,
    from: f64,
    to: f64,
    size: f64,
}

/// A character of the line being written.
struct Drawn {
    c: char,
    /// How far along the line it starts, and how far along it ends.
    span: (f64, f64),
    /// The size of its font, which gaps are measured in.
    size: f64,
    /// Of a space put in where there was a gap: how wide the gap is, in sizes of the font.
    gap: Option<f64>,
}

/// The widest gap between letters that is taken for the letters of one word set apart, in
/// sizes of the font, and the narrowest that is taken for a space at all.
const SET_APART: f64 = 0.2;
const SPACE: f64 = 0.1;
/// The most characters held as one line. Longer than this it is no line, and goes to the text
/// in pieces.
const MAX_LINE: usize = 1 << 16;

/// The characters of a page as pdf-extract draws them, made into words and lines.
struct Words<'a> {
    text: &'a mut String,
    line: Vec<Drawn>,
    last: Option<Spot>,
    /// A new string of characters starts. Only there is a gap looked for: within a string the
    /// characters follow each other as the font's widths say.
    start: bool,
    span: Option<Span>,
    seen: usize,
    until: Instant,
}

impl<'a> Words<'a> {
    fn new(text: &'a mut String, until: Instant) -> Self {
        Words { text, line: vec![], last: None, start: true, span: None, seen: 0, until }
    }

    /// What one glyph stands for, put on the line side by side where the glyph is, the first
    /// of it on the side it is read from. A character drawn again where it already is, as Word
    /// draws a word twice, a hair apart, for a font with no bold face, is not put twice.
    fn put(&mut self, shown: &str, place: f64, reach: f64, size: f64) {
        let (count, leftward) = (shown.chars().count() as f64, shown.chars().next().is_some_and(right_to_left));
        for (nth, c) in shown.chars().enumerate() {
            let nth = if leftward { count - 1.0 - nth as f64 } else { nth as f64 };
            let span = (place + reach * nth / count, place + reach * (nth + 1.0) / count);
            if reach <= 0.0 || !self.line.iter().rev().take(64).any(|drawn| drawn.c == c && (drawn.span.0 - span.0).abs() < size * 0.05) {
                self.line.push(Drawn { c, span, size, gap: None });
            }
        }
    }

    /// The line goes to the text, and ends there if it is `whole`: a word broken at its end
    /// goes on with what comes next.
    fn close_line(&mut self, whole: bool) {
        let mut line = std::mem::take(&mut self.line);
        if line.iter().any(|drawn| right_to_left(drawn.c)) {
            self.text.push_str(&reading_order(&as_seen(line)));
        } else {
            close_up(&mut line);
            self.text.extend(line.iter().map(|drawn| drawn.c));
        }
        if whole && !self.text.ends_with('\n') && !self.text.is_empty() {
            self.text.push('\n');
        }
        self.last = None;
    }

    /// Stops pdf-extract when the time or the room is used up.
    fn go_on(&mut self) -> Result<(), OutputError> {
        self.seen += 1;
        if self.text.len() + self.line.len() > MAX_TEXT || (self.seen.is_multiple_of(64) && Instant::now() > self.until) {
            return Err(OutputError::FormatError(std::fmt::Error));
        }
        Ok(())
    }
}

/// A line with Hebrew or Arabic in it as it is seen, from the left to the right, with a space
/// where there is a gap. Such a line is drawn from either side, a word or all of it at a time,
/// and what stands between its words often before them all: the order it was drawn in says
/// little, and neither do the gaps that were found on the way.
fn as_seen(mut line: Vec<Drawn>) -> String {
    line.retain(|drawn| drawn.gap.is_none());
    line.sort_by(|one, other| one.span.0.total_cmp(&other.span.0));
    let mut seen = String::new();
    for (nth, drawn) in line.iter().enumerate() {
        if nth > 0 && drawn.span.0 - line[nth - 1].span.1 > drawn.size * SPACE {
            seen.push(' ');
        }
        seen.push(drawn.c);
    }
    seen
}

/// The letters of a word set apart, as those of a heading often are, are one word: where three
/// or more single characters follow each other with gaps narrower than a space between words
/// is, the gaps are no spaces.
fn close_up(line: &mut Vec<Drawn>) {
    let alone = |at: usize| line[at].c != ' ' && (at == 0 || line[at - 1].c == ' ') && line.get(at + 1).is_none_or(|next| next.c == ' ');
    let narrow = |at: &usize| line[*at].gap.is_some_and(|gap| gap < SET_APART) && *at > 0 && at + 1 < line.len() && alone(at - 1) && alone(at + 1);
    let closed: HashSet<usize> = (0..line.len()).filter(narrow).collect();
    if closed.len() >= 2 {
        let mut at = 0;
        line.retain(|_| {
            at += 1;
            !closed.contains(&(at - 1))
        });
    }
}

impl OutputDev for Words<'_> {
    fn output_character(&mut self, trm: &Transform, width: f64, _: f64, font_size: f64, char: &str) -> Result<(), OutputError> {
        self.go_on()?;
        // In the frame of the line, so that text turned on its side has words too.
        let length = trm.m11.hypot(trm.m12);
        let way = if length > 0.0 && length.is_finite() { (trm.m11 / length, trm.m12 / length) } else { (1.0, 0.0) };
        let size = (trm.m11 * trm.m22 - trm.m12 * trm.m21).abs().sqrt() * font_size.abs();
        let size = if size > 0.0 && size.is_finite() { size } else { 1.0 };
        let spot = Spot { at: (trm.m31, trm.m32), way, reach: width * font_size * length, size };
        let place = spot.at.0 * way.0 + spot.at.1 * way.1;
        if let (Some(last), true) = (&self.last, self.start) {
            let (x, y) = (spot.at.0 - last.at.0, spot.at.1 - last.at.1);
            let (along, across) = (x * last.way.0 + y * last.way.1, (y * last.way.0 - x * last.way.1).abs());
            // The gap between the two characters, whichever is to the left.
            let gap = (along - last.reach).max(-(along + spot.reach));
            if across > size * 1.5 || (along < last.reach && across > size * 0.5) {
                // A word broken over two lines is one word, as it is to pdftotext: "hyphen-" and
                // "ation", and "rtcSet-" and "Geometry", where a name goes on in its own case.
                let first = char.chars().next().unwrap_or(' ');
                let ends = matches!(&self.line[..], [.., before, Drawn { c: '-', .. }] if before.c.is_alphabetic());
                let goes_on = matches!(&self.line[..], [.., before, _] if before.c.is_lowercase()) && first.is_uppercase();
                let broken = ends && (first.is_lowercase() || goes_on);
                if broken {
                    self.line.pop();
                }
                self.close_line(!broken);
            } else if gap > size * SPACE {
                self.line.push(Drawn { c: ' ', span: (place, place), size, gap: Some(gap / size) });
            } else if size > last.size * 1.2 && x * last.way.1 - y * last.way.0 > last.size * 0.25 {
                // The mark of a footnote, small and raised, and the footnote right after it
                // are two words, as they are to pdftotext: the size grows and the line drops.
                // A power or an index after its base is not: the size shrinks.
                self.line.push(Drawn { c: ' ', span: (place, place), size, gap: Some(SET_APART) });
            }
        }
        if self.line.len() > MAX_LINE {
            self.close_line(false);
        }
        let mut shown = String::new();
        for c in char.chars() {
            match c {
                'ﬀ' => shown.push_str("ff"),
                'ﬁ' => shown.push_str("fi"),
                'ﬂ' => shown.push_str("fl"),
                'ﬃ' => shown.push_str("ffi"),
                'ﬄ' => shown.push_str("ffl"),
                'ﬅ' | 'ﬆ' => shown.push_str("st"),
                // The hyphen of a word broken at the end of a line, as some write it.
                '\u{ad}' => shown.push('-'),
                c if c.is_control() => {}
                c => shown.push(c),
            }
        }
        // A glyph that takes room and stands for nothing that is written is a space, or a
        // letter the font's map lacks: either way the word ends there. InDesign has the spaces
        // of some fonts as U+0008.
        if shown.is_empty() && spot.reach > 0.0 {
            shown.push(' ');
        }
        match &mut self.span {
            // The glyphs of a span say where its text goes, not what it is.
            Some(span) => (span.from, span.to, span.size) = (span.from.min(place), span.to.max(place + spot.reach), size),
            None => self.put(&shown, place, spot.reach, size),
        }
        (self.start, self.last) = (false, Some(spot));
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.start = true;
        self.go_on()
    }

    /// Where a path starts at `SPAN`, it is the text of a marked span that `rewritten` put
    /// there, or the end of the span.
    fn fill(&mut self, _: &Transform, _: &pdf_extract::ColorSpace, _: &[f64], path: &pdf_extract::Path) -> Result<(), OutputError> {
        use pdf_extract::PathOp::{LineTo, MoveTo};
        if let [MoveTo(x, _), points @ ..] = &path.ops[..]
            && *x == SPAN
        {
            let units: Vec<u16> = points.iter().filter_map(|op| if let LineTo(unit, _) = op { Some(*unit as u16) } else { None }).collect();
            match (self.span.take(), &units[..]) {
                (Some(Span { text, from, to, size }), [1]) if from.is_finite() => self.put(&text, from, to - from, size),
                (_, [1]) => {}
                _ => self.span = Some(Span { text: String::from_utf16(&units).unwrap_or_default(), from: f64::INFINITY, to: f64::NEG_INFINITY, size: 1.0 }),
            }
        }
        self.go_on()
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn begin_page(&mut self, _: u32, _: &MediaBox, _: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn stroke(&mut self, _: &Transform, _: &pdf_extract::ColorSpace, _: &[f64], _: &pdf_extract::Path) -> Result<(), OutputError> {
        self.go_on()
    }
}

/// Hebrew, Arabic and the scripts between them, and the forms their letters take in fonts.
fn right_to_left(c: char) -> bool {
    matches!(c, '\u{590}'..='\u{8ff}' | '\u{fb1d}'..='\u{fdff}' | '\u{fe70}'..='\u{feff}')
}

/// A line as it is seen, from the left, has its Hebrew and Arabic last letter first. A line that
/// is mostly of such letters is turned as a whole, of any other the runs of them are.
fn reading_order(line: &str) -> String {
    let mut chars: Vec<char> = line.chars().collect();
    let letters = chars.iter().filter(|c| c.is_alphabetic()).count();
    if chars.iter().filter(|c| right_to_left(**c)).count() * 2 > letters {
        turn(&mut chars);
        return chars.into_iter().collect();
    }
    let mut at = 0;
    while at < chars.len() {
        // A run of them goes as far as the last before a letter that reads from the left.
        let run = chars[at..].iter().take_while(|c| right_to_left(**c) || !c.is_alphabetic()).count();
        let last = chars[at..at + run].iter().rposition(|c| right_to_left(*c)).filter(|_| right_to_left(chars[at]));
        if let Some(last) = last {
            turn(&mut chars[at..=at + last]);
        }
        at += last.map_or(1, |last| last + 1);
    }
    chars.into_iter().collect()
}

/// Last to first, but for the words and numbers among it that read from the left.
fn turn(chars: &mut [char]) {
    let from_left = |c: char| c.is_alphanumeric() && !right_to_left(c);
    chars.reverse();
    let mut at = 0;
    while at < chars.len() {
        let run = chars[at..].iter().take_while(|c| !right_to_left(**c)).count();
        let last = chars[at..at + run].iter().rposition(|c| from_left(*c)).filter(|_| from_left(chars[at]));
        if let Some(last) = last {
            chars[at..=at + last].reverse();
        }
        at += last.map_or(1, |last| last + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::folder;
    use crate::extract::text_of;
    use pdf_extract::encryption::encrypt_object;
    use pdf_extract::{EncryptionVersion, Permissions};
    use std::io::Write;

    const CATALOG: &[u8] = b"<< /Type /Catalog /Pages 2 0 R >>";
    const HELVETICA: &[u8] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
    const FONT: &str = "/Font << /F1 3 0 R >>";
    const HELLO: &[u8] = b"BT /F1 12 Tf 72 720 Td (Hello there) Tj ET\n";

    /// A stream: what its dictionary has beside the length, and its content.
    fn stream(entries: &str, content: &[u8]) -> Vec<u8> {
        [format!("<< /Length {} {entries} >>\nstream\n", content.len()).as_bytes(), content, b"\nendstream"].concat()
    }

    fn deflated(content: &[u8]) -> Vec<u8> {
        let mut zlib = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::fast());
        zlib.write_all(content).unwrap();
        zlib.finish().unwrap()
    }

    fn packed(entries: &str, content: &[u8]) -> Vec<u8> {
        stream(&format!("/Filter /FlateDecode {entries}"), &deflated(content))
    }

    /// A PDF file of these objects, numbered from 1, the first of them the catalog.
    fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut file = b"%PDF-1.4\n".to_vec();
        let mut starts = vec![];
        for (n, object) in objects.iter().enumerate() {
            starts.push(file.len());
            file.extend([format!("{} 0 obj\n", n + 1).as_bytes(), object, b"\nendobj\n"].concat());
        }
        let table = file.len();
        file.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        starts.iter().for_each(|at| file.extend(format!("{at:010} 00000 n \n").bytes()));
        file.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1).bytes());
        file
    }

    /// A document with a page for each content. Object 3 is Helvetica, `more` are numbered
    /// from 4, and `resources` is what the pages draw with.
    fn document(resources: &str, more: &[Vec<u8>], contents: &[Vec<u8>]) -> Vec<u8> {
        let first = 4 + more.len();
        let kids: String = (0..contents.len()).map(|n| format!("{} 0 R ", first + 2 * n)).collect();
        let mut objects = vec![CATALOG.to_vec(), format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", contents.len()).into_bytes(), HELVETICA.to_vec()];
        objects.extend(more.iter().cloned());
        for (n, content) in contents.iter().enumerate() {
            let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << {resources} >> /Contents {} 0 R >>", first + 2 * n + 1);
            objects.extend([page.into_bytes(), content.clone()]);
        }
        pdf(&objects)
    }

    fn read(name: &str, file: &[u8]) -> Option<String> {
        let folder = folder(&format!("pdf-{name}"));
        std::fs::write(folder.join("read.pdf"), file).unwrap();
        let text = text_of(&folder.join("read.pdf"), file.len() as u64, 1 << 30);
        let _ = std::fs::remove_dir_all(folder);
        text
    }

    /// A font of two-byte codes as objects 4 to 7, named /F2 by `TWO_BYTE`: with this map from
    /// its codes to characters, and these entries in the font within it.
    fn two_byte_font(map: &[u8], inner: &str) -> Vec<Vec<u8>> {
        let font = b"<< /Type /Font /Subtype /Type0 /BaseFont /Mine /Encoding /Identity-H /DescendantFonts [5 0 R] /ToUnicode 7 0 R >>".to_vec();
        let inner = format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Mine /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor 6 0 R {inner} >>"
        );
        let descriptor =
            b"<< /Type /FontDescriptor /FontName /Mine /Flags 4 /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>";
        let map = [
            &b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"[..],
            b"/CMapName /Adobe-Identity-UCS def 1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
            map,
            b"\nendcmap CMapName currentdict /CMap defineresource pop end end",
        ]
        .concat();
        vec![font, inner.into_bytes(), descriptor.to_vec(), packed("", &map)]
    }
    const TWO_BYTE: &str = "/Font << /F1 3 0 R /F2 4 0 R >>";

    #[test]
    fn pdf_gives_the_words_of_every_page_in_their_order() {
        let first = br"BT /F1 24 Tf 72 720 Td (Launch report) Tj ET
            BT /F1 12 Tf 72 690 Td [(Bl\345b\346rgr\370d p\345 en \370)] TJ 0 -14 Td [(The caf\351 is) -300 (open) 20 (ed)] TJ ET
            BT /F1 12 Tf 72 640 Td (Stage) Tj 100 0 Td (Fuel) Tj 100 0 Td (Thrust) Tj
            -200 -14 Td (First) Tj 100 0 Td (Kerosene) Tj 100 0 Td (7600 kN) Tj ET";
        let second = b"BT /F1 12 Tf 1 0 0 1 72 720 Tm (Second) Tj 1 0 0 1 120 720 Tm (page) Tj ET q 0 0 m 10 10 l S Q BT /F1 12 Tf 72 700 Td (zeppelin) Tj ET";
        let file = document(FONT, &[], &[stream("", first), packed("", second)]);
        let lines = ["Launch report", "Blåbærgrød på en ø", "The café is opened", "Stage Fuel Thrust", "First Kerosene 7600 kN", "Second page", "zeppelin"];
        assert_eq!(read("pages", &file).as_deref(), Some(&*lines.join("\n")));
    }

    #[test]
    fn pdf_follows_the_map_of_a_font_to_hebrew_chinese_and_the_letters_of_a_ligature() {
        let map = b"6 beginbfchar <0001> <05E9> <0002> <05DC> <0003> <05D5> <0004> <05DD> % shalom
            <0005> <FB01> <0007> <0020> endbfchar
            2 beginbfrange <0010> <0029> <0061> <0030> <0031> [<4F60> <597D>] endbfrange";
        // Hebrew is drawn as it is seen, from the left, its last letter first.
        let content = b"BT /F2 12 Tf 72 700 Td <0004000300020001> Tj <0007> Tj <00300031> Tj <0007> Tj <001E0015 0005 00120014> Tj ET";
        let file = document(TWO_BYTE, &two_byte_font(map, "/DW 600"), &[stream("", content)]);
        assert_eq!(read("map", &file).as_deref(), Some("שלום 你好 office"));

        assert_eq!(reading_order("They write דלונ 1948 תנשב, so it is."), "They write בשנת 1948 נולד, so it is.", "numbers read from the left");
        assert_eq!(reading_order("12.5 ok םלועה לכל םולש"), "שלום לכל העולם 12.5 ok", "a line that reads from the right");
    }

    #[test]
    fn pdf_turns_hebrew_to_reading_order_however_it_is_drawn() {
        let more = two_byte_font(b"4 beginbfchar <0001> <05E9> <0002> <05DC> <0003> <05D5> <0004> <05DD> endbfchar", "/DW 600");
        // "שלום לו." as LibreOffice draws it: the full stop and the space first, where they are
        // seen, then the letters from the left, the last first. Below it, "שלום" as some draw
        // it: letter by letter from the right.
        let content = b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (.) Tj 1 0 0 1 89.8 700 Tm ( ) Tj ET
            BT /F2 12 Tf 1 0 0 1 75.4 700 Tm <00030002> Tj 1 0 0 1 96 700 Tm <0004000300020001> Tj
            1 0 0 1 200 680 Tm [<0001> 1200 <0002> 1200 <0003> 1200 <0004>] TJ ET";
        assert_eq!(read("drawn", &document(TWO_BYTE, &more, &[stream("", content)])).as_deref(), Some("שלום לו.\nשלום"));
    }

    #[test]
    fn pdf_reads_the_widths_of_a_font_given_as_runs() {
        // Digits and letters as a browser writes them: a run of codes with the one width, which
        // pdf-extract reads as nothing, and no width by default. The second string starts where
        // the first ends, and is one word with it.
        let content = b"BT /F2 12 Tf 72 700 Td <00010002> Tj 12 0 Td <00030004> Tj ET";
        let map = b"1 beginbfrange <0001> <0004> <0061> endbfrange";
        assert_eq!(read("runs", &document(TWO_BYTE, &two_byte_font(map, "/DW 0 /W [1 4 500]"), &[stream("", content)])).as_deref(), Some("abcd"));
        assert_eq!(read("cut", &document(TWO_BYTE, &two_byte_font(map, "/DW 500 /W [1 4]"), &[stream("", content)])).as_deref(), Some("abcd"));
        let mut room = 7;
        assert_eq!(widths_of(&[0.into(), vec![1.into(), 2.into()].into(), 5.into(), 9.into(), 3.into(), 20.into(), 30.into(), 40.into()], &mut room).len(), 4);
        assert_eq!(room, 0, "two of the first run, five of the second, none of the third");
    }

    #[test]
    fn pdf_takes_letters_set_apart_for_one_word() {
        // A heading with its letters set apart, two words set apart, numbers a space apart, and
        // two letters alone.
        let content = b"BT /F1 24 Tf 72 700 Td [(P) -119 (L) -119 (A) -119 (N)] TJ 0 -40 Td [(d) -119 (e) -590 (s) -119 (e) -119 (m)] TJ
            0 -40 Td [(1) -300 (2) -300 (3) -300 (4)] TJ 0 -40 Td [(a) -119 (b)] TJ ET";
        assert_eq!(read("apart", &document(FONT, &[], &[stream("", content)])).as_deref(), Some("PLAN\nde sem\n1 2 3 4\na b"));
    }

    #[test]
    fn pdf_ends_a_word_at_a_glyph_that_takes_room_and_says_nothing() {
        // InDesign has the spaces of some fonts as U+0008, and the map of a font may lack the
        // space altogether.
        let map = b"1 beginbfrange <0001> <0004> <0061> endbfrange 1 beginbfchar <0005> <0008> endbfchar";
        let content = b"BT /F2 12 Tf 72 700 Td <000100020005 00030004 0006 00010002> Tj ET";
        assert_eq!(read("room", &document(TWO_BYTE, &two_byte_font(map, "/DW 600"), &[stream("", content)])).as_deref(), Some("ab cd ab"));
    }

    #[test]
    fn pdf_weighs_what_lopdf_would_build_and_refuses_too_much() {
        assert_eq!(things(b"1 0 R /Name (a string) [1 2] << /K 1 >>", false), 18);
        assert_eq!(things(b"0 0 m 1 1 l S", true), 19, "an operator is five");
        assert_eq!(weight(b"[1 2 (a]b) <<>>] endobj [3 4]", 1000), 10, "the bracket in the string closes nothing");
        assert_eq!(weight(b"<< /A (endobj) /B [1] >> endobj", 1000), 11);
        assert_eq!(weight(b"42 endobj", 1000), 1);
        assert_eq!(weight(&b"[".repeat(500), 100), 105, "no further than asked");
        // A string that never closes, after every `obj`: not read to the end for each of them.
        let start = Instant::now();
        assert!(heavy(&b"1 0 obj (".repeat(200_000), 1 << 20));
        assert!(start.elapsed() < crate::test_limit(Duration::from_secs(5)), "took {:?}", start.elapsed());

        // A list of numbers heavier than the file allows, and one that is not.
        let listed = |count: usize| document(FONT, &[format!("[{}]", "0 ".repeat(count)).into_bytes()], &[stream("", HELLO)]);
        assert_eq!(read("light", &listed(100_000)).as_deref(), Some("Hello there"));
        assert_eq!(read("heavy", &listed(600_000)), None);
        // A page of operators that would take gigabytes to decode is left out; one that would not is read.
        let operators = |count: usize| document(FONT, &[], &[packed("", &[HELLO, &b"n ".repeat(count)].concat())]);
        assert_eq!(read("operators", &operators(300_000)).as_deref(), Some("Hello there"));
        assert_eq!(read("too many", &operators(500_000)), None);
    }

    #[test]
    fn pdf_reads_text_on_its_side_and_joins_a_word_broken_over_two_lines() {
        // A word broken at a hyphen, a name that goes on in its own case, as the names of an
        // API do in a manual, and one that goes on in capitals, which is left as it is.
        let content = b"BT /F1 12 Tf 14 TL 72 720 Td (The words come out one by one, hyphen-) Tj T* (ation and all, rtcSet-) Tj T* (Geometry too. RTC_VER-) Tj
            T* (TEX stays.) Tj ET BT /F1 12 Tf 0 1 -1 0 300 100 Tm [(Up) -400 (the) -400 (si) 10 (de)] TJ 0 1 -1 0 320 100 Tm (Next to it) Tj ET";
        let lines = ["The words come out one by one, hyphenation and all, rtcSetGeometry too. RTC_VER-", "TEX stays.", "Up the side", "Next to it"];
        assert_eq!(read("side", &document(FONT, &[], &[stream("", content)])).as_deref(), Some(&*lines.join("\n")));
    }

    #[test]
    fn pdf_reads_strings_shown_with_quotes_and_content_packed_twice_over() {
        let quotes = b"BT /F1 12 Tf 14 TL 72 720 Td (It's the first line) Tj (and the second) ' 0 0 (and the third) \" ET";
        // ASCII85 of deflated content, as ReportLab writes it.
        let mut armoured = String::new();
        for four in deflated(HELLO).chunks(4) {
            let mut word = [0u8; 4];
            word[..four.len()].copy_from_slice(four);
            let digits: Vec<u8> = (0..5).rev().map(|n| (u32::from_be_bytes(word) / 85u32.pow(n) % 85) as u8 + b'!').collect();
            armoured.push_str(std::str::from_utf8(&digits[..four.len() + 1]).unwrap());
        }
        let file = document(FONT, &[], &[packed("", quotes), stream("/Filter [/ASCII85Decode /FlateDecode]", format!("{armoured}~>").as_bytes())]);
        assert_eq!(read("quotes", &file).as_deref(), Some("It's the first line\nand the second\nand the third\nHello there"));
    }

    #[test]
    fn pdf_reads_what_is_written_in_the_fields_of_a_form() {
        let page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R /Annots [6 0 R 8 0 R] >>";
        let field = b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (Ada Lovelace) /Rect [100 690 300 710] /AP << /N 7 0 R >> >>";
        let look = stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 20] /Resources << /Font << /F1 3 0 R >> >>",
            b"/Tx BMC BT /F1 12 Tf 2 5 Td (Ada Lovelace) Tj ET EMC",
        );
        let link = b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] >>";
        let objects =
            [CATALOG, b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>", HELVETICA, page, &stream("", b"BT /F1 12 Tf 72 700 Td (Name:) Tj ET"), field, &look, link];
        assert_eq!(read("form", &pdf(&objects.map(<[u8]>::to_vec))).as_deref(), Some("Name:\nAda Lovelace"));
    }

    #[test]
    fn pdf_opens_with_an_empty_password_and_reads_objects_kept_in_streams() {
        // Encrypted as PDF 1.4 has it, by lopdf. The page is in an object stream and nowhere else.
        let locked = |password: &str| {
            let mut keys = Document::new();
            keys.trailer.set("ID", vec![Object::string_literal("0123456789abcdef"), Object::string_literal("0123456789abcdef")]);
            let version =
                EncryptionVersion::V2 { document: &keys, owner_password: "owner", user_password: password, key_length: 128, permissions: Permissions::all() };
            let state = EncryptionState::try_from(version).unwrap();
            let sealed = |id: u32, entries: &str, content: &[u8]| {
                let mut object = Object::Stream(Stream::new(Dictionary::new(), content.to_vec()));
                encrypt_object(&state, (id, 0), &mut object).unwrap();
                stream(entries, &object.as_stream().unwrap().content)
            };
            let lock: String = state
                .encode()
                .unwrap()
                .iter()
                .map(|(key, value)| {
                    format!(
                        "/{} {} ",
                        String::from_utf8_lossy(key),
                        match value {
                            Object::String(bytes, _) => format!("<{}>", bytes.iter().map(|byte| format!("{byte:02X}")).collect::<String>()),
                            other => format!("{other:?}"),
                        }
                    )
                })
                .collect();
            let page = format!("7 0 << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << {FONT} >> /Contents 4 0 R >>");
            let lock = format!("<< {lock}>>").into_bytes();
            let objects = [
                CATALOG,
                b"<< /Type /Pages /Kids [7 0 R] /Count 1 >>",
                HELVETICA,
                &sealed(4, "", HELLO),
                &sealed(5, "/Type /ObjStm /N 1 /First 4", page.as_bytes()),
                &lock,
            ];
            let mut file = pdf(&objects.map(<[u8]>::to_vec));
            let root = memmem::rfind(&file, b"/Root 1 0 R").unwrap();
            file.splice(root..root, *b"/Encrypt 6 0 R /ID [(0123456789abcdef) (0123456789abcdef)] ");
            assert!(memmem::find(&file, b"Hello").is_none() && memmem::find(&file, b"/Page ").is_none());
            file
        };
        assert_eq!(read("open", &locked("")).as_deref(), Some("Hello there"));
        assert_eq!(read("locked", &locked("secret")), None, "it takes a password");

        let mut file = vec![];
        Document::load_mem(&document(FONT, &[], &[stream("", HELLO)])).unwrap().save_modern(&mut file).unwrap();
        assert!(memmem::find(&file, b"/ObjStm").is_some() && memmem::find(&file, b"/Type /Page").is_none());
        assert_eq!(read("streams", &file).as_deref(), Some("Hello there"));
    }

    #[test]
    fn pdf_without_text_is_none_and_no_bytes_make_it_panic() {
        let image = stream("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8", b"\x00\x7f\x7f\xff");
        let scan = document("/XObject << /Im 4 0 R >>", &[image], &[stream("", b"q 612 0 0 792 0 0 cm /Im Do Q")]);
        assert_eq!(read("scan", &scan), None);

        let folder = folder("pdf-no-text");
        std::fs::write(folder.join("empty.pdf"), b"").unwrap();
        assert_eq!(text(&folder.join("empty.pdf"), 1000), None);
        assert_eq!(text(&folder.join("missing.pdf"), 1000), None);
        let good = document(FONT, &[], &[packed("", HELLO)]);
        std::fs::write(folder.join("large.pdf"), &good).unwrap();
        assert_eq!(text(&folder.join("large.pdf"), good.len() as u64 - 1), None, "larger than allowed");
        assert_eq!(text(&folder.join("large.pdf"), good.len() as u64).as_deref(), Some("Hello there\n"));
        let _ = std::fs::remove_dir_all(folder);

        // Bytes of no meaning: by themselves, after a good start, in the middle of a good file.
        let mut seed = 7u32;
        let mut noise = |len: usize| -> Vec<u8> {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            };
            (0..len).map(|_| next()).collect()
        };
        assert_eq!(read("noise", &noise(5000)), None);
        assert_eq!(read("noise", &[&b"%PDF-1.7\n"[..], &noise(5000)].concat()), None);
        for at in (0..good.len()).step_by(good.len() / 40) {
            let _ = read("cut", &good[..at]);
            let _ = read("noise", &[&good[..at], &noise(50), &good[at..]].concat());
            let _ = read("noise", &[&good[..at], &noise(10), &good[(at + 10).min(good.len())..]].concat());
        }
    }

    #[test]
    fn pdf_mends_the_fonts_pdf_extract_gives_up_on() {
        let reads = |name: &str, font: &str, more: &[Vec<u8>]| {
            let more: Vec<Vec<u8>> = [&[font.as_bytes().to_vec()], more].concat();
            assert_eq!(read(name, &document("/Font << /F1 4 0 R >>", &more, &[stream("", HELLO)])).as_deref(), Some("Hello there"), "{name}");
        };
        reads("an encoding it does not know", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /StandardEncoding >>", &[]);
        reads(
            "widths that do not match",
            "<< /Type /Font /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 40 /Widths [250 250 250] >>",
            &[],
        );
        let few = b"1 beginbfchar <48> <0048> endbfchar";
        reads("a map that lacks letters", "<< /Type /Font /Subtype /Type1 /BaseFont /Mine /ToUnicode 5 0 R >>", &[stream("", few)]);
        reads(
            "a map that is no map",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 5 0 R >>",
            &[stream("", b"<< /Not [a map")],
        );
        reads("a map by a name", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode /Identity-V >>", &[]);
    }

    #[test]
    fn pdf_holds_for_what_is_made_to_break_it() {
        let within = |name: &str, file: &[u8]| {
            let start = Instant::now();
            let text = read(name, file);
            assert!(start.elapsed() < crate::test_limit(TIME + Duration::from_secs(20)), "{name} took {:?}", start.elapsed());
            text
        };
        let form = |content: &[u8], resources: &str| stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 9 9] /Resources << {resources} >>"), content);
        let drawing = [HELLO, b"/X Do\n"].concat();

        // A form that draws itself, and eight forms that each draw the next a hundred times.
        let itself = document(&format!("{FONT} /XObject << /X 4 0 R >>"), &[form(b"/X Do", "/XObject << /X 4 0 R >>")], &[stream("", &drawing)]);
        assert_eq!(within("itself", &itself).as_deref(), Some("Hello there"));
        let forms: Vec<Vec<u8>> = (0..8).map(|n| form(&b"/X Do\n".repeat(100), &format!("/XObject << /X {} 0 R >>", (5 + n).min(11)))).collect();
        let hundreds = document(&format!("{FONT} /XObject << /X 4 0 R >>"), &forms, &[stream("", &drawing)]);
        assert_eq!(within("hundreds", &hundreds).as_deref(), Some("Hello there"));

        // Forms in forms: read as deep as `DEPTH`, and left out when they go deeper.
        let nested = |deep: usize| {
            let forms: Vec<Vec<u8>> = (1..=deep)
                .map(|n| {
                    form(if n < deep { b"/X Do" } else { b"BT /F1 12 Tf 72 700 Td (Deep down) Tj ET" }, &format!("{FONT} /XObject << /X {} 0 R >>", 4 + n))
                })
                .collect();
            document(&format!("{FONT} /XObject << /X 4 0 R >>"), &forms, &[stream("", &drawing)])
        };
        assert_eq!(within("deep", &nested(DEPTH)).as_deref(), Some("Hello there\nDeep down"));
        assert_eq!(within("deeper", &nested(DEPTH + 1)).as_deref(), Some("Hello there"));
        let brackets = format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /Deep {}{} >>", "[".repeat(90), "]".repeat(90));
        assert_eq!(within("brackets", &document("/Font << /F1 4 0 R >>", &[brackets.into()], &[stream("", HELLO)])).as_deref(), Some("Hello there"));

        // A page that is its own parent, with nothing of its own to stop at.
        let orphan =
            pdf(&[CATALOG, b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>", HELVETICA, b"<< /Type /Page /Parent 4 0 R /Contents 5 0 R >>", &stream("", HELLO)]
                .map(<[u8]>::to_vec));
        assert_eq!(within("orphan", &orphan), None);

        // A page that names the same megabyte of content two thousand times.
        let megabyte = [HELLO, &b" ".repeat(1 << 20)].concat();
        let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Resources << {FONT} >> /Contents [{}] >>", "5 0 R ".repeat(2000));
        let many =
            pdf(&[CATALOG.to_vec(), b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(), HELVETICA.to_vec(), page.into_bytes(), packed("", &megabyte)]);
        assert_eq!(within("many", &many), None);

        // Maps and a font program of brackets in brackets, and a map of every code there is.
        let mapped = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 5 0 R >>";
        for open in ["[", "(", "<<"] {
            let file = document("/Font << /F1 4 0 R >>", &[mapped.into(), packed("", open.repeat(100_000).as_bytes())], &[stream("", HELLO)]);
            assert_eq!(within("opened", &file).as_deref(), Some("Hello there"), "{open}");
        }
        let every = b"1 beginbfrange <00000000> <FFFFFFFF> <0041> endbfrange 1 beginbfrange <00> <FF> <D800> endbfrange 1 beginbfchar <48> <D800> endbfchar";
        within("every", &document("/Font << /F1 4 0 R >>", &[mapped.into(), stream("", every)], &[stream("", HELLO)]));
        let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Mine /FirstChar 32 /LastChar 32 /Widths [250] /FontDescriptor 5 0 R >>";
        let descriptor = "<< /Type /FontDescriptor /FontName /Mine /Flags 4 /FontBBox [0 0 1 1] /ItalicAngle 0 /Ascent 1 /Descent 0 /CapHeight 1 /StemV 1 \
                          /FontFile 6 0 R >>";
        let program = packed("/Length1 200005", &[&b"{".repeat(200_000)[..], b"eexec"].concat());
        let braces = document("/Font << /F1 4 0 R >>", &[font.into(), descriptor.into(), program], &[stream("", HELLO)]);
        assert_eq!(within("braces", &braces).as_deref(), Some("Hello there"));

        // Text before any font is named: what pdf-extract gives up on costs that page, no more.
        let unnamed = document(FONT, &[], &[stream("", b"BT 72 720 Td (No font) Tj ET"), stream("", HELLO)]);
        assert_eq!(within("unnamed", &unnamed).as_deref(), Some("Hello there"));
    }

    #[test]
    fn pdf_unpacks_no_more_than_it_may_and_reads_no_longer() {
        let spaces = b" ".repeat(100_000);
        let of = |filter: &str, content: Vec<u8>| {
            let mut stream = Stream::new(Dictionary::new(), content);
            stream.dict.set("Filter", filter);
            stream
        };
        assert_eq!(unpacked(&of("FlateDecode", deflated(&spaces)), 100_000).map(|b| b.len()), Some(100_000));
        assert_eq!(unpacked(&of("FlateDecode", deflated(&spaces)), 99_999).map(|b| b.len()), None);
        assert_eq!(unpacked(&of("FlateDecode", [b"xx", &deflated(&spaces)[2..]].concat()), 99_999).map(|b| b.len()), None, "deflate without the start of zlib");
        assert_eq!(unpacked(&of("ASCII85Decode", b"z".repeat(1000)), 3999).map(|b| b.len()), None);
        assert_eq!(unpacked(&of("LZWDecode", vec![0; 1000]), 4_000_000 - 1).map(|b| b.len()), None);
        assert_eq!(unpacked(&of("DCTDecode", spaces.clone()), 10).map(|b| b.len()), Some(100_000), "not unpacked at all");

        // An object stream is unpacked by lopdf as it loads: all of them share what room there is.
        let mut kept = Object::Stream(of("FlateDecode", deflated(&spaces)));
        kept.as_stream_mut().unwrap().dict.set("Type", "ObjStm");
        let mut over = kept.clone();
        let size = deflated(&spaces).len();
        LOAD.set(Some(Load { room: 150_000, things: MAX_THINGS, left: 2 * size as u64, until: Instant::now() + TIME, locked: false }));
        at_load((1, 0), &mut kept);
        at_load((2, 0), &mut over);
        assert_eq!((LOAD.get().unwrap().room, kept.as_stream().unwrap().content.len(), &over.as_stream().unwrap().content[..]), (50_000, size, &b"\n"[..]));
        // And a file has no more bytes of streams than it has bytes: one listed a third time ends the loading.
        assert!(catch_unwind(AssertUnwindSafe(|| at_load((3, 0), &mut kept.clone()))).is_err());

        // Three hundred pages are read to the last, and none when the time is up.
        let pages: Vec<Vec<u8>> = (1..=300).map(|n| stream("", format!("BT /F1 12 Tf 72 720 Td (Page {n} of many) Tj ET").as_bytes())).collect();
        let file = document(FONT, &[], &pages);
        let text = super::read(file.clone(), Instant::now() + Duration::from_secs(60)).unwrap();
        assert_eq!((text.lines().count(), text.lines().last()), (300, Some("Page 300 of many")));
        assert_eq!(super::read(file, Instant::now()), None);
    }

    #[test]
    fn pdf_looks_through_the_tables_of_a_file_before_they_are_read() {
        // A table in a stream: well made, with fields wider than any file is long, without
        // any width, and unpacking to more than the file it is the table of.
        let table = |widths: &str, rows: &[u8]| {
            let mut file = b"%PDF-1.5\n".to_vec();
            let mut starts = vec![];
            for (n, object) in [
                CATALOG,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
                &stream("", HELLO),
                HELVETICA,
            ]
            .iter()
            .enumerate()
            {
                starts.push(file.len() as u32);
                file.extend([format!("{} 0 obj\n", n + 1).as_bytes(), object, b"\nendobj\n"].concat());
            }
            let at = file.len();
            let listed: Vec<u8> = [0].iter().chain(&starts).flat_map(|start| [&[1u8][..], &start.to_be_bytes(), &[0]].concat()).collect();
            let rows = if rows.is_empty() { &listed } else { rows };
            file.extend([b"6 0 obj\n", &packed(&format!("/Type /XRef /Size 6 /Root 1 0 R /W [{widths}]"), rows)[..], b"\nendobj\n"].concat());
            file.extend(format!("startxref\n{at}\n%%EOF\n").bytes());
            file
        };
        assert_eq!(read("table", &table("1 4 1", b"")).as_deref(), Some("Hello there"));
        // A table that is refused is not read: the file is read by a table made of its objects.
        assert_eq!(read("wide", &table("20000000000 4 1", b"")).as_deref(), Some("Hello there"));
        assert_eq!(read("narrow", &table("0 0 0", b"")).as_deref(), Some("Hello there"));
        assert_eq!(read("long", &table("1 4 1", &[0; 100_000])).as_deref(), Some("Hello there"));

        // One stream listed ten thousand times, in a file as it is and in one that says it is
        // encrypted: neither is read ten thousand times. The first is read by a table made anew,
        // the second takes a password it does not have.
        for (trailer, text) in [("", Some("Hello there")), ("/Encrypt 7 0 R /ID [(0123456789abcdef) (0123456789abcdef)]", None)] {
            let lock = b"<< /Filter /Standard /V 1 /R 2 /O (oooooooooooooooooooooooooooooooo) /U (uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu) /P -4 >>";
            let objects = [
                CATALOG,
                b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>",
                HELVETICA,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>",
                &stream("", HELLO),
                &stream("", &[b' '; 100_000]),
                lock,
            ];
            let file = pdf(&objects.map(<[u8]>::to_vec));
            let table = memmem::find(&file, b"xref\n0 8").unwrap();
            let listed = format!("xref\n0 10008\n{}", format!("{:010} 00000 n \n", memmem::find(&file, b"6 0 obj").unwrap()).repeat(10_000));
            let end = format!("trailer\n<< /Size 10008 /Root 1 0 R {trailer} >>\nstartxref\n{table}\n%%EOF\n");
            let file = [&file[..table], listed.as_bytes(), &file[table + 9..memmem::find(&file, b"trailer").unwrap()], end.as_bytes()].concat();
            let start = Instant::now();
            assert_eq!(read("listed", &file).as_deref(), text, "{trailer}");
            assert!(start.elapsed() < crate::test_limit(Duration::from_secs(10)), "{trailer} took {:?}", start.elapsed());
        }
    }

    #[test]
    fn pdf_reads_a_file_with_junk_before_its_header_or_tables_it_cannot_use() {
        let good = document(FONT, &[], &[stream("", HELLO)]);
        assert_eq!(read("junk", &[b"\xef\xbb\xbfsome junk header line\r\n", &good[..]].concat()).as_deref(), Some("Hello there"));
        // A comment put in after the header, which puts every offset off; a startxref that
        // points nowhere; a file cut before its table, with no trailer at all.
        assert_eq!(read("offsets", &[&good[..9], b"% a comment added later\n", &good[9..]].concat()).as_deref(), Some("Hello there"));
        let word = memmem::rfind(&good, b"startxref").unwrap();
        assert_eq!(read("nowhere", &[&good[..word], b"startxref\n999999\n%%EOF\n"].concat()).as_deref(), Some("Hello there"));
        assert_eq!(read("no table", &good[..memmem::find(&good, b"xref\n0 ").unwrap()]).as_deref(), Some("Hello there"));
        // The same, of a file that keeps its page and its catalog in an object stream.
        let mut modern = vec![];
        Document::load_mem(&good).unwrap().save_modern(&mut modern).unwrap();
        let word = memmem::rfind(&modern, b"startxref").unwrap();
        assert_eq!(read("modern", &[&modern[..word], b"startxref\n0\n%%EOF\n"].concat()).as_deref(), Some("Hello there"));
        assert_eq!(rebuilt(b"%PDF-1.4\nnothing 3 obj here endobj"), None);
    }

    #[test]
    fn pdf_takes_the_text_a_marked_span_says_its_glyphs_stand_for() {
        // A ligature drawn as a glyph the font maps to Þ, said to be "first" by Word; a hyphen
        // put in to break the line, said to be nothing; and an Arabic letter drawn by LibreOffice
        // as the base glyph of another letter, which a span says is ح, and a dot.
        let map = b"2 beginbfchar <0001> <062E> <0002> <0628> endbfchar";
        let content = b"/Span << /ActualText (first) >> BDC BT /F1 12 Tf 72 700 Td (\\336rst) Tj ET EMC BT /F1 12 Tf 72 680 Td (plain) Tj
            /Span << /ActualText <FEFF> >> BDC (-) Tj EMC ET
            BT /F2 12 Tf 72 660 Td /Span << /ActualText <FEFF062D> >> BDC <0001> Tj EMC <0002> Tj ET";
        let file = document(TWO_BYTE, &two_byte_font(map, "/DW 600"), &[stream("", content)]);
        assert_eq!(read("span", &file).as_deref(), Some("first\nplain\nبح"));
    }

    #[test]
    fn pdf_ends_a_word_where_the_size_or_the_line_changes() {
        // The mark of a footnote in a small font, raised, and the footnote right after it with
        // no space between, as LaTeX draws them. A power and an index stay with their base.
        let content = b"BT /F1 12 Tf 72 700 Td (E = mc) Tj 5 Ts /F1 8 Tf (2) Tj 0 Ts /F1 12 Tf ( and F) Tj -3 Ts /F1 8 Tf (s) Tj 0 Ts /F1 12 Tf (/2) Tj ET
            BT /F1 6 Tf 1 0 0 1 61.2 59.64 Tm (1) Tj /F1 8 Tf 1 0 0 1 64.68 56.76 Tm (Footnotebody here) Tj ET";
        assert_eq!(read("footnote", &document(FONT, &[], &[stream("", content)])).as_deref(), Some("E = mc2 and Fs/2\n1 Footnotebody here"));
    }

    #[test]
    fn pdf_reads_differences_from_the_built_in_encoding_of_a_standard_font() {
        // Ghostscript writes the standard fonts with differences and no base: "fi" is \256 in
        // the standard encoding, and 39 is the right quote, as wide as the font lays it out.
        let times = "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding << /Differences [150 /endash] >> >>";
        let symbol = "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol /Encoding << /Differences [1 /alpha] >> >>";
        let content = b"BT /F1 12 Tf 72 700 Td (\\256rst that's \\226 fine) Tj ET BT /F2 12 Tf 72 680 Td (\\001bg) Tj ET";
        let file = document("/Font << /F1 4 0 R /F2 5 0 R >>", &[times.into(), symbol.into()], &[stream("", content)]);
        assert_eq!(read("differences", &file).as_deref(), Some("first that’s – fine\nαβγ"));
    }

    #[test]
    fn pdf_writes_a_word_drawn_twice_for_bold_once() {
        // Word draws a word a second time a hair to the right for a font that has no bold face.
        let content = b"BT /F1 12 Tf 72 700 Td (Boldword and normal) Tj ET BT /F1 12 Tf 72.3 700 Td (Boldword) Tj ET";
        assert_eq!(read("bold", &document(FONT, &[], &[stream("", content)])).as_deref(), Some("Boldword and normal"));
    }
}

//! Presentations: PowerPoint (.pptx, .ppsx, .potx) and OpenDocument (.odp). Both are zip
//! files of XML.
//!
//! PowerPoint keeps each slide in a part of its own, `ppt/slides/slide7.xml`, and what belongs
//! to a slide in further parts, which the slide's list of relations names: its diagrams, its
//! charts, its speaker notes and its comments. A chart has such a list too, for the text boxes
//! that are drawn onto it. Masters and layouts are left out: their text is "Click to edit
//! title". OpenDocument keeps every slide, with its notes and its comments, in `content.xml`,
//! and a chart or a formula in a folder of its own.

use std::io::Read;
use std::path::Path;

use super::{MAX_ENTRY, MAX_TEXT, start, xml_text};

type Zip = zip::ZipArchive<std::fs::File>;
/// How a part is read by `xml_text`: the elements that end a line, and those left out.
type Rules = (&'static [&'static str], &'static [&'static str]);

/// A slide, its notes, its comments, its diagrams and the text boxes on its charts in
/// PowerPoint. A paragraph, a line break and a comment end a line. So does a field, which
/// ONLYOFFICE sets before the words of its paragraph with nothing between, and so do the parts
/// of a formula: what stands over and under the line of a fraction has nothing between it.
/// Left out are the animations, whose only text is the names of what they change,
/// "style.visibility"; the older form that PowerPoint writes beside a newer one, which would
/// say everything twice; the name of a table's style, "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";
/// and where a text box stands on its chart, "0.0312480469970627".
// ponytail: all text between the tags counts, so a slide written with a line break between two
// runs of one word gives two words. No program that was tried writes slides so; read only what
// stands in `a:t` if one turns up.
const SLIDE: Rules = (
    &["p", "br", "cm", "fld", "num", "den", "e", "sub", "sup", "deg", "lim", "fName"],
    &["timing", "Fallback", "tableStyleId", "from", "to"],
);
/// A chart in PowerPoint. Its title is paragraphs; the names of its series and categories are
/// values, each of which ends a line. Left out are where in the sheet a value is from,
/// "Sheet1!$B$1", how a number is shown, "General", and the numbers that are drawn.
const CHART: Rules = (&["p", "br", "v", "pt"], &["f", "formatCode", "numCache", "numDim"]);
/// The parts that hold words of a slide, by the start of their names under `ppt/`, in the
/// order they are read in: what is on the slide comes before what is said about it. The last
/// kind is named by a chart, not by a slide: the text boxes that are drawn onto it.
const LINKED: &[&str] = &["diagrams/data", "charts/chart", "notesSlides/", "comments/", "drawings/drawing"];

/// OpenDocument. A paragraph, a heading and a line break end a line, and so does the blank
/// space that is written as an element. Then the texts that are no paragraphs, and would join
/// the words next to them: the title and the description of a picture, the author, the
/// initials and the date of a comment, the header, the footer and the date that the slides
/// share, and the letters, the numbers and the signs of a formula. The initials have one name
/// in the standard and another in the files that LibreOffice wrote before it. Left out are a
/// picture written as text, and the styles, where the form of a date or a number holds text:
/// ".", "km".
const OPENDOCUMENT: Rules = (
    &[
        "p", "h", "line-break", "tab", "s", "title", "desc", "creator", "creator-initials", "sender-initials", "date", "header-decl", "footer-decl",
        "date-time-decl", "mi", "mn", "mo", "mtext",
    ],
    &["binary-data", "automatic-styles"],
);

/// What stands in a field whose value the program did not know when it wrote the file: the
/// number of the slide, how many there are, and its name. All else in a field is read: a date,
/// a number, and the words that some programs write into fields.
const UNKNOWN: [&str; 3] = ["<number>", "<count>", "<slide-name>"];

/// What a part costs of the room at the least. To find and to open a part takes time also
/// when it is empty, and a list of relations may name an empty part four million times. No
/// part that a program wrote is as small as this, so for those nothing changes.
const TOLL: u64 = 64;

pub fn text(path: &Path, max: u64) -> Option<String> {
    within(path, max, MAX_ENTRY)
}

/// The text, when all the parts read unpack to no more than `room` together. One part may
/// unpack to `MAX_ENTRY` and a file may hold thousands of parts: without a limit for them
/// together, a small file could keep the reader busy for hours.
fn within(path: &Path, max: u64, mut room: u64) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > max {
        return None;
    }
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut out = String::new();
    if zip.index_for_name("content.xml").is_some() {
        // The slides, then what is set into them as documents of their own, each in its folder.
        let objects: Vec<String> = zip.file_names().filter(|name| name.ends_with("/content.xml")).map(String::from).collect();
        let _ = std::iter::once("content.xml".to_string()).chain(objects).all(|name| add(&mut out, &mut zip, &name, &mut room));
    } else {
        powerpoint(&mut out, &mut zip, &mut room);
    }
    let out = UNKNOWN.iter().fold(out, |out, unknown| out.replace(unknown, ""));
    (!out.trim().is_empty()).then_some(out)
}

/// The slides in the order of their numbers, each followed by what belongs to it. Every part
/// in the folder of the slides is one, whatever its name; one without a number comes first.
// ponytail: the order the slides are shown in is written in `ppt/presentation.xml`, and is the
// order of their numbers only when the program that moved a slide gave the parts new names.
// The words found are the same either way; read the order from there if it comes to matter.
fn powerpoint(out: &mut String, zip: &mut Zip, room: &mut u64) {
    let mut slides: Vec<(u64, String)> = zip
        .file_names()
        .filter_map(|name| {
            let file = name.strip_prefix("ppt/slides/")?.strip_suffix(".xml")?;
            Some((file.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0), name.to_string()))
        })
        .collect();
    slides.sort();
    // A slide, its chart, and the text boxes on that chart: two steps from the slide.
    let _ = slides.iter().all(|(_, name)| follow(out, zip, name, room, 2));
}

/// Adds the text of a part, then of the parts that its relations name, then of those that
/// theirs name, and so on for `steps` steps. `false` when nothing more is to be read.
fn follow(out: &mut String, zip: &mut Zip, name: &str, room: &mut u64, steps: u8) -> bool {
    // The part before its relations: should those take all the room, the part is read.
    let more = add(out, zip, name, room);
    let Some((folder, file)) = name.rsplit_once('/').filter(|_| more && steps > 0) else { return more };
    let relations = part(zip, &format!("{folder}/_rels/{file}.rels"), room).unwrap_or_default();
    linked(&relations).all(|name| follow(out, zip, &name, room, steps - 1))
}

/// The names of the parts that a part's relations point to and that hold words of a slide. A
/// relation names its part as seen from the slide, "../notesSlides/notesSlide1.xml", or from
/// the top, "/ppt/notesSlides/notesSlide1.xml". The number in that name is not the slide's:
/// the first slide that has notes has "notesSlide1.xml", wherever it stands.
fn linked(relations: &str) -> impl Iterator<Item = String> {
    // Every value of an attribute stands between two quotes, and only the name of a part
    // starts as these do.
    let parts = || relations.split(['"', '\'']).filter_map(|value| value.strip_prefix("../").or_else(|| value.strip_prefix("/ppt/")));
    LINKED.iter().flat_map(move |kind| parts().filter(move |part| part.starts_with(kind)).map(|part| format!("ppt/{part}")))
}

/// Adds the text of one part, with a line break after it, so that a slide ends a line.
/// `false` when nothing more is to be read: the text is as long as it may be, or `room` is
/// used up.
fn add(out: &mut String, zip: &mut Zip, name: &str, room: &mut u64) -> bool {
    if let Some(xml) = part(zip, name, room) {
        let (lines, skip) = if name.starts_with("ppt/charts/") {
            CHART
        } else if name.starts_with("ppt/") {
            SLIDE
        } else {
            OPENDOCUMENT
        };
        out.push_str(start(&xml_text(xml.as_bytes(), lines, skip), MAX_TEXT.saturating_sub(out.len())));
        out.push('\n');
    }
    out.len() <= MAX_TEXT && *room > 0
}

/// One part of the zip, unpacked, if `room` has room for it; what it took is taken from `room`,
/// and `TOLL` at the least. A part that is not UTF-8, or has a zero byte in it, is no text: it
/// is encrypted, or written in UTF-16, and would be read as noise, three bytes of it for each
/// byte of the part.
fn part(zip: &mut Zip, name: &str, room: &mut u64) -> Option<String> {
    let mut bytes = vec![];
    let read = zip.by_name(name).ok()?.take(room.saturating_add(1)).read_to_end(&mut bytes);
    let fits = bytes.len() as u64 <= *room;
    *room = room.saturating_sub(TOLL.max(bytes.len() as u64));
    String::from_utf8(bytes).ok().filter(|text| read.is_ok() && fits && !text.contains('\0'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::tests::{folder, zip_file};
    use crate::extract::{text_of, tidy};

    const MAX: u64 = 20 * 1024 * 1024;

    /// What `text_of` gives for a file: by its extension, and tidied, as the store gets it.
    fn read(path: &Path) -> Option<String> {
        text_of(path, std::fs::metadata(path).unwrap().len(), MAX)
    }

    fn write(path: &Path, entries: &[(&str, Vec<u8>)]) {
        zip_file(path, &entries.iter().map(|(name, bytes)| (*name, bytes.as_slice())).collect::<Vec<_>>());
    }

    /// A slide, a notes slide, a master or a layout of PowerPoint, around these shapes. The
    /// blank space between some of the tags is there as LibreOffice writes it.
    fn slide(shapes: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree><p:nvGrpSpPr>    <p:cNvPr id="1" name=""/>    </p:nvGrpSpPr>{shapes}</p:spTree></p:cSld></p:sld>"#
        )
        .into_bytes()
    }

    /// A shape with these paragraphs, each given as what stands inside `a:p`.
    fn shape(paragraphs: &[&str]) -> String {
        let paragraphs: String = paragraphs.iter().map(|p| format!(r#"<a:p><a:pPr lvl="1"/>{p}<a:endParaRPr lang="da-DK"/></a:p>"#)).collect();
        format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1" descr="Not read"/></p:nvSpPr><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp>"#)
    }

    /// A run of text as PowerPoint writes it.
    fn run(text: &str) -> String {
        format!(r#"<a:r><a:rPr lang="da-DK" b="1"/><a:t>{text}</a:t></a:r>"#)
    }

    /// The relations of a slide to these parts.
    fn relations(targets: &[&str]) -> Vec<u8> {
        let all: String = targets
            .iter()
            .enumerate()
            .map(|(n, t)| format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/x" Target="{t}"/>"#))
            .collect();
        format!(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{all}</Relationships>"#)
            .into_bytes()
    }

    /// A deck of three slides: the second has a table, notes, comments, a chart and a diagram.
    fn deck() -> Vec<(&'static str, Vec<u8>)> {
        let first = slide(&shape(&[
            &run("Årsmøde på Ærø"),
            &(run("Blåbærgrød, café og ") + &run("שלום") + &run(" og ") + &run("你好世界")),
            &(run("Sammen") + &run("sat") + &run(" ord")),
            &(run("Første linje") + r#"<a:br><a:rPr lang="da-DK"/></a:br>"# + &run("anden linje")),
            &(run("venstre") + &run("&#9;") + &run("højre &amp; &lt;bred&gt;")),
            &(run("Side ") + r#"<a:fld id="{1}" type="slidenum"><a:rPr/><a:t>&lt;number&gt;</a:t></a:fld>"#),
        ]));
        let table = "<p:graphicFrame><a:graphic><a:graphicData><a:tbl>".to_string()
            + &["Vare", "Pris", "Raket", "4200"].map(|cell| format!("<a:tc><a:txBody><a:bodyPr/><a:p>{}</a:p></a:txBody></a:tc>", run(cell))).concat()
            + "</a:tbl></a:graphicData></a:graphic></p:graphicFrame>";
        let twice = format!(
            r#"<mc:AlternateContent><mc:Choice Requires="a14">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
            shape(&[&run("Ligning")]),
            shape(&[&run("Gammel form")])
        );
        // The animations stand after the shapes, last in the slide.
        let moving = "<p:timing><p:tnLst><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:tnLst></p:timing></p:sld>";
        let second = String::from_utf8(slide(&(shape(&[&run("Priser")]) + &table + &twice))).unwrap().replace("</p:sld>", moving).into_bytes();
        let chart = r#"<c:chartSpace><c:chart>
<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Salg i tal</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:ser>
<c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Norden</c:v></c:pt></c:strCache></c:strRef></c:tx>
<c:cat><c:strRef><c:f>Sheet1!$A$2:$A$3</c:f><c:strCache>
<c:pt idx="0"><c:v>Forår</c:v></c:pt><c:pt idx="1"><c:v>Efterår</c:v></c:pt></c:strCache></c:strRef></c:cat>
<c:val><c:numRef><c:f>Sheet1!$B$2:$B$3</c:f><c:numCache><c:formatCode>General</c:formatCode>
<c:pt idx="0"><c:v>46</c:v></c:pt></c:numCache></c:numRef></c:val>
</c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let diagram = r#"<dgm:dataModel><dgm:ptLst>
<dgm:pt modelId="{1}"><dgm:prSet phldrT="[Text]"/><dgm:t><a:bodyPr/><a:p><a:r><a:t>Plan</a:t></a:r></a:p></dgm:t></dgm:pt>
<dgm:pt modelId="{2}"><dgm:t><a:bodyPr/><a:p><a:r><a:t>Byg</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst></dgm:dataModel>"#;
        let comments = r#"<p:cmLst><p:cm authorId="0" idx="1"><p:pos x="1" y="1"/><p:text>For dyrt</p:text></p:cm>
<p:cm authorId="0" idx="2"><p:pos x="2" y="2"/><p:text>Enig</p:text></p:cm></p:cmLst>"#;
        vec![
            // Not in the order they are read in, as a zip has no order that means anything.
            ("ppt/slides/slide10.xml", slide(&shape(&[&run("Tak for i dag")]))),
            ("ppt/slideMasters/slideMaster1.xml", slide(&shape(&[&run("Click to edit Master title style")]))),
            ("ppt/slideLayouts/slideLayout1.xml", slide(&shape(&[&run("Click to edit title")]))),
            ("ppt/notesMasters/notesMaster1.xml", slide(&shape(&[&run("Click to edit the notes")]))),
            ("ppt/notesSlides/notesSlide2.xml", slide(&shape(&[&run("Sidste note: spørgsmål?")]))),
            ("ppt/notesSlides/notesSlide1.xml", slide(&shape(&[&run("Husk at nævne budgettet."), &run("Tak til Søren.")]))),
            ("ppt/notesSlides/notesSlide7.xml", slide(&shape(&[&run("Notes of no slide")]))),
            ("ppt/comments/comment1.xml", comments.as_bytes().to_vec()),
            ("ppt/charts/chart1.xml", chart.as_bytes().to_vec()),
            ("ppt/diagrams/data1.xml", diagram.as_bytes().to_vec()),
            ("ppt/diagrams/layout1.xml", b"<dgm:layoutDef><dgm:title>Not read</dgm:title></dgm:layoutDef>".to_vec()),
            ("ppt/slides/slide2.xml", second),
            ("ppt/slides/slide1.xml", first),
            // The notes come first and from the top here, to show that the order read in is not theirs.
            (
                "ppt/slides/_rels/slide2.xml.rels",
                relations(&[
                    "/ppt/notesSlides/notesSlide1.xml",
                    "../slideLayouts/slideLayout1.xml",
                    "../comments/comment1.xml",
                    "../charts/chart1.xml",
                    "../diagrams/layout1.xml",
                    "../diagrams/data1.xml",
                ]),
            ),
            ("ppt/slides/_rels/slide10.xml.rels", relations(&["../slideLayouts/slideLayout1.xml", "../notesSlides/notesSlide2.xml"])),
            ("[Content_Types].xml", b"<Types/>".to_vec()),
        ]
    }

    /// What is read from `deck`, line by line.
    const DECK: &[&str] = &[
        "Årsmøde på Ærø",
        "Blåbærgrød, café og שלום og 你好世界",
        "Sammensat ord",
        "Første linje",
        "anden linje",
        "venstre højre & <bred>",
        "Side",
        "Priser",
        "Vare",
        "Pris",
        "Raket",
        "4200",
        "Ligning",
        "Plan",
        "Byg",
        "Salg i tal",
        "Norden",
        "Forår",
        "Efterår",
        "Husk at nævne budgettet.",
        "Tak til Søren.",
        "For dyrt",
        "Enig",
        "Tak for i dag",
        "Sidste note: spørgsmål?",
    ];

    #[test]
    fn slides_powerpoint_gives_each_slide_then_what_belongs_to_it() {
        let d = folder("slides-powerpoint");
        write(&d.join("deck.pptx"), &deck());
        assert_eq!(read(&d.join("deck.pptx")).unwrap().lines().collect::<Vec<_>>(), DECK);

        // A show, a template and a name in capitals are the same file by another name. Not
        // "DECK.PPTX": on Windows and macOS that is deck.pptx itself.
        for name in ["deck.ppsx", "deck.potx", "COPY.PPTX"] {
            std::fs::copy(d.join("deck.pptx"), d.join(name)).unwrap();
            assert_eq!(read(&d.join(name)), read(&d.join("deck.pptx")), "{name}");
        }

        // A byte order mark before a slide is no part of its first word.
        let marked = [b"\xEF\xBB\xBF".to_vec(), slide(&shape(&[&run("Alene")]))].concat();
        write(&d.join("marked.pptx"), &[("ppt/slides/slide1.xml", marked)]);
        assert_eq!(read(&d.join("marked.pptx")).as_deref(), Some("Alene"));

        // Nothing but masters and layouts, or slides nothing is written on: no text.
        write(
            &d.join("bare.potx"),
            &[("ppt/slideLayouts/slideLayout1.xml", slide(&shape(&[&run("Click to edit title")]))), ("ppt/slides/slide1.xml", slide(&shape(&[""])))],
        );
        assert_eq!(read(&d.join("bare.potx")), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_opendocument_gives_slides_notes_comments_and_charts() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">
<office:automatic-styles><style:style style:name="dp1"/></office:automatic-styles><office:body><office:presentation>
<presentation:footer-decl presentation:name="ftr1">Fortrolig sidefod</presentation:footer-decl><draw:page draw:name="Forside">
<draw:frame presentation:class="title"><draw:text-box><text:p>Årsmøde på Ærø</text:p></draw:text-box></draw:frame>
<draw:frame presentation:class="outline"><draw:text-box><text:list><text:list-item><text:p>Blåbærgrød, café og שלום og 你好世界</text:p>
<text:list><text:list-item>
<text:p><text:span text:style-name="T1">Sammen</text:span><text:span text:style-name="T2">sat</text:span> ord</text:p>
</text:list-item></text:list></text:list-item>
<text:list-item><text:p>Første linje<text:line-break/>anden linje</text:p></text:list-item>
<text:list-item><text:p>venstre<text:tab/>højre<text:s text:c="3"/>&amp; &lt;bred&gt;</text:p></text:list-item></text:list></draw:text-box></draw:frame>
<draw:frame><draw:image><office:binary-data>iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk</office:binary-data></draw:image>
<svg:title>Raketten</svg:title><svg:desc>Et billede af raketten</svg:desc></draw:frame>
<draw:frame><draw:text-box>
<text:p>Side <text:page-number>&lt;number&gt;</text:page-number> af <text:page-count>&lt;count&gt;</text:page-count></text:p>
</draw:text-box></draw:frame>
<presentation:notes><draw:page-thumbnail draw:page-number="1"/><draw:frame presentation:class="notes"><draw:text-box>
<text:p>Husk at nævne budgettet.</text:p><text:p>Tak til Søren.</text:p></draw:text-box></draw:frame></presentation:notes>
</draw:page><draw:page draw:name="Tabel"><draw:frame presentation:class="title"><draw:text-box><text:p>Priser</text:p></draw:text-box></draw:frame>
<draw:frame><table:table><table:table-column/>
<table:table-row><table:table-cell><text:p>Vare</text:p></table:table-cell><table:table-cell><text:p>Pris</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell><text:p>Raket</text:p></table:table-cell><table:table-cell><text:p>4200</text:p></table:table-cell></table:table-row>
</table:table></draw:frame><draw:frame><draw:object xlink:href="./Object 1"/></draw:frame>
<presentation:notes><draw:frame presentation:class="notes" presentation:placeholder="true"><draw:text-box/></draw:frame></presentation:notes>
<officeooo:annotation><dc:creator>Anna Berg</dc:creator><dc:date>2026-09-29T10:00:00</dc:date><text:p>For dyrt</text:p></officeooo:annotation>
</draw:page></office:presentation></office:body></office:document-content>"#;
        let chart = r#"<office:document-content><office:body><office:chart><chart:chart><chart:title><text:p>Salg i tal</text:p></chart:title>
<table:table><table:table-row><table:table-cell><text:p>Forår</text:p></table:table-cell><table:table-cell><text:p>46</text:p></table:table-cell>
</table:table-row></table:table></chart:chart></office:chart></office:body></office:document-content>"#;
        let d = folder("slides-opendocument");
        let entries = [
            ("mimetype", b"application/vnd.oasis.opendocument.presentation".to_vec()),
            ("Object 1/content.xml", chart.as_bytes().to_vec()),
            ("content.xml", content.as_bytes().to_vec()),
            // The masters, with what stands on every slide until something is written there.
            ("styles.xml", b"<office:document-styles><text:p>Click to edit the title</text:p></office:document-styles>".to_vec()),
        ];
        write(&d.join("deck.odp"), &entries);
        let expected = [
            "Fortrolig sidefod",
            "Årsmøde på Ærø",
            "Blåbærgrød, café og שלום og 你好世界",
            "Sammensat ord",
            "Første linje",
            "anden linje",
            "venstre",
            "højre",
            "& <bred>",
            "Raketten",
            "Et billede af raketten",
            "Side af",
            "Husk at nævne budgettet.",
            "Tak til Søren.",
            "Priser",
            "Vare",
            "Pris",
            "Raket",
            "4200",
            "Anna Berg",
            "2026-09-29T10:00:00",
            "For dyrt",
            "Salg i tal",
            "Forår",
            "46",
        ];
        assert_eq!(read(&d.join("deck.odp")).unwrap().lines().collect::<Vec<_>>(), expected);

        // Encrypted, the content is bytes of any kind under the same name: no text, not noise.
        let noise: Vec<u8> = (0..4000u32).map(|n| (n.wrapping_mul(2654435761) >> 13) as u8).collect();
        write(&d.join("locked.odp"), &[("content.xml", noise)]);
        assert_eq!(read(&d.join("locked.odp")), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_broken_files_give_none_or_some_text_and_never_panic() {
        let d = folder("slides-broken");
        let path = d.join("broken.pptx");
        let given = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            text(&path, MAX)
        };
        assert_eq!(given(b""), None, "an empty file");
        assert_eq!(given(b"PK\x03\x04"), None, "the start of a zip and no more");
        assert_eq!(given(b"Not a presentation at all, only named as one."), None);
        assert_eq!(text(&d.join("missing.pptx"), MAX), None, "no such file");
        assert_eq!(text(&d, MAX), None, "a folder");

        // Bytes of any kind, of many lengths, with and without the start of a zip.
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut random = |length: usize| -> Vec<u8> {
            (0..length)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect()
        };
        for length in [1, 22, 100, 4096, 70_000] {
            let bytes = random(length);
            assert_eq!(given(&bytes), None);
            assert_eq!(given(&[b"PK\x03\x04", bytes.as_slice()].concat()), None);
        }

        // A good file cut off anywhere, and with any one byte of it changed. It is a small one, a
        // slide with its notes, as each byte of it is tried.
        let small = [
            ("ppt/slides/slide1.xml", slide(&shape(&[&run("Alene")]))),
            ("ppt/slides/_rels/slide1.xml.rels", relations(&["../notesSlides/notesSlide1.xml"])),
            ("ppt/notesSlides/notesSlide1.xml", slide(&shape(&[&run("Husk det")]))),
        ];
        write(&d.join("good.pptx"), &small);
        let good = std::fs::read(d.join("good.pptx")).unwrap();
        for cut in 0..good.len() {
            assert_eq!(given(&good[..cut]), None, "cut off at {cut}: where the parts are is written last");
        }
        let whole = given(&good).expect("the whole file");
        let mut changed = 0;
        for at in 0..good.len() {
            let mut bytes = good.clone();
            bytes[at] ^= 0xFF;
            changed += usize::from(given(&bytes) != Some(whole.clone()));
        }
        assert!(changed > 0 && changed < good.len(), "{changed} changes were seen");

        // A slide that breaks off gives what is left of it, and the slides after it are read.
        let broken = [
            ("ppt/slides/slide1.xml", b"<p:sld><a:p><a:r><a:t>Halvt</a:t></a:r></a:p><a:p><a:r><a:t>og af".to_vec()),
            ("ppt/slides/slide2.xml", slide(&shape(&[&run("Helt")]))),
        ];
        write(&path, &broken);
        assert_eq!(read(&path).as_deref(), Some("Halvt\nog af\nHelt"));

        // Relations that point nowhere, out of the file, or to the slide itself a thousand times.
        let around = relations(&["../notesSlides/missing.xml", "../notesSlides/../../../etc/passwd", "../comments/"]);
        let again = relations(&["../notesSlides/../slides/slide1.xml"; 1000]);
        write(&path, &[("ppt/slides/slide1.xml", slide(&shape(&[&run("Alene")]))), ("ppt/slides/_rels/slide1.xml.rels", around)]);
        assert_eq!(read(&path).as_deref(), Some("Alene"));
        write(&path, &[("ppt/slides/slide1.xml", slide(&shape(&[&run("Alene")]))), ("ppt/slides/_rels/slide1.xml.rels", again)]);
        assert_eq!(read(&path).as_deref(), Some("Alene"));

        // A slide with a zero byte in it, as one in UTF-16 has, is left out; the others are read.
        let wide: Vec<u8> = "<p:sld><a:t>Bred</a:t></p:sld>".encode_utf16().flat_map(u16::to_le_bytes).collect();
        write(&path, &[("ppt/slides/slide1.xml", wide), ("ppt/slides/slide2.xml", slide(&shape(&[&run("Smal")])))]);
        assert_eq!(read(&path).as_deref(), Some("Smal"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_read_no_more_than_the_limits_allow() {
        let d = folder("slides-limits");
        let (path, other) = (d.join("deck.pptx"), d.join("deck.odp"));
        let slides: Vec<Vec<u8>> = ["En", "To", "Tre"].iter().map(|word| slide(&shape(&[&run(word)]))).collect();
        let (one, two) = (slides[0].len() as u64, slides[1].len() as u64);
        write(
            &path,
            &[("ppt/slides/slide1.xml", slides[0].clone()), ("ppt/slides/slide2.xml", slides[1].clone()), ("ppt/slides/slide3.xml", slides[2].clone())],
        );
        let size = std::fs::metadata(&path).unwrap().len();
        assert_eq!(text(&path, size).map(|t| tidy(&t)).as_deref(), Some("En\nTo\nTre"));
        assert_eq!(text(&path, size - 1), None, "a file larger than allowed is not opened");

        // The parts share what may be unpacked: the slide that does not fit ends the reading.
        let with = |path: &Path, room: u64| within(path, MAX, room).map(|t| tidy(&t));
        assert_eq!(with(&path, one + two).as_deref(), Some("En\nTo"));
        assert_eq!(with(&path, one + two - 1).as_deref(), Some("En"));
        assert_eq!(with(&path, one - 1), None);
        assert_eq!(with(&path, 0), None);

        // Relations too large for what is left cost the slide its notes, not its own text.
        let (list, notes) = (relations(&["../notesSlides/notesSlide1.xml"]), slide(&shape(&[&run("Husk det")])));
        let (listed, noted) = (list.len() as u64, notes.len() as u64);
        write(&path, &[("ppt/slides/slide1.xml", slides[0].clone()), ("ppt/slides/_rels/slide1.xml.rels", list), ("ppt/notesSlides/notesSlide1.xml", notes)]);
        assert_eq!(with(&path, one + listed + noted).as_deref(), Some("En\nHusk det"));
        assert_eq!(with(&path, one + listed + noted - 1).as_deref(), Some("En"));
        assert_eq!(with(&path, one + listed - 1).as_deref(), Some("En"));

        // A part with nothing in it has its price: relations may name it over and over, and each time it is looked up.
        let list = relations(&["../comments/none.xml"; 10]);
        let listed = list.len() as u64;
        let (first, second) = (("ppt/slides/slide1.xml", slides[0].clone()), ("ppt/slides/slide2.xml", slides[1].clone()));
        write(&path, &[first, ("ppt/slides/_rels/slide1.xml.rels", list), ("ppt/comments/none.xml", vec![]), second]);
        assert_eq!(with(&path, one + listed + 10 * TOLL + two).as_deref(), Some("En\nTo"));
        assert_eq!(with(&path, one + listed + 10 * TOLL + two - 1).as_deref(), Some("En"));

        // The same holds for OpenDocument, where one part holds every slide.
        let content = b"<office:body><text:p>Alt i en</text:p></office:body>";
        write(&other, &[("content.xml", content.to_vec())]);
        assert_eq!(with(&other, content.len() as u64).as_deref(), Some("Alt i en"));
        assert_eq!(with(&other, content.len() as u64 - 1), None);

        // Text past the most that is kept ends the reading as well: the third slide is not read.
        let long = slide(&shape(&[&run(&"ord ".repeat(MAX_TEXT / 4 + 1))]));
        write(&path, &[("ppt/slides/slide1.xml", long.clone()), ("ppt/slides/slide2.xml", long), ("ppt/slides/slide3.xml", slides[2].clone())]);
        let read = text(&path, MAX).unwrap();
        assert!(read.len() > MAX_TEXT && read.len() < MAX_TEXT + 100 && !read.contains("Tre"), "{} bytes", read.len());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_powerpoint_reads_the_text_boxes_drawn_onto_a_chart() {
        let d = folder("slides-chart-boxes");
        let path = d.join("deck.pptx");
        let chart = r#"<c:chartSpace><c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Flights by vehicle</a:t></a:r></a:p></c:rich></c:tx></c:title>
</c:chart><c:userShapes r:id="rId1"/></c:chartSpace>"#;
        // Where a box stands is written as text, right before the box.
        let boxes = r#"<c:userShapes><cdr:relSizeAnchor><cdr:from><cdr:x>0.0312480469970627</cdr:x><cdr:y>0.833240751027664</cdr:y></cdr:from>
<cdr:to><cdr:x>0.531216798950066</cdr:x><cdr:y>0.944339517831352</cdr:y></cdr:to><cdr:sp><cdr:txBody><a:bodyPr/><a:p><a:r>
<a:t>Source: telemetry archive</a:t></a:r></a:p></cdr:txBody></cdr:sp></cdr:relSizeAnchor></c:userShapes>"#;
        write(
            &path,
            &[
                ("ppt/slides/slide1.xml", slide(&shape(&[&run("Flights per year")]))),
                ("ppt/slides/_rels/slide1.xml.rels", relations(&["../charts/chart1.xml"])),
                ("ppt/charts/chart1.xml", chart.as_bytes().to_vec()),
                // The chart names its text boxes; the slide does not.
                ("ppt/charts/_rels/chart1.xml.rels", relations(&["../drawings/drawing1.xml"])),
                ("ppt/drawings/drawing1.xml", boxes.as_bytes().to_vec()),
            ],
        );
        assert_eq!(read(&path).as_deref(), Some("Flights per year\nFlights by vehicle\nSource: telemetry archive"));

        // A chart that names itself as its text boxes is read twice, and that is the end of it.
        write(
            &path,
            &[
                ("ppt/slides/slide1.xml", slide(&shape(&[&run("Flights per year")]))),
                ("ppt/slides/_rels/slide1.xml.rels", relations(&["../charts/chart1.xml"])),
                ("ppt/charts/chart1.xml", b"<c:chart><a:p>Round</a:p></c:chart>".to_vec()),
                ("ppt/charts/_rels/chart1.xml.rels", relations(&["../charts/chart1.xml"])),
            ],
        );
        assert_eq!(read(&path).as_deref(), Some("Flights per year\nRound\nRound"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_keep_the_words_that_stand_in_fields() {
        let d = folder("slides-fields");
        // ONLYOFFICE writes every run of a text box that has the number of its slide in it as that number.
        let field = |text: &str| format!(r#"<a:fld id="{{D038279B-FC19-497E-A7D1-5ADD9CAF016F}}" type="slidenum"><a:rPr/><a:t>{text}</a:t></a:fld>"#);
        let first = field("This is slide ") + &field("*") + &field(" of the confidential briefing");
        // And it sets a date before the words of its paragraph, with nothing between them.
        let date = field("26-09-29") + &run("Prepared on ") + &run(" by the propulsion team");
        write(&d.join("deck.pptx"), &[("ppt/slides/slide1.xml", slide(&shape(&[&first, &field("No field here: oxidiser tanks are full"), &date])))]);
        let expected =
            ["This is slide", "*", "of the confidential briefing", "No field here: oxidiser tanks are full", "26-09-29", "Prepared on by the propulsion team"];
        assert_eq!(read(&d.join("deck.pptx")).unwrap().lines().collect::<Vec<_>>(), expected);

        // The same in OpenDocument, beside a number that the program knew and one that it did not.
        let content = r#"<office:body><text:p><text:page-number>Prepared on </text:page-number><text:date>Autumn 2026</text:date></text:p>
<text:p>Page <text:page-number>&lt;number&gt;</text:page-number> of <text:page-count>8</text:page-count></text:p></office:body>"#;
        write(&d.join("deck.odp"), &[("content.xml", content.as_bytes().to_vec())]);
        assert_eq!(read(&d.join("deck.odp")).as_deref(), Some("Prepared on Autumn 2026\nPage of 8"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_opendocument_with_a_password_gives_no_text() {
        let d = folder("slides-locked");
        // Each part is encrypted by itself and keeps its name. A small one may have no zero byte in it.
        let slides: Vec<u8> = (0..4736u32).map(|n| (n.wrapping_mul(2654435761) >> 13) as u8).collect();
        let formula: Vec<u8> = (0..272u32).map(|n| 1 + ((n.wrapping_mul(2246822519) >> 11) % 255) as u8).collect();
        assert!(slides.contains(&0) && !formula.contains(&0));
        write(&d.join("locked.odp"), &[("content.xml", slides), ("Object 2/content.xml", formula)]);
        assert_eq!(read(&d.join("locked.odp")), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_powerpoint_leaves_out_the_name_of_a_tables_style() {
        let d = folder("slides-table-style");
        // The style stands right before the first row, as PowerPoint and ONLYOFFICE write it.
        let style = r#"<a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr>"#;
        let row = format!(r#"<a:tr h="370840"><a:tc><a:txBody><a:bodyPr/><a:p>{}</a:p></a:txBody><a:tcPr/></a:tc></a:tr>"#, run("Item"));
        let table = format!("<p:graphicFrame><a:graphic><a:graphicData><a:tbl>{style}{row}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
        write(&d.join("deck.pptx"), &[("ppt/slides/slide1.xml", slide(&table))]);
        assert_eq!(read(&d.join("deck.pptx")).as_deref(), Some("Item"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_keep_the_words_of_a_formula_apart() {
        let d = folder("slides-formula");
        let words = |path: &Path| read(path).unwrap().split_whitespace().map(String::from).collect::<Vec<_>>();
        // A fraction and a power: nothing stands between what is over and under the line, or between a base and what it is raised to.
        let part = |text: &str| format!("<m:r><m:t>{text}</m:t></m:r>");
        let fraction = format!("<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>", part("budget"), part("fuel"));
        let power = format!("<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>", part("growth"), part("years"));
        let formula = format!("<a14:m><m:oMath>{fraction}{power}</m:oMath></a14:m>");
        write(&d.join("deck.pptx"), &[("ppt/slides/slide1.xml", slide(&shape(&[&formula])))]);
        assert_eq!(words(&d.join("deck.pptx")), ["budget", "fuel", "growth", "years"]);

        // OpenDocument writes a formula twice, as it is drawn and as it was typed, the one right after the other.
        let formula = r#"<math><semantics><mrow><mi>speed</mi><mo stretchy="false">=</mo><mfrac><mi>distance</mi><mi>time</mi></mfrac></mrow>"#.to_string()
            + r#"<annotation encoding="StarMath 5.0">speed = {distance} over {time}</annotation></semantics></math>"#;
        write(&d.join("deck.odp"), &[("content.xml", b"<office:body/>".to_vec()), ("Object 1/content.xml", formula.into_bytes())]);
        assert_eq!(words(&d.join("deck.odp")), ["speed", "=", "distance", "time", "speed", "=", "{distance}", "over", "{time}"]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_opendocument_reads_no_styles_and_keeps_the_initials_apart() {
        let d = folder("slides-styles");
        // The form of a date holds the signs between its parts as text, and the initials stand right before the date.
        let content = r#"<office:document-content><office:automatic-styles><number:date-style style:name="D1">
<number:day number:style="long"/><number:text>.</number:text><number:month number:style="long"/><number:text>.</number:text><number:year number:style="long"/>
</number:date-style></office:automatic-styles><office:body><office:presentation><draw:page><draw:frame><draw:text-box>
<text:p>Quarterly rocket review</text:p><text:p>Slide name: <loext:page-name>&lt;slide-name&gt;</loext:page-name><text:s/>end</text:p>
</draw:text-box></draw:frame><officeooo:annotation><dc:creator>Anna Berg</dc:creator>
<meta:creator-initials>AB</meta:creator-initials><dc:date>2026-09-29T10:00:00</dc:date><text:p>Too expensive</text:p></officeooo:annotation>
</draw:page></office:presentation></office:body></office:document-content>"#;
        write(&d.join("deck.odp"), &[("content.xml", content.as_bytes().to_vec())]);
        let expected = ["Quarterly rocket review", "Slide name:", "end", "Anna Berg", "AB", "2026-09-29T10:00:00", "Too expensive"];
        assert_eq!(read(&d.join("deck.odp")).unwrap().lines().collect::<Vec<_>>(), expected);

        // The initials as LibreOffice named them before the standard did.
        let older = content.replace("meta:creator-initials", "loext:sender-initials");
        write(&d.join("older.odp"), &[("content.xml", older.into_bytes())]);
        assert_eq!(read(&d.join("older.odp")).unwrap().lines().collect::<Vec<_>>(), expected);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_powerpoint_reads_a_slide_whatever_its_part_is_named() {
        let d = folder("slides-names");
        let named = |name: &'static str, word: &str| (name, slide(&shape(&[&run(word)])));
        let slides = [named("ppt/slides/Slide10.xml", "Tenth"), named("ppt/slides/page3.xml", "Third"), named("ppt/slides/slide2.xml", "Second")];
        // The first part of a kind may have no number at all.
        write(&d.join("deck.pptx"), &[slides.as_slice(), &[named("ppt/slides/slide.xml", "First")]].concat());
        assert_eq!(read(&d.join("deck.pptx")).as_deref(), Some("First\nSecond\nThird\nTenth"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_give_no_control_characters_and_no_soft_hyphens() {
        let d = folder("slides-control");
        // The marks of the store, the start of a command for a terminal, and a place where a word may be broken.
        let runs = [run("before&#1;marked&#2;after &#27;[31mred"), run("The bud\u{AD}get")];
        write(&d.join("deck.pptx"), &[("ppt/slides/slide1.xml", slide(&shape(&[&runs[0], &runs[1]])))]);
        assert_eq!(read(&d.join("deck.pptx")).as_deref(), Some("beforemarkedafter [31mred\nThe budget"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn slides_build_no_more_text_than_is_kept() {
        let d = folder("slides-memory");
        let path = d.join("deck.pptx");
        // One text, longer than what is kept, is cut where it is read and not after it was built.
        write(&path, &[("ppt/slides/slide1.xml", [b"<a:t>".to_vec(), b"word ".repeat(2 * MAX_TEXT / 5)].concat())]);
        let built = text(&path, MAX).map_or(0, |text| text.len());
        assert!(built > MAX_TEXT - 100 && built <= MAX_TEXT + 100, "{built} bytes of text were built");

        // Bytes that are no UTF-8 would each become a sign of three bytes: such a part is no text.
        write(&path, &[("ppt/slides/slide1.xml", [b"<a:t>".to_vec(), vec![0xFF; 2 * MAX_TEXT]].concat())]);
        assert_eq!(text(&path, MAX), None);
        write(&d.join("deck.odp"), &[("content.xml", [b"<text:p>".to_vec(), vec![0xFF; 2 * MAX_TEXT]].concat())]);
        assert_eq!(text(&d.join("deck.odp"), MAX), None);
        let _ = std::fs::remove_dir_all(d);
    }
}

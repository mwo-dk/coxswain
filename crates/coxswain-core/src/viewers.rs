//! Which viewer shows a file: one ordered table of rules, the first that fits names the kind
//! (`markdown`, `chess`, `sheet` …). The desktop app gets the table with its config and
//! evaluates it (`gui/src/lib.js` `previewKind`); `kind_of` evaluates it here, and a test holds
//! the two to the same answers (`gui/src/viewer-kinds.json`).
//!
//! A rule looks at names only, so it is safe to ask of every entry the cursor comes to (a file
//! only in the cloud is not downloaded, a file inside an archive not unpacked). What a file's
//! bytes say (a BOM in a `.json`, a `.vhd` that is VHDL) is asked later, by the viewer the rule
//! chose.

use serde::Serialize;
use std::sync::OnceLock;

/// What a rule tests. A `dir` rule is asked of folders only, every other kind of rule of files
/// only. Names are compared in lower case (`to_lowercase`, JavaScript's `toLowerCase`), against
/// patterns written in lower case: no case-blind matching, whose Unicode folding differs between
/// Rust and JavaScript.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "m", rename_all = "snake_case")]
pub enum Match {
    /// A folder; with `re`, one whose name it matches.
    Dir { re: Option<&'static str> },
    /// The name matches `re` (a regular expression both Rust's `regex` and JavaScript read the
    /// same way: no look-around, `[0-9]` for digits).
    Name { re: &'static str },
    /// The name's ending, the part after its last dot, is one of these ("" with no dot).
    Ending { endings: Vec<&'static str> },
    /// The name ends with one of these (`.tar.gz`, `.7z.001`).
    Suffix { suffixes: Vec<&'static str> },
}

/// One rule: when `when` fits, the file is shown as `kind`.
#[derive(Clone, Debug, Serialize)]
pub struct Rule {
    pub when: Match,
    pub kind: &'static str,
}

fn rule(when: Match, kind: &'static str) -> Rule {
    Rule { when, kind }
}
fn name(re: &'static str, kind: &'static str) -> Rule {
    rule(Match::Name { re }, kind)
}
fn ending(endings: &[&'static str], kind: &'static str) -> Rule {
    rule(Match::Ending { endings: endings.to_vec() }, kind)
}

/// Pictures a web view shows itself.
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "avif"];
/// Tracker music: facts and sample names, no playback.
const TRACKER: &[&str] = &["mod", "xm", "s3m", "it"];
const VIDEO: &[&str] = &["mp4", "webm", "mkv", "mov", "m4v", "ogv"];
const AUDIO: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "opus", "aac"];
const FONT: &[&str] = &["ttf", "otf", "woff", "woff2"];
const SHEET: &[&str] = &["csv", "tsv", "xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Kinds drawn in the browser (graphviz, asciidoc, parquet) or by an external tool
/// (`preview_tools`), by their endings.
const CONVERTED: &[(&str, &[&str])] = &[
    ("graphviz", &["dot", "gv"]),
    ("asciidoc", &["adoc", "asciidoc"]),
    ("scad", &["scad"]),
    ("kicad", &["kicad_sch", "kicad_pcb"]),
    ("parquet", &["parquet", "pq"]),
    ("latex", &["tex", "ltx"]),
    ("typst", &["typ"]),
    ("postscript", &["ps", "eps"]),
    // What LibreOffice opens and the app cannot draw itself.
    (
        "office",
        &[
            "doc", "docm", "dot", "dotx", "odt", "ott", "rtf", "wri", "ppt", "pptx", "pps", "ppsx", "pot", "potx", "odp", "otp", "odg", "vsd", "vsdx", "pub", "wpd", "wps", "wk1", "wks", "123", "lwp", "sxw", "sxc", "sxi", "sxd", "abw", "cwk", "pages", "key", "numbers", "fodt", "fods", "fodp", "fodg", "cdr", "pmd", "wmf", "emf",
        ],
    ),
    ("xps", &["xps", "oxps"]),
    ("plantuml", &["puml", "plantuml", "pu", "iuml", "wsd"]),
    ("d2", &["d2"]),
    ("ditaa", &["ditaa"]),
    ("svgbob", &["bob", "svgbob"]),
    ("pikchr", &["pikchr"]),
    ("nomnoml", &["nomnoml", "noml"]),
    ("wavedrom", &["wavedrom"]),
    ("vega", &["vg", "vega"]),
    ("vegalite", &["vl", "vegalite"]),
    ("dbml", &["dbml"]),
    ("erd", &["er", "erd"]),
    ("bytefield", &["bytefield"]),
    ("rst", &["rst", "rest"]),
    ("drawio", &["drawio", "dio"]),
    ("duckdb", &["duckdb", "ddb"]),
    // E-books calibre reads; a .pdb is one only by its header, so it is not here.
    ("ebook", &["mobi", "azw", "azw3", "fb2", "lit"]),
];

/// Endings that name a kind of their own (databases, books, certificates, mail, data).
const SPECIAL: &[(&str, &[&str])] = &[
    ("sqlite", &["db", "sqlite", "sqlite3", "db3", "dbf"]),
    ("epub", &["epub"]),
    ("cert", &["pem", "crt", "cer", "der"]),
    ("mail", &["eml", "msg"]),
    ("plist", &["plist"]),
    ("calendar", &["ics"]),
    ("contacts", &["vcf"]),
    ("jsonl", &["jsonl", "ndjson"]),
    ("data", &["json", "geojson", "yaml", "yml", "toml"]),
    ("log", &["log"]),
];

/// The rules, in the order they are asked.
pub fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(build)
}

fn build() -> Vec<Rule> {
    let media = crate::media::endings();
    let geo = crate::geo::endings();
    let vector: Vec<&'static str> = geo.get("vector").cloned().unwrap_or_default();
    let mut r = vec![
        // A Core ML package is a folder Netron draws.
        rule(Match::Dir { re: Some(r"\.mlpackage$") }, "netron"),
        rule(Match::Dir { re: None }, "folder"),
        // Neural network models Netron draws (`neural::named`); a `.h5` is one only by its bytes.
        name(r"\.(onnx|tflite|mlmodel|keras)$", "netron"),
        // A draw.io diagram, a picture with one in it too (`export::is_drawio`).
        name(r"\.(drawio|dio|drawio\.svg|drawio\.png)$", "drawio"),
        // Games (`chess::format`): Go on its own board, the rest on the games board.
        name(r"\.sgf$", "go"),
        name(r"\.(pgn(\.gz|\.zst)?|si[45]|cbh|cbv|epd|fen|pdn|kif|kifu|csa|pbn)$", "chess"),
        // In-toto attestations and CycloneDX BOMs by name (`provenance::sniff_name`, `bom::sniff_name`).
        name(r"(\.intoto\.jsonl?|\.sigstore(\.json)?|\.dsse\.json|\.provenance\.json|\.build\.slsa|^provenance\.json)$", "provenance"),
        name(r"(\.cdx\.(json|xml)|\.cbom\.json|^bom\.(json|xml))$", "bom"),
        // A comic is a zip of pictures, shown page by page.
        name(r"\.cbz$", "comic"),
        rule(Match::Suffix { suffixes: crate::archive::endings().0 }, "archive"),
        // A .vhd or .img that turns out to be none is shown as text (the preview).
        rule(Match::Suffix { suffixes: crate::disk_image::endings() }, "disk_image"),
        // A local model: its facts (`local_models::facts`); a pickle is named, never read.
        name(r"\.(gguf|safetensors|pt|pth|ckpt)$|^pytorch_model.*\.bin$", "model"),
        ending(&["pdf"], "pdf"),
    ];
    // Geodata: a map of its own shapes, a GPX file's drawn without GDAL.
    if vector.contains(&"gpx") {
        r.push(ending(&["gpx"], "gpx"));
    }
    r.push(rule(Match::Ending { endings: vector.into_iter().filter(|e| *e != "gpx").collect() }, "geo"));
    r.extend([
        ending(FONT, "font"),
        ending(SHEET, "sheet"),
        ending(&["docx"], "docx"),
        ending(&["ipynb"], "notebook"),
        ending(&["mmd", "mermaid"], "mermaid"),
        ending(&["html", "htm", "xhtml"], "html"),
        // A log by its name, rotated ones and /var/log's without an ending too (`logs::is_log_name`);
        // a plain .jsonl or .ndjson is JSON Lines.
        name(
            r"\.log(\.[0-9]+)?$|\.(jsonl|ndjson)\.[0-9]+$|^(syslog|messages|kern\.log|auth\.log|daemon\.log|secure|maillog|access_log|error_log|dmesg)(\.[0-9]+)?$",
            "log",
        ),
    ]);
    r.extend(SPECIAL.iter().map(|(k, e)| ending(e, k)));
    r.extend(CONVERTED.iter().map(|(k, e)| ending(e, k)));
    for k in ["video_other", "audio_other", "heif", "magick", "raw"] {
        r.push(rule(Match::Ending { endings: media.get(k).cloned().unwrap_or_default() }, k));
    }
    r.extend([
        ending(IMAGE, "image"),
        // A .hdr or .mod that is none (another program's header, Modula-2) is shown as text.
        rule(Match::Ending { endings: crate::pictures::endings() }, "picture"),
        ending(TRACKER, "tracker"),
        ending(VIDEO, "video"),
        ending(AUDIO, "audio"),
        ending(&["md", "markdown"], "markdown"),
    ]);
    r
}

/// The kind a file or folder named `name` is shown as; `text` when no rule fits.
pub fn kind_of(name: &str, is_dir: bool) -> &'static str {
    static COMPILED: OnceLock<Vec<Option<regex::Regex>>> = OnceLock::new();
    let compiled = COMPILED.get_or_init(|| {
        let re = |s: &str| Some(regex::Regex::new(s).expect("a viewer rule's pattern"));
        rules()
            .iter()
            .map(|r| match &r.when {
                Match::Dir { re: Some(s) } | Match::Name { re: s } => re(s),
                _ => None,
            })
            .collect()
    });
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    for (r, re) in rules().iter().zip(compiled) {
        let fits = match &r.when {
            Match::Dir { .. } if !is_dir => false,
            Match::Dir { re: None } => true,
            Match::Dir { .. } => re.as_ref().is_some_and(|re| re.is_match(&lower)),
            _ if is_dir => false,
            Match::Name { .. } => re.as_ref().is_some_and(|re| re.is_match(&lower)),
            Match::Ending { endings } => endings.contains(&ext),
            Match::Suffix { suffixes } => suffixes.iter().any(|s| lower.ends_with(s)),
        };
        if fits {
            return r.kind;
        }
    }
    "text"
}

/// Names the desktop app's evaluation of the table is checked against, each with its kind.
#[cfg(test)]
const CASES: &[(&str, bool)] = &[
    ("rocket", true),
    ("model.mlpackage", true),
    ("weights.onnx", true),
    ("weights.onnx", false),
    ("net.TFLite", false),
    ("flow.drawio", false),
    ("flow.drawio.svg", false),
    ("flow.drawio.png", false),
    ("game.sgf", false),
    ("club.pgn", false),
    ("club.pgn.gz", false),
    ("club.pgn.zst", false),
    ("club.si4", false),
    ("Classics.cbh", false),
    ("twic.cbv", false),
    ("mates.epd", false),
    ("start.fen", false),
    ("draughts.pdn", false),
    ("shogi.kifu", false),
    ("shogi.csa", false),
    ("bridge.pbn", false),
    ("build.intoto.jsonl", false),
    ("bundle.sigstore.json", false),
    ("provenance.json", false),
    ("app.cdx.json", false),
    ("bom.xml", false),
    ("BOM.json", false),
    ("comic.cbz", false),
    ("all.zip", false),
    ("all.tar.gz", false),
    ("all.7z.001", false),
    ("disk.iso", false),
    ("disk.vhd", false),
    ("llama.gguf", false),
    ("pytorch_model-00001.bin", false),
    ("openings.bin", false),
    ("paper.pdf", false),
    ("track.gpx", false),
    ("roads.shp", false),
    ("map.geojson", false),
    ("map.kml", false),
    ("font.woff2", false),
    ("sheet.xlsx", false),
    ("table.csv", false),
    ("letter.docx", false),
    ("analysis.ipynb", false),
    ("chart.mmd", false),
    ("page.html", false),
    ("app.log", false),
    ("app.log.1", false),
    ("events.jsonl", false),
    ("events.jsonl.2", false),
    ("events.ndjson", false),
    ("syslog", false),
    ("syslog.3", false),
    ("messages", false),
    (".log", false),
    ("store.sqlite", false),
    ("table.dbf", false),
    ("book.epub", false),
    ("site.pem", false),
    ("note.eml", false),
    ("Info.plist", false),
    ("dates.ics", false),
    ("people.vcf", false),
    ("config.json", false),
    ("ci.yml", false),
    ("Cargo.toml", false),
    ("graph.dot", false),
    ("guide.adoc", false),
    ("part.scad", false),
    ("board.kicad_pcb", false),
    ("data.parquet", false),
    ("paper.tex", false),
    ("notes.typ", false),
    ("figure.eps", false),
    ("old.doc", false),
    ("slides.pptx", false),
    ("doc.xps", false),
    ("seq.puml", false),
    ("arch.d2", false),
    ("tree.bob", false),
    ("spec.vl", false),
    ("schema.dbml", false),
    ("readme.rst", false),
    ("ducks.duckdb", false),
    ("novel.mobi", false),
    ("clip.avi", false),
    ("song.wma", false),
    ("photo.heic", false),
    ("art.xcf", false),
    ("shot.cr2", false),
    ("logo.png", false),
    ("logo.SVG", false),
    ("scan.tif", false),
    ("art.psd", false),
    ("tune.xm", false),
    ("tune.mod", false),
    ("film.mp4", false),
    ("song.flac", false),
    ("README.md", false),
    ("notes.txt", false),
    ("Makefile", false),
    ("main.rs", false),
    ("archive.tar", false),
    // As bom::sniff_name and provenance::sniff_name take them, and leave them.
    ("App.CDX.XML", false),
    ("x.cbom.json", false),
    ("x.cbom.xml", false),
    ("package.json", false),
    ("sbom.txt", false),
    ("X.INTOTO.JSON", false),
    ("a.sigstore", false),
    ("a.dsse.json", false),
    ("rocket.provenance.json", false),
    ("b.build.slsa", false),
    ("intoto.txt", false),
    ("provenance.json.bak", false),
    // Case folds only as lower case does, alike in both apps.
    ("GAME.SGF", false),
    ("x.\u{17f}gf", false),
    ("Model.MLPackage", true),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The table and its cases as the desktop app's test reads them: `gui/src/viewer-kinds.json`,
    /// written anew with `COXSWAIN_WRITE_VARIANTS=1`, and failing here while it is out of date.
    #[test]
    fn the_desktop_apps_copy_of_the_table_is_current() {
        let cases: Vec<serde_json::Value> = CASES.iter().map(|(n, d)| serde_json::json!({ "name": n, "is_dir": d, "kind": kind_of(n, *d) })).collect();
        let want = serde_json::to_string_pretty(&serde_json::json!({ "rules": rules(), "cases": cases })).unwrap() + "\n";
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gui/src/viewer-kinds.json");
        if std::env::var_os("COXSWAIN_WRITE_VARIANTS").is_some() {
            std::fs::write(&file, &want).unwrap();
        }
        let have = std::fs::read_to_string(&file).unwrap_or_default().replace("\r\n", "\n");
        assert!(have == want, "gui/src/viewer-kinds.json is out of date: run COXSWAIN_WRITE_VARIANTS=1 cargo test -p coxswain-core viewers");
    }

    #[test]
    fn every_pattern_compiles_and_names_a_kind() {
        for r in rules() {
            if let Match::Dir { re: Some(s) } | Match::Name { re: s } = &r.when {
                assert!(regex::Regex::new(s).is_ok(), "{s}");
                assert_eq!(*s, s.to_lowercase(), "patterns are matched against the lower-case name");
            }
            assert!(!r.kind.is_empty());
        }
        assert_eq!(kind_of("Makefile", false), "text");
        assert_eq!(kind_of("x.onnx", true), "folder", "a folder named like a model is a folder");
    }
}

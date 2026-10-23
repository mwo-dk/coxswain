//! Preview tools: which program shows each kind of file, in one table. A kind (LaTeX, Office
//! documents, Diagrams …) has its tools, the ids the previews run (`latex`, `d2`, `waveform`);
//! each tool has the installed programs that can do it, best first, the line that installs one
//! (`tools::install_line`), and its image under `[preview.images]`, whose default, size and cost
//! are here too. Settings → Previews → Preview tools in both apps, the desktop app's previews,
//! the conversions, the defaults of `[preview.images]` and the table in `docs/previews/tools.md`
//! all come from this table.
//!
//! The user chooses per kind under `[preview.tools]`: `local` (an installed program), `container`
//! or `off`; a kind without a choice follows `[preview] prefer`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::config::PreviewConfig;
use crate::t;

/// One thing a preview runs: `id` is the name the previews and `[preview.images]` know.
pub struct Tool {
    pub id: &'static str,
    /// Installed programs that do it, best first.
    pub programs: &'static [&'static str],
    /// The name `tools::install_line` knows its program by; "" when Coxswain has no line.
    pub install: &'static str,
    /// Its image under `[preview.images]`; Kroki's diagram kinds share `kroki`. "" for none.
    pub image: &'static str,
}

/// A kind of file in Settings: `ptools.kind.<id>` names it, `[preview.tools] <id>` is its choice.
pub struct Kind {
    pub id: &'static str,
    /// Its page under `docs/previews/`.
    pub docs: &'static str,
    /// The file endings it shows, in lists of the modules that read them.
    pub exts: &'static [&'static [&'static str]],
    pub tools: &'static [Tool],
    /// A small file of this kind that *Try* shows: its name and its text. None: no sample.
    pub sample: Option<(&'static str, &'static str)>,
}

/// An image under `[preview.images]`: its default, pinned by the digest of its multi-arch index
/// (`name:tag@sha256:…`, so a pull fetches exactly that build; `tools/images-pin.py` moves the
/// pins to a tag's newest build), and megabytes to download and on disk
/// (amd64, measured 2026-10). `image.what.<key>` and `image.cost.<key>` say what it gives and
/// costs. An empty default: no image is set, the user may set one they trust.
pub struct Image {
    pub key: &'static str,
    pub default: &'static str,
    pub download: u32,
    pub disk: u32,
}

const fn tool(id: &'static str, programs: &'static [&'static str], install: &'static str, image: &'static str) -> Tool {
    Tool { id, programs, install, image }
}

/// Every image Coxswain knows, its default and size.
pub const IMAGES: &[Image] = &[
    Image { key: "latex", default: "docker.io/texlive/texlive:latest@sha256:e8148b126545105c4aeeaa212f8a7bd02d05b500351372881055e1e59ae67d33", download: 2800, disk: 5900 },
    Image { key: "typst", default: "ghcr.io/typst/typst:latest@sha256:032e292249bcd378480cc7c142cfa324b63ef8aadeb88d7e7230320c4c9c422f", download: 29, disk: 65 },
    // Gotenberg's image, LibreOffice only: LibreOffice publishes none of its own.
    Image { key: "libreoffice", default: "docker.io/gotenberg/gotenberg:8-libreoffice@sha256:2098722190575dbd975d8abf02e4ce83ffe20783114c5cc56f1b2ac309e6a083", download: 440, disk: 1030 },
    // OCRmyPDF's own image, which has Ghostscript too: one pull for both.
    Image { key: "ghostscript", default: "docker.io/jbarlow83/ocrmypdf-alpine:latest@sha256:4a0b45ca35c1b2c43875b257d624f8bc8ad530ba4f75b52078e3baca17d66f7c", download: 310, disk: 850 },
    // GhostXPS: in none of the images, and no project publishes one.
    Image { key: "gxps", default: "", download: 0, disk: 0 },
    Image { key: "plantuml", default: "docker.io/plantuml/plantuml:latest@sha256:9b5071ab56a4192aac458ab4b0d42261c22c8109695894ae5be6013c3be6cac1", download: 141, disk: 353 },
    // The programs of a dozen diagram kinds.
    Image { key: "kroki", default: "docker.io/yuzutech/kroki:latest@sha256:a4341ac44cd2590124da7142d13fdf5d83eadc2b94c023d018a2227695a30da1", download: 1200, disk: 2830 },
    Image { key: "pandoc", default: "docker.io/pandoc/core:latest@sha256:62518bfb6833705dc90a1634d8d879299bd820b3f74ad3b32fba8b618452553a", download: 57, disk: 246 },
    // No official image; set one you trust (see docs/previews/containers.md).
    Image { key: "duckdb", default: "", download: 0, disk: 0 },
    // A static ffmpeg and ffprobe, rebuilt with each release (`media`).
    Image { key: "ffmpeg", default: "docker.io/mwader/static-ffmpeg:latest@sha256:ac3ffd0125b2a783c5d3e8e2bbb1fe2505708a3b558e6af216276aa853ade09a", download: 126, disk: 315 },
    // Coxswain's own ImageMagick 7 on Alpine, with HEIC, AVIF and RAW (packaging/images/imagemagick,
    // `media`).
    Image { key: "imagemagick", default: "ghcr.io/mwo-dk/coxswain-imagemagick:latest@sha256:bf3f7a4570d309c744105c119e1df883afd1f8bf6593ec6c24ff7704cde93483", download: 21, disk: 52 },
    // GDAL's small Alpine build, pinned to a release by its digest (`geo`).
    Image { key: "gdal", default: "ghcr.io/osgeo/gdal:alpine-small-3.13.3@sha256:58e21a794e3e49c2882a078cf39625aa8d1f86188e0273f74156e9f3f3cd0253", download: 40, disk: 98 },
    // OpenSCAD's own build of a dated snapshot, pinned by its digest (`scad`): its last release
    // (2021.01) has no Manifold, which renders in a second what took minutes.
    Image { key: "openscad", default: "docker.io/openscad/openscad:trixie.2026-10-19@sha256:bbed39fad63b66f26113d82e2b39ce1e14b2568378426fb2cdfeb41e34d7837b", download: 241, disk: 635 },
    // LinuxServer.io's calibre, pinned to the release tested (`ebook`); calibre publishes no
    // image of its own.
    Image { key: "calibre", default: crate::ebook::IMAGE, download: 1300, disk: 3640 },
    // KiCad's own image of its newest release (amd64 only), pinned by its digest (`kicad`).
    Image { key: "kicad", default: "docker.io/kicad/kicad:9.0.9@sha256:e638b79b0321f29395a5b783e94bb9f3c73303e8da15da27b8f5cb4b67a37729", download: 476, disk: 1710 },
    // whisper.cpp's own image, with ffmpeg, for Transcribe when no whisper-cli is installed
    // (`transcribe`), pinned by its digest.
    Image { key: "whisper", default: "ghcr.io/ggml-org/whisper.cpp:main@sha256:b3a5ce497d31f8eef2f8d6a123f834d4217150d2dd2a391f37a6881f725036af", download: 505, disk: 1140 },
    // Apache Tika's own server image, for `[search] tika`, pinned to a release by its digest.
    Image { key: "tika", default: "docker.io/apache/tika:4.1.0@sha256:06bcdbd09aca073293e5a171ad161418cc93c91d58d9e4e205064eade84889d4", download: 190, disk: 440 },
    Image { key: "ocrmypdf", default: "docker.io/jbarlow83/ocrmypdf-alpine:latest@sha256:4a0b45ca35c1b2c43875b257d624f8bc8ad530ba4f75b52078e3baca17d66f7c", download: 310, disk: 850 },
    // Coxswain's own Stockfish build (packaging/images/stockfish), for the chess engine when
    // none is installed (`chess::play`); its two evaluation networks are most of it.
    Image { key: "stockfish", default: "ghcr.io/mwo-dk/coxswain-stockfish:latest@sha256:2d042e01f4a3751b1495843fc61f121e42ef446b29fc0d776200774ea5a5acc4", download: 70, disk: 80 },
    // Asciidoctor's own image, with Asciidoctor PDF: *Build PDF of AsciiDoc* (`export`).
    Image { key: "asciidoctor", default: "docker.io/asciidoctor/docker-asciidoctor:latest@sha256:16d479c874b9646cd4fd20d1a954fb7897d23c3787c1ddec29cfbb23533a58d2", download: 665, disk: 1500 },
    // draw.io's desktop app with a display of its own (`export`); used only with `drawio` on.
    Image { key: "drawio", default: "docker.io/rlespinasse/drawio-desktop-headless:latest@sha256:b7468f89320a6b4c6d8099da224c837a03abab4832701bb4b2355e0901f805fd", download: 560, disk: 1310 },
    // ClamAV's own Debian image without a database (it is fetched apart, at scan time), for
    // x86-64, ARM64 and POWER; pinned, so a scan always runs the same one (`viruses`).
    Image { key: "clamav", default: "docker.io/clamav/clamav-debian:1.5_base@sha256:dfe9a0c9ea8d7f14034f742010c0f91e0d3f7a719a526c426c60d2e1f5c490eb", download: 80, disk: 240 },
];

/// Other images whose size is known: what a user may set instead of a default.
const OTHER_SIZES: &[(&str, u32, u32)] = &[("docker.io/texlive/texlive:latest-medium", 960, 2200)];

/// Every kind, in the order Settings and the docs list them.
pub const KINDS: &[Kind] = &[
    Kind {
        id: "latex",
        docs: "latex.md",
        exts: &[&["tex", "ltx"]],
        tools: &[tool("latex", &["latexmk", "tectonic", "pdflatex"], "latex", "latex")],
        sample: Some(("sample.tex", "\\documentclass{article}\n\\begin{document}\n\\section*{Coxswain}\nA LaTeX preview: $e^{i\\pi} + 1 = 0$.\n\\end{document}\n")),
    },
    Kind {
        id: "typst",
        docs: "documents.md#typst",
        exts: &[&["typ"]],
        tools: &[tool("typst", &["typst"], "typst", "typst")],
        sample: Some(("sample.typ", "= Coxswain\nA Typst preview: $e^(i pi) + 1 = 0$.\n")),
    },
    Kind {
        id: "office",
        docs: "office.md",
        exts: &[&[
            "doc", "docm", "dot", "dotx", "odt", "ott", "rtf", "wri", "ppt", "pptx", "pps", "ppsx", "pot", "potx", "odp", "otp", "odg", "vsd", "vsdx", "pub", "wpd", "wps", "wk1", "wks", "123", "lwp", "sxw", "sxc", "sxi", "sxd", "abw", "cwk", "pages", "key", "numbers", "fodt", "fods", "fodp", "fodg", "cdr", "pmd", "wmf", "emf",
        ]],
        tools: &[tool("libreoffice", &["soffice", "libreoffice"], "soffice", "libreoffice")],
        sample: Some(("sample.rtf", "{\\rtf1\\ansi{\\fonttbl\\f0\\fswiss Helvetica;}\\f0\\fs48 {\\b Coxswain}\\par\\fs24 An office preview, by LibreOffice.\\par}\n")),
    },
    Kind {
        id: "postscript",
        docs: "documents.md#postscript-and-xps",
        exts: &[&["ps", "eps", "xps", "oxps"]],
        tools: &[tool("ghostscript", &["gs", "gswin64c"], "gs", "ghostscript"), tool("gxps", &["gxps"], "", "gxps")],
        sample: Some(("sample.ps", "%!PS\n/Helvetica findfont 36 scalefont setfont\n72 700 moveto (Coxswain) show\n/Helvetica findfont 14 scalefont setfont\n72 670 moveto (A PostScript preview, by Ghostscript.) show\nshowpage\n")),
    },
    Kind {
        id: "diagrams",
        docs: "diagrams.md",
        exts: &[&[
            "puml", "plantuml", "pu", "iuml", "wsd", "d2", "ditaa", "bob", "svgbob", "pikchr", "nomnoml", "noml", "wavedrom", "vg", "vega", "vl", "vegalite", "dbml", "er", "erd", "bytefield",
        ]],
        tools: &[
            tool("plantuml", &["plantuml"], "plantuml", "plantuml"),
            // Kroki's image has them all; these four are packaged on their own too.
            tool("d2", &["d2"], "", "kroki"),
            tool("ditaa", &[], "", "kroki"),
            tool("svgbob", &["svgbob"], "", "kroki"),
            tool("pikchr", &["pikchr"], "", "kroki"),
            tool("nomnoml", &[], "", "kroki"),
            tool("wavedrom", &[], "", "kroki"),
            tool("vega", &[], "", "kroki"),
            tool("vegalite", &[], "", "kroki"),
            tool("dbml", &[], "", "kroki"),
            tool("erd", &["erd"], "", "kroki"),
            tool("bytefield", &[], "", "kroki"),
        ],
        sample: Some(("sample.puml", "@startuml\nYou -> Coxswain: show this diagram\nCoxswain --> You: drawn by PlantUML\n@enduml\n")),
    },
    Kind {
        id: "markup",
        docs: "documents.md",
        exts: &[&["rst", "rest"]],
        tools: &[tool("pandoc", &["pandoc"], "pandoc", "pandoc")],
        sample: Some(("sample.rst", "Coxswain\n========\n\nA *reStructuredText* preview, by **pandoc**.\n\n- one\n- two\n")),
    },
    Kind { id: "databases", docs: "data.md", exts: &[&["duckdb", "ddb"]], tools: &[tool("duckdb", &["duckdb"], "", "duckdb")], sample: None },
    Kind {
        id: "video",
        docs: "media.md#video-and-sound-with-ffmpeg",
        exts: &[crate::media::VIDEO_OTHER, crate::media::AUDIO_OTHER],
        // A video's frame, a song's waveform.
        tools: &[tool("ffmpeg", &["ffmpeg"], "ffmpeg", "ffmpeg"), tool("waveform", &["ffmpeg"], "ffmpeg", "ffmpeg")],
        sample: None,
    },
    Kind {
        id: "pictures",
        docs: "media.md#photos-only-imagemagick-or-ffmpeg-reads",
        exts: &[crate::media::HEIF, crate::media::MAGICK_ONLY, crate::media::RAW],
        // HEIC: either program installed; RAW: an installed ImageMagick with LibRaw; both, else
        // Coxswain's ImageMagick image, which has libheif and LibRaw.
        tools: &[
            tool("heif", &["magick", "ffmpeg"], "magick", "imagemagick"),
            tool("imagemagick", &["magick"], "magick", "imagemagick"),
            tool("raw", &["magick"], "magick", "imagemagick"),
        ],
        sample: None,
    },
    // KiCad (`kicad`): a schematic's sheets or a board's layers, and a board in 3D as its
    // thumbnail.
    Kind {
        id: "kicad",
        docs: "kicad.md",
        exts: &[&["kicad_sch", "kicad_pcb"]],
        tools: &[tool("kicad", &["kicad-cli"], "kicad-cli", "kicad"), tool("kicad_thumb", &["kicad-cli"], "kicad-cli", "kicad")],
        sample: None,
    },
    // E-books, once `[preview] ebooks` turns them on (`ebook`).
    Kind { id: "ebooks", docs: "documents.md", exts: &[crate::ebook::FORMATS], tools: &[tool("calibre", &["ebook-convert"], "calibre", "calibre")], sample: None },
    Kind {
        id: "maps",
        docs: "geodata.md",
        exts: &[&["gpkg", "shp", "geojson", "kml", "gpx"]],
        // GDAL's programs come together; gdalinfo stands for them.
        tools: &[tool("gdal", &["gdalinfo"], "gdalinfo", "gdal")],
        sample: Some(("sample.geojson", "{\"type\":\"FeatureCollection\",\"features\":[{\"type\":\"Feature\",\"properties\":{\"name\":\"Coxswain\"},\"geometry\":{\"type\":\"Polygon\",\"coordinates\":[[[12.5,55.6],[12.7,55.6],[12.7,55.7],[12.5,55.7],[12.5,55.6]]]}}]}\n")),
    },
    Kind {
        id: "models",
        docs: "openscad.md",
        exts: &[&["scad"]],
        tools: &[tool("openscad", &["openscad"], "openscad", "openscad")],
        sample: Some(("sample.scad", "difference() {\n  cube(20, center = true);\n  sphere(13);\n}\n")),
    },
    // Search's reader of other formats (`[search] tika`): listed for its image, which Pull and
    // Remove keep, and what it reads; it makes no preview, so it has no choice here.
    Kind { id: "other", docs: "../search/tika.md", exts: &[crate::extract::tika::FORMATS], tools: &[tool("tika", &[], "", "tika")], sample: None },
    // Transcripts of speech, on request (`transcribe`): no preview of its own.
    Kind { id: "transcripts", docs: "../search/transcripts.md", exts: &[crate::transcribe::FORMATS], tools: &[tool("whisper", &["whisper-cli"], "", "whisper")], sample: None },
    // The chess view's engine (`chess::play`): the files it is asked about; it makes no
    // preview, so `for_path` leaves it out.
    Kind { id: "chess_engine", docs: "chess-play.md", exts: &[&["pgn", "epd", "fen"]], tools: &[tool("stockfish", &["stockfish"], "stockfish", "stockfish")], sample: None },
    Kind { id: "ocr", docs: "../files/searchable-pdf.md", exts: &[&["pdf"]], tools: &[tool("ocrmypdf", &["ocrmypdf"], "ocrmypdf", "ocrmypdf")], sample: None },
    // Build PDF of AsciiDoc (`export`): the preview itself is built in.
    Kind { id: "asciidoc", docs: "../files/asciidoc-pdf.md", exts: &[&["adoc", "asciidoc"]], tools: &[tool("asciidoctor", &["asciidoctor-pdf"], "asciidoctor-pdf", "asciidoctor")], sample: None },
    // Export draw.io diagram and its thumbnails (`export`), only in draw.io's container, with
    // `[preview] drawio` on: no choice, no installed program.
    Kind { id: "drawio", docs: "../files/drawio-export.md", exts: &[&["drawio", "dio"]], tools: &[tool("drawio", &[], "", "drawio")], sample: None },
    // Scan for viruses (`viruses`): any file, only in ClamAV's container, with `[preview]
    // virus_scan` on: no choice, no installed program.
    Kind { id: "viruses", docs: "../files/virus-scan.md", exts: &[], tools: &[tool("clamav", &[], "", "clamav")], sample: None },
];

/// The choices of `[preview.tools]`; no entry follows `[preview] prefer`.
pub const CHOICES: [&str; 3] = ["local", "container", "off"];

/// The tool called `id`.
pub fn find(id: &str) -> Option<&'static Tool> {
    KINDS.iter().flat_map(|k| k.tools).find(|t| t.id == id)
}

/// The kind `tool` (a tool's id, or a kind's own) belongs to.
pub fn kind_of(tool: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == tool || k.tools.iter().any(|t| t.id == tool))
}

/// The kind a file of this ending is shown by. PDF is no preview of a tool's: Make PDF
/// searchable is asked for.
pub fn for_path(path: &Path) -> Option<&'static Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    KINDS.iter().filter(|k| !matches!(k.id, "ocr" | "other" | "transcripts" | "chess_engine" | "asciidoc" | "drawio" | "viruses")).find(|k| k.exts.iter().any(|e| e.contains(&ext.as_str())))
}

/// The image of `tool` under `[preview.images]`: Kroki's diagram kinds share `kroki`, a
/// video's frame, a song's waveform and HEIC photos ffmpeg's. A name the table does not know
/// is its own.
pub fn image_key(tool: &str) -> &str {
    find(tool).map_or(tool, |t| if t.image.is_empty() { t.id } else { t.image })
}

/// The installed programs that do `tool`, best first.
pub fn programs(tool: &str) -> &'static [&'static str] {
    find(tool).map_or(&[], |t| t.programs)
}

/// How `tool` is made: `[preview.tools]` of its kind ("local", "container", "off"), else
/// `[preview] prefer` ("auto", "local", "container").
pub fn choice<'a>(cfg: &'a PreviewConfig, tool: &str) -> &'a str {
    kind_of(tool).and_then(|k| cfg.tools.get(k.id)).filter(|c| CHOICES.contains(&c.as_str())).unwrap_or(&cfg.prefer)
}

/// The kind of `tool` is *Off*: nothing runs for it unless the user asks (a conversion) — no
/// preview, thumbnail or facts.
pub fn off(cfg: &PreviewConfig, tool: &str) -> bool {
    choice(cfg, tool) == "off"
}

/// `choice` for what the user asked for (a conversion, Make PDF searchable): "off" turns off
/// only what runs by itself (`off`), so it counts as `prefer` here. Never for a preview, a
/// thumbnail or facts: those check `off` first.
pub fn asked(cfg: &PreviewConfig, tool: &str) -> String {
    match choice(cfg, tool) {
        "off" => cfg.prefer.clone(),
        c => c.to_string(),
    }
}

/// Set the choice of `kind` in the config.toml at `path`; "" removes it, so the kind follows
/// `prefer` again.
pub fn set_choice(path: &Path, kind: &str, value: &str) -> Result<crate::config::Config, String> {
    if kind_of(kind).is_none_or(|k| k.id != kind) || !can_choose(kind) || !(value.is_empty() || CHOICES.contains(&value)) {
        return Err(t!("err.unknown_setting", "name" => format!("preview.tools.{kind}")));
    }
    crate::config::update_config(path, |text| {
        let at = ["preview", "tools", kind];
        let new = if value.is_empty() { crate::config::Config::unset(text, &at)? } else { crate::config::Config::edit(text, &at, value.into())? };
        let cfg = crate::config::Config::parse(&new)?;
        Ok((Some(new), cfg))
    })
}

/// Whether `[preview.tools]` has a choice for the kind: not Tika's, which `[search] tika` turns on,
/// nor draw.io's or ClamAV's, which run only in their containers.
pub fn can_choose(kind: &str) -> bool {
    !matches!(kind, "other" | "drawio" | "viruses")
}

/// The image `key`'s entry.
pub fn image(key: &str) -> Option<&'static Image> {
    IMAGES.iter().find(|i| i.key == key)
}

/// The defaults of `[preview.images]`.
pub fn default_images() -> std::collections::BTreeMap<String, String> {
    IMAGES.iter().map(|i| (i.key.to_string(), i.default.to_string())).collect()
}

/// Megabytes to download and on disk of `image`, when it is one Coxswain knows.
pub fn sizes(image: &str) -> Option<(u32, u32)> {
    let full = if image.starts_with(&format!("{}/", crate::settings::registry(image))) { image.to_string() } else { format!("docker.io/{image}") };
    IMAGES.iter().filter(|i| !i.default.is_empty()).map(|i| (i.default, i.download, i.disk)).chain(OTHER_SIZES.iter().copied()).find(|s| crate::images::short(s.0) == crate::images::short(&full)).map(|s| (s.1, s.2))
}

// ---------------------------------------------------------------- what is here

/// An installed program of a kind: where it is and the version it says.
#[derive(Clone, Debug, Serialize)]
pub struct Program {
    pub name: &'static str,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

/// An image of a kind, as the runtime has it.
#[derive(Clone, Debug, Serialize)]
pub struct ImageState {
    pub key: &'static str,
    /// As `[preview.images]` has it; empty when none is set.
    pub image: String,
    /// Bytes on disk, when pulled.
    pub pulled: Option<u64>,
    /// "126 MB", when the size is known.
    pub download: Option<String>,
    /// What it gives, its size and cost (`images::about`).
    pub about: Option<String>,
}

/// A row of Settings → Previews → Preview tools.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub kind: &'static str,
    /// The ids of its tools, as the previews name them.
    pub tools: Vec<&'static str>,
    pub title: String,
    /// ".tex .ltx"
    pub exts: String,
    pub programs: Vec<Program>,
    /// The line that installs its program here.
    pub install: Option<String>,
    pub images: Vec<ImageState>,
    /// `[preview.tools]`'s, "" when it follows `prefer`.
    pub choice: String,
    /// What makes its previews now, in words: "latexmk 4.86", "podman, texlive:latest", "Off".
    pub uses: String,
    /// Nothing can make them now.
    pub missing: bool,
    /// Has a sample for *Try*.
    pub sample: bool,
    /// `[preview.tools]` has a choice for it (`can_choose`).
    pub can_choose: bool,
}

/// The version a program says, first line, cached for the session: `--version` asked once per
/// program, five seconds at most.
fn version(name: &str, path: &Path) -> Option<String> {
    static SAID: Mutex<Option<HashMap<PathBuf, Option<String>>>> = Mutex::new(None);
    if let Some(v) = SAID.lock().unwrap().get_or_insert_with(HashMap::new).get(path) {
        return v.clone();
    }
    let flag = match name {
        "ffmpeg" | "plantuml" => "-version",
        _ => "--version",
    };
    let mut c = crate::tools::low(path);
    c.arg(flag);
    let said = crate::tools::capture(c, Duration::from_secs(5), 64 << 10).ok().and_then(|o| {
        let text = String::from_utf8_lossy(if o.stdout.is_empty() { &o.stderr } else { &o.stdout }).into_owned();
        version_in(&text)
    });
    SAID.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.to_path_buf(), said.clone());
    said
}

/// The first word that looks like a version (`4.86`, `v0.13.1`, `24.8.4.2`) in `text`.
fn version_in(text: &str) -> Option<String> {
    text.lines().take(5).flat_map(|l| l.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')).find_map(|w| {
        let w = w.trim_start_matches(['v', 'V']).trim_end_matches(['.', ':']);
        (w.starts_with(|c: char| c.is_ascii_digit()) && w.contains('.')).then(|| w.to_string())
    })
}

/// Every kind as it stands here: what is installed (with versions, asked in parallel), the
/// images and whether the runtime has them, the choice, and what is used. Asks every program
/// and the runtime: run it off the screen's thread.
pub fn rows(cfg: &PreviewConfig) -> Vec<Row> {
    rows_of(cfg, KINDS)
}

/// The row of the kind `tool` belongs to (a tool's id or a kind's), for *Preview with…*.
pub fn row_of(cfg: &PreviewConfig, tool: &str) -> Option<Row> {
    let k = kind_of(tool)?;
    rows_of(cfg, std::slice::from_ref(k)).pop()
}

fn rows_of(cfg: &PreviewConfig, kinds: &'static [Kind]) -> Vec<Row> {
    let runtime = crate::tools::container_runtime(cfg);
    let mut names: Vec<&'static str> = kinds.iter().flat_map(|k| k.tools).flat_map(|t| t.programs.iter().copied()).collect();
    names.sort();
    names.dedup();
    let found: HashMap<&str, Program> = std::thread::scope(|s| {
        let jobs: Vec<_> = names
            .iter()
            .map(|&name| {
                s.spawn(move || {
                    let path = crate::tools::which(name);
                    let version = path.as_deref().and_then(|p| version(name, p));
                    (name, Program { name, path, version })
                })
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    kinds.iter().map(|k| row(cfg, k, &found, runtime.as_ref())).collect()
}

fn row(cfg: &PreviewConfig, k: &'static Kind, found: &HashMap<&str, Program>, runtime: Option<&(&'static str, PathBuf)>) -> Row {
    let mut programs: Vec<Program> = vec![];
    for name in k.tools.iter().flat_map(|t| t.programs.iter()) {
        if !programs.iter().any(|p| p.name == *name) {
            programs.push(found.get(name).cloned().unwrap_or(Program { name, path: None, version: None }));
        }
    }
    let install = k.tools.iter().map(|t| t.install).find(|i| !i.is_empty()).and_then(crate::tools::install);
    let mut images: Vec<ImageState> = vec![];
    for key in k.tools.iter().map(|t| t.image).filter(|i| !i.is_empty()) {
        if images.iter().any(|i| i.key == key) {
            continue;
        }
        let image = cfg.images.get(key).cloned().unwrap_or_default();
        let pulled = runtime.filter(|_| !image.is_empty()).and_then(|(_, rt)| crate::tools::image_size(rt, &image));
        images.push(ImageState { key, download: crate::images::download(&image), about: crate::images::about(key, &image), pulled, image });
    }
    let choice = cfg.tools.get(k.id).filter(|c| CHOICES.contains(&c.as_str())).cloned().unwrap_or_default();
    let how = if choice.is_empty() { cfg.prefer.as_str() } else { choice.as_str() };
    let local = programs.iter().find(|p| p.path.is_some()).map(|p| p.version.as_ref().map_or_else(|| p.name.to_string(), |v| format!("{} {v}", p.name)));
    let container = runtime.and_then(|(rt, _)| images.iter().find(|i| !i.image.is_empty()).map(|i| (rt, i)));
    let container = container.map(|(rt, i)| {
        let image = crate::images::short(&i.image).rsplit('/').next().unwrap_or_default().to_string();
        if i.pulled.is_some() { t!("ptools.uses_container", "runtime" => *rt, "image" => image) } else { t!("ptools.uses_pull", "runtime" => *rt, "image" => image) }
    });
    let uses = match how {
        "off" => Some(t!("ptools.choice.off")),
        "local" => local,
        "container" => container.or(local),
        _ => local.or(container),
    };
    Row {
        kind: k.id,
        tools: k.tools.iter().map(|t| t.id).collect(),
        title: t!(&format!("ptools.kind.{}", k.id)),
        exts: k.exts.iter().copied().flatten().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" "),
        programs,
        install,
        images,
        choice,
        missing: uses.is_none(),
        uses: uses.unwrap_or_else(|| t!("ptools.uses_nothing")),
        sample: k.sample.is_some(),
        can_choose: can_choose(k.id),
    }
}

/// *Try*: the sample of `kind` written into the cache (`coxswain/samples`), for the preview to
/// show; its path.
pub fn sample(kind: &str) -> Result<PathBuf, String> {
    let (name, text) = kind_of(kind).and_then(|k| k.sample).ok_or_else(|| t!("ptools.no_sample"))?;
    let dir = crate::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("samples");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---------------------------------------------------------------- the docs

/// The table of `docs/previews/tools.md` (between `<!-- tools-table -->` and
/// `<!-- /tools-table -->`), from this one: a test fails when they differ.
pub fn docs_table() -> String {
    let mut s = String::from("| Kind (`[preview.tools]`) | Files | Installed programs it looks for | Container image (default) |\n|---|---|---|---|\n");
    for k in KINDS {
        let mut programs: Vec<String> = vec![];
        for p in k.tools.iter().flat_map(|t| t.programs) {
            let p = format!("`{p}`");
            if !programs.contains(&p) {
                programs.push(p);
            }
        }
        let mut images: Vec<String> = vec![];
        for key in k.tools.iter().map(|t| t.image).filter(|i| !i.is_empty()) {
            let Some(i) = image(key) else { continue };
            let line = if i.default.is_empty() {
                format!("`{key}`: none; set one you trust")
            } else {
                format!("`{key}`: `{}` (about {} to download, {} on disk)", crate::images::short(i.default), en(i.download), en(i.disk))
            };
            if !images.contains(&line) {
                images.push(line);
            }
        }
        let files = k.exts.iter().copied().flatten().map(|e| format!("`.{e}`")).collect::<Vec<_>>().join(" ");
        let files = if files.is_empty() { "any".to_string() } else { files };
        let title = crate::i18n::tr_in("en-GB", &format!("ptools.kind.{}", k.id));
        s.push_str(&format!(
            "| [{title}]({}) (`{}`) | {files} | {} | {} |\n",
            k.docs,
            k.id,
            if programs.is_empty() { "none".into() } else { programs.join(", ") },
            if images.is_empty() { "none".into() } else { images.join("<br>") }
        ));
    }
    s
}

/// The table of install lines in `docs/previews/tools.md` (between `<!-- install-table -->`
/// and `<!-- /install-table -->`): each program's line for every package manager.
pub fn docs_install_table() -> String {
    use crate::tools::Manager::*;
    let managers = [
        ("FreeBSD (`pkg`)", Pkg),
        ("NetBSD (`pkgin`)", Pkgin),
        ("OpenBSD (`pkg_add`)", PkgAdd),
        ("illumos (`pkg`)", Ips),
        ("Arch (`pacman`)", Pacman),
        ("Debian, Ubuntu (`apt`)", Apt),
        ("Fedora (`dnf`)", Dnf),
        ("openSUSE (`zypper`)", Zypper),
        ("macOS (Homebrew)", Brew),
        ("Windows (`winget`)", Winget),
    ];
    let mut programs: Vec<&str> = vec![];
    for t in KINDS.iter().flat_map(|k| k.tools) {
        if !t.install.is_empty() && !programs.contains(&t.install) {
            programs.push(t.install);
        }
    }
    let mut s = format!("| System | {} |\n|---|{}\n", programs.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(" | "), "---|".repeat(programs.len()));
    for (name, m) in managers {
        let cells: Vec<String> = programs.iter().map(|p| crate::tools::install_line(p, m).map_or_else(|| "none".into(), |l| format!("`{l}`"))).collect();
        s.push_str(&format!("| {name} | {} |\n", cells.join(" | ")));
    }
    s
}

/// Megabytes in the docs' English: "2.8 GB", "141 MB".
fn en(mb: u32) -> String {
    if mb >= 1000 { format!("{}.{} GB", mb / 1000, mb % 1000 / 100) } else { format!("{mb} MB") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptools_every_tool_is_in_one_kind_and_every_image_is_known() {
        let mut ids: Vec<&str> = KINDS.iter().flat_map(|k| k.tools).map(|t| t.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "a tool in two kinds");
        for t in KINDS.iter().flat_map(|k| k.tools) {
            assert!(t.image.is_empty() || image(t.image).is_some(), "{}: image {} not in IMAGES", t.id, t.image);
            assert!(t.install.is_empty() || crate::tools::install_line(t.install, crate::tools::Manager::Pacman).is_some() || crate::tools::install_line(t.install, crate::tools::Manager::Brew).is_some(), "{}", t.id);
        }
        assert_eq!(image_key("d2"), "kroki");
        assert_eq!((image_key("waveform"), image_key("heif"), image_key("raw")), ("ffmpeg", "imagemagick", "imagemagick"));
        assert_eq!(image_key("custom"), "custom");
        assert_eq!(kind_of("waveform").unwrap().id, "video");
        assert_eq!(for_path(Path::new("/x/A.TEX")).unwrap().id, "latex");
        assert!(for_path(Path::new("/x/a.pdf")).is_none());
        assert_eq!(default_images()["ghostscript"], default_images()["ocrmypdf"], "one pull serves both");
    }

    #[test]
    fn ptools_a_kinds_choice_wins_and_off_only_stops_previews() {
        let mut cfg = PreviewConfig::default();
        assert_eq!(choice(&cfg, "d2"), "auto");
        cfg.tools.insert("diagrams".into(), "container".into());
        assert_eq!(choice(&cfg, "d2"), "container");
        assert_eq!(choice(&cfg, "plantuml"), "container");
        cfg.tools.insert("video".into(), "off".into());
        assert_eq!(choice(&cfg, "waveform"), "off");
        assert_eq!(asked(&cfg, "ffmpeg"), "auto", "Convert video still runs");
        cfg.tools.insert("latex".into(), "nonsense".into());
        assert_eq!(choice(&cfg, "latex"), "auto");
    }

    /// *Off* runs nothing by itself: no facts by ffprobe or GDAL, whatever is installed; what
    /// the user asks for still finds its program.
    #[test]
    fn ptools_off_runs_nothing_by_itself() {
        let dir = std::env::temp_dir().join(format!("coxswain-ptools-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (video, map) = (dir.join("a.mkv"), dir.join("a.geojson"));
        std::fs::write(&video, b"not a video").unwrap();
        std::fs::write(&map, b"{}").unwrap();
        let mut cfg = PreviewConfig::default();
        cfg.tools.insert("video".into(), "off".into());
        cfg.tools.insert("maps".into(), "off".into());
        assert!(off(&cfg, "ffmpeg") && off(&cfg, "waveform") && off(&cfg, "gdal"));
        assert!(crate::media::facts(&cfg, &video).is_empty());
        assert!(crate::geo::facts(&cfg, &map).is_empty());
        assert_eq!(asked(&cfg, "ffmpeg"), "auto", "a conversion still runs");
        assert!(!off(&cfg, "latex"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ptools_set_choice_writes_and_removes_the_kind() {
        let dir = std::env::temp_dir().join(format!("coxswain-ptools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "# mine\n[preview]\nprefer = \"local\"\n").unwrap();
        let cfg = set_choice(&path, "latex", "container").unwrap();
        assert_eq!(cfg.preview.tools["latex"], "container");
        assert!(std::fs::read_to_string(&path).unwrap().contains("# mine"));
        let cfg = set_choice(&path, "latex", "").unwrap();
        assert!(cfg.preview.tools.is_empty());
        assert!(set_choice(&path, "d2", "local").is_err(), "a kind, not a tool");
        assert!(set_choice(&path, "latex", "auto").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ptools_versions_are_read_from_what_programs_say() {
        assert_eq!(version_in("Latexmk, John Collins, 7 Apr. 2024. Version 4.85").as_deref(), Some("4.85"));
        assert_eq!(version_in("ffmpeg version 7.1.1 Copyright (c) 2000-2025").as_deref(), Some("7.1.1"));
        assert_eq!(version_in("typst 0.13.1 (8ace67d9)").as_deref(), Some("0.13.1"));
        assert_eq!(version_in("v0.6.9").as_deref(), Some("0.6.9"));
        assert_eq!(version_in("OpenSCAD version 2021.01").as_deref(), Some("2021.01"));
        assert_eq!(version_in("no version here"), None);
    }

    #[test]
    fn ptools_rows_say_what_is_used() {
        crate::i18n::set_language("en-GB");
        let mut cfg = PreviewConfig { container: "off".into(), ..Default::default() };
        cfg.tools.insert("models".into(), "off".into());
        let rows = rows(&cfg);
        assert_eq!(rows.len(), KINDS.len());
        let models = rows.iter().find(|r| r.kind == "models").unwrap();
        assert_eq!((models.uses.as_str(), models.missing), ("Off", false));
        let diagrams = rows.iter().find(|r| r.kind == "diagrams").unwrap();
        assert_eq!(diagrams.images.iter().map(|i| i.key).collect::<Vec<_>>(), ["plantuml", "kroki"]);
        for r in &rows {
            assert!(!r.title.starts_with("ptools."), "{}: no title", r.kind);
        }
    }
}

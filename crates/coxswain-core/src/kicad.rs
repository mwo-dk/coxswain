//! KiCad, in both apps: the desktop app's preview of a schematic (every sheet, a page each) and
//! of a board (its outer layers, a page each, with the board's outline on every page), their
//! thumbnails (a schematic's first sheet, a board's 3D view from above), and *Export from
//! KiCad*: PDF, SVG, a bill of materials as CSV, Gerber and drill files, next to the original.
//!
//! `kicad-cli` runs installed first, else from KiCad's container image (`[preview.images]
//! kicad`), through `media`'s pipeline, which gives it the file's folder (a schematic's sheets
//! and a board's project are files beside it). Results made of several files (a schematic's
//! sheets as SVG, Gerbers) are zipped into one, so a result is always one new file.

use std::path::Path;

use crate::media::Run;
use crate::t;

/// The name under `[preview.images]` and `prefer_tool`.
pub const KICAD: &str = "kicad";

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// A KiCad schematic by its name.
pub fn is_schematic(path: &Path) -> bool {
    ending(path) == "kicad_sch"
}

/// A KiCad board by its name.
pub fn is_board(path: &Path) -> bool {
    ending(path) == "kicad_pcb"
}

pub fn is_kicad(path: &Path) -> bool {
    is_schematic(path) || is_board(path)
}

/// A board's layers the preview and the PDF show, a page each, and those of its SVG.
const LAYERS: &str = "F.Cu,B.Cu,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask";
const OUTLINE: &str = "Edge.Cuts";
const SVG_LAYERS: &str = "F.Cu,B.Cu,F.Silkscreen,Edge.Cuts";

/// What *Export from KiCad* makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    /// A schematic's sheets, or a board's layers, a page each.
    Pdf,
    /// A board's front, back and outline in one SVG; a schematic's sheets, an SVG each, zipped.
    Svg,
    /// A schematic's bill of materials: reference, value, footprint, how many.
    Bom,
    /// A board's Gerber and drill files, zipped, for the board maker.
    Gerber,
}

impl To {
    pub const ALL: [To; 4] = [To::Pdf, To::Svg, To::Bom, To::Gerber];

    pub fn id(self) -> &'static str {
        match self {
            To::Pdf => "pdf",
            To::Svg => "svg",
            To::Bom => "bom",
            To::Gerber => "gerber",
        }
    }

    pub fn parse(s: &str) -> Option<To> {
        To::ALL.into_iter().find(|t| t.id() == s)
    }

    pub fn takes(self, path: &Path) -> bool {
        match self {
            To::Pdf | To::Svg => is_kicad(path),
            To::Bom => is_schematic(path),
            To::Gerber => is_board(path),
        }
    }

    /// The ending of what it makes of `path`.
    pub(crate) fn result_ending(self, path: &Path) -> &'static str {
        match self {
            To::Pdf => "pdf",
            To::Svg if is_schematic(path) => "svg.zip",
            To::Svg => "svg",
            To::Bom => "bom.csv",
            To::Gerber => "gerbers.zip",
        }
    }
}

/// The preview: a schematic's sheets or a board's layers as one PDF, into `name`.
pub(crate) fn preview(run: &mut Run, path: &Path, name: &str) -> Result<(), String> {
    export(run, path, To::Pdf, name)
}

/// The thumbnail (SVG): a schematic's first sheet, or a board seen in 3D from above.
pub(crate) fn thumbnail(run: &mut Run, path: &Path) -> Result<String, String> {
    let input = run.input.clone();
    if is_schematic(path) {
        let dir = run.out("sheets");
        std::fs::create_dir_all(run.scratch.join("sheets")).map_err(|e| e.to_string())?;
        run.go("kicad-cli", &["sch", "export", "svg", "-o", &dir, &input])?;
        // The root sheet is named after the schematic; the others after it and their sheet.
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let root = run.scratch.join("sheets").join(format!("{stem}.svg"));
        let first = if root.is_file() { Some(root) } else { files(&run.scratch.join("sheets")).into_iter().next() };
        let first = first.ok_or_else(|| t!("convert.no_result"))?;
        return std::fs::read_to_string(first).map_err(|e| e.to_string());
    }
    let png = run.out("board.png");
    run.go("kicad-cli", &["pcb", "render", "-o", &png, "--width", "1024", "--height", "768", "--side", "top", "--quality", "basic", &input])?;
    let bytes = std::fs::read(run.scratch.join("board.png")).map_err(|e| e.to_string())?;
    Ok(crate::media::png_in_svg(&bytes, 1024, 768))
}

/// *Export from KiCad*: `path` as `to`, into `name` in the scratch folder.
pub(crate) fn export(run: &mut Run, path: &Path, to: To, name: &str) -> Result<(), String> {
    if !to.takes(path) {
        return Err(t!(if to == To::Bom { "kicad.needs_schematic" } else { "kicad.needs_board" }));
    }
    let (input, out) = (run.input.clone(), run.out(name));
    let sch = is_schematic(path);
    match to {
        To::Pdf if sch => run.go("kicad-cli", &["sch", "export", "pdf", "-o", &out, &input]),
        To::Pdf => {
            // A PDF of many pages is written into a folder, named after the board.
            let dir = run.out("pages");
            std::fs::create_dir_all(run.scratch.join("pages")).map_err(|e| e.to_string())?;
            run.go("kicad-cli", &["pcb", "export", "pdf", "--layers", LAYERS, "--common-layers", OUTLINE, "--mode-multipage", "-o", &dir, &input])?;
            let pdf = files(&run.scratch.join("pages")).into_iter().find(|p| p.extension().is_some_and(|e| e == "pdf")).ok_or_else(|| t!("convert.no_result"))?;
            return std::fs::rename(pdf, run.scratch.join(name)).map_err(|e| e.to_string());
        }
        To::Svg if sch => {
            let dir = run.out("parts");
            std::fs::create_dir_all(run.scratch.join("parts")).map_err(|e| e.to_string())?;
            run.go("kicad-cli", &["sch", "export", "svg", "-o", &dir, &input])?;
            return zip(&run.scratch.join("parts"), &run.scratch.join(name));
        }
        To::Svg => run.go("kicad-cli", &["pcb", "export", "svg", "--layers", SVG_LAYERS, "--mode-single", "-o", &out, &input]),
        To::Bom => run.go("kicad-cli", &["sch", "export", "bom", "-o", &out, &input]),
        To::Gerber => {
            let dir = format!("{}/", run.out("parts"));
            std::fs::create_dir_all(run.scratch.join("parts")).map_err(|e| e.to_string())?;
            run.go("kicad-cli", &["pcb", "export", "gerbers", "--board-plot-params", "-o", &dir, &input])?;
            run.go("kicad-cli", &["pcb", "export", "drill", "-o", &dir, &input])?;
            return zip(&run.scratch.join("parts"), &run.scratch.join(name));
        }
    }
    .map(drop)
}

/// The files of `dir`, by name.
fn files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    v.sort();
    v
}

/// The files of `dir` zipped into `to`, each under its own name.
fn zip(dir: &Path, to: &Path) -> Result<(), String> {
    let all = files(dir);
    if all.is_empty() {
        return Err(t!("convert.no_result"));
    }
    let f = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for p in all {
        z.start_file(p.file_name().unwrap_or_default().to_string_lossy(), opts).map_err(|e| e.to_string())?;
        let mut src = std::fs::File::open(&p).map_err(|e| e.to_string())?;
        std::io::copy(&mut src, &mut z).map_err(|e| e.to_string())?;
    }
    z.finish().map(drop).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs kicad-cli installed, or with `COXSWAIN_KICAD_CONTAINER=1` podman with KiCad's image
    /// pulled, and `COXSWAIN_KICAD_PROJECT` a folder with one schematic and one board (a copy
    /// of one of KiCad's demos): `cargo test -p coxswain-core -- --ignored real_kicad`.
    #[test]
    #[ignore]
    fn real_kicad() {
        use crate::media::{Op, Program};
        crate::i18n::set_language("en-GB");
        let dir = std::path::PathBuf::from(std::env::var("COXSWAIN_KICAD_PROJECT").unwrap());
        let pick = |e: &str| files(&dir).into_iter().find(|p| p.extension().is_some_and(|x| x == e)).unwrap();
        let (sch, pcb) = (pick("kicad_sch"), pick("kicad_pcb"));
        let container = std::env::var("COXSWAIN_KICAD_CONTAINER").is_ok();
        let cfg = crate::config::PreviewConfig { prefer: if container { "container" } else { "local" }.into(), ..Default::default() };
        let stop = std::sync::atomic::AtomicBool::new(false);
        let run = |op: Op, f: &Path| {
            crate::media::run_all(&cfg, &op, &[f.to_path_buf()], &stop, |_| {}, |s| {
                if let crate::media::Said::Line(l) = s {
                    eprintln!("{l}")
                }
            })
            .remove(0)
        };
        let mut made = vec![];
        for (to, f, ending) in [(To::Pdf, &sch, "pdf"), (To::Svg, &sch, "svg.zip"), (To::Bom, &sch, "bom.csv"), (To::Pdf, &pcb, "pdf"), (To::Svg, &pcb, "svg"), (To::Gerber, &pcb, "gerbers.zip")] {
            let out = run(Op::Kicad(to), f).unwrap_or_else(|e| panic!("{to:?}: {e}"));
            assert!(out.to_string_lossy().ends_with(ending) && std::fs::metadata(&out).unwrap().len() > 100, "{}", out.display());
            made.push(out);
        }
        assert_eq!(run(Op::Kicad(To::Gerber), &sch).unwrap_err(), t!("kicad.needs_board"));
        let how = crate::media::how(&cfg, Program::Kicad).unwrap();
        let d = std::env::temp_dir().join(format!("coxswain-kicad-real-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for (op, f, head) in [(Op::Board, &pcb, "%PDF"), (Op::BoardThumb, &pcb, "<svg"), (Op::BoardThumb, &sch, "<svg")] {
            let out = d.join("out");
            crate::media::preview(&how, &op, f, &out, std::time::Duration::from_secs(120)).unwrap();
            assert!(std::fs::read_to_string(&out).map(|s| s.contains(head)).unwrap_or_else(|_| std::fs::read(&out).unwrap().starts_with(head.as_bytes())), "{op:?}");
        }
        for m in made {
            std::fs::remove_file(m).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn kicad_exports_take_their_kinds_and_zip_several_files() {
        let (s, b) = (Path::new("amp.kicad_sch"), Path::new("amp.kicad_pcb"));
        assert!(To::Bom.takes(s) && !To::Bom.takes(b) && To::Gerber.takes(b) && !To::Gerber.takes(s));
        assert!(To::Pdf.takes(s) && To::Svg.takes(b) && !To::Pdf.takes(Path::new("amp.kicad_pro")));
        assert_eq!((To::Svg.result_ending(s), To::Svg.result_ending(b)), ("svg.zip", "svg"));
        assert_eq!(To::ALL.map(|t| To::parse(t.id())), To::ALL.map(Some));
        let d = std::env::temp_dir().join(format!("coxswain-kicad-{}", std::process::id()));
        let parts = d.join("parts");
        std::fs::create_dir_all(&parts).unwrap();
        std::fs::write(parts.join("a-F_Cu.gtl"), "G04*").unwrap();
        std::fs::write(parts.join("a.drl"), "M48").unwrap();
        zip(&parts, &d.join("a.gerbers.zip")).unwrap();
        let z = zip::ZipArchive::new(std::fs::File::open(d.join("a.gerbers.zip")).unwrap()).unwrap();
        assert_eq!(z.file_names().count(), 2);
        assert!(zip(&d.join("none"), &d.join("b.zip")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}

// Readers that take a path: pictures, disk image headers, office files and the rest of the
// search text readers, tables, the name index, the search store, chess, state.json.
#![no_main]
use libfuzzer_sys::fuzz_target;
mod common;

const ENDINGS: &[&str] = &[
    "psd", "iff", "lbm", "pcx", "xpm", "tif", "tga", "pnm", "dds", "hdr", "exr", "qoi", "ff", "jxl",
    "iso", "vhd", "vhdx", "vmdk", "qcow2", "vdi", "wim", "dmg", "img", "chm",
    "docx", "odt", "xlsx", "xls", "xlsb", "ods", "pptx", "odp", "epub", "html", "ipynb", "drawio",
    "eml", "msg", "dbf", "rtf", "pdf", "pgn", "parquet", "sqlite", "bin", "json", "mod", "xm", "s3m", "it",
];

fuzz_target!(|data: &[u8]| {
    let Some((_dir, path, ending)) = common::file(ENDINGS, data) else { return };
    use coxswain_core::{chess, disk_image, extract, pictures, tables};
    let p = path.as_path();
    const MAX: u64 = 1 << 20;
    let _ = coxswain_core::facts::of(p);
    match ending {
        "psd" | "iff" | "lbm" | "pcx" | "xpm" | "tif" | "tga" | "pnm" | "dds" | "hdr" | "exr" | "qoi" | "ff" | "jxl" => {
            let _ = pictures::facts(p);
            let _ = pictures::decode(p, 0);
            let _ = pictures::decode(p, 1);
        }
        "iso" | "vhd" | "vhdx" | "vmdk" | "qcow2" | "vdi" | "wim" | "dmg" | "img" | "chm" => {
            if let Some(f) = disk_image::facts(p) {
                let _ = disk_image::lines(&f);
            }
        }
        "docx" | "odt" => drop(extract::word::text(p, MAX)),
        "xlsx" | "xls" | "xlsb" | "ods" => drop(extract::sheet::text(p, MAX)),
        "pptx" | "odp" => drop(extract::slides::text(p, MAX)),
        "epub" | "html" => {
            let _ = extract::book::text(p, MAX);
            let _ = extract::book::first_chapter(p);
        }
        "ipynb" | "drawio" => drop(extract::notebook::text(p, MAX)),
        "eml" => {
            let _ = extract::mail::text(p, MAX);
            let _ = extract::mail::summary(p, MAX);
        }
        "msg" => {
            let _ = extract::msg::text(p, MAX);
            let _ = extract::msg::summary(p);
        }
        "dbf" => {
            let _ = coxswain_core::dbf::search_text(p, MAX);
            let _ = tables::of(p, 20);
        }
        "rtf" => drop(extract::rtf::text(p, MAX)),
        "pdf" => drop(extract::pdf::text(p, MAX)),
        "pgn" => {
            let _ = chess::games::search_text(p, 1 << 16);
            let _ = chess::read_positions(p);
        }
        "parquet" | "sqlite" => drop(tables::of(p, 20)),
        "bin" => {
            let _ = coxswain_core::index::Index::facts(p);
            let _ = coxswain_core::store::facts(p);
        }
        "json" => drop(coxswain_core::state::AppState::load_from(p)),
        _ => drop(coxswain_core::music::read(p)),
    }
});

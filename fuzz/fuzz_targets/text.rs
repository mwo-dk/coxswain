// Readers of text and bytes in memory: encodings, the viewer, PGN, passages for search by
// meaning, the diagram and markdown readers, config.toml, git's config.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    use coxswain_core::{chess, code, config, extract, meaning, view};
    let Some((&pick, bytes)) = data.split_first() else { return };
    let _ = view::decode(bytes);
    let _ = view::is_utf8(bytes);
    let _ = view::decode_line(bytes, pick & 1 == 0);
    let _ = view::hex(bytes, 4);
    let _ = coxswain_core::music::parse(bytes);
    let _ = coxswain_core::git_config::parse(bytes, std::path::Path::new("/r"));
    let text = extract::utf8(bytes);
    match pick % 8 {
        0 => {
            let g = chess::pgn::parse(text);
            let _ = chess::positions(text);
            drop(g);
        }
        1 => drop(meaning::passages(text, "a.md")),
        2 => drop(meaning::passages(text, "a.rs")),
        3 => drop(meaning::passages(text, "a.txt")),
        4 => {
            let _ = extract::diagram::markdown(text.to_string());
            let _ = extract::diagram::mermaid(text.to_string());
            let _ = extract::diagram::graphviz(text.to_string());
            let _ = extract::diagram::plantuml(text.to_string());
        }
        5 => {
            let _ = extract::xml_text(bytes, &["p"], &["script"]);
            let _ = extract::page_text(bytes, &["p"], &["script"]);
        }
        6 => drop(config::Config::parse(text)),
        _ => {
            let _ = extract::tidy(text);
            let _ = extract::tidy_code(text);
            let _ = code::language("a", text);
            let _ = chess::pgn::board(text);
            let _ = chess::squares(text);
        }
    }
});

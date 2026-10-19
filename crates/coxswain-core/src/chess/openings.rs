//! Opening names, from lichess-org/chess-openings (CC0 1.0, `openings.tsv`): by a game's first
//! moves, else by its ECO code.

use std::collections::HashMap;
use std::sync::OnceLock;

static TABLE: &str = include_str!("openings.tsv");

struct Openings {
    /// ECO code and name, per line of the table.
    entries: Vec<(&'static str, &'static str)>,
    /// The moves as a tree: (node, move) -> node; node 0 is the start.
    next: HashMap<(u32, &'static str), u32>,
    /// Per node: the entry whose moves end there.
    named: Vec<Option<u32>>,
    /// Per ECO code: its shortest name.
    by_eco: HashMap<&'static str, &'static str>,
}

/// A move as the table writes it: no check signs, castling with letters.
pub(crate) fn plain(san: &str) -> &str {
    let s = san.trim_end_matches(['+', '#', '!', '?']);
    match s {
        "0-0" => "O-O",
        "0-0-0" => "O-O-O",
        s => s,
    }
}

fn get() -> &'static Openings {
    static O: OnceLock<Openings> = OnceLock::new();
    O.get_or_init(|| {
        let mut o = Openings { entries: vec![], next: HashMap::new(), named: vec![None], by_eco: HashMap::new() };
        for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.split('\t');
            let (Some(eco), Some(name), Some(moves)) = (f.next(), f.next(), f.next()) else { continue };
            let id = o.entries.len() as u32;
            o.entries.push((eco, name));
            let shorter = o.by_eco.get(eco).is_none_or(|n| name.len() < n.len());
            if shorter {
                o.by_eco.insert(eco, name);
            }
            let mut at = 0u32;
            for m in moves.split(' ') {
                let fresh = o.named.len() as u32;
                at = *o.next.entry((at, plain(m))).or_insert(fresh);
                if at == fresh {
                    o.named.push(None);
                }
            }
            o.named[at as usize].get_or_insert(id);
        }
        o
    })
}

/// The opening the moves `sans` (the game's first, in order) are: ECO code and name, of the
/// longest line of the table they follow.
pub fn by_moves<'a>(sans: impl IntoIterator<Item = &'a str>) -> Option<(&'static str, &'static str)> {
    let o = get();
    let mut at = 0u32;
    let mut best = None;
    for m in sans {
        match o.next.get(&(at, plain(m))) {
            Some(&n) => at = n,
            None => break,
        }
        if let Some(e) = o.named[at as usize] {
            best = Some(o.entries[e as usize]);
        }
    }
    best
}

/// The shortest name the table has for an ECO code.
pub fn by_eco(eco: &str) -> Option<&'static str> {
    get().by_eco.get(eco).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_by_moves_and_code() {
        assert_eq!(by_moves(["e4", "e5", "Nf3", "d6"]), Some(("C41", "Philidor Defense")));
        // Past the table's line: the longest it has.
        assert_eq!(by_moves(["e4", "e5", "Nf3", "d6", "d4", "Bg4", "a3"]).map(|o| o.0), Some("C41"));
        assert_eq!(by_moves(["e4", "c5"]).map(|o| o.1), Some("Sicilian Defense"));
        assert_eq!(by_moves(["h4", "h5", "Rh3"]).map(|o| o.0), Some("A00"));
        assert_eq!(by_moves(["Ke2"]), None);
        assert_eq!(by_eco("B20"), Some("Sicilian Defense"));
        assert_eq!(by_eco("Z99"), None);
    }
}

//! Bridge in PBN (docs/previews/bridge.md), for both apps: a PBN file's deals go through the
//! chess table (`chess::Library`), each deal shown as four hands round a table, its auction,
//! and its play trick by trick, the cards played taken out of the hands and each trick's winner
//! worked out from the contract's trumps. Reading only.
//!
//! A deal is read as a `pgn::Game`: one node per trick (`san` its four cards, `number` the
//! trick, `fen` the table after it, `State::text`), so the apps step through it as through a
//! game's moves. Any text at all is read without a panic and without recursion; every list is
//! capped (13 tricks, `MAX_BIDS` calls).

use crate::chess::pgn::{self, Game, Node};

/// The file endings of PBN files, for the search store.
pub const ENDINGS: [&str; 1] = [".pbn"];
/// The most calls of an auction kept: a real one has a few dozen.
pub const MAX_BIDS: usize = 320;

const SEATS: [char; 4] = ['N', 'E', 'S', 'W'];
const SUITS: [char; 4] = ['S', 'H', 'D', 'C'];
const RANKS: &str = "23456789TJQKA";

/// A seat from its letter: N 0, E 1, S 2, W 3.
fn seat(c: char) -> Option<usize> {
    SEATS.iter().position(|&s| s == c.to_ascii_uppercase())
}

/// A card: its suit (0 spades … 3 clubs) and rank (0 the two … 12 the ace).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub suit: u8,
    pub rank: u8,
}

impl Card {
    /// A card as PBN writes it: `SA`, `HT`, `D10`, `C2`.
    fn parse(s: &str) -> Option<Card> {
        let mut c = s.chars();
        let first = c.next()?.to_ascii_uppercase();
        let suit = SUITS.iter().position(|&x| x == first)? as u8;
        let r: String = c.collect::<String>().to_ascii_uppercase();
        let r = if r == "10" { "T".to_string() } else { r };
        if r.chars().count() != 1 {
            return None;
        }
        let rank = RANKS.find(r.as_str())? as u8;
        Some(Card { suit, rank })
    }

    fn code(&self) -> String {
        format!("{}{}", SUITS[self.suit as usize], &RANKS[self.rank as usize..self.rank as usize + 1])
    }

    /// As the apps show it: `♠A`, `♥10`.
    pub fn shown(&self) -> String {
        let r = &RANKS[self.rank as usize..self.rank as usize + 1];
        format!("{}{}", ['♠', '♥', '♦', '♣'][self.suit as usize], if r == "T" { "10" } else { r })
    }
}

/// The table at a moment: each seat's cards still held, the trick on the table (seat, card),
/// who leads next, the tricks won by North–South and East–West.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub hands: [Vec<Card>; 4],
    pub trick: Vec<(usize, Card)>,
    pub leader: Option<usize>,
    pub won: [u8; 2],
}

impl State {
    /// As kept in a node's `fen`: `N:AKQ.T98.765.432 E:… S:… W:…;W:SA N:S3 E:SK S:S2;N;1;0`
    /// (hands as suits from spades down, the trick, the next leader, tricks North–South and
    /// East–West). A hand not known is `-`.
    pub fn text(&self) -> String {
        let hands: Vec<String> = (0..4)
            .map(|s| {
                let suits: Vec<String> = (0..4u8)
                    .map(|su| {
                        let mut r: Vec<u8> = self.hands[s].iter().filter(|c| c.suit == su).map(|c| c.rank).collect();
                        r.sort_unstable_by(|a, b| b.cmp(a));
                        r.iter().map(|&r| &RANKS[r as usize..r as usize + 1]).collect()
                    })
                    .collect();
                format!("{}:{}", SEATS[s], suits.join("."))
            })
            .collect();
        let trick: Vec<String> = self.trick.iter().map(|(s, c)| format!("{}:{}", SEATS[*s], c.code())).collect();
        format!("{};{};{};{};{}", hands.join(" "), trick.join(" "), self.leader.map_or('-', |l| SEATS[l]), self.won[0], self.won[1])
    }

    pub fn from_text(t: &str) -> State {
        let mut f = t.split(';');
        let mut s = State::default();
        for h in f.next().unwrap_or("").split_whitespace() {
            let Some((who, cards)) = h.split_once(':') else { continue };
            let Some(who) = who.chars().next().and_then(seat) else { continue };
            s.hands[who] = hand(cards);
        }
        for c in f.next().unwrap_or("").split_whitespace().take(4) {
            if let Some((who, card)) = c.split_once(':') {
                if let (Some(w), Some(card)) = (who.chars().next().and_then(seat), Card::parse(card)) {
                    s.trick.push((w, card));
                }
            }
        }
        s.leader = f.next().and_then(|l| l.chars().next()).and_then(seat);
        s.won = [f.next().and_then(|n| n.parse().ok()).unwrap_or(0), f.next().and_then(|n| n.parse().ok()).unwrap_or(0)];
        s
    }
}

/// A hand as PBN writes it: `AKQ.T98.765.432`, spades first.
fn hand(s: &str) -> Vec<Card> {
    let mut out = vec![];
    for (suit, ranks) in s.split('.').take(4).enumerate() {
        for r in ranks.chars().take(13) {
            if let Some(rank) = RANKS.find(r.to_ascii_uppercase()) {
                let c = Card { suit: suit as u8, rank: rank as u8 };
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// A deal: `N:AKQ.T98.765.432 …`, the first seat named, then the others clockwise.
fn deal(s: &str) -> [Vec<Card>; 4] {
    let mut hands: [Vec<Card>; 4] = Default::default();
    let (first, rest) = s.trim().split_once(':').unwrap_or(("N", s));
    let first = first.trim().chars().next().and_then(seat).unwrap_or(0);
    for (i, h) in rest.split_whitespace().take(4).enumerate() {
        hands[(first + i) % 4] = if h == "-" { vec![] } else { hand(h) };
    }
    hands
}

/// The contract as the apps show it: `4♥`, `3NT×`, `Pass`; with the trump suit (None in no
/// trumps) and the level.
pub fn contract(c: &str) -> (String, Option<u8>, u8) {
    let c = c.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("pass") {
        return (if c.is_empty() { String::new() } else { "Pass".into() }, None, 0);
    }
    let level = c.chars().next().and_then(|l| l.to_digit(10)).filter(|l| (1..=7).contains(l)).unwrap_or(0) as u8;
    let rest = c.get(1..).unwrap_or("").to_ascii_uppercase();
    let (denom, trump) = if rest.starts_with("NT") || rest.starts_with('N') {
        ("NT", None)
    } else {
        match rest.chars().next() {
            Some('S') => ("♠", Some(0)),
            Some('H') => ("♥", Some(1)),
            Some('D') => ("♦", Some(2)),
            Some('C') => ("♣", Some(3)),
            _ => ("", None),
        }
    };
    let doubled = if rest.ends_with("XX") { "××" } else if rest.ends_with('X') { "×" } else { "" };
    (format!("{level}{denom}{doubled}"), trump, level)
}

/// A call of the auction as shown: `1♥`, `2NT`, `Pass`, `×`, `××`. None for what is no call.
fn call(w: &str) -> Option<String> {
    let w = w.trim_end_matches(['!', '?']);
    Some(match w.to_ascii_uppercase().as_str() {
        "PASS" | "P" => "Pass".into(),
        "X" | "DBL" => "×".into(),
        "XX" | "RDBL" => "××".into(),
        "AP" => "AP".into(),
        "-" => "–".into(),
        b => {
            let (c, _, l) = contract(b);
            if l == 0 || b.len() > 3 || c.ends_with(char::is_numeric) {
                return None;
            }
            c
        }
    })
}

/// The tags kept of a PBN game, and its auction and play sections' words.
struct Sections {
    tags: Vec<(String, String)>,
    auction: Vec<String>,
    play: Vec<Vec<String>>,
    comment: String,
}

/// A PBN game's text cut into its tags, auction and play.
fn sections(text: &str) -> Sections {
    let mut s = Sections { tags: vec![], auction: vec![], play: vec![], comment: String::new() };
    let mut section = "";
    let mut in_brace = false;
    for line in text.lines().take(10_000) {
        let l: String = line.chars().take(4096).collect();
        let t = l.trim();
        if in_brace {
            if let Some(i) = t.find('}') {
                in_brace = false;
                let _ = i;
            }
            continue;
        }
        if t.starts_with('%') || t.is_empty() {
            continue;
        }
        if t.starts_with('[') {
            if let Some((k, v)) = pgn::tag_pair(t) {
                section = if k.eq_ignore_ascii_case("Auction") {
                    "auction"
                } else if k.eq_ignore_ascii_case("Play") {
                    "play"
                } else {
                    ""
                };
                if s.tags.len() < 128 {
                    s.tags.push((k, v));
                }
            }
            continue;
        }
        // Comments in braces: kept when they stand alone, skipped in the sections.
        let mut words = vec![];
        let mut rest = t;
        while !rest.is_empty() {
            if let Some(b) = rest.find('{') {
                words.extend(rest[..b].split_whitespace().map(str::to_string));
                match rest[b + 1..].find('}') {
                    Some(e) => {
                        if s.comment.len() < 4096 {
                            s.comment.push_str(rest[b + 1..b + 1 + e].trim());
                            s.comment.push(' ');
                        }
                        rest = &rest[b + 2 + e..];
                    }
                    None => {
                        in_brace = true;
                        rest = "";
                    }
                }
            } else {
                words.extend(rest.split_whitespace().map(str::to_string));
                rest = "";
            }
        }
        let words: Vec<String> = words.into_iter().filter(|w| !(w.starts_with('=') && w.ends_with('=')) && !w.starts_with('$')).collect();
        match section {
            "auction" => {
                for w in words {
                    if s.auction.len() < MAX_BIDS {
                        if let Some(c) = call(&w) {
                            s.auction.push(c);
                        }
                    }
                }
            }
            "play" if s.play.len() < 13 && !words.iter().all(|w| w == "*") => s.play.push(words),
            _ => {}
        }
    }
    s
}

/// A PBN game's text, read and played out: its tags (with `Auction` as its calls, from the
/// dealer's seat), the deal at the start, a node per trick.
pub fn parse(text: &str) -> Game {
    let s = sections(text);
    let tag = |k: &str| s.tags.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).unwrap_or("");
    let mut g = Game { tags: s.tags.clone(), comment: s.comment.trim().to_string(), ..Game::default() };
    // The calls, kept as a tag, the seat that called first before them.
    let first = tag("Auction").chars().next().and_then(seat).or_else(|| tag("Dealer").chars().next().and_then(seat));
    if !s.auction.is_empty() {
        let who = first.map_or('-', |f| SEATS[f]);
        g.tags.retain(|(k, _)| !k.eq_ignore_ascii_case("Auction"));
        g.tags.push(("Auction".into(), format!("{who} {}", s.auction.join(" "))));
    }
    let mut state = State { hands: deal(tag("Deal")), ..State::default() };
    let (_, trump, _) = contract(tag("Contract"));
    let play_from = tag("Play").chars().next().and_then(seat);
    state.leader = play_from.or_else(|| tag("Declarer").chars().next().and_then(seat).map(|d| (d + 1) % 4));
    g.start = state.text();
    let mut at: Option<u32> = None;
    for (i, row) in s.play.iter().enumerate() {
        let Some(from) = play_from else { break };
        // The columns are seats, clockwise from the Play tag's.
        let cards: Vec<(usize, Card)> = row.iter().take(4).enumerate().filter_map(|(k, w)| Card::parse(w).map(|c| ((from + k) % 4, c))).collect();
        if cards.is_empty() {
            break;
        }
        let leader = state.leader.unwrap_or(from);
        let mut order = cards.clone();
        order.sort_by_key(|(s, _)| (s + 4 - leader) % 4);
        let led = order[0].1.suit;
        // The highest of the trumps, else of the suit led.
        let top = trump.filter(|&t| order.iter().any(|(_, c)| c.suit == t)).unwrap_or(led);
        let winner = order.iter().filter(|(_, c)| c.suit == top).max_by_key(|(_, c)| c.rank).map(|x| x.0).unwrap_or(leader);
        for (who, c) in &order {
            state.hands[*who].retain(|h| h != c);
        }
        state.trick = order.clone();
        state.leader = Some(winner);
        state.won[winner % 2] = state.won[winner % 2].saturating_add(1);
        let id = g.nodes.len() as u32;
        g.nodes.push(Node {
            san: order.iter().map(|(_, c)| c.shown()).collect::<Vec<_>>().join(" "),
            parent: at,
            fen: Some(state.text()),
            white: true,
            number: i as u32 + 1,
            ..Node::default()
        });
        match at {
            Some(p) => g.nodes[p as usize].children.push(id),
            None => g.roots.push(id),
        }
        at = Some(id);
    }
    g.result = result(tag("Contract"), tag("Declarer"), tag("Result"));
    g
}

/// The result as the table shows it: the contract by its declarer and the tricks over or under
/// it, `4♥ S =`, `3NT N +1`, `2♠× E −2`; `Pass` for a deal passed out.
pub fn result(contract_tag: &str, declarer: &str, tricks: &str) -> String {
    let (c, _, level) = contract(contract_tag);
    if level == 0 {
        return c;
    }
    let by = declarer.trim().chars().next().filter(|c| seat(*c).is_some()).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default();
    let over = tricks.trim().parse::<i32>().ok().filter(|t| (0..=13).contains(t)).map(|t| t - 6 - level as i32);
    let over = match over {
        Some(0) => "=".to_string(),
        Some(n) if n > 0 => format!("+{n}"),
        Some(n) => format!("−{}", -n),
        None => String::new(),
    };
    [c, by, over].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// A deal's pairs and contract, as the table shows them: `["Ben, Di", "Cy, Ada"]` (North and
/// South, East and West) and `3NT S` (the contract and its declarer).
pub fn summary(g: &Game) -> ([String; 2], String) {
    let tag = |k: &str| g.tag(k).filter(|v| !v.is_empty() && *v != "#" && *v != "?").unwrap_or("");
    let pair = |a: &str, b: &str| [tag(a), tag(b)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    let (c, _, level) = contract(tag("Contract"));
    let by = tag("Declarer");
    ([pair("North", "South"), pair("East", "West")], if level > 0 && !by.is_empty() { format!("{c} {by}") } else { c })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEAL: &str = r#"[Event "Club pairs"]
[Site "Hangar 3"]
[Date "2026.05.20"]
[Board "1"]
[West "Ada"]
[North "Ben"]
[East "Cy"]
[South "Di"]
[Dealer "N"]
[Vulnerable "None"]
[Deal "N:AKQ5.T98.765.432 JT9.J765.432.AKQ 876.432.AKQJ.JT9 432.AKQ.T98.8765"]
[Scoring "MP"]
[Declarer "S"]
[Contract "3NT"]
[Result "9"]
[Auction "N"]
1C Pass 1D =1= Pass
3NT AP
[Note "1:Natural"]
[Play "W"]
HA H8 H5 H2
C5 C2 CA C9
S2 SA S9 S6
*
"#;

    #[test]
    fn a_deal_played_out() {
        let g = parse(DEAL);
        assert_eq!(g.result, "3NT S =");
        assert_eq!(g.tag("Auction"), Some("N 1♣ Pass 1♦ Pass 3NT AP"));
        let start = State::from_text(&g.start);
        assert_eq!(start.hands.iter().map(Vec::len).collect::<Vec<_>>(), [13, 13, 13, 13]);
        assert_eq!(start.leader, Some(3));
        assert_eq!(g.nodes.len(), 3);
        // Trick 1: West leads ♥A and wins it.
        let t1 = State::from_text(g.nodes[0].fen.as_deref().unwrap());
        assert_eq!(g.nodes[0].san, "♥A ♥8 ♥5 ♥2");
        assert_eq!((t1.leader, t1.won), (Some(3), [0, 1]));
        assert_eq!(t1.hands.iter().map(Vec::len).collect::<Vec<_>>(), [12, 12, 12, 12]);
        // Trick 2: West leads ♣5, East's ♣A wins; trick 3: East leads, North's ♠A wins.
        let t2 = State::from_text(g.nodes[1].fen.as_deref().unwrap());
        assert_eq!(t2.trick[0], (3, Card { suit: 3, rank: 3 }));
        assert_eq!((t2.leader, t2.won), (Some(1), [0, 2]), "{t2:?}");
        assert_eq!(g.nodes[2].number, 3);
        let t3 = State::from_text(g.nodes[2].fen.as_deref().unwrap());
        assert_eq!((t3.trick[0].0, t3.leader, t3.won), (1, Some(0), [1, 2]));
        assert_eq!(State::from_text(&t2.text()), t2);
    }

    #[test]
    fn contracts_and_results() {
        assert_eq!(contract("4HX"), ("4♥×".into(), Some(1), 4));
        assert_eq!(contract("7NTXX").0, "7NT××");
        assert_eq!(contract("Pass").0, "Pass");
        assert_eq!(result("4S", "E", "8"), "4♠ E −2");
        assert_eq!(result("1NT", "W", "9"), "1NT W +2");
        assert_eq!(result("", "", ""), "");
        assert_eq!(Card::parse("D10"), Card::parse("DT"));
        assert_eq!(Card::parse("SA").unwrap().shown(), "♠A");
        assert!(Card::parse("ZA").is_none() && Card::parse("S").is_none() && Card::parse("Sé").is_none());
        assert!(call("8NT").is_none() && call("1NT!").is_some());
    }

    #[test]
    fn trumps_win() {
        let t = "[Deal \"W:K... A... 2... ...3\"]\n[Contract \"1C\"]\n[Declarer \"S\"]\n[Play \"W\"]\nSK SA S2 C3\n";
        let g = parse(t);
        let s = State::from_text(g.nodes[0].fen.as_deref().unwrap());
        // South's ♣3 trumps the ♠A.
        assert_eq!(s.leader, Some(2), "{:?}", g.nodes);
    }

    #[test]
    fn whatever_the_text() {
        for t in ["", "[", "[Deal \"\"]", "[Deal \"Q:é.é\"]", "[Play \"X\"]\nSA", "[Play \"N\"]\n- - - -\n* * * *", "[Auction \"N\"]\n{", "{\n[Deal \"N:AAAA.AAAA.AAAA.AAAA\"]", "[Contract \"9Z\"]", "[Play \"W\"]\nS10 H10 D10 C10 S10"] {
            let g = parse(t);
            let _ = State::from_text(&g.start);
            for n in &g.nodes {
                let _ = State::from_text(n.fen.as_deref().unwrap_or(""));
            }
        }
        // Endless calls and tricks: capped.
        let t = format!("[Auction \"N\"]\n{}\n[Play \"W\"]\n{}", "Pass ".repeat(100_000), "SA HA DA CA\n".repeat(1000));
        let g = parse(&t);
        assert!(g.nodes.len() <= 13);
        assert!(g.tag("Auction").unwrap().split(' ').count() <= MAX_BIDS + 1);
    }

    #[test]
    fn a_file_in_the_table_and_found() {
        let d = std::env::temp_dir().join(format!("coxswain-pbn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("club.pbn");
        let second = "[Event \"#\"]\n[Board \"2\"]\n[North \"Eve\"]\n[South \"Fay\"]\n[Contract \"4S\"]\n[Declarer \"N\"]\n[Result \"8\"]\n";
        std::fs::write(&p, format!("% PBN 2.1\n{DEAL}\n{second}")).unwrap();
        let lib = crate::chess::Library::open(&p).unwrap();
        while !lib.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(lib.progress().games, 2);
        let r = lib.row(0).unwrap();
        assert_eq!((r.white.as_str(), r.black.as_str(), r.opening.as_str(), r.result.as_str(), r.moves), ("Ben, Di", "Cy, Ada", "3NT S", "3NT S =", 3));
        let r = lib.row(1).unwrap();
        assert_eq!((r.event.as_str(), r.result.as_str()), ("Club pairs", "4♠ N −2"));
        assert_eq!(lib.game(0).unwrap().nodes.len(), 3);
        let text = crate::extract::text_of(&p, 5000, 1 << 20).unwrap();
        assert!(text.contains("Eve, Fay") && !text.contains("SA"), "{text}");
        let (o, c) = crate::store::MARK;
        assert_eq!(crate::chess::games::game_of_hit(&p, &format!("{o}Fay{c}")), Some(1));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

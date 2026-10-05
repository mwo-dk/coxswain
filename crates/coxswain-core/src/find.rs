//! One Find: names, words in files, files about the words, and Ask, in one field and one list.
//!
//! The list is grouped: *Names*, *In files* (the files with the words), *About this* (files
//! close in meaning that lack the words) and *History* (commits). The groups come in an order
//! that follows the query's shape, and *In files* fuses the word rank with the meaning rank.
//! Both apps draw the rows `rows` makes, so they show the same list.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::SearchConfig;
use crate::index::{Hit, Results};
use crate::store::Store;
use crate::t;

/// Which part of Find is shown: everything, or one group, or Ask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    All,
    Names,
    InFiles,
    About,
    Ask,
}

impl Kind {
    pub const EVERY: [Kind; 5] = [Kind::All, Kind::Names, Kind::InFiles, Kind::About, Kind::Ask];

    /// The next kind (Tab), or the one before (Shift+Tab), round.
    pub fn next(self, back: bool) -> Kind {
        let i = Self::EVERY.iter().position(|k| *k == self).unwrap_or(0);
        Self::EVERY[(i + if back { 4 } else { 1 }) % 5]
    }

    /// Its chip's label.
    pub fn label(self) -> String {
        t!(match self {
            Kind::All => "find.kind_all",
            Kind::Names => "find.kind_names",
            Kind::InFiles => "find.kind_in_files",
            Kind::About => "find.kind_about",
            Kind::Ask => "find.kind_ask",
        })
    }
}

/// What a query looks like, which decides the order of the groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Name syntax (`*.rs`, `ext:md`, `!test`, `src/ foo`): names only.
    Syntax,
    /// One or two plain words: names first.
    Short,
    /// Three plain words or more: the files' words and meaning first.
    Long,
    /// Ends in `?`: as long, and the cursor starts on Ask.
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupId {
    Names,
    InFiles,
    About,
    History,
}

impl GroupId {
    pub fn label(self) -> String {
        t!(match self {
            GroupId::Names => "find.group_names",
            GroupId::InFiles => "find.group_in_files",
            GroupId::About => "find.group_about",
            GroupId::History => "find.group_history",
        })
    }

    /// The kind that shows this group alone.
    pub fn kind(self) -> Kind {
        match self {
            GroupId::Names => Kind::Names,
            GroupId::InFiles | GroupId::History => Kind::InFiles,
            GroupId::About => Kind::About,
        }
    }
}

/// Some hits, best first, and how many there were before the cut.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Group {
    pub hits: Vec<Hit>,
    pub total: usize,
}

/// Why a part of Find has nothing to show, and the one step that changes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "off", content = "detail", rename_all = "snake_case")]
pub enum Off {
    /// The helper, which holds the files' text, is not running.
    NoHelper,
    /// Search inside files is off.
    TextOff,
    /// The scope is outside every folder whose text is read.
    NotRead(PathBuf),
    /// Search by meaning is off.
    MeaningOff,
    /// Search by meaning failed: why.
    MeaningError(String),
    /// Ask needs search by meaning first.
    AskNeedsMeaning,
    /// Ask has no chat model.
    AskNoModel,
    /// Ask's chat model cannot answer: why.
    AskModel(String),
}

impl Off {
    /// What is missing and why it matters, for the user.
    pub fn text(&self) -> String {
        match self {
            Off::NoHelper => t!("find.off_no_helper"),
            Off::TextOff => t!("find.off_text"),
            Off::NotRead(dir) => t!("find.off_not_read", "folder" => dir.display().to_string()),
            Off::MeaningOff => t!("find.off_meaning"),
            Off::MeaningError(why) => why.clone(),
            Off::AskNeedsMeaning => t!("find.off_ask_meaning"),
            Off::AskNoModel => t!("find.off_ask_model"),
            Off::AskModel(why) => why.clone(),
        }
    }

    /// The step's label: "Start it", "Read this folder too", "Set up".
    pub fn step(&self) -> String {
        t!(match self {
            Off::NoHelper => "find.step_start",
            Off::NotRead(_) => "find.step_read_too",
            Off::TextOff => "find.step_turn_on",
            Off::MeaningError(_) => "find.step_fix",
            _ => "find.step_set_up",
        })
    }

    /// The id under which it is not shown again (Delete), for the tips that can go.
    pub fn dismiss_id(&self) -> Option<&'static str> {
        match self {
            Off::MeaningOff => Some("find-about"),
            Off::AskNeedsMeaning | Off::AskNoModel => Some("find-ask"),
            _ => None,
        }
    }
}

/// What Find found for a query: each group, and why some have nothing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Found {
    pub names: Group,
    pub in_files: Group,
    pub about: Group,
    pub history: Group,
    pub off: Vec<Off>,
}

impl Found {
    pub fn group(&self, id: GroupId) -> &Group {
        match id {
            GroupId::Names => &self.names,
            GroupId::InFiles => &self.in_files,
            GroupId::About => &self.about,
            GroupId::History => &self.history,
        }
    }
}

/// A prefix typed at the start turns its kind on: `text:` in files, `about:` meaning alone, a
/// leading `?` Ask. English in every language, like `ext:`. The query without it.
pub fn parse(input: &str) -> (Option<Kind>, &str) {
    let s = input.trim_start();
    let prefix = |p: &str| s.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p)).map(|_| s[p.len()..].trim_start());
    if let Some(rest) = prefix("text:") {
        (Some(Kind::InFiles), rest)
    } else if let Some(rest) = prefix("about:") {
        (Some(Kind::About), rest)
    } else if let Some(rest) = s.strip_prefix('?') {
        (Some(Kind::Ask), rest.trim_start())
    } else {
        (None, input)
    }
}

/// The query's shape (6.2 of the design): name syntax, short, long or a question.
pub fn shape(query: &str) -> Shape {
    let q = query.trim();
    let question = q.ends_with('?');
    let body = q.trim_end_matches('?');
    let lower = body.to_lowercase();
    if body.contains(['*', '?', '!', '|', '"', '/', '\\']) || ["ext:", "file:", "folder:", "case:"].iter().any(|k| lower.contains(k)) {
        return Shape::Syntax;
    }
    match body.split_whitespace().count() {
        _ if question => Shape::Question,
        0..=2 => Shape::Short,
        _ => Shape::Long,
    }
}

/// The groups in the order a query of this shape shows them.
pub fn order(shape: Shape) -> [GroupId; 4] {
    use GroupId::*;
    match shape {
        Shape::Syntax | Shape::Short => [Names, InFiles, About, History],
        Shape::Long | Shape::Question => [InFiles, About, Names, History],
    }
}

/// Whether the Ask row shows: two words or more, or a question; never for name syntax.
pub fn asks(query: &str) -> bool {
    let s = shape(query);
    s == Shape::Question || (s == Shape::Long || query.split_whitespace().count() > 1) && s != Shape::Syntax
}

/// Reciprocal rank fusion: a file scores `words / (k + its rank among the word hits)` plus
/// `meaning / (k + its rank among the meaning hits)`; `k` damps the lead of the very first ranks.
/// A file with only some of the words (the fallback when few have them all) weighs `some`
/// instead of `words`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub k: f32,
    pub words: f32,
    pub some: f32,
    pub meaning: f32,
}

/// The word hits for fusion: best first, the first `every` with every word.
#[derive(Clone, Copy)]
pub struct Words<'a> {
    pub hits: &'a [Hit],
    pub every: usize,
}

/// Chosen with the search-quality eval (docs/reference/performance.md, "Search quality").
pub const WEIGHTS: Weights = Weights { k: 60.0, words: 1.0, some: 0.0, meaning: 2.0 };

/// A hit's fused score: its weighted reciprocal rank in each list it is in.
fn score(words: Words, meaning: &[Hit], w: Weights, h: &Hit) -> f32 {
    let rank = |list: &[Hit]| list.iter().position(|x| x.path == h.path);
    let by_words = rank(words.hits).map_or(0.0, |r| if r < words.every { w.words } else { w.some } / (w.k + r as f32 + 1.0));
    by_words + rank(meaning).map_or(0.0, |r| w.meaning / (w.k + r as f32 + 1.0))
}

/// `hits` best first by fused score; equal scores keep their order.
fn by_score(words: Words, meaning: &[Hit], w: Weights, hits: impl Iterator<Item = Hit>) -> Vec<Hit> {
    let mut v: Vec<(f32, Hit)> = hits.map(|h| (score(words, meaning, w, &h), h)).collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    v.into_iter().map(|(_, h)| h).collect()
}

/// The word hits and the meaning hits, fused by rank: *In files* (every file with the words,
/// a file also close in meaning raised; a file with only some of them only when meaning finds
/// it too, or meaning is off), *About this* (files found by meaning alone), and *History*
/// (commits, from either). A file keeps its word snippet; ties keep the word order.
pub fn fuse(words: Words, meaning: &[Hit], w: Weights) -> (Vec<Hit>, Vec<Hit>, Vec<Hit>) {
    let commit = |h: &Hit| crate::history::is_history(&h.path);
    let close = |h: &Hit| meaning.iter().any(|m| m.path == h.path);
    // A file with only some of the words stays when meaning agrees, or when there is no meaning.
    let kept = |(i, h): &(usize, &Hit)| *i < words.every || meaning.is_empty() || close(h);
    let words_kept: Vec<Hit> = words.hits.iter().enumerate().filter(kept).map(|(_, h)| h.clone()).collect();
    let only_meaning = || meaning.iter().filter(|h| !words.hits.iter().any(|x| x.path == h.path));
    let in_files = by_score(words, meaning, w, words_kept.iter().filter(|h| !commit(h)).cloned());
    let about = by_score(words, meaning, w, only_meaning().filter(|h| !commit(h)).cloned());
    let history = by_score(words, meaning, w, words_kept.iter().chain(only_meaning()).filter(|h| commit(h)).cloned());
    (in_files, about, history)
}

/// Every file of both lists once, best first by fused score: what the search-quality eval
/// measures against words alone and meaning alone.
pub fn fused(words: Words, meaning: &[Hit], w: Weights) -> Vec<Hit> {
    by_score(words, meaning, w, words.hits.iter().chain(meaning.iter().filter(|h| !words.hits.iter().any(|x| x.path == h.path))).cloned())
}

/// What Find looks for: names from `names`, words and meaning from `store` (or why there is
/// none), below `scope` when given, `rows` hits per group at most. `text_roots`: the folders
/// whose text is read.
pub fn run(names: impl FnOnce(&str, usize) -> Results, store: Result<&Store, Off>, text_roots: &[PathBuf], query: &str, scope: Option<&Path>, kind: Kind, rows: usize) -> Found {
    let (prefix, q) = parse(query);
    let kind = prefix.unwrap_or(kind);
    let mut found = Found::default();
    if q.trim().is_empty() || kind == Kind::Ask {
        return found;
    }
    let syntax = shape(q) == Shape::Syntax;
    if matches!(kind, Kind::All | Kind::Names) {
        let r = names(q, rows);
        found.names = Group { hits: r.hits, total: r.total };
    }
    if kind == Kind::Names || kind == Kind::All && syntax {
        return found;
    }
    let store = match store {
        Ok(s) => s,
        Err(off) => {
            found.off.push(off);
            return found;
        }
    };
    if let Some(dir) = scope.filter(|d| !text_roots.iter().any(|r| d.starts_with(r) || r.starts_with(d))) {
        found.off.push(Off::NotRead(dir.to_path_buf()));
        return found;
    }
    // Each list deeper than shown, so the fusion has room.
    let deep = rows.saturating_mul(4);
    let (words, every) = if kind == Kind::About { Default::default() } else { store.search_words(q, scope, deep) };
    let meaning = if store.meaning.load(std::sync::atomic::Ordering::Relaxed) {
        store.similar(q, scope, deep)
    } else {
        found.off.push(Off::MeaningOff);
        vec![]
    };
    if let Some(why) = store.meaning_error.lock().unwrap().clone().filter(|_| meaning.is_empty() && !found.off.contains(&Off::MeaningOff)) {
        found.off.push(Off::MeaningError(why));
    }
    let commits_found = words.hits.iter().filter(|h| crate::history::is_history(&h.path)).count();
    let (in_files, about, history) = fuse(Words { hits: &words.hits, every }, &meaning, WEIGHTS);
    let group = |mut hits: Vec<Hit>, total: usize| {
        let total = total.max(hits.len());
        hits.truncate(rows);
        Group { hits, total }
    };
    // With the fallback to any of the words, the files meaning did not agree with are not shown:
    // the count is of those that are.
    let with_words = if words.hits.len() > every { in_files.len() } else { words.total.saturating_sub(commits_found) };
    found.in_files = group(in_files, with_words);
    found.about = group(about, 0);
    found.history = group(history, commits_found);
    found
}

/// Whether Ask can be asked, from the settings: it needs search by meaning and a chat model.
pub fn ask_ready(cfg: &SearchConfig) -> Result<(), Off> {
    if !cfg.meaning {
        Err(Off::AskNeedsMeaning)
    } else if cfg.ask_model.is_empty() {
        Err(Off::AskNoModel)
    } else {
        Ok(())
    }
}

/// A row of Find's list.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "row", rename_all = "snake_case")]
pub enum Row {
    /// Ask the query; `off` when Ask cannot be asked yet.
    Ask { off: Option<Off> },
    /// A group's heading: how many of how many are shown.
    Head { group: GroupId, shown: usize, total: usize },
    Hit { group: GroupId, hit: Hit },
    /// More in the group than shown: Enter (or Tab) shows the group alone.
    More { group: GroupId, n: usize },
    /// Why the group has nothing, and its step.
    Off { group: GroupId, off: Off },
}

impl Row {
    /// Whether the cursor stops on it.
    pub fn selectable(&self) -> bool {
        !matches!(self, Row::Head { .. })
    }
}

/// In *All*, each group shows this many hits; the rest are a Tab away.
pub const ALL_ROWS: usize = 5;

/// The rows for `query` under the kind `chip` (a prefix in the query wins): the Ask row, then
/// each group with hits or a reason, in the order of the query's shape. `ask`: whether Ask can
/// be asked; `dismissed`: the tips the user sent away.
pub fn rows(query: &str, chip: Kind, found: &Found, ask: Result<(), Off>, dismissed: impl Fn(&str) -> bool) -> Vec<Row> {
    let (prefix, q) = parse(query);
    let kind = prefix.unwrap_or(chip);
    let mut out = vec![];
    if q.trim().is_empty() || kind == Kind::Ask {
        return out;
    }
    let shape = shape(q);
    let shown = |off: &Off| kind != Kind::All || off.dismiss_id().is_none_or(|id| !dismissed(id));
    if kind != Kind::Names && asks(q) {
        let off = ask.err();
        if off.as_ref().is_none_or(shown) {
            out.push(Row::Ask { off });
        }
    }
    let wanted = |g: GroupId| match kind {
        Kind::All => true,
        Kind::InFiles => matches!(g, GroupId::InFiles | GroupId::History),
        _ => g.kind() == kind,
    };
    for g in order(shape).into_iter().filter(|g| wanted(*g)) {
        let group = found.group(g);
        let mut hits = group.hits.iter();
        if group.hits.is_empty() {
            // Why the group is empty: the reasons that belong to it.
            let mine = |o: &Off| match o {
                Off::NoHelper | Off::TextOff | Off::NotRead(_) => g == GroupId::InFiles,
                Off::MeaningOff | Off::MeaningError(_) => g == GroupId::About,
                _ => false,
            };
            for off in found.off.iter().filter(|o| mine(o) && shown(o)) {
                out.push(Row::Head { group: g, shown: 0, total: 0 });
                out.push(Row::Off { group: g, off: off.clone() });
            }
            continue;
        }
        let n = if kind == Kind::All { ALL_ROWS } else { usize::MAX };
        let shown = group.hits.len().min(n);
        out.push(Row::Head { group: g, shown, total: group.total.max(group.hits.len()) });
        out.extend(hits.by_ref().take(n).map(|h| Row::Hit { group: g, hit: h.clone() }));
        if kind == Kind::All && group.total > shown {
            out.push(Row::More { group: g, n: group.total - shown });
        }
    }
    out
}

/// Where the cursor starts: on the Ask row for a question, else on the first hit.
pub fn start(query: &str, rows: &[Row]) -> usize {
    let (_, q) = parse(query);
    let ask = shape(q) == Shape::Question;
    rows.iter().position(|r| if ask { matches!(r, Row::Ask { .. }) } else { matches!(r, Row::Hit { .. }) }).or_else(|| rows.iter().position(Row::selectable)).unwrap_or(0)
}

/// The next selectable row from `at`, `by` rows on (negative: back), within the list.
pub fn step(rows: &[Row], at: usize, by: isize) -> usize {
    let selectable: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].selectable()).collect();
    if selectable.is_empty() {
        return 0;
    }
    let here = selectable.iter().position(|&i| i >= at).unwrap_or(selectable.len() - 1);
    selectable[(here as isize + by).clamp(0, selectable.len() as isize - 1) as usize]
}

/// The first line under the list: what can be searched, and what is still under way.
pub fn footer(status: &crate::helper::Status) -> String {
    let mut s = crate::tn!("search.indexed", status.len as u64);
    if status.state == crate::index::State::Building {
        s += &t!("search.building");
    }
    if status.texts > 0 || status.pending > 0 {
        s += &format!(" · {}", crate::tn!("search.texts", status.texts as u64));
    }
    if status.meaning {
        s += &format!(" · {}", t!("find.meaning_for", "n" => status.meaning_done));
    }
    if status.pending > 0 {
        s += &t!("search.reading", "n" => status.pending);
    }
    if status.paused {
        s += &format!(" · {}", t!("find.paused"));
    }
    s
}

/// The folders read, with `dir` added: what "Read this folder too" saves as `text_roots`. The
/// home folder stays when none were set, since that is what an empty list reads.
pub fn read_too(cfg: &SearchConfig, dir: &Path) -> Vec<PathBuf> {
    let mut roots = crate::store::roots(cfg);
    if !roots.iter().any(|r| r == dir) {
        roots.push(dir.to_path_buf());
    }
    roots
}

/// "Read this folder too": `dir` added to the folders read in config.toml, and the config as
/// it is now. The helper reads it once it starts again.
pub fn save_read_too(cfg: &SearchConfig, dir: &Path) -> Result<crate::config::Config, String> {
    let roots: toml_edit::Array = read_too(cfg, dir).iter().map(|p| p.display().to_string()).collect();
    crate::config::Config::save_value(&["search", "text_roots"], toml_edit::Value::Array(roots))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(p: &str) -> Hit {
        Hit { path: PathBuf::from(p), is_dir: false, snippet: Some(p.into()), similar: None }
    }

    fn paths(v: &[Hit]) -> Vec<String> {
        v.iter().map(|h| h.path.display().to_string()).collect()
    }

    #[test]
    fn find_parse_prefixes() {
        assert_eq!(parse("text: rocket fuel"), (Some(Kind::InFiles), "rocket fuel"));
        assert_eq!(parse("ABOUT:brændstof"), (Some(Kind::About), "brændstof"));
        assert_eq!(parse("? what does it cost"), (Some(Kind::Ask), "what does it cost"));
        assert_eq!(parse("a?c.txt"), (None, "a?c.txt"), "a ? in the middle stays a wildcard");
        assert_eq!(parse("ext:rs"), (None, "ext:rs"));
        assert_eq!(parse("texture"), (None, "texture"));
    }

    #[test]
    fn find_shape_of_queries() {
        for q in ["*.rs", "ext:md", "src/ foo", "!test", "foo|bar", "\"a b\"", "a?c", "file: x", "case: X"] {
            assert_eq!(shape(q), Shape::Syntax, "{q}");
        }
        assert_eq!(shape("rocket"), Shape::Short);
        assert_eq!(shape("rocket fuel"), Shape::Short);
        assert_eq!(shape("rocket fuel cost"), Shape::Long);
        assert_eq!(shape("what does the rocket fuel cost?"), Shape::Question);
        assert_eq!(shape("rocket?"), Shape::Question);
        assert_eq!(order(Shape::Long)[0], GroupId::InFiles);
        assert_eq!(order(Shape::Short)[0], GroupId::Names);
        // The Ask row: two words or more, or a question; never name syntax, never one word.
        assert!(!asks("rocket") && asks("rocket fuel") && asks("rocket?") && !asks("*.rs") && asks("rocket fuel cost"));
    }

    #[test]
    fn find_fuse_by_rank() {
        let words = vec![hit("/a"), hit("/b"), hit("/c")];
        let meaning = vec![hit("/c"), hit("/d"), hit("/r/@history/abc")];
        let all = |hits| Words { hits, every: usize::MAX };
        let (in_files, about, history) = fuse(all(&words), &meaning, WEIGHTS);
        // c has the words and is closest in meaning: it rises above a and b.
        assert_eq!(paths(&in_files), ["/c", "/a", "/b"]);
        assert_eq!(paths(&about), ["/d"], "found by meaning alone");
        assert!(in_files[0].snippet.as_deref() == Some("/c"), "the word snippet is kept");
        assert_eq!(paths(&history), ["/r/@history/abc"], "a commit goes to History");
        // A file with only some of the words stays when meaning finds it too, or meaning is off.
        let some = |every| Words { hits: &words, every };
        assert_eq!(paths(&fuse(some(1), &[hit("/b")], WEIGHTS).0), ["/b", "/a"]);
        assert_eq!(paths(&fuse(some(1), &[], WEIGHTS).0), ["/a", "/b", "/c"]);
        // Ties keep the word order.
        let (in_files, ..) = fuse(all(&words), &[], WEIGHTS);
        assert_eq!(paths(&in_files), ["/a", "/b", "/c"]);
    }

    #[test]
    fn find_rows_follow_the_shape() {
        let found = Found {
            names: Group { hits: (0..7).map(|i| hit(&format!("/n{i}"))).collect(), total: 9 },
            in_files: Group { hits: vec![hit("/w")], total: 1 },
            off: vec![Off::MeaningOff],
            ..Found::default()
        };
        let none = |_: &str| false;
        let r = rows("rocket", Kind::All, &found, Ok(()), none);
        assert!(matches!(r[0], Row::Head { group: GroupId::Names, shown: 5, total: 9 }), "{r:?}");
        assert_eq!(r.iter().filter(|r| matches!(r, Row::Hit { group: GroupId::Names, .. })).count(), 5);
        assert!(r.contains(&Row::More { group: GroupId::Names, n: 4 }));
        assert!(r.contains(&Row::Off { group: GroupId::About, off: Off::MeaningOff }));
        assert!(!r.iter().any(|r| matches!(r, Row::Ask { .. })), "one word: no Ask row");
        assert_eq!(start("rocket", &r), 1);
        // Long: words first, the Ask row on top; a question starts on it.
        let r = rows("rocket fuel cost", Kind::All, &found, Err(Off::AskNoModel), none);
        assert_eq!(r[0], Row::Ask { off: Some(Off::AskNoModel) });
        assert!(matches!(r[1], Row::Head { group: GroupId::InFiles, .. }));
        assert_eq!(start("rocket fuel cost", &r), 2);
        assert_eq!(start("rocket fuel cost?", &rows("rocket fuel cost?", Kind::All, &found, Ok(()), none)), 0);
        // Dismissed tips go, in All.
        let r = rows("rocket fuel cost", Kind::All, &found, Err(Off::AskNoModel), |id| id == "find-ask" || id == "find-about");
        assert!(!r.iter().any(|r| matches!(r, Row::Ask { .. } | Row::Off { .. })), "{r:?}");
        // One chip: its group alone, all of it; the Names chip never asks.
        let r = rows("rocket fuel", Kind::Names, &found, Ok(()), none);
        assert_eq!(r.iter().filter(|r| matches!(r, Row::Hit { .. })).count(), 7);
        assert!(!r.iter().any(|r| matches!(r, Row::Ask { .. } | Row::More { .. })));
        // A prefix wins over the chip.
        let r = rows("text: rocket", Kind::Names, &found, Ok(()), none);
        assert!(r.iter().all(|r| !matches!(r, Row::Hit { group: GroupId::Names, .. })));
        // The cursor steps over headings.
        let r = rows("rocket", Kind::All, &found, Ok(()), none);
        let after_names = r.iter().position(|r| matches!(r, Row::More { .. })).unwrap();
        assert!(matches!(r[step(&r, after_names, 1)], Row::Hit { group: GroupId::InFiles, .. }));
        assert_eq!(step(&r, 1, -5), 1);
    }

    #[test]
    fn find_ask_ready_and_read_too() {
        let mut cfg = SearchConfig::default();
        assert_eq!(ask_ready(&cfg), Err(Off::AskNeedsMeaning));
        cfg.meaning = true;
        assert_eq!(ask_ready(&cfg), Err(Off::AskNoModel));
        cfg.ask_model = "qwen3:8b".into();
        assert_eq!(ask_ready(&cfg), Ok(()));
        cfg.text_roots = vec![PathBuf::from("/home/u")];
        assert_eq!(read_too(&cfg, Path::new("/mnt/a")), [PathBuf::from("/home/u"), PathBuf::from("/mnt/a")]);
        assert_eq!(read_too(&cfg, Path::new("/home/u")).len(), 1);
    }
}

//! The built-in viewer (F3 on a file, when `viewer` is not set): the file full screen,
//! highlighted in the theme's colours as the desktop app's preview colours code
//! (`coxswain_core::view::TOKENS`), with line numbers, wrapping, search and go to line.
//!
//! A big file is never read whole: a thread finds where its lines start
//! (`coxswain_core::logs::Lines`), and only the lines on screen are read, so a 100 MB log opens
//! at once. Highlighting starts from the nearest point it has passed before (every `STEP`
//! lines), and a file over `PLAIN_OVER` is shown plain. The grammar is the one
//! `coxswain_core::code::language` names (the desktop app's), looked up among syntect's and
//! two-face's; else the file's extension, else its first line.
//!
//! A log (by its name, or a format its first lines are in) is coloured by level instead, and
//! gets the log keys: **F** follows it as it is written, **e** shows warnings and errors, then
//! errors, **&** keeps the lines with a text (or a field's value), **c** shows its fields as
//! columns, **z** folds stack traces under their line. **F** and **&** work on any text.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::logs::{self, Format, Grew, Level};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use crate::ui::Cols;
use unicode_width::UnicodeWidthChar;

use crate::bom::faint;
use crate::ui::{fit, sty};

pub(crate) use coxswain_core::logs::LONGEST_READ;

/// Files bigger than this are shown without colours: a log of that size is read for its
/// lines, not its syntax.
const PLAIN_OVER: u64 = 16 << 20;
/// A line longer than this is not highlighted (a minified file): it would take the grammar long.
const LONGEST_HIGHLIGHTED: usize = 4000;
/// Highlighting keeps where it stands every this many lines, to start from there again.
const STEP: usize = 64;
/// Further than this past the last point kept, highlighting starts afresh a little above the
/// screen, as editors do: a comment opened far above may then show as code.
const FARTHEST: usize = 4000;
/// Lines of a hex dump: the first 64 KB, as the desktop app's preview shows.
const HEX_LINES: usize = 4096;
/// Columns a tab takes.
const TAB: usize = 4;
/// How often a followed file is looked at for new lines.
const FOLLOW_EVERY: Duration = Duration::from_millis(250);


/// syntect's grammars and two-face's (about 200 languages), read once, on the first F3.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The grammars `code::language` names (highlight.js's names), each with an extension syntect
/// knows its own grammar by. One it lacks is left out, and its files are shown plain.
const SAME_AS: &[(&str, &str)] = &[
    ("actionscript", "as"), ("ada", "adb"), ("applescript", "applescript"), ("asciidoc", "adoc"),
    ("awk", "awk"), ("bash", "sh"), ("c", "c"), ("clojure", "clj"), ("cmake", "cmake"),
    ("coffeescript", "coffee"), ("cpp", "cpp"), ("crystal", "cr"), ("csharp", "cs"), ("css", "css"),
    ("d", "d"), ("dart", "dart"), ("delphi", "pas"), ("diff", "diff"), ("django", "j2"),
    ("dockerfile", "Dockerfile"), ("dos", "bat"), ("elixir", "ex"), ("elm", "elm"), ("erb", "erb"),
    ("erlang", "erl"), ("fortran", "f90"), ("fsharp", "fsx"), ("glsl", "glsl"), ("go", "go"),
    ("gradle", "gradle"), ("graphql", "graphql"), ("groovy", "groovy"), ("haml", "haml"),
    ("haskell", "hs"), ("http", "http"), ("ini", "ini"), ("java", "java"), ("javascript", "js"),
    ("json", "json"), ("julia", "jl"), ("kotlin", "kt"), ("latex", "tex"), ("less", "less"),
    ("lisp", "lisp"), ("llvm", "ll"), ("lua", "lua"), ("makefile", "mk"), ("markdown", "md"),
    ("matlab", "matlab"), ("nginx", "nginx.conf"), ("nim", "nim"), ("nix", "nix"), ("nsis", "nsi"),
    ("objectivec", "m"), ("ocaml", "ml"), ("perl", "pl"), ("php", "php"), ("properties", "properties"),
    ("protobuf", "proto"), ("puppet", "pp"), ("python", "py"), ("qml", "qml"), ("r", "r"),
    ("ruby", "rb"), ("rust", "rs"), ("scala", "scala"), ("scheme", "rkt"), ("scss", "scss"),
    ("sml", "sml"), ("solidity", "sol"), ("sql", "sql"), ("stylus", "styl"), ("svelte", "svelte"),
    ("swift", "swift"), ("tcl", "tcl"), ("terraform", "tf"), ("twig", "twig"),
    ("typescript", "ts"), ("verilog", "v"), ("vhdl", "vhd"), ("vim", "vim"), ("x86asm", "asm"),
    ("xml", "xml"), ("yaml", "yaml"), ("zig", "zig"), ("apache", "htaccess"),
];

/// The grammar for a file: by its extension when syntect has one for it and no other language
/// shares it; else the one the desktop app takes (`code::language`, which reads the text where
/// languages share an extension); else by its first line. `None` for plain text, and for the
/// languages only the desktop app has a grammar for (COBOL, BASIC, PowerShell, Prolog, Coq,
/// Smalltalk, AutoHotkey, ARM and MIPS assembly).
pub fn syntax_for(path: &Path, head: &str) -> Option<&'static SyntaxReference> {
    let ss = syntaxes();
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let shared = coxswain_core::code::SHARED.contains(&ext.as_str());
    let by_ext = || (!ext.is_empty() && !shared).then(|| ss.find_syntax_by_extension(&ext)).flatten();
    let by_lang = || {
        let l = coxswain_core::code::language(&path.to_string_lossy(), head)?;
        let ext = SAME_AS.iter().find(|(h, _)| *h == l)?.1;
        ss.find_syntax_by_extension(ext)
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    by_ext()
        .or_else(|| ss.find_syntax_by_extension(&name))
        .or_else(by_lang)
        .or_else(|| ss.find_syntax_by_first_line(head))
        .filter(|s| s.name != "Plain Text")
}

/// A line in pieces, each with the theme slot it is coloured with.
pub(crate) type Pieces = Vec<(Option<&'static str>, String)>;

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Close, and open the file in the editor (`Edit`) or the built-in one (`EditHere`).
    Edit(PathBuf, Action),
    /// Close, and open the file in the pager (`$PAGER`, else less).
    Pager(PathBuf),
    /// Show the file a text copy was made of as it is (`made_of`), this one again after it.
    Original(PathBuf),
}

/// What the file is read as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Utf8,
    Latin1,
    Utf16,
    Binary,
}

/// The file's text: lines read from the disk as they are shown, or held whole.
enum Body {
    /// Its lines found by a thread, as a log when it is one (`logs::View`).
    File(logs::View),
    /// UTF-16, decoded whole, and a hex dump.
    Whole(Vec<String>),
}

/// A line typed at the bottom: a search, a line number, or a filter.
enum Typing {
    Search(String),
    Line(String),
    Filter(String),
}

/// Highlighting's state before a line: the grammar's, and the scopes open there.
#[derive(Clone)]
pub(crate) struct Point {
    parse: ParseState,
    scopes: ScopeStack,
}

impl Point {
    /// Before the first line.
    pub(crate) fn new(syntax: &SyntaxReference) -> Point {
        Point { parse: ParseState::new(syntax), scopes: ScopeStack::new() }
    }
}

/// The theme slot each scope takes, looked up once.
pub(crate) type Slots = HashMap<Scope, Option<&'static str>>;

/// One line's pieces, from `point`, which moves past it.
pub(crate) fn paint(point: &mut Point, line: &str, slots: &mut Slots) -> Pieces {
    if line.len() > LONGEST_HIGHLIGHTED {
        return vec![(None, line.to_owned())];
    }
    let ss = syntaxes();
    let with_end = format!("{line}\n");
    let Ok(ops) = point.parse.parse_line(&with_end, ss) else {
        return vec![(None, line.to_owned())];
    };
    let mut out = vec![];
    let mut from = 0;
    // The slot of the innermost scope open that has one.
    let mut slot = |scopes: &ScopeStack| {
        scopes.as_slice().iter().rev().find_map(|s| *slots.entry(*s).or_insert_with(|| coxswain_core::view::slot_of_scope(&s.build_string())))
    };
    for (at, op) in ops.into_iter().chain([(line.len(), syntect::parsing::ScopeStackOp::Noop)]) {
        let at = at.min(line.len());
        if at > from {
            out.push((slot(&point.scopes), line[from..at].to_owned()));
            from = at;
        }
        let _ = point.scopes.apply(&op);
    }
    out
}

/// A search going on: the rows `a..b` still to look through (from the end for `back`), of
/// `from`, the whole stretch.
struct Seek {
    q: String,
    a: usize,
    b: usize,
    back: bool,
    from: (usize, usize),
}

/// A row on the screen: the line it shows (from 0), its pieces, and lines folded under it.
type Shown = (usize, Pieces, u32);
/// What the rows drawn last were drawn for: first row, rows, rows known, and the two eras.
type ShownFor = (usize, usize, usize, u64, u64);

pub struct Viewer {
    pub path: PathBuf,
    title: String,
    /// What the title calls the text, for a text copy of another file (its format); else the
    /// highlighting's language, or plain text.
    kind: Option<String>,
    /// A text copy's file (`made_of`): **h** shows it as it is, a hex dump for a binary one.
    original: Option<PathBuf>,
    body: Body,
    encoding: Encoding,
    syntax: Option<&'static SyntaxReference>,
    /// A log's fields as columns (**c**).
    columns: bool,
    /// Following the file as it is written (**F**), and stopping at an error that comes (**E**).
    follow: bool,
    pause_on_error: bool,
    /// When the followed file was last looked at, and the lines its errors were looked for in.
    grown: Instant,
    followed_to: usize,
    /// The line the screen showed at its top before the rows changed: the top again once
    /// the new rows reach it.
    keep_line: Option<usize>,
    /// Points highlighting has passed, every `STEP` lines from the start: index i is line i·STEP.
    points: Vec<Point>,
    slots: Slots,
    /// Bumped when the columns change, so the rows drawn last are not taken again.
    era: u64,
    /// The rows last drawn: (first, count, rows known then, eras) and the rows.
    shown: Option<(ShownFor, Vec<Shown>)>,
    /// The first row on the screen.
    top: usize,
    left: usize,
    numbers: bool,
    wrap: bool,
    typing: Option<Typing>,
    /// The last search, and the row it was found on.
    query: String,
    found: Option<usize>,
    /// A search still going on.
    seek: Option<Seek>,
    said: Option<String>,
    /// Rows of text on the screen, as last drawn: what PgUp and PgDn move.
    page: usize,
    key_line: String,
    /// Who changed each line, and a function's history (**b**, **f**), for a file in a repository.
    git: Option<crate::blame::Git>,
    /// A function's history, shown over this file until Esc.
    sub: Option<Box<Viewer>>,
}

impl Viewer {
    /// Open `path`: its first bytes say how it is read; the rest is read as it is shown.
    pub fn open(path: &Path, labels: [&str; 2]) -> std::io::Result<Viewer> {
        let file = logs::open_file(path)?;
        let len = file.metadata()?.len();
        let mut head = vec![0u8; 8192];
        let n = logs::read_at(&file, &mut head, 0)?;
        head.truncate(n);
        let utf16 = head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]);
        let binary = !utf16 && head.contains(&0);
        let mut cut = false;
        let (body, encoding, text_head) = if binary || utf16 {
            let most = if binary { (HEX_LINES * 16) as u64 } else { PLAIN_OVER };
            let mut all = vec![0u8; len.min(most) as usize];
            let n = logs::read_at(&file, &mut all, 0)?;
            all.truncate(n);
            let lines: Vec<String> = match coxswain_core::view::decode(&all).filter(|_| utf16) {
                Some(text) => text.lines().map(str::to_owned).collect(),
                None => coxswain_core::view::hex(&all, HEX_LINES),
            };
            let enc = if utf16 { Encoding::Utf16 } else { Encoding::Binary };
            if utf16 && len > PLAIN_OVER {
                cut = true;
            }
            let head = lines.iter().take(20).cloned().collect::<Vec<_>>().join("\n");
            (Body::Whole(lines), enc, head)
        } else {
            let text = coxswain_core::view::decode(&head).unwrap_or_default();
            // The start says it for the whole file, as each line is then read.
            let utf8 = coxswain_core::view::is_utf8(&head);
            let enc = if utf8 { Encoding::Utf8 } else { Encoding::Latin1 };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (Body::File(logs::View::new(logs::Lines::new(path, file, utf8)?, &name, &text, len)), enc, text)
        };
        let log = matches!(&body, Body::File(v) if v.log);
        let syntax = (!log && encoding != Encoding::Binary && len <= PLAIN_OVER).then(|| syntax_for(path, &text_head)).flatten();
        let git = matches!(body, Body::File(_)) && path.parent().is_some_and(coxswain_core::blame::in_repository);
        let [edit, view] = labels;
        let mut v = Viewer {
            title: path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
            kind: None,
            original: None,
            path: path.to_path_buf(),
            body,
            encoding,
            points: syntax.map(|s| vec![Point::new(s)]).unwrap_or_default(),
            syntax,
            columns: true,
            follow: false,
            pause_on_error: false,
            grown: Instant::now(),
            followed_to: 0,
            keep_line: None,
            slots: HashMap::new(),
            era: 0,
            shown: None,
            top: 0,
            left: 0,
            numbers: true,
            wrap: false,
            typing: None,
            query: String::new(),
            found: None,
            seek: None,
            said: if cut {
                Some(t!("viewer.first_part"))
            } else {
                (len > PLAIN_OVER && matches!(encoding, Encoding::Utf8 | Encoding::Latin1)).then(|| t!("viewer.plain_big"))
            },
            page: 20,
            key_line: t!("viewer.keys", "edit" => edit, "view" => view),
            git: git.then(Default::default),
            sub: None,
        };
        if matches!(v.body, Body::File(_)) {
            let keys = if log { t!("viewer.keys_log") } else { t!("viewer.keys_follow") };
            v.key_line = format!("{keys} · {}", v.key_line);
        }
        Ok(v)
    }

    /// Long lines wrapped from the start, as **w** does: a text of paragraphs, not of lines.
    pub fn wrapped(mut self) -> Viewer {
        self.wrap = true;
        self
    }

    /// The keys of blame and a function's history on the key line, for a file in a repository.
    pub fn git_keys(mut self, blame: &str, history: &str) -> Viewer {
        if self.git.is_some() {
            self.key_line = format!("{} · {}", self.key_line, t!("viewer.keys_git", "blame" => blame, "history" => history));
        }
        self
    }

    /// Who changed each line shown, or put away.
    pub fn blame(&mut self) {
        let path = self.path.clone();
        if let Some(g) = &mut self.git {
            self.said = Some(g.toggle_blame(&path));
        }
    }

    /// Text held whole (a function's history), coloured as a diff.
    pub fn text(title: String, text: &str) -> Viewer {
        let ss = syntaxes();
        let syntax = ss.find_syntax_by_extension("diff");
        Viewer {
            title,
            kind: None,
            original: None,
            path: PathBuf::new(),
            body: Body::Whole(text.lines().map(str::to_owned).collect()),
            encoding: Encoding::Utf8,
            points: syntax.map(|s| vec![Point::new(s)]).unwrap_or_default(),
            syntax,
            slots: HashMap::new(),
            shown: None,
            top: 0,
            left: 0,
            numbers: false,
            wrap: false,
            typing: None,
            query: String::new(),
            found: None,
            seek: None,
            said: None,
            page: 20,
            key_line: t!("viewer.keys_history"),
            git: None,
            sub: None,
        }
    }

    /// A text copy shown as the file it was made of: its name and format in the title.
    pub fn made_of(mut self, of: &Path, kind: String) -> Viewer {
        self.title = of.file_name().map_or_else(|| of.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.kind = Some(kind);
        self.original = Some(of.to_path_buf());
        // Its lines are not the file's: no blame.
        self.git = None;
        self.key_line = format!("{} · {}", t!("viewer.keys_original"), self.key_line);
        self
    }

    /// Whether it shows a log (by its name or its lines), with the log keys.
    pub fn is_log(&self) -> bool {
        matches!(&self.body, Body::File(v) if v.log)
    }

    /// The log's view, when the file is read by lines and is a log.
    fn log(&self) -> Option<&logs::View> {
        match &self.body {
            Body::File(v) if v.log => Some(v),
            _ => None,
        }
    }

    /// A log shown in columns.
    fn in_columns(&self) -> bool {
        self.columns && self.log().is_some_and(|v| v.format.has_fields())
    }

    /// Follow the file from the start, as **F** does (the action *Follow log*).
    pub fn following(mut self) -> Viewer {
        self.set_follow(true);
        self
    }

    /// Say `text` under the file until the next key (a hint).
    pub fn saying(mut self, text: String) -> Viewer {
        if self.said.is_none() {
            self.said = Some(text);
        }
        self
    }

    /// Rows known so far, and whether that is all of them.
    fn count(&self) -> (usize, bool) {
        match &self.body {
            Body::File(v) => v.count(),
            Body::Whole(lines) => (lines.len(), true),
        }
    }

    /// The lines of rows `a..b` (as many as there are), the level of each in a log, and the
    /// lines folded under each.
    fn rows_at(&self, a: usize, b: usize) -> Vec<(usize, Option<Level>, u32)> {
        match &self.body {
            Body::File(v) => v.rows(a, b),
            Body::Whole(lines) => (a.min(lines.len())..b.min(lines.len())).map(|i| (i, None, 0)).collect(),
        }
    }

    /// The text of rows `a..b` (as many of them as there are), read from the disk or held;
    /// each at most `most` bytes, a longer one cut with `…`.
    fn lines(&self, a: usize, b: usize, most: u64) -> Vec<String> {
        match &self.body {
            Body::Whole(lines) => lines.get(a.min(lines.len())..b.min(lines.len())).unwrap_or_default().to_vec(),
            Body::File(v) => v.texts(a, b, most),
        }
    }

    /// Rows `a..a+n` in pieces, each with the theme slot it is coloured with: a log by its
    /// level (or in columns), else by its grammar.
    fn highlighted(&mut self, a: usize, n: usize) -> Vec<Shown> {
        let view_era = if let Body::File(v) = &self.body { v.era() } else { 0 };
        let key = (a, n, self.count().0, self.era, view_era);
        if let Some((at, rows)) = &self.shown
            && *at == key
        {
            return rows.clone();
        }
        let filtered = matches!(&self.body, Body::File(v) if v.filtered());
        let out = match self.syntax {
            Some(syntax) if !filtered => self.code_rows(syntax, a, n),
            _ => self.log_rows(a, n),
        };
        self.shown = Some((key, out.clone()));
        out
    }

    /// Rows of a log, or of a text kept by a filter: coloured by level, or its fields as
    /// columns; with the lines folded under each said.
    fn log_rows(&self, a: usize, n: usize) -> Vec<Shown> {
        let rows = self.rows_at(a, a + n);
        let texts = self.lines(a, a + n, LONGEST_READ);
        rows.into_iter()
            .zip(texts)
            .map(|((line, level, folded), text)| {
                let slot = level.map(Level::slot);
                let mut pieces = match self.log() {
                    Some(log) if self.in_columns() => in_columns(log, &text, slot),
                    _ => vec![(slot, text)],
                };
                if folded > 0 {
                    pieces.push((Some("hidden"), format!("  ▸ +{}", tn!("viewer.lines", folded as usize))));
                }
                (line, pieces, folded)
            })
            .collect()
    }

    /// Lines `a..a+n` of code, highlighted by its grammar.
    fn code_rows(&mut self, syntax: &'static SyntaxReference, a: usize, n: usize) -> Vec<Shown> {
        // Start from the last point kept at or above `a`, or afresh a little above it.
        let kept = (a / STEP).min(self.points.len() - 1);
        let (mut at, mut point, exact) = if a - kept * STEP <= FARTHEST {
            (kept * STEP, self.points[kept].clone(), true)
        } else {
            let from = a.saturating_sub(STEP);
            (from, Point::new(syntax), false)
        };
        // Lines above the screen only lead up to it: a line too long to highlight is not read whole.
        let mut text = self.lines(at, a, LONGEST_HIGHLIGHTED as u64 + 1);
        text.extend(self.lines(a, a + n, LONGEST_READ));
        let mut out = vec![];
        for line in text {
            if exact && at % STEP == 0 && at / STEP == self.points.len() {
                self.points.push(point.clone());
            }
            let pieces = paint(&mut point, &line, &mut self.slots);
            if at >= a {
                out.push((at, pieces, 0));
            }
            at += 1;
        }
        out
    }

    /// Keep and fold by `shape`; the line at the top stays at the top once the new rows reach it.
    fn reshape(&mut self, shape: logs::Shape) {
        let top_line = self.rows_at(self.top, self.top + 1).first().map(|r| r.0);
        let Body::File(v) = &mut self.body else { return };
        v.set_shape(shape);
        let filtered = v.filtered();
        self.found = None;
        self.seek = None;
        match top_line {
            Some(l) if filtered => (self.top, self.keep_line) = (0, Some(l)),
            Some(l) => (self.top, self.keep_line) = (l, None),
            None => self.keep_line = None,
        }
        if self.follow {
            self.go_to_end();
        }
    }

    /// What is kept and folded now.
    fn shape(&self) -> logs::Shape {
        match &self.body {
            Body::File(v) => v.shape().clone(),
            Body::Whole(_) => logs::Shape::default(),
        }
    }

    /// The file changed since its lines were found (a log written to, rotated): find the new
    /// ones, or all of them again. On End, and as it is followed.
    fn recheck(&mut self) {
        let Body::File(v) = &mut self.body else { return };
        match v.grow() {
            Grew::No => return,
            Grew::More => {}
            Grew::Anew => {
                self.points.truncate(1);
                (self.top, self.found, self.followed_to) = (0, None, 0);
            }
        }
        // Wait a moment for the end, so End goes there; a big file catches up on the next draws.
        let Body::File(v) = &self.body else { return };
        for _ in 0..50 {
            if v.reader().count().1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The last screen.
    fn go_to_end(&mut self) {
        let (n, _) = self.count();
        self.top = n.saturating_sub(self.page.max(1));
    }

    fn set_follow(&mut self, on: bool) {
        if !matches!(self.body, Body::File(_)) {
            return;
        }
        self.follow = on;
        if on {
            self.recheck();
            self.followed_to = self.lines_known();
            self.go_to_end();
            self.said = Some(t!("viewer.following"));
        } else {
            self.said = Some(t!("viewer.not_following"));
        }
    }

    /// Following: new lines found, the screen at the end, unless an error came and the pause
    /// on errors is on, which stops there.
    fn follow_on(&mut self) {
        if self.grown.elapsed() < FOLLOW_EVERY {
            return;
        }
        self.grown = Instant::now();
        self.recheck();
        let hit = match &self.body {
            Body::File(v) if self.pause_on_error && self.lines_known() > self.followed_to => v.first_error(self.followed_to),
            _ => None,
        };
        if let Some(row) = hit {
            let line = self.rows_at(row, row + 1).first().map_or(row, |r| r.0);
            self.follow = false;
            self.found = Some(row);
            self.top = row.saturating_sub(self.page / 3);
            self.said = Some(t!("viewer.stopped_at_error", "line" => line + 1));
            return;
        }
        self.followed_to = self.lines_known();
        self.go_to_end();
    }

    /// Lines of the file found so far.
    fn lines_known(&self) -> usize {
        match &self.body {
            Body::File(v) => v.reader().count().0,
            Body::Whole(lines) => lines.len(),
        }
    }

    /// Scroll by `by` rows (the mouse wheel, the arrows).
    pub fn scroll(&mut self, by: isize) {
        let (n, _) = self.count();
        self.top = (self.top as isize + by).clamp(0, n.saturating_sub(1) as isize) as usize;
        if by < 0 && self.follow {
            self.follow = false;
            self.said = Some(t!("viewer.not_following"));
        }
    }

    /// The far end of a batch of rows for `find`, starting at `from` and going towards `limit`
    /// (`forward`, or backwards): at most a few thousand lines and a few megabytes, but always
    /// one row.
    fn batch(&self, from: usize, forward: bool, limit: usize) -> usize {
        match &self.body {
            Body::File(v) => v.batch(from, forward, limit),
            Body::Whole(_) if forward => (from + 4096).min(limit),
            Body::Whole(_) => from.saturating_sub(4096).max(limit),
        }
    }

    /// Start a search for the next row (or the one before, `back`) after the screen's first
    /// with the search on it, case aside. It goes on a batch at a time in `poll`, so a big file
    /// never holds the screen still, and Esc stops it.
    fn find(&mut self, back: bool) {
        if self.query.is_empty() {
            return;
        }
        let (n, _) = self.count();
        // A new search starts at the screen's first line; `n` and `N` go on from the hit.
        let (a, b) = match (self.found, back) {
            (Some(f), false) => (f + 1, n),
            (None, false) => (self.top, n),
            (f, true) => (0, f.unwrap_or(self.top)),
        };
        self.seek = Some(Seek { q: self.query.to_lowercase(), a, b, back, from: (a, b) });
        self.poll();
    }

    /// Whether a search is still going on, so the screen is drawn again soon; or the rows a
    /// filter keeps are still being found, or the file is followed.
    pub fn busy(&self) -> bool {
        self.seek.is_some()
            || self.follow
            || matches!(&self.body, Body::File(v) if v.filtered() && !v.count().1)
            || self.git.as_ref().is_some_and(crate::blame::Git::busy)
            || self.sub.as_ref().is_some_and(|v| v.busy())
    }

    /// The search going on, for a few dozen milliseconds: rows in batches of at most a few
    /// thousand and a few megabytes (each line read to a megabyte at most), from the search's
    /// start onwards, or backwards for `N`. A followed file is looked at for new lines.
    pub fn poll(&mut self) {
        if let (Some(line), Body::File(v)) = (self.keep_line, &self.body)
            && let Some(row) = v.row_of(line)
        {
            self.top = row;
            self.keep_line = None;
        }
        if let Some(v) = &mut self.sub {
            v.poll();
        }
        match self.git.as_mut().map(crate::blame::Git::poll) {
            Some(crate::blame::Came::Said(s)) => self.said = Some(s),
            Some(crate::blame::Came::History(h)) => {
                let title = crate::blame::title(&h, &self.path);
                self.sub = Some(Box::new(Viewer::text(title, &h.text())));
            }
            _ => {}
        }
        if self.follow {
            self.follow_on();
        }
        let Some(mut s) = self.seek.take() else { return };
        let until = Instant::now() + Duration::from_millis(40);
        while s.a < s.b {
            let (lo, hi) = if s.back { (self.batch(s.b, false, s.a), s.b) } else { (s.a, self.batch(s.a, true, s.b)) };
            let lines = self.lines(lo, hi, LONGEST_READ);
            let has = |i: usize| lines.get(i - lo).is_some_and(|l| l.to_lowercase().contains(&s.q));
            let hit = if s.back { (lo..hi).rev().find(|&i| has(i)) } else { (lo..hi).find(|&i| has(i)) };
            if let Some(i) = hit {
                self.found = Some(i);
                self.top = i.saturating_sub(self.page / 3);
                self.said = None;
                self.follow = false;
                return;
            }
            if s.back { s.b = lo } else { s.a = hi }
            if Instant::now() >= until && s.a < s.b {
                let (a, b) = s.from;
                let pct = 100 * (b - a - (s.b - s.a)) / (b - a).max(1);
                self.said = Some(t!("viewer.searching", "text" => &self.query, "pct" => pct));
                self.seek = Some(s);
                return;
            }
        }
        let (n, done) = self.count();
        // The lines are still being found (a big file just opened): the search goes on over
        // them as they come, rather than stopping at the ones found so far.
        if !s.back && (!done || s.b < n) {
            (s.b, s.from.1) = (n, n);
            self.said = Some(t!("viewer.still_reading"));
            self.seek = Some(s);
            return;
        }
        self.said = Some(if done { t!("viewer.not_found", "text" => &self.query) } else { t!("viewer.still_reading") });
    }

    /// What the log keys have set, for the line under the text: following, the level and
    /// filter kept, traces folded; empty when nothing is.
    fn state(&self) -> String {
        let mut out = vec![];
        if self.follow {
            out.push(t!("viewer.state_following"));
        }
        let shape = self.shape();
        if let Some(l) = shape.level {
            out.push(t!(&format!("viewer.level_{}", l.name())));
        }
        let typed = shape.typed_text();
        if !typed.is_empty() {
            out.push(t!("viewer.state_filter", "filter" => typed));
        }
        if self.is_log() && shape.fold {
            out.push(t!("viewer.state_folded"));
        }
        if self.pause_on_error {
            out.push(t!("viewer.state_pause"));
        }
        out.join(" · ")
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        // A function's history over the file: its keys are its own, Esc comes back here.
        if let Some(v) = &mut self.sub {
            if let Outcome::Close = v.key(key, action) {
                self.sub = None;
            }
            return Outcome::Stay;
        }
        if let Some(typing) = &mut self.typing {
            let field = match typing {
                Typing::Search(s) | Typing::Line(s) | Typing::Filter(s) => s,
            };
            match key.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Backspace => {
                    field.pop();
                }
                // Tab finishes a field's name, from the log's columns.
                KeyCode::Tab => {
                    if let (Typing::Filter(s), Body::File(log)) = (&mut *typing, &self.body)
                        && let Some((name, _)) = log.columns.iter().find(|(c, _)| !s.is_empty() && !s.contains('=') && c.to_lowercase().starts_with(&s.to_lowercase()))
                    {
                        *s = format!("{name}=");
                    }
                }
                KeyCode::Enter => match self.typing.take() {
                    Some(Typing::Search(s)) => {
                        self.query = s;
                        self.found = None;
                        self.find(false);
                    }
                    Some(Typing::Line(s)) => match s.trim().parse::<usize>() {
                        Ok(n) if n > 0 => {
                            // A line a filter left out: the row of the line above it.
                            self.top = match &self.body {
                                Body::File(v) => v.row_of(n - 1).unwrap_or(0),
                                Body::Whole(_) => (n - 1).min(self.count().0.saturating_sub(1)),
                            };
                            self.found = None;
                            self.follow = false;
                        }
                        _ => {}
                    },
                    Some(Typing::Filter(s)) => {
                        let format = self.log().map_or(Format::Plain, |l| l.format.clone());
                        let mut shape = self.shape();
                        shape.typed(&format, &s);
                        let said = if s.trim().is_empty() { t!("viewer.filter_off") } else { t!("viewer.filtered", "filter" => shape.typed_text()) };
                        self.reshape(shape);
                        self.said = Some(said);
                    }
                    None => {}
                },
                KeyCode::Char(c) if !key.ctrl && !key.alt => field.push(c),
                _ => {}
            }
            return Outcome::Stay;
        }
        // Esc stops a search going on, rather than close the file.
        if key.code == KeyCode::Esc && self.seek.take().is_some() {
            self.said = None;
            return Outcome::Stay;
        }
        // And blame or a history being read.
        if key.code == KeyCode::Esc && self.git.as_mut().is_some_and(crate::blame::Git::stop) {
            self.said = Some(t!("blame.stopped"));
            return Outcome::Stay;
        }
        self.seek = None;
        self.said = None;
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
            _ => None,
        };
        let page = self.page.max(1) as isize;
        let file = matches!(self.body, Body::File(_));
        let log = self.is_log();
        match (key.code, ch) {
            (KeyCode::Esc, _) => return Outcome::Close,
            _ if matches!(action, Some(Action::Quit | Action::View)) => return Outcome::Close,
            // A text copy is not what is edited: its file is (the editor says when it is no text).
            _ if matches!(action, Some(Action::Edit | Action::EditHere)) => {
                return Outcome::Edit(self.original.clone().unwrap_or_else(|| self.path.clone()), action.unwrap());
            }
            (KeyCode::Up, _) => self.scroll(-1),
            (KeyCode::Down, _) | (KeyCode::Enter, _) => self.scroll(1),
            (KeyCode::PageUp, _) => self.scroll(-page),
            (KeyCode::PageDown, _) | (_, Some(' ')) => self.scroll(page),
            (KeyCode::Home, _) => {
                (self.top, self.left) = (0, 0);
                self.follow = false;
            }
            (KeyCode::End, _) => {
                self.recheck();
                self.go_to_end();
            }
            (KeyCode::Left, _) if !self.wrap => self.left = self.left.saturating_sub(8),
            (KeyCode::Right, _) if !self.wrap => self.left += 8,
            (KeyCode::F(7), _) if key.shift => self.find(true),
            (KeyCode::F(7), _) | (_, Some('/')) => self.typing = Some(Typing::Search(String::new())),
            (_, Some('n')) => self.find(false),
            (_, Some('N')) => self.find(true),
            (_, Some('g')) => self.typing = Some(Typing::Line(String::new())),
            (_, Some('l')) => self.numbers = !self.numbers,
            (_, Some('w')) => {
                self.wrap = !self.wrap;
                self.left = 0;
            }
            (_, Some('p')) => return Outcome::Pager(self.path.clone()),
            (_, Some('h')) if self.original.is_some() => return Outcome::Original(self.original.clone().unwrap_or_default()),
            (_, Some('F')) if file => self.set_follow(!self.follow),
            (_, Some('&')) if file => self.typing = Some(Typing::Filter(self.shape().typed_text())),
            (_, Some('E')) if file => {
                self.pause_on_error = !self.pause_on_error;
                self.said = Some(if self.pause_on_error { t!("viewer.pause_on") } else { t!("viewer.pause_off") });
            }
            (_, Some('e')) if log => {
                let mut shape = self.shape();
                shape.level = Level::next_filter(shape.level);
                self.said = Some(t!(&format!("viewer.level_{}", shape.level.map_or("all", Level::name))));
                self.reshape(shape);
            }
            (_, Some('z')) if log => {
                let mut shape = self.shape();
                shape.fold = !shape.fold;
                self.said = Some(if shape.fold { t!("viewer.fold_on") } else { t!("viewer.fold_off") });
                self.reshape(shape);
            }
            (_, Some('c')) if self.log().is_some_and(|l| l.format.has_fields()) => {
                self.columns = !self.columns;
                self.left = 0;
                self.era += 1;
            }
            _ if action == Some(Action::Blame) && self.git.is_some() => self.blame(),
            // The function at the line found or gone to, else at the top of the screen.
            _ if action == Some(Action::LineHistory) && self.git.is_some() => {
                let row = self.found.unwrap_or(self.top);
                let line = self.rows_at(row, row + 1).first().map_or(row, |r| r.0);
                let text = std::fs::read(&self.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                let path = self.path.clone();
                self.said = self.git.as_mut().map(|g| g.trace(&path, text, line, line));
            }
            _ if matches!(action, Some(Action::Blame | Action::LineHistory)) => self.said = Some(t!("blame.not_here")),
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        if let Some(v) = &mut self.sub {
            return v.draw(f, theme);
        }
        let area = f.area();
        let base = sty(&theme.panel);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let (count, done) = self.count();
        let grammar = self.kind.clone().unwrap_or_else(|| match self.log() {
            Some(log) => t!(&format!("logs.format.{}", log.format.id())),
            None => self.syntax.map_or_else(|| t!("viewer.plain"), |s| s.name.clone()),
        });
        let encoding = match self.encoding {
            Encoding::Utf8 => "UTF-8",
            Encoding::Latin1 => "Windows-1252",
            Encoding::Utf16 => "UTF-16",
            Encoding::Binary => "hex",
        };
        let (lines_known, lines_done, capped) = match &self.body {
            Body::File(v) => {
                let (n, d) = v.reader().count();
                (n, d, v.reader().capped())
            }
            Body::Whole(l) => (l.len(), true, false),
        };
        let lines_said = if capped {
            tn!("viewer.first_lines", lines_known)
        } else if lines_done {
            tn!("viewer.lines", lines_known)
        } else {
            tn!("viewer.lines_so_far", lines_known)
        };
        // Rows that are the lines (nothing left out or folded yet) are said as lines.
        let filtered = matches!(&self.body, Body::File(v) if v.filtered()) && !(done && count == lines_known);
        let lines_said = match (filtered, done) {
            (true, true) => t!("viewer.rows_of", "n" => count, "lines" => lines_said),
            (true, false) => t!("viewer.rows_so_far", "n" => count, "lines" => lines_said),
            _ => lines_said,
        };
        let title = format!(" {} · {grammar} · {encoding} · {lines_said} ", self.title);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.border).bg(bg))
            .style(base)
            .title(Line::from(title).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area);
        f.render_widget(block, area);
        // The key line on as many lines as it needs, three at most.
        let foot_h = self.key_line.width().div_ceil((inner.width as usize).max(1)).clamp(1, 3) as u16;
        let header_h = u16::from(self.in_columns());
        let [header, body, info, foot] =
            Layout::vertical([Constraint::Length(header_h), Constraint::Min(1), Constraint::Length(1), Constraint::Length(foot_h)]).areas(inner);
        let h = body.height as usize;
        self.page = h;
        let gutter = if self.numbers { lines_known.max(1).to_string().len() + 1 } else { 0 };
        let blamed = self.git.as_ref().is_some_and(|g| g.blame.is_some());
        let margin = if blamed { crate::blame::WIDTH } else { 0 };
        let w = (body.width as usize).saturating_sub(gutter + margin).max(1);
        let rows = self.highlighted(self.top, h);
        let slots: HashMap<&str, Style> = theme.slots().into_iter().map(|(n, s)| (n, sty(s))).collect();
        let token = |slot: Option<&str>| match slot.and_then(|s| slots.get(s)).and_then(|s| s.fg) {
            Some(fg) if slot == Some("hidden") => base.fg(fg).add_modifier(Modifier::ITALIC),
            Some(fg) => base.fg(fg),
            None => base,
        };
        let hit = sty(&theme.cursor);
        let q = self.query.to_lowercase();
        // A row's cells: each character with its style, tabs and controls made safe.
        let cells_of = |pieces: &Pieces| {
            let mut cells: Vec<(char, Style)> = vec![];
            let text: String = pieces.iter().map(|(_, s)| s.as_str()).collect();
            let marks = marked(&text, &q);
            // Marks and characters both go left to right: one cursor walks the marks.
            let (mut at, mut m) = (0, 0);
            for (slot, s) in pieces {
                for c in s.chars() {
                    while marks.get(m).is_some_and(|&(_, b)| b <= at) {
                        m += 1;
                    }
                    let style = if marks.get(m).is_some_and(|&(a, _)| a <= at) { hit } else { token(*slot) };
                    at += c.len_utf8();
                    match c {
                        '\t' => {
                            let n = TAB - cells.len() % TAB;
                            cells.extend(std::iter::repeat_n((' ', style), n));
                        }
                        c if c.is_control() => cells.push(('·', style)),
                        c => cells.push((c, style)),
                    }
                }
            }
            cells
        };
        if header_h > 0
            && let Some(log) = self.log()
        {
            let names: String = log.columns.iter().map(|(c, w)| format!("{}  ", fit(c, *w))).collect();
            let cells: Vec<(char, Style)> = names.chars().map(|c| (c, dim.add_modifier(Modifier::BOLD))).collect();
            let mut spans = vec![Span::styled(" ".repeat(gutter + margin), dim)];
            spans.extend(clip(&cells, self.left, w).iter().map(|(c, s)| Span::styled(c.to_string(), *s)));
            f.render_widget(Paragraph::new(Line::from(spans)).style(base), header);
        }
        let mut drawn: Vec<Line> = vec![];
        for (k, (line, pieces, _)) in rows.into_iter().enumerate() {
            let no = line + 1;
            let cells = cells_of(&pieces);
            let chunks: Vec<&[(char, Style)]> = if self.wrap { wrapped(&cells, w) } else { vec![clip(&cells, self.left, w)] };
            for (j, chunk) in chunks.into_iter().enumerate() {
                let mut spans = vec![];
                if let Some(m) = self.git.as_ref().and_then(|g| g.margin(no - 1)) {
                    spans.push(Span::styled(if j == 0 { m } else { " ".repeat(margin) }, dim));
                }
                if self.numbers {
                    let n = if j == 0 { format!("{no:>width$} ", width = gutter - 1) } else { " ".repeat(gutter) };
                    spans.push(Span::styled(n, if self.found == Some(self.top + k) { hit } else { dim }));
                }
                spans.extend(chunk.iter().map(|(c, s)| Span::styled(c.to_string(), *s)));
                drawn.push(Line::from(spans));
                if drawn.len() >= h {
                    break;
                }
            }
            if drawn.len() >= h {
                break;
            }
        }
        f.render_widget(Paragraph::new(drawn).style(base), body);
        let pct = ((self.top + h).min(count) * 100).checked_div(count).unwrap_or(100).min(100);
        let info_text = match &self.typing {
            Some(Typing::Search(s)) => format!("{}: {s}▏", t!("viewer.search")),
            Some(Typing::Line(s)) => format!("{}: {s}▏", t!("viewer.go_to_line")),
            Some(Typing::Filter(s)) => {
                let fields: Vec<&str> = self.log().iter().flat_map(|l| l.columns.iter().map(|(c, _)| c.as_str())).collect();
                if fields.is_empty() {
                    format!("{}: {s}▏", t!("viewer.filter"))
                } else {
                    format!("{}: {s}▏   {}", t!("viewer.filter"), t!("viewer.filter_fields", "fields" => fields.join(", ")))
                }
            }
            None => self.said.clone().unwrap_or_else(|| {
                let at = match self.rows_at(self.top, self.top + 1).first() {
                    Some((line, ..)) => t!("viewer.at", "line" => line + 1, "pct" => pct),
                    None => t!("viewer.at", "line" => self.top + 1, "pct" => pct),
                };
                let state = self.state();
                if state.is_empty() { at } else { format!("{at} · {state}") }
            }),
        };
        f.render_widget(Paragraph::new(fit(&info_text, inner.width as usize)).style(sty(&theme.status)), info);
        f.render_widget(Paragraph::new(self.key_line.as_str()).wrap(ratatui::widgets::Wrap { trim: true }).style(dim), foot);
    }
}

/// A log's line as its columns, each cut or padded to its width, the fields with no column
/// after them as `name=value`; a line with no fields (the next line of a stack trace) as it is.
fn in_columns(log: &logs::View, text: &str, slot: Option<&'static str>) -> Pieces {
    let Some((cols, rest)) = log.cells(text) else {
        return vec![(slot, text.to_owned())];
    };
    let mut out = String::new();
    for (v, (_, w)) in cols.iter().zip(&log.columns) {
        out.push_str(&fit(v, *w));
        out.push_str("  ");
    }
    for (k, v) in rest {
        out.push_str(&format!("{k}={v} "));
    }
    vec![(slot, out.trim_end().to_owned())]
}

/// Where `q` stands in `text` (byte ranges), case aside; nothing for an empty search.
fn marked(text: &str, q: &str) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return vec![];
    }
    let lower = text.to_lowercase();
    // Lowercasing can change a text's length (İ): marks only where it did not.
    if lower.len() != text.len() {
        return vec![];
    }
    lower.match_indices(q).map(|(i, m)| (i, i + m.len())).collect()
}

/// Cells from column `left`, at most `w` columns of them.
fn clip(cells: &[(char, Style)], left: usize, w: usize) -> &[(char, Style)] {
    let a = left.min(cells.len());
    let mut used = 0;
    let mut b = a;
    while b < cells.len() && used + cells[b].0.width().unwrap_or(0) <= w {
        used += cells[b].0.width().unwrap_or(0);
        b += 1;
    }
    &cells[a..b]
}

/// Cells cut into rows of `w` columns; an empty line is one empty row.
fn wrapped(cells: &[(char, Style)], w: usize) -> Vec<&[(char, Style)]> {
    let mut out = vec![];
    let mut a = 0;
    while a < cells.len() {
        let row = clip(cells, a, w);
        let n = row.len().max(1);
        out.push(&cells[a..(a + n).min(cells.len())]);
        a += n;
    }
    if out.is_empty() {
        out.push(&cells[..0]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    fn wait(v: &Viewer) {
        while !v.count().1 {
            std::thread::yield_now();
        }
    }

    /// The search to its end, as the event loop's ticks take it.
    fn finish(v: &mut Viewer) {
        while v.busy() {
            v.poll();
        }
    }

    #[test]
    fn every_hit_on_a_line_is_marked_and_nothing_between() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-marks-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("hits.txt");
        std::fs::write(&p, "ab-ab-ab\n").unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        wait(&v);
        v.query = "ab".into();
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(30, 5)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let hit = sty(&Theme::default().cursor);
        let row: String = term.backend().buffer().content.iter().filter(|c| c.style().bg == hit.bg && c.style().fg == hit.fg).map(|c| c.symbol()).collect();
        assert_eq!(row, "ababab");
        drop(v);
        std::fs::remove_dir_all(&d).ok();
    }

    fn screen(v: &mut Viewer, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        term.backend().buffer().content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn a_file_is_read_by_lines_highlighted_searched_and_gone_to() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("orbit.rs");
        let body: String = (1..=500).map(|i| format!("fn burn_{i}() {{ let fuel = {i}; }} // stage\n")).collect();
        std::fs::write(&p, format!("{body}\tlast\x1b[31m line")).unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        wait(&v);
        assert_eq!(v.count().0, 501);
        assert_eq!(v.syntax.map(|s| s.name.as_str()), Some("Rust"));
        let first = v.highlighted(0, 1).remove(0).1;
        assert!(first.iter().any(|(slot, s)| *slot == Some("symlink") && s == "fn"), "{first:?}");
        assert!(first.iter().any(|(slot, s)| *slot == Some("hidden") && s.contains("stage")), "{first:?}");
        // Search: `/`, the text, Enter; `n` goes on, `N` back.
        v.key(key(KeyCode::Char('/')), None);
        for c in "burn_300(".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert_eq!(v.found, Some(299));
        v.query = "fuel".into();
        v.key(key(KeyCode::Char('n')), None);
        assert_eq!(v.found, Some(300));
        v.key(key(KeyCode::Char('N')), None);
        assert_eq!(v.found, Some(299));
        // Go to line 501, the last: a tab and an escape sequence drawn harmless.
        v.key(key(KeyCode::Char('g')), None);
        for c in "501".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert_eq!(v.top, 500);
        let s = screen(&mut v, 60, 8);
        assert!(s.contains("501     last·[31m line"), "{s}");
        assert!(!s.contains('\x1b'));
        // Esc closes, F4 edits, p hands it to the pager.
        assert!(matches!(v.key(key(KeyCode::F(4)), Some(Action::Edit)), Outcome::Edit(_, Action::Edit)));
        assert!(matches!(v.key(key(KeyCode::Char('p')), None), Outcome::Pager(_)));
        assert!(matches!(v.key(key(KeyCode::Esc), None), Outcome::Close));
        drop(v);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn binary_shows_as_hex_utf16_as_text_and_old_languages_get_their_grammar() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-kinds-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // A text copy of an old picture: titled with the picture's name and format.
        std::fs::write(d.join("boing.iff.txt"), "boing.iff\n").unwrap();
        let mut v = Viewer::open(&d.join("boing.iff.txt"), ["F4", "F3"]).unwrap().made_of(Path::new("/x/boing.iff"), "Amiga IFF ILBM".into());
        let s = screen(&mut v, 80, 4);
        assert!(s.contains(" boing.iff · Amiga IFF ILBM · ") && !s.contains("boing.iff.txt ·"), "{s}");
        // h: the file the card was made of, as it is; the key line says so.
        assert!(v.key_line.starts_with(&t!("viewer.keys_original")));
        assert!(matches!(v.key(key(KeyCode::Char('h')), None), Outcome::Original(p) if p == Path::new("/x/boing.iff")));
        assert!(matches!(v.key(key(KeyCode::F(4)), Some(Action::Edit)), Outcome::Edit(p, _) if p == Path::new("/x/boing.iff")));
        std::fs::write(d.join("a.bin"), b"MZ\0\x01hello").unwrap();
        let v = Viewer::open(&d.join("a.bin"), ["F4", "F3"]).unwrap();
        assert_eq!(v.encoding, Encoding::Binary);
        let wide: Vec<u8> = [0xff, 0xfe].into_iter().chain("hi\nthere".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(d.join("w.txt"), wide).unwrap();
        let v = Viewer::open(&d.join("w.txt"), ["F4", "F3"]).unwrap();
        assert_eq!(v.lines(0, 5, LONGEST_READ), ["hi", "there"]);
        for (name, text, grammar) in [
            ("orbit.f90", "program orbit\nend program\n", Some("Fortran (Modern)")),
            ("ORBIT.FOR", "      PROGRAM ORBIT\n      END\n", Some("Fortran (Fixed Form)")),
            ("old.pas", "program Hello;\nbegin\nend.\n", Some("Pascal")),
            ("Coroutines.mod", "MODULE Coroutines;\nBEGIN\nEND Coroutines.\n", Some("Pascal")),
            ("adder.v", "module adder;\nendmodule\n", Some("Verilog")),
            ("Prog.fs", "let x = 1\n", Some("F#")),
            ("light.fs", "#version 330\nvoid main() {}\n", Some("GLSL")),
            ("Cargo.toml", "[package]\n", Some("TOML")),
            ("RUN.BAT", "@echo off\n", Some("Batch File")),
            ("rules.pl", ":- initialization(main).\n", None),
            ("PAYROLL.cbl", "       IDENTIFICATION DIVISION.\n", None),
        ] {
            std::fs::write(d.join(name), text).unwrap();
            let v = Viewer::open(&d.join(name), ["F4", "F3"]).unwrap();
            assert_eq!(v.syntax.map(|s| s.name.as_str()), grammar, "{name}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A search started while the lines are still being found (a big file just opened) goes
    /// on over the lines that come, instead of stopping at *Not found yet*.
    #[test]
    fn a_search_goes_on_while_the_lines_are_found() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-follow-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("big.txt");
        let body: String = (0..500).map(|i| if i == 480 { "the needle\n".to_string() } else { format!("line {i}\n") }).collect();
        std::fs::write(&p, body).unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        wait(&v);
        // As it is a moment after opening: ten lines found, the rest still to come.
        let Body::File(lines) = &v.body else { panic!() };
        let rest = lines.reader().hold_back(10);
        v.query = "needle".into();
        v.find(false);
        v.poll();
        assert!(v.busy(), "still searching");
        assert_eq!((v.found, v.said.as_deref()), (None, Some(t!("viewer.still_reading").as_str())));
        let Body::File(lines) = &v.body else { panic!() };
        lines.reader().give_back(rest);
        finish(&mut v);
        assert_eq!(v.found, Some(480), "found once its line is");
        drop(v);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn search_reads_long_lines_a_few_megabytes_at_a_time() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-search-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Twelve lines of two megabytes and a short one with the word: each batch holds at most
        // a few lines, not thousands of megabyte-long ones.
        let p = d.join("wide.txt");
        let mut body: String = (0..12).map(|_| "y".repeat(2 << 20) + "\n").collect();
        body.push_str("needle here\n");
        std::fs::write(&p, body).unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        wait(&v);
        assert!(v.batch(0, true, 13) <= 5, "at most READ_AT_ONCE bytes a batch");
        assert!(v.batch(13, false, 0) >= 8, "backwards too");
        assert_eq!(v.batch(0, true, 0), 0);
        v.query = "NEEDLE".into();
        v.find(false);
        finish(&mut v);
        assert_eq!(v.found, Some(12));
        v.find(true);
        finish(&mut v);
        assert_eq!(v.found, Some(12), "nothing before it");
        // Esc stops a search going on and leaves the file open.
        v.seek = Some(Seek { q: "x".into(), a: 0, b: 13, back: false, from: (0, 13) });
        assert!(matches!(v.key(key(KeyCode::Esc), None), Outcome::Stay));
        assert!(!v.busy());
        assert!(matches!(v.key(key(KeyCode::Esc), None), Outcome::Close));
        drop(v);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_huge_line_is_cut_a_growing_log_is_followed_and_a_pipe_is_refused() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-big-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Three megabytes and no newline: read to a megabyte, then `…`.
        let p = d.join("min.js");
        std::fs::write(&p, "x".repeat(3 << 20)).unwrap();
        let v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        wait(&v);
        let line = v.lines(0, 1, LONGEST_READ).remove(0);
        assert_eq!(line.len(), (1 << 20) + '…'.len_utf8());
        assert!(line.ends_with('…'));
        // A log written to after it was opened: End finds the new lines.
        let log = d.join("app.log");
        std::fs::write(&log, "one\ntwo\n").unwrap();
        let mut v = Viewer::open(&log, ["F4", "F3"]).unwrap();
        wait(&v);
        assert_eq!(v.count().0, 2);
        std::fs::write(&log, "one\ntwo\nthree\nfour\n").unwrap();
        v.key(key(KeyCode::End), None);
        wait(&v);
        assert_eq!(v.count().0, 4);
        assert_eq!(v.lines(3, 4, LONGEST_READ), ["four"]);
        // Rotated: shorter now, read again from the start.
        std::fs::write(&log, "new\n").unwrap();
        v.key(key(KeyCode::End), None);
        wait(&v);
        assert_eq!(v.lines(0, 5, LONGEST_READ), ["new"]);
        // Windows-1252 by its start; one stray line stays readable.
        std::fs::write(d.join("old.txt"), b"\x93Caf\xe9\x94\n").unwrap();
        let v = Viewer::open(&d.join("old.txt"), ["F4", "F3"]).unwrap();
        wait(&v);
        assert_eq!((v.encoding, v.lines(0, 1, LONGEST_READ)), (Encoding::Latin1, vec!["“Café”".to_string()]));
        #[cfg(unix)]
        {
            let fifo = d.join("pipe");
            let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            assert!(Viewer::open(&fifo, ["F4", "F3"]).is_err(), "never opened, so never blocks");
        }
        // Closed first: Windows does not delete a file still open.
        drop(v);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The rows of a viewer, as the event loop's ticks find them.
    fn settled(v: &mut Viewer) {
        let limit = Instant::now() + coxswain_core::test_limit(Duration::from_secs(10));
        while !v.count().1 || v.keep_line.is_some() {
            assert!(Instant::now() < limit, "rows never found");
            v.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn typed(v: &mut Viewer, s: &str) {
        for c in s.chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
    }

    #[test]
    fn a_log_is_coloured_by_level_folded_filtered_shown_in_columns_and_followed() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-log-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("launch.log");
        let log = "\
2026-10-26 10:00:00 INFO countdown started
2026-10-26 10:00:01 ERROR valve stuck
java.lang.IllegalStateException: valve 3
\tat rocket.Valve.open(Valve.java:12)
2026-10-26 10:00:02 WARN fuel low
2026-10-26 10:00:03 DEBUG temp=91
";
        std::fs::write(&p, log).unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
        settled(&mut v);
        assert!(v.is_log() && v.syntax.is_none());
        // Folded at open: the trace's two lines under the error, said on its row.
        let rows = v.highlighted(0, 9);
        assert_eq!(rows.iter().map(|r| (r.0, r.2)).collect::<Vec<_>>(), [(0, 0), (1, 2), (4, 0), (5, 0)]);
        assert_eq!(rows[1].1[0], (Some("git_deleted"), "2026-10-26 10:00:01 ERROR valve stuck".to_string()));
        assert!(rows[1].1[1].1.contains(&format!("▸ +{}", tn!("viewer.lines", 2))), "{:?}", rows[1].1);
        let s = screen(&mut v, 80, 10);
        assert!(s.contains("2 2026-10-26 10:00:01 ERROR") && s.contains("5 2026-10-26 10:00:02 WARN"), "line numbers are the file's: {s}");
        // z unfolds: the trace coloured as its error.
        v.key(key(KeyCode::Char('z')), None);
        settled(&mut v);
        let rows = v.highlighted(0, 9);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[3].1[0].0, Some("git_deleted"));
        // e: warnings and errors, then errors, then all again.
        v.key(key(KeyCode::Char('e')), None);
        settled(&mut v);
        assert_eq!(v.highlighted(0, 9).iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(v.said.as_deref(), Some(t!("viewer.level_warn").as_str()));
        v.key(key(KeyCode::Char('e')), None);
        settled(&mut v);
        assert_eq!(v.highlighted(0, 9).iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3]);
        v.key(key(KeyCode::Char('e')), None);
        // &: only the entries with a text in them, case aside; empty takes it off.
        v.key(key(KeyCode::Char('&')), None);
        typed(&mut v, "VALVE.java");
        settled(&mut v);
        assert_eq!(v.highlighted(0, 9).iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(v.said, Some(t!("viewer.filtered", "filter" => "VALVE.java")));
        v.key(key(KeyCode::Down), None);
        assert!(screen(&mut v, 120, 10).contains(&t!("viewer.state_filter", "filter" => "VALVE.java")));
        v.key(key(KeyCode::Char('&')), None);
        for _ in 0..20 {
            v.key(key(KeyCode::Backspace), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert!(matches!(&v.body, Body::File(x) if !x.filtered()) && v.count().0 == 6, "every line, read straight from the file");
        // F follows: lines written later show, the screen at the end; E stops at an error.
        v.key(key(KeyCode::Char('F')), None);
        v.key(key(KeyCode::Char('E')), None);
        assert!(v.follow && v.pause_on_error && v.busy());
        std::fs::write(&p, format!("{log}2026-10-26 10:00:04 INFO burn\n")).unwrap();
        v.grown = Instant::now() - FOLLOW_EVERY;
        v.poll();
        assert_eq!((v.count().0, v.follow), (7, true));
        std::fs::write(&p, format!("{log}2026-10-26 10:00:04 INFO burn\n2026-10-26 10:00:05 ERROR abort\n2026-10-26 10:00:06 INFO after\n")).unwrap();
        v.grown = Instant::now() - FOLLOW_EVERY;
        v.poll();
        assert!(!v.follow, "stopped at the error");
        assert_eq!((v.found, v.said.clone()), (Some(7), Some(t!("viewer.stopped_at_error", "line" => 8))));
        drop(v);
        // JSON Lines: its fields as columns, with a header; c shows the lines as they are.
        let j = d.join("events.jsonl");
        std::fs::write(&j, "{\"time\":\"10:00\",\"level\":\"error\",\"msg\":\"lost\",\"user\":\"ada\"}\n{\"time\":\"10:01\",\"level\":\"info\",\"msg\":\"ok\",\"user\":\"bob\",\"extra\":1}\n").unwrap();
        let mut v = Viewer::open(&j, ["F4", "F3"]).unwrap();
        settled(&mut v);
        let s = screen(&mut v, 80, 14);
        assert!(s.contains("time   level  msg   user"), "{s}");
        assert!(s.contains("10:01  info   ok    bob   extra=1"), "{s}");
        v.key(key(KeyCode::Char('c')), None);
        assert!(screen(&mut v, 80, 14).contains("{\"time\":\"10:01\""));
        // A field's value, typed as name=value; Tab finishes the name.
        v.key(key(KeyCode::Char('&')), None);
        v.key(key(KeyCode::Char('u')), None);
        v.key(key(KeyCode::Tab), None);
        typed(&mut v, "BOB");
        settled(&mut v);
        assert_eq!(v.highlighted(0, 9).iter().map(|r| r.0).collect::<Vec<_>>(), [1]);
        assert_eq!(v.shape().field, Some(("user".into(), "BOB".into())));
        drop(v);
        // Not a log: no log keys, but F and & work.
        std::fs::write(d.join("notes.txt"), "alpha\nbeta\n").unwrap();
        let mut v = Viewer::open(&d.join("notes.txt"), ["F4", "F3"]).unwrap();
        assert!(!v.is_log() && v.key_line.starts_with(&t!("viewer.keys_follow")));
        v.key(key(KeyCode::Char('e')), None);
        assert!(v.shape().level.is_none());
        v.key(key(KeyCode::Char('&')), None);
        typed(&mut v, "BET");
        settled(&mut v);
        assert_eq!(v.lines(0, 9, LONGEST_READ), ["beta"]);
        drop(v);
        std::fs::remove_dir_all(&d).ok();
    }
}

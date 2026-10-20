//! The built-in viewer (F3 on a file, when `viewer` is not set): the file full screen,
//! highlighted in the theme's colours as the desktop app's preview colours code
//! (`coxswain_core::view::TOKENS`), with line numbers, wrapping, search and go to line.
//!
//! A big file is never read whole: a thread finds where its lines start, and only the lines
//! on screen are read, so a 100 MB log opens at once. Highlighting starts from the nearest
//! point it has passed before (every `STEP` lines), and a file over `PLAIN_OVER` is shown plain.
//! The grammar is the one `coxswain_core::code::language` names (the desktop app's), looked
//! up among syntect's and two-face's; else the file's extension, else its first line.

use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::bom::faint;
use crate::ui::{fit, sty};

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
/// Bytes of one line read at most (a minified file, one without a newline): the rest is `…`.
const LONGEST_READ: u64 = 1 << 20;
/// Lines found at most: their starts take 8 bytes each (160 MB for these).
const MOST_LINES: usize = 20_000_000;
/// Bytes read in one go when lines are read together; past it, one line at a time.
const READ_AT_ONCE: u64 = 4 << 20;

/// syntect's grammars and two-face's (about 200 languages), read once, on the first F3.
fn syntaxes() -> &'static SyntaxSet {
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
type Pieces = Vec<(Option<&'static str>, String)>;

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Close, and open the file in the editor.
    Edit(PathBuf),
    /// Close, and open the file in the pager (`$PAGER`, else less).
    Pager(PathBuf),
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
    /// Where each line starts, found by a thread (`done` once it has read to the end); the
    /// last entry is where the next line would start.
    File { file: File, index: Arc<Index>, utf8: bool },
    /// UTF-16, decoded whole, and a hex dump.
    Whole(Vec<String>),
}

/// Where a file's lines start, found by a thread.
#[derive(Default)]
struct Index {
    /// Each line's first byte; the last entry is where the next line would start.
    starts: Mutex<Vec<u64>>,
    /// The thread has read to the end, or to `MOST_LINES`.
    done: AtomicBool,
    /// It stopped at `MOST_LINES`.
    capped: AtomicBool,
    /// The viewer closed: the thread stops.
    stop: AtomicBool,
}

impl Index {
    /// A thread finding the lines of `path`.
    fn start(file: File) -> Arc<Index> {
        let index = Arc::new(Index { starts: Mutex::new(vec![0]), ..Index::default() });
        let i = index.clone();
        std::thread::spawn(move || find_lines(&file, &i));
        index
    }
}

/// A line typed at the bottom: a search or a line number.
enum Typing {
    Search(String),
    Line(String),
}

/// Highlighting's state before a line: the grammar's, and the scopes open there.
#[derive(Clone)]
struct Point {
    parse: ParseState,
    scopes: ScopeStack,
}

pub struct Viewer {
    pub path: PathBuf,
    title: String,
    /// What the title calls the text, for a text copy of another file (its format); else the
    /// highlighting's language, or plain text.
    kind: Option<String>,
    body: Body,
    encoding: Encoding,
    syntax: Option<&'static SyntaxReference>,
    /// Points highlighting has passed, every `STEP` lines from the start: index i is line i·STEP.
    points: Vec<Point>,
    /// The theme slot each scope takes, looked up once.
    slots: HashMap<Scope, Option<&'static str>>,
    /// The lines last drawn, highlighted: (first, count, lines known then) and per line its
    /// pieces.
    shown: Option<((usize, usize, usize), Vec<Pieces>)>,
    top: usize,
    left: usize,
    numbers: bool,
    wrap: bool,
    typing: Option<Typing>,
    /// The last search, and the line it was found on.
    query: String,
    found: Option<usize>,
    said: Option<String>,
    /// Rows of text on the screen, as last drawn: what PgUp and PgDn move.
    page: usize,
    key_line: String,
}

impl Viewer {
    /// Open `path`: its first bytes say how it is read; the rest is read as it is shown.
    pub fn open(path: &Path, labels: [&str; 2]) -> std::io::Result<Viewer> {
        let file = open_file(path)?;
        let len = file.metadata()?.len();
        let mut head = vec![0u8; 8192];
        let n = read_at(&file, &mut head, 0)?;
        head.truncate(n);
        let utf16 = head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]);
        let binary = !utf16 && head.contains(&0);
        let mut cut = false;
        let (body, encoding, text_head) = if binary || utf16 {
            let most = if binary { (HEX_LINES * 16) as u64 } else { PLAIN_OVER };
            let mut all = vec![0u8; len.min(most) as usize];
            let n = read_at(&file, &mut all, 0)?;
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
            // The thread reads its own handle of the same file, never the path again.
            let index = Index::start(file.try_clone()?);
            (Body::File { file, index, utf8 }, enc, text)
        };
        let syntax = (encoding != Encoding::Binary && len <= PLAIN_OVER).then(|| syntax_for(path, &text_head)).flatten();
        let [edit, view] = labels;
        Ok(Viewer {
            title: path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
            kind: None,
            path: path.to_path_buf(),
            body,
            encoding,
            points: syntax.map(|s| vec![Point { parse: ParseState::new(s), scopes: ScopeStack::new() }]).unwrap_or_default(),
            syntax,
            slots: HashMap::new(),
            shown: None,
            top: 0,
            left: 0,
            numbers: true,
            wrap: false,
            typing: None,
            query: String::new(),
            found: None,
            said: if cut {
                Some(t!("viewer.first_part"))
            } else {
                (len > PLAIN_OVER && matches!(encoding, Encoding::Utf8 | Encoding::Latin1)).then(|| t!("viewer.plain_big"))
            },
            page: 20,
            key_line: t!("viewer.keys", "edit" => edit, "view" => view),
        })
    }

    /// A text copy shown as the file it was made of: its name and format in the title.
    pub fn made_of(mut self, of: &Path, kind: String) -> Viewer {
        self.title = of.file_name().map_or_else(|| of.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.kind = Some(kind);
        self
    }

    /// Lines known so far, and whether that is all of them.
    fn count(&self) -> (usize, bool) {
        match &self.body {
            Body::File { index, .. } => {
                let done = index.done.load(Ordering::Relaxed);
                let n = index.starts.lock().unwrap().len();
                (n.saturating_sub(1), done)
            }
            Body::Whole(lines) => (lines.len(), true),
        }
    }

    /// Lines `a..b` (as many of them as there are), read from the disk or held; each at most
    /// `most` bytes, a longer one cut with `…`.
    fn lines(&mut self, a: usize, b: usize, most: u64) -> Vec<String> {
        match &mut self.body {
            Body::Whole(lines) => lines.get(a.min(lines.len())..b.min(lines.len())).unwrap_or_default().to_vec(),
            Body::File { file, index, utf8 } => {
                let cuts = {
                    let s = index.starts.lock().unwrap();
                    let last = s.len().saturating_sub(1);
                    let (a, b) = (a.min(last), b.min(last));
                    if a >= b {
                        return vec![];
                    }
                    s[a..=b].to_vec()
                };
                let (from, to) = (cuts[0], cuts[cuts.len() - 1]);
                // Short lines in one read; a span of long ones a line at a time, each cut.
                let whole = if to - from <= READ_AT_ONCE {
                    let mut buf = vec![0u8; (to - from) as usize];
                    read_at(file, &mut buf, from).is_ok().then_some(buf)
                } else {
                    None
                };
                cuts.windows(2)
                    .map(|w| {
                        let (len, cut) = ((w[1] - w[0]).min(most), w[1] - w[0] > most);
                        let mut own = vec![];
                        let line = match &whole {
                            Some(buf) => &buf[(w[0] - from) as usize..(w[0] - from + len) as usize],
                            None => {
                                own.resize(len as usize, 0);
                                if read_at(file, &mut own, w[0]).is_err() {
                                    own.clear();
                                }
                                &own[..]
                            }
                        };
                        let line = line.strip_suffix(b"\n").unwrap_or(line);
                        let line = line.strip_suffix(b"\r").unwrap_or(line);
                        let mut text = coxswain_core::view::decode_line(line, *utf8);
                        if cut {
                            text.push('…');
                        }
                        text
                    })
                    .collect()
            }
        }
    }

    /// The file changed since its lines were found (a log written to, rotated): find them
    /// again. Asked on End.
    fn recheck(&mut self) {
        let Body::File { file, index, .. } = &mut self.body else { return };
        if !index.done.load(Ordering::Relaxed) || index.capped.load(Ordering::Relaxed) {
            return;
        }
        let known = index.starts.lock().unwrap().last().copied().unwrap_or(0);
        if file.metadata().is_ok_and(|m| m.len() != known) {
            index.stop.store(true, Ordering::Relaxed);
            let Ok(again) = file.try_clone() else { return };
            *index = Index::start(again);
            self.points.truncate(1);
            self.shown = None;
            // Wait a moment for the end, so End goes there; a big file catches up on the next draws.
            for _ in 0..50 {
                if index.done.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    /// Lines `a..a+n` in pieces, each with the theme slot it is coloured with.
    fn highlighted(&mut self, a: usize, n: usize) -> Vec<Pieces> {
        let key = (a, n, self.count().0);
        if let Some((at, lines)) = &self.shown
            && *at == key
        {
            return lines.clone();
        }
        let Some(syntax) = self.syntax else {
            return self.lines(a, a + n, LONGEST_READ).into_iter().map(|l| vec![(None, l)]).collect();
        };
        // Start from the last point kept at or above `a`, or afresh a little above it.
        let kept = (a / STEP).min(self.points.len() - 1);
        let (mut at, mut point, exact) = if a - kept * STEP <= FARTHEST {
            (kept * STEP, self.points[kept].clone(), true)
        } else {
            let from = a.saturating_sub(STEP);
            (from, Point { parse: ParseState::new(syntax), scopes: ScopeStack::new() }, false)
        };
        // Lines above the screen only lead up to it: a line too long to highlight is not read whole.
        let mut text = self.lines(at, a, LONGEST_HIGHLIGHTED as u64 + 1);
        text.extend(self.lines(a, a + n, LONGEST_READ));
        let mut out = vec![];
        for line in text {
            if exact && at % STEP == 0 && at / STEP == self.points.len() {
                self.points.push(point.clone());
            }
            let pieces = self.paint(&mut point, &line);
            if at >= a {
                out.push(pieces);
            }
            at += 1;
        }
        self.shown = Some((key, out.clone()));
        out
    }

    /// One line's pieces, from `point`, which moves past it.
    fn paint(&mut self, point: &mut Point, line: &str) -> Pieces {
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
        for (at, op) in ops.into_iter().chain([(line.len(), syntect::parsing::ScopeStackOp::Noop)]) {
            let at = at.min(line.len());
            if at > from {
                let slot = self.slot(&point.scopes);
                out.push((slot, line[from..at].to_owned()));
                from = at;
            }
            let _ = point.scopes.apply(&op);
        }
        out
    }

    /// The slot of the innermost scope open that has one.
    fn slot(&mut self, scopes: &ScopeStack) -> Option<&'static str> {
        scopes.as_slice().iter().rev().find_map(|s| *self.slots.entry(*s).or_insert_with(|| coxswain_core::view::slot_of_scope(&s.build_string())))
    }

    /// Scroll by `by` lines (the mouse wheel, the arrows).
    pub fn scroll(&mut self, by: isize) {
        let (n, _) = self.count();
        self.top = (self.top as isize + by).clamp(0, n.saturating_sub(1) as isize) as usize;
    }

    /// The far end of a batch of lines for `find`, starting at `from` and going towards `limit`
    /// (`forward`, or backwards): at most 4096 lines, and at most `READ_AT_ONCE` bytes counting
    /// each line to `LONGEST_READ`, but always one line.
    fn batch(&self, from: usize, forward: bool, limit: usize) -> usize {
        const LINES: usize = 4096;
        let Body::File { index, .. } = &self.body else {
            return if forward { (from + LINES).min(limit) } else { from.saturating_sub(LINES).max(limit) };
        };
        let s = index.starts.lock().unwrap();
        let len = |i: usize| match (s.get(i), s.get(i + 1)) {
            (Some(a), Some(b)) => (b - a).min(LONGEST_READ),
            _ => 0,
        };
        let (mut at, mut bytes) = (from, 0u64);
        loop {
            let next = if forward { at + 1 } else { at.wrapping_sub(1) };
            let line = if forward { at } else { next };
            if (forward && at >= limit) || (!forward && at <= limit) || from.abs_diff(at) >= LINES {
                return at;
            }
            bytes += len(line);
            if bytes > READ_AT_ONCE && at != from {
                return at;
            }
            at = next;
        }
    }

    /// The next line (or the one before, `back`) after the screen's first with the search on
    /// it, case aside. The disk is read a megabyte at a time, so a big file takes a moment.
    fn find(&mut self, back: bool) {
        if self.query.is_empty() {
            return;
        }
        let q = self.query.to_lowercase();
        let (n, done) = self.count();
        // A new search starts at the screen's first line; `n` and `N` go on from the hit.
        let (mut a, mut b) = match (self.found, back) {
            (Some(f), false) => (f + 1, n),
            (None, false) => (self.top, n),
            (f, true) => (0, f.unwrap_or(self.top)),
        };
        // Lines in batches of at most a few thousand and a few megabytes (each line read to a
        // megabyte at most), from the search's start onwards, or backwards for `N`.
        while a < b {
            let (lo, hi) = if back { (self.batch(b, false, a), b) } else { (a, self.batch(a, true, b)) };
            let lines = self.lines(lo, hi, LONGEST_READ);
            let has = |i: usize| lines.get(i - lo).is_some_and(|l| l.to_lowercase().contains(&q));
            let hit = if back { (lo..hi).rev().find(|&i| has(i)) } else { (lo..hi).find(|&i| has(i)) };
            if let Some(i) = hit {
                self.found = Some(i);
                self.top = i.saturating_sub(self.page / 3);
                self.said = None;
                return;
            }
            if back { b = lo } else { a = hi }
        }
        self.said = Some(if done { t!("viewer.not_found", "text" => &self.query) } else { t!("viewer.still_reading") });
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if let Some(typing) = &mut self.typing {
            let field = match typing {
                Typing::Search(s) | Typing::Line(s) => s,
            };
            match key.code {
                KeyCode::Esc => self.typing = None,
                KeyCode::Backspace => {
                    field.pop();
                }
                KeyCode::Enter => match self.typing.take() {
                    Some(Typing::Search(s)) => {
                        self.query = s;
                        self.found = None;
                        self.find(false);
                    }
                    Some(Typing::Line(s)) => match s.trim().parse::<usize>() {
                        Ok(n) if n > 0 => {
                            let (count, _) = self.count();
                            self.top = (n - 1).min(count.saturating_sub(1));
                            self.found = None;
                        }
                        _ => {}
                    },
                    None => {}
                },
                KeyCode::Char(c) if !key.ctrl && !key.alt => field.push(c),
                _ => {}
            }
            return Outcome::Stay;
        }
        self.said = None;
        let ch = match key.code {
            KeyCode::Char(c) if !key.ctrl && !key.alt => Some(c),
            _ => None,
        };
        let page = self.page.max(1) as isize;
        match (key.code, ch) {
            (KeyCode::Esc, _) => return Outcome::Close,
            _ if matches!(action, Some(Action::Quit | Action::View)) => return Outcome::Close,
            _ if action == Some(Action::Edit) => return Outcome::Edit(self.path.clone()),
            (KeyCode::Up, _) => self.scroll(-1),
            (KeyCode::Down, _) | (KeyCode::Enter, _) => self.scroll(1),
            (KeyCode::PageUp, _) => self.scroll(-page),
            (KeyCode::PageDown, _) | (_, Some(' ')) => self.scroll(page),
            (KeyCode::Home, _) => (self.top, self.left) = (0, 0),
            (KeyCode::End, _) => {
                self.recheck();
                let (n, _) = self.count();
                self.top = n.saturating_sub(page as usize);
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
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area();
        let base = sty(&theme.panel);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let (count, done) = self.count();
        let grammar = self.kind.clone().unwrap_or_else(|| self.syntax.map_or_else(|| t!("viewer.plain"), |s| s.name.clone()));
        let encoding = match self.encoding {
            Encoding::Utf8 => "UTF-8",
            Encoding::Latin1 => "Windows-1252",
            Encoding::Utf16 => "UTF-16",
            Encoding::Binary => "hex",
        };
        let capped = matches!(&self.body, Body::File { index, .. } if index.capped.load(Ordering::Relaxed));
        let lines_said = if capped {
            tn!("viewer.first_lines", count)
        } else if done {
            tn!("viewer.lines", count)
        } else {
            tn!("viewer.lines_so_far", count)
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
        // The key line on two lines when one is too narrow for it.
        let foot_h = if self.key_line.width() > inner.width as usize { 2 } else { 1 };
        let [body, info, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(foot_h)]).areas(inner);
        let h = body.height as usize;
        self.page = h;
        let gutter = if self.numbers { count.max(1).to_string().len() + 1 } else { 0 };
        let w = (body.width as usize).saturating_sub(gutter).max(1);
        let lines = self.highlighted(self.top, h);
        let slots: HashMap<&str, Style> = theme.slots().into_iter().map(|(n, s)| (n, sty(s))).collect();
        let token = |slot: Option<&str>| match slot.and_then(|s| slots.get(s)).and_then(|s| s.fg) {
            Some(fg) if slot == Some("hidden") => base.fg(fg).add_modifier(Modifier::ITALIC),
            Some(fg) => base.fg(fg),
            None => base,
        };
        let hit = sty(&theme.cursor);
        let q = self.query.to_lowercase();
        let mut rows: Vec<Line> = vec![];
        for (k, pieces) in lines.into_iter().enumerate() {
            let no = self.top + k + 1;
            // Cells of the line: each character with its style, tabs and controls made safe.
            let mut cells: Vec<(char, Style)> = vec![];
            let text: String = pieces.iter().map(|(_, s)| s.as_str()).collect();
            let marks = marked(&text, &q);
            let mut at = 0;
            for (slot, s) in &pieces {
                for c in s.chars() {
                    let style = if marks.iter().any(|(a, b)| (*a..*b).contains(&at)) { hit } else { token(*slot) };
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
            let chunks: Vec<&[(char, Style)]> = if self.wrap { wrapped(&cells, w) } else { vec![clip(&cells, self.left, w)] };
            for (j, chunk) in chunks.into_iter().enumerate() {
                let mut spans = vec![];
                if self.numbers {
                    let n = if j == 0 { format!("{no:>width$} ", width = gutter - 1) } else { " ".repeat(gutter) };
                    spans.push(Span::styled(n, if self.found == Some(no - 1) { hit } else { dim }));
                }
                spans.extend(chunk.iter().map(|(c, s)| Span::styled(c.to_string(), *s)));
                rows.push(Line::from(spans));
                if rows.len() >= h {
                    break;
                }
            }
            if rows.len() >= h {
                break;
            }
        }
        f.render_widget(Paragraph::new(rows).style(base), body);
        let pct = ((self.top + h).min(count) * 100).checked_div(count).unwrap_or(100).min(100);
        let info_text = match &self.typing {
            Some(Typing::Search(s)) => format!("{}: {s}▏", t!("viewer.search")),
            Some(Typing::Line(s)) => format!("{}: {s}▏", t!("viewer.go_to_line")),
            None => self.said.clone().unwrap_or_else(|| t!("viewer.at", "line" => self.top + 1, "pct" => pct)),
        };
        f.render_widget(Paragraph::new(fit(&info_text, inner.width as usize)).style(sty(&theme.status)), info);
        f.render_widget(Paragraph::new(self.key_line.as_str()).wrap(ratatui::widgets::Wrap { trim: true }).style(dim), foot);
    }
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

/// Open `path` only when it is a regular file, asked of the opened handle: a pipe or a device
/// would block the read (or never end), and a file swapped for one between a look at the path
/// and the open is caught too. On Unix the open itself does not wait (`O_NONBLOCK`, which a FIFO
/// with no writer would); the flag is taken off again for the reads.
fn open_file(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other(t!("viewer.not_a_file")));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        // SAFETY: fcntl on a descriptor this function owns, reading and setting its flags.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(file)
}

/// Read into `buf` from byte `at`, as far as the file goes: the bytes read. Each read says
/// where, so the viewer and its indexing thread never move each other's place in the file.
fn read_at(f: &File, buf: &mut [u8], at: u64) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        #[cfg(unix)]
        let k = std::os::unix::fs::FileExt::read_at(f, &mut buf[n..], at + n as u64)?;
        #[cfg(windows)]
        let k = std::os::windows::fs::FileExt::seek_read(f, &mut buf[n..], at + n as u64)?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// Find where each line of the file starts, a megabyte at a time, into `index`; `done` at the
/// end, at `MOST_LINES`, or when the file cannot be read further; nothing more once `stop`.
fn find_lines(f: &File, index: &Index) {
    let mut buf = vec![0u8; 1 << 20];
    let mut at = 0u64;
    let mut found = vec![];
    while !index.stop.load(Ordering::Relaxed) {
        let n = match read_at(f, &mut buf, at) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        found.extend(buf[..n].iter().enumerate().filter(|(_, b)| **b == b'\n').map(|(i, _)| at + i as u64 + 1));
        at += n as u64;
        let mut s = index.starts.lock().unwrap();
        s.append(&mut found);
        if s.len() > MOST_LINES {
            s.truncate(MOST_LINES + 1);
            index.capped.store(true, Ordering::Relaxed);
            drop(s);
            return index.done.store(true, Ordering::Relaxed);
        }
    }
    let mut s = index.starts.lock().unwrap();
    // A last line without its newline still counts.
    if s.last().is_some_and(|&l| l < at) {
        s.push(at);
    }
    drop(s);
    index.done.store(true, Ordering::Relaxed);
}

impl Drop for Viewer {
    fn drop(&mut self) {
        if let Body::File { index, .. } = &self.body {
            index.stop.store(true, Ordering::Relaxed);
        }
    }
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
        let first = v.highlighted(0, 1).remove(0);
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
        assert!(matches!(v.key(key(KeyCode::F(4)), Some(Action::Edit)), Outcome::Edit(_)));
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
        std::fs::write(d.join("a.bin"), b"MZ\0\x01hello").unwrap();
        let v = Viewer::open(&d.join("a.bin"), ["F4", "F3"]).unwrap();
        assert_eq!(v.encoding, Encoding::Binary);
        let wide: Vec<u8> = [0xff, 0xfe].into_iter().chain("hi\nthere".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(d.join("w.txt"), wide).unwrap();
        let mut v = Viewer::open(&d.join("w.txt"), ["F4", "F3"]).unwrap();
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

    #[test]
    fn search_reads_long_lines_a_few_megabytes_at_a_time() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-search-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Twelve lines of two megabytes and a short one with the word: each batch holds at most
        // a few lines, not thousands of megabyte-long ones.
        let p = d.join("wide.log");
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
        assert_eq!(v.found, Some(12));
        v.find(true);
        assert_eq!(v.found, Some(12), "nothing before it");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_huge_line_is_cut_a_growing_log_is_followed_and_a_pipe_is_refused() {
        let d = std::env::temp_dir().join(format!("coxswain-viewer-big-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Three megabytes and no newline: read to a megabyte, then `…`.
        let p = d.join("min.js");
        std::fs::write(&p, "x".repeat(3 << 20)).unwrap();
        let mut v = Viewer::open(&p, ["F4", "F3"]).unwrap();
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
        let mut v = Viewer::open(&d.join("old.txt"), ["F4", "F3"]).unwrap();
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
}

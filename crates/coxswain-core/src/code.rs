//! Code read as code, in any language: its passages keep their lines and indentation and are
//! cut where an item (a function, a class, a block) ends, each named after the item it is in;
//! and a map of a project for questions about it as a whole: what each file says it is, its
//! public items, and what its project files say it uses.
//!
//! No parser: brackets, indentation, keywords and comments, the same for every language, so
//! a brace language (C, Java, Go, Rust, JavaScript …), an indentation one (Python, YAML), one
//! with `end` (Ruby, Lua, Elixir, shell) or with parentheses (Lisp) and SQL are all cut at
//! their items.

use std::io::Read;
use std::path::Path;

use crate::meaning::Passage;

/// Bytes of a passage of code at most, and at least before the next item joins it.
const MOST: usize = 1500;
const LEAST: usize = 500;

/// How a language writes its comments and strings.
#[derive(Clone, Copy, Default)]
struct Lang {
    /// `//` and `/* */`.
    slash: bool,
    /// `#` to the end of the line.
    hash: bool,
    /// `--` to the end of the line.
    dash: bool,
    /// `;` to the end of the line (Lisp, assembly).
    semi: bool,
    /// `'…'` is a string, not a character or a lifetime.
    quote: bool,
}

/// Files of code without an extension, by name.
const NAMES: &[&str] = &["Makefile", "makefile", "GNUmakefile", "Dockerfile", "Containerfile", "CMakeLists.txt", "Rakefile", "Gemfile", "Justfile", "justfile", "Vagrantfile", "BUILD", "WORKSPACE", "meson.build", "PKGBUILD", "APKBUILD"];

/// Extensions of code, by how they write comments and strings: `//` (`'` a character),
/// `//` (`'` a string too), `#`, `--`, `;`, and others.
const SLASH: &[&str] = &["rs", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "hxx", "m", "mm", "java", "kt", "kts", "scala", "sc", "swift", "go", "cs", "fs", "fsx", "dart", "zig", "groovy", "gradle", "proto", "d", "sol", "v", "glsl", "hlsl", "wgsl", "cu"];
const SLASH_QUOTE: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "svelte", "vue", "php", "json", "jsonc", "json5", "css", "scss", "less"];
const HASH: &[&str] = &["py", "pyi", "rb", "sh", "bash", "zsh", "fish", "ksh", "pl", "pm", "r", "jl", "ex", "exs", "nim", "cr", "tcl", "ps1", "psm1", "yaml", "yml", "toml", "ini", "cfg", "conf", "cmake", "nix", "mk", "coffee", "gd", "awk", "sed", "rake", "gemspec", "podspec", "tf", "hcl"];
const DASH: &[&str] = &["sql", "lua", "hs", "lhs", "elm", "ada", "adb", "ads", "vhd", "vhdl", "purs"];
const SEMI: &[&str] = &["lisp", "lsp", "el", "clj", "cljs", "cljc", "edn", "scm", "ss", "rkt", "fnl", "asm", "s"];
const OTHER: &[&str] = &["erl", "hrl", "ml", "mli", "vb", "bas", "f90", "f", "pas", "pp"];

/// The ends of the names of files of code, for the store to read them again: `.rs`, `/Makefile`.
pub(crate) fn endings() -> Vec<String> {
    [SLASH, SLASH_QUOTE, HASH, DASH, SEMI, OTHER].concat().iter().map(|e| format!(".{e}")).chain(NAMES.iter().map(|n| format!("/{n}"))).collect()
}

/// The language of a file, by its name; `None` for what is no code.
fn lang(path: &str) -> Option<Lang> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let l = |slash, hash, dash, semi, quote| Some(Lang { slash, hash, dash, semi, quote });
    if NAMES.contains(&name) {
        return l(false, true, false, false, true);
    }
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let e = ext.as_str();
    if SLASH.contains(&e) {
        l(true, false, false, false, false)
    } else if SLASH_QUOTE.contains(&e) {
        l(true, false, false, false, true)
    } else if HASH.contains(&e) {
        l(false, true, false, false, true)
    } else if DASH.contains(&e) {
        l(false, false, true, false, true)
    } else if SEMI.contains(&e) {
        l(false, false, false, true, false)
    } else if OTHER.contains(&e) {
        l(false, false, false, false, true)
    } else {
        None
    }
}

/// Whether a file is code (or a project or configuration file), read with its lines kept.
pub fn is_code(path: &str) -> bool {
    lang(path).is_some()
}

/// One line of code: where it starts, how deep it is in brackets before it, its indentation.
struct Line<'a> {
    text: &'a str,
    depth: i32,
    indent: usize,
}

impl Line<'_> {
    fn blank(&self) -> bool {
        self.text.trim().is_empty()
    }
    fn level(&self) -> (i32, usize) {
        (self.depth, self.indent)
    }
}

/// Where a comment or a string is left open at the end of a line.
#[derive(Default)]
struct Open {
    block: bool,
    string: Option<&'static str>,
}

/// How much deeper in brackets the end of `line` is than its start; comments and strings do
/// not count.
fn deeper(line: &str, l: Lang, open: &mut Open) -> i32 {
    let b = line.as_bytes();
    let (mut i, mut d) = (0, 0);
    while i < b.len() {
        let r = &b[i..];
        if open.block {
            if r.starts_with(b"*/") {
                open.block = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(end) = open.string {
            if b[i] == b'\\' {
                i += 2;
            } else if r.starts_with(end.as_bytes()) {
                open.string = None;
                i += end.len();
            } else {
                i += 1;
            }
            continue;
        }
        if l.slash && r.starts_with(b"//") || l.hash && b[i] == b'#' || l.dash && r.starts_with(b"--") || l.semi && b[i] == b';' {
            break;
        }
        if l.slash && r.starts_with(b"/*") {
            open.block = true;
            i += 2;
            continue;
        }
        if let Some(q) = ["\"\"\"", "'''", "\"", "`"].into_iter().find(|q| r.starts_with(q.as_bytes())) {
            open.string = Some(q);
            i += q.len();
            continue;
        }
        if b[i] == b'\'' {
            if l.quote {
                open.string = Some("'");
                i += 1;
            } else {
                // A character ('x', '\n', 'é') is skipped; a lifetime ('a) is not one.
                let end = if r.get(1) == Some(&b'\\') { 3 } else { 2 };
                i += r.iter().take(6).skip(end).position(|&c| c == b'\'').map_or(1, |p| end + p + 1);
            }
            continue;
        }
        match b[i] {
            b'{' | b'(' | b'[' => d += 1,
            b'}' | b')' | b']' => d -= 1,
            _ => {}
        }
        i += 1;
    }
    d
}

fn lines(text: &str, l: Lang) -> Vec<Line<'_>> {
    let mut open = Open::default();
    let mut depth = 0;
    text.lines()
        .map(|text| {
            let indent = text.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum();
            let line = Line { text, depth, indent };
            depth = (depth + deeper(text, l, &mut open)).max(0);
            line
        })
        .collect()
}

/// A comment, an attribute, an annotation or a decorator: it goes with the item below it.
fn is_note(t: &str) -> bool {
    let t = t.trim_start();
    ["//", "/*", "*", "#", "@", "--", ";", "%", "\"\"\"", "'''", "(*"].iter().any(|s| t.starts_with(s)) || t.starts_with('[') && t.trim_end().ends_with(']') && !t.contains('=')
}

/// A line that ends a block or goes on with one: no item starts there.
fn is_closer(t: &str) -> bool {
    let t = t.trim_start();
    let word = t.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
    t.starts_with(['}', ')', ']']) || matches!(word, "end" | "else" | "elif" | "elsif" | "except" | "finally" | "catch" | "rescue" | "ensure" | "fi" | "done" | "esac" | "until" | "then" | "when" | "case" | "default" | "where")
}

/// Words before an item's keyword that say how it is seen or kept.
const MODIFIERS: &[&str] = &[
    "pub", "export", "default", "public", "private", "protected", "internal", "static", "async", "unsafe", "extern", "abstract", "final", "sealed", "open", "override", "inline", "virtual", "local", "partial", "readonly", "declare", "data", "inner", "suspend", "synchronized", "noinline", "constexpr", "lazy",
];

/// Words an item starts with.
const ITEMS: &[&str] = &[
    "fn", "def", "class", "struct", "enum", "trait", "impl", "interface", "func", "function", "module", "mod", "namespace", "type", "record", "object", "union", "protocol", "extension", "actor", "defmodule", "defp", "defmacro", "defn", "defun", "defclass", "defstruct", "defprotocol", "macro", "macro_rules!", "sub", "proc", "procedure", "method", "const", "let", "var", "val", "typedef", "typealias", "instance", "newtype", "create", "alter", "contract", "library", "message", "service", "resource",
];

/// Words that start a statement, not an item.
const STATEMENTS: &[&str] = &["if", "for", "while", "switch", "match", "return", "loop", "do", "try", "with", "assert", "print", "echo", "yield", "await", "throw", "raise", "sizeof"];

/// The name of the item `line` starts, and whether a keyword says it is one: "fn sources",
/// "class Cart", "impl Store", "CREATE TABLE links", "kv_hash()"; "" for none.
fn item(line: &str) -> (String, bool) {
    let t = line.trim();
    let mut words = t.split_whitespace().peekable();
    while let Some(&w) = words.peek() {
        let bare = w.split('(').next().unwrap_or(w);
        let lower = bare.to_ascii_lowercase();
        // `const fn`, `pub(crate)`, `extern "C"`.
        if MODIFIERS.contains(&bare) || lower == "const" && words.clone().nth(1) == Some("fn") || w.starts_with('"') {
            words.next();
        } else {
            break;
        }
    }
    let rest: Vec<&str> = words.collect();
    let Some(&first) = rest.first() else { return (String::new(), false) };
    let key = first.trim_start_matches('(').to_ascii_lowercase();
    if ITEMS.contains(&key.as_str()) && rest.len() > 1 {
        let name = |w: &str| w.split(|c: char| !(c.is_alphanumeric() || "_$!?.:-".contains(c))).next().unwrap_or("").trim_end_matches([':', '.']).to_string();
        let shown = first.trim_start_matches('(');
        let label = match key.as_str() {
            // `impl<T> Display for Tree<T>` up to its brace.
            "impl" | "extension" | "instance" => format!("{shown} {}", rest[1..].join(" ").split(['{', '=']).next().unwrap_or("").split(" where").next().unwrap_or("").trim()),
            // SQL: `CREATE TABLE IF NOT EXISTS links`.
            "create" | "alter" => {
                let w: Vec<&str> = rest.iter().skip(1).filter(|w| !matches!(w.to_ascii_lowercase().as_str(), "or" | "replace" | "if" | "not" | "exists" | "temporary" | "temp" | "unique" | "materialized")).copied().collect();
                format!("{shown} {}", w.iter().take(2).copied().map(name).collect::<Vec<_>>().join(" "))
            }
            // Go's methods: `func (s *Store) Save(`.
            "func" if rest[1].starts_with('(') => format!("func {}", rest.iter().skip_while(|w| !w.contains(')')).nth(1).map_or(String::new(), |w| name(w))),
            _ => format!("{shown} {}", name(rest[1])),
        };
        return (label.chars().take(80).collect(), true);
    }
    // `uint64_t kv_hash(const char *key)`, `public Loan lend(String isbn)`: the name before the
    // first parenthesis, unless it is a call that is a statement.
    if let Some(open) = t.find('(') {
        let before = &t[..open];
        let name = before.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
        let call = before.contains('=') || STATEMENTS.contains(&key.as_str()) || t.ends_with(';') || before.trim().is_empty() || before.contains('.') || !before.trim().contains(' ') && !t.trim_end().ends_with('{');
        if !name.is_empty() && !call && !STATEMENTS.contains(&name) {
            return (format!("{name}()"), false);
        }
    }
    // `[dependencies]`.
    if t.starts_with('[') && t.ends_with(']') {
        return (t.chars().take(60).collect(), false);
    }
    (String::new(), false)
}

/// The first lines of the items at `level` from `a` to `b`, `a` among them: where a line at
/// that level starts an item, after a blank line, a block's end or a statement's `;`, or
/// with a keyword; with the comments and attributes right above it.
fn starts(ls: &[Line], a: usize, b: usize, level: (i32, usize)) -> Vec<usize> {
    let mut out = vec![a];
    for j in a + 1..b {
        let l = &ls[j];
        if l.blank() || l.level() != level || is_closer(l.text) || is_note(l.text) {
            continue;
        }
        let prev = &ls[j - 1];
        let after = prev.blank() || prev.level() == level && is_closer(prev.text) || prev.depth == level.0 && prev.text.trim_end().ends_with(';') || is_note(prev.text);
        // Only comments since the last start: they are this item's.
        if !(after || item(l.text).1) || ls[*out.last().unwrap()..j].iter().all(|l| l.blank() || is_note(l.text)) {
            continue;
        }
        let mut k = j;
        while k - 1 > *out.last().unwrap() && !ls[k - 1].blank() && is_note(ls[k - 1].text) && ls[k - 1].indent == level.1 {
            k -= 1;
        }
        if k > *out.last().unwrap() {
            out.push(k);
        }
    }
    out
}

fn bytes(ls: &[Line], a: usize, b: usize) -> usize {
    ls[a..b].iter().map(|l| l.text.len() + 1).sum()
}

/// The name of the item from `a` to `b`: its first line that is no comment.
fn name_of(ls: &[Line], a: usize, b: usize) -> String {
    ls[a..b].iter().find(|l| !l.blank() && !is_note(l.text)).map(|l| item(l.text).0).unwrap_or_default()
}

/// `outer › inner`.
fn within(outer: &str, inner: String) -> String {
    match (outer.is_empty(), inner.is_empty()) {
        (true, _) => inner,
        (_, true) => outer.to_string(),
        _ => format!("{outer} › {inner}"),
    }
}

/// Lines `a` to `b` in pieces of `MOST` bytes or fewer, `outer` the item they are in: whole
/// items where they fit, the items inside one that does not, else its paragraphs.
fn cut(ls: &[Line], a: usize, b: usize, outer: &str, out: &mut Vec<(usize, usize, String)>) {
    let Some(level) = ls[a..b].iter().filter(|l| !l.blank() && !is_note(l.text) && !is_closer(l.text)).map(Line::level).min() else { return paragraphs(ls, a, b, outer, out) };
    let items = starts(ls, a, b, level);
    for (i, &s) in items.iter().enumerate() {
        let e = items.get(i + 1).copied().unwrap_or(b);
        let name = within(outer, name_of(ls, s, e));
        if bytes(ls, s, e) <= MOST {
            out.push((s, e, name));
        } else {
            inside(ls, s, e, &name, out);
        }
    }
}

/// One item too long for a passage, `name` its name: the items inside it, or its paragraphs.
fn inside(ls: &[Line], a: usize, b: usize, name: &str, out: &mut Vec<(usize, usize, String)>) {
    let head = ls[a..b].iter().position(|l| !l.blank() && !is_note(l.text)).map_or(a, |p| a + p);
    let outer = ls[head].level();
    let inner = ls[head + 1..b].iter().filter(|l| !l.blank() && !is_note(l.text) && !is_closer(l.text) && l.level() > outer).map(Line::level).min();
    if let Some(level) = inner {
        let first = (head + 1..b).find(|&j| ls[j].level() == level && !ls[j].blank()).unwrap_or(b);
        if starts(ls, first, b, level).len() > 1 {
            // The item's own first lines, then what is inside it.
            out.push((a, first, name.to_string()));
            return cut(ls, first, b, name, out);
        }
    }
    paragraphs(ls, a, b, name, out);
}

/// Lines `a` to `b` in pieces of `MOST` bytes at most, cut at blank lines where there are
/// some, else at a line.
fn paragraphs(ls: &[Line], a: usize, b: usize, name: &str, out: &mut Vec<(usize, usize, String)>) {
    let (mut s, mut blank, mut size) = (a, None, 0);
    for j in a..b {
        let n = ls[j].text.len() + 1;
        if size + n > MOST && j > s {
            let e = blank.filter(|&k| k > s && bytes(ls, s, k) >= LEAST).unwrap_or(j);
            out.push((s, e, name.to_string()));
            size = bytes(ls, e, j);
            s = e;
            blank = None;
        }
        if ls[j].blank() {
            blank = Some(j);
        }
        size += n;
    }
    if s < b {
        out.push((s, b, name.to_string()));
    }
}

/// The passages of code: whole items where they fit, small ones together, each named after
/// the items it holds ("impl Store › fn passages"); the lines as they are, without the blank
/// ones at either end. With whether each starts an item, for `meaning::passages`.
pub(crate) fn passages(text: &str, path: &str) -> Vec<(Passage, bool)> {
    let Some(l) = lang(path) else { return vec![] };
    let ls = lines(text, l);
    if ls.is_empty() {
        return vec![];
    }
    let mut pieces = vec![];
    cut(&ls, 0, ls.len(), "", &mut pieces);
    // Small pieces go with the next while they fit.
    let mut merged: Vec<(usize, usize, Vec<String>)> = vec![];
    for (a, b, name) in pieces {
        match merged.last_mut() {
            Some(m) if m.1 == a && (bytes(&ls, m.0, m.1) < LEAST || bytes(&ls, a, b) < LEAST / 2) && bytes(&ls, m.0, b) <= MOST => {
                m.1 = b;
                // "class Cart" and then "class Cart › def total": the second says both.
                if m.2.last().is_some_and(|l| name.starts_with(&format!("{l} › "))) {
                    m.2.pop();
                }
                if !name.is_empty() && !m.2.contains(&name) {
                    m.2.push(name);
                }
            }
            _ => merged.push((a, b, if name.is_empty() { vec![] } else { vec![name] })),
        }
    }
    merged
        .into_iter()
        .map(|(a, b, names)| {
            let text = ls[a..b].iter().map(|l| l.text.trim_end()).collect::<Vec<_>>().join("\n").trim_matches('\n').to_string();
            let heading = if names.len() > 3 { format!("{}, …", names[..3].join(", ")) } else { names.join(", ") };
            let starts = !heading.is_empty();
            (Passage { text, heading }, starts)
        })
        .filter(|(p, _)| !p.text.trim().is_empty())
        .collect()
}

// ---------------------------------------------------------------- the map of a project

/// Folders a project's map does not go into: what is built or fetched, not written.
const BUILT: &[&str] = &["target", "node_modules", "dist", "build", "out", "vendor", "__pycache__", "venv", "bin", "obj", "Pods", "DerivedData", "coverage", "site-packages", "zig-cache", "zig-out", "_build", "deps"];

/// Files a project names what it uses in, by name.
pub(crate) fn is_project_file(name: &str) -> bool {
    matches!(
        name,
        "Cargo.toml" | "package.json" | "pyproject.toml" | "setup.cfg" | "setup.py" | "Pipfile" | "go.mod" | "pom.xml" | "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" | "Directory.Packages.props" | "Gemfile" | "composer.json" | "Package.swift" | "mix.exs" | "CMakeLists.txt" | "meson.build" | "Makefile" | "deno.json" | "pubspec.yaml" | "Dockerfile" | "Containerfile" | "compose.yaml" | "compose.yml" | "docker-compose.yml" | "docker-compose.yaml" | "flake.nix" | "DESCRIPTION" | "stack.yaml" | "build.zig" | "Project.toml" | "rebar.config" | "dune-project"
    ) || name.starts_with("requirements") && name.ends_with(".txt")
        || [".csproj", ".fsproj", ".vbproj", ".sln", ".cabal", ".gemspec", ".nimble", ".podspec"].iter().any(|e| name.ends_with(e))
}

/// Whether `dir` is a project: a repository, or a folder with a project file.
pub fn is_project(dir: &Path) -> bool {
    dir.join(".git").exists() || std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| is_project_file(&e.file_name().to_string_lossy()))
}

/// The name in a requirement: `fastapi>=0.110` → `fastapi`, `github.com/go-chi/chi/v5 v5.0.12` → the path.
fn requirement(s: &str) -> String {
    s.trim().trim_matches(['"', '\'', ',']).split(|c: char| c.is_whitespace() || "<>=!~;[@^".contains(c)).next().unwrap_or("").to_string()
}

/// What a project file says the project is and what it uses: its description, and the names
/// of what it depends on.
fn uses(name: &str, text: &str) -> (String, Vec<String>) {
    let mut desc = String::new();
    let mut names: Vec<String> = vec![];
    let quoted = |l: &str| -> Vec<String> { l.split(['"', '\'']).skip(1).step_by(2).map(requirement).filter(|s| !s.is_empty()).collect() };
    let attr = |l: &str, a: &str| l.split(&format!("{a}=\"")).nth(1).and_then(|r| r.split('"').next()).map(String::from);
    if name.ends_with(".json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            desc = v["description"].as_str().unwrap_or("").to_string();
            for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies", "require", "require-dev", "imports"] {
                names.extend(v[key].as_object().into_iter().flat_map(|o| o.keys().cloned()));
            }
        }
        return (desc, names);
    }
    let (mut section, mut array, mut block, mut indent) = (String::new(), false, false, 0);
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let key = t.split(['=', ':']).next().unwrap_or("").trim().trim_matches('"');
        if desc.is_empty() && matches!(key, "description" | "summary" | "Description" | "Title") && t.len() > key.len() + 2 {
            desc = t[key.len()..].trim_start_matches(|c: char| c == '=' || c == ':' || c.is_whitespace()).trim_matches(['"', '\'', ',']).to_string();
        }
        if name.ends_with(".toml") || name == "Pipfile" {
            if t.starts_with('[') && !array {
                section = t.trim_matches(['[', ']']).to_string();
                // `[dependencies.serde]`
                if let Some(dep) = section.split_once("dependencies.").map(|(_, d)| d) {
                    names.push(dep.to_string());
                }
                continue;
            }
            let deps = section.ends_with("dependencies") || section.ends_with("packages") || section == "deps";
            if array {
                names.extend(quoted(t));
                array = !t.contains(']');
            } else if (key.contains("dependencies") || key == "requires") && t.contains('[') {
                names.extend(quoted(t.split_once('[').map_or("", |(_, r)| r)));
                array = !t.contains(']');
            } else if deps && t.contains("= [") {
                names.extend(quoted(t.split_once('[').map_or("", |(_, r)| r)));
                array = !t.contains(']');
            } else if deps && t.contains('=') {
                names.push(key.split('.').next().unwrap_or(key).to_string());
            }
        } else if name.starts_with("requirements") {
            if !t.starts_with('-') {
                names.push(requirement(t));
            }
        } else if name == "go.mod" {
            if t.starts_with("require (") {
                block = true;
            } else if t == ")" {
                block = false;
            } else if block || t.starts_with("require ") {
                names.push(requirement(t.trim_start_matches("require ")));
            }
        } else if name.ends_with(".xml") || name.ends_with("proj") || name.ends_with(".props") {
            if t.starts_with("<dependency>") {
                block = true;
            } else if t.starts_with("</dependency>") {
                block = false;
            } else if block && t.starts_with("<artifactId>") {
                names.push(t.trim_start_matches("<artifactId>").split('<').next().unwrap_or("").to_string());
            } else if desc.is_empty() && t.starts_with("<description>") {
                desc = t.trim_start_matches("<description>").split('<').next().unwrap_or("").to_string();
            }
            if t.contains("PackageReference") || t.contains("PackageVersion") {
                names.extend(attr(t, "Include"));
            }
        } else if name.contains("gradle") {
            let word = t.split(['(', ' ', '\'', '"']).next().unwrap_or("");
            if ["implementation", "api", "compileOnly", "runtimeOnly", "testImplementation", "kapt", "ksp", "annotationProcessor"].contains(&word) {
                names.extend(t.split(['"', '\'']).nth(1).map(|c| c.split(':').nth(1).unwrap_or(c).to_string()));
            }
        } else if name == "Gemfile" || name.ends_with(".gemspec") {
            if t.starts_with("gem ") || t.contains("add_dependency") || t.contains("add_runtime_dependency") {
                names.extend(quoted(t).into_iter().take(1));
            }
        } else if name == "mix.exs" || name == "rebar.config" {
            names.extend(t.split("{:").skip(1).map(|r| r.split([',', '}', ' ']).next().unwrap_or("").to_string()));
        } else if name == "Package.swift" {
            if let Some(url) = t.split("url:").nth(1) {
                names.extend(quoted(url).first().map(|u| u.trim_end_matches(".git").rsplit('/').next().unwrap_or("").to_string()));
            }
        } else if name == "CMakeLists.txt" {
            let lower = t.to_ascii_lowercase();
            for call in ["find_package(", "pkg_check_modules(", "fetchcontent_declare("] {
                if let Some(r) = lower.find(call).map(|i| &t[i + call.len()..]) {
                    names.extend(r.split([' ', ')']).find(|w| !w.is_empty()).map(String::from));
                }
            }
        } else if name == "meson.build" {
            names.extend(t.split("dependency(").skip(1).flat_map(|r| quoted(r).into_iter().take(1)));
        } else if name == "pubspec.yaml" || name == "stack.yaml" {
            if !line.starts_with(' ') {
                section = key.to_string();
            } else if section.ends_with("dependencies") && line.len() - line.trim_start().len() <= indent.max(2) {
                indent = line.len() - line.trim_start().len();
                names.push(key.to_string());
            }
        } else if matches!(name, "Dockerfile" | "Containerfile") {
            if t.to_ascii_uppercase().starts_with("FROM ") {
                names.extend(t.split_whitespace().nth(1).map(String::from));
            }
        } else if name.contains("compose")
            && let Some(image) = t.strip_prefix("image:") {
                names.push(image.trim().trim_matches(['"', '\'']).to_string());
            }
    }
    names.retain(|n| !n.is_empty() && n != "python" && n != "php");
    names.dedup();
    (desc, names)
}

/// What a file of code says it is: its first comment, or a Python docstring, without licence
/// lines; one line.
fn doc_line(text: &str) -> String {
    for line in text.lines().take(30) {
        let t = line.trim();
        if t.is_empty() || t.starts_with("#!") || t.starts_with("#include") || t.starts_with("#[") || t.starts_with("#pragma") || t.starts_with("#ifndef") || t.starts_with("#define") || t.contains("-*-") {
            continue;
        }
        let words = t.trim_start_matches(['/', '!', '*', '#', '-', ';', '%', '"', '\'', '(', ' ']).trim_end_matches(['*', '/', '"', '\'', ')', ' ']);
        let lower = words.to_ascii_lowercase();
        if !is_note(t) {
            // Past the comments at the top: the file says nothing of itself.
            if !(t.starts_with("package ") || t.starts_with("module ") || t.starts_with("use ") || t.starts_with("import ") || t.starts_with("from ") || t.starts_with("namespace ") || t.starts_with('<')) {
                return String::new();
            }
            continue;
        }
        if words.chars().filter(|c| c.is_alphabetic()).count() < 8 || ["copyright", "spdx", "license", "licence", "all rights", "@ts-", "eslint", "prettier", "noinspection", "type:"].iter().any(|w| lower.contains(w)) {
            continue;
        }
        return words.chars().take(160).collect();
    }
    String::new()
}

/// Whether an item is public, as its language says: `pub`, `export`, `public`, a capital in
/// Go, no `_` in front in Python and the like, not `static` in C.
fn public(line: &str, path: &str, name: &str) -> bool {
    let t = line.trim_start();
    let ext = path.rsplit('.').next().unwrap_or("");
    let bare = name.split_whitespace().last().unwrap_or("").trim_end_matches("()");
    match ext {
        "rs" => t.starts_with("pub ") || t.starts_with("pub(crate)") || t.starts_with("impl"),
        "go" => bare.starts_with(|c: char| c.is_uppercase()),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => t.starts_with("export"),
        "java" | "cs" | "php" | "dart" => t.contains("public ") || t.starts_with("class ") || t.starts_with("interface ") || t.starts_with("record ") || t.starts_with("enum "),
        "kt" | "kts" | "swift" | "scala" => !t.contains("private ") && !t.contains("fileprivate ") && !t.contains("internal "),
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" => !t.starts_with("static ") && !t.starts_with("#"),
        _ => !bare.starts_with('_') && !t.starts_with("defp ") && !t.starts_with("local "),
    }
}

/// The public items of a file of code, as their first line says them, `most` at most.
fn signatures(text: &str, path: &str, most: usize) -> Vec<String> {
    let Some(l) = lang(path) else { return vec![] };
    if !path.rsplit('.').next().is_some_and(|e| !matches!(e, "json" | "jsonc" | "json5" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "css" | "scss" | "less" | "sql" | "nix")) {
        return vec![];
    }
    let mut out = vec![];
    for line in lines(text, l) {
        if line.depth > 1 || line.indent > 4 || line.blank() || is_note(line.text) || is_closer(line.text) {
            continue;
        }
        let (name, keyword) = item(line.text);
        let t = line.text.trim();
        // An item without a keyword (`int main(void)`, a Java method) opens a block.
        let fits = keyword && !matches!(name.split_whitespace().next(), Some("let" | "var" | "val" | "const")) || !keyword && !name.is_empty() && !name.starts_with('[') && (t.ends_with('{') || t.ends_with(')'));
        if fits && public(line.text, path, &name) {
            let sig = t.trim_end_matches(['{', ':']).trim_end();
            out.push(if sig.chars().count() > 120 { format!("{}…", sig.chars().take(120).collect::<String>()) } else { sig.to_string() });
            if out.len() == most {
                out.push("…".into());
                break;
            }
        }
    }
    out
}

/// A map of the project in `dir`, about `room` bytes at most: each project file with what it
/// says the project is and uses, each workflow by its name, and each file of code with what it
/// says it is and its public items, the files nearest the top first. Folders `skip` names,
/// hidden ones and built ones are left out. Empty when `dir` is no project.
// ponytail: read at each broad question, at most 4,000 files and 24 MB; keep it in the store
// if a large monorepo makes Ask wait for it.
pub fn map(dir: &Path, room: usize, skip: &[String]) -> String {
    if !is_project(dir) {
        return String::new();
    }
    let mut files: Vec<(usize, String)> = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            if is_dir && (name.starts_with('.') && name != ".github" && name != "workflows" || BUILT.contains(&name.as_str()) || skip.contains(&name)) || files.len() >= 4000 {
                continue;
            }
            let rel = e.path().strip_prefix(dir).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if is_dir {
                stack.push((e.path(), depth + 1));
            } else if is_code(&name) || is_project_file(&name) {
                files.push((depth, rel));
            }
        }
    }
    files.sort();
    let mut read = 0usize;
    let mut text_of = |rel: &str| -> String {
        let mut buf = vec![];
        if read <= 24 << 20
            && let Ok(f) = std::fs::File::open(dir.join(rel)) {
                let _ = f.take(512 << 10).read_to_end(&mut buf);
            }
        read += buf.len();
        String::from_utf8(buf).unwrap_or_default()
    };
    let mut projects: Vec<String> = vec![];
    let mut code: Vec<(String, String, String)> = vec![];
    for (_, rel) in &files {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let text = text_of(rel);
        if rel.starts_with(".github/workflows/") {
            let title = text.lines().find_map(|l| l.strip_prefix("name:")).map_or(String::new(), |n| format!(": {}", n.trim().trim_matches(['"', '\''])));
            projects.push(format!("{rel}{title}\n"));
        } else if is_project_file(name) {
            let (desc, names) = uses(name, &text);
            let desc = if desc.is_empty() { String::new() } else { format!(": {desc}") };
            let names = if names.is_empty() { String::new() } else { format!("; uses {}", names.join(", ")) };
            projects.push(format!("{rel}{desc}{names}\n"));
        } else {
            code.push((rel.clone(), doc_line(&text), text));
        }
    }
    let mut out = String::new();
    let fits = |out: &String, line: &str| out.len() + line.len() <= room;
    if !projects.is_empty() {
        let head = "Project files and what they use:\n";
        if fits(&out, head) {
            out.push_str(head);
        }
        for line in &projects {
            if !fits(&out, line) {
                break;
            }
            out.push_str(line);
        }
    }
    let head = "\nFiles of code and their public items:\n";
    if code.is_empty() || !fits(&out, head) {
        return out;
    }
    out.push_str(head);
    // Every file with its line first; the items as far as they fit, the same number for each.
    let lines: usize = code.iter().map(|(r, d, _)| r.len() + d.len() + 4).sum();
    let per = room.saturating_sub(out.len() + lines) / code.len() / 70;
    for (rel, doc, text) in &code {
        let line = if doc.is_empty() { format!("{rel}\n") } else { format!("{rel}: {doc}\n") };
        if out.len() + line.len() > room {
            break;
        }
        out.push_str(&line);
        for sig in signatures(text, rel, per.min(12)) {
            if out.len() + sig.len() + 3 > room {
                break;
            }
            out.push_str(&format!("  {sig}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut_up(text: &str, path: &str) -> Vec<(String, String)> {
        passages(text, path).into_iter().map(|(p, _)| (p.heading, p.text)).collect()
    }

    /// `n` lines of filler at `indent`.
    fn body(n: usize, indent: &str, end: &str) -> String {
        (0..n).map(|i| format!("{indent}let value_{i} = compute_something_long(alpha, beta, gamma){end}\n")).collect()
    }

    #[test]
    fn rust_is_cut_at_its_items_with_their_doc_comments() {
        let text = format!(
            "//! The fuel module.\n\nuse std::io;\n\n/// How much fuel a stage burns.\n#[inline]\npub fn burn(stage: u32) -> u32 {{\n{}}}\n\npub struct Tank {{\n    litres: u32,\n}}\n\nimpl Tank {{\n    /// Fill it.\n    pub fn fill(&mut self) {{\n{}    }}\n\n    fn drain(&mut self) {{\n{}    }}\n}}\n",
            body(20, "    ", ";"),
            body(14, "        ", ";"),
            body(14, "        ", ";")
        );
        let p = cut_up(&text, "src/fuel.rs");
        let burn = p.iter().find(|(h, _)| h.contains("fn burn")).expect("fn burn");
        assert!(burn.1.contains("/// How much fuel a stage burns.\n#[inline]\npub fn burn"), "the doc comment and attribute go with it: {}", burn.1);
        assert!(burn.1.contains("\n    let value_0"), "indentation stays: {}", burn.1);
        assert!(p.iter().any(|(h, t)| h.ends_with("impl Tank › fn fill") && t.contains("    /// Fill it.\n    pub fn fill")), "{p:#?}");
        assert!(p.iter().any(|(h, _)| h == "impl Tank › fn drain"), "{p:#?}");
        assert!(p.iter().all(|(_, t)| t.len() <= MOST), "{p:#?}");
    }

    #[test]
    fn small_items_go_together() {
        let text = "def a():\n    return 1\n\n\ndef b():\n    return 2\n\n\nclass C:\n    pass\n";
        let p = cut_up(text, "x.py");
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].0, "def a, def b, class C");
        assert!(p[0].1.starts_with("def a():\n    return 1\n\n\ndef b()"), "{}", p[0].1);
    }

    #[test]
    fn python_classes_are_cut_at_their_methods_and_decorators_stay() {
        let text = format!("import os\n\n\nclass Cart:\n    \"\"\"A cart.\"\"\"\n\n    @property\n    def total(self):\n{}\n    def add(self, sku):\n{}", body(16, "        ", ""), body(16, "        ", ""));
        let p = cut_up(&text, "shop/cart.py");
        assert!(p.iter().any(|(h, t)| h.ends_with("class Cart › def total") && t.contains("\n\n    @property\n    def total")), "{p:#?}");
        assert!(p.iter().any(|(h, _)| h == "class Cart › def add"), "{p:#?}");
    }

    #[test]
    fn every_family_of_languages_is_cut_at_its_items() {
        let long = |open: &str, close: &str, indent: &str| format!("{open}\n{}{close}\n", body(18, indent, ""));
        let cases = [
            ("lib/cart.rb", format!("# A cart.\n{}\n{}", long("def total", "end", "  "), long("def add(sku)", "end", "  ")), ["def total", "def add"]),
            ("lib/cart.ex", format!("{}\n{}", long("defmodule Cart do\n  def total(c) do", "  end\nend", "    "), long("defmodule Tax do\n  def vat(n) do", "  end\nend", "    ")), ["defmodule Cart", "defmodule Tax"]),
            ("src/cart.lua", format!("{}\n{}", long("local function total(c)", "end", "  "), long("function M.add(sku)", "end", "  ")), ["function total", "function M.add"]),
            ("src/cart.clj", format!(";; Carts.\n{}\n{}", long("(defn total [c]", "  )", "  "), long("(defn add [sku]", "  )", "  ")), ["defn total", "defn add"]),
            ("db/schema.sql", format!("-- Links.\n{}\n{}", long("CREATE TABLE IF NOT EXISTS links (", ");", "  "), long("CREATE INDEX links_code ON links (", ");", "  ")), ["CREATE TABLE links", "CREATE INDEX links_code"]),
            ("src/table.c", format!("#include <stdlib.h>\n\n/* The hash. */\n{}\n{}", long("uint64_t kv_hash(const char *key)\n{", "}", "    "), long("static void grow(kv_table *t)\n{", "}", "    ")), ["kv_hash()", "grow()"]),
            ("src/Loans.java", format!("package x;\n\n/** Loans. */\npublic class Loans {{\n{}\n{}}}\n", long("    /** Lends. */\n    @Transactional\n    public Loan lend(String isbn) {", "    }", "        "), long("    public Fine giveBack(String isbn) {", "    }", "        ")), ["class Loans › lend()", "class Loans › giveBack()"]),
            ("web/handlers.go", format!("package web\n\n{}\n{}", long("// Router mounts the routes.\nfunc Router(db *Store) http.Handler {", "}", "\t"), long("func (s *Store) Save(code string) error {", "}", "\t")), ["func Router", "func Save"]),
            ("src/api.ts", format!("{}\n{}", long("/** Fetch. */\nexport async function request<T>(path: string): Promise<T> {", "}", "  "), long("export const useTasks = create((set) => ({", "}));", "  ")), ["function request", "const useTasks"]),
        ];
        for (path, text, names) in &cases {
            let p = cut_up(text, path);
            for name in names {
                assert!(p.iter().any(|(h, _)| h.contains(name)), "{path}: no {name} in {:?}", p.iter().map(|(h, _)| h).collect::<Vec<_>>());
            }
            assert!(p.iter().all(|(_, t)| t.len() <= MOST), "{path}");
        }
        let java = cut_up(&cases[6].1, cases[6].0);
        assert!(java.iter().any(|(_, t)| t.contains("\n    /** Lends. */\n    @Transactional\n    public Loan lend")), "{java:#?}");
    }

    #[test]
    fn brackets_in_strings_comments_and_characters_do_not_count() {
        let l = lang("a.rs").unwrap();
        let mut open = Open::default();
        assert_eq!(deeper("let s = \"{(\"; // {{{", l, &mut open), 0);
        assert_eq!(deeper("if c == '{' || c == '\\'' { f::<'a>(", l, &mut open), 2);
        assert_eq!(deeper("/* { */ }", l, &mut open), -1);
        let py = lang("a.py").unwrap();
        assert_eq!(deeper("x = '{' # (", py, &mut open), 0);
        assert_eq!(deeper("s = \"\"\"{", py, &mut open), 0);
        assert_eq!(deeper("}\"\"\" + (", py, &mut open), 1);
    }

    #[test]
    fn text_that_is_no_code_has_no_code_passages() {
        assert!(!is_code("notes.md") && !is_code("report.txt") && is_code("CMakeLists.txt") && is_code("Dockerfile") && is_code("a/b.tsx"));
        assert!(passages("Some words.", "notes.md").is_empty());
    }

    #[test]
    fn project_files_say_what_the_project_uses() {
        let cargo = "[package]\nname = \"core\"\ndescription = \"The core\"\n\n[dependencies]\nserde = { version = \"1\" }\nrusqlite.workspace = true\n\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n\n[dependencies.candle-core]\nversion = \"0.9\"\n";
        assert_eq!(uses("Cargo.toml", cargo), ("The core".into(), vec!["serde".into(), "rusqlite".into(), "libc".into(), "candle-core".into()]));
        let py = "[project]\ndescription = \"A shop\"\ndependencies = [\n    \"fastapi>=0.110\",\n    \"uvicorn[standard]>=0.29\",\n]\n[project.optional-dependencies]\ndev = [\"pytest>=8\"]\n";
        assert_eq!(uses("pyproject.toml", py).1, ["fastapi", "uvicorn", "pytest"]);
        assert_eq!(uses("package.json", r#"{"description":"Web","dependencies":{"react":"^18"},"devDependencies":{"vite":"^5"}}"#), ("Web".into(), vec!["react".into(), "vite".into()]));
        assert_eq!(uses("go.mod", "module x\n\nrequire (\n\tgithub.com/go-chi/chi/v5 v5.0.12\n)\nrequire go.uber.org/zap v1.27.0\n").1, ["github.com/go-chi/chi/v5", "go.uber.org/zap"]);
        let pom = "<project>\n<artifactId>library</artifactId>\n<description>Lending</description>\n<dependencies>\n<dependency>\n<groupId>org.postgresql</groupId>\n<artifactId>postgresql</artifactId>\n</dependency>\n</dependencies>\n</project>\n";
        assert_eq!(uses("pom.xml", pom), ("Lending".into(), vec!["postgresql".into()]));
        assert_eq!(uses("App.csproj", "<PackageReference Include=\"Serilog\" Version=\"3\" />").1, ["Serilog"]);
        assert_eq!(uses("build.gradle.kts", "dependencies {\n    implementation(\"io.ktor:ktor-server-core:2.3.0\")\n}\n").1, ["ktor-server-core"]);
        assert_eq!(uses("Gemfile", "source 'https://rubygems.org'\ngem 'rails', '~> 7.1'\n").1, ["rails"]);
        assert_eq!(uses("CMakeLists.txt", "find_package(ZLIB REQUIRED)\nfind_package(Threads)\n").1, ["ZLIB", "Threads"]);
        assert_eq!(uses("mix.exs", "  defp deps do\n    [{:phoenix, \"~> 1.7\"}, {:ecto_sql, \"~> 3.10\"}]\n").1, ["phoenix", "ecto_sql"]);
        assert_eq!(uses("requirements.txt", "# web\nflask==3.0\n-r base.txt\nrequests>=2\n").1, ["flask", "requests"]);
        assert_eq!(uses("Dockerfile", "FROM rust:1.80 AS build\nRUN cargo build\n").1, ["rust:1.80"]);
    }

    #[test]
    fn the_map_names_files_their_items_and_what_the_project_uses() {
        let d = std::env::temp_dir().join(format!("coxswain-code-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for dir in ["src", "target/debug", ".github/workflows", "node_modules/x"] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
        }
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"rocket\"\ndescription = \"A rocket planner\"\n[dependencies]\nserde = \"1\"\n").unwrap();
        std::fs::write(d.join("src/fuel.rs"), "//! Fuel budgets.\n\nuse std::io;\n\n/// The fuel.\npub fn burn(stage: u32) -> u32 {\n    stage * 2\n}\n\nfn hidden() {}\n\npub struct Tank {\n    litres: u32,\n}\n\nimpl Tank {\n    pub fn fill(&mut self) {\n    }\n}\n").unwrap();
        std::fs::write(d.join("src/main.py"), "\"\"\"The launcher.\"\"\"\n\ndef launch():\n    pass\n\ndef _secret():\n    pass\n").unwrap();
        std::fs::write(d.join("target/debug/built.rs"), "pub fn built() {}\n").unwrap();
        std::fs::write(d.join(".github/workflows/ci.yml"), "name: CI\non: push\n").unwrap();
        let m = map(&d, 10_000, &[]);
        assert!(m.contains("Cargo.toml: A rocket planner; uses serde\n") && m.contains(".github/workflows/ci.yml: CI\n"), "{m}");
        assert!(m.contains("src/fuel.rs: Fuel budgets.\n  pub fn burn(stage: u32) -> u32\n  pub struct Tank\n  impl Tank\n  pub fn fill(&mut self)\n"), "{m}");
        assert!(m.contains("src/main.py: The launcher.\n  def launch()\n") && !m.contains("_secret") && !m.contains("hidden") && !m.contains("built"), "{m}");
        assert!(map(&d, 120, &[]).len() <= 120);
        assert!(map(&d.join("src"), 10_000, &[]).is_empty(), "no project");
        std::fs::remove_dir_all(d).unwrap();
    }
}

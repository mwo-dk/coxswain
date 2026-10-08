[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask about code

[Ask](ask.md) reads code as code. A question about a program (*how does the search helper
start?*, *which web framework does this use?*, *where is VAT added to a price?*) gets whole
functions and classes with their lines and indentation, each named after the item it is in, and
a question about the project as a whole gets a map of it: what each file says it is, its public
items, and what its project files say it uses.

It works in both apps the same way, for any language, with no setting: there is nothing to turn
on. It needs what Ask needs ([What it needs](ask.md#what-it-needs)).

## Contents

- [What changes for code](#what-changes-for-code)
- [A question about code](#a-question-about-code)
- [The map of a project](#the-map-of-a-project)
- [Which languages](#which-languages)
- [How well it does](#how-well-it-does)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## What changes for code

Prose is cut into passages of about 120 words, joined into one line
([How it works](meaning.md#how-it-works)). Code cut that way lost its line breaks and was cut in
the middle of a function, so the chat model read fragments it could not follow. A file of code
is now cut like this:

- **at its items**: a function, a method, a class, a struct, an `impl`, a module, a top-level
  block, an SQL statement. Small items (a few `use` lines, two short functions) go together; an
  item longer than about 1,500 bytes is cut at the items inside it (the methods of a class), and
  a long function at its blank lines;
- **with its lines and indentation as they are**, and one blank line where the file has some;
- **with its comments**: the doc comment, the attributes, annotations and decorators above an
  item (`///`, `/** */`, `#[…]`, `@Override`, `@property`) stay with it; a Python docstring is
  inside it already;
- **named**: each passage carries the items it holds, *impl Store › fn passages*,
  *class Cart › def total*, *CREATE TABLE links*. Search by meaning sees the name with the file's
  name and folder; Ask's excerpt of code starts with it (`› impl Store › fn passages`), so the
  model knows where it is even in the middle of a long function.

Neighbouring passages of code join into one excerpt line by line. What you see in Find's list
(*About this*, the snippet of each hit) is unchanged.

The first scan after the update reads your code files again and gives every passage a new
vector (Find and Settings say *Renewing search by meaning: … files to go, about …*); words inside files
are searchable all the while. See [Why is search by meaning re-reading everything?](meaning.md#why-is-search-by-meaning-re-reading-everything).

## A question about code

Ask takes a question for one about code when most of what it found is code (half the sources
or more are files of code; project, configuration and data files such as `Cargo.toml`,
`package.json` or `config.yaml` do not count), or when the question names something in code:
`coxswain_core::menu`, `passages()`, `src/store.rs`, `MAX_LOAD`, `kv_hash`, `fetchTasks`. No
model decides it: it is worked out from the sources and the words, at once.

What it changes: the line over the sources says so. A question about code gets the same excerpts as any other; only a
question about the whole project gets the [map](#the-map-of-a-project). Giving a specific
question the map too was measured and left out: the excerpts it pushed out said more
([Performance → Code questions](../reference/performance.md#code-questions)).

The dim line over the sources says it: *Code question: 9 excerpts from 6 files, about 2000
words* in place of *9 excerpts from 6 files, about 2000 words*, in both apps.

## The map of a project

A question about the project as a whole, its architecture, its frameworks, libraries or
dependencies (*what is the architecture of this repository?*, *which crates does the terminal
app use?*, *welche Bibliotheken nutzt das Projekt?*) gets the overview of the folder first
([A question about the whole folder](ask.md#how-much-it-reads)). When the folder is a project (it
has `.git` or a project file at its top), the overview holds its map, in up to half of the room:

1. **each project file** with its description and the names of what it depends on:
   `Cargo.toml`, `package.json`, `composer.json`, `deno.json`, `pyproject.toml`,
   `requirements*.txt`, `setup.cfg`, `Pipfile`, `go.mod`, `pom.xml`, `build.gradle(.kts)`,
   `*.csproj`, `*.fsproj`, `Directory.Packages.props`, `Gemfile`, `mix.exs`, `Package.swift`,
   `pubspec.yaml`, `CMakeLists.txt`, `meson.build`, `Makefile`, `Dockerfile`, `compose.yaml`,
   `flake.nix` and others;
2. **each workflow** in `.github/workflows/` with its name;
3. **each file of code**, nearest the top first, with the first line of its first comment
   (`//!`, a docstring, a package comment) and its public items, as their first line says them:
   `pub fn context(cfg: &SearchConfig) -> usize`, `export async function request<T>(…)`,
   `public Loan lend(String isbn, String member)`. Public is what the language says: `pub`,
   `export`, `public`, a capital letter in Go, no `_` in front in Python, not `static` in C.

The README gets a quarter of the room and the tree of folders an eighth then. The map leaves out
hidden folders, the folders in *Left out everywhere* (`text_exclude`), the names in the
`.gitignore` and folders that are built or fetched (`target`, `node_modules`, `dist`, `build`,
`vendor`, …). It is made from the files on disk when the question is asked, at most 4,000 files
and 24 MB, so it is never out of date. Only the names of dependencies are given: nothing is
looked up on the internet.

## Which languages

No parser: brackets, indentation, keywords and comments, the same rules for every language.

| Family | Languages | An item ends where |
|---|---|---|
| Braces | C, C++, C#, Java, Kotlin, Scala, Swift, Go, Rust, JavaScript, TypeScript, PHP, Dart, Zig, Svelte, Vue | its brackets close, at the left edge |
| Indentation | Python, YAML, F#, Nim, CoffeeScript | the next line at its indentation |
| `end` | Ruby, Lua, Elixir, Julia, shell, Crystal | the next line at its indentation |
| Parentheses | Lisp, Clojure, Scheme, Racket, Emacs Lisp | its parentheses close |
| Statements | SQL | its `;` |
| Configuration | TOML, INI, JSON, Nix, Makefile, Dockerfile, CMake | a blank line or a `[section]` |

Brackets inside strings, comments and character literals (`'{'`) do not count. A file of a kind
not in the list (Markdown, plain text, HTML) is read as prose, as before.

## How well it does

Measured with `crates/coxswain-core/tests/code_eval.rs`: 20 questions about this repository at a
fixed commit and 20 about small projects in Python, TypeScript, Java, Go and C, each with the
files that hold the answer and the facts the answer must name. *Right file* is how often a file
that holds the answer is among what Ask reads; *facts* the share of the facts the answer names.
bge-m3 and qwen3:8b on Ollama, on an RTX 4070 Laptop GPU (8 GB).

| | Right file read | Facts in what was read | Facts in the answer |
|---|---|---|---|
| This repository, 20 questions, before (2.17) | 13 of 20 | 0.75 | 0.51 |
| This repository, with code read as code (2.18) | 16 of 20 | 0.79 | 0.57 |
| Five small projects, 20 questions, before | 19 of 20 | 0.93 | 0.78 |
| Five small projects, with code read as code | 20 of 20 | 0.98 | 0.88 |

The small projects fit almost whole into the room, so they show mostly what the map adds (the
C project's *which libraries does it link against?* found nothing before). In this repository,
whose docs say much of what its code does, the questions about the terminal app's crates, the
GUI framework and the architecture now get the project files from the map.

The numbers per language and on the processor are in
[Performance → Code questions](../reference/performance.md#code-questions).

## Settings and config.toml

None of its own. What Ask reads is set as for Ask: `ask_context` (the room, 8,192 tokens by
default) and `text_exclude` (folders left out of the map too) in `[search]`.

## Questions

#### Does Ask understand code now?

It reads it as code: whole functions with their lines, named after their item, and for a
question about the project a map of its files, public items and dependencies. The answer is
still the chat model's. A small model (Qwen3 1.7B, a model on the processor with 2,048 tokens of
room) sees one or two items; a model with a larger context sees many. Ask in Find:
**Ctrl+Enter** in the desktop app, **Alt+Enter** in the terminal app, or **Ctrl+F7** in both.

#### Which languages are cut at their functions?

Every language in the table under [Which languages](#which-languages): braces, indentation,
`end`, parentheses and SQL. Others with a known ending are read with their lines kept. Markdown
and plain text are read as prose, as before.

#### Why is search by meaning re-reading my files?

The way files are cut changed (passages scheme 3): every vector is made again, code files are
read again with their indentation. Find and Settings say *Renewing search by meaning: … files
to go, about …* until it is done. Word search works meanwhile; nothing is downloaded.

#### Why does a question about libraries get the README and a list of files?

Words like *architecture*, *framework*, *library*, *dependencies*, *crates* (in the app's
languages) make it a question about the project as a whole: Ask reads the overview first, with
the map of the project, then the closest excerpts. The overview's sources are listed under the
answer like the others; **Enter** on the folder's source opens it in the active panel.

#### Why does the line over the sources say *Code question*?

Half or more of what Ask found is files of code, or the question names something in code (a
path, `name()`, `snake_case`, `camelCase`, `a::b`). It reads the same excerpts as for any
question; the line only says how the question was taken.

#### Does the map send anything anywhere?

No. It is read from the files on disk, on this machine, when you ask; with a server's chat
model it goes to that server with the rest of the sources, as every source does
([What is sent, and where?](ask.md#what-is-sent-and-where)).

#### My project is large. Is the map complete?

It reads up to 4,000 files of code and project files and 24 MB, the files nearest the top
first, and gives each file its line and as many public items as fit, the same number for each.
In a large project, ask about a part: switch Find's scope to the active panel's folder (**Ctrl+F**
inside Find, both apps) with the panel on that part, and the map is of that folder when it has a project file.

---
[← Previous: Ask](ask.md) · [Next: Ask without a server →](ask-builtin.md)

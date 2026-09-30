[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Name syntax

Names are searched with the syntax of [Everything](https://www.voidtools.com/), the Windows
search tool: words, `!`, `|`, wildcards, `ext:`, `file:`, `folder:`, `case:` and paths. It is
case-insensitive unless you ask. It applies to [names everywhere](names.md) and
[names in this folder](names.md#names-in-this-folder), not to [text in files](text.md).

![The terminal app's Find file with "*.rs|*.toml src/" typed: Rust files in src folders, six hits](../screenshots/tui-search.png)
*`*.rs|*.toml src/`: names ending in `.rs` or `.toml`, under a folder named `src`.*

## How to use it

1. Open [Find file](find-file.md) with **Alt+F7** or **Ctrl+F** (names everywhere), or **Tab** once
   for names in this folder.
2. Type terms separated by spaces. Every term must match.
3. **F1** in the terminal app lists the syntax; in the desktop app, **F1** (Help) lists it under
   *Find file uses Everything's syntax:*. The desktop field's placeholder shows a few:
   *Search every file…  \*.rs · ext:md · src/ foo · !test*.

| Query | Matches |
|---|---|
| `foo bar` | Names containing both |
| `foo\|bar` | Either |
| `!foo` | Names without foo |
| `*.rs`, `a?c` | Wildcards over the whole name: `*` any run, `?` one character |
| `ext:rs;toml` | Files with one of these extensions |
| `file:` / `folder:` | Only files / only folders |
| `src/ lib` | A term with `/` matches the full path: here anything under a `src` folder with `lib` in its name |
| `case:` | Case-sensitive, for the whole query |
| `"a b"` | A phrase with a space in it |

They combine: `ext:pdf invoice !draft`, `folder: src/`, `case: README`. `|` works inside a term
(`ext:jpg|ext:png`), and `!` in front of one (`!ext:log`). Paths are matched with `/` on every
system, so `src/ui` works on Windows too.

## What you see

The hits and the count line of [Find file](find-file.md#what-you-see): `6 matches in 3.48 ms ·
452393 files indexed`. Nothing marks which term matched; with a path term the folder shown under
each hit is where to look.

## Settings and config.toml

None. The syntax cannot be changed.

## In the terminal app

The same syntax, word for word. **F1** lists it, and the footer ends with `syntax: F1`.

## Questions

#### Why does `*.rs` not find `main.rs.bak`?
Wildcards match the whole name. Use `.rs` (a plain substring) or `*.rs*`.

#### How do I find a folder, not files?
`folder: name`. And `file:` for files only.

#### How do I find something under a certain folder?
Put a `/` in a term: `projects/ budget` finds names with "budget" anywhere under a folder whose
path contains `projects/`. Or open the folder and press **Tab** once for
[names in this folder](names.md#names-in-this-folder).

#### How do I leave out a whole folder, like `node_modules`?
`!node_modules/` drops every hit whose path runs through one. To leave it out of the index for
good, see `exclude` in [Names everywhere](names.md#settings-and-configtoml).

#### Does the syntax work in "Text in files"?
No. Text search takes words: every word must be there, the last may be the start of one, and
`!`, `|`, `ext:` and quotes mean nothing there. See [Text in files](text.md).

#### How do I search for a name with a space in it?
Quote it: `"annual report"`. Without quotes it is two terms, both anywhere in the name.

#### Is `case:` for one term or the whole query?
For the whole query. `case: README` finds `README.md` but not `readme.md`.

#### Can I search by size or date?
No. Everything's `size:` and `dm:` are not part of Coxswain's syntax; sort the panel by size or
date instead ([Sorting](../panels/sorting.md)).

---
[← Previous: Names everywhere](names.md) · [Next: Text in files →](text.md)

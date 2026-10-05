[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Name syntax

Names are searched with the syntax of [Everything](https://www.voidtools.com/), the Windows
search tool: words, `!`, `|`, wildcards, `ext:`, `file:`, `folder:`, `case:` and paths. It is
case-insensitive unless you ask. It applies to Find's *Names* group, everywhere or in [one folder](names.md#names-in-this-folder),
not to the words of [In files](text.md). A query with this syntax shows names only.

<!-- screenshot: tui-search.png: the terminal app (Classic blue), Find with "*.rs|*.toml src/" typed: NAMES only, eight hits, two inside a zip -->
*`*.rs|*.toml src/`: names ending in `.rs` or `.toml`, under a folder named `src`.*

## How to use it

1. Open [Find](find-file.md) with **Alt+F7** or **Ctrl+F**; **Ctrl+F** again inside Find
   limits it to the active panel's folder.
2. Type terms separated by spaces. Every term must match.
3. **F1** inside Find shows the syntax and the prefixes in place of the list, in both apps
   (**F1** or **Esc** again for the list); **F1** on the panels (Help) lists it too.

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

The hits and the count line of [Find](find-file.md#what-you-see): `6 matches in 3.48 ms ·
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
path contains `projects/`. Or open the folder and press **Ctrl+F** inside Find for
[names in this folder](names.md#names-in-this-folder).

#### How do I leave out a whole folder, like `node_modules`?
`!node_modules/` drops every hit whose path runs through one. To leave it out of the index for
good, see `name_exclude` in [Names everywhere](names.md#settings-and-configtoml).

#### Does the syntax work for the words in files?
No. A query with name syntax shows names only. The words of *In files* are words: every word must
be there (or, failing that, any of them), the last may be the start of one, and `!`, `|`, `ext:`
and quotes mean nothing there. See [Text in files](text.md).

#### How do I search for a name with a space in it?
Quote it: `"annual report"`. Without quotes it is two terms, both anywhere in the name.

#### Is `case:` for one term or the whole query?
For the whole query. `case: README` finds `README.md` but not `readme.md`.

#### Can I search by size or date?
No. Everything's `size:` and `dm:` are not part of Coxswain's syntax; sort the panel by size or
date instead ([Sorting](../panels/sorting.md)).

---
[← Previous: Names everywhere](names.md) · [Next: Text in files →](text.md)

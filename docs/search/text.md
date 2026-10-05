[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Text in files

Text in files is the *In files* group of [Find file](find-file.md): the files whose text has your
words, from code, notes, PDFs, Word documents, spreadsheets, slides, mail and books. Use it when
you remember what a file says but not what it is called.

<!-- screenshot: search-find-groups.png: the desktop app (Cyber), Find with "engine" typed: In files with main.rs, sequence.puml, launch-pad.drawio and their passages, "engine" highlighted, then History -->
*Seven files with "engine" in them: code, a PlantUML diagram, YAML, a log, a mail and a LaTeX paper.*

## Contents

- [How to use it](#how-to-use-it)
- [How words match](#how-words-match)
- [What you see](#what-you-see)
- [What is read, and when](#what-is-read-and-when)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Press **Ctrl+F** for [Find file](find-file.md): words are searched along with names, and
   *In files* is one of its groups (first for three words or more). For the words alone, press
   **Shift+F7** (or **Ctrl+Shift+F** in the desktop app): Find opens at the *In files* kind. The
   F9 command list calls it *Search inside files*. Inside Find, **Shift+F7** switches to *In
   files* and back, and the prefix `text:` does the same.
2. To search one folder only, switch the scope with **Ctrl+F** inside Find (*In rocket*).
3. Type words: `rocket budget`. Best matches come first.
4. **Enter** goes to the file, **F4** edits it, **F3** views it (terminal app).

## How words match

- **Every word must be there**, anywhere in the file, in any order. When fewer than 10 files
  have them all and you typed two words or more, the files with **any** of the words of four
  letters or more come after them, so a question typed as a question (*what does the rocket
  fuel cost*) still finds the file that says "fuel" and "cost".
- **The last word may be the start of one**, so `rocket bud` finds "rocket budget" while you type.
  The other words are whole words.
- **Case and accents do not matter**: `cafe` finds "Café".
- **Punctuation separates words**, so `fuel_cost` is the words "fuel" and "cost".
- The [name syntax](name-syntax.md) (`!`, `|`, `ext:`) does not apply here, and quotes are ignored.
- With [search by meaning](meaning.md) on, the files with the words are ordered by both: a file
  that has the words and is also close in meaning comes first ([how the two are
  fused](meaning.md#how-words-and-meaning-are-ranked-together)). Files *about* your words that lack
  them follow.

## What you see

The *In files* group: its heading counts the files (*5 of 19*; in *All* the first five show, and
*14 more: Enter shows them all* follows). Under the list: *text of 31,208 files · 412 still to
read*; the last part shows only while the helper still has files to read.

**Each hit:** the name, its folder, and the passage that matched (up to 18 words around your
words, with `…` where it is cut), your words highlighted. Files found by meaning alone are under
*About this*, their passage in italics. Commits are under *History*.

**Nothing to search:** with *Words inside files* off, the *In files* group says *Find can also
search the words inside your files.* with *Turn on*; with the [helper](helper.md) not running,
*Words in files cannot be searched now: background reading is not running.* with *Start it*.
With the scope on a folder that is not read: *… is not among the folders read, so its words are
not searched.* with *Read this folder too* ([all states](find-file.md#when-something-is-missing)).

## What is read, and when

| | |
|---|---|
| **Which folders** | Your home folder, unless you choose others ([Choosing the folders](folders.md)) |
| **Which files** | Anything that is plain text (code, Markdown, logs, configuration …), the [documents](documents.md) Coxswain reads, [diagrams](diagrams.md) as sentences, the files [inside archives](archives.md), and with installed programs, [scans and older Office files](scans.md). Up to 20 MB each (`text_max_size`), and at most 4 MB of text from one file |
| **Left out** | Hidden folders (names starting with a dot); folders named `node_modules`, `target`, `build`, `dist`, `out`, `vendor`, `__pycache__` or `Trash` (`text_exclude`); folders marked *Names only*; any folder holding a file named `.nosearch`; pictures, video and other files without text; symbolic links; files locked with a password, inside archives too; the files inside archives in caches and programs' folders ([Which archives](archives.md#which-archives)) |
| **When** | In the background, by the [search helper](helper.md): a batch of files, then a rest as long as the work took (at most two seconds), so it runs at about half speed. Not while a laptop is on its [battery](battery.md). Changes are read within seconds |
| **Also kept** | Every file's size and date, and the total of each folder left out, so [folder sizes](../panels/folder-sizes.md) are sums; the hashes of files that share a size, for [Duplicates](../files/duplicates.md) |
| **Where** | `search.db` in Coxswain's cache folder, readable by you alone ([Where things are kept](../reference/where-things-are-kept.md)) |

Coxswain reads the documents itself, starting no other program (except the
[optional ones](scans.md) you install), and no reader touches the network. A file that cannot be
read (damaged, locked, not what its name says) is marked as without text and tried again when it
changes.

**Following changes.** The helper follows the file watcher: it collects changes for two seconds,
then reads the files that changed. A change inside a folder left out is measured for folder sizes
within a minute. A walk every ten minutes catches anything the watcher missed.

## Settings and config.toml

*Settings → Finding files → Details* (desktop app); `[search]` in `config.toml`. Every item is in
[Search settings](settings.md).

| Key | Type | Default | Does |
|---|---|---|---|
| `text` | bool | `true` | *What is read → Words inside files* |
| `text_roots` | list of paths | `[]` (your home folder) | *Folders → Folders read* |
| `text_exclude` | list of strings | `["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash"]` | *Folders → Left out everywhere*: folder names and file patterns left out, wherever they are |
| `names_only` | list of paths | `[]` | *Folders → Names only* |
| `text_max_size` | bytes | `20971520` (20 MB) | *What is read → Largest file read (MB)*: larger files are not read |
| `archives` | bool | `true` | *What is read → Look inside archives*: the files in zip, 7z and tar archives are read too ([Inside archives](archives.md)) |

## In the terminal app

The same store, the same hits, found by the same helper. The passage goes on a second line under
each name, your words in the `search_hit` colour. There is no Settings window: set the keys in
`config.toml`, and the helper takes them at its next start ([questions](folders.md#questions)).

<!-- screenshot: tui-text-search.png: the terminal app (Classic blue), Find at In files with "engine" typed: each hit a name and folder, the passage on the next line, "engine" highlighted -->

## Questions

#### Why doesn't a file I just saved show up in text search?
It is read within seconds of the save, if all of these hold:

- It is inside a [folder that is read](folders.md) (your home folder by default), and not in a
  folder left out (hidden, `node_modules`, `target` …, names only, `.nosearch`).
- It is a kind with text, and no larger than 20 MB.
- The machine is not on its [battery](battery.md): reading waits for the mains.
- The helper is not still reading a backlog: Find's footer says ` · 412 still to read`, and the
  new file waits its turn. *Read now* in Settings reads the backlog at full speed.

#### Why does `budge` not find "budgets"?
Only the last word may be a start; the others must be whole words. Put the partial word last, or
type it in full.

#### Text search finds nothing at all.
Check *Settings → Finding files*: the *Words* line must not say *Off* (choose the level *Names and
text*) or *Not running* (press **Start it**). Right after the first start the
store is still being filled: ` · 31,000 still to read` counts down.

#### Can I search the text in one folder only?
Yes: open the folder in the active panel and press **Ctrl+F** inside Find. The scope reads *In
rocket*, and *In files* (with every other group, and Ask) keeps to that folder and below. The
folder must be among the [folders read](folders.md); if it is not, *Read this folder too* adds it.

#### Why does a question find files with only some of its words?
When fewer than 10 files have every word, files with any of the words of four letters or more are
added, so *what does the rocket fuel cost* finds the budget that says "fuel" and "cost". With
search by meaning on, such a file is shown only when meaning finds it too.

#### Does anything leave my machine?
Not for search by words: the text stays in `search.db` on your disk. Search by meaning with the
built-in model stays on the machine too; with a [server on another machine](servers.md) the text is
sent to it. See [Privacy](../reference/privacy.md).

#### How big does `search.db` get?
About the size of the text in your files, plus a little for each file known. Settings shows it on the
*Words* line of *Finding files*: *Files read: 31,208 · waiting: 0 · 412 MB on disk*, and on the
button *Delete what was read (412 MB)*.

#### How do I start the index afresh?
*Settings → Finding files → Details → Background reading → Delete what was read (412 MB)*, then click again (*Click again to delete*). It
empties `search.db`, sizes, hashes and vectors included, and the helper fills it again from the
start. Or delete `search.db` while no Coxswain runs.

#### Why is only part of a very large file found?
At most 4 MB of text is kept from one file, and a very long PDF is read for three seconds. Words
past that are not in the store.

---
[← Previous: Name syntax](name-syntax.md) · [Next: Documents it reads →](documents.md)

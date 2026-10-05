[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Performance: what keeps Coxswain quick

Coxswain is meant to feel instant: a key moves the cursor before anything is read from disk, a
folder of 100,000 files lists in a fraction of a second, and nothing that takes a while runs
where it would freeze the window. This page says what each app does for that, and gives the
numbers measured on synthetic data, so you know what to expect and can measure again.

- [What both apps do](#what-both-apps-do)
- [The desktop app](#the-desktop-app)
- [The terminal app](#the-terminal-app)
- [The numbers](#the-numbers)
- [Search quality](#search-quality)
- [Measuring again](#measuring-again)
- [Questions](#questions)

## What both apps do

- **Listing** reads a folder once (`read_dir`, one `stat` per entry) and sorts it in natural
  order (`file2` before `file10`, case ignored) without allocating per comparison.
- **Folder sizes** are measured in the background by two threads, the first folders on screen
  first, and stop when you leave the folder. Sizes under your home folder come from the search
  helper's store at once. See [Folder sizes](../panels/folder-sizes.md).
- **Find file** searches an in-memory index of every name (a million names in about 140 MB)
  in a few milliseconds; the index is shared by every window and the terminal app through the
  helper, and loads from its cache in well under a second. See [Names everywhere](../search/names.md).
- **git status** runs on its own thread, so a big repository never holds up the listing; the
  statuses of a folder's entries come from git's answer alone, without reading the folder again.
- **Search follows a changed archive member by member:** a member that kept its size and date
  in the archive keeps its text and vectors; only the changed ones are read again. An archive
  written in the last three seconds (a download) waits until it has settled.
- **Search by meaning reads only what it shows:** the few hundred closest passages are ranked
  by their vectors alone, and the text of a file is read only when one of its passages is
  among the hits.
- **Git's walk never holds a listing:** a folder sorted by commit lists by name at once when
  the walk is not done yet, and is sorted again when it is; the map of last commits is sent to
  the page only when HEAD moved; the repository's `.git` is watched, so a commit brings the
  status and the column up to date without a reread by hand (desktop app).
- **Ask** has Ollama load the chat model while the sources are looked up, and **Esc** stops
  the wait for the first word at any moment.
- **Anything that takes a while** says so: while files are copied, moved, deleted, extracted
  or packed, the status line reads *Working on …*; a folder that takes longer than 150 ms to
  read says *Working on <folder>…* until it arrives.

## The desktop app

- **Only the rows on screen are in the page.** The details view and the Miller columns keep
  a screenful of rows plus a few beyond each edge in the DOM, and empty space of the right
  height for the rest. A folder of 100,000 files costs what a folder of 40 does: it lists in
  about 30 ms once the data is there, and a cursor move takes a millisecond. Every row is
  exactly `font_size × line_height` pixels high, so the list never jumps. Thumbnails do the
  same with rows of tiles: a folder of 100,000 files opens in thumbnails in 15 ms.
- **A listing is sent small.** An entry's path is left out when it is the folder's path and
  the name (the page joins them), and so are flags that are off and tags that are not set:
  100,000 entries are 14 MB of JSON instead of 26.
- **The window's thread does no disk work.** Every command that reads or writes a file, the
  clipboard or the config runs on the async runtime; the heavy ones (git, searches, folder
  sizes, duplicates, previews made by tools, the text of a preview, a folder's properties) go
  to blocking threads of their own, so a slow share holds none of the runtime's few workers.
- **Each tab keeps its scroll position**; switching tabs brings the list back where it was.
- **Previews wait for the cursor to settle.** Text, data and images load 80 ms after the
  cursor stops; previews made by tools (LibreOffice, LaTeX) wait longer and only start by
  themselves when the tool is quick. A preview that comes back for a file you have moved on
  from is dropped. Highlighting runs on a worker, off the window's thread, and a newer
  preview drops the one still being highlighted; it stops at 200,000 characters, bigger text
  shows plain.
- **Find file** shows at most 500 hits (the count still says how many there are); duplicates
  show 500 groups; the BOM tree draws the branches that are open.
- **Folders watched** reread themselves at most four times a second while something writes
  into them.
- **The session** is saved 600 ms after the last change, and git status writes the state file
  only when a repository moves up the recent list.

## The terminal app

- **A frame is built from the rows on screen only**, about half a millisecond with 100,000
  entries in both panels, whether or not anything is marked (the marked bytes are kept, not
  summed every frame).
- **Sorting again sorts the entries it has**; it does not reread the folder.
- **Searches, git status, folder sizes, Ask and the update check** run on threads and arrive
  between frames; the main loop polls for keys every 200 ms (30 ms while a search is out).
- **Copy, move, delete, extract and pack** run on a thread of their own; the keys keep
  working, and the status line says *Working on …* with the item it is on (`(2/3)`). One runs
  at a time; quitting waits for it to finish, so no half-copied file is left.
- **A history's commits** are read by git on a thread, so a long history (or a rarely changed
  file in a big repository, which git has to walk the whole history for) never holds up keys.

## The numbers

Measured on 2026-10-01 on a 22-core Linux machine, tmpfs, release builds, headless Chromium
for the page. "Before" is version 1.23.1, and 1.26.0 for the rows about JSON size,
thumbnails, highlighting, the terminal app's copies and histories, and archives, and 1.27.3
for the rows about the search store and git (measured on 2026-10-03). A folder of 100,000
files and 1,000 subfolders; an index of a million names in a thousand folders.

### Listing and sorting (coxswain-core)

| What | Before | After |
|---|---|---|
| List 100,000 entries (read_dir + stat) | 101 ms, +27 MB | 101 ms, +27 MB |
| Sort 100,000 by name (natural order) | 93–111 ms | 82 ms |
| Sort by extension | 101–131 ms | 91 ms |
| Sort by time / size | 25 ms / 8 ms | 20 ms / 6 ms |
| Measure the folder's size (dir_size) | 30 ms | 30 ms |

### Find file (a million names)

| What | Time |
|---|---|
| Build the index from disk | 88 ms, 138 MB |
| `mod_500_7` (111 hits) | 0.5 ms |
| `*.rs` (111,000 hits, 500 shown) | 4 ms |
| `mod 12 3` (two words, 13,680 hits) | 7 ms |
| `src99/ toml` (a path part) | 4.5 ms |
| Same, scoped to one folder | 1–15 ms |
| Save / load the cache | 16 ms / 62 ms |

### The desktop app

| What | Before | After |
|---|---|---|
| `list_dir` body for 100,000 entries (list, sort, icons) | 200 ms | 200 ms, on a blocking thread |
| Its JSON (sent to the page) | 26 MB, 29 ms to write, 64 ms to parse | 14 MB, 15 ms to write, 50 ms to parse, 1 ms to join the paths |
| Page: show 1,000 rows | 222 ms | 37 ms |
| Page: show 20,000 rows | 4.0 s, 576 MB heap | 33 ms, 15 MB |
| Page: show 100,000 rows | not done after 10 minutes | 32 ms, 37 MB |
| Page: move the cursor one row (20,000 rows) | 60–90 ms | 1 ms |
| Page: cursor to the end / home (100,000 rows) | – | 15 ms / 11 ms |
| Page: reread the same folder (20,000 rows) | 1.25 s | 4 ms |
| Page: the folder sorted the other way (20,000 rows) | 18.9 s | 12 ms |
| Page: thumbnails, show 1,000 / 20,000 / 100,000 files | 115 ms / 2.9 s, 716 MB / – | 6 ms / 17 ms, 13 MB / 14 ms, 36 MB |
| Page: thumbnails, folder sorted the other way (20,000) | 16.1 s | 7 ms |
| Page: thumbnails, cursor one tile (20,000) | 50 ms | 0.5 ms |
| Page: a short folder after scrolling far down a long one | blank (no rows drawn) | its rows |
| Page: highlighting a 170 KB source file | 30 ms (by its type), 180 ms (guessed), on the window's thread | on a worker; 0.4 ms on the window's thread |
| Page: a disk-touching command (save session, tags, notes, rename plan, clipboard) | on the window's thread | on the async runtime |
| Start-up pieces: default config and keymap, texts, state file | 0.1 ms, 3 ms, 0.2 ms | the same |

### The terminal app

| What | Before | After |
|---|---|---|
| Start with 100,000 entries in both panels (list, sort) | 471 ms | 390 ms |
| A frame, nothing changed (200×50) | 0.55 ms | 0.54 ms |
| Down + a frame | 0.56 ms | 0.54 ms |
| A frame with every entry marked | 16.6 ms | 0.54 ms |
| Sort by size | 286 ms (a reread) | 6 ms |
| Copy, move, delete, extract, pack | on the main loop: no keys until done | on a thread: keys at once, `(2/3)` progress |
| A history's list of commits (git log) | on the main loop | on a thread |

### Archives and git history (both apps)

A zip, a tar.gz and a solid 7z (as 7-Zip makes them) of 10,000 files.

| What | Before | After |
|---|---|---|
| Preview of a file in a solid 7z, any but the first | failed (`ChecksumVerificationFailed`) | 29 ms |
| F5, F6, F8 of files in a solid 7z; search reading its text | failed after the first file skipped | works |
| Preview of the first file in a tar.gz | 31 ms (unpacked to the end) | 0.3 ms |
| Preview of the first file in a zip | 14 ms | 9 ms |
| Archives whose listing is kept | 1 (the preview of another pushed it out) | 4 |
| A history's commits and its *Last commit* column | `git log` twice (three times sorted by commit) | once, kept until HEAD moves |
| A last-commit walk git was stopped in (4 s) | kept as if whole until HEAD moved | looked at again |

### The search store and git (both apps)

A zip of 10,000 small text files under the home folder; 400 files of 100 KB with their
vectors from an embedding server that answers at once (so the store's own work is measured).

| What | Before | After |
|---|---|---|
| The store follows one member added to the zip (`refresh`) | 1,490 ms (every member read again) | 105 ms (one member read) |
| A member that did not change | a new row, read and embedded again | keeps its row, text and vectors |
| An archive still being downloaded | unpacked again at every change | waits until three seconds after its last write |
| Search by meaning, `similar` / `passages` over 400 files × 100 KB | 21 ms (the text of every candidate read) | 2–3 ms (the text of the ten files shown) |
| A folder sorted by commit | listed after git's walk (seconds in a big repository) | listed at once by name, sorted when the walk answers; from the cache after |
| The map of last commits on a reread of the folder | sent to the page every time | sent when HEAD moved |
| The status and the *Last commit* column after a commit from the command line | stale until a reread by hand | current within a moment (`.git` is watched) |
| A file's history past a rename | stopped at the rename | goes on (`git log --follow`) |
| Ask: Esc while the model loads | waited for the first word, up to five minutes | stops within 100 ms; Ollama loads the model while the sources are looked up |
| Name index after many archive edits | the entries left behind stayed until the hourly rebuild | rebuilt when a quarter of the nodes are gone |

## Search quality

How often search by meaning and Ask find the right file, measured on 5 October 2026 (before any
change to ranking), so later changes can be measured against it.

**Method.** `crates/coxswain-core/tests/search-quality/` holds 38 documents of the kind people
keep: Markdown and text notes, recipes, invoices in `invoices/2025/` and `invoices/2024/`, a
CV, meeting notes, a CSV, a Mermaid and two draw.io diagrams, Danish house rules and
recipes, and three long documents of about 1,900–2,000 words (an employee handbook, a project
report, the Danish minutes of a housing association meeting). `questions.tsv` has 32
questions written the way a user asks, each with the file or files that answer it and a phrase
of the passage that holds the answer:

| Kind | Questions | What it tests |
|---|---|---|
| meaning | 13 | Paraphrases of what a short file says, in English and Danish |
| late | 6 | The answer is past the first 960 words of a long file |
| cross | 5 | A Danish question for an English file, or an English one for a Danish file |
| name | 3 | The question names the file or its folder (*my cv*, *invoices from 2025*) |
| words | 3 | The question shares its key words with the file |
| diagram | 2 | The answer is a box or an arrow in a diagram |

The test copies the corpus to a folder of its own, scans it with a store of its own, and
asks each question of three lists, taking the first 20 of each: words alone (`Store::search`,
as *Text in files* without meaning), meaning alone (`Store::similar`), and the combined list
*Text in files* shows (`helper::with_meaning`: the word hits, then the meaning hits the words
missed). Recall@k is the share of questions with an expected file in the first k; MRR is the
mean of 1 / the rank of the first expected file (0 when it is not there). For Ask, it takes
the 10 passages `Store::passages` sends to the chat model and checks whether one of them comes
from the expected file, and whether one holds the answer's phrase.

**Built-in model** (multilingual-e5-small), 38 files read and embedded in 15 s:

| List | Recall@1 | Recall@5 | MRR |
|---|---|---|---|
| Words alone (*Text in files*) | 0.09 | 0.09 | 0.09 |
| Meaning alone | 0.69 | 0.91 | 0.78 |
| Combined list (words, then meaning) | 0.69 | 0.91 | 0.78 |

Ask: the right file among the 10 passages for 31 of 32 questions, the passage that answers
for 24 of 32.

| Kind | Meaning recall@5 | Combined recall@5 | Ask: passage that answers |
|---|---|---|---|
| meaning | 0.92 | 0.92 | 13 of 13 |
| late | 0.83 | 0.83 | 0 of 6 |
| cross | 0.80 | 0.80 | 3 of 5 |
| name | 1.00 | 1.00 | 3 of 3 |
| words | 1.00 | 1.00 | 3 of 3 |
| diagram | 1.00 | 1.00 | 2 of 2 |

**Ollama, bge-m3**, the same corpus:

| List | Recall@1 | Recall@5 | MRR |
|---|---|---|---|
| Words alone (*Text in files*) | 0.09 | 0.09 | 0.09 |
| Meaning alone | 0.75 | 0.81 | 0.78 |
| Combined list (words, then meaning) | 0.78 | 0.84 | 0.81 |

Ask: the right file among the 10 passages for 26 of 32 questions, the passage that answers
for 23 of 32.

| Kind | Meaning recall@5 | Combined recall@5 | Ask: passage that answers |
|---|---|---|---|
| meaning | 1.00 | 1.00 | 13 of 13 |
| late | 0.50 | 0.67 | 0 of 6 |
| cross | 0.60 | 0.60 | 3 of 5 |
| name | 0.67 | 0.67 | 2 of 3 |
| words | 1.00 | 1.00 | 3 of 3 |
| diagram | 1.00 | 1.00 | 2 of 2 |

**What is weak, by these numbers:**

- **Answers past the first 960 words are never sent to Ask** (0 of 6 with either model). A
  file gets vectors for its first 8 passages of 120 words only (`meaning::passages`), so the
  expenses chapter of the handbook (word 1,459) or the roof decision in the minutes (word
  1,102) has no vector. The file is still often found, by its early passages (5 of 6 with the
  built-in model), but Ask gets the wrong part of it.
- **Words alone find almost nothing for a question** (0.09): *Text in files* wants every word
  of the query in the file, and a question has words the file does not (*how*, *I*, *my*).
  The 3 hits are questions whose every word is in the file.
- **The combined list is the meaning list with word hits put first**, not merged: with the
  built-in model it is no better than meaning alone, and a word hit that is a poorer match
  would push a better meaning hit down.
- **bge-m3 shows too few meaning hits**: one file for 19 of 32 questions and none for 6. The
  cut-off (scores at least 0.5, and within 0.10 of the best) was set from e5's and bge-m3's
  scores on a few examples; on this corpus it drops right answers. The built-in model, with
  its own cut-off, shows 3–20 files.
- **File names and folders are not in the passages**: *my cv* and *invoices from 2025* are
  found because the text says *Data engineer* and *INVOICE*, not because of
  `cv/curriculum-vitae.md` or `invoices/2025/`. bge-m3 misses *my cv*.
- **No reranking**: the passages are ranked by one vector each. Danish questions for English
  files rank the right file 3rd or 4th with the built-in model and miss it with bge-m3.

To measure again, with the built-in model installed or Ollama running with `bge-m3`:

```sh
cargo test --release -p coxswain-core --test search_quality -- --ignored --nocapture
COXSWAIN_EVAL_ENGINE=ollama cargo test --release -p coxswain-core --test search_quality -- --ignored --nocapture
```

Each question's line shows its rank in the three lists (`-`: not in the first 20), `P` when the
passage that answers went to Ask (`f`: only other passages of the file, `-`: none), and how many
files meaning found.

## Measuring again

The benchmarks are tests that are ignored unless asked for. They make their data under
`$COXSWAIN_BENCH_DIR` (the temp folder by default) and keep it for the next run.

```sh
export COXSWAIN_BENCH_DIR=/tmp/coxswain-bench
cargo test --release -p coxswain-core --test perf -- --ignored --nocapture   # list, sort, index, store, meaning
cargo test --release -p coxswain --bin coxswain -- --ignored --nocapture perf_ # terminal frames
cargo test --release -p coxswain-gui -- --ignored --nocapture perf_            # list_dir JSON
```

The page is measured with the real details view (or thumbnails, with `&view=grid`) mounted
in a browser: `cd gui && npx vite --port 1421`, then open `http://localhost:1421/bench/index.html?n=100000` (or headless:
`chromium --headless=new --dump-dom <that url>` and read the `<pre id="out">`).

## Questions

**Why does a huge folder still take a moment to open?** Reading 100,000 entries means 100,000
`stat` calls (about 100 ms on a fast disk, more on a network share) and 33 MB of JSON for the
desktop app to parse (70 ms). The page itself is drawn in 30 ms. After 150 ms the status line
says *Working on <folder>…* so you know it is coming.

**Why does holding Down stay smooth in a folder of 100,000 files?** Only the rows on screen
exist in the page; a cursor move changes two of them and scrolls. The preview waits 80 ms
after the last move before it reads anything.

**Can I keep working while the terminal app copies?** Yes: the copy runs on a thread, the
status line says which item it is on, and the keys work meanwhile. A second copy waits until
the first is done (the status line says so), and quitting waits for it too, so nothing is left
half-copied.

**Why does F3 show plain text for a big file?** Syntax highlighting of more than 200,000
characters would take longer than reading the file; the text shows at once instead, cut at
512 KB.

**Does the file watcher cost anything when nothing changes?** No: it sleeps in the kernel's
notification and wakes at most four times a second while files change.

---
[← Previous: Licences and bills of materials](bills-of-materials.md) · [Next: Questions, collected →](../faq.md)

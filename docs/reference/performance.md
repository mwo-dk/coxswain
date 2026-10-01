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
  sizes, duplicates, previews made by tools) go to blocking threads of their own.
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
thumbnails, highlighting, the terminal app's copies and histories, and archives. A folder of 100,000 files and 1,000 subfolders;
an index of a million names in a thousand folders.

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

## Measuring again

The benchmarks are tests that are ignored unless asked for. They make their data under
`$COXSWAIN_BENCH_DIR` (the temp folder by default) and keep it for the next run.

```sh
export COXSWAIN_BENCH_DIR=/tmp/coxswain-bench
cargo test --release -p coxswain-core --test perf -- --ignored --nocapture   # list, sort, index
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
[← Previous: Security](security.md) · [Next: Questions, collected →](../faq.md)

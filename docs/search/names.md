[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Names everywhere

Every file and folder on the machine is in the name index, so Find file finds any of them by name
in milliseconds: 1.4 million files in under 10 ms on a laptop. It needs nothing to be turned on.

![The terminal app's Find file at "everywhere:", with "*.rs|*.toml src/" typed and eight hits, each a name and its folder, two of them inside a zip](../screenshots/tui-search.png)
*Names everywhere: `*.rs|*.toml src/` finds Rust and TOML files in any `src` folder.*

## How to use it

1. **Alt+F7** or **Ctrl+F** opens [Find file](find-file.md) at names everywhere.
2. Type part of a name, or use the [name syntax](name-syntax.md): `ext:pdf invoice !draft`.
3. **Enter** goes to the hit; **F4** edits it; **F3** views it (terminal app).

For names in this folder only, press **Tab** once (below).

## What you see

The prompt `everywhere: ` (terminal app) or the highlighted *Everywhere* button (desktop app).
Above the hits: `128 matches in 3.1 ms · 1,402,311 files indexed`. Each hit is a name and its
folder. While the first index is built, ` · building index…` is added and hits come in as the walk
goes; while a fresh index replaces the saved one after a start, ` · refreshing index`.

## How it stays current

| When | What happens |
|---|---|
| At start | The index saved in Coxswain's cache folder (`index.bin`) loads, in about 90 ms, so searching works at once |
| Right after | A fresh index is built in the background (` · refreshing index` meanwhile) |
| All the time | The file watcher (inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows) adds and removes names: a new file can be found within a second |
| Every hour | The whole index is built again, which catches anything the watcher missed |

**What is indexed:** all of `/` on Linux and macOS, every fixed drive on Windows. Hidden files and
folders are indexed like any other. **Left out:** `/proc`, `/sys`, `/dev`, `/run` and
`/tmp/.X11-unix` by default. In `exclude`, a path skips that tree and a bare name (`node_modules`)
skips every folder of that name.

The index is held by the [search helper](helper.md) and shared by every window and terminal app.
If the helper cannot be reached, each app builds a name index of its own.

## Names in this folder

**Tab** once. The same name search, limited to the active panel's folder and everything below
it. The prompt names the folder (`in ~/projects: `); the desktop app's button reads *In
projects*. The same index answers it, so it is as fast, and the syntax is the same.

## How names stay fast

The index (`crates/coxswain-core/src/index.rs`) keeps each name once in a `\0`-separated byte
buffer, with a 12-byte node per entry (parent, offset, length, flags). A query runs one SIMD
`memmem` scan over that buffer, split across all cores; full paths are built only for hits. Run
`cargo run --release -p coxswain-core --example bench -- /` to measure it on your machine.

Known limits: on Linux, inotify needs one watch per folder, so past `fs.inotify.max_user_watches`
the hourly rebuild catches changes (fanotify would remove the limit, but needs root). On Windows
the first index comes from a folder walk; reading the MFT and USN journal directly, as Everything
does, would make cold starts faster.

## Settings and config.toml

Under `[search]`. None of these is in the Settings window; edit `config.toml`.

| Key | Type | Default | Does |
|---|---|---|---|
| `roots` | list of paths | `[]` | Where the name index looks; empty is `/`, or every fixed drive on Windows |
| `exclude` | list of strings | `["/proc", "/sys", "/dev", "/run", "/tmp/.X11-unix"]` | Paths and folder names left out of the name index. Listing it replaces the default |
| `watch` | bool | `true` | Follow changes live. Off: only the hourly rebuild updates the index |
| `max_results` | number | `10000` | Most hits of one search |

## In the terminal app

The same index, depths and syntax. **F1** lists the syntax. The terminal app shows up to
`max_results` hits, as many as fit; the desktop app lists the first 500.

## Questions

#### Why does a file I just made not show up?
The watcher reports it within a second. On Linux the watcher needs one inotify watch per folder;
with more folders than `fs.inotify.max_user_watches` allows, changes in the rest are caught only by
the hourly rebuild. Raise the limit (`sysctl fs.inotify.max_user_watches=1048576`) or wait. With
`watch = false`, only the hourly rebuild updates the index.

#### Why do I see files from `.cache` and `node_modules`?
Every name on the machine is indexed, hidden ones included. Leave them out of one search with
`!.cache` or `!node_modules/`, or out of the index with
`exclude = ["/proc", "/sys", "/dev", "/run", "node_modules", ".cache"]` (listing `exclude` replaces
the default list, so repeat what you want to keep).

#### Why does the first search after installing say "building index"?
There is no saved index yet. Hits come in as the walk goes: a few seconds to a minute on a large
disk. From the second start on, the saved index answers at once.

#### Can I index only some folders?
Yes: `roots = ["/home/me", "/mnt/data"]` under `[search]`. Empty means the whole machine.

#### Does the name index read my files?
No. It holds names and folders only. Reading the text is a separate, optional depth:
[Text in files](text.md).

#### Why does "In projects" miss a file I can see in the panel?
It searches the index, which leaves out `exclude`d folders. If the file is in one of them, open the
folder and use [Quick search](../panels/quick-search.md), or narrow `exclude`.

#### Does it slow the machine down?
The saved index loads at once; the fresh build after a start and the hourly rebuild walk the disk
in the background. A search is a single scan of memory, over in milliseconds.

---
[← Previous: Find file](find-file.md) · [Next: Name syntax →](name-syntax.md)

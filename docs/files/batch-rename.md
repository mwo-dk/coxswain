[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Batch rename (Ctrl+M)

In the desktop app, **Ctrl+M** renames many files at once with a regular expression and a
counter. Every new name is shown before anything changes, and conflicts are caught first.

<!-- screenshot: files-batch-rename.png: desktop app, Cyber theme: Batch rename 12 items with Find IMG_(\d+) and Replace with Holiday-$1, the live list of old → new names, and one conflict marked red with 'name already taken' -->

## How to use it

1. Mark the entries to rename. With nothing marked, every entry in the folder is taken.
2. Press **Ctrl+M** (or *Batch rename* in the F9 command list).
3. Fill in the fields. The list below updates as you type.
4. Press **Enter** or *Rename*. **Esc** or *Cancel* closes without changing anything.

| Field | Does |
|---|---|
| *Find (regex)* | A regular expression ([Rust syntax](https://docs.rs/regex/latest/regex/#syntax)), e.g. `IMG_(\d+)` |
| *Replace with* | The new text: `$1` or `${name}` for groups, `{n}` for the entry's number in the list (from 1), `{n:3}` for the number padded to three digits (`001`) |
| *Ignore case* | Letters match in either case |
| *Replace all matches* | Every match in the name, not only the first |
| *Include extension* | Match the whole name; unticked, only the part before the extension is matched and the extension is kept |

**Examples:**

| Find | Replace with | `IMG_0042.JPG` becomes |
|---|---|---|
| `IMG_(\d+)` | `Holiday-$1` | `Holiday-0042.JPG` |
| `.*` | `rocket-{n:3}` | `rocket-001.JPG` (the numbers follow the list's order) |
| `\.JPG$` with *Include extension* | `.jpg` | `IMG_0042.jpg` |
| `(\d{4})-(\d{2})-(\d{2})` | `$3.$2.$1` | a date `2026-09-30` in the name becomes `30.09.2026` |
| ` ` with *Replace all matches* | `_` | every space becomes `_` |

## What you see

- The title says *Batch rename 12 items*.
- The list shows every name as `old → new`; names that change are highlighted.
- A rename that cannot happen is marked red with its reason: `empty name`, `contains a path
  separator`, or `name already taken` (two files getting the same name, or a name taken by a
  file that is not itself being renamed).
- A regular expression that does not parse shows its error in red above the list.
- **Rename** stays greyed out while there is a conflict or nothing changes.
- Afterwards the status line says `Renamed 12 items` and the pane is read again.

Swaps work (`a → b` and `b → a` at once): Coxswain first renames every file to a temporary
name, then to its new one.

## Settings and config.toml

None. The key is `batch_rename` in `[keys]`.

## In the terminal app

Not there: the live list of old and new names needs the desktop app's dialog. **Ctrl+M** in the
terminal app says *Batch rename is available in the desktop app (coxswain-gui)*. Rename one file
with **F6**, or run a tool such as `rename` or `mmv` on the [command line](../commands/command-line.md).

## Questions

#### Why is the Rename button greyed out?

A name in the list is red (a conflict), or no name changes. Read the reason next to the red one.

#### In which order does `{n}` count?

The order of the list, which is the folder's current sort order (marked entries in that order).
Sort first (**Ctrl+F3** to **Ctrl+F6**, [Sorting](../panels/sorting.md)) to number by name,
extension, date or size.

#### Why did `$1_x` not work as I expected?

It works: Coxswain reads `$1` followed by `_x`, since people mean group 1. For a named group use
`${name}`.

#### Why was the extension not changed?

With *Include extension* unticked, only the part before the last dot is matched, and the
extension is kept. Tick it to match and change the whole name.

#### Can I rename files inside an archive with it?

No; batch rename works on folders on disk. Inside an archive, rename one entry at a time with
**F6** ([Archives as folders](archives.md)).

#### What if something fails half-way?

Each file is first renamed to a temporary name `.coxswain-rename-…` and then to its new name. If
the system refuses the second step (a disk removed mid-way), that file is left under its
temporary name, and the error is shown in the dialog. Rename it back with **F6**.

---
[← Previous: Drag and drop](drag-and-drop.md) · [Next: Archives as folders →](archives.md)

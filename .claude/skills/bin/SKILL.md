---
name: bin
description: "Model-checks the bin (trash) protocol of crates/coxswain-core/src/bin.rs and xdg_trash.rs with TLA+: every order of put, restore, delete for good and empty by two programs, with a crash between any two steps, against the promise that no file is lost or hidden. Use before and after any change to fd::restore_in, move_new, delete_in, empty_in, list_in or xdg_trash::put_in, and to read a bin bug as a sequence of steps."
---

# The bin: what the checker proves and how to run it

A file the user deletes with **F8** goes to the freedesktop.org bin (`files/<name>` with
`info/<name>.trashinfo`). Coxswain restores it, deletes it for good or empties the bin in
several system calls, and it is never alone on the bin: the terminal app and the desktop app,
the desktop's own trash, a browser, all read and write the same two folders, and any of them can
die between two steps. Tests run one program and no crash. `Bin.tla` beside this file is the
protocol as the code performs it, one model step per system call, and TLC checks every order
of steps of two programs plus a crash anywhere.

## Run it

```
.claude/skills/bin/check.sh
```

Needs `java` (any JRE 11 or newer) and the network once: the first run fetches tla2tools 1.8.0
(4.5 MB, SHA-256 pinned in the script) into `.worktrees/tlaplus/` of the main checkout, which
git ignores. A run takes about 30 seconds on a laptop and prints one line per promise:

```
promise          result put during empty allowed / assumed not
NoLoss           FAIL   with a put during empty: FAIL (…/NoLoss-TRUE.log); without: FAIL (…/NoLoss-FALSE.log)
Restorable       FAIL   with a put during empty: FAIL (…); without: FAIL (…)
NeverOverwrite   PASS
```

Exit 0: every promise holds. Exit 1: a log named on the line holds the counterexample (the
`State n:` blocks, each headed by the model action that made it). Exit 2: the tool itself failed
(no java, a parse error in `Bin.tla`): read the log it names.

**Read every result with the pstack `/statechart` skill**, giving it `Bin.tla` and a `.cfg` from
the run folder: it tells each violation as its shortest path in plain words, and when a promise
holds it draws how the bin moves between states. Do not read the raw `State n:` dumps to the
user.

**What to expect at the sync commit (below):** exactly the table above. `NoLoss` and
`Restorable` fail on the code as it is, through the findings in issues #720 and #721 (told
below); `NeverOverwrite` holds. Anything else is news: a `PASS` where a `FAIL` is expected
means a fix landed (close the issue and update this file); a `FAIL` where `PASS` is expected, or
a trace that is not one of those told below, is a new bug or a model that no longer matches the
code. Decide which with the coverage table.

## The promises, in one sentence each

| Promise | Means | A break costs |
|---|---|---|
| `NoLoss` | A file the user put in the bin, and has not deleted for good or emptied, still exists: at its place, beside it as `name (2)`, or in the bin. | The user's file is gone and nothing said so. |
| `Restorable` | Such a file, when it is only in the bin, is listed there (its `.trashinfo` names its place), so the user sees it and can restore it. Holds at every instant, between steps too. | The file sits in `files/` unlisted, invisible in the panel, and the next Empty deletes it. |
| `NeverOverwrite` | A file at its place is never replaced by another; it only leaves its place by being put in the bin. | A restore silently replaces the new `report.txt` the user saved since. |

What the user has asked to lose: an item `delete_in` has `find`-listed (the user confirmed it by
name and size), and every item `empty_in`'s `list_in` listed when Empty started (the user
confirmed *N items, size*). Anything else the user still wants.

## Bounds and what is left out

Checked: 2 programs, 2 files that share one name (`report.txt`, so names collide in the bin and
at the origin), 2 bin names (`report.txt`, `report.txt (2)`), 2 places (`~/Documents/report.txt`
and `report.txt (2)` beside it), a crash of either program between any two steps, the user
saving a new file at a free place at any time, restores with and without *Restore under a free
name*, every order of Empty's deletions. 7.4 million distinct states; TLC visits all of them.

Not modelled, so not proven:

- Folders: `move_new` restores a folder by `rename`, which replaces an empty folder at the
  place (its own comment says so); `delete_in` removes it with `remove_dir_all`.
- The cross-disk fall-back in `move_new` (`crate::fs::rename`: copy, then delete), FAT disks
  without hard links, Windows (`trash::os_limited`, the shell) and macOS (Finder). The model is
  the freedesktop.org bin: FreeBSD, the other BSDs, illumos, Linux, Termux.
- The `trash` crate's put differs from `xdg_trash::put_in` in one step: it makes a placeholder
  at `files/<name>` first, so its rename never replaces. The model's put is `put_in`, whose
  rename would; no trace found depends on that.
- `fd::real` (a bin whose `files` or `info` is a link is refused) and `bin::inside` (nothing is
  made in `@bin`): checked at the start of each operation, not raced here.
- Disk full, permissions, a bin on a disk that is unplugged mid-step: a failed system call is
  modelled only where the code reads its result (`hard_link`, `remove_file`, `rename` of a
  missing source).
- More than two programs or files: a trace needing three has not been looked for.
- Liveness (an operation finishes): safety only.

## What it found

**#720, `NoLoss` and `Restorable`: `empty_in`'s sweep deletes what was put in while Empty
ran, and `delete_in` removes by a name that may be reused.** The shortest trace, in the code's
words:

1. The user confirms *Empty the bin*; `empty_in` calls `list_in` and begins deleting the list.
2. Another program (or the desktop app's own panel, F8 while Empty runs on its blocking thread)
   trashes `~/Documents/report.txt`: `create_new(info/report.txt.trashinfo)`, `write_all`,
   `rename` into `files/`.
3. `empty_in`'s sweep over `read_dir(files)` removes `files/report.txt`; its sweep over
   `read_dir(info)` removes the info file.

`report.txt` is gone for good. A put that lands between the two sweeps leaves `files/report.txt`
without its info file: `Restorable` breaks, the panel does not show it, the next Empty deletes it.
The second trace (`NoLoss-FALSE.log`, the run that assumes no put overlaps an Empty): two
programs both `find` `report.txt` for delete for good; the first removes it and its info file; a
new `report.txt` is trashed and gets the free name; the second program's `remove(p)` deletes it.

**#721, `NeverOverwrite`, shown by the `restore_renames` variant:** `xdg_trash::restore_in`
(undo after F8 on Termux and illumos) restores by `symlink_metadata` then `rename`, so a file
made at the place between the two calls is replaced. `fd::restore_in` takes the hard-link way
and passes; the model's `r_link` is that way, which is why the code-as-is run says PASS here.

Do not weaken `Bin.tla` to make these pass. A fix is in the code; the check then says PASS and
this section shrinks.

## Does my change touch the verified code?

The model describes these functions at the sync commit. A change to any of them, or to the
order of system calls in them, needs the model looked at (and the sync commit moved only after).

| Model action(s) | Code (at the sync commit) |
|---|---|
| `PutStart`, `PutCheck`, `PutClaim`, `PutWrite`, `PutMove`, `PutFail` | `xdg_trash::put_in` (`crates/coxswain-core/src/xdg_trash.rs`, lines 47–94): the `name_candidates` loop, `symlink_metadata(files/n)`, `create_new`, `write_all`, `rename`, the `remove_file(&i)` on failure. Also the `trash` crate's `move_to_trash`, same order. |
| `Listed`, `RestoreStart`, `DeleteStart`, `EmptyStart` | `fd::list_in` (`bin.rs` 438–478: an info file whose `parse` succeeds and whose item exists) and `fd::find` (533–540). |
| `RCheck` | `fd::restore_in` (553–581): `symlink_metadata(&to)`, `taken`, `free_in` with `name_candidates`. |
| `RLink`, `RUnlink` | `fd::move_new` (589–632), the file branch: `hard_link(from, to)`, then `remove_file(from)`. |
| `RInfo` | `fd::restore_in`: `remove_file(info_of(&item.path))` after a successful move. |
| `DRemove`, `DInfo` | `fd::delete_in` (642–659): `remove(p)`, then `remove_file(info_of(p))`. |
| `EFile`, `EInfo`, `ESweepFiles`, `ESweepInfo` | `fd::empty_in` (661–685): `delete_in` on `list_in`'s list, then `read_dir(files)` and `remove` each, then `read_dir(info)` and `remove_file` each. |
| `Make` | Not Coxswain: the user or any program saving a file at a free place. |
| `Crash` | Not Coxswain: the process or machine dying between two system calls. |
| The three promises | `docs/files/bin.md`, *Restore, delete for good, empty*: "nothing is overwritten", "Deletes everything in the bin for good" (what was listed and confirmed), the panel lists every item. |

**Sync commit: `2bd2293a6`** (`bin.rs` and `xdg_trash.rs` as on master at that commit). To see
whether the covered code changed since:

```
git diff 2bd2293a6 HEAD -- crates/coxswain-core/src/bin.rs crates/coxswain-core/src/xdg_trash.rs
```

An empty diff: the model still describes the code; run the check and read the table. A diff
that touches a function in the table: compare the function with its model action step by step
(one system call, one step, in the code's order) before trusting a PASS.

## A change to the bin's code: what to do

1. Find the function's row above and read its model action in `Bin.tla`.
2. Change the action so it performs the new system calls in the new order; a new check the code
   reads is a new `IF` on the state; a new call that writes is a new step with its own `pc`
   value. Keep the names of the code.
3. Run `.claude/skills/bin/check.sh`. Read the table against *What to expect*. Read any new
   trace with `/statechart`.
4. A trace the code can really take is a bug in the change: tell it as steps in the code's words
   and fix the code, never the promise. A trace the code cannot take (a step the model allows
   that the code does not, a call the model left out) is a model fault: fix the action to match
   the code, and say so in the PR.
5. When the table is as expected (or better, with the issue closed), move the sync commit above
   to the new commit, update the line numbers in the table, and say in the PR that the bin model
   was rerun.

## Able to fail

Each promise has been seen failing on a broken variant, run with `--variant`:

| Variant | Breaks in the code | Fails |
|---|---|---|
| `restore_renames` | `fd::restore_in` moves with `fs::rename` instead of `move_new` (what `xdg_trash::restore_in` does today, #721) | `NeverOverwrite` in 10 states: the restore finds the place free, the user saves a new `report.txt` there, the rename replaces it. `NoLoss` too. |
| `put_moves_first` | `put_in` renames the file into `files/` before writing its info file | `Restorable` in 5 states: the file is in the bin with an empty info file, which `parse` rejects; a crash there leaves it invisible. |

The known findings (#720) are the shortest `NoLoss` and `Restorable` traces on the code as it is;
a different break of those two promises shows as a different trace, not a different word in the
table. Compare the trace with the ones told above.

## When this skill is stale

A change to the covered code that did not rerun the check, a closed issue the table still
expects, or a sync commit that is far behind: run `/maintain-formal-verification-skill` on this
skill, which brings the model, the table and this file back in line with the code.

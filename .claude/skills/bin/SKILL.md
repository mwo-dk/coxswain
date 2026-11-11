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
of steps of two Coxswains plus a crash anywhere, and with `--outside` of one Coxswain and one
other program that takes no lock of ours.

## Run it

```
.claude/skills/bin/check.sh
```

Needs `java` (any JRE 11 or newer) and the network once: the first run fetches tla2tools 1.8.0
(4.5 MB, SHA-256 pinned in the script) into `.worktrees/tlaplus/` of the main checkout, which
git ignores. A run takes about a minute on a laptop and prints one line per promise:

```
promise          result put during empty allowed / assumed not
NoLoss           PASS
Restorable       PASS
NeverOverwrite   PASS
TakesOnlyItsItem PASS
```

`--outside` runs one Coxswain beside another program (the desktop's own trash: `ODelete`,
`ORestore`, `OInfo`, and its puts), a few minutes. `--variant` picks a broken variant (*Able to
fail*, below). `--files 3` checks with three files instead of two: a restore that links another
program's file and a file the user saves at the place since (#923) need three. `--no-link`
models a disk without hard links (Termux, FAT): `move_new` makes an empty file at the place
and renames the item over it (#931).

A promise that fails prints `FAIL` with the log of its counterexample, and runs again assuming no
put overlaps an Empty: `FAIL   with a put during empty: FAIL (…/NoLoss-TRUE.log); without: …`.

Exit 0: every promise holds. Exit 1: a log named on the line holds the counterexample (the
`State n:` blocks, each headed by the model action that made it). Exit 2: the tool itself failed
(no java, a parse error in `Bin.tla`): read the log it names.

**Read every result with the pstack `/statechart` skill**, giving it `Bin.tla` and a `.cfg` from
the run folder: it tells each violation as its shortest path in plain words, and when a promise
holds it draws how the bin moves between states. Do not read the raw `State n:` dumps to the
user.

**What to expect at the sync commit (below):** exactly the table above: every promise holds;
`--no-link` too.
With `--outside`: `NoLoss` and `Restorable` FAIL, `NeverOverwrite` and `TakesOnlyItsItem` PASS,
and every trace runs
through the instant between a re-check (`DCheck`, `ECheck`, `RId`, `RUnlinkCheck`, `RBackCheck`,
the info checks) and the call it guards: the residue of #726, which no program can close alone (*What it
found*). `--outside --no-link`: the same, and `TakesOnlyItsItem` FAILs too, through the instant
between `RNlCheck` and `RRename`. `--outside --variant atomic`, the same with each re-check and its call one step: every
promise holds (with `--no-link` too); so does `--outside --variant link_open` (`--files 3` too, about ten minutes), where
the link has no re-check (`RId` skipped), so the #901 and #923 steps are reached. Any other `FAIL` is news: a new bug or a model that no longer matches the code.
Decide which with the coverage table. The variants (*Able to fail*, below) must still fail as
told there.

## The promises, in one sentence each

| Promise | Means | A break costs |
|---|---|---|
| `NoLoss` | A file the user put in the bin, and has not deleted for good or emptied, still exists: at its place, beside it as `name (2)`, or in the bin. | The user's file is gone and nothing said so. |
| `Restorable` | Such a file, when it is only in the bin, is listed there (its `.trashinfo` names its place), so the user sees it and can restore it. Holds at every instant, between steps too. | The file sits in `files/` unlisted, invisible in the panel, and the next Empty deletes it. |
| `NeverOverwrite` | A file at its place is never replaced by another; it only leaves its place by being put in the bin. | A restore silently replaces the new `report.txt` the user saved since. |
| `TakesOnlyItsItem` | A restore's rename (`--no-link`) takes out of the bin only the file it looked at, never another program's file put under the item's name since. | Another program's file leaves its bin for the item's place, and its info file is left naming nothing. |

What the user has asked to lose: an item `delete_in` has `find`-listed (the user confirmed it by
name and size), and every item `empty_in`'s `list_in` listed when Empty started (the user
confirmed *N items, size*). Anything else the user still wants.

## Bounds and what is left out

Checked: 2 programs, 2 files that share one name (`report.txt`, so names collide in the bin and
at the origin), 2 bin names (`report.txt`, `report.txt (2)`), 2 places (`~/Documents/report.txt`
and `report.txt (2)` beside it), a crash of either program between any two steps, the user
saving a new file at a free place at any time, restores with and without *Restore under a free
name*, every order of Empty's deletions. 3.4 million distinct states (the lock serialises restore,
delete and Empty; the unlocked variant has 57 million), about a minute; with `--outside` one
Coxswain and one other program, 8.2 million (4.0 million with `--variant atomic`, 5.3 million
with `link_open`, 60 million with `link_open --files 3`), a few
minutes. `--no-link`: 1.8 million; with `--outside`, 2.4 million (1.5 million with `--variant
atomic`). TLC visits all of them.

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
- A put that copies (the `trash` crate's fall-back when the file is on another disk than the
  home bin: copy into `files/`, then delete the original). A half-made copy is listed, so an
  Empty then would delete it, and the put the original. Coxswain's own put (`fs::trash_named`)
  waits for the lock of the home bin and its own disk's bin (`fd::bins_for`; `fd::wait`, Stop
  ends the wait), so no Coxswain Empty runs during it; another
  program's copying put is not covered.
- A busy bin: `fd::hold` takes the lock with `try_lock`, and when another Coxswain holds it the
  operation does nothing and says `bin.busy`. The model's `Free` waits instead; refusing is a
  step not taken, which no promise can break.
- Undo after F8 (`undo::Record::undo` for a delete or a sync: the `trash` crate's
  `restore_all`, or `xdg_trash::restore_in` on Termux and illumos) waits for the lock once for
  the whole undo (Esc or Stop ends the wait, and the undo then does nothing); its steps are not
  modelled.
- An info file that is not UTF-8 or does not parse: `list_in` reads it lossily, so a raw
  `Path=` still lists the item, and joins an item's name as bytes, so a name that is not UTF-8
  is listed too (#900); one that does not parse at all is never a stray, and its item
  stays through Empty. The model's info files are written whole or empty.
- Other programs' own steps (`--outside`): their delete for good, restore and Empty are taken
  to be right for themselves: each removes or moves out the file it listed in one step, then
  that item's info file. A program that removes by a name it looked at earlier can lose files
  on its own, beside anything; that is its bug, not modelled. One other program, beside one
  Coxswain.
- `move_new` without hard links (Termux, FAT), with `--no-link`: a program that replaces or
  removes the empty file at the place before the rename (an editor's save by rename, a put of
  it, which the model takes as failed) is not modelled; the rename would replace that file. The
  hard-link way's `TakesOnlyItsItem` is not this promise: there the link of another's file is
  taken back (#923), and `RUnlinkCheck` reaches it. On a disk whose inode numbers are made up
  per cached file (vfat, some FUSE), a number that changes between restore's look and
  `RNlCheck` makes a false *Not in the bin*: the item stays, nothing is lost, a retry works.
- Info files without their item: Empty leaves them (one may be a put under way). Not modelled
  as a cost: they hold no data, and a put of that name takes the next name.
- Disk full, permissions, a bin on a disk that is unplugged mid-step: a failed system call is
  modelled only where the code reads its result (`hard_link`, `remove_file`, `rename` of a
  missing source).
- More than two programs: a trace needing three has not been looked for. Three files only with
  `--files 3` (#923's traces).
- `move_new`'s check after the link (#901) compares device and inode only, as the link changes
  the change time: another program's file given the freed inode of the item within that
  instant goes unseen.
- `fd::id` is the device, inode and change time: a file freed and another made since may get
  the same inode number, and on a coarse clock (Linux before 6.13: 1–4 ms ticks) the same change
  time only if made within the tick of the old item's last change (its trashing): negligible,
  not zero. Two bin items that are hard links of one file share a change time, so removing one
  makes the other "another file": left and said, not lost. The model's `Make` reuses a file only
  while no program holds it (`Reusable`). A program that changes an item's metadata in the bin
  (a `chmod`) makes it "another file" to the re-check: left, and said.
- The listing and its `fd::ids` are one step in the model (`EmptyStart`, `DeleteStart`,
  `RestoreStart`); in the code `find` and `ids` are a few system calls apart, and Empty takes the
  ids after `list_in` has read the whole bin, so an outside empty and put during the listing is
  taken as the item. The docs say so.
- Liveness (an operation finishes): safety only.

## What it found

The first two findings are fixed; the variants keep them as proof the model sees them. The
third is narrowed to an instant and cannot be closed by Coxswain alone.

**#720, `NoLoss` and `Restorable`** (variants `sweep_all` and `unlocked`): `empty_in`'s sweep
deleted whatever was in `files/` and `info/` at the end, so a file put in while Empty ran went
too (or lost its info file and was hidden); and `delete_in` removed by a name that another
program could free, and a new put take, between its `find` and its `remove`. Now Empty removes
only what it listed and the strays (no info file, or an empty one) it saw at the
listing that still are strays and still the same file (dev, ino), and restore, delete for good,
Empty hold the bin's lock (`fd::hold`, `flock` on
`<bin>/.coxswain-lock`) from their `find` to their last step, so no other Coxswain frees a name
in between. Both undos and Coxswain's own put wait for it (`fd::wait`).

**#721, `NeverOverwrite`** (variant `restore_renames`): `xdg_trash::restore_in` (undo after F8
on Termux and illumos) restored by `symlink_metadata` then `rename`, replacing a file made at the
place in between. It now moves with `fd::move_new`, as `fd::restore_in` does, under the undo's lock.

**#726, `NoLoss` and `Restorable` beside another program** (`--outside`, variant
`no_recheck`): the lock keeps Coxswains apart, but the desktop's own trash takes no lock of
ours, and the freedesktop.org spec has none. Between Coxswain's look at an item and its removal
another program empties the item and a put takes its name: Coxswain removed the new file (or its
info file, hiding it). Also, Coxswain's restore found its bin file gone after linking it home
and removed the info file, which by then a put had made anew. Now restore, delete for good and
Empty take `fd::ids` (file and info file: device, inode, change time) at the look and compare
them just before each removal (`delete_listed`, `restore_in`, `move_new`'s unlink,
`remove_info`): one gone counts as done and its info file is left to the program that took it;
another file under the name is left and said (`err.not_in_trash`). What remains is the instant
between each re-check and its call: TLC finds it (`--outside`), and with each pair made one step
(`--variant atomic`) every promise holds. Closing it needs the other program to take a lock
too; the docs say two programs emptying at once is not guarded.

**#901, a restore taking another program's file** (no promise breaks: the file is not lost,
only moved): `move_new` compared the bin file with the file at the place after the link, both
then the same, so a file another program put under the item's name between restore's re-check
(`RId`) and the link went to the item's place. It now compares the file at the place with the
id found (device and inode); another file stays in the bin.

**#931, the same without hard links** (`TakesOnlyItsItem`, `--no-link`): on Termux and FAT
`move_new` made the empty file at the place and renamed the bin file over it without looking
at it again, so another program's file put under the item's name after restore's re-check
(`RId`) went to the place. It now compares `id(from)` (device, inode) with the id found just
before the rename (`RNlCheck`); another file stays in the bin, and the empty file is taken away
while it is still the one made (`RUnclaimCheck`, `RUnclaim`). Unix only: elsewhere `fd::id` is
only a length. What remains is the instant between `RNlCheck` and `RRename`.

**#923, that other file's link left at the place** (no promise breaks: a second name of a file
still in the bin): `move_new` now takes it away again (`RBackCheck`, `RBack`), but only while the
name at the place and the bin file are still one file (`id(to) == id(from)`: device, inode,
change time), so the removal never takes data off the disk. Taking it away by name, trusting
the `id(to)` read right after the link (variant `back_by_name`), loses a file: another program
puts the linked file in its bin, the user saves a new `report.txt` at the place, and the
removal takes that (`NoLoss` with `--outside --files 3`; it needs a third file). What remains
is the instant between `RBackCheck` and `RBack`, like every re-check of #726. Unix only: on
other systems `fd::id` is only a length, and the link is left there.

Do not weaken `Bin.tla` to make a promise pass. A fix is in the code.

## Does my change touch the verified code?

The model describes these functions at the sync commit. A change to any of them, or to the
order of system calls in them, needs the model looked at (and the sync commit moved only after).

| Model action(s) | Code (at the sync commit) |
|---|---|
| `PutStart`, `PutCheck`, `PutClaim`, `PutWrite`, `PutMove`, `PutFail` | `xdg_trash::put_in` (`crates/coxswain-core/src/xdg_trash.rs`, lines 47–91): the `name_candidates` loop, `symlink_metadata(files/n)`, `create_new`, `write_all`, `rename`, the `remove_file(&i)` on failure. Also the `trash` crate's `move_to_trash`, same order. |
| `Free`, `Take`, `Done`, `lock` | `fd::hold` (`bin.rs` 370–414, `lock_all`): `flock` on `<bin>/.coxswain-lock` (`try_lock`; when busy, nothing is done) at the start of `restore_in`, `delete_in`, `empty_in`; `fd::wait` (waits, until Stop) for `undo::Record::undo` and `fs::trash_named`. Let go when they return (`fd::Held`: an unlock before the close, as a program another thread starts holds a copy of the handle until it runs, #797) or the program dies. |
| `Listed`, `RestoreStart`, `DeleteStart`, `EmptyStart`, `iid`, `SameFile`, `SameInfo` | `fd::list_in` (`bin.rs` 591–607, `item` 610–635: an info file whose `parse` succeeds and whose item exists, names as bytes), `fd::find` (690–697) and `fd::ids` (703–705: `fd::id`, 434–446, of the file and its info file). |
| `RCheck`, `RId` | `fd::restore_in` (726–773): `symlink_metadata(&to)`, `taken`, `free_in` (numbered as `name_candidates` does, as bytes); then `id(&item.path)` against the one found. |
| `RLink`, `RUnlinkCheck`, `RBackCheck`, `RBack`, `RUnlink` | `fd::move_new` (787–916), the file branch: `hard_link(from, to)`, `id(to)` against the id found (device, inode; else `err.not_in_trash`, and on Unix `remove_file(to)` when `id(to) == id(from)`, else both links left), `id(from) == id(to)`, then `remove_file(from)`; `false` when either finds the file gone or another's; also `xdg_trash::restore_in` (101–136), which takes the ids before reading the info file and re-checks the file before `move_new`. |
| `RLink` (`NoLink`), `RNlCheck`, `RRename`, `RUnclaimCheck`, `RUnclaim` | `fd::move_new`, the branch where `hard_link` fails with `PermissionDenied` or `Unsupported`: `create_new(to)` and its `id` from the handle, on Unix `id(from)` against the id found (device, inode), `rename(from, to)`; on either failing, `remove_file(to)` while `id(to)` is still the empty file's. |
| `RInfoCheck`, `RInfo` | `fd::restore_in`: when `move_new` says `true`, `remove_info` (709–714: `id(info)` against the one found, then `remove_file`). |
| `DCheck`, `DRemove`, `DInfoCheck`, `DInfo` | `fd::delete_in` (926–945: `find` and `ids` per path) and `fd::delete_listed` (950–976): `id(p)` against the one listed, `remove(p)`, then `remove_info`. |
| `EFile`, `ECheck`, `ERemove`, `EInfoCheck`, `EInfo`, `ESweepCheck`, `ESweepRm` | `fd::empty_in` (983–1022): `list_in` with `ids` of each item, and the strays of `read_dir(files)` (`unlisted`: info file missing or empty) with their `id`, `delete_listed` on the list, then for each stray `unlisted` and the same `id` again before `remove`. `ESweepFiles`, `ESweepInfo` are the `sweep_all` variant only. |
| `ODelete`, `ORestore`, `OInfo` | Not Coxswain: another program's delete for good, restore and Empty (`--outside`), taking no lock. |
| `Make` | Not Coxswain: the user or any program saving a file at a free place. |
| `Crash` | Not Coxswain: the process or machine dying between two system calls. |
| The four promises | `docs/files/bin.md`, *Restore, delete for good, empty*: "nothing is overwritten", "Deletes everything in the bin for good" (what was listed and confirmed), the panel lists every item; *What if the desktop's own trash works on the bin at the same time?*: another file put under an item's name "stays in the bin". |

**Sync commit: the merge of pull request #934** (the fix of #931, the no-link re-check, after #923
in #930, #900 and #901 in #912 and the re-checks of #789; `bin.rs` and `xdg_trash.rs` as on master at that commit). To see whether the covered code changed since:

```
sync=$(git log -1 --format=%h --grep='(#934)$' origin/master)
git diff "$sync" HEAD -- crates/coxswain-core/src/bin.rs crates/coxswain-core/src/xdg_trash.rs
```

When you move it, write the new commit's hash here.

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
| `restore_renames` | `fd::restore_in` moves with `fs::rename` instead of `move_new` (what `xdg_trash::restore_in` did before #721) | `NeverOverwrite` in 10 states: the restore finds the place free, the user saves a new `report.txt` there, the rename replaces it. `NoLoss` and `Restorable` too. |
| `sweep_all` | `empty_in` sweeps every entry of `files/` and `info/` at the end (before #720) | `NoLoss` and `Restorable` with a put during Empty: the put lands after the listing, the sweep deletes it. Both pass assuming no put overlaps an Empty. |
| `unlocked` | restore, delete and Empty take no lock (before #720) | `NoLoss` and `Restorable` even with no put during Empty: two programs `find` `report.txt` for delete for good, the first removes it, a new `report.txt` is put under the freed name, the second removes that. |
| `put_moves_first` | `put_in` renames the file into `files/` before writing its info file | `Restorable` in 5 states: the file is in the bin with an empty info file, which `parse` rejects; a crash there leaves it invisible. `NoLoss` too: the next Empty sweeps it as a stray. |
| `no_recheck`, with `--outside` | restore, delete for good and Empty remove by name, without `fd::ids` (before #726) | `NoLoss` and `Restorable` in 17 states: Coxswain finds `report.txt` to delete it for good, the other program empties it, a put of another file takes the name, Coxswain removes that. Passes without `--outside`: the lock holds Coxswains apart. |
| `back_by_name`, with `--outside --files 3` | `move_new` takes the link of another program's file away by name, trusting the `id(to)` read after the link (the first try at #923) | `NoLoss` in 19 states: the restore links the item at the place, the other program puts it back in its bin, the user saves a new `report.txt` there, the removal takes it. `--variant link_open` (the code, every re-check one step, the link without its re-check) holds every promise. |
| `rename_open`, with `--outside --no-link` | `move_new` without hard links renames the bin file over the empty file without comparing `id(from)` with the id found (before #931) | `TakesOnlyItsItem` in 18 states: the restore finds the item, the other program deletes it for good, the restore makes the empty file at the place, the other program puts another file under the item's name, the rename moves that file to the place. The other promises hold; `--outside --no-link --variant atomic` holds all four. |
| `--outside` (the code) | none: the instant between a re-check and its call | `NoLoss` and `Restorable` the same way, with the other program's empty and put inside that instant (between `DCheck` and `DRemove`). `--variant atomic` (each pair one step): every promise holds, so nothing else is left. |


## When this skill is stale

A change to the covered code that did not rerun the check, a closed issue the table still
expects, or a sync commit that is far behind: run `/maintain-formal-verification-skill` on this
skill, which brings the model, the table and this file back in line with the code.

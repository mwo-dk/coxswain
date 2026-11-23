[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# The bin

The system's bin (the trash, the Recycle Bin) opens as a panel in both apps. Each file is listed
where it came from: its name, the folder it was deleted from, and when it was deleted. From
there you put files back where they were, delete them for good, or empty the whole bin. The
preview, **F3**, **F5** (copy out) and Find work on the files in it as on any other.

- [How to reach it](#how-to-reach-it)
- [What you see](#what-you-see)
- [Restore, delete for good, empty](#restore-delete-for-good-empty)
- [Which bin, on each system](#which-bin-on-each-system)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

![The desktop app: the sidebar's Bin row, 5 items · 106 KB, highlighted; the left pane is the bin, each name followed by the folder it came from (budget 2025.ods /home/demo/Documents) and a Deleted column (2d, 23h); above it the badge 5 items · 106 KB and three icon buttons: restore, delete for good, empty](../screenshots/gui-bin.png)
*The bin in the desktop app, opened from the sidebar. The status line's hint names the action menu.*

## How to reach it

| Where | Desktop app | Terminal app |
|---|---|---|
| The sidebar | The *Bin* row, under *Clouds* (and FreeBSD's *Jails*), above the favourites, with what is in it (*3 items · 1.2 MB*). A click opens it in the active tab; a right-click or its **⋯** offers *The bin* and, when it holds something, *Empty the bin* | No sidebar |
| The go-to list | – | **Alt+F1** (left panel) or **Alt+F2** (right panel): *Bin: 3 items · 1.2 MB*, under *Type a path…*, the drives and the clouds |
| The command list | **F9** → *The bin* | **F9** → *The bin* |
| A path | `~/@bin` in the path bar (**Ctrl+L**) | `cd ~/@bin` does not work (it is no folder); use the go-to list |

The bin's own actions have no default key: they are in the action menu (**Shift+F10**, or a
right-click on a row when Settings → Behaviour → Right-click opens the menu), in **F9**, and,
in the desktop app, as buttons above the panel. In the bin, **F8** and **Shift+F8** (Delete,
Delete permanently) delete for good; the terminal app's key line calls **F8** *Erase* there.
Give them keys under `[keys]` as `bin`, `bin_restore`, `bin_delete` and `bin_empty`
([Changing keys](../customise/keys.md)).

![The terminal app in Classic blue: the left panel is the bin, titled Bin [5 items · 106 KB · Shift+F10: restore, delete for good]; each name is followed by ← and the folder it came from, the time column is when it was deleted, and F8 on the key line reads Erase](../screenshots/tui-bin.png)
*The bin in the terminal app, opened with Alt+F1 → Bin.*

## What you see

| Part | Desktop app | Terminal app |
|---|---|---|
| The title | The tab is called *Bin*; the badge above the panel says what it holds (*3 items · 1.2 MB*), then three icon buttons, *Restore from the bin* (not on a Mac), *Delete from the bin for good* and *Empty the bin*: hover one for its name | The panel's title: *Bin [3 items · 1.2 MB · Shift+F10: restore, delete for good]* |
| Each row | The name it had, then the folder it came from, dimmed; hover it for *Deleted from …* | The name, then `← /the/folder/it/came/from` |
| The date | The date column is called *Deleted* and shows when it was deleted; sort by it with **Ctrl+F5** | The time column shows when it was deleted |
| `..` | Back to your home folder | The same |

A folder in the bin opens with **Enter** like any folder; on the BSDs, illumos and Linux its `..` leads back to the bin (on Windows and macOS to the bin's own folder on disk). The
newest deletion is at the top. Hidden files (a name starting with `.`) follow **Alt+.** as
anywhere. A name that is not UTF-8 (a file with a Latin-1 name, put in the bin by another
program) is listed too, shown with `�` for the bytes it cannot show, and is restored (under a
free name too, keeping its bytes), deleted for good and emptied like any other.

The sidebar's row and an open bin follow what goes in and out: on Linux, the BSDs and illumos
the bin's folders are watched; on Windows and macOS the row is read again every 30 seconds
while the window is in view, and whenever it comes back to the front.

**Find** (**Ctrl+F** in the desktop app, **Alt+F7** in the terminal app) limited to the folder
(*In Bin*) looks for names in the bin; each hit says the folder it came from. Find everywhere
does not look in the bin: its files are not indexed.

## Restore, delete for good, empty

All three ask first, naming how many items and their size, and say that it cannot be undone.
**Ctrl+Z** does not take them back.

| Action | Does |
|---|---|
| *Restore from the bin* | Puts the marked items (or the one under the cursor) back where they were; on the BSDs, illumos and Linux the folder is made again if it is gone. When another file has the place now, nothing is overwritten: Coxswain asks *Restore under a free name*, which puts it beside that file as `name (2)`; Esc leaves it in the bin. A file that came into your home's bin from another disk goes back across, copied and then removed from the bin, as **F6** moves between disks |
| *Delete from the bin for good* | Deletes the marked items, folders with all in them. Also **F8** and **Shift+F8** in the bin |
| *Empty the bin* | Deletes everything in the bin for good: what it held when it started. On the BSDs, illumos and Linux a file put in the bin while it empties (from the other app, a browser, the desktop) stays there. Also from the sidebar row's menu, from anywhere |

The status line then says *Restored 3 items*, *Deleted 2 items for good* or *The bin is empty*.
What could not be done (a file in use, no permission) is listed in a dialog with its name.

In the bin nothing is moved, renamed or made: **F6**, **Shift+F6** and **F7** say *Nothing is
moved, renamed or made in the bin: restore it, or copy it out*. **F5** copies files out of it. Nothing goes in: **F5**, **F6**, *Extract*, *Pack* or a drop onto the bin's panel say the same, and nothing is made. To put files in the bin, delete them with **F8**.

## Which bin, on each system

| System | Which bin | What works |
|---|---|---|
| FreeBSD, the other BSDs, illumos, Linux | The freedesktop.org bin: the one in your home (`~/.local/share/Trash`, or `$XDG_DATA_HOME/Trash`) and each disk's own (`.Trash/<uid>` or `.Trash-<uid>` at the disk's top) | Everything. In Termux only the home bin; on illumos the home bin and those at mounted disks' tops |
| Linux, the Flatpak | The app's own bin only, not the system's | Everything, on what is in that bin; the notice *In the Flatpak, Coxswain sees only its own bin…* says so once. Files you delete in the Flatpak go to the system's bin, which the Flatpak cannot list |
| macOS | `~/.Trash` | Listing it needs Full Disk Access (System Settings → Privacy & Security → Full Disk Access → Coxswain). Without it the row says *in Finder* and opens the bin in Finder. There is no restore: macOS gives apps no way to put a file back (use *Put Back* in Finder). Delete for good works; *Empty the bin* asks Finder, so macOS asks once whether Coxswain may control Finder; Finder empties the bin of every disk, not only `~/.Trash`, and the question says so. A notice says this once |
| Windows | The Recycle Bin of every drive | Everything |

Only real folders count as a bin: a disk's `.Trash-1000` whose `files` or `info` is a link (to your home, say) is left out, so emptying can never reach through it.

On FreeBSD, the other BSDs, illumos and Linux a disk's bin is listed, restored from and emptied
only when no other user of the machine can have made it or can change it: the same checks
**F8** makes before it puts files there ([Delete](delete.md#why-did-f8-on-a-shared-disk-put-my-files-in-the-home-bin)).
A bin that fails them is left out, and when it holds items, a notice names it once
([Why is a disk's bin left out?](#why-is-a-disks-bin-left-out)). An item whose recorded place
leads out of its disk (through `..`, a whole path elsewhere, or a folder on the way that is a
link or another disk) is listed and goes with *Empty the bin*, but is never restored there. A restore
goes from the disk's top folder by folder, holding each one open and making a missing one
inside the one it holds, so a folder another user swaps for a link meanwhile cannot lead it,
or the folders it makes, anywhere else. A folder on the way must be yours or the system's,
and others may write it only when it has the sticky bit: a restore through a folder of someone
else's, or one your group may write (`775`), is refused the same way. Folders it makes are
writable by you alone.

## Settings and config.toml

No Settings item. The keys are `bin`, `bin_restore`, `bin_delete` and `bin_empty` under
`[keys]`, none by default. Whether **Delete** asks first (`confirm_delete`) does not apply here:
the bin always asks.

## Questions

#### How do I get a deleted file back?

Open the bin (desktop app: the *Bin* row in the sidebar; terminal app: **Alt+F1**, then *Bin*),
put the cursor on the file or mark several with **Insert**, press **Shift+F10** and pick
*Restore from the bin* (desktop app: the button above the panel too). Enter confirms. The file
goes back to the folder it came from; the status line says *Restored 1 item*.

#### What if a file of that name is there again?

Nothing is overwritten. After the others are restored, a second question says *Another file has
the place of report.pdf now. Restore it beside that one, under a free name such as “name (2)”?*
Enter restores it as `report (2).pdf` next to the other; Esc leaves it in the bin.

#### Why can I not restore on a Mac?

macOS keeps where a file came from in a form apps cannot use, and gives them no way to put a
file back. With Full Disk Access Coxswain lists the bin and deletes for good; to restore, use
*Put Back* in Finder, or copy the file out with **F5**. The original folder is not shown on a
Mac for the same reason.

#### The Bin row says "in Finder" on my Mac.

Coxswain may not read `~/.Trash` without Full Disk Access. Give it in System Settings → Privacy
& Security → Full Disk Access, then restart Coxswain; until then a click opens the bin in Finder.

#### Does undo bring back what I deleted for good from the bin?

No. *Delete from the bin for good* and *Empty the bin* remove the files as `rm` would; the
question says so first. Only backups or recovery tools can help then.

#### Why does the bin not show what I deleted in the Flatpak?

In the Flatpak, Delete sends files to the system's bin through the desktop, which the Flatpak
cannot list; Coxswain shows the Flatpak's own bin instead, which is usually empty. Open the
system's bin in your file manager, or install Coxswain another way ([Install](../../README.md#install)).

#### A USB stick's deleted files are not listed.

Each disk keeps its own bin at its top (`.Trash-1000` on a stick). It is listed while the disk
is mounted. On FreeBSD, the other BSDs and Linux, files deleted on a disk without a bin of
yours went to the bin in your home folder, so look there; elsewhere they were deleted for good
(see [Delete](delete.md#why-did-f8-on-a-shared-disk-put-my-files-in-the-home-bin)).

#### Why is a disk's bin left out?

Another user of the machine could have made a `.Trash-1000` on a shared disk, filled it with
items of their choosing, and named the places they would be restored to. So a disk's bin counts
only when the disk's top folder is yours or the system's (others may write in it only when it has
the sticky bit, as `/tmp` has), a shared `.Trash` there has the sticky bit and belongs to the
system or the top's owner, and your `.Trash-1000` (or `.Trash/1000`) and its `files` and `info`
are real folders of yours that no one else may write. When one is left out and holds items, the
notice *The bin … is not yours alone* names it once (terminal app: the status line; desktop
app: *Settings → What's new*). Check who owns it with `ls -ld`; when it is yours and only its
mode is wrong, `chmod 700` it and its `files` and `info`, and the notice goes at the next look.
A disk mounted so that everyone may write its top (NTFS or FAT mounted without `dmask=022` or
`umask=022`), or one that shows another owner (sshfs without `idmap=user`), has no bin Coxswain
uses: mount it with those options, and its bin is listed again.

#### Why does a restore say the place lies outside its disk?

A disk's bin records each item's place relative to the disk's top. One that leads out of the
disk, through `..`, as a whole path elsewhere, or through a folder on the way that is now a link
or another disk, was not written by a put on that disk, or no longer leads where it did. A folder
on the way that is someone else's, or that others may write without the sticky bit (a `775`
folder your group may write), is refused too: they could swap it for a link while the file
moves. `chmod g-w` on that folder lets the restore through. In each case Coxswain
does not put it back there, and the dialog of what could not be done says *The place it was
deleted from lies outside its disk: not put back*. Copy it out with **F5**, or delete it.

#### The bin opens a folder called @bin instead.

`~/@bin` is the bin only while no real folder of that name is in your home. When another
program made one, the *Bin* row, **F9** → *The bin* and `~/@bin` open that folder, and a notice
says once *A real folder named @bin in your home (…) hides the bin*. Rename or move that folder
and the bin is back. Coxswain itself never makes it: nothing is copied, moved or made into the
bin, also when your home is reached through a link to it (`/home` a link to `/usr/home`, say).
#### What if the desktop's own trash works on the bin at the same time?

Two Coxswains (the terminal app and the desktop app) take turns: while one restores, deletes
for good or empties, the other says *Another Coxswain is working in the bin* and does nothing. Other programs (the
desktop's file manager, `gio trash`, `trash-cli`) take no turn of ours, as the freedesktop.org
bin has no lock. So on the BSDs, illumos, Linux and Termux Coxswain looks at each item again
just before each removal: the file and its info file must still be the ones listed (the same inode
and change time). One that another program took away meanwhile counts as done; another file put
under its name meanwhile stays in the bin, listed, and the dialog names it *Not in the bin*.
A restore checks once more after it has linked the file back at its place: when that turns out to
be such another file, it stays in the bin, named *Not in the bin* the same way, and its second name
at the place is taken away again, but only while that name is still the same file as the one in the
bin: a file saved at the place since stays. On a disk without hard links (Termux, FAT) the restore
makes an empty file at the place and moves the item over it; it looks at the item just before that
move instead, and when it is such another file, it stays in the bin and the empty file is taken
away again.
What is left is the instant between that last look and the removal itself, a few microseconds,
and, for *Empty the bin*, the time it takes to list the bin before it looks at the first item
(longer in a bin of thousands); no program can close these alone: emptying or deleting from the
bin in two programs at once is not guarded.

#### Can Find search inside the files in the bin?

Only by name, with Find limited to the bin (*In Bin*). Their text is not read, and Find
everywhere leaves the bin out.

---
[← Previous: Undo (Ctrl+Z)](undo.md) · [Next: Clipboard →](clipboard.md)

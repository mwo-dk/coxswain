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
| The go-to list | – | **Alt+F1** (left panel) or **Alt+F2** (right panel): *Bin: 3 items · 1.2 MB*, under *Type a path…* and the clouds |
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
anywhere.

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
| *Restore from the bin* | Puts the marked items (or the one under the cursor) back where they were; on the BSDs, illumos and Linux the folder is made again if it is gone. When another file has the place now, nothing is overwritten: Coxswain asks *Restore under a free name*, which puts it beside that file as `name (2)`; Esc leaves it in the bin |
| *Delete from the bin for good* | Deletes the marked items, folders with all in them. Also **F8** and **Shift+F8** in the bin |
| *Empty the bin* | Deletes everything in the bin for good. Also from the sidebar row's menu, from anywhere |

The status line then says *Restored 3 items*, *Deleted 2 items for good* or *The bin is empty*.
What could not be done (a file in use, no permission) is listed in a dialog with its name.

In the bin nothing is moved, renamed or made: **F6**, **Shift+F6** and **F7** say *Nothing is
moved, renamed or made in the bin: restore it, or copy it out*. **F5** copies files out of it.

## Which bin, on each system

| System | Which bin | What works |
|---|---|---|
| FreeBSD, the other BSDs, illumos, Linux | The freedesktop.org bin: the one in your home (`~/.local/share/Trash`, or `$XDG_DATA_HOME/Trash`) and each disk's own (`.Trash/<uid>` or `.Trash-<uid>` at the disk's top) | Everything. In Termux only the home bin; on illumos the home bin and those at mounted disks' tops |
| Linux, the Flatpak | The app's own bin only, not the system's | Everything, on what is in that bin; the notice *In the Flatpak, Coxswain sees only its own bin…* says so once. Files you delete in the Flatpak go to the system's bin, which the Flatpak cannot list |
| macOS | `~/.Trash` | Listing it needs Full Disk Access (System Settings → Privacy & Security → Full Disk Access → Coxswain). Without it the row says *in Finder* and opens the bin in Finder. There is no restore: macOS gives apps no way to put a file back (use *Put Back* in Finder). Delete for good works; *Empty the bin* asks Finder, so macOS asks once whether Coxswain may control Finder. A notice says this once |
| Windows | The Recycle Bin of every drive | Everything |

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
is mounted; files deleted on a disk without a bin were deleted for good (see
[Delete](delete.md)).

#### Can Find search inside the files in the bin?

Only by name, with Find limited to the bin (*In Bin*). Their text is not read, and Find
everywhere leaves the bin out.

---
[← Previous: Undo (Ctrl+Z)](undo.md) · [Next: Clipboard →](clipboard.md)

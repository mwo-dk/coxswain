[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Compare folders (Ctrl+F2)

**Ctrl+F2** compares the left panel's folder with the right panel's, in both apps: every file and
folder under them, matched by its path. Both panels then mark each entry: only on this side,
newer here, older here, different or the same. Mark the differences and copy them with **F5**.
Making one side like the other (copying, writing over and deleting after a plan) comes in a
later version.

![The desktop app comparing two folders: in the left pane a + beside a file only there and a > beside a newer one, in the right pane a < beside the older copy, and the summary on the status line](../screenshots/compare-folders-gui.png)
*Two copies of a project compared: **+** only here, **>** newer here, **<** older here, **≠**
different, **=** the same. The status line sums it up.*

## Contents

- [How to use it](#how-to-use-it)
- [What the marks mean](#what-the-marks-mean)
- [The menu](#the-menu)
- [Copying the differences](#copying-the-differences)
- [Links](#links)
- [Archives, container images, snapshots and histories](#archives-container-images-snapshots-and-histories)
- [Big trees](#big-trees)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Open one folder in the left panel and the other in the right panel.
2. Press **Ctrl+F2**, or pick *Compare folders* in the F9 command list or the action menu
   (**Shift+F10**).

The compare runs on a thread of its own: the status line counts what it has looked at
(*Comparing the folders: 1,204 looked at…*) and the marks appear in both panels as they are
found. When it is done, the status line sums it up: *3 only on the left (12 KB) · 1 newer on the
right (4 KB) · 40 the same (2 MB)*, or *The folders are the same*.

The marks follow you into subfolders: open one in either panel and its entries are marked too.
**Ctrl+F2** again opens [the menu](#the-menu).

## What the marks mean

The mark stands where the git mark does, before the name (the desktop app's details view):

| Mark | Colour | Means |
|---|---|---|
| **+** | git *added* | Only on this side |
| **>** | git *modified* | Newer on this side |
| **<** | git *renamed* | Older on this side: the other side's is newer |
| **≠** | git *conflict* | Different, and neither is newer (the same time, or a file against a folder); a folder with differences inside |
| **=** | git *ignored* | The same |

What is only on the other side has no entry here, so it shows only there, as **+**. Hover over a
mark in the desktop app for its words.

Two files match by their **size and time of change**: the same size and at most two seconds
apart (FAT keeps times to two seconds) is the same. *Compare again, byte by byte* in the menu
reads both files instead, a megabyte at a time, and stops at the first byte that differs.

## The menu

**Ctrl+F2** while a compare is on opens its menu; each entry has its number as its key:

| Key | Entry | Does |
|---|---|---|
| **1** | *Mark the differences* | Marks, in both panels, what is only there, newer there or different: ready for **F5** |
| **2** | *Show the differences only* / *Show everything again* | Leaves what is the same out of both panels |
| **3** | *Compare again, byte by byte* / *by size and date* | Starts again the other way |
| **4** | *Stop comparing* | The marks go, and everything shows again |

## Copying the differences

*Mark the differences* (**1**) marks them in both panels; the status line says *Marked 4: F5
copies them to the other side*. **F5** then copies the marked entries of the active panel, as
any copy does ([Copy](copy.md)): it never writes over a file that is there, so a newer file goes
across only where the other side has none. **Ctrl+Z** undoes the copy.

## Links

A link is compared as a link, by where it points, and never walked into: two links to the same
place are **=**, otherwise **≠**. A link to a folder elsewhere is not looked inside, so nothing
outside the two folders is read.

## Archives, container images, snapshots and histories

Each folder is listed as the panels list it, so either side can be an archive opened as a folder
([Archives as folders](archives.md)), a [container image](container-images.md), a
[ZFS snapshot](zfs-snapshots.md) or a commit of a [git history](../panels/git.md). These are read
only: they are compared and copied from. Inside an archive the bytes are not
at hand, so files there match by size and time even when *byte by byte* is chosen.

## Big trees

The two trees are walked side by side, a folder at a time, their names in order: nothing is read
ahead and nothing is kept but what was found, which the panels show as it comes in. **Stop
comparing** (**4**) ends the walk at once. Byte by byte reads two megabytes at a time at most,
however big the files. The walk goes 1,024 folders deep at most: a folder deeper down is
marked **≠** without being looked into, and the summary ends with *folders deeper than 1024
levels not compared (marked different)*.

## In the terminal app

![The terminal app in Classic blue comparing Backups/rocket on the left with projects/rocket on the right: a red ≠ beside src in both panels, > beside Cargo.toml on the left and < on the right, + beside notes.txt on the left and beside docs, target and README.md on the right, and the summary on the status line](../screenshots/compare-folders-tui.png)
*The same two folders in the terminal app: the marks in the column before the names, the summary
on the line under the panels.*

The same marks in the column before the name, in the theme's git colours, and the same menu
(**Ctrl+F2**, then **1** to **4**).

## Questions

#### Why is a file marked different when its contents are the same?

By size and date, two files of the same size whose times of change are more than two seconds
apart count as newer and older. Pick *Compare again, byte by byte* (**3**) to look at their
bytes.

#### Why does F5 not copy a newer file over the older one?

Copy never writes over a file that is there. Delete the older one first (**F8**, to the trash),
then copy. Making one side like the other in one step comes in a later version.

#### Does it delete anything?

No. Compare folders only reads and marks; **F5** copies what you marked, as any copy does.

#### Does it follow links to other folders?

No. A link is compared by where it points (**=** when both point to the same place, **≠**
otherwise) and is never walked into, so nothing outside the two folders is read.

#### Can I compare against a zip file or a snapshot?

Yes: open it like a folder in one panel and press **Ctrl+F2**. It is read-only, so it can be
compared and copied from.

#### Why does the status line say "Compare folders needs both panes"?

The desktop app shows one pane: turn the second one on (**Ctrl+O**).

---
[← Previous: Compare files (Shift+F2)](compare-files.md) · [Next: ZFS snapshots as folders →](zfs-snapshots.md)

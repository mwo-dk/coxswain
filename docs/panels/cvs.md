[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# CVS working copies

In a [CVS](https://www.nongnu.org/cvs/) working copy, Coxswain shows its status the way it shows
git's: the line at the bottom of the pane names `cvs` and the branch or tag, and each changed
file carries its glyph. It reads only the `CVS/Entries` files in each folder, so nothing is sent
to the server and `cvs` need not be installed for that. The history comes from the server, and
only once you say yes to that server: **Ctrl+G** asks first, then lists the commits from
`cvs log`, and **Enter** on one shows its files as they were, with their changes, read-only.
**Alt+D** updates the working copy (`cvs update`) when no file has changes here
([Fetch and pull](fetch-and-pull.md)). Commit stays with CVS: **Alt+G** opens a terminal in the folder for it.
Both apps do this.

![The terminal app in Classic blue: the CVS working copy kite on the left with src marked modified and notes.txt unknown, the line cvs HEAD with its counts in the panel's bottom border; on the right the Subversion working copy balloon with svn r2 trunk](../screenshots/cvs-line.png)

## Contents

- [What you see](#what-you-see)
- [How a change is found](#how-a-change-is-found)
- [History from the server](#history-from-the-server)
- [Inside a commit](#inside-a-commit)
- [Branches, update and commit](#branches-update-and-commit)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## What you see

| Where | What |
|---|---|
| The line (desktop app: the pane footer; terminal app: in the panel's bottom border) | `cvs:` in ASCII glyphs (or the branch glyph and `cvs` with Nerd Font glyphs), the branch or tag from the top folder's `CVS/Tag` (`HEAD` when there is none), then the counts as on the [git line](git.md#the-git-line): `+` added, `~` modified, `-` removed or missing, `?` unknown, `!` in conflict, `=` when clean |
| Each entry | The glyph of its change; a folder takes the loudest of its contents |
| Ignored files | Dimmed: CVS's own list (`*.o`, `*~`, `core`, `*.bak` and the like), then `~/.cvsignore` and each folder's `.cvsignore`; a `!` in one clears the list |

## How a change is found

As CVS does, from `CVS/Entries` and `CVS/Entries.Log`:

- **Added** (`cvs add`, revision `0`) and **removed** (`cvs remove`, a revision with `-`) from the
  records.
- **Modified:** a file whose time differs from the one recorded, or one a merge wrote
  (*Result of merge*).
- **In conflict:** a file a merge left with conflicts (*Result of merge+…*).
- **Missing:** in the records and not on the disk.
- **Unknown:** on the disk and in no record.

A file only touched counts as modified, as it does for CVS itself, until `cvs update` sets its
time again.

## History from the server

CVS keeps the history on its server, and Coxswain contacts a server only when you say so.
**Ctrl+G** (*Git history*, also in the F9 command list and the Shift+F10 action menu) in a working
copy first asks, once per server, as for [Subversion](subversion.md#history-from-the-server):
*Read the history from the server?* naming the server (`pserver://cvs.example.org`,
`ext://host`, or the folder of a repository on this machine). The desktop app shows a dialog
(**Read the history** or **Cancel**), the terminal app a box (**Enter** or **Y** reads, **Esc** or
**N** cancels).

![The terminal app in Classic blue in ~/projects/kite, a CVS working copy, with the box Read the history from the server?: CVS keeps this history on its server, /home/demo/repos/cvsroot, … Settings → Privacy and updates forgets it, and the key line Enter Read the history · Esc Cancel](../screenshots/tui-cvs-ask.png)

With yes, Coxswain runs `cvs -f -d <root> log -N` in the folder (`-f`: your `~/.cvsrc` is not
read; `-d`: the root of the folder asked about, for every folder below too, whatever their own
`CVS/Root` says) and shows the result as a history:

- CVS keeps a history per file. Revisions with the same commit id (CVS 1.12), or else the same
  author and message within five minutes, are one entry: its date in *Modified*, its author in
  *Last commit* (desktop app) or on the info line (terminal app), and the first line of its
  message as its name. At most 2000 are listed. The list has no *All branches* and *Against
  upstream* buttons: those are git's, and **Alt+A** and **Alt+Y** say so on the status line.
- **Which roots:** `:pserver:` and `:ext:` (through `CVS_RSH`, ssh by default) to a host name,
  and a repository on this machine (`/path`, `:local:`, `:fork:`). Any other method, options
  after the method (`:ext;CVS_RSH=…:`) or a user or host starting with `-` is never handed to
  `cvs`. A password in `CVS/Root` is never shown; the question names the method and the host.
- **Remembered** in the state file per server; **Settings → Privacy and updates → History from
  servers** lists them, and **Forget** (desktop app) or **Delete** (terminal app) forgets one.
- **Slow or gone:** a server that does not answer within 30 seconds is left, and the status line
  says so. Without `cvs` installed, a notice says to install it.
- **A file's history** is the commits of its folder that touched it, so a commit has the same
  name in both.

![The terminal app's kite/@history after the yes: 1d6dd92 Spar a little longer, 672e4cb Longer spar, red sail, 2287482 Initial revision and 055056e Start the kite, newest first](../screenshots/tui-cvs-history.png)

## Inside a commit

**Enter** on a commit in the list opens the folder as it was then, read-only, as a [git
commit](git-history.md#what-works-in-a-history) does; **Backspace** leads back to the list. The
folder comes from the log already read: each file at its revision in that commit, or else at
its last revision before it on the main line (the trunk, or the vendor branch an import leaves);
files removed by then are left out. Only the file contents are asked of the server:

| What | Desktop app | Terminal app | What runs |
|---|---|---|---|
| The folder at that commit | The pane lists it with the commit's date; sizes show as 0, as CVS lists none without reading every file | The same | Nothing: the log read for the list |
| A file as it was | **Space** or **F3**: the preview pane | **F3**: the built-in viewer, on a copy | `cvs update -p -r <revision>` |
| What the commit changed | **Diff** at the top of the preview pane: each file's change against its revision before | – | `cvs diff -N -u -r <before> -r <revision>` |
| A copy out | **F5** on a file, a folder or a commit in the list, to the other pane | The same | `cvs update -p`, one file at a time |

![The terminal app in Classic blue inside a commit of kite: the panel titled /home/demo/projects/kite/@history/38962ab664a4 [commit 38962ab] lists frame.txt, line.txt and README.txt as they were at Longer spar, red sail, each dated with the commit and sized 0](../screenshots/tui-cvs-commit.png)

- **Nothing a working copy names runs:** only `log`, `update -p` and `diff` are run, always with
  `-f` and `-d <root>`. `update -p` writes nothing to the working copy and runs no
  `CVS/Update.prog`; none of them reads `CVSROOT/modules`, whose `-o` and `-e` programs run on
  checkout and export.
- **Copied out** files get the commit's date. The executable bit is not carried over. A `.git`,
  `.hg` or `.jj` folder is never copied out. Each file is written as it comes. A file over 1 GB,
  or one the server stops sending part way, is refused, and nothing of that copy is left.
  Before a drag the files are counted ([Drag and drop](../files/drag-and-drop.md)); CVS lists no
  sizes, so the bytes are counted as they are written: a drag that passes 200 MB stops, what it
  wrote is removed, and the status line says it is too much to copy out for a drag.
- **Branch commits:** a commit on a branch shows its own files at the branch's revisions and
  the rest of the folder as on the main line then.
- **Large commits:** a diff covers at most 200 files, 2 MB and about 30 seconds; when the commit
  changed more, its first line says *Cut short: this commit changed more than Coxswain shows in
  one diff.*

## Branches, update and commit

**Alt+B** says on the status line: *CVS keeps its branches and old files on the server: Open in
your git tool opens a terminal in the working copy for that.* **Alt+G** opens a terminal in the
folder, where `cvs commit` and an update that merges run as usual. A plain update is **Alt+D**
([Fetch and pull](fetch-and-pull.md)).

## In the terminal app

The same line and glyphs, in the panel's bottom border.

## Questions

#### How do I see the history of a CVS working copy?

**Ctrl+G** asks once whether Coxswain may read it from the server it names; with yes it runs
`cvs log` there and groups the files' revisions into commits. Settings → Privacy and updates →
History from servers forgets the answer.

#### Can I see the files as they were at a commit?

Yes: **Enter** on the commit in the history shows its folder as it was. **F3** shows a file
(read with `cvs update -p` from the server you said yes to), the desktop app's **Diff** what the
commit changed in it, and **F5** copies it out.

#### Why are some commits split in two?

CVS records each file on its own. Coxswain joins revisions with the same commit id, or the same
author and message within five minutes; a commit that took longer, or whose files carry different
messages, shows as more than one entry.

#### How does Coxswain know a file changed?

As CVS does: by its time against the one `CVS/Entries` keeps. A file only touched shows as
changed until `cvs update` sets the time again.

#### How do I update the working copy?

Press **Alt+D** (*Pull (fast-forward only)*). Coxswain first runs `cvs -n update`, which
changes nothing; when it lists a file changed here (`M` or `C`), the update is refused and the
file named, since `cvs update` would merge into it. When that look fails (the server refuses
without a `cvs login`, or does not answer), nothing is updated and the status line shows cvs's
own words. Otherwise `cvs update` runs and the status line says how many files came from the
server. A folder with `CVS/Update.prog` (a program
`cvs update` would run) is refused. There is no Fetch in CVS.

#### Which files are left out as ignored?

CVS's own list (`*.o`, `*~`, `core` and the like), then `~/.cvsignore` and each folder's
`.cvsignore`; a `!` in one clears the list.

#### Where is the top of the working copy?

The highest folder of the unbroken run of folders with `CVS/Entries` above the one you are in.
The line and the counts cover everything from there down.

---
[← Previous: Subversion working copies](subversion.md) · [Next: The mouse →](mouse.md)

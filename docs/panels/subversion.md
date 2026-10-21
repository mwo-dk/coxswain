[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Subversion working copies

In a [Subversion](https://subversion.apache.org/) working copy, Coxswain shows its status the way
it shows git's: the line at the bottom of the pane names `svn`, the revision and the branch, and
each changed file carries its glyph. It reads only the working copy's own records,
`.svn/wc.db`, so nothing is sent to the server and `svn` need not be installed. History,
branches, update and commit stay with Subversion: **Alt+G** opens a terminal in the working copy
for them. Both apps do this.

<!-- screenshot: svn-line.png: the desktop app (Cyber) in a Subversion working copy: the pane footer reads svn r2 trunk with one modified and one unversioned file -->

## Contents

- [What you see](#what-you-see)
- [How a change is found](#how-a-change-is-found)
- [History, update and commit](#history-update-and-commit)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## What you see

| Where | What |
|---|---|
| The line (desktop app: the pane footer; terminal app: in the panel's bottom border) | `svn:` in ASCII glyphs (or the branch glyph and `svn` with Nerd Font glyphs), the working copy's revision (`r2`), its branch (`trunk`, a branch's or tag's name from the repository path, else its last folder), then the counts as on the [git line](git.md#the-git-line): `+` added, `~` modified or replaced, `-` deleted or missing, `?` unversioned, `!` in conflict, `=` when clean |
| Each entry | The glyph of its change; a folder takes the loudest of its contents |
| Ignored files | Dimmed, as git's: those `svn:ignore` names on their folder, and Subversion's own list (`*.o`, `*.pyc`, `__pycache__`, `*~`, `.DS_Store` and the like) |

The revision is the one the top of the working copy was last updated to; a mixed working copy
(some folders updated further) shows the top's.

## How a change is found

As Subversion does, without asking the server:

- **Added, deleted, replaced, in conflict:** from the working copy's records (what `svn add`,
  `svn rm`, `svn copy` and a merge left there).
- **Modified:** a file whose size differs from the one recorded is changed. One whose size is the
  same but whose time moved is compared byte for byte with the pristine copy Subversion keeps in
  `.svn/pristine`; only a difference counts.
- **Missing:** a file in the records and not on the disk counts as deleted.
- **Unversioned:** what is on the disk and not in the records; an unversioned folder is one entry.

Keywords (`svn:keywords`) and line endings (`svn:eol-style`) are not expanded for the
comparison: a file with them that was only touched may show as modified until `svn status` or
`svn update` records its time again.

## History, update and commit

Subversion keeps the history and the branches on its server, and Coxswain never contacts a
server by itself. **Ctrl+G** and **Alt+B** in a working copy say so on the status line:
*Subversion keeps its history and branches on the server, which Coxswain does not ask: Open in
your git tool opens a terminal in the working copy for that.* **Alt+G** (*Open in your git tool*)
opens a terminal there, in this terminal for the terminal app or a new window for the desktop
app, where `svn log`, `svn update` and `svn commit` run as usual. A git tool is never opened for
a working copy; a command of your own in *Settings → Behaviour → Git tool* is.

## In the terminal app

The same line and glyphs, in the panel's bottom border.

## Questions

#### Why is there no history for a Subversion working copy?

Subversion keeps it on its server, and Coxswain never contacts a server by itself. **Alt+G**
opens a terminal in the working copy for `svn log` and `svn update`.

#### Does Coxswain need svn installed?

No. It reads the working copy's own database, `.svn/wc.db`; `svn` is needed only for what you
run yourself in the terminal.

#### Why is a file I only touched not shown as changed?

When its time moved but its size did not, Coxswain compares it with the copy Subversion keeps,
and shows it changed only when the bytes differ.

#### Which working copies does it read?

Those of Subversion 1.7 and later, which keep one `.svn` folder with `wc.db` at the top. An older
working copy (a `.svn` in every folder, no `wc.db`) shows no line: `svn upgrade` turns it into a
current one.

---
[← Previous: Mercurial in the panels](mercurial.md) · [Next: CVS working copies →](cvs.md)

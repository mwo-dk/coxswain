[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Who changed each line

**Alt+L** shows, before each line of a file, the commit that last changed it, its author and
its date: `a1b2c3d Ada          2026-09-30`. It is `git blame`, in both apps: in the viewer, in
the built-in editor and in the desktop app's preview pane. Lines you changed since the last
commit say *not committed yet*. From a line, the [history of its function](git-line-history.md)
is one key or one click away.

<!-- screenshot: blame-tui.png: the terminal app's viewer in Classic blue on engine.rs with the blame margin (short id, author, date) before each line and the key line naming Alt+L and Alt+J -->
<!-- screenshot: blame-gui.png: the desktop app's preview pane in Cyber on engine.rs with Who changed on: each line's commit, author and date in grey before it, one not committed in yellow -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Which repositories](#which-repositories)
- [Limits](#limits)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to use it

| Where | Terminal app | Desktop app |
|---|---|---|
| A file in the panel | **Alt+L** opens it in the viewer with who changed each line | **Alt+L** shows the preview pane with *Who changed* on |
| The viewer, **F3** | **Alt+L** shows or hides it | The preview pane: the *Who changed* button at its top |
| The built-in editor, **Shift+F4** | **Alt+L** shows or hides it beside the line numbers | **Alt+L**, or the *Who changed* button at the top |
| The action menu, **Shift+F10**, and **F9** | *Who changed each line* | Same |

It is read on a thread: *Reading who changed each line…* shows until it comes, and **Esc**
stops it (*Stopped*). In the desktop app's preview a *Stop* link does the same.

## What you see

- **Terminal app:** a grey margin before each line, 31 columns: the short commit id, the
  author (cut to 12 characters) and the date. The line numbers stay after it. The key line ends
  with *Alt+L who changed each line · Alt+J history of this function*.
- **Desktop app, preview pane:** each line of the file with its commit, author and date in grey
  before it; a line not committed yet in the *modified* colour. Hover a commit for its subject;
  click it for the [history of the function](git-line-history.md) on that line.
- **Desktop app, editor:** the margin goes in front of the line numbers. The *Who changed*
  button is lit while it shows.

In the editor, typing puts it away (*The lines changed: save, then ask who changed them
again*), since the lines no longer match; with changes not saved it is not shown
at all (*Save first*).

## Which repositories

| | |
|---|---|
| git | Yes, through Coxswain's own git command: no hooks, no filters and no text conversion the repository names |
| [Jujutsu](jujutsu.md) | Yes, through git and the commits jj keeps in it (best with `.git` beside `.jj`) |
| [Mercurial](mercurial.md) | Yes, through `hg annotate` (needs `hg` 5.4 or later); commit, author and date, no subject |
| [Subversion](subversion.md), [CVS](cvs.md) | No: *Who changed each line is for files in a git, Jujutsu or Mercurial repository* |

## Limits

| | |
|---|---|
| Lines | The first 100,000; the status line says the rest were left out |
| File size | Up to 32 MB; a bigger file is refused (*too big to blame*) |
| Time | git gets a minute; then it is stopped (*git took too long*) |
| Desktop preview | The first 512 KB of the file, as the preview reads it |

Nothing is written: blame only reads the repository. Nothing leaves your machine.

## Settings and config.toml

There is no setting. The key is `blame` under `[keys]` (default **Alt+L**), in the viewer and
the editor too:

```toml
[keys]
blame = ["Alt+L"]
```

## Questions

#### Why does a line say *not committed yet*?

You changed it (or added it) since the last commit. Commit it and the line gets that commit.

#### Why does Alt+L do nothing in my editor?

Blame is of the file as saved: with changes not saved it says *Save first*. Save with
**Ctrl+S**, then **Alt+L**.

#### Why is the margin gone after I typed?

The lines moved, so the margin would point at the wrong commits. Save and press **Alt+L**
again.

#### Does it work in a Mercurial repository?

Yes, with `hg` 5.4 or later: commit, author and date per line. Without `hg`, a notice says what
to install.

#### How do I see the whole commit a line came from?

Note its id, then **Ctrl+G** on the file ([Git history as folders](git-history.md)) and **Enter**
on that commit. For the commits of that function alone, **Alt+J**
([History of a function](git-line-history.md)).

#### It takes long on a big file. Can I stop it?

**Esc** stops it; after a minute it stops by itself.

---
[← Previous: Git history as folders](git-history.md) · [Next: History of a function →](git-line-history.md)

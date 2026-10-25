[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# History of a function

**Alt+J** lists the commits that changed the function the cursor is in, newest first, each
with its author, date and subject and what it changed there. With lines selected, it follows
those lines instead. It is `git log -L`, in both apps: in the viewer, the built-in editor and
the desktop app's [who changed each line](git-blame.md) view.

<!-- screenshot: line-history-gui.png: the desktop app's History of a function window in Cyber over the editor: engine.rs › fn stop, three commits with author and date, the newest open showing its coloured diff -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [How the function is found](#how-the-function-is-found)
- [Limits](#limits)
- [Questions](#questions)

## How to use it

| Where | Terminal app | Desktop app |
|---|---|---|
| The built-in editor, **Shift+F4** | **Alt+J**: the function the cursor is in, or the lines selected | **Alt+J**, or the *History of this function* button |
| The viewer, **F3** | **Alt+J**: the function at the line found (**/**) or gone to (**g**), else at the top of the screen | – |
| The preview pane, with *Who changed* on | – | Click a line's commit |
| A file in the panel | **Alt+J** says where to ask it | Same |

*Reading the history…* shows while git reads; **Esc** stops it.

## What you see

- **Terminal app:** a viewer over the file, titled *engine.rs › fn stop*: each commit as
  `commit a1b2c3d · Ada · 2026-09-30`, its subject below, then its change coloured as a diff.
  **/**, **n** and **N** search it; **Esc** comes back to the file.
- **Desktop app:** a window titled the same, the number of commits under it, and each commit as
  a line you can open (the newest open): its id, author, date and subject, then its change.
  **Esc** or *Close* comes back to the editor or the panels as they were.

A function no commit changed since it came shows the commit that brought it. One not in the
last commit yet says *fn stop is not in the last commit: it has no history yet*.

## How the function is found

Coxswain reads the code the way it cuts code for [search by meaning](../search/meaning.md): by
brackets, indentation and keywords, for every language it knows as code. The innermost named
item at the line is taken (a method in a class, a function in a module), from the comments
above it to its last line. That item is then looked up by its name in the file as the last
commit has it, so changes not committed yet do not move it. Lines selected, a line in no
function, or a file that is no code: those lines alone, as numbered in the last commit.

## Limits

| | |
|---|---|
| Commits | The newest 200; a line at the end says when there were more |
| Size | 4 MB of history at most |
| Time | git gets a minute |
| Repositories | git, and [Jujutsu](jujutsu.md) through git; not Mercurial, Subversion or CVS |

The key is `line_history` under `[keys]` (default **Alt+J**).

## Questions

#### Why does it show a different function from the one I meant?

It takes the innermost named item at the cursor. Put the cursor inside the function's body, or
select the lines you mean.

#### Why does it say the function is not in the last commit?

It is new, or renamed, since the last commit: there is no history of it yet. Commit it first.

#### Does it follow a function moved to another file?

No: `git log -L` follows lines within the file, and the file through renames.

#### Why only 200 commits?

To keep it quick. For more, run `git log -L :name:file` on the command line.

#### Can I see it in the desktop app's preview pane?

Turn on *Who changed* at the top of the preview and click a line's commit.

---
[← Previous: Who changed each line](git-blame.md) · [Next: Git branches and worktrees →](git-branches.md)

[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Git history in search

Find file searches the history of your git repositories as well as your files: the message of
every commit, who wrote it, and the paths it changed. Type `fuel valve` in Find and
the commit *Fix the fuel valve* is found under *History*, marked as a commit; **Enter** opens the repository as
it was at that commit. With [search by meaning](meaning.md) on, commits about your words are
found too, and [Ask](ask.md) may answer from commit messages. Both apps have it, through the
[search helper](helper.md).

<!-- screenshot: search-history.png: the desktop app (Cyber), Find with "fuel valve" typed: the History group with the commit 6ba7e53 · Ada · Fix the fuel valve in /home/demo/projects/rocket, marked as a commit, under In files with files on disk and one inside a zip -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [What is kept, and when](#what-is-kept-and-when)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Press **Ctrl+F** for [Find file](find-file.md), or **Shift+F7** (**Ctrl+Shift+F** in the
   desktop app) for its *In files* kind, which shows *In files* and *History* alone.
2. Type words from a commit message, an author's name, or a path a commit changed:
   `valve`, `Ada`, `engine.rs`.
3. Commits show in the **History** group, after the files. **Enter** on one: the active panel shows the repository's top
   folder as it was at that commit ([Git history as folders](../panels/git-history.md)).
   **Backspace** goes to the list of commits, and again to the repository on disk.

## What you see

A commit hit has the git commit glyph, the repository's name in bold, *commit in
/home/me/projects/rocket* after it, and a passage that starts with which commit it is:

```
rocket   commit in /home/me/projects/rocket
   commit a1b2c3d · Ada · 2026-09-30 · Fix the fuel valve
```

Your words are highlighted, as in any text hit; a commit found by meaning alone is under
*History* too, its passage in italics. In Ask, a commit is a numbered source like a file; **Enter** on it opens the
commit.

Find's footer, *text of 31,208 files*, counts the commits kept as well.

## What is kept, and when

| | |
|---|---|
| Which repositories | Every git work tree (a folder holding `.git`) inside the [folders read](folders.md), outside the folders left out (`node_modules`, hidden folders, `.nosearch`) |
| Which commits | The newest 2000 reachable from HEAD of each repository |
| What of each | Its short id, author, date, the whole message, and up to 200 paths it changed. Never the files' contents at that commit: those are searched as files on disk |
| When | At each scan of the helper (at its start and every ten minutes). A repository whose HEAD moved on gets only the new commits; one whose history was rewritten (rebase, reset) is read again; one that is gone loses its commits |
| How fast | 2000 commits of a big repository take git well under a second; the helper rests after each repository as it does after reading files |
| Where | In the [search store](text.md), `search.db`, with the text of your files. **Delete the index** in Settings empties it too |

A new commit is found within ten minutes; a change of a search setting in the desktop app
restarts the helper, which then reads at once.

## Settings and config.toml

| Setting | config.toml | Default |
|---|---|---|
| *Settings → Search inside files → Search the history of git repositories too: commit messages, authors and changed paths* | `[search] history` | `true` |
| Which folders are read (and so which repositories) | `[search] text_roots`, `names_only`, `text_exclude` | your home folder |

Turned off, the commits are taken out of the store at the next scan. It needs *Search inside
files* (`[search] text`) on.

## In the terminal app

The same: commits are hits in *text:*, with the passage starting `commit a1b2c3d · Ada · …`.
**Enter** opens the commit in the panel; **F3** and **F4** do nothing on a commit hit. Turn it
off with `history = false` under `[search]` in `config.toml`; the helper uses it from its next
start.

## Questions

#### Why does Find file not find my latest commit?

The helper reads repositories at its scans, at most ten minutes apart, so wait a little.
Commits in a repository outside the folders read are never kept.

#### Why does it not find an old commit?

Only the newest 2000 of each repository are kept. Use `git log --grep=…` on the command line
for older ones.

#### Does it search the code as it was in old commits?

No: the messages, authors and changed paths. Files are searched as they are on disk. To read a
file as it was, open the commit and preview it ([Git history as folders](../panels/git-history.md)).

#### Does it send my commits anywhere?

No. They stay in the store on your machine. With search by meaning on a server, their text goes
to that server for vectors, as your files' text does ([Servers](servers.md)).

#### Can I keep one repository's history out?

Put an empty `.nosearch` file in the repository, or add it to *Names only*: then neither its files
nor its commits are read.

---
[← Previous: Diagrams read as sentences](diagrams.md) · [Next: Inside archives →](archives.md)

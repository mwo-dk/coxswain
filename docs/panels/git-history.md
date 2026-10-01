[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Git history as folders

**Ctrl+G** on a file or folder in a git repository opens its history the way **Enter** opens
an archive: a listing of the commits that touched it, newest first. **Enter** on a commit
shows the files as they were then, as read-only folders: move around, preview a file, see what
the commit changed in it, view it with **F3** and copy an old version out with **F5**.
**Backspace** leads back up to the commits, then to the folder on disk. Both apps do this.

<!-- screenshot: git-history-commits.png: desktop app, Cyber theme: the left pane showing ~/projects/rocket/src/main.rs/@history, tinted, the badge "history of main.rs" at the end of the path bar, rows "a1b2c3d Fly the rocket", "d15e851 Start the rocket" with Modified and Last commit (author) columns; the right pane ordinary -->

<!-- screenshot: git-history-file.png: desktop app, Cyber theme: inside a commit (badge "commit a1b2c3d"), main.rs under the cursor, the preview pane showing its Diff of that commit -->

<!-- screenshot: tui-git-history.png: terminal app, NC theme: the left panel titled '/home/demo/projects/rocket/src/main.rs/@history [history]' with two commits, the info line showing 'a1b2c3d 2026-09-30 14:02 Ada' -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [What works in a history](#what-works-in-a-history)
- [Paths through a history](#paths-through-a-history)
- [Limits](#limits)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. In a folder of a git repository, put the cursor on a file or folder and press **Ctrl+G**.
   On `..` it is the history of the folder you are in; at the top of the repository, of the
   whole repository. The same is in **F9** as *Git history*, and in the desktop app's preview
   pane as the button *History of main.rs* under *Git*.
2. The pane lists the commits that changed it, newest first: `a1b2c3d Fly the rocket`, with the
   commit's date in *Modified*. In the desktop app the *Last commit* column shows its author;
   in the terminal app the line under the panel shows id, date and author.
3. **Enter** on a commit: the folder as it was at that commit (for a file, the folder it was
   in). Go into folders as usual.
4. On a file: the desktop app's preview pane (**Space**, or **F3**) shows it as it was, and
   **Diff** at its top what that commit changed in it. The terminal app's **F3** opens it in
   your pager.
5. **F5** copies the file or folder under the cursor, as it was then, to the other panel.
   **F5** on a commit in the list copies the whole folder at that commit, named
   `src-a1b2c3d`. The files copied out get the commit's date, not today's.
6. **Backspace** (or `..`) goes up: from a commit to the list of commits, from the list back to
   the folder on disk, with the cursor on the file you started from.

## What you see

| | Desktop app | Terminal app |
|---|---|---|
| The pane | Tinted, as inside an archive | – |
| The path | `…/main.rs/@history/a1b2c3d4e5f6/…`, the `@history` part marked | The same path in the panel's title |
| A badge | *history of main.rs* in the list; *commit a1b2c3d* inside a commit (hover it for the full id, author, date and subject) | `[history]`, `[commit a1b2c3d]` after the title |
| A commit in the list | Its short id and subject as the name, the commit's date in *Modified*, its author in *Last commit* | Short id and subject, the date in *Modified*; id, date and author in the info line |
| A file at a commit | Its size then, the commit's date; *Last commit* as of that commit | The same; the info line shows its last commit as of then |

## What works in a history

| Key | In a history |
|---|---|
| **Enter** | On a commit or folder: in. On a file: the status line says *This is the file as it was at that commit: F3 shows it, F5 copies it out* |
| **Backspace**, `..` | Up: to the commits, then to the folder on disk |
| **F3** | Desktop app: the preview pane, with *File / Diff*. Terminal app: your pager, on a copy of the file |
| **F5** | Copies out, as it was then. Never over an existing file |
| **F4**, **F6**, **F7**, **F8** | Refused: *A history is read-only: F5 copies a file or folder out of it* |
| **Ctrl+G** | The status line says history is for files and folders on disk: you are in one already |
| Find file | Works as anywhere; *In this folder* searches names on disk, not in the history |

The preview pane reads a copy of the file, made when the cursor rests on it (up to 64 MB), so
every kind of file previews as it would on disk: pictures, PDFs, Markdown, code.

## Paths through a history

A history is a path, as an archive is: `<file or folder>/@history` is the list of commits, and
`<file or folder>/@history/<commit>/<path>` the folder at that commit. You can type one in the
path bar (**Ctrl+L**) or **Alt+F1**, or copy it to the command line. The commit is
the first 12 characters of its id; any 4 to 40 work. A real folder named `@history` is just a
folder.

## Limits

- **2000 commits** at most in a list; the oldest are left out. The repository's own history
  (`..` at its top) is the longest one; a single file's is usually short.
- **Four seconds** at most for git to answer a list, so a huge repository stays usable; a file
  that changed rarely in a very long history may then show fewer commits than it has.
- **Renames are not followed:** a file's history starts where it got its current name.
- **Merge commits** are listed only when the merge itself changed it, as `git log` does.
- **Submodules** show as empty entries; their own history is in their own folder.
- **Read-only:** nothing is ever written to the repository. It runs `git log`, `git ls-tree`,
  `git cat-file` and `git show`, with `GIT_OPTIONAL_LOCKS=0`.
- It needs `git` on your `PATH`, and a commit to start from: a new repository without commits
  has no history yet.

## Settings and config.toml

| What | config.toml | Default |
|---|---|---|
| The key | `history` in `[keys]` | `["Ctrl+G"]` |
| The *Last commit* column and line | `[git] last_commit` | `true` |
| Commits in Find file | `[search] history` ([History in search](../search/history.md)) | `true` |

The first time you open a folder of a repository, the status line tells you once that the
history is there and which key opens it ([Notices](../search/notices.md)).

## In the terminal app

Everything above works the same, keys included; the differences are in
[The terminal app](../reference/terminal-app.md#git-history-in-the-terminal-app): the title says
`[history]` or `[commit a1b2c3d]`, and **F3** opens a copy in your pager instead of the preview
pane. A long list of commits is read on a thread: the status line says *Working on …* until it
is there, and the keys keep working meanwhile.

## Questions

#### How do I see who changed a file, and when?

The *Last commit* column (desktop app) or the line under the panel (terminal app) shows the
last one. **Ctrl+G** lists them all, each with its date and author.

#### How do I get back an old version of a file?

**Ctrl+G** on the file, **Enter** on the commit you want, then **F5** on the file to copy it to
the other panel. Copying never overwrites: copy it to another folder, or delete the newer one
first.

#### How do I see what a commit changed?

In the desktop app, put the cursor on the file inside the commit and pick **Diff** at the top
of the preview pane: that commit's change to that file. For the whole commit, use the user
menu's *git log* (**F2**, `l`) or a command such as `git show a1b2c3d`.

#### How do I browse the whole repository as it was?

Press **Ctrl+G** on `..` at the top of the repository, then **Enter** on a commit.

#### Why does a file's history stop at a rename?

Coxswain follows the path, as `git log -- <path>` does without `--follow`. Look in the history of
the folder it was in, or of the repository, for the commit that renamed it.

#### Why do I not see all commits?

A list has at most 2000, and git gets four seconds for it. Use `git log` in the command line
for the full history.

#### Can I change files in a history, or check out a commit?

No: a history is read-only, and Coxswain never changes the repository. Copy what you need out
with **F5**; for a checkout use `git switch` or `git checkout` on the command line.

#### How do I know I am looking at an old version and not the file on disk?

The desktop app tints the pane and shows *commit a1b2c3d* at the end of the path; the terminal
app ends the title in `[commit a1b2c3d]`. The path holds `@history`.

---
[← Previous: Git in the panels](git.md) · [Next: The mouse →](mouse.md)

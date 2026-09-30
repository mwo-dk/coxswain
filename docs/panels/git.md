[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Git in the panels

Inside a git repository every panel shows where you stand, oh-my-posh style: the branch,
commits ahead of and behind the upstream, and counts of what changed. Each file and folder
gets a glyph for its own state. Both apps do this; the desktop app adds a diff in the preview
pane and a list of the repositories you visited.

![The desktop app in a repository: the git line in the pane's footer, glyphs next to changed files](../screenshots/gui-details.png)
*`src` carries the pencil of a modified file inside it, `target` the crossed-out eye of an ignored folder, `TODO.txt` the question mark of an untracked file. The footer reads `master ↑1` and the counts.*

## Contents

- [How to use it](#how-to-use-it)
- [The git line](#the-git-line)
- [File and folder glyphs](#file-and-folder-glyphs)
- [When it updates](#when-it-updates)
- [The diff of a file (desktop app)](#the-diff-of-a-file-desktop-app)
- [Recent repositories (desktop app)](#recent-repositories-desktop-app)
- [Git commands from the user menu](#git-commands-from-the-user-menu)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Go into a folder of a git work tree. The git line and the glyphs appear as soon as git
   answers; there is nothing to switch on.
2. Press **Ctrl+R** after committing or pulling in another terminal, so the line is read again.
3. Desktop app: with the cursor on a changed file, open the preview pane (**Space**) and pick
   **Diff** at its top.
4. Press **F2** for the user menu's git commands (see below).

It needs `git` on your `PATH`. Coxswain only reads: it runs `git status` (with
`GIT_OPTIONAL_LOCKS=0`, so it never takes a lock that could get in the way of your own git
commands) and, for the diff, `git diff`. It never commits, stages or changes anything.

## The git line

In the terminal app it is at the bottom left of the panel's border; in the desktop app at the
right of the pane's footer (hover it for the repository's folder). A folder's preview in the
desktop app shows it too, as *Git*.

With plain characters (`glyphs = "ascii"`) a line reads:

```
git: master ^1 v2 +3 ~1 -2 ?4 !1 $1
```

| Part | Nerd Font glyph | ASCII | Means |
|---|---|---|---|
| Branch | branch (U+E0A0) | `git:` | The branch, or the short commit id when HEAD is detached |
| Ahead | `↑` | `^` | Commits not yet pushed to the upstream |
| Behind | `↓` | `v` | Commits on the upstream not yet pulled |
| Staged | ticked box (U+F046) | `+` | Files with changes in the index |
| Modified | pencil (U+F044) | `~` | Files changed in the work tree |
| Deleted | bin (U+F014) | `-` | Files deleted in the work tree |
| Untracked | question mark (U+F128) | `?` | Files git does not track |
| Conflicts | warning sign (U+F071) | `!` | Files with merge conflicts |
| Stash | box (U+EB4B) | `$` | Entries in the stash |
| Clean | tick (U+F00C) | `=` | Shown when nothing is staged, changed, deleted, untracked or in conflict |

A count of zero is left out. The line is drawn in the theme's `git_branch` colour (bold
magenta in NC).

## File and folder glyphs

Each entry in a repository gets a glyph before its name (terminal app) or after it (desktop
app, details view), in the colour of its state:

| State | Glyph (Nerd / ASCII) | Colour slot | NC colour |
|---|---|---|---|
| Modified | pencil / `~` | `git_modified` | yellow |
| Added (staged new file) | ticked box / `+` | `git_added` | green |
| Untracked | question mark / `?` | `git_untracked` | red |
| Deleted | bin / `-` | `git_deleted` | dark red |
| Renamed | arrow (U+F45A) / `>` | `git_renamed` | blue |
| Conflict | warning sign / `!` | `git_conflict` | bold red |
| Ignored | crossed-out eye (U+F070) / `.` | `git_ignored` | grey |

**Folders take the loudest state of what is inside them**, so a changed file deep in `src/`
marks `src` too. From quiet to loud: ignored, untracked, added, renamed, deleted, modified,
conflict. Everything inside an untracked or ignored folder shows that state. In the desktop
app, hovering a glyph names the state (*modified*), with *(staged)* when the change is in the
index.

Deleted files are no longer on disk, so they have no row; they only count in the git line.

## When it updates

| | Terminal app | Desktop app |
|---|---|---|
| Read | When the panel changes folder, after an operation or a command, and on **Ctrl+R** | Every time the folder is loaded: opening it, a change the watcher sees in it, **Ctrl+R**, and after operations |
| Runs | In the background; the line appears when git answers | The same |

## The diff of a file (desktop app)

With the cursor on a file git sees as changed (modified, added, renamed or in conflict; not
untracked or ignored), the [preview pane](../previews/README.md) (**Space**) gets a **File /
Diff** switch at the top. **Diff** (*Changes against HEAD*) shows `git diff HEAD` for that
file: staged and unstaged changes together, added lines green, removed lines red. The choice
sticks for the next changed file and across restarts. A file with nothing against HEAD shows
*(no changes against HEAD)*.

![Four previews side by side: a calendar, the git diff of main.rs with the Diff switch on, a log file and an EPUB book](../screenshots/gui-previews-more.png)
*The second panel is the diff of `main.rs`: the line changed from `30.0` to `45.0`.*

## Recent repositories (desktop app)

Every repository you open a folder of is remembered, most recent first, up to twelve. They are
listed under **Git repositories** in the [sidebar](../organise/sidebar.md); click one to go
there. A tab whose folder is in a repository shows the git icon instead of the folder icon.

## Git commands from the user menu

The default [user menu](../commands/user-menu.md) (**F2**) has *git status* (`s`), *git log*
(`l`, the last 50 commits as a graph), *git diff (file)* (`d`, of the file under the cursor)
and *git blame (file)* (`b`, through `less`).

## Settings and config.toml

| Setting | config.toml | Default |
|---|---|---|
| *Settings → Appearance → Icons and git glyphs*: *Nerd Font* or *Plain characters (ASCII)* | `glyphs` (`"nerd"` / `"ascii"`) | `"nerd"` |
| Each glyph yourself | `[glyph_set]` table | – |
| The colours | theme slots `git_branch`, `git_modified`, `git_added`, `git_untracked`, `git_deleted`, `git_renamed`, `git_conflict`, `git_ignored` | per theme |
| The user menu's git commands | `[[user_menu]]` | four entries |

See [Glyphs and fonts](../customise/glyphs-and-fonts.md) and [Your own theme](../customise/own-theme.md).

## In the terminal app

The same git line (bottom left of the border) and the same glyphs (in a column before the
name). No diff, no list of repositories and no git icon on tabs, since it has no preview pane,
sidebar or tabs; the user menu's `git diff (file)` shows the diff instead.

## Questions

#### Why is there no git line?

The folder is not inside a git work tree, or `git` is not on your `PATH`. On macOS, the desktop
app started from the Dock reads `PATH` from your login shell; a `git` installed elsewhere is
not found. Inside an archive there is no git line either.

#### Why are the counts different from `git status`?

They are the same counts, taken from `git status --porcelain=v2`, but grouped: a file both
staged and modified counts in both. Ignored files are never counted.

#### Why is `target` shown with a crossed-out eye?

It is ignored by your `.gitignore`. Ignored files and folders get that glyph so you can tell
build output from sources.

#### I see boxes or question marks instead of glyphs.

The font has no Nerd Font glyphs. In the terminal, use a Nerd Font as the terminal's font. In
the desktop app, install one of the fonts listed in `[gui] icon_font` (for example *Symbols
Nerd Font Mono*), or set *Settings → Appearance → Icons and git glyphs* to *Plain characters
(ASCII)*. The config key is `glyphs = "ascii"`, for both apps.

#### I committed in a terminal and the git line did not change.

A commit changes files inside `.git`, not the folder on screen, so nothing tells Coxswain.
Press **Ctrl+R**.

#### Is it slow in a huge repository?

`git status` runs in the background, so browsing is not held up; the line and glyphs appear
when it finishes. Its speed is git's own.

#### Why is there no Diff switch for a new file?

An untracked file has nothing to compare with. Stage it (`git add`) and it becomes *added*:
the switch appears, and the diff shows the whole file as new.

#### Can Coxswain commit or stage?

No, it only reads. Put the commands you want in the [user menu](../commands/user-menu.md)
(**F2**) or a [script](../commands/scripts.md).

---
[← Previous: Folder sizes](folder-sizes.md) · [Next: The mouse →](mouse.md)

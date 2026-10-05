[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# The screen

Both apps show two panels, a command line and a row of function keys, as Norton Commander
did. This page names every part of the screen, says which panel is active, what the title
bar and the status line tell you, and how to start the apps in the folders you want.

![The desktop app: two panes, the sidebar, git status in the left pane, the command line and the F-key bar](../screenshots/gui-details.png)
*The desktop app in the Cyber theme: the left pane is active (green frame), the right one shows `~/Documents`.*

![The terminal app: two blue panels with double borders, git status bottom left, the command line and the F-key bar](../screenshots/tui-panels.png)
*The terminal app in the Norton Commander theme: the left panel is active (its path is drawn black on cyan).*

<!-- screenshot: panels-title.png: the desktop app's window (Cyber) with the title Coxswain 1.41.0 in its title bar, the panes below -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [The title bar: the version](#the-title-bar-the-version)
- [The status line](#the-status-line)
- [Starting in a folder](#starting-in-a-folder)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Press **Tab** to make the other panel active. The keys act on the active panel, and
   **F5** (copy) and **F6** (move) offer the other panel's folder as the target.
2. Click in a panel to make it active (both apps).
3. Type anywhere: characters that are not a key of their own go to the command line
   ([The command line](../commands/command-line.md)).
4. Click an F-key in the bottom row (desktop app) to run it, or press the key.
5. **Ctrl+O** switches the desktop app between one pane and two
   ([Tabs, back and forward, one pane or two](tabs-and-panes.md)); in the terminal app it
   hides the panels and shows the output of the last command.

## What you see

| Part | Terminal app | Desktop app |
|---|---|---|
| Panels | Two, with double borders | Two panes, each with tabs, a path bar, the list and a footer |
| Active panel | Its path at the top is drawn in the cursor colour (black on cyan in NC), and only it shows the cursor bar | A coloured frame |
| Folder path | Centred in the top border; a long path keeps its end (`…/projects/rocket`) | The path bar under the tabs, one button per part (`~ › projects › rocket`) |
| Columns | Name, Size, Modified; Modified goes when the panel is narrower than 44 columns | Name, Type, Size, Modified by default; Files and Created can be added ([Views](views.md#the-columns-menu)) |
| Git line | Bottom left of the border, in the `git_branch` colour | Right side of the pane's footer ([Git in the panels](git.md)) |
| Info line | Under the list: the name under the cursor and its size (`name -> target` for a link), or `4.2 MB in 3 selected` | The footer: `17 items`, and `· 3 selected (1.2 MB)` when files are marked |
| Sort order | A letter bottom right: `n` name, `x` extension, `t` time, `s` size; upper case when reversed | An arrow next to the column header that sorts ([Sorting](sorting.md)) |
| Command line | The row under the panels: `/home/demo/projects/rocket> ` | The row under the panes: the folder, `❯`, and *Type a command…* |
| Status | In the command line row, until the next key | Right of the command line, until the next key |
| Settings | – | *Settings* with a cog, at the right of the command line row |
| F-key bar | The bottom row: the number and label of **F1** to **F10** | The bottom row of buttons; a key with no action is greyed |
| Window title | The terminal's title: `Coxswain 1.41.0` | The window's title, the same text |

The F-key labels are Norton Commander's: *Help*, *Menu*, *View*, *Edit*, *Copy*, *RenMov*,
*Mkdir*, *Delete*, *PullDn*, *Quit*. They follow your `[keys]`: bind another action to
**F2** and its name shows there (two actions on one key: the one the key runs).

**Colours.** In the Norton Commander theme (the terminal app's default) folders are bold
white, marked files bold yellow, programs green, links magenta, hidden files dim cyan, and the
cursor is black on cyan. In Cyber (the desktop app's default) the cursor row is a brighter
green band and folders are cyan. Other themes: [Themes](../customise/themes.md).

**Inside an archive** the panel says so, so a copy out of it is not taken for a copy between
two folders. The desktop app tints the pane, marks the archive's name in the path bar and
shows an *archive* badge (*archive, locked* for one with a password); the terminal app puts
`[archive]` after the path in the top border. See [Archives as folders](../files/archives.md).

**A folder that cannot be read** (it was deleted, or you may not open it): the panel goes up
to the nearest folder it can list. The terminal app shows the error in red on the info line;
the desktop app shows it above the list or in the status line.

## The title bar: the version

Both apps put the version in their title:

```
Coxswain 1.41.0
```

Before 1.41.0 the title also listed the kinds of search that were on (`search: names · text ·
meaning`); that now shows where it is used, on the first footer line of [Find
file](../search/find-file.md#what-you-see): *563 files indexed · text of 112 files · meaning for
112*. On Linux the title bar that GTK draws is told as well, so it shows the same text as the
task bar. The terminal app sets the terminal's title every five seconds, starting five seconds
after it starts; terminals that show titles show it in their title bar or tab.

## The status line

Messages go to the command line row and stay until the next key: *Reread*, `Copied "a.txt"`,
`Moved 3 items to the bin`, `Opened report.pdf`, `Not a folder: /tmp/x`, and errors.

Two kinds of message come by themselves:

- **Updates:** `Coxswain 1.21.0 is available: <how>` when a newer release is out
  ([Update checks](../reference/updates.md)). In the desktop app it is a button that opens
  the release page.
- **Tips** of what search can do that you have not turned on (*Install tesseract to search the
  words in scans, screenshots and pictures*), and after an upgrade what it brought. The desktop
  app counts them on its **⚙ Settings** button and lists them under *Settings → What's new*; only
  a problem with search (*Search inside files has stopped: …*) is a button in the status line,
  with a `×`. The terminal app shows one tip at a time once in the status line, with the command
  that turns it on, and counts it as seen; after an upgrade it points to `coxswain --whats-new`.
  See [Notices and what's new](../search/notices.md).

## Starting in a folder

Both apps take up to two folders on the command line: the left and the right panel. A file
opens its folder with the cursor on it.

```sh
coxswain ~/src ~/Downloads
coxswain-gui .
coxswain-gui ~/Pictures/cat.jpg
```

| | Terminal app | Desktop app |
|---|---|---|
| No folders named | Both panels open in the current folder | The last session comes back ([What the apps remember](session.md)); the current folder is added to the left pane as a new tab unless one of its tabs already shows it |
| Folders named | Left and right panel | With a saved session, the first folder is added to the left pane as a new tab; without one, left and right pane |
| Started from a desktop menu (current folder `/`) | – | Your home folder takes the place of the current folder |

Every flag: [Command-line flags](../reference/command-line-flags.md).

## Settings and config.toml

The screen itself has no settings of its own. What changes it:

| Setting | config.toml | Default |
|---|---|---|
| *Settings → Appearance*, theme | `theme` (terminal app), `[gui] theme` (desktop app) | `"nc"`, `"cyber"` |
| *Settings → Appearance → Icons and git glyphs* | `glyphs` (`"nerd"` or `"ascii"`) | `"nerd"` |
| *Settings → Search inside files* | `[search] text` (the `text` in the title) | `true` |
| – | `check_updates` (the update message) | `true` |

See [Configuration](../reference/configuration.md) for every key.

## In the terminal app

The same two panels, command line and F-key bar, drawn in text. What differs:

- A folder shows `SUB-DIR` in the Size column until its size is measured, and `..` shows
  `UP--DIR`, as in NC ([Folder sizes](folder-sizes.md)).
- Sizes are plain bytes up to 100,000,000; above that they are shown as `95M`, `12G`.
- No tabs, sidebar or preview pane: a terminal has no room for them, and the F3 viewer and F4
  editor take their place ([View and edit](../commands/view-and-edit.md)).
- The terminal's title is set every five seconds; there is no window to own.

## Questions

#### Which panel is active?

In the terminal app, the one whose path at the top is drawn in the cursor colour and that
shows the cursor bar; the other panel hides its cursor. In the desktop app, the pane with the
coloured frame. **Tab** switches, and a click in a panel makes it active.

#### Why is the Modified column gone in the terminal app?

The panel is narrower than 44 columns inside its border, so only Name and Size fit. Make the
terminal wider. Below 12 columns or 4 rows the panel shows its border only.

#### Where did "search: names · text" in the title go?

To Find file, where it is used: its first footer line says what can be searched, *563 files
indexed · text of 112 files · meaning for 112*, and what is still under way. When *meaning for*
is missing though you turned it on, the helper has not started it yet or its model is not
there; see [Search by meaning](../search/meaning.md#questions).

#### The title of my terminal does not change. Why?

The terminal app sets it with the usual escape sequence, every five seconds. Some terminals
ignore it, or show a title of their own (tmux and screen need `set-titles on`). Nothing else
depends on it.

#### Why is there a new tab in the left pane every time I start the desktop app?

The folder it was started in is not open in any of the left pane's tabs, so it is added. As
long as one of those tabs shows that folder, none is added. A start from the desktop menu
begins in your home folder, so keeping a tab on home stops it.

#### A folder of my last session is gone. What happens?

The pane goes up to the parent folder, with the cursor where the folder was; if that is gone
too, to your home folder.

#### Why does the terminal app show 104857600 but the desktop app 100 MB?

The terminal app keeps exact bytes up to 100,000,000, as NC did, so sizes line up digit by
digit. The desktop app shows exact bytes below 10 KB and then one decimal (`32.7 KB`).

#### What is the "archive" badge in the path bar?

The pane is inside an archive, browsed like a folder. **F5** copies out of it, **F6** moves
out, **F8** takes files out, and copies into it are added to it. See
[Archives as folders](../files/archives.md).

---
[← Previous: Panels and keys](README.md) · [Next: Moving around and going to a folder →](moving.md)

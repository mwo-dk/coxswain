[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# The command list (F9) and Help (F1)

**F9** opens a list of every action, with its key, that you filter by typing: you never need
to remember a key. **F1** shows the keys as your config has them, under group headings, with
the search syntax and where the config file is.

![The desktop app's F9 Commands list filtered by theme: Theme: Cyber marked current, then Dark, Light, Nord and the other themes](../screenshots/panels-command-list.png)
![The terminal app's F1 Help at 120 columns: the groups in two columns, Moving at the top left, Files, Archives, Search and Git on the right](../screenshots/panels-help.png)

## How to use it

### The command list

1. Press **F9**. The *Commands* list opens.
2. Type part of an action's name: `sort`, `tab`, `size`. The list keeps the entries whose
   name contains it, in any case.
3. **Up** and **Down** move, **Enter** runs the entry, **Esc** closes. In the desktop app a
   click runs an entry too.

| Key | In the list |
|---|---|
| Letters | Filter |
| **Backspace** | Take a letter back (terminal app; in the desktop app the filter is a text field) |
| **Up** / **Down** | Move |
| **Enter** | Run |
| **Esc** | Close (in the terminal app **F10** too) |

### Help

Press **F1**.

| | Terminal app | Desktop app |
|---|---|---|
| Scroll | **Up**, **Down**, **PageUp**, **PageDown** | The mouse wheel |
| The [first-run guide](first-run.md) again | **G** | The **Show the guide again** button at the top |
| Close | Any other key | **Esc**, **Enter**, **F1**, or a click outside it |

## What you see

**The groups.** Both lists put the actions in nine groups, in the order a new user needs
them, and in each group the most used action first:

| Group | Holds |
|---|---|
| Moving | Open, Up, Down, Parent folder, Page up, Page down, First, Last, Back, Forward, Left: go to, Right: go to, Edit path |
| Panels and tabs | Other panel, Hidden files, Refresh, Other panel here, Swap panels, Panels on/off, the tabs, the views, the sidebar, sorting, folder sizes, columns |
| Marking | Mark, Mark all, Mark group, Unmark group, Invert marks |
| Files | Copy, Move, Delete, New folder, the clipboard, Delete permanently, Properties, Batch rename, Colour tag, ZFS snapshots, File flags, Files of this package |
| Archives | Extract archive, Pack into an archive |
| Search | Find, Find in files, Ask your files, Find duplicates |
| Git | Git history, Git branches, Switch to branch, Git worktrees, New branch here |
| Viewing and editing | View, Edit, Preview, Path to command line, Menu (the user menu), Folder notes |
| App | Help, Commands (this list), Settings, Quit |

[Every default key](keys.md) has the same tables, with each key.

**The command list.** Each entry has its name and its first key. With the filter empty, the
desktop app shows the group headings above their entries, and the themes come last, under
*App*; once you type, the headings go and the filter searches every entry. The terminal app
keeps the same order without headings. The terminal app lists the
actions it has. The desktop app lists all of them, including those with no key
(*Folder sizes*, *Columns and folder sizes*), plus one entry per built-in theme
(*Theme: Windows 95*); the current theme is marked *current*, and picking one switches to it
and saves it in `config.toml`. *Up*, *Down* and the list itself are left out.

**Help.**

| | Terminal app | Desktop app |
|---|---|---|
| Title | *Help*, then `Coxswain 1.20.0 — the ship's officer who gets the work done.` | `Coxswain 1.20.0 · keyboard shortcuts` |
| Keys | *Keys (from your config):* every action it has, with all its keys, under the group headings; in two columns when the terminal is wide enough (about 100 columns), else in one | Every action with all its keys, written as on the keyboard (`Ctrl+G`), under the group headings |
| Also | Alt+letter quick search, typing goes to the command line, `cd`, Ctrl+O, the mouse | *Mouse: double-click opens · Ctrl-click / right-click marks · Shift-click marks a range · drag …* |
| Search | *Find (Everything syntax):* with one example per line | *Find uses Everything's syntax:* with the examples |
| Config | `Config: <path>` and *Run `coxswain --dump-config` for every option with its default.* | `Config: <path> · every option: coxswain --dump-config` |
| The guide | *G: the first-run guide again.* at the end | The **Show the guide again** button under the title |

The names are in the language you chose ([Languages](../customise/languages.md)), and the
keys are the ones in force, so a rebinding shows at once.

## Settings and config.toml

| Action | Config name | Default key |
|---|---|---|
| The command list (*Commands*) | `menu` | `F9` |
| Help | `help` | `F1` |

The actions in the list are all the `[keys]` names; see [Every default key](keys.md).

## In the terminal app

The same list and help, drawn as dialogs. It lists only the actions the terminal app has;
the desktop-only ones are left out rather than shown and refused. It has no theme entries:
*Settings* in the list opens [its Settings](../customise/settings.md#in-the-terminal-app), where
*Looks* chooses the theme.

## Questions

#### Where is the pull-down menu?

**F9** opens the searchable command list, *Commands*, instead. It reaches every action that a
menu would. The terminal app's F-key bar still labels it *PullDn*, for NC's sake; the desktop
app's says *Commands*.

#### How do I see the first-run guide again?

From Help: **F1**, then **G** in the terminal app, or the **Show the guide again** button in the
desktop app. Or *Settings → Overview → Show the guide again* in both apps. See
[First-run guide](first-run.md).

#### Where did *Reread*, *RenMov* and *Mkdir* go in the list?

2.0 names each action in plain words, the same in both apps: *Refresh* (**Ctrl+R**), *Move*
(**F6**), *New folder* (**F7**), *Commands* (**F9**), *Find* (**Alt+F7**), *Find in files*
(**Shift+F7**), *Parent folder*, *Mark group*. Type the new word in the filter. The terminal
app's F-key bar keeps *RenMov*, *Mkdir* and *PullDn*.

#### How do I run an action that has no key?

Find it in the **F9** list: *Folder sizes* and *Columns and folder sizes* have no key by
default. Or give it one in `[keys]` ([Changing keys](../customise/keys.md)).

#### How do I switch themes quickly?

Desktop app: **F9**, type `theme`, pick one. It is saved at once. Terminal app: set `theme`
in `config.toml` and restart.

#### Why does Help not list Colour tag in the terminal app?

The terminal app's help lists only the actions it has. The desktop app's help lists all of
them.

#### Why are the actions not in alphabetical order?

They are grouped by what they do, so the keys that belong together sit together: moving
first, the app last, and in each group the most used first. To find one by name, type part
of it in **F9**: the filter searches every group.

#### Why is the list's key column empty for some entries?

Those actions have no key, by default or because you unbound them (`= []`).

#### How do I find the config file?

**F1** shows its path at the bottom. `coxswain --config-path` prints it, and
`coxswain --paths` prints where everything is kept ([Where things are kept](../reference/where-things-are-kept.md)).

---
[← Previous: The mouse](mouse.md) · [Next: What the apps remember →](session.md)

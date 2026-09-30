[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Every default key

Every key both apps have out of the box, with the action's name in the **F9** list and the
config name that `[keys]` uses. Use it as a reference, or as the list to start from when you
[change keys](../customise/keys.md).

<!-- screenshot: panels-keys.png: desktop app, Cyber theme, the F1 help window "Coxswain 1.20.0 · keyboard shortcuts" listing actions with their keys -->

## Contents

- [How to use it](#how-to-use-it)
- [Files and panels](#files-and-panels)
- [Moving](#moving)
- [Marking and sorting](#marking-and-sorting)
- [Search, views and panes](#search-views-and-panes)
- [Desktop app only](#desktop-app-only)
- [Keys that are not actions](#keys-that-are-not-actions)
- [Keys inside dialogs](#keys-inside-dialogs)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

Find the key in the tables below. *Both* means the key does the same in each app; where they
differ, the columns say how. An action marked *–* in the terminal column answers there with
the status *… is available in the desktop app (coxswain-gui)*. **F1** shows the keys you have
now, and **F9** finds any action by name.

On a Mac the shortcuts are the same, with **Ctrl**, not Cmd.

## What you see

### Files and panels

| Key | Action (F9 name) | Config name | Desktop app | Terminal app |
|---|---|---|---|---|
| **F1** | Help | `help` | The help window | The help screen ([Help](command-list.md)) |
| **F2** | Menu | `user_menu` | Scripts and the user menu | The user menu ([The user menu](../commands/user-menu.md)) |
| **F3** | View | `view` | The preview pane on or off | The file in your viewer (`less`) ([View and edit](../commands/view-and-edit.md)) |
| **F4** | Edit | `edit` | The file in your editor | The same |
| **F5** | Copy | `copy` | Copy to the other pane ([Copy](../files/copy.md)) | The same |
| **F6** | RenMov | `move` | Move or rename ([Move and rename](../files/move-and-rename.md)) | The same |
| **F7** | Mkdir | `mkdir` | New folder ([New folder](../files/new-folder.md)) | The same |
| **F8**, **Delete** | Delete | `delete` | To the trash ([Delete](../files/delete.md)) | The same |
| **Shift+F8**, **Shift+Delete** | Delete permanently | `delete_forever` | Delete for good | The same |
| **F9** | PullDn | `menu` | The command list | The same |
| **F10** | Quit | `quit` | Close the window | Quit |
| **Alt+F5** | Pack into an archive | `pack` | Pack the marked files ([Pack and extract](../files/pack-and-extract.md)) | The same |
| **Ctrl+E** | Extract archive | `extract` | Extract into the other pane | The same |
| **Ctrl+Enter**, **Ctrl+J** | Path to command line | `copy_path` | The name under the cursor to the command line | The same |

### Moving

| Key | Action (F9 name) | Config name | Desktop app | Terminal app |
|---|---|---|---|---|
| **Up** / **Down** | Up / Down | `up` / `down` | The cursor ([Moving around](moving.md)) | The same |
| **PageUp**, **Left** | Page up | `page_up` | A page up (columns view: **Left** goes up; thumbnails: one tile left) | A page up |
| **PageDown**, **Right** | Page down | `page_down` | A page down (columns view: **Right** opens the folder; thumbnails: one tile right) | A page down |
| **Home** / **End** | First / Last | `home` / `end` | First / last entry | The same |
| **Enter** | Open | `open` | Open a folder or archive here, a file in its program | The same; a program runs in the panel's folder |
| **Ctrl+PageUp**, **Backspace** | Parent dir | `parent` | Up to the parent folder | The same |
| **Tab** | Other panel | `switch_panel` | The other pane | The other panel |
| **Ctrl+U** | Swap panels | `swap_panels` | Swap the panes | Swap the panels |
| **Alt+O** | Other panel here | `same_dir` | The active pane's folder in the other pane | The same |
| **Alt+F1** / **Alt+F2** | Left: go to / Right: go to | `goto_left` / `goto_right` | Type a path in the left / right pane's path bar | *Left panel* / *Right panel* dialog |
| **Ctrl+R** | Reread | `refresh` | Reread the panes | Reread the panels |

### Marking and sorting

| Key | Action (F9 name) | Config name | Desktop app | Terminal app |
|---|---|---|---|---|
| **Insert**, **Shift+Down** | Mark | `mark` | Mark and move down ([Marking files](marking.md)) | The same |
| `+` | Select group | `select_group` | Mark files by pattern | The same |
| `-` | Unselect group | `unselect_group` | Unmark files by pattern | The same |
| `*` | Invert selection | `invert_selection` | Invert the marks of files | The same |
| **Ctrl+F3** | Sort by name | `sort_name` | By name; again reverses ([Sorting](sorting.md)) | The same |
| **Ctrl+F4** | Sort by extension | `sort_ext` | By extension | The same |
| **Ctrl+F5** | Sort by time | `sort_time` | By time, newest first | The same |
| **Ctrl+F6** | Sort by size | `sort_size` | By size, largest first | The same |
| **Alt+.** | Hidden files | `toggle_hidden` | Show or hide hidden files | The same |

### Search, views and panes

| Key | Action (F9 name) | Config name | Desktop app | Terminal app |
|---|---|---|---|---|
| **Alt+F7**, **Ctrl+F** | Find file | `search` | Find file ([Find file](../search/find-file.md)) | The same |
| **Ctrl+O** | Panels on/off | `toggle_panels` | One pane or two ([Tabs and panes](tabs-and-panes.md)) | The output of the last command |
| – | Folder sizes | `dir_sizes` | Measure the marked folders, or all, again ([Folder sizes](folder-sizes.md)) | Measure the panel's folders again |

### Desktop app only

| Key | Action (F9 name) | Config name | Does |
|---|---|---|---|
| **Ctrl+T** | New tab | `new_tab` | A new tab in the same folder and view |
| **Ctrl+W** | Close tab | `close_tab` | Close the tab (the last one stays) |
| **Ctrl+Tab** | Next tab | `next_tab` | The next tab of the pane |
| **Ctrl+Shift+Tab** | Previous tab | `prev_tab` | The previous tab |
| **Alt+Left** / **Alt+Right** | Back / Forward | `back` / `forward` | The tab's history |
| **Space** | Preview | `toggle_preview` | The preview pane on or off ([The preview pane](../previews/README.md)) |
| **Alt+V** | Details/columns/thumbnails | `toggle_view` | The next view ([Views](views.md)) |
| **Ctrl+B** | Sidebar | `toggle_sidebar` | The sidebar on or off ([The sidebar](../organise/sidebar.md)) |
| **Ctrl+L** | Edit path | `edit_path` | Type a path in the active pane |
| **Ctrl+M** | Batch rename | `batch_rename` | Rename the marked files by pattern ([Batch rename](../files/batch-rename.md)) |
| **Alt+T** | Colour tag | `tag` | Tag the marked files ([Colour tags](../organise/tags.md)) |
| **Alt+N** | Folder notes | `notes` | The folder's notes in the preview pane ([Folder notes](../organise/notes.md)) |
| **Ctrl+C** / **Ctrl+X** / **Ctrl+V** | Copy to clipboard / Cut to clipboard / Paste | `clip_copy` / `clip_cut` / `paste` | Files through the system clipboard ([Clipboard](../files/clipboard.md)) |
| **Alt+Enter** | Properties | `properties` | Size, dates, permissions ([Properties](../files/properties.md)) |
| **Ctrl+D** | Find duplicates | `duplicates` | The duplicate finder ([Finding duplicates](../files/duplicates.md)) |
| **Ctrl+,** | Settings | `settings` | The Settings window ([Settings](../customise/settings.md)) |
| – | Columns and folder sizes | `columns` | The columns menu ([Views](views.md#the-columns-menu)); also a right-click on a column header |

### Keys that are not actions

These are fixed; `[keys]` does not change them.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Alt+letter** | [Quick search](quick-search.md) (letters and digits not bound to an action) | Quick search (any character not bound) |
| Other characters | Go to the command line | The same |
| **Enter** with text in the command line | Runs it; output in the preview pane ([The command line](../commands/command-line.md)) | Runs it with the panels hidden |
| **Esc** | Clears the command line; with it empty, closes the preview pane; closes a dialog | Clears the command line; closes a dialog |
| **Backspace** with text in the command line | Edits it | The same |
| **Left**, **Right**, **Home**, **End**, **Delete**, **Space** with text in the command line | Edit it | **Space** is typed; the others keep their panel meaning (**Delete** asks to delete the entry under the cursor) |
| **Ctrl+C**, **Ctrl+X**, **Ctrl+V** with text in the command line | Copy, cut and paste text | – |
| **Left** / **Right** in the columns view | Up / into the folder | – |
| Arrows in thumbnails | Move in two dimensions | – |

### Keys inside dialogs

| Dialog | Keys |
|---|---|
| Any prompt (copy, move, new folder, go to, select group, password) | **Enter** OK, **Esc** cancel. Terminal app: **Backspace** deletes, **Ctrl+U** clears the line, **F10** cancels too |
| Delete confirmation | **Enter** or **Y** deletes, **Esc** or **N** cancels |
| Find file | Type to search, **Tab** the next depth, **Up** / **Down** / **PageUp** / **PageDown** move, **Enter** goes to the hit, **F4** edits it, **Esc** closes; terminal app also **F3** views it ([Find file](../search/find-file.md)) |
| Command list (**F9**), columns menu | Type to filter, **Up** / **Down**, **Enter** runs, **Esc** closes |
| User menu (**F2**) | An entry's own key runs it at once; **Up** / **Down** and **Enter** too; **Esc** closes |
| Colour tag (**Alt+T**, desktop app) | **1** to **7** a colour, **0** removes the tag, **Esc** closes |
| Help (**F1**) | Desktop app: **Esc**, **Enter** or **F1** closes. Terminal app: **Up** / **Down** / **PageUp** / **PageDown** scroll, any other key closes |
| Error message | Desktop app: **Enter** or **Esc**. Terminal app: any key |
| After a program or a waiting command (terminal app) | **Enter** at `-- press Enter --` |
| **Ctrl+O** output (terminal app) | Any key back to the panels |

## Settings and config.toml

Every action above is a key under `[keys]` in `config.toml`, with a list of keys:

```toml
[keys]
search = ["Ctrl+P"]        # replaces Alt+F7 and Ctrl+F
tag = []                   # unbinds Alt+T, freeing it for quick search
dir_sizes = ["Ctrl+Q"]     # gives Folder sizes a key
```

Listing an action replaces its default keys; `[]` unbinds it. Key names are written as in the
tables (`Ctrl+Shift+Tab`, `Alt+.`, `F5`, `Space`). `coxswain --dump-config` prints every
action with its default keys. The rules and names: [Changing keys](../customise/keys.md) and
[Configuration](../reference/configuration.md).

## In the terminal app

It has every action in the first four tables. The desktop-only ones answer with *… is
available in the desktop app (coxswain-gui)* in the status line, because they need what a
terminal lacks: tabs, a preview pane, a sidebar, pictures, the system clipboard. Keys the
terminal itself takes do not reach it: many terminals send **Ctrl+Enter** as plain **Enter**,
which is why **Ctrl+J** does the same; some keep **Ctrl+F3** to **Ctrl+F6** or **Alt+letter**
for themselves.

## Questions

#### How do I change a key?

In `config.toml`, under `[keys]`: `search = ["Ctrl+P"]`. Listing an action replaces its
default keys; `[]` unbinds it. See [Changing keys](../customise/keys.md).

#### I bound a key and now the desktop app ignores my whole config. Why?

A key it cannot read (`"Ctlr+P"`) makes the config invalid. The terminal app refuses to start
and says `coxswain: unknown key 'Ctlr+P'`; the desktop app starts with every default and
prints the error on its terminal. Fix the name and restart.

#### Why does Ctrl+C in the command line copy text, not files?

While the command line has text, **Ctrl+C**, **Ctrl+X** and **Ctrl+V** edit it, as in any text
field. With the command line empty they copy, cut and paste files.

#### Space in the terminal app says "Preview is available in the desktop app". Why?

**Space** is bound to the preview pane, which the terminal app does not have. With text in the
command line, **Space** is typed as usual. Unbind it (`toggle_preview = []`) to silence the
message.

#### Why is F3 the preview in the desktop app, not a viewer?

NC's **F3** showed the file; the preview pane does that in place, and **Esc** or **F3** again
closes it. For a file in your own viewer, use a [script](../commands/scripts.md) or open it
with **Enter**.

#### Why are the shortcuts Ctrl on a Mac, not Cmd?

Both apps read the same `[keys]`, and Norton Commander's keys are Ctrl keys. `[keys]` knows
Ctrl, Alt and Shift, not Cmd. Cmd-click still marks, as in other Mac apps.

#### Two actions have the same key. Which one wins?

One key drives one action. When two actions list the same key, the one that comes later in
the `[keys]` part of `coxswain --dump-config` takes it, and **F1** in the desktop app shows the
key only there. Give each key to one action only.

#### Can a key run a shell command?

Not from `[keys]`. Put the command in the [user menu](../commands/user-menu.md) (**F2** and a
letter) or a [script](../commands/scripts.md).

---
[← Previous: What the apps remember](session.md) · [Next: Tags, notes, favourites and the sidebar →](../organise/README.md)

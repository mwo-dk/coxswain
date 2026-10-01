[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Views: details, columns, thumbnails

The desktop app shows a folder in three ways, chosen per tab: **details** with columns,
**Miller columns** that show where you are in the tree, and **thumbnails** that show pictures
as themselves. **Alt+V** goes from one to the next.

![Miller columns: two parent folders, the current folder src and a source file shown in the preview pane](../screenshots/gui-columns.png)
*The columns view: `projects`, `rocket` and `src`, with `main.rs` under the cursor and in the preview pane.*

![Thumbnails of the Pictures folder, with the details view in the right pane and sunset.jpg in the preview pane](../screenshots/gui-thumbnails.png)
*Thumbnails on the left; the right pane stays in details.*

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [The columns menu](#the-columns-menu)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Press **Alt+V**, or click the view button at the far right of the path bar
   (*Details / columns / thumbnails (Alt+V)*). The tab goes details → columns → thumbnails →
   details.
2. Move with the keys of that view:

| View | Keys that differ |
|---|---|
| Details | None: **Left** and **Right** page, as everywhere ([Moving around](moving.md)) |
| Columns | **Left** goes up to the parent folder; **Right** opens the folder under the cursor |
| Thumbnails | **Left**, **Right**, **Up** and **Down** move in two dimensions, one tile or one row |

Every other key (**Enter**, **Backspace**, **Insert**, **F5**, …) works as in details.

## What you see

| View | Shows |
|---|---|
| Details | One row per entry: icon, name, colour tag, git glyph, then Type, Size, Modified with its age chip and, in a git repository, Last commit; Files and Created on request ([The columns menu](#the-columns-menu)) |
| Columns | Up to two parent folders, the current folder, and a peek into the folder under the cursor (*Empty folder* when there is nothing in it). The folder you are in is highlighted in its parent's column; folders end in a `›` |
| Thumbnails | A grid of tiles, at least 112 pixels wide: pictures show themselves, other files and folders their icon; the colour tag is a dot before the name |

In the columns view, a click on an entry in a parent column or in the peek column goes to
that folder with the cursor on the entry.

**The age chip** in the Modified column is coloured by age: red within the hour, yellow
within a day, green within a week, cyan within a month, blue within a year, grey after, with
the hue sliding in between. In themes with the CRT look (Cyber) it goes from cyan when new
to dim green. Its text is the age: `3m`, `5h`, `2d`, `3w`, `4mo`, `1y`. Hover it for the
exact date.

**Narrow panes drop columns**: below about 620 pixels Type, Files, Last commit and Created go and
Modified shrinks to its age chip; below 340 pixels Size goes too. Widen the pane (drag the
bar between the panes, or **Ctrl+O** for one pane) to bring them back.

## The columns menu

Right-click a column header in the details view (its tooltip: *Right-click for columns and
folder sizes*), or pick **Columns and folder sizes** in the **F9** list. A menu of tick boxes
opens; **Enter** or a click ticks one, and the menu stays open for the next. **Esc** closes it.

| Entry | Shows | Default |
|---|---|---|
| Type | The extension, or *Folder* | On |
| Size | The file's size; a folder's once measured | On |
| Files (in folders) | For folders: how many files are inside, all levels down | Off |
| Modified | The age chip and the date and time | On |
| Last commit | In a git repository: the date and author of the last commit that changed it, *older* beyond the newest 5000 commits; click its header to sort by it ([Last commit per file](git.md#last-commit-per-file)) | On (listed when `[git] last_commit` is on) |
| Created | The date the file was created, where the file system records it | Off |
| Measure folder sizes automatically | Not a column: the switch for [folder sizes](folder-sizes.md) | On |

The name column is always there. The choices apply to every tab.

<!-- screenshot: panels-columns-menu.png: desktop app, Cyber theme, the "Columns and folder sizes" menu opened by a right-click on the column header, with Files (in folders) and Created ticked -->

## Settings and config.toml

| What | Where | Default |
|---|---|---|
| The view of each tab | The session only | Details |
| The columns | The session only (the columns menu) | Type, Size, Modified, Last commit |
| Last commit at all | `[git] last_commit`, *Settings → Behaviour* | `true` |
| Measure folder sizes automatically | `folder_sizes` (bool) to start with, then the session | `true` |
| Keys | `toggle_view` = `Alt+V`; `columns` has no key | – |

## In the terminal app

One view: Name, Size and Modified, as in NC; Modified goes when the panel is narrower than 44
columns. A text grid has no room for Miller columns and cannot draw pictures in every
terminal, so **Alt+V** says *Details/columns/thumbnails is available in the desktop app
(coxswain-gui)*.

## Questions

#### Why is my photo folder slow in thumbnails?

Thumbnails are the pictures themselves, decoded at full size by the webview; there is no
thumbnail cache. A folder of large photos takes a moment, and tiles load as they scroll into
sight. Only the rows of tiles on screen are in the page, so a folder of 100,000 files opens in
thumbnails as quickly as in details.

#### Why do I not see git glyphs in the columns or thumbnails view?

Only the details view has room for them. The git line in the pane's footer still shows the
repository's state ([Git in the panels](git.md)).

#### Why is the Created column empty?

The file system does not record when files were created: common on older Linux file systems
and network shares.

#### Can each tab have its own view?

Yes. The view belongs to the tab, and **Ctrl+T** opens a new tab in the view of the one you
are in. Each tab's view is kept in the session.

#### Why did Type and Modified's date disappear?

The pane is narrower than about 620 pixels. Widen it, hide the sidebar (**Ctrl+B**) or
switch to one pane (**Ctrl+O**).

#### Can I sort in the columns or thumbnails view?

Yes, with **Ctrl+F3** to **Ctrl+F6**; the order is the tab's, in every view. Only the details
view has headers to click.

#### What does the peek column show for a file?

Nothing: it peeks into folders only. For a file, open the preview pane (**Space**).

---
[← Previous: Tabs, back and forward, one pane or two](tabs-and-panes.md) · [Next: Folder sizes →](folder-sizes.md)

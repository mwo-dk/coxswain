[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# The mouse

Coxswain is made for the keyboard, but the mouse works in both apps: click to move, double-click
to open, right-click to mark. The desktop app adds dragging, the path bar, the F-key buttons
and bars to resize.

![The desktop app: panes, sidebar, path bars, F-key buttons and the bars between them](../screenshots/gui-details.png)
*Everything here can be clicked: the tabs, the path bar parts, the column headers, the sidebar, the F-key buttons.*

## How to use it

| Mouse | Terminal app | Desktop app |
|---|---|---|
| Click | Moves the cursor there and makes the panel active | The same |
| Double-click | Opens (a folder or archive in the panel, a file in its program) | The same |
| Right-click | Marks or unmarks, and makes the panel active | Marks or unmarks, in the details and thumbnails views |
| Wheel | Moves the cursor of the panel under the mouse by three | Scrolls the list |
| Ctrl-click (Cmd-click on a Mac) | – | Marks or unmarks, in every view |
| Shift-click | – | Marks the range from the cursor to the click (details view) |
| Drag | – | Copies or moves to the other pane or to another application; see [Drag and drop](../files/drag-and-drop.md) |
| Column header | – | Sorts; a second click reverses. Right-click opens the [columns menu](views.md#the-columns-menu) |
| Path bar | – | A part goes to that folder; empty space turns it into a text field. The `←`, `→` and `↑` buttons go back, forward and up |
| Tabs | – | A click shows the tab, a middle click or the `×` closes it, the `+` opens a new one |
| View and pane buttons (right of the path bar) | – | Details / columns / thumbnails; one pane or two |
| F-key bar | – | Runs that key's action |
| *Settings* (cog, right of the command line) | – | Opens Settings |
| Bars between the sidebar, the panes and the preview pane | – | Drag to resize: the sidebar 150 to 480 pixels, the preview pane 240 to 900, the split between the panes 20% to 80% |

In the desktop app's columns view, a click in a parent column or in the peek column goes to
that folder with the cursor on the entry you clicked ([Views](views.md)).

## What you see

- The panel you click becomes active: its frame (desktop app) or its path (terminal app)
  lights up.
- While you drag files from another application over a pane, the pane gets a dashed outline;
  dropping opens a menu with *Copy here* (`c`) and *Move here* (`m`).
- A double-click is two clicks on the same spot within 0.4 seconds in the terminal app, and the
  system's own double-click time in the desktop app.

## Settings and config.toml

None. The widths you drag to are kept in the desktop app's session
([What the apps remember](session.md)).

## In the terminal app

Click, double-click, right-click and the wheel, in the panels only. There is no drag, since a
terminal cannot hand files to other programs, and clicks do nothing while a dialog is open:
use its keys. Some terminals keep the mouse for selecting text; hold **Shift** while you drag
to select text anyway.

## Questions

#### Why does right-click mark instead of opening a menu?

That is Norton Commander's way, and it makes marking with the mouse quick. The actions a menu
would offer are in the command list (**F9**).

#### How do I mark a range with the mouse?

In the desktop app's details view: click the first entry, then Shift-click the last. In the
other views and in the terminal app, right-click (or Ctrl-click) each one, or use `+` with a
pattern ([Marking files](marking.md)).

#### Can I copy by dragging in the terminal app?

No. Mark the files and press **F5** (copy) or **F6** (move).

#### Mouse clicks in the terminal app select text instead. Why?

The terminal passes the mouse to Coxswain only when it supports mouse reporting and has it on;
some keep it for themselves (or behind a setting). Coxswain turns reporting on at start. With
it on, hold **Shift** to select text.

#### How do I type a path with the mouse?

Click an empty part of the path bar (desktop app): it becomes a text field with the path
selected. **Enter** goes there, **Esc** or a click elsewhere leaves it.

#### How do I make the panes the same width again?

Drag the bar between them back to the middle. The split is kept between runs, so it will not
reset by itself.

---
[← Previous: Git in the panels](git.md) · [Next: The command list (F9) and Help (F1) →](command-list.md)

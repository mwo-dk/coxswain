[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# What the apps remember

The desktop app comes back the way you left it: its tabs, folders, views and pane widths. The
terminal app starts fresh where you start it, as NC did. This page lists what is kept, where,
and what is forgotten on purpose.

<!-- screenshot: panels-session.png: desktop app, Cyber theme, just after a restart: the left pane with three tabs, the columns view in one of them, the preview pane open at its saved width -->

## How to use it

There is nothing to do: the desktop app saves its session a moment (about half a second)
after anything in it changes, and restores it at the next start. To start from a clean
session, quit the desktop app and remove the `session` part of `state.json` (or the whole
file, which also forgets favourites, tags and notes).

To start in given folders instead, name them on the command line
([Starting in a folder](the-screen.md#starting-in-a-folder)).

## What you see

### Kept by the desktop app (the session)

| What | Details |
|---|---|
| Tabs | Every pane's tabs with their folders, views (details, columns, thumbnails) and sort orders, and which tab is current |
| Panes | Which pane is active, one pane or two, the split between them |
| Hidden files | Shown or hidden (**Alt+.**) |
| Sidebar and preview pane | Shown or not, and their widths |
| Columns | The columns menu's choices and *Measure folder sizes automatically* |
| Preview choices | *Source* or rendered, *File* or *Diff*, the engine picked for a tool |

### Kept by both apps, outside the session

| What | Where |
|---|---|
| Favourites, colour tags, folder notes, recent git repositories | `state.json` (desktop app features: [Tags, notes, favourites and the sidebar](../organise/README.md)) |
| Notices dismissed or shown, the version last started, the last update check | `state.json`, shared by both apps ([Notices](../search/notices.md)) |
| The theme picked in **F9** or Settings, and every other setting | `config.toml` |

### Forgotten on purpose

| What | Why |
|---|---|
| Marks | They belong to a task, and go when you leave the folder anyway |
| The cursor position in each tab | The folder comes back; the cursor starts at the top |
| Back and forward history | Each tab starts with an empty history |
| The command line's text and the last command's output | They were for that moment |
| Passwords of encrypted archives | Kept in memory for the app run only, never written ([Archive passwords](../files/archive-passwords.md)) |

## Settings and config.toml

The session has no settings. `show_hidden` and `folder_sizes` in `config.toml` are only where
the desktop app starts the first time; after that the session's own choice wins. Where
`state.json` lives on each system: [Where things are kept](../reference/where-things-are-kept.md),
or run `coxswain --paths`.

## In the terminal app

It remembers no session: every start opens the folders you name, or the current folder in
both panels, with name order and `show_hidden` from the config. A terminal app is started
from a shell in a folder, so that folder is the natural place to begin. It does read and
write `state.json` for the notices, so a notice shown in one app is not shown again in the
other.

## Questions

#### Where is the session stored?

In `state.json`, in Coxswain's data folder (`~/.local/share/coxswain/state.json` on Linux).
`coxswain --paths` prints the exact path.

#### How do I reset the desktop app's layout?

Quit it, then delete `state.json` or just its `"session"` part. The next start opens both
panes in the current folder with the defaults. Deleting the file also drops favourites, tags
and notes.

#### Why do I get a new tab every time I start the desktop app?

The folder it was started in is not open in any tab of the left pane, so it is added; see
[The screen](the-screen.md#why-is-there-a-new-tab-in-the-left-pane-every-time-i-start-the-desktop-app).

#### What if `state.json` is broken?

It is moved aside to `state.json.bad` and the app starts with defaults, so nothing is
overwritten. Fix or delete the `.bad` file.

#### Does the terminal app remember the last folder?

No. To start where you left off, start it in that folder, or name it:
`coxswain ~/projects/rocket`.

#### Can two desktop windows share one session?

The session is one per user. The window that saves last wins, so the next start comes back
the way that window was.

---
[← Previous: The command list (F9) and Help (F1)](command-list.md) · [Next: Every default key →](keys.md)

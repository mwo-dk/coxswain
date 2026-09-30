[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# The terminal app

`coxswain` (or `cox` for short) is Coxswain in a terminal: two Norton Commander panels that work
in any terminal and over SSH. It shares its core with the desktop app, reads the same
`config.toml` and uses the same search helper, so an index built by one serves the other. This
page says what it has, what it lacks and why, and where it behaves differently.

![The terminal app: two blue panels, the git repository rocket on the left with its branch line, the home folder on the right](../screenshots/tui-panels.png)
*The terminal app in the NC theme. The left panel is a git repository: the bottom line shows `master`, ahead by one, and a count per kind of change.*

## Contents

- [How to use it](#how-to-use-it)
- [What it has](#what-it-has)
- [What only the desktop app has](#what-only-the-desktop-app-has)
- [Where the two differ](#where-the-two-differ)
- [Archives in the terminal app](#archives-in-the-terminal-app)
- [Search by meaning in the terminal app](#search-by-meaning-in-the-terminal-app)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [The terminal it needs](#the-terminal-it-needs)
- [Questions](#questions)

## How to use it

1. Start it from a shell:

   ```sh
   coxswain                     # both panels in the current folder
   coxswain ~/src ~/Downloads   # left and right
   coxswain notes.md            # its folder, with the cursor on the file
   ```

2. Move with the arrows, **Enter** to open, **Tab** for the other panel, **F5** to copy,
   **F10** to quit. The keys are the same as in the desktop app: see [Every default key](../panels/keys.md).
3. **F9** lists every action the terminal app has, with its key. **F1** shows the keys.

Every flag: [Command-line flags](command-line-flags.md#the-terminal-app-coxswain).

## What it has

| Feature | Page |
|---|---|
| Two panels, NC's keys, marking, groups, sorting, quick search, hidden files, go to folder, swap, other panel here | [Panels and keys](../panels/README.md) |
| The command line (with `cd`) and the user menu (**F2**) | [The command line](../commands/command-line.md), [The user menu](../commands/user-menu.md) |
| View (**F3**) and edit (**F4**) in your own programs | [View and edit](../commands/view-and-edit.md) |
| Copy (**F5**), move and rename (**F6**), new folder (**F7**), delete to the trash (**F8**) or for good (**Shift+F8**) | [Files](../files/README.md) |
| Archives as folders: look inside, copy and move in and out, take out, extract (**Ctrl+E**), pack (**Alt+F5**), passwords | [Archives in the terminal app](#archives-in-the-terminal-app) |
| Find file (**Alt+F7**, **Ctrl+F**) at all three depths: names everywhere, names here, text (with meaning) | [Find file](../search/find-file.md) |
| Search by meaning, turned on with `coxswain --meaning on` | [Search by meaning in the terminal app](#search-by-meaning-in-the-terminal-app) |
| Folder sizes in the Size column | [Folder sizes](../panels/folder-sizes.md) |
| The git line and a glyph per file | [Git in the panels](../panels/git.md) |
| Every theme's colours, Nerd Font or ASCII glyphs, your own themes | [Themes](../customise/themes.md), [Glyphs and fonts](../customise/glyphs-and-fonts.md) |
| All 18 languages | [Languages](../customise/languages.md) |
| Your own keys from `[keys]` | [Changing keys](../customise/keys.md) |
| The command list (**F9**) and help (**F1**) | [The command list](../panels/command-list.md) |
| The mouse: click, double-click, right-click marks, wheel | [The mouse](../panels/mouse.md) |
| Update checks and notices, in the status line; the version and search depths in the terminal's title | [Update checks](updates.md), [Notices](../search/notices.md) |

## What only the desktop app has

| Feature | Why the terminal app lacks it |
|---|---|
| [The preview pane](../previews/README.md) | A terminal cannot draw PDFs, pictures, pages or diagrams. **F3** hands the file to your pager instead |
| [Tabs, back and forward](../panels/tabs-and-panes.md), one pane or two | NC's two fixed panels are the terminal app's layout; **Ctrl+O** is taken by the command output |
| [Columns and thumbnails views](../panels/views.md), the columns menu | Miller columns and thumbnails need more room and pictures than a terminal has |
| [The sidebar](../organise/sidebar.md), [favourites](../organise/favourites.md), [colour tags](../organise/tags.md), [folder notes](../organise/notes.md) | They need a side pane and editing widgets the panels do not have room for. They are kept in `state.json`, which both apps share, so they are there when you open the desktop app |
| [The clipboard](../files/clipboard.md) (**Ctrl+C**, **Ctrl+X**, **Ctrl+V**) and [drag and drop](../files/drag-and-drop.md) | A terminal has no file clipboard or drag source; **Ctrl+C** belongs to the terminal |
| [Batch rename](../files/batch-rename.md) (**Ctrl+M**) | It needs a live preview table of old and new names |
| [Properties and permissions](../files/properties.md) (**Alt+Enter**) | A dialog of many fields; `ls -l`, `chmod` on the command line do the same |
| [Finding duplicates](../files/duplicates.md) (**Ctrl+D**) | A long-running scan with groups to tick; use `coxswain-gui --duplicates FOLDER` |
| [The Settings window](../customise/settings.md) (**Ctrl+,**) | Every setting is a `config.toml` key; the few that do more than set a value have [flags](#settings-and-configtoml) |
| [Scripts](../commands/scripts.md) in **F2** | The terminal app's **F2** is `[[user_menu]]` only |
| [Restoring the last session](../panels/session.md) | It starts where you start it, as a shell command does |
| Panels that reread themselves when a folder changes | The desktop app watches the folders it shows; the terminal app rereads after its own operations and on **Ctrl+R** |

Their keys are bound in the terminal app too (both apps share one keymap). Pressed there, the
status line says *Colour tag is available in the desktop app (coxswain-gui)*, with the
action's name. The **F9** list and **F1** help leave them out.

## Where the two differ

| | Terminal app | Desktop app |
|---|---|---|
| **F3** | Opens the file in `viewer`, `$PAGER` or `less` (`more` on Windows) | The preview pane on or off |
| **F4** | `editor`, `$VISUAL`, `$EDITOR` or `vi` (`notepad` on Windows), in the terminal | `editor` without a terminal, or the default application |
| **Ctrl+O** | Shows the terminal with the last command's output; any key comes back | One pane or two |
| **Enter** on a program | Runs it in the panel's folder and waits for Enter | Hands it to the desktop |
| **Enter** on another file | Opens it in its default application | Opens it in its default application |
| A command's output | In the terminal, the panels hidden meanwhile | In the preview pane |
| `wait` in `[[user_menu]]` | Waits for Enter afterwards (`-- press Enter --`) | Ignored: the output is always shown |
| Changes on disk | Reread after operations, commands and **Ctrl+R** | Folders on screen reread themselves |
| Theme | `theme` (default `nc`), colours only; `look` is ignored | `[gui] theme` (default `cyber`), with its look |
| Hidden files at start | Always from `show_hidden` | From the last session |
| Marked size | Files only: `4.2 MB in 3 selected` | Measured folders count too |
| Find file, **F3** | Views the hit in your pager | Does nothing (**F4** edits in both) |
| Inside an archive | The panel title ends in `[archive]` | A badge and a tint on the pane |
| An archive's password | Asked in a *Locked archive* box, shown as stars | Asked in a dialog |
| Notices | Shown once in the status line, then counted as seen | A button with `×` until dismissed or acted on |
| Update notice | `Coxswain 1.21.0 is available: brew upgrade coxswain` in the status line, checked at start | A button that opens the release page, checked every hour |
| Window title | Sets the terminal's title: `Coxswain 1.20.0 · search: names · text` | The window's title |
| Hebrew | Letters reversed in terminals without bidi support | Mirrored layout |

## Archives in the terminal app

Zip, tar (plain, `.tar.gz`, `.tar.bz2`, `.tar.xz`, `.tar.zst` and their short forms) and 7z
archives open as folders, as in the desktop app ([Archives as folders](../files/archives.md)).

| To | Do |
|---|---|
| Look inside | **Enter** on the archive. The panel's title becomes `/home/me/tools.zip [archive]` |
| Copy a file out | **F5** with the other panel on a real folder |
| Copy into the archive | **F5** from the other panel while this one is inside the archive: the files are added |
| Move out, rename inside | **F6** |
| Make a folder inside | **F7** |
| Take something out | **F8**. It asks *Take … out of tools.zip? The archive is written anew without it; there is no trash inside an archive.* |
| Extract a whole archive | **Ctrl+E**: *Extract … into a new folder in:* the other panel's folder |
| Pack into a new archive | **Alt+F5**: *Pack … into (.zip, .tar or .tar.gz):*, proposed as `name.zip` in the other panel |

**Enter** on a file inside an archive does not open it; the status line says
*tools.zip is an archive: F5 copies this file out of it*.

**Passwords.** When an encrypted zip or 7z is opened, copied from or extracted, a box titled
*Locked archive* asks *Its password (kept only for this, never saved):*. The password is typed
as stars. A wrong one asks again: *That password did not open it. Try again:*. **Esc** gives up.
A password given to look inside is kept in memory until the app quits, so moving around in the
archive does not ask again; one given for a copy or extract is used for that run only. See
[Passwords for encrypted zip and 7z](../files/archive-passwords.md).

RAR is not read, in either app: see [Archives as folders](../files/archives.md#questions).

## Search by meaning in the terminal app

Search by meaning works the same in both apps: the search helper does it, and Find file's text
depth shows its hits. What differs is how you turn it on.

1. Run `coxswain --meaning on`. It downloads the built-in model (about 488 MB), showing
   *Downloading the model for search by meaning: 42%*, sets `[search] meaning = true` and starts
   a new helper. Or `coxswain --meaning ollama` or `coxswain --meaning server URL MODEL` for a
   server ([Command-line flags](command-line-flags.md#--meaning)).
2. Start `coxswain`, press **Alt+F7**, then **Tab** twice for `text: `.
3. Type what you look for. Hits found by meaning have a second line that starts with
   `similar to:`, then the passage that was close. Words that match exactly are marked in the
   theme's search-hit colour.

While it is off and a text search finds nothing, Find file says in a dim line
*Also find files about your words, in any language: coxswain --meaning on*. The terminal's
title shows `meaning` among the search depths once the helper has it running. Full details:
[Search by meaning](../search/meaning.md), [on a server](../search/servers.md).

## What you see

- **The panels:** double blue frames, the folder path in the title (with `[archive]` inside an
  archive), columns Name, Size and Modified; `UP--DIR` and `SUB-DIR` in the Size column.
- **The status line** under the panels: the result of the last operation (`Copied 3 files`), a
  notice, an update, or the size of what is marked.
- **The command line** with the panel's folder as the prompt, and the F-key bar below it.
- **Dialogs** are grey boxes in the middle, with `Enter = OK   Esc = Cancel` or
  `[ Yes: Enter/Y ]   [ No: Esc/N ]` at the bottom.

![Find file in the terminal app at the text depth: the word engine found in seven files, each with a line of its passage](../screenshots/tui-text-search.png)
*Find file at the text depth in the terminal app: each hit takes two lines, the file and the passage.*

## Settings and config.toml

The terminal app has no Settings window. Everything the desktop app's Settings change is a key in
`config.toml` ([Configuration: every key](configuration.md)); edit it, then start the app
again. The keys only the terminal app reads are `theme` and `viewer`. Things that do more
than set a value have flags:

| To | Run |
|---|---|
| Turn search by meaning on (downloads the model) | `coxswain --meaning on` |
| Use Ollama on this machine, or another server | `coxswain --meaning ollama [MODEL]`, `coxswain --meaning server URL MODEL` |
| Back to the built-in model | `coxswain --meaning builtin` |
| Turn it off, or delete the model | `coxswain --meaning off`, `coxswain --meaning delete` |
| Start the search helper with your session | `coxswain --index-service on` (`off` to stop) |
| See every default | `coxswain --dump-config` |

Each `--meaning` and `--index-service` flag starts a new helper with the new settings.

## The terminal it needs

- **True colour** (24-bit) for the built-in themes, which use exact RGB. Nearly every current
  terminal has it.
- **A Nerd Font** as the terminal's font for icons and git glyphs, or `glyphs = "ascii"`.
- **Mouse reporting** for clicks and the wheel. While Coxswain has the mouse, most terminals
  select text with **Shift** held down.
- **Right-to-left text** only for Hebrew: Konsole, or GNOME Terminal with bidi on.

## Questions

#### Can I use the terminal app over SSH?

Yes. It runs on the remote machine, with that machine's files, `config.toml` and search helper.
Only the terminal's keys and colours cross the connection.

#### Why can't I select text with the mouse?

Coxswain takes the mouse for clicks and the wheel. Hold **Shift** while selecting (in most
terminals), or quit with **F10** first.

#### How do I see a file's contents without leaving the panels?

There is no preview pane in the terminal. **F3** opens it in your viewer (`viewer`, else
`$PAGER`, else `less`); quit the viewer (`q` in `less`) to come back to the panels.

#### I pressed Alt+T and it said "is available in the desktop app". Why is the key there at all?

Both apps share one keymap, so your `[keys]` work the same in each. The terminal app knows the
desktop-only actions and says so in the status line instead of doing nothing. **F9** and **F1**
leave them out.

#### How do I open a file inside a zip in the terminal app?

Copy it out first: **Enter** on the zip to look inside, the cursor on the file, **F5** to the
other panel's folder, then **F3** or **F4** there. **Enter** on the file only says
*tools.zip is an archive: F5 copies this file out of it*.

#### It asks for an archive's password every time I start it.

Passwords are never saved: they live in memory until the app quits. That is on purpose, so no
password is ever written to disk.

#### Does the terminal app remember where I was?

No: it starts where you start it, with the folders you name. Tags, notes and favourites the
desktop app set are kept in `state.json`, which the terminal app does not show.

#### How do I turn on search by meaning without the desktop app?

`coxswain --meaning on`, then Find file's text depth (**Alt+F7**, **Tab**, **Tab**). See
[Search by meaning in the terminal app](#search-by-meaning-in-the-terminal-app).

---
[← Previous: Reference](README.md) · [Next: Command-line flags →](command-line-flags.md)

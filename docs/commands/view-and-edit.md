[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# View and edit, F3 and F4

**F3** views the file under the cursor and **F4** edits it, as in Norton Commander. In the
terminal app **F3** opens the [built-in viewer](#the-built-in-viewer), full screen and
highlighted, and **F4** starts your editor in the terminal. In the desktop app **F3**
is the [preview pane](../previews/README.md), which shows far more than a pager can, and **F4**
starts your editor or the file's default application. **Shift+F4** edits the file in
Coxswain's own [built-in editor](editor.md), in both apps.

![The desktop app with the preview pane open at the right: the cursor is on launch-report.pdf in Documents and the pane shows its first page, Launch report, with the PDF toolbar above it and the empty notes field under it](../screenshots/gui-pdf.png)
*In the desktop app **F3** shows and hides the preview pane, here on a PDF.*

![The terminal app's built-in viewer in Classic blue after F3 on src/engine.rs in ~/projects/rocket: line numbers on the left, keywords magenta, the doc comment and the comment in italics, the title naming engine.rs, Rust, UTF-8 and 17 lines, Line 1 · 100% under the text and the key line at the bottom](../screenshots/commands-view-tui.png)

## Contents

- [How to use it](#how-to-use-it)
- [The built-in viewer](#the-built-in-viewer)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a file. Both keys do nothing on a folder or on `..`.
2. Press **F3** to view or **F4** to edit. **F9** lists them as *View* and *Edit*.

| Key | Terminal app | Desktop app |
|---|---|---|
| **F3** | Opens the file in the [built-in viewer](#the-built-in-viewer), full screen. With `viewer` set in the config, runs that program instead; the panels step aside until it ends | Shows or hides the [preview pane](../previews/README.md). When the pane shows command output, **F3** switches it back to the file preview |
| **F4** | Opens the file in your editor: `editor` from the config, else `$VISUAL`, else `$EDITOR`, else the [built-in editor](editor.md). Your own editor gets the whole terminal until it ends; one that is not installed is said in the status line instead: *nano was not found: install it, or name your editor in Settings → Behaviour → Editor* | Starts `editor` from the config with the file, in the background; without `editor`, opens the file in its default application, as **Enter** does |
| **F4** in [Find](../search/find-file.md) | Edits the hit under the cursor; the search stays open | Same |
| **F3** in Find | Views the hit under the cursor | — (**Enter** goes to the hit; the preview shows it there) |

A variable that is set but empty counts as unset.

## The built-in viewer

The terminal app's **F3** shows the file full screen over the panels: highlighted in the
theme's colours (the same colour for each kind of token as the desktop app's preview), with
line numbers at the left, and a title that names the file, its language, its encoding and its
number of lines. The line under the text says where you are (*Line 120 · 34%*), or what a key
said; the key line at the bottom lists the keys.

| Key | Does |
|---|---|
| **↑** **↓**, **Enter**, mouse wheel | One line up or down (the wheel three) |
| **PgUp** **PgDn**, **Space** | A screen up or down |
| **Home** **End** | The first line, the last screen |
| **←** **→** | Sideways, eight columns, when lines are not wrapped |
| **/** or **F7** | Search: type the text, **Enter**. Case does not count; each hit on the screen is marked, and the line number of the one found too |
| **n** **N** (**Shift+F7**) | The next hit, the one before |
| **g** | Go to a line: type its number, **Enter** |
| **l** | Line numbers on and off |
| **w** | Wrap long lines on and off |
| **p** | Hand the file to your pager: `$PAGER`, else `less` (`more` on Windows) |
| **h** | On a facts card (an old picture, a module, a [local model](../files/local-models.md#in-the-terminal-app), a neural network model): the file itself, a hex dump for a binary one; **Esc** comes back to the card |
| **F4** | Edit the file |
| **Esc**, **F3**, **F10** | Close; the panels come back. While a search is still going, **Esc** stops it instead |

What it reads:

- **Any size.** A thread finds where each line starts and only the lines on the screen are read
  from the disk, so a 50 MB log opens at once (the title says *lines so far* until the end is
  reached). Search reads ahead a few thousand lines at a time, between redraws: on a big file
  the status line says *Searching for error: 40% · Esc stops*, the keys still work, and **Esc**
  stops it. A search started while the title still says *lines so far* goes on over the lines
  as they are found, rather than stopping at those found then. Over 16 MB the text shows
  without colours (*Over 16 MB: shown without colours.*), and a line over 4,000 characters (a
  minified file) is shown plain.
- **Encodings.** UTF-8, UTF-16 with its byte order mark, and Windows-1252 when the start of
  the file is not UTF-8: the same as the desktop app's preview. The title says which
  (*UTF-8*, *UTF-16*, *Windows-1252*, *hex*). A UTF-16 file over 16 MB shows its first 16 MB
  (*Showing the first 16 MB.*).
- **Very long lines and very many.** A line is read to 1 MB at most and ends in `…` there (a
  minified file, a file without newlines). The first 20 million lines are found; the title then
  says *the first 20000000 lines*.
- **Only files.** A pipe or a device is not opened (it could block): the status line says
  *Not a regular file (a pipe or a device): the viewer reads files only.*
- **A log that grows.** **End** looks again: lines written since it opened are found, and a log
  that was rotated (shorter now) is read again from its start.
- **Binary files.** The desktop app's hex dump of the first 64 KB: offset, 16 bytes in hex, the
  printable characters.
- **Languages.** The grammar comes from the table the desktop app uses
  ([Languages](../previews/text-and-code.md#languages)), from the grammars of syntect and
  two-face (about 200). COBOL, BASIC and Visual Basic, PowerShell, Prolog, Coq,
  Smalltalk, AutoHotkey and ARM or MIPS assembly have none there and show plain.

Control characters in a file (a log's colour codes) show as `·`, so nothing in a file can
send commands to your terminal. **F1** in the viewer opens F1 → Features at *View and edit*.

## What you see

- **Terminal app, F3:** the [built-in viewer](#the-built-in-viewer) covers the panels until
  **Esc**.
- **Terminal app, F4** (and **F3** with `viewer` set): the panels disappear and the program has
  the whole terminal. When you quit it (`:q` in `vi`, **q** in `less`), the panels come back and
  are read again, so changes you saved show at once. The program runs in the file's folder.
- **Desktop app, F3:** the preview pane opens at the right, or closes. What it shows depends on
  the file: text with syntax colours, PDF pages, pictures, tables, trees ([The preview pane](../previews/README.md)).
- **Desktop app, F4:** your editor opens in its own window. Coxswain does not wait for it: the
  panels stay usable. When the editor cannot be found, nothing happens.

## Settings and config.toml

Both are top-level keys in `config.toml` ([Configuration](../reference/configuration.md)), and both are
in Settings → *Behaviour* of both apps (**Ctrl+,**; **F12** in the terminal app): *Editor* and *Viewer (terminal app)*
([Behaviour](../customise/settings.md#behaviour)).

| Key | Type | Default | Used by |
|---|---|---|---|
| `editor` | text: a command, may have arguments | not set | **F4** in both apps |
| `viewer` | text: a command, may have arguments | not set: the built-in viewer | **F3** in the terminal app |
| `[keys]` `view` | list of keys | `["F3"]` | |
| `[keys]` `edit` | list of keys | `["F4"]` | |

```toml
editor = "hx"
viewer = "bat --paging=always"
```

The command is run with the shell, with the file's full path added at the end, quoted when it
needs it: `hx '/home/demo/My notes.txt'`. So arguments work (`editor = "code --wait"`), and so do
`~` and variables in the command.

## In the terminal app

**F3** is the built-in viewer and **F4** the editor, run in the terminal: see the table above.
The desktop app has no terminal to lend, which is why its **F3** is the preview pane and its
**F4** wants a windowed editor. Both apps read the same `editor` key.

Two exceptions: **F3** on a CycloneDX BOM opens the terminal app's own
[BOM viewer](../previews/bom.md), and **F3** on build provenance its
[provenance viewer](../previews/provenance.md). **F3** (or `s`) in either shows the source in
the built-in viewer (or in `viewer`'s program), and **Esc** there goes back to the BOM or the
provenance; `bom_viewer = false` or `provenance_viewer = false` makes F3 show the source straight
away. **F3** on a chess, Go or draughts file (`.pgn`, `.pgn.gz`, `.pgn.zst`, `.epd`, `.fen`, `.sgf`,
`.pdn`, or a `.bin` Polyglot book) opens its games, positions or book moves full screen
([Chess games](../previews/chess.md), [Go games](../previews/go.md),
[Draughts games](../previews/draughts.md), [Xiangqi games](../previews/xiangqi.md)), and on a neural network
model (`.onnx`, `.tflite`, `.mlmodel`, `.keras`, a Keras `.h5`) its facts card: format, inputs
and outputs with shapes, operators ([Neural network models](../previews/neural-networks.md)).
**F3** on a MOBI, Kindle, FictionBook, LIT or Palm book shows its title, authors and text, made by
calibre, when *E-books with calibre* is on ([E-books](../previews/ebooks.md#in-the-terminal-app)).

## Questions

#### F4 in the desktop app does nothing, though `editor = "hx"` works in the terminal app.

The desktop app starts the editor without a terminal, so a terminal editor such as `hx`, `vim`
or `nano` has nowhere to show. `editor` is shared by both apps. Either set it to a windowed
editor (`editor = "code"`, `editor = "gnome-text-editor"`), or leave it out of the config and
set `$VISUAL` or `$EDITOR` in your shell for the terminal app: the desktop app then opens files
in their default application.

#### Can I use a terminal editor from the desktop app anyway?

Yes, by starting a terminal with it: `editor = "alacritty -e hx"`, `editor = "kitty hx"` or
`editor = "gnome-terminal -- hx"`. The terminal app would then also open a new terminal window,
so set `$EDITOR` for it and leave `editor` for this.

#### How do I read a file in the desktop app without the preview?

**Enter** opens it in its default application ([Opening files](opening-files.md)). **F4**
without an `editor` does the same.

#### Why does F3 in the desktop app not start my viewer?

The desktop app's **F3** is the preview pane, as Norton Commander's F3 was its built-in viewer;
the preview reads far more formats than a pager. `viewer` is for the terminal app only. To run
a viewer from the desktop app, add it to the [user menu](user-menu.md): `command = "bat %f"`.

#### Which editor and which viewer does the terminal app use?

For **F4**, the first that is set: `editor` in `config.toml`, then `$VISUAL`, then `$EDITOR`;
with none of them, the [built-in editor](editor.md). One that is set but not there is named in the
status line, *… was not found: install it, or name your editor in Settings → Behaviour → Editor*,
and nothing runs. **F3** uses the built-in viewer unless `viewer` names a
program; **p** in the viewer hands the file to `$PAGER`, else `less` (`more` on Windows).

#### How do I get `less` (or my own pager) back on F3?

Set `viewer = "less"` (or `viewer = "bat --paging=always"`) in `config.toml`, or in Settings →
*Behaviour* → *Viewer (terminal app)*: **F3** then runs it as before. For one file, press **p**
in the built-in viewer.

#### Why are a log's colours gone in the viewer?

The viewer shows the colour codes in a file as `·` rather than passing them to the terminal,
which would let a file send commands to it. `less -R` shows them: press **p** (with
`PAGER="less -R"`), or set `viewer = "less -R"`.

#### Does the viewer read a huge log into memory?

No. It keeps where each line starts (8 bytes a line: 5 MB for a 50 MB log of 600,000 lines)
and reads only the lines on the screen. Opening such a log takes a few milliseconds, the end of
it is there within a tenth of a second, and a search through all of it about 30 ms. A search
through many gigabytes goes on while the screen is drawn, its share done on the status line,
and **Esc** stops it.

#### F3 and F4 do nothing on a folder.

They work on files only. To look into a folder, press **Enter**; its summary, size and
[note](../organise/notes.md) show in the desktop app's preview pane.

#### Why did empty black windows pop up in the desktop app on Windows?

Versions before 2.20.1 ran git, converters and the commands behind the previews the way Windows
runs a console program from an app without a console: each got a window of its own, empty since
its output went to Coxswain. Nothing was wrong with your folders or your setup. From 2.20.1 the
desktop app starts them without a window; the terminal app has a console and its programs share
it, so an editor it opens still shows.

#### Can I edit a file inside an archive with F4?

No: the editor would change a copy, and the change would be lost. Copy it out with **F5** first
([Archives as folders](../files/archives.md)). **F3** in the terminal app views it (a copy goes
to the viewer), and the desktop app's preview pane shows it.

#### F4 on a hit in Find: does the search close?

No. The editor starts for the hit under the cursor and the search stays as it was, so you can
edit several hits in a row. **Enter** goes to the hit's folder instead.

---
[← Previous: Scripts](scripts.md) · [Next: The built-in editor, Shift+F4 →](editor.md)

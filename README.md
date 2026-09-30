# Coxswain

*The one at the helm, who steers the boat and keeps the crew in time.*

Coxswain is a two-panel file manager in the Norton Commander tradition, built for developers. It
comes as a terminal app (Rust + Ratatui) and a desktop app (Tauri + Svelte 5). Both run on one
shared Rust core and read the same config file.

![The desktop app in Cyber, its default theme: a green phosphor terminal with two panes, git status, color tags and the F-key bar](docs/screenshots/gui-details.png)
*The desktop app in **Cyber**, its default: a green phosphor terminal, glow and scanlines
included. Seventeen more themes are one F9 away, from Norton Commander blue to Windows 95 and
Mac OS 9; see [themes](#themes).*

- **Norton Commander at heart.** Blue panels in the terminal, F-key bar, command line, and
  NC's keys by default.
- **Eighteen themes with the looks of their era.** Cyber by default in the desktop app;
  Windows 3.11 to 11 and Mac System 7 to today come with their corners, bevels and fonts.
- **Git in every panel.** oh-my-posh style branch, ahead/behind, staged/modified/untracked
  counts and stashes, plus a glyph per file (Nerd Font, or ASCII).
- **Search like Everything.** Every file name on the machine sits in RAM and queries run in
  parallel: about 1.4M files index in ~0.2 s and answer in under 10 ms. Every window and the
  terminal app share one index.
- **Search inside your files.** Tab in Find file switches from names to the text of your
  files, with the passage that matched under each hit: text and code, PDF, Word, spreadsheets,
  presentations, mail, books and notebooks. It stays on your machine.
- **See before you open.** The desktop app previews over 60 file types: code, Markdown with
  Mermaid diagrams and math, Jupyter notebooks, Word, spreadsheets, PDFs, SQLite databases,
  JSON/YAML/TOML as trees, Parquet, certificates, e-mail, calendars, EPUB books, fonts,
  images, video and audio, plus the git diff of any changed file.
- **Builds what needs building.** LaTeX to PDF, Office and Visio files through LibreOffice,
  PlantUML, Graphviz, AsciiDoc and reStructuredText, with an installed tool or in a podman or
  docker container, so rarely used tools need not be installed. See
  [the full list](docs/previews.md).
- **Folder sizes, without asking.** In your home folder they are there at once: the
  [search helper](#how-search-stays-fast) already knows every file's size. Other folders are
  measured in the background and filled in as they come, on two threads so the machine stays
  yours.
- **Plays well with the desktop.** Delete goes to the trash, folders refresh themselves, files
  drag to and from other apps, and Ctrl+C / Ctrl+V share files with your other file manager.
- **Finds duplicates.** Ctrl+D compares folders and whole disks, old backups included, and
  finds duplicate files and folders by content, whatever they are called: sizes first, then
  BLAKE3 hashes, kept for next time. The helper hashes your home folder's look-alikes ahead.
  Mark the extra copies by rule and move them to the trash. See
  [finding duplicates](docs/duplicates.md).
- **Speaks your language.** 18 languages, from British, Australian, Canadian and New Zealand
  English to Danish, Finnish, the Baltic languages, Catalan, Basque and Hebrew (right to left).
  Coxswain picks your system's language, or the nearest one it has; Settings changes it.
- **Configurable.** Key bindings, color schemes, glyphs, user menu and fonts all live in one
  TOML file.

![The terminal app: two panels, git status on the left](docs/screenshots/tui-panels.png)

| | |
|---|---|
| ![Find file searches every file on the machine](docs/screenshots/tui-search.png) | ![Nine of the themes: Windows 3.11, 95, XP, 7 and 11, Mac System 7, Mac OS 9, Aqua and macOS](docs/screenshots/gui-themes.png) |
| Find file: every file on the machine, in milliseconds | Themes with the look of their era |
| ![Thumbnails and an image preview](docs/screenshots/gui-thumbnails.png) | ![A PDF in the preview pane, next to a git repository](docs/screenshots/gui-pdf.png) |
| Thumbnails (Alt+V) with the preview pane (Space) | PDFs preview in place; the left pane shows git status |
| ![The contents of a tar.gz archive in the preview](docs/screenshots/gui-archive.png) | ![Miller columns with a source file in the preview](docs/screenshots/gui-columns.png) |
| Archives list their contents; Ctrl+E extracts them | Miller columns with the preview pane |

![The preview pane showing Markdown with a Mermaid diagram and math, a Jupyter notebook, a spreadsheet, a Word document and a font](docs/screenshots/gui-previews.png)
*The preview pane (Space): Markdown with a Mermaid diagram and math, a Jupyter notebook, a
spreadsheet, a Word document and a font.*

![The preview pane showing a YAML tree, a SQLite database, a certificate and an e-mail](docs/screenshots/gui-previews-data.png)
*YAML as a tree, a SQLite database's tables, a certificate about to expire, and an e-mail.*

![The preview pane showing a calendar, a git diff, a log file and an EPUB book](docs/screenshots/gui-previews-more.png)
*A calendar, the git diff of a changed file, a log colored by level, and an EPUB book.*

![A LaTeX document with a pgfplots chart, built with tectonic and shown in the preview](docs/screenshots/gui-latex.png)
*A LaTeX document, built with tectonic; the buttons pick the engine, an installed program or a
container.
Multi-file projects build from any of their files: Coxswain finds the main document
(`% !TEX root`), the engine (`% !TEX program`) and the project folder, like a LaTeX editor.*

![A PlantUML sequence diagram, a Graphviz graph and an AsciiDoc guide](docs/screenshots/gui-previews-tools.png)
*PlantUML (in a container), Graphviz and AsciiDoc (both built in).*

![The Duplicates window with a duplicated folder, a PDF downloaded twice, a photo in three places and a document in an old backup](docs/screenshots/gui-duplicates.png)
*Duplicates (Ctrl+D): a backed-up Pictures folder, a PDF downloaded twice, a photo in three
places and a document in an old backup.*

## Install

Coxswain comes as a **terminal app** (`coxswain`, or `cox` for short) and a **desktop app**
(`coxswain-gui`). Pick one or both. Both check once a day for a newer release and tell you the
exact update command for the way you installed them.

### macOS

| How | App | Install | Update |
|---|---|---|---|
| [Homebrew](https://brew.sh) | Terminal | `brew install mwo-dk/coxswain/coxswain` | `brew upgrade coxswain` |
| Homebrew | Desktop | `brew install --cask mwo-dk/coxswain/coxswain-gui` | `brew upgrade --cask coxswain-gui` |
| Download | Desktop | `Coxswain_<version>_aarch64.dmg` (Apple silicon) or `_x64.dmg` (Intel) from [Releases](https://github.com/mwo-dk/coxswain/releases/latest); drag Coxswain to Applications | Download the new `.dmg` and drag it over the old app |
| Download | Terminal | `coxswain-terminal-<version>-aarch64-apple-darwin.tar.gz` (or `x86_64-…`); put `coxswain` on your PATH | Replace the file |
| [Cargo](https://rustup.rs) | Terminal | `cargo install coxswain` | `cargo install coxswain` again |

### Linux

| How | App | Install | Update |
|---|---|---|---|
| [Homebrew](https://brew.sh) | Terminal | `brew install mwo-dk/coxswain/coxswain` | `brew upgrade coxswain` |
| Homebrew | Desktop (x86-64) | `brew install --cask mwo-dk/coxswain/coxswain-gui` | `brew upgrade --cask coxswain-gui` |
| Debian, Ubuntu | Desktop | `sudo apt install ./Coxswain_<version>_amd64.deb` | The same with the new `.deb` |
| Fedora, openSUSE | Desktop | `sudo dnf install ./Coxswain-<version>-1.x86_64.rpm` | The same with the new `.rpm` |
| Any distro | Desktop | `Coxswain_<version>_amd64.AppImage`: `chmod +x` and run it | Replace the file |
| Any distro | Terminal | `coxswain-terminal-<version>-x86_64-unknown-linux-musl.tar.gz` (or `aarch64-…`), a static binary; put `coxswain` on your PATH | Replace the file |
| [Cargo](https://rustup.rs) | Terminal | `cargo install coxswain` | `cargo install coxswain` again |

### Windows

| How | App | Install | Update |
|---|---|---|---|
| Download | Desktop | `Coxswain_<version>_x64_en-US.msi` or `_x64-setup.exe` from [Releases](https://github.com/mwo-dk/coxswain/releases/latest) | Run the new installer; it upgrades in place |
| Download | Terminal | `coxswain-terminal-<version>-x86_64-pc-windows-msvc.zip`; put `coxswain.exe` on your PATH | Replace the file |
| [Cargo](https://rustup.rs) | Terminal | `cargo install coxswain` | `cargo install coxswain` again |

**The builds are not code-signed.** The first start shows a warning: on Windows, click *More
info* and then *Run anyway*. On macOS, if the app "is damaged" or "can't be opened", run
`xattr -cr /Applications/Coxswain.app` once; the Homebrew cask does this for you.

### From source (any platform)

```sh
git clone https://github.com/mwo-dk/coxswain.git
cd coxswain
./install/install.sh                                          # Linux, macOS
powershell -ExecutionPolicy Bypass -File install\install.ps1  # Windows
```

To update, `git pull` in that folder and run the script again. The script offers to install
Rust and Node.js when they are missing, and asks first. See
[install/INSTALL.md](install/INSTALL.md). For development: `cargo run -p coxswain`, or
`cd gui && npm ci && npx tauri dev`.

### Fonts

Git glyphs need a [Nerd Font](https://www.nerdfonts.com/). In the terminal, use one as your
terminal font; the GUI picks up any installed Nerd Font listed in `gui.icon_font`. Without
one, set `glyphs = "ascii"`.

## Keys (defaults)

| Key | Action | Key | Action |
|---|---|---|---|
| F1 | Help | Tab | Other panel |
| F2 | User menu | Insert / Shift+Down | Mark |
| F3 | View | + / - / * | Select / unselect group, invert |
| F4 | Edit | Alt+F7 / Ctrl+F | Find file |
| F5 | Copy | Ctrl+R | Reread |
| F6 | Rename/move | Ctrl+U | Swap panels |
| F7 | Mkdir | Ctrl+O | Show command output |
| F8 / Delete | Move to trash | Alt+F1 / Alt+F2 | Left/right panel: go to |
| Shift+F8 / Shift+Delete | Delete permanently | | |
| F9 | Command palette | Ctrl+F3..F6 | Sort by name/ext/time/size (again = reverse) |
| F10 | Quit | Alt+letter | Quick search |

Typing goes to the command line. Enter runs it in the panel's directory, and `cd` works. Both
apps support the mouse: click, double-click, right-click to mark, and the wheel. The GUI also
does Shift-click ranges, drag-and-drop between panels, and clickable column headers and paths.

## Using Coxswain

Start either app with up to two folders: `coxswain ~/src ~/Downloads` or `coxswain-gui .`. A file
opens its folder with the cursor on it, so `coxswain-gui ~/Pictures/cat.jpg` shows that picture.
Without arguments both panels open in the current folder (the desktop app restores your last
session).

**The NC way.** One panel is active. Move with the arrows, Enter opens a folder or file, and
Backspace goes up. Mark files with Insert (or `+` with a pattern like `*.rs`), then F5 copies
or F6 moves them to the *other* panel's folder. With nothing marked, the file under the cursor
is used. F3 views, F4 opens your `$EDITOR`, F8 moves to the trash (after asking), and Shift+F8
deletes for good. F9 opens a searchable list of every command, so you never need to remember a
key.

**Reading the git line.** Inside a repository the panel's bottom line shows the branch,
commits ahead/behind the upstream, and counts of staged, modified and untracked files and
stashes. Each file and folder gets the same glyph, so a changed file deep
in `src/` marks `src` too. Ignored files get a crossed-out eye.

**Find file** (Alt+F7 or Ctrl+F). Start typing; results update per key. Enter jumps to the
file with the cursor on it, F3 and F4 view and edit it in place, and Tab limits the search to
the current folder. The first start builds the index in the background; after that it is
loaded from disk and kept current while Coxswain runs.

**Desktop app extras** (all rebindable, like everything else):

| Key | Action | Key | Action |
|---|---|---|---|
| Ctrl+T / Ctrl+W | New / close tab | Space | Preview pane (see below) |
| Ctrl+Tab | Next tab | Alt+V | Details, Miller columns or thumbnails |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, paste files (shared with other file managers) | Alt+Enter | Properties and permissions |
| Alt+Left / Alt+Right | Back / forward | Ctrl+E | Extract a zip or tar archive to the other pane |
| Ctrl+B | Sidebar | Ctrl+D | Find duplicates |
| Ctrl+, | Settings (language, theme, fonts, ...) | Ctrl+L | Type a path |
| Ctrl+M | Batch rename with regex, previewed | Alt+T | Color tag |
| Alt+N | Notes for this folder | Alt+. | Hidden files |

**The preview pane** (Space) follows the cursor. At a glance:

| Files | Preview |
|---|---|
| Code, config, logs | Highlighted; logs colored by level. A changed file gets **File / Diff** |
| Markdown, `.mmd` | Rendered, with Mermaid diagrams and math; **Rendered / Source** |
| JSON, YAML, TOML | A collapsible tree; **Tree / Source** |
| `.ipynb`, `.docx`, `.epub`, `.eml`, PDF | Notebook with outputs, Word document, first chapter, e-mail, pages |
| Spreadsheets, CSV, JSON Lines, SQLite | Tables, a button per sheet; database tables with row counts and schema |
| `.ics`, `.vcf`, `.plist`, certificates | Events, contact cards, property lists, certificate details with expiry |
| Images, video, audio, fonts | Shown or played; photo EXIF, audio tags, font samples |
| LaTeX, Office/Visio/RTF, PlantUML, draw.io, `.rst` | Built to PDF, SVG or HTML by an installed tool or a container; buttons pick the engine |
| Graphviz, AsciiDoc, Parquet, `.duckdb` | Graphs, rendered documents, tables with schema |
| Archives, programs, folders | Contents; the platform a binary is built for; folder counts, sizes, notes |

**[docs/previews.md](docs/previews.md) lists every format, what it shows and how it works.**
Where there is a choice, buttons at the top of the preview pick it, and the choice sticks.
Ctrl+O, or the button next to the view switcher, toggles one or two panes.

**Duplicates.** Ctrl+D (or `coxswain-gui --duplicates <folders>`) scans the folders and drives
you tick for duplicate files and whole duplicate folders, and lists them by wasted space.
"Mark all but the newest / oldest / the one under a folder" marks the extra copies; at least
one copy of each always stays, and marked copies go to the trash.
**[docs/duplicates.md](docs/duplicates.md)** explains how it works and stays fast.

**Columns and folder sizes.** Folders show their size without being asked: each is measured
in the background as you open its parent, in both apps. Right-click the column header (or F9,
"Columns and folder sizes") to add **Files** and **Created** columns, hide Type, or switch the
measuring off.

![The details view with every column and automatically measured folder sizes](docs/screenshots/gui-folder-sizes.png)

Folders reread themselves when something changes in them. Drag files to the other pane, to
another application, or in from one; Coxswain asks whether to copy or move.

The sidebar holds places, drives with free space, favorite groups (right-click a group, then
"Add current folder") and the git repositories you visited recently.

### Themes

Pick a theme from the F9 command list (type "theme") or in Settings. Besides Cyber, a green
phosphor terminal and the desktop default, and Norton Commander blue, the terminal default,
there are modern ones (dark, light, Nord, Tokyo Night) and period looks from Windows 3.11 to 11
and Mac System 7 to today, each with the corners, bevels and fonts of its era.

![Windows 3.11, 95, XP, 7 and 11, Mac System 7, Mac OS 9, Aqua and macOS](docs/screenshots/gui-themes.png)

## Languages

<img src="docs/flags/gb.svg" width="20" alt="British English"> <img src="docs/flags/au.svg" width="20" alt="Australian English">
<img src="docs/flags/ca.svg" width="20" alt="Canadian English"> <img src="docs/flags/nz.svg" width="20" alt="New Zealand English">
<img src="docs/flags/dk.svg" width="20" alt="Danish"> <img src="docs/flags/se.svg" width="20" alt="Swedish">
<img src="docs/flags/fi.svg" width="20" alt="Finnish"> <img src="docs/flags/ee.svg" width="20" alt="Estonian">
<img src="docs/flags/lv.svg" width="20" alt="Latvian"> <img src="docs/flags/lt.svg" width="20" alt="Lithuanian">
<img src="docs/flags/de.svg" width="20" alt="German"> <img src="docs/flags/fr.svg" width="20" alt="French">
<img src="docs/flags/it.svg" width="20" alt="Italian"> <img src="docs/flags/nl.svg" width="20" alt="Dutch">
<img src="docs/flags/ar.svg" width="20" alt="Argentinian Spanish"> <img src="docs/flags/es-ct.svg" width="20" alt="Catalan">
<img src="docs/flags/es-pv.svg" width="20" alt="Basque"> <img src="docs/flags/il.svg" width="20" alt="Hebrew">

Both apps speak **English** (British, Australian, Canadian, New Zealand), **Dansk**, **Svenska**,
**Suomi**, **Eesti**, **Latviešu**, **Lietuvių**, **Deutsch**, **Français**, **Italiano**,
**Nederlands**, **Español (Argentina)**, **Català**, **Euskara** and **עברית** (Hebrew, laid out
right to left in the desktop app).

- **Automatic by default.** Coxswain uses your system's language, or the nearest one it has:
  Norwegian gets Danish, any Spanish gets Argentinian Spanish, US English gets Canadian, any other
  English gets British.
- **Your choice, remembered.** Pick a language in **Settings** (Ctrl+,), where each is listed by
  its own name and flag, or set `language = "da"` in the config. It is saved in `config.toml`
  and used by both apps.
- **All of it:** menus, the F-key bar, dialogs, previews, the duplicate finder, messages, sizes
  (`Ko` in French) and numbers (1.234,5 in German).

![Coxswain in Danish, previewing a picture](docs/screenshots/gui-lang-da.png)
*Danish: the F-key bar, sidebar, columns and preview all in Danish.*

![Coxswain in Hebrew, laid out right to left, previewing a PDF](docs/screenshots/gui-lang-he.png)
*Hebrew: the whole window is mirrored; file names, sizes and paths stay left to right.*

**[docs/languages.md](docs/languages.md)** has the full list, how the language is chosen, and
how to correct or add a translation.

## Settings

**Ctrl+,** (or F9 → *Settings*, or `coxswain-gui --settings[=search]` from a terminal) opens the desktop
app's settings:

![The Settings window: languages with their flags, appearance, behaviour and previews](docs/screenshots/gui-settings.png)

| Section | Settings |
|---|---|
| Language | Automatic, or any of the 18, each with its flag |
| Appearance | Theme, Nerd Font glyphs or plain ASCII, text size, fonts |
| Behaviour | Show hidden files, ask before deleting, check for updates |
| Previews made by tools | Installed programs or containers, podman or docker, LaTeX image, timeout; each container image with its size, and Pull (which also updates it) and Remove |

Every change applies at once and is written to `config.toml`, **keeping your comments and
layout**; a change that would make the file invalid is refused rather than saved. The terminal
app reads the same file. Everything else (keys, colour themes, the user menu) is set in
`config.toml` directly; see [Configuration](#configuration).

## Find file

**Alt+F7** or **Ctrl+F** opens it in both apps. Results update as you type.

| Key | Action |
|---|---|
| Up / Down, PageUp / PageDown | Move through the results |
| Enter | Go to the file, with the cursor on it |
| F3 / F4 | View / edit the file without leaving the search |
| Tab | Names everywhere, names in the current folder, or the **text inside your files** |
| Esc | Close |

**Alt+letter** is the other, smaller search: it jumps to the first name in the current panel
starting with that letter. Keep typing to narrow it; Backspace takes a letter back, Esc ends it.

The desktop app lists the first 500 hits and counts the rest; type more to narrow them down.

### Search inside your files

Press **Tab** twice in Find file and type words: Coxswain finds the files whose text has all
of them, best match first, each with the passage that matched. The last word may be the start
of one, so `rocket bud` finds "rocket budget".

![Find file searching the text of files: seven hits for "engine", each with the passage that matched](docs/screenshots/gui-text-search.png)

| | |
|---|---|
| **What is read** | Your home folder: text, code, Markdown, logs, configuration and any other file that is plain text, and the [documents](#documents-it-reads) below, up to 20 MB each |
| **What is left out** | Hidden folders, `node_modules`, `target`, `build`, `dist`, `out`, `vendor`, `__pycache__` and the trash; folders you mark *names only* in Settings; any folder holding a file named `.nosearch`; pictures, video, archives and other files without text |
| **When** | In the background, one file at a time and at half speed, by the [helper](#how-search-stays-fast), and not at all while a laptop runs on its battery (*Index now* in Settings reads anyway). It follows the file watcher, so a change shows up in searches within seconds |
| **Also kept** | Every file's size and date, the total of each folder left out, so folder sizes are a sum; and the hash of files that share a size, for Duplicates |
| **Where it is kept** | `search.db` in Coxswain's cache folder, readable by you alone. Nothing leaves your machine. Delete the file to start afresh |
| **Switching it off** | Settings → *Search inside files*, or `text = false` under `[search]` in `config.toml` |

**Settings → Search inside files** (`coxswain-gui --settings=search`) shows how many files can
be searched, how many are still to be read and how much room the index takes. *Index now* reads
the backlog at full speed; *Delete the index* empties it, and it fills again from the start.
There you also pick the folders that are read (your home folder when none are set) and the
folders kept to *names only*: found by name and counted in folder sizes, never read.

![Settings, Search inside files: 31 files searchable, Index now and Delete the index, the folders read and the names-only folders](docs/screenshots/gui-settings-search.png)

**Removable disks** can be folders read too. Each is known by its disk (file system UUID, or
the volume serial on Windows), not only by its path: while the disk is not plugged in its text
stays in the index, out of search results, and Settings shows it as away; plugged in again, at
the same place or another, it is searched again without being read afresh. *Remove* forgets it.

Other excludes and another size limit are `text_exclude` and `text_max_size` under `[search]`;
the folders are `text_roots` and `names_only`.

#### Documents it reads

| Kind | Files |
|---|---|
| PDF | `.pdf`; a very long manual is read for three seconds, which is most of it |
| Word and OpenDocument text | `.docx` `.docm` `.dotx` `.odt` `.ott`, with headers, footers, footnotes and comments |
| Rich text | `.rtf` |
| Spreadsheets | `.xlsx` `.xlsm` `.xlsb` `.xls` `.ods`: every sheet, the values and not the formulas |
| Presentations | `.pptx` `.ppsx` `.potx` `.odp`, with the speaker notes |
| Mail | `.eml` and `.mbox`: subject, sender, receivers, the message and the names of its attachments |
| Books and web pages | `.epub` `.html` `.htm` `.xhtml` |
| Notebooks and diagrams | Jupyter `.ipynb` with what the cells printed, draw.io `.drawio` `.dio` |

Coxswain reads them itself: no other program is started and nothing is installed. A scanned
page is a picture and has no text to find, and a file locked with a password is not read.
The first start after an update reads your files again, since the documents are new to it.

The terminal app does the same, with the passage on a second line:

![The terminal app searching the text of files](docs/screenshots/tui-text-search.png)

Everything's syntax:

| Query | Matches |
|---|---|
| `foo bar` | names containing both |
| `foo\|bar` | either |
| `!foo` | not foo |
| `*.rs`, `a?c` | wildcards, whole name |
| `ext:rs;toml` | by extension |
| `file:` / `folder:` | only files / only folders |
| `src/ lib` | a term with `/` matches the full path |
| `case:` | case-sensitive |
| `"a b"` | phrase with a space |

## Configuration

`coxswain --config-path` shows where the file lives (`~/.config/coxswain/config.toml` on Linux).
`coxswain --dump-config` prints every option with its default. Set only what you want to change:

```toml
language = "auto"           # or "en-GB", "da", "de", "es-AR", "he", ... (docs/languages.md)
theme = "nc"                # terminal app; or "cyber", "win95", ... or your own [themes.<name>]
glyphs = "nerd"             # or "ascii"
folder_sizes = true         # measure folders in the background; false to switch it off
editor = "hx"               # else $VISUAL / $EDITOR

[keys]
quit = ["F10", "Ctrl+Q"]    # listing an action replaces its default keys
search = ["Ctrl+P"]

[themes.mine.panel]         # unset slots fall back to the NC scheme
fg = "#e0e0e0"
bg = "#101820"

[[user_menu]]               # F2; %f file, %d dir, %s selection (shell-quoted)
key = "t"
label = "cargo test"
command = "cargo test"
wait = true

[search]
exclude = ["/proc", "/sys", "node_modules"]   # a path skips a tree; a bare name skips every such directory
watch = true

[preview]                   # previews made by tools: LaTeX, LibreOffice, PlantUML, ...
prefer = "container"        # use podman/docker even when a tool is installed ("auto", "local")
images.latex = "docker.io/texlive/texlive:latest-medium"   # see docs/previews.md

[gui]
font_size = 15
```

## How search stays fast

The index (`crates/coxswain-core/src/index.rs`) keeps each name once in a `\0`-separated byte
buffer, with a 12-byte node per entry (parent, offset, length, flags). A query runs a single
SIMD `memmem` scan over that buffer, split across all cores. Full paths are built only for
hits. The index is saved to the cache directory, so it loads in ~90 ms on the next start. It is
then rebuilt in the background and kept current by file system events (inotify, FSEvents,
ReadDirectoryChangesW).

**One index for every window.** The index lives in a helper process, which the first window
or terminal app starts and the others find. Two windows and a terminal share one copy in
memory and one scan of the disk, and a new window searches at once. The helper is the app
itself, started with `--index-helper`; it leaves ten minutes after the last app has closed.
Nothing is installed as a service. The apps talk to it over a local socket that only you can
use, and if it cannot be reached, each app indexes by itself as before.

Run `cargo run --release -p coxswain-core --example bench -- /` to measure it on your machine.

Known limits and upgrade paths:

- **Linux:** inotify needs one watch per directory. Past `fs.inotify.max_user_watches`, the
  hourly rebuild catches changes. fanotify would remove that limit, but it needs root.
- **Windows:** the first index comes from a directory walk. Reading the MFT and USN journal
  directly, as Everything does, would make cold starts faster.

## Layout

```
crates/coxswain-core   config, fs ops, git status, search index (shared)
crates/coxswain        terminal UI (package and binary: coxswain)
gui/                Svelte 5 frontend
gui/src-tauri       Tauri backend (binary: coxswain-gui)
```

## License

MIT

Norton Commander is a trademark of Gen Digital Inc. Coxswain is an independent project, not
affiliated with or endorsed by Gen Digital.

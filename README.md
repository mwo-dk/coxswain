# Coxswain

*The ship's officer who actually gets the work done.*

Coxswain is a two-panel file manager in the Norton Commander tradition, built for developers. It
comes as a terminal app (Rust + Ratatui) and a desktop app (Tauri + Svelte 5). Both run on one
shared Rust core and read the same config file.

- **Norton Commander at heart.** Blue panels, F-key bar, command line, and NC's keys by default.
- **Git in every panel.** oh-my-posh style branch, ahead/behind, staged/modified/untracked
  counts and stashes, plus a glyph per file (Nerd Font, or ASCII).
- **Search like Everything.** Every file name on the machine sits in RAM and queries run in
  parallel: about 1.4M files index in ~0.2 s and answer in under 10 ms.
- **See before you open.** The desktop app previews over 60 file types: code, Markdown with
  Mermaid diagrams and math, Jupyter notebooks, Word, spreadsheets, PDFs, SQLite databases,
  JSON/YAML/TOML as trees, Parquet, certificates, e-mail, calendars, EPUB books, fonts,
  images, video and audio, plus the git diff of any changed file.
- **Builds what needs building.** LaTeX to PDF, Office and Visio files through LibreOffice,
  PlantUML, Graphviz, AsciiDoc and reStructuredText, with an installed tool or in a podman or
  docker container, so rarely used tools need not be installed. See
  [the full list](docs/previews.md).
- **Plays well with the desktop.** Delete goes to the trash, folders refresh themselves, files
  drag to and from other apps, and Ctrl+C / Ctrl+V share files with your other file manager.
- **Configurable.** Key bindings, color schemes, glyphs, user menu and fonts all live in one
  TOML file.

![The terminal app: two panels, git status on the left](docs/screenshots/tui-panels.png)

| | |
|---|---|
| ![Find file searches every file on the machine](docs/screenshots/tui-search.png) | ![The desktop app with sidebar, tabs and tags](docs/screenshots/gui-details.png) |
| Find file: every file on the machine, in milliseconds | Desktop app: sidebar, tabs, color tags, git |
| ![Thumbnails and an image preview](docs/screenshots/gui-thumbnails.png) | ![A PDF in the preview pane, next to a git repository](docs/screenshots/gui-pdf.png) |
| Thumbnails (Alt+V) with the preview pane (Space) | PDFs preview in place; the left pane shows git status |
| ![The contents of a tar.gz archive in the preview](docs/screenshots/gui-archive.png) | ![Miller columns, light theme](docs/screenshots/gui-columns.png) |
| Archives list their contents; Ctrl+E extracts them | Miller columns and preview, light theme |

![The preview pane showing Markdown with a Mermaid diagram and math, a Jupyter notebook, a spreadsheet, a Word document and a font](docs/screenshots/gui-previews.png)
*The preview pane (Space): Markdown with a Mermaid diagram and math, a Jupyter notebook, a
spreadsheet, a Word document and a font.*

![The preview pane showing a YAML tree, a SQLite database, a certificate and an e-mail](docs/screenshots/gui-previews-data.png)
*YAML as a tree, a SQLite database's tables, a certificate about to expire, and an e-mail.*

![The preview pane showing a calendar, a git diff, a log file and an EPUB book](docs/screenshots/gui-previews-more.png)
*A calendar, the git diff of a changed file, a log colored by level, and an EPUB book.*

![A LaTeX document with a pgfplots chart and a TikZ diagram, built in a container and shown in the preview](docs/screenshots/gui-latex.png)
*A LaTeX document, built with the texlive image in podman; the buttons pick the engine.*

![A PlantUML sequence diagram, a Graphviz graph and an AsciiDoc guide](docs/screenshots/gui-previews-tools.png)
*PlantUML (in a container), Graphviz and AsciiDoc (both built in).*

## Install

Coxswain comes as a **terminal app** (`coxswain`) and a **desktop app** (`coxswain-gui`). Pick one or
both. Both check once a day for a newer release and tell you the exact update command for the
way you installed them.

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
| [winget](https://learn.microsoft.com/windows/package-manager/) | Desktop | `winget install mwo-dk.Coxswain` | `winget upgrade mwo-dk.Coxswain` |
| winget | Terminal | `winget install mwo-dk.Coxswain.Terminal` | `winget upgrade mwo-dk.Coxswain.Terminal` |
| [Scoop](https://scoop.sh) | Terminal | `scoop bucket add coxswain https://github.com/mwo-dk/scoop-coxswain`, then `scoop install coxswain` | `scoop update coxswain` |
| Download | Desktop | `Coxswain_<version>_x64_en-US.msi` or `_x64-setup.exe` from [Releases](https://github.com/mwo-dk/coxswain/releases/latest) | Run the new installer; it upgrades in place |
| Download | Terminal | `coxswain-terminal-<version>-x86_64-pc-windows-msvc.zip`; put `coxswain.exe` on your PATH | Replace the file |
| [Cargo](https://rustup.rs) | Terminal | `cargo install coxswain` | `cargo install coxswain` again |

winget listing is new: if `winget install` does not find Coxswain yet, Microsoft is still
reviewing it; use one of the other ways meanwhile. `brew upgrade` (no name) and
`winget upgrade --all` update Coxswain along with everything else.

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
| Ctrl+B | Sidebar | | |
| Ctrl+L | Type a path | Ctrl+Space | Folder sizes |
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

**Columns and folder sizes.** Right-click the column header (or F9, "Columns and folder
sizes") to add **Files** and **Created** columns or hide Type, and to turn on **automatic folder
sizes**: every folder is measured as you open its parent, in the background.

![The details view with every column and automatically measured folder sizes](docs/screenshots/gui-folder-sizes.png)

Folders reread themselves when something changes in them. Drag files to the other pane, to
another application, or in from one; Coxswain asks whether to copy or move.

The sidebar holds places, drives with free space, favorite groups (right-click a group, then
"Add current folder") and the git repositories you visited recently. Pick a theme from the F9 command list (type "theme").

## Find file

**Alt+F7** or **Ctrl+F** opens it in both apps. Results update as you type.

| Key | Action |
|---|---|
| Up / Down, PageUp / PageDown | Move through the results |
| Enter | Go to the file, with the cursor on it |
| F3 / F4 | View / edit the file without leaving the search |
| Tab | Search everywhere, or only the current folder |
| Esc | Close |

**Alt+letter** is the other, smaller search: it jumps to the first name in the current panel
starting with that letter. Keep typing to narrow it; Backspace takes a letter back, Esc ends it.

The desktop app lists the first 500 hits and counts the rest; type more to narrow them down.

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
theme = "midnight"          # or "nc", or your own [themes.<name>]
glyphs = "nerd"             # or "ascii"
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

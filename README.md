# Coxswain

*The one at the helm, who steers the boat and keeps the crew in time.*

Coxswain is a two-panel file manager in the Norton Commander tradition, built for developers.
It comes as a terminal app (`coxswain`, Rust and Ratatui) and a desktop app (`coxswain-gui`,
Tauri and Svelte 5), both on one shared Rust core and one config file. It finds any file by
its name, its text or what it is about, previews over 60 kinds of file, opens archives like
folders, shows git in every panel, and keeps everything on your machine.

![The desktop app in Cyber, its default theme: a green phosphor terminal with two panes, git status, colour tags and the F-key bar](docs/screenshots/gui-details.png)
*The desktop app in **Cyber**, its default. Seventeen more themes are one F9 away, from Norton
Commander blue to Windows 95 and Mac OS 9.*

## Highlights

- **Norton Commander at heart:** two panels, the F-key bar, a command line and NC's keys. [Panels and keys](docs/panels/README.md)
- **Find by name, text or meaning:** Ctrl+F, and Tab goes deeper, from every name on the machine to what a file is about, inside your zip, 7z and tar archives too. [Search](docs/search/README.md) · [Inside archives](docs/search/archives.md)
- **Ask your files:** a question in your own words, answered by your own chat model from the closest passages, with numbered sources. [Ask](docs/search/ask.md)
- **Search by meaning, in any language:** a small model on your machine, or your own Ollama, Lemonade or OpenAI-style server. [Search by meaning](docs/search/meaning.md)
- **See before you open:** code, Markdown, PDF, Word, PowerPoint, spreadsheets, SQLite, HTML, fonts, video and more. [The preview pane](docs/previews/README.md)
- **Builds what needs building:** LaTeX, Office, PlantUML and Graphviz previews, with your tools or a sealed container. [LaTeX](docs/previews/latex.md) · [Tools](docs/previews/tools.md)
- **Reads cryptography bills of materials:** a CycloneDX CBOM as a rated tree or sunburst, compared with last month's scan, in both apps. [CBOMs](docs/previews/bom.md)
- **Archives are folders:** zip, 7z and tar (gz, bz2, xz, zst): go in, preview, copy, move, rename, pack, with passwords to open them and to lock new zips and 7z (AES-256). [Archives](docs/files/archives.md)
- **Git in every panel:** branch, ahead and behind, counts, a glyph per file, and who last committed it and when. [Git](docs/panels/git.md)
- **History as folders:** **Ctrl+G** on a file or folder lists its commits; Enter on one browses the files as they were, F5 copies an old version out, and Find file finds commit messages. [Git history](docs/panels/git-history.md) · [History in search](docs/search/history.md)
- **Folder sizes without asking,** instant in your home folder. [Folder sizes](docs/panels/folder-sizes.md)
- **Finds duplicates** by content across folders and disks, and marks the extra copies by rule. [Duplicates](docs/files/duplicates.md)
- **Eighteen themes with the looks of their era,** and 18 languages, Hebrew right to left. [Themes](docs/customise/themes.md) · [Languages](docs/customise/languages.md)
- **Nothing leaves your machine** unless you ask for it. [Privacy](docs/reference/privacy.md)

| | |
|---|---|
| ![The terminal app: two blue panels, git status on the left](docs/screenshots/tui-panels.png) | ![Find file searching the text of files](docs/screenshots/gui-text-search.png) |
| [The terminal app](docs/reference/terminal-app.md) | [Find file, inside files](docs/search/text.md) |
| ![The preview pane: Markdown with a Mermaid diagram and math, a notebook, a spreadsheet, a Word document and a font](docs/screenshots/gui-previews.png) | ![A tar.gz archive open like a folder](docs/screenshots/gui-archive.png) |
| [The preview pane](docs/previews/README.md) | [Archives as folders](docs/files/archives.md) |

## Install

| System | Terminal app | Desktop app |
|---|---|---|
| macOS | `brew install mwo-dk/coxswain/coxswain` | `brew install --cask mwo-dk/coxswain/coxswain-gui`, or the `.dmg` |
| Linux | `brew install mwo-dk/coxswain/coxswain`, or the static `x86_64-unknown-linux-musl` binary | `.deb`, `.rpm`, `.AppImage`, or the Homebrew cask (x86-64) |
| Windows | `coxswain-terminal-<version>-x86_64-pc-windows-msvc.zip` | `.msi` or `-setup.exe` |
| Any | `cargo install coxswain` | `./install/install.sh` (or `install\install.ps1`) from a clone |

Downloads are on the [releases page](https://github.com/mwo-dk/coxswain/releases/latest).
**The builds are not code-signed:** on Windows click *More info* and *Run anyway*; on macOS, if
the app "is damaged", run `xattr -cr /Applications/Coxswain.app` once (the cask does it for you).
Both apps check once a day for a newer release and give the exact update command
([Update checks](docs/reference/updates.md)). Every way to install and update, and building
from source: [install/INSTALL.md](install/INSTALL.md). Git glyphs want a
[Nerd Font](https://www.nerdfonts.com/); without one set `glyphs = "ascii"`
([Glyphs and fonts](docs/customise/glyphs-and-fonts.md)).

## First steps

1. **Start it** in a folder: `coxswain` (or `cox`) in a terminal, or `coxswain-gui`. Two folders
   open the two panels: `coxswain ~/src ~/Downloads`.
2. **Move** with the arrows, **Enter** opens (a folder, or an archive), **Backspace** goes up,
   **Tab** switches panels.
3. **Mark** with **Insert**, then **F5** copies or **F6** moves to the other panel. **F8** moves
   to the trash, **F7** makes a folder.
4. **Find** anything with **Ctrl+F**; **Tab** looks deeper.
5. **Type** a command and press **Enter**: it runs in the panel's folder.
6. In the desktop app, **Space** shows the preview pane and **Ctrl+,** opens Settings.
7. Forgot a key? **F9** lists every command, and **F1** shows your keys.

More in [Panels and keys](docs/panels/README.md) and [Every default key](docs/panels/keys.md).

## Documentation

The [docs index](docs/README.md) is the full map; every feature has its own page with the keys,
what you see, the settings, and the questions people ask.

| Area | What is in it |
|---|---|
| [Panels and keys](docs/panels/README.md) | The screen, moving, quick search, marking, sorting, tabs, views, folder sizes, git, git history, the mouse, every key |
| [Tags, notes, favourites and the sidebar](docs/organise/README.md) | Colour tags, folder notes, favourite groups, places and drives |
| [Commands, the user menu and scripts](docs/commands/README.md) | The command line, F2 menu, scripts, F3 and F4, opening files |
| [Search](docs/search/README.md) | Names, text, scans, diagrams, git history, meaning, servers, the helper, disks, battery, notices |
| [The preview pane](docs/previews/README.md) | Every format, HTML, Office, diagrams, LaTeX, tools and containers |
| [Files](docs/files/README.md) | Copy, move, delete, clipboard, drag and drop, batch rename, archives, properties, duplicates |
| [Customising](docs/customise/README.md) | Settings, themes, looks, your own theme, languages, keys, glyphs |
| [Reference](docs/reference/README.md) | The terminal app, flags, every config key, privacy, updates, where files are kept |
| [Questions, collected](docs/faq.md) | The common questions, linked to their full answers |

## Layout

```
crates/coxswain-core   config, fs ops, archives, git status and history, search index and store, duplicates (shared)
crates/coxswain        terminal UI (package and binaries: coxswain, cox)
gui/                   Svelte 5 frontend
gui/src-tauri          Tauri backend (binary: coxswain-gui)
```

## Licence

MIT

Norton Commander is a trademark of Gen Digital Inc. Coxswain is an independent project, not
affiliated with or endorsed by Gen Digital.

## Changelog

Newest first. Downloads for each release are on the [releases page](https://github.com/mwo-dk/coxswain/releases).

| Version | Date | What's new |
|---|---|---|
| **1.26.2** | 2026-10-01 | Final performance review: thumbnails keep only the rows on screen (100,000 files in 14 ms), a folder's listing is half the size, highlighting runs off the window's thread, the terminal app copies on a thread with `(2/3)` progress and lists a history without freezing, files copied out of a history keep the commit's date, a solid 7z previews, copies and searches every file again, a short folder after a long one no longer shows blank, and Ask follows its answer only while you are at the bottom. [Performance](docs/reference/performance.md) |
| **1.26.1** | 2026-10-01 | Final security review: a repository someone else made can no longer run its own programs through git (filter drivers, file system monitor, signature checker, fetch); LibreOffice converts with macros and link updates off; on Windows a file opens through the shell, not `cmd`; archives cannot lead outside, fill the disk on a preview or lose files on a move, and keep their long names and permissions when changed; a 7z key is made as 7-Zip makes it; Stop ends an Ask while the model thinks; settings saves never overwrite each other; releases come only from master. [Safety](docs/previews/safety.md) · [Security](docs/reference/security.md) |
| **1.26.0** | 2026-10-01 | Pack with a password: Alt+F5 into a zip or 7z takes a password (typed twice), every file locked with AES-256, and a 7z's file names hidden too if you like; the new archive opens without asking again while the app runs. Both apps. [Passwords](docs/files/archive-passwords.md#locking-a-new-archive) |
| **1.25.2** | 2026-10-01 | Nothing leaves the machine from a preview: the webview is held to the app and your disk, HTML pages see their own folder only, a project's `latexmkrc` no longer runs, and a locked 7z stays locked when changed. [Safety](docs/previews/safety.md) · [Privacy](docs/reference/privacy.md) |
| **1.25.1** | 2026-10-01 | Performance review: the desktop app keeps only the rows on screen in the page, so a folder of 100,000 files lists in 30 ms and the cursor moves in one; nothing that touches the disk runs on the window's thread; both apps say *Working on …* during a copy, move, delete, extract or pack and while a slow folder is read; the terminal app sorts without rereading and draws a frame in half a millisecond however much is marked. [Performance](docs/reference/performance.md) |
| **1.25.0** | 2026-10-01 | Search inside archives: the files in your zip, 7z and tar archives are found by name, by their text, by meaning and by Ask, shown as a path through the archive, and Enter opens the archive there; a changed archive is read again, a locked one keeps its secrets. On by default, *Search inside archives* in Settings. [Inside archives](docs/search/archives.md) |
| **1.24.0** | 2026-10-01 | Git history as folders: Ctrl+G on a file or folder lists the commits that touched it, Enter on one browses the files as they were, with a preview, the commit's diff, F3 and F5; a *Last commit* column (date and author) in both apps; commit messages, authors and changed paths in Find file, by text and by meaning. Text previews of files inside archives show again. [Git history](docs/panels/git-history.md) · [History in search](docs/search/history.md) |
| **1.23.1** | 2026-10-01 | Dependencies reviewed: every crate and npm package at its latest version, the one known vulnerability (lodash in the Mermaid renderer) fixed, the terminal app without widgets it never draws, draw.io diagrams drawn without a word to diagrams.net, and from now on a weekly check of every dependency for advisories, with Dependabot proposing updates. [Security](docs/reference/security.md) |
| **1.23.0** | 2026-10-01 | Files inside archives show in the preview pane, and F3 views them in the terminal app, from a copy of just that file; locked ones ask for their password. [Archives](docs/files/archives.md) |
| **1.22.3** | 2026-10-01 | Both apps reviewed: Ctrl+C in a command keeps the terminal app, Delete while typing a command deletes nothing, Alt+F2 paths start from the right panel, the F-key bar names the action a shared key runs; the desktop app lists your own themes under F9, runs F2 entries with an upper-case key, shows USB sticks under `/run/media`, draws ASCII or your own folder/file/link glyphs, keeps Automatic on British English, and says when an editor or opener fails. [Keys](docs/panels/keys.md) · [Glyphs](docs/customise/glyphs-and-fonts.md) |
| **1.22.2** | 2026-10-01 | Archive review: a 7z with locked contents asks for its password when changed, F5/F6/Ctrl+V into an archive keep the name you give, F8 inside says Delete, only the locked files run again after a password, an archive is listed once while it does not change and written by one change at a time, 7z entries keep their dates, extraction reads a 7z once, an archive inside one is not opened empty, moves fall back to copying only across disks, case-only renames work where case does not count, and a batch rename that fails gives names back. [Archives](docs/files/archives.md) |
| **1.22.1** | 2026-10-01 | Search review: folder totals twenty times faster over a big store, searches go on while the index reads a moved-in folder, a damaged index cache never crashes the helper, a server's refusal of one file no longer stops search by meaning, API keys reach the model lists, diagram sentences get vectors in long files, and honest preview notes. [Search](docs/search/README.md) |
| **1.22.0** | 2026-10-01 | Ask: Tab to the fourth depth of Find file, ask a question, and your own chat model answers from your files, citing them; follow-ups work, nothing is kept. [Ask](docs/search/ask.md) |
| **1.21.0** | 2026-10-01 | Cryptography bills of materials (CycloneDX CBOMs): every algorithm, key, certificate and protocol rated, as a tree or sunburst, compared with an older scan, in both apps. [CBOMs](docs/previews/bom.md) |
| **1.20.0** | 2026-10-01 | More archive kinds: tar.bz2, tar.xz, tar.zst and 7z, read and written, and locked 7z archives open with their password. [Archives](docs/files/archives.md) |
| **1.19.0** | 2026-10-01 | Archives are folders: go in with Enter, copy and move in, out and between them, rename, make folders, take out, pack with Alt+F5, with passwords for locked zips. [Archives](docs/files/archives.md) |
| **1.18.0** | 2026-09-30 | Diagrams are searched by what they say: a sentence per arrow in draw.io, Mermaid, Graphviz and PlantUML. [Diagrams in search](docs/search/diagrams.md) |
| **1.17.0** | 2026-09-30 | HTML files show as pages, safely sealed off; the title bar shows the version and search depths on Linux too. [HTML pages](docs/previews/html.md) |
| **1.16.0** | 2026-09-30 | Search by meaning on your own Ollama, Lemonade or any OpenAI-style server. [Servers](docs/search/servers.md) |
| **1.15.0** | 2026-09-30 | Notices say once what search can do and what is off; the titles show the version and search depths. [Notices](docs/search/notices.md) |
| **1.14.0** | 2026-09-30 | LaTeX builds by itself when its sources change, and another engine takes over when one fails. [LaTeX](docs/previews/latex.md) |
| **1.13.0** | 2026-09-30 | Find file shows where it can look, and that search by meaning is there to turn on. [Find file](docs/search/find-file.md) |
| **1.12.1** | 2026-09-30 | Terminal app: typing in Find file never waits for the search. [Find file](docs/search/find-file.md) |
| **1.12.0** | 2026-09-30 | Search by meaning: a small multilingual model on your machine finds files about what you type. [Search by meaning](docs/search/meaning.md) |
| **1.11.0** | 2026-09-30 | LaTeX picks XeLaTeX or LuaLaTeX by the packages and builds past errors; the preview cache can be cleared. [LaTeX](docs/previews/latex.md) |
| **1.10.0** | 2026-09-30 | PowerPoint decks show at once, drawn in the app; LibreOffice's exact view follows. [Office](docs/previews/office.md) |
| **1.9.1** | 2026-09-30 | Office previews work again from the AppImage. [Office](docs/previews/office.md) |
| **1.9.0** | 2026-09-30 | Search reads pictures, scans and older Office files with programs you have installed. [Scans and pictures](docs/search/scans.md) |
| **1.8.0** | 2026-09-30 | The search helper can start with your session: systemd, launchd or the Windows Run key. [The search helper](docs/search/helper.md) |
| **1.7.0** | 2026-09-30 | draw.io diagrams drawn in the app by draw.io's own viewer; slides and documents render by themselves. [Diagrams](docs/previews/diagrams.md) |
| **1.6.0** | 2026-09-30 | Search pauses on battery, and knows removable disks wherever they are mounted. [Battery](docs/search/battery.md) · [Removable disks](docs/search/removable-disks.md) |
| **1.5.0** | 2026-09-30 | Search inside files in Settings: the folders read, names only, Index now, Delete the index. [Choosing the folders](docs/search/folders.md) |
| **1.4.2** | 2026-09-30 | A new app icon in Cyber's colours. [Themes](docs/customise/themes.md) |
| **1.4.1** | 2026-09-30 | Search follows changes on disk within seconds; Duplicates reuses the hashes search already has. [Text in files](docs/search/text.md) · [Duplicates](docs/files/duplicates.md) |
| **1.4.0** | 2026-09-30 | Folder sizes in your home folder are there at once. [Folder sizes](docs/panels/folder-sizes.md) |
| **1.3.1** | 2026-09-29 | Search inside PDFs. [Documents it reads](docs/search/documents.md) |
| **1.3.0** | 2026-09-29 | Search inside Word, spreadsheets, slides, mail, books and notebooks. [Documents it reads](docs/search/documents.md) |
| **1.2.0** | 2026-09-29 | Search inside your files: Tab in Find file. [Text in files](docs/search/text.md) |
| **1.1.0** | 2026-09-29 | One file name index for every window and the terminal app. [Names everywhere](docs/search/names.md) |
| **1.0.1** | 2026-09-29 | Screenshots in Cyber, diagrams in the theme's colours, Homebrew as the one package manager. [Install](install/INSTALL.md) |
| **1.0.0** | 2026-09-29 | Coxswain under its own name: two panels in the terminal and on the desktop, previews, duplicates, 18 themes and 18 languages. [Docs index](docs/README.md) |

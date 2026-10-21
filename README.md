# Coxswain

*The one at the helm, who steers the boat and keeps the crew in time.*

Coxswain is a two-panel file manager in the Norton Commander tradition, built for developers.
It comes as a terminal app (`coxswain`, Rust and Ratatui) and a desktop app (`coxswain-gui`,
Tauri and Svelte 5), both on one shared Rust core and one config file. It finds any file by
its name, its text or what it is about, previews over 60 kinds of file, opens archives like
folders, shows git in every panel, and keeps everything on your machine.

<picture><source media="(prefers-reduced-motion: reduce)" srcset="docs/screenshots/find.png"><img src="docs/screenshots/find.gif" alt="The desktop app: Ctrl+F opens Find and engine is typed; the groups fill in as it is typed, Names with engine.rs, In files with main.rs and engine.rs and the lines that say engine, About this; Esc closes Find"></picture>
*Find: type a word and names, the text inside files and what files are about come in, grouped, as you type.*

## Why Coxswain

- **One core, two apps:** a terminal app and a desktop app with the same keys, the same settings and one search store.
- **Finds by name, by the words inside files and by what they are about,** offline, with a small model on your machine or your own Ollama or LM Studio.
- **Ask your files** a question in your own words, and get an answer with numbered sources, from a model on your machine.
- **Archives, git history, branches and ZFS snapshots open like folders:** go in, look, copy out.
- **Runs where you work:** FreeBSD first, then NetBSD, OpenBSD, illumos, Linux (ARM too), Android (Termux), macOS and Windows; 31 languages.
- **Nothing leaves your machine** unless you ask, and online-only cloud files are never downloaded behind your back.

## Highlights

- **On FreeBSD and illumos: browse ZFS snapshots like folders.** **Alt+Z** lists a folder's snapshots, Enter shows it as it was, Diff compares a file with now and F5 copies it back, with no root; the dataset, its compression and space sit in the footer. Properties names a file's package and lists its files, shows and sets chflags, and the sidebar has your boot environments and jails. OpenZFS on Linux and macOS too, and on TrueNAS over SSH. [ZFS snapshots](docs/files/zfs-snapshots.md) · [FreeBSD](docs/systems/freebsd.md#zfs-packages-flags-boot-environments-and-jails)
- **FreeBSD, the other BSDs, illumos, Linux, macOS and Windows:** the terminal app on FreeBSD, NetBSD, OpenBSD, illumos, Linux, macOS and Windows, the desktop app on all but NetBSD and illumos, each BSD and illumos with an install line, its own package tool for what is missing, its own service for the search helper (rc.d, rcctl, SMF) and a manual page; TrueNAS over SSH. [FreeBSD](docs/systems/freebsd.md) · [NetBSD](docs/systems/netbsd.md) · [OpenBSD](docs/systems/openbsd.md) · [illumos](docs/systems/illumos.md) · [TrueNAS](docs/systems/truenas.md)
- **Norton Commander at heart:** two panels, the F-key bar, a command line and NC's keys. [Panels and keys](docs/panels/README.md)
- **See where the space goes:** **Alt+U** on a folder or a drive shows every file as a rectangle by its size, inside its folder and coloured by its kind, with the bytes by type and the largest files beside it; zoom in, and delete what you do not need from there. Both apps. [Space](docs/panels/space.md)
- **One Find for names, words, meaning and answers:** Ctrl+F and type; the hits come in groups (*Names*, *In files*, *About this*, *History*) ranked by words and meaning together, Ctrl+Enter asks, one key limits it to this folder, inside your zip, 7z and tar archives too; you choose what is read and what is left out (`*.log`, a folder). [Search](docs/search/README.md) · [Inside archives](docs/search/archives.md) · [Choosing the folders](docs/search/folders.md)
- **Gentle with your cloud:** OneDrive, Dropbox, Google Drive, Proton Drive and iCloud files that are only online are found by name, marked with a cloud, and never downloaded unless you open one. Each cloud account is one entry in the sidebar and under **Alt+F1**, found on your machine, with no logins. [Cloud files](docs/search/cloud-files.md) · [Clouds](docs/organise/clouds.md)
- **Ask your files:** a question in your own words, answered from the closest passages with numbered sources, by your own chat model or one built in that needs no server (on a Mac it uses the GPU). [Ask](docs/search/ask.md) · [Without a server](docs/search/ask-builtin.md) · [Set it up](docs/search/setup.md)
- **Search by meaning, in any language:** a small model on your machine (on Apple Silicon it uses the GPU), or your own Ollama, Lemonade or OpenAI-style server. [Search by meaning](docs/search/meaning.md) · [Set it up](docs/search/setup.md)
- **See before you open:** code in about 200 languages (COBOL, Fortran and Pascal too), Markdown, PDF, Word, PowerPoint, spreadsheets, SQLite and Parquet (in the terminal app F3 too, in its built-in viewer), HTML, fonts, video and more. [The preview pane](docs/previews/README.md) · [Data](docs/previews/data.md)
- **Builds what needs building:** LaTeX, Office, PlantUML and Graphviz previews, videos and photos the player cannot show, MP4, MP3, smaller or location-free copies of your videos, songs and photos, maps of your geodata and pictures and STL files of your OpenSCAD models, with your tools or a sealed container. [LaTeX](docs/previews/latex.md) · [Tools](docs/previews/tools.md) · [Convert video or audio](docs/files/convert-media.md) · [Convert or edit pictures](docs/files/edit-pictures.md) · [Geodata](docs/previews/geodata.md) · [OpenSCAD](docs/previews/openscad.md)
- **Reads cryptography bills of materials:** a CycloneDX CBOM as a rated tree or sunburst, compared with last month's scan, in both apps. [CBOMs](docs/previews/bom.md)
- **Reads build provenance:** SLSA and in-toto files as inputs → build → outputs, with who signed them, whether the files here are the ones built, and whether the commit is in your checkout, in both apps. [Build provenance](docs/previews/provenance.md)
- **Reads chess games:** a PGN file (packed too) as a table of games to filter by player, opening, result, year and rating, fast with millions of games; each game on a board in the theme's colours with its moves in figurines, variations and comments; EPD and FEN positions; a Polyglot opening book's moves for each position; Find finds games by player or opening. Go games in SGF too, on a board with their captures and variations, and draughts in PDN, international and checkers, on a numbered board. In both apps. [Chess games](docs/previews/chess.md) · [Go games](docs/previews/go.md) · [Draughts games](docs/previews/draughts.md)
- **Shows neural network models:** ONNX, TensorFlow Lite, Core ML and Keras files with their inputs and outputs, shapes, nodes and parameters in both apps; the desktop app draws the graph with Netron, offline and sandboxed. Nothing in the model runs. [Neural network models](docs/previews/neural-networks.md)
- **Archives are folders:** zip, 7z and tar (gz, bz2, xz, zst): go in, preview, copy, move, rename, pack, with passwords to open them and to lock new zips and 7z (AES-256). [Archives](docs/files/archives.md)
- **Git in every panel:** branch, ahead and behind, counts, a glyph per file, and who last committed it and when; the git config as git uses it, secrets hidden. [Git](docs/panels/git.md), [Git config](docs/panels/git-config.md)
- **History as folders:** **Ctrl+G** on a file or folder lists its commits; Enter on one browses the files as they were, F5 copies an old version out, and Find finds commit messages. [Git history](docs/panels/git-history.md) · [History in search](docs/search/history.md)
- **Branches and worktrees too:** **Alt+B** lists the branches, Enter browses one's files, **Alt+S** switches to it (git refuses to lose your changes); **Alt+W** lists the worktrees. The git line names a worktree, a detached HEAD and a merge or rebase under way. [Git branches](docs/panels/git-branches.md)
- **Folder sizes without asking,** instant in your home folder. [Folder sizes](docs/panels/folder-sizes.md)
- **Finds duplicates** by content across folders and disks, and marks the extra copies by rule. [Duplicates](docs/files/duplicates.md)
- **Thirty-three themes with the looks of their era, from the Amiga and the home computers to Windows and Mac,** and 31 languages, listed by region: Hebrew and Persian right to left, Greek capitals without accents, Japanese and Korean lined up in the terminal. [Themes](docs/customise/themes.md) · [Languages](docs/customise/languages.md)
- **Nothing leaves your machine** unless you ask for it. [Privacy](docs/reference/privacy.md)

| | |
|---|---|
| ![The terminal app: two blue panels, git status on the left](docs/screenshots/tui-panels.png) | ![The desktop app in Cyber, its default theme: a green phosphor terminal with two panes, git status, colour tags and the F-key bar](docs/screenshots/gui-details.png) |
| [The terminal app](docs/reference/terminal-app.md) | [The desktop app in Cyber](docs/customise/themes.md) |
| ![The preview pane: Markdown with a Mermaid diagram and math, a notebook, a spreadsheet, a Word document and a font](docs/screenshots/gui-previews.png) | ![A tar.gz archive open like a folder](docs/screenshots/gui-archive.png) |
| [The preview pane](docs/previews/README.md) | [Archives as folders](docs/files/archives.md) |
| ![Morphy's Opera Game in the desktop app at 15.Bxd7+: the board in Cyber's greens, the moves beside it in figurines with a variation in brackets](docs/screenshots/gui-chess.png) | ![The terminal app's chess view: the board in Unicode pieces on blue squares, the moves beside it](docs/screenshots/tui-chess.png) |
| [Chess games](docs/previews/chess.md) | [Chess in the terminal app](docs/previews/chess.md#a-game-on-the-board) |
| ![Space in the desktop app: the demo home as a treemap, videos, the virtual machine, music and photos as coloured rectangles inside their folders, the bytes by type beside it](docs/screenshots/space.png) | ![Space in the terminal app: the same treemap in coloured half-blocks, folder names on their title rows, the bytes by type on the right](docs/screenshots/tui-space.png) |
| [Space: where the space goes](docs/panels/space.md) | [Space in the terminal app](docs/panels/space.md#in-the-terminal-app) |

## Smart search: names, words, meaning and answers

One field finds every file by **name** in a blink, by the **words** inside it, by what it is
**about** in any language, and **Ask** answers a question from your files with numbered sources:
type, and the hits come in groups, the ones that fit your query first; **Ctrl+Enter** asks.
It all runs on your own machine, or on your own server (Ollama, Lemonade, LM Studio, llama.cpp
…); nothing leaves it unless you choose a server elsewhere. Three steps:

1. **Settings → Finding files → Set up…** (terminal app: `coxswain --setup-search`). The guide
   finds the model servers on your machine and says what suits it.
2. **Use the graphics card or NPU** if you have one: the guide checks that the server does.
3. **Let it read in the background:** *Start with my session* keeps the index current between
   launches (a systemd user service, a launchd agent, a Run entry or an XDG autostart entry on
   FreeBSD; no administrator rights).

The whole walk-through: [Smart search in a few minutes](docs/search/setup.md).

<picture><source media="(prefers-reduced-motion: reduce)" srcset="docs/screenshots/ask.png"><img src="docs/screenshots/ask.gif" alt="The desktop app: Ctrl+F opens Find, rocket fuel cost is typed and Ctrl+Enter asks; the answer appears in place, The rocket fuel costs 2,105 kEUR in April and 2,655 kEUR in June [1], with the sources budget.txt and budget-da.txt numbered under it"></picture>

*Ask with `qwen3:8b` on Ollama on the same machine. The wait for the first word is cut short.*

## Install

Quickest, per system (every option is in the table below):

```mermaid
flowchart LR
  you([Your system]) --> fb[FreeBSD] --> fbs["install-freebsd.sh<br>both apps; port under review"]
  you --> bsd[NetBSD, OpenBSD] --> unix["install-unix.sh<br>(ftp)"]
  you --> ill[illumos] --> unixc["install-unix.sh<br>(curl)"]
  you --> nas[TrueNAS] --> nasb["FreeBSD or Linux build<br>in your home, over SSH"]
  you --> lin[Linux] --> linb["brew, or the static binary<br>.deb, .rpm, .AppImage<br>Flatpak, Nix"]
  you --> and[Android] --> ter["Termux: cargo install,<br>pkg once packaged"]
  you --> cros[ChromeOS] --> deb[".deb in Linux"]
  you --> any[Any, with Rust] --> cargo["cargo install --locked coxswain"]
  you --> mac[macOS] --> brew["brew, brew --cask, .dmg"]
  you --> win[Windows] --> wg["winget, .msi, -setup.exe"]
```

```sh
fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh   # FreeBSD
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh         # NetBSD, OpenBSD
curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh       # illumos
brew install mwo-dk/coxswain/coxswain                                                                 # Linux, macOS: terminal app
cargo install --locked coxswain                                                                       # anywhere with Rust, Termux too
brew install --cask mwo-dk/coxswain/coxswain-gui                                                      # macOS: desktop app
winget install mwo-dk.Coxswain.Terminal                                                               # Windows: terminal app
```

| System | Terminal app | Desktop app |
|---|---|---|
| FreeBSD 14, 15 | `fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh \| sh` installs both apps ([FreeBSD](docs/systems/freebsd.md)); `--terminal-only` for the terminal app alone | The same line (experimental), with the packages it needs from `pkg` |
| NetBSD 10, 11 | `ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh \| sh` ([NetBSD](docs/systems/netbsd.md)) | Not yet: webkit-gtk41's binary packages miss dependencies |
| OpenBSD 7.9 | `ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh \| sh` installs both apps ([OpenBSD](docs/systems/openbsd.md)); `--terminal-only` for the terminal app alone | The same line (experimental), with `webkitgtk41` from `pkg_add` |
| illumos (OmniOS, OpenIndiana) | `curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh \| sh` ([illumos](docs/systems/illumos.md)) | Not built: no WebKitGTK 4.1 |
| TrueNAS | The Linux build (Community Edition) or the FreeBSD one (CORE) in your home, over SSH ([TrueNAS](docs/systems/truenas.md)) | Not on a NAS |
| Linux | `brew install mwo-dk/coxswain/coxswain`, or the static `x86_64-unknown-linux-musl` binary | `.deb`, `.rpm`, `.AppImage`, or the Homebrew cask (x86-64) |
| Linux, Flatpak | Not in it: the line above | Not on Flathub yet: build it with `flatpak-builder` ([Flatpak](docs/systems/flatpak.md)) |
| Linux on ARM64 (Raspberry Pi OS 64-bit, Asahi, ARM servers) | The static `aarch64-unknown-linux-musl` binary, or `brew` as above | `_arm64.deb`, `.aarch64.rpm`, `_aarch64.AppImage` ([Linux on ARM](docs/systems/linux-arm.md)) |
| Nix (Linux, macOS) | `nix run github:mwo-dk/coxswain`, or `nix profile install github:mwo-dk/coxswain` ([Nix](docs/systems/nix.md)) | `nix run github:mwo-dk/coxswain#coxswain-gui` (Linux) |
| Android (Termux) | `pkg install coxswain` once it is in Termux's repository; until then `pkg install rust git && cargo install --locked coxswain` ([Termux](docs/systems/termux.md)) | Not on Android |
| ChromeOS | The Linux binary for its processor, in the Linux terminal | The `.deb` for its processor, in Linux ([ChromeOS](docs/systems/chromeos.md)) |
| Any | `cargo install --locked coxswain` | `./install/install.sh` (or `install\install.ps1`) from a clone |
| macOS | `brew install mwo-dk/coxswain/coxswain` | `brew install --cask mwo-dk/coxswain/coxswain-gui`, or the `.dmg` |
| Windows | `winget install mwo-dk.Coxswain.Terminal`, or `coxswain-terminal-<version>-x86_64-pc-windows-msvc.zip` | `.msi` or `-setup.exe` |
| Windows on ARM | `coxswain-terminal-<version>-aarch64-pc-windows-msvc.zip` | `_arm64_en-US.msi` or `_arm64-setup.exe` ([Windows on ARM](docs/systems/windows-arm.md)) |

Downloads are on the [releases page](https://github.com/mwo-dk/coxswain/releases/latest).
**The builds are not code-signed:** on macOS, if the app "is damaged", run
`xattr -cr /Applications/Coxswain.app` once (the cask does it for you); on Windows click
*More info* and *Run anyway*. Why macOS may ask about your folders again after an update: [macOS](docs/systems/macos.md#why-a-new-version-may-ask-again).
Both apps check for a newer release when they start, then once a day while open, and give the exact update command
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
   to the trash, **F7** makes a folder, and **Ctrl+Z** undoes the last of them.
4. **Find** anything with **Ctrl+F**; **Tab** steps through the kinds: *Names*, *In files*,
   *About* (meaning) and *Ask*.
   **Turn on smart search** (meaning and Ask): *Settings → Finding files → Set up…*, or
   `coxswain --setup-search` ([the guide](docs/search/setup.md)).
5. **Type** a command and press **Enter**: it runs in the panel's folder.
6. **Settings**, sorted by task (Finding files, Previews, Looks, Behaviour, Keys, Privacy and
   updates): **Ctrl+,** or the gear button in the desktop app, **F12** (*Settings* at the right end
   of the key bar), **Shift+F10** → *Settings* or **F9** → *Settings* in the terminal app. In the desktop app, **Space** shows the preview pane.
7. Not sure what you can do with a file? **Shift+F10** (or the **Menu** key, or the **⋯** on the
   row) lists what fits it, each with its key: pack, extract, rename, history
   ([The action menu](docs/panels/action-menu.md)). Forgot a key? **F9** lists every command,
   and **F1** shows your keys.

More in [Panels and keys](docs/panels/README.md) and [Every default key](docs/panels/keys.md).

## Documentation

The [docs index](docs/README.md) is the full map; every feature has its own page with the keys,
what you see, the settings, and the questions people ask.

| Area | What is in it |
|---|---|
| [Panels and keys](docs/panels/README.md) | The first-run guide, the screen, notices and what's new, moving, quick search, marking, sorting, tabs, views, folder sizes, git, git history, branches and worktrees, the mouse, the command list (F9), Features (F1), the action menu, what the apps remember, every key |
| [Tags, notes, favourites and the sidebar](docs/organise/README.md) | Colour tags, folder notes, favourite groups, places and drives |
| [Commands, the user menu and scripts](docs/commands/README.md) | The command line, F2 menu, scripts, F3 and F4, opening files |
| [Search](docs/search/README.md) | The guided setup, Find, names, text, documents, scans, diagrams, git history, archives, cloud files, removable disks, meaning, servers, Ask, Ask about code, Ask without a server, built-in models, the helper, battery, search settings |
| [The preview pane](docs/previews/README.md) | Text and code, documents, HTML, Office, data, CBOMs, build provenance, media, diagrams, LaTeX, tools and containers, safety |
| [Files](docs/files/README.md) | Copy, move, new folder, delete, undo, clipboard, drag and drop, batch rename, archives, pack and extract, passwords, disk images, local models by name, properties, duplicates, ZFS snapshots |
| [Customising](docs/customise/README.md) | Settings, themes, looks, your own theme, languages, keys, glyphs |
| [Reference](docs/reference/README.md) | The terminal app, flags, every config key, privacy, updates, where files are kept, disk use, security, licences and bills of materials, performance |
| [Systems](docs/systems/README.md) | FreeBSD, NetBSD, OpenBSD, illumos, TrueNAS, Linux on ARM, Flatpak, Nix, Termux on Android, ChromeOS, macOS, Windows on ARM |
| [Questions, collected](docs/faq.md) | The common questions, linked to their full answers |
| [Changelog](docs/changelog.md) | Every version, newest first; the last ten are below |

## Layout

```
crates/coxswain-core   config, fs ops, archives, git status and history, search index and store, duplicates (shared)
crates/coxswain        terminal UI (package and binaries: coxswain, cox)
gui/                   Svelte 5 frontend
gui/src-tauri          Tauri backend (binary: coxswain-gui)
```

## Licence

MIT. Every release carries the licences of what it is built from (`THIRD-PARTY-NOTICES.md`, also
inside the apps), an SBOM for each app and a CBOM of the cryptography they use; see
[Licences and bills of materials](docs/reference/bills-of-materials.md).

Norton Commander is a trademark of Gen Digital Inc. Coxswain is an independent project, not
affiliated with or endorsed by Gen Digital.

## Changelog

Newest first. Downloads for each release are on the [releases page](https://github.com/mwo-dk/coxswain/releases).

| Version | Date | What's new |
|---|---|---|
| **2.32.1** | 2026-10-20 | **A security fix, and fixes from the 2.32.0 test round.** **Security:** in the desktop app a LaTeX file's name could run a command: a name starting with `-` was read by latexmk as an option, on a build that starts by itself. The document now goes as `./name`, a name the shell would act on is built without latexmk, and `% !TEX root` is followed only inside the project ([LaTeX projects](docs/previews/latex.md#what-coxswain-works-out)). **No lost files:** a move (**F6**) of a folder into an archive is refused when it holds a link, a pipe or a device the archive would leave out; Find duplicates scans a ticked folder inside another ticked folder once, so on Windows no file is its own duplicate; a failed folder copy leaves nothing behind and copied files keep their date; copying a pipe no longer hangs; a crafted 7z can no longer fill the memory; **Ctrl+E** leaves out a tar's links, and on Windows names such as `NUL` are not written ([Copy](docs/files/copy.md#what-you-see)). **No freeze, no crash:** the terminal viewer no longer stalls on a long line of hits, and a search through a huge file shows how far it is (**Esc** stops it); Space, folder sizes and the file name index get through thousands of nested folders; Space draws in a terminal 20 to 30 columns wide; a broken PSD is refused ([View and edit](docs/commands/view-and-edit.md#the-built-in-viewer)). **Chess and search:** a PGN file with a byte order mark no longer shows an empty first game, a null move (`--`) passes the turn, a file dated in the future gets its vectors, and an older Coxswain leaves what a newer one's code reader read ([Chess](docs/previews/chess.md#files-it-reads)). **Desktop app:** a fast scroll in the table of games no longer leaves it at an older place, and a tag, a note, F4 with no editor or a session that cannot be saved now say why in the status line ([Chess games](docs/previews/chess.md#the-table-of-games)). **OpenBSD and NetBSD:** the install script and the notice name OpenBSD's `gst-plugins-good`, and NetBSD's rc.d script no longer warns at boot ([OpenBSD](docs/systems/openbsd.md#the-packages-and-what-each-is-for) · [NetBSD](docs/systems/netbsd.md#from-boot-with-rcd)). Nothing is read or made again. |
| **2.32.0** | 2026-10-20 | **Where the space goes, chess games, old files and pictures, disk images, about 200 programming languages, a built-in viewer, and Norwegian.** **Alt+U** shows a folder or a drive as a treemap coloured by kind, with the bytes by type and the 100 largest files, on one disk ([Space](docs/panels/space.md)). PGN chess files (also `.pgn.gz`, `.pgn.zst`) open as a table of games and a board with figurine notation, **Alt+K** ([Chess games](docs/previews/chess.md)). Old pictures and files show without anything installed: TIFF, TGA, Netpbm, DDS, HDR, OpenEXR, QOI, JPEG XL, Amiga IFF, PCX, XPM, XBM, Photoshop, CBZ comics, MBOX mailboxes, Outlook `.msg`, dBase tables and tracker music ([Old pictures and files](docs/previews/old-formats.md)). Disk images (`.iso`, `.dmg`, `.vhd`, `.vhdx`, `.vmdk`, `.qcow2` …) show their facts, and with *Open disk images like folders* on and 7-Zip installed open read-only like folders, with CHM and CBR ([Disk images](docs/files/disk-images.md)). **Alt+P** makes a scanned PDF searchable, and Typst, PostScript, XPS, D2 and ten more diagram kinds, old office files and clip art preview through installed programs or sealed containers ([Make PDF searchable](docs/files/searchable-pdf.md) · [Containers](docs/previews/containers.md#the-images)). Code in about 200 languages is highlighted in both apps, old ones too, and **F3** in the terminal app opens a built-in viewer with search ([Text and code](docs/previews/text-and-code.md#languages) · [View and edit](docs/commands/view-and-edit.md#the-built-in-viewer)). Norwegian in Bokmål and Nynorsk, 31 languages in all, and a Norwegian system no longer gets Danish ([Languages](docs/customise/languages.md#the-languages)). Copying a folder out of a disk image is safe with any 7-Zip, and the terminal viewer searches files of long lines a few megabytes at a time. A file an app keeps saving no longer shows as "1 file to go" for search by meaning ([Search by meaning](docs/search/meaning.md#why-does-it-say-a-file-is-changing)). **Read again once for search:** `.ps`/`.eps`; with LibreOffice installed `.wri`, `.wk1`, `.wks`, `.123`, `.sxc`, `.abw`, `.cwk`, `.numbers`; files with the newly known code endings (`.cob`, `.for`, `.bat`, `.mod`, `.pro`, `.inc` …); every PGN file; every `.msg` and `.dbf` file. Nothing else is redone. |
| **2.31.5** | 2026-10-19 | **Quicker with many hits, calmer while files change.** Find in the terminal app stays quick with thousands of hits: only the lines on screen are drawn. The desktop app reads a folder again at most every two seconds while something keeps writing into it, as the terminal app does, instead of four times a second. A meaning server's address in a message never shows part of a password or a token in its query. The terminal app's hint on showing hidden git secrets is only spent when there is one to show. [Find file](docs/search/find-file.md) **A hung cloud mount no longer freezes the terminal app.** **Alt+F1** and **Alt+F2** wait at most a moment for the cloud folders: when a mount whose server has gone (rclone, sshfs, gvfs) does not answer, the list opens without the clouds and the status line says so. The desktop app's sidebar leaves them out until the mount answers again, instead of waiting on it every 30 seconds. On FreeBSD, the other BSDs and illumos, a folder made inside a new folder while Coxswain was reading it is now followed for changes too, and a folder that cannot be followed is named once in `helper.log` and no longer counts against the number that can. Nothing is read or made again. [Clouds](docs/organise/clouds.md#why-does-altf1-say-the-clouds-did-not-answer) |
| **2.31.4** | 2026-10-19 | **Newer libraries.** The model tokenizer (tokenizers 0.23) and the 7z and xz readers (sevenz-rust2 0.24.1, lzma-rust2 0.23) are newer, as are the other Rust and web libraries, and the builds use Node 24. Nothing changes in how you use Coxswain. [Archives](docs/files/archives.md) |
| **2.31.3** | 2026-10-19 | **Safer git config and image updates.** The terminal app no longer closes when **Alt+C** shows a git config with a curly quote, a guillemet or another non-ASCII character right before an address (`!xdg-open “https://…”`). A token used as the user name in an http(s) address (`https://ghp_…:x-oauth-basic@github.com`, `https://<PAT>:@dev.azure.com`) is now hidden as `••••` too, as is the whole user part of such an address, and so are query keys like *jwt*, *ticket* and *pat*, keys ending in *pat* or *bearer*, and a `credential.helper` shell line with `--pat` or a token in it. An image update never removes the only copy of an image when the runtime cannot say which copy it now has; every image is tried even after one fails, and one image that always fails no longer makes the others pull again every hour. The terminal app's update check no longer writes over notices you dismissed or folders it noted while it waited for GitHub. [Git config](docs/panels/git-config.md#hidden-secrets) · [Containers](docs/previews/containers.md#keeping-images-up-to-date) |
| **2.31.2** | 2026-10-19 | **The update notice says when your channel gets a release later, and knows Nix.** WinGet, Flathub, Termux and nixpkgs take a new version a few days after GitHub, so their upgrade command could find nothing: the notice now adds *it can reach WinGet a few days later; or the release page*, in both apps. A copy run from `/nix/store/` is recognised and gets `nix profile upgrade coxswain` instead of the releases page. [Update checks](docs/reference/updates.md#the-command-finds-no-new-version) |
| **2.31.1** | 2026-10-19 | **Both apps hear of a new version sooner.** Each app now asks GitHub for a new version every time it starts, then once a day while it is open. Before, a start within a day of the last answer did not ask, and the terminal app asked only when it started, so one left open for days never heard; it now looks every hour too and names each new version once in the status line. Nothing else is sent. [Update checks](docs/reference/updates.md) |
| **2.31.0** | 2026-10-19 | **The git config, as git uses it.** **Alt+C** (also in **F9**, the action menu, the git line's right-click menu and the preview pane) shows every git setting that holds in the panel's folder: its value, its scope (system, global, local, worktree, command line, included) and the file it comes from, with values set earlier dimmed below the one that counts. Passwords and tokens (in URLs, `http.extraHeader`, keys named like *password* or *token*) show as `••••` until **Enter** or *Show*, and a copied line stays hidden until then. **F4** opens a line's file in your editor; **Alt+G** opens your git tool to change it, as Coxswain only reads. Outside a repository: the system and global config. Both apps. [Git config](docs/panels/git-config.md) |
| **2.30.0** | 2026-10-19 | **Container images kept up to date, and removed when unused.** *Settings → Previews → Keep images up to date* (Off, Weekly, Monthly; `[preview] image_updates`) has the desktop app pull the newest version of the preview images you already pulled, in the background and never on battery, and remove the copy each one replaced, by its own id, so no other image of yours is touched. *Remove images not used for 90 days* (`[preview] image_prune`) frees the room of an image no preview needed for that long. Both are off until you turn them on, and Settings names the registry they reach. Each image in *Container images* and in Disk use now says what it gives, how big it is (TeX Live: about 2.8 GB to download, 5.9 GB on disk) and what it costs. [Containers](docs/previews/containers.md#keeping-images-up-to-date) |
| **2.29.0** | 2026-10-19 | **Clouds.** Your cloud accounts, one entry each, like drives in a file explorer: *OneDrive – Personal*, *OneDrive – Contoso*, *Google Drive – you@example.com*, *Dropbox – Personal*, *Nextcloud – me@cloud.example.com*, *iCloud Drive*. In the desktop app they are a *Clouds* section of the sidebar; in the terminal app **Alt+F1** and **Alt+F2** list them, as *Cloud: …*. Coxswain finds them only from what is on the machine: rclone, onedriver and davfs2 mounts and GNOME's *Online Accounts* on FreeBSD and Linux, Nextcloud's and ownCloud's sync folders and Dropbox's everywhere, `~/Library/CloudStorage` and iCloud on macOS, and the sync roots on Windows. It never logs in to a cloud or asks one anything. [Clouds](docs/organise/clouds.md) |

All versions, back to 1.0.0: [the full changelog](docs/changelog.md).

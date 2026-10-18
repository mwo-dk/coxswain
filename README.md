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
- **Runs where you work:** FreeBSD first, then NetBSD, OpenBSD, illumos, Linux (ARM too), Android (Termux), macOS and Windows; 29 languages.
- **Nothing leaves your machine** unless you ask, and online-only cloud files are never downloaded behind your back.

## Highlights

- **On FreeBSD and illumos: browse ZFS snapshots like folders.** **Alt+Z** lists a folder's snapshots, Enter shows it as it was, Diff compares a file with now and F5 copies it back, with no root; the dataset, its compression and space sit in the footer. Properties names a file's package and lists its files, shows and sets chflags, and the sidebar has your boot environments and jails. OpenZFS on Linux and macOS too, and on TrueNAS over SSH. [ZFS snapshots](docs/files/zfs-snapshots.md) · [FreeBSD](docs/systems/freebsd.md#zfs-packages-flags-boot-environments-and-jails)
- **FreeBSD, the other BSDs, illumos, Linux, macOS and Windows:** the terminal app on FreeBSD, NetBSD, OpenBSD, illumos, Linux, macOS and Windows, the desktop app on all but NetBSD and illumos, each BSD and illumos with an install line, its own package tool for what is missing, its own service for the search helper (rc.d, rcctl, SMF) and a manual page; TrueNAS over SSH. [FreeBSD](docs/systems/freebsd.md) · [NetBSD](docs/systems/netbsd.md) · [OpenBSD](docs/systems/openbsd.md) · [illumos](docs/systems/illumos.md) · [TrueNAS](docs/systems/truenas.md)
- **Norton Commander at heart:** two panels, the F-key bar, a command line and NC's keys. [Panels and keys](docs/panels/README.md)
- **One Find for names, words, meaning and answers:** Ctrl+F and type; the hits come in groups (*Names*, *In files*, *About this*, *History*) ranked by words and meaning together, Ctrl+Enter asks, one key limits it to this folder, inside your zip, 7z and tar archives too; you choose what is read and what is left out (`*.log`, a folder). [Search](docs/search/README.md) · [Inside archives](docs/search/archives.md) · [Choosing the folders](docs/search/folders.md)
- **Gentle with your cloud:** OneDrive, Dropbox, Google Drive, Proton Drive and iCloud files that are only online are found by name, marked with a cloud, and never downloaded unless you open one. [Cloud files](docs/search/cloud-files.md)
- **Ask your files:** a question in your own words, answered from the closest passages with numbered sources, by your own chat model or one built in that needs no server (on a Mac it uses the GPU). [Ask](docs/search/ask.md) · [Without a server](docs/search/ask-builtin.md) · [Set it up](docs/search/setup.md)
- **Search by meaning, in any language:** a small model on your machine (on Apple Silicon it uses the GPU), or your own Ollama, Lemonade or OpenAI-style server. [Search by meaning](docs/search/meaning.md) · [Set it up](docs/search/setup.md)
- **See before you open:** code, Markdown, PDF, Word, PowerPoint, spreadsheets, SQLite and Parquet (in the terminal app F3 too), HTML, fonts, video and more. [The preview pane](docs/previews/README.md) · [Data](docs/previews/data.md)
- **Builds what needs building:** LaTeX, Office, PlantUML and Graphviz previews, with your tools or a sealed container. [LaTeX](docs/previews/latex.md) · [Tools](docs/previews/tools.md)
- **Reads cryptography bills of materials:** a CycloneDX CBOM as a rated tree or sunburst, compared with last month's scan, in both apps. [CBOMs](docs/previews/bom.md)
- **Reads build provenance:** SLSA and in-toto files as inputs → build → outputs, with who signed them, whether the files here are the ones built, and whether the commit is in your checkout, in both apps. [Build provenance](docs/previews/provenance.md)
- **Archives are folders:** zip, 7z and tar (gz, bz2, xz, zst): go in, preview, copy, move, rename, pack, with passwords to open them and to lock new zips and 7z (AES-256). [Archives](docs/files/archives.md)
- **Git in every panel:** branch, ahead and behind, counts, a glyph per file, and who last committed it and when. [Git](docs/panels/git.md)
- **History as folders:** **Ctrl+G** on a file or folder lists its commits; Enter on one browses the files as they were, F5 copies an old version out, and Find finds commit messages. [Git history](docs/panels/git-history.md) · [History in search](docs/search/history.md)
- **Branches and worktrees too:** **Alt+B** lists the branches, Enter browses one's files, **Alt+S** switches to it (git refuses to lose your changes); **Alt+W** lists the worktrees. The git line names a worktree, a detached HEAD and a merge or rebase under way. [Git branches](docs/panels/git-branches.md)
- **Folder sizes without asking,** instant in your home folder. [Folder sizes](docs/panels/folder-sizes.md)
- **Finds duplicates** by content across folders and disks, and marks the extra copies by rule. [Duplicates](docs/files/duplicates.md)
- **Thirty-three themes with the looks of their era, from the Amiga and the home computers to Windows and Mac,** and 29 languages, listed by region: Hebrew and Persian right to left, Greek capitals without accents, Japanese and Korean lined up in the terminal. [Themes](docs/customise/themes.md) · [Languages](docs/customise/languages.md)
- **Nothing leaves your machine** unless you ask for it. [Privacy](docs/reference/privacy.md)

| | |
|---|---|
| ![The terminal app: two blue panels, git status on the left](docs/screenshots/tui-panels.png) | ![The desktop app in Cyber, its default theme: a green phosphor terminal with two panes, git status, colour tags and the F-key bar](docs/screenshots/gui-details.png) |
| [The terminal app](docs/reference/terminal-app.md) | [The desktop app in Cyber](docs/customise/themes.md) |
| ![The preview pane: Markdown with a Mermaid diagram and math, a notebook, a spreadsheet, a Word document and a font](docs/screenshots/gui-previews.png) | ![A tar.gz archive open like a folder](docs/screenshots/gui-archive.png) |
| [The preview pane](docs/previews/README.md) | [Archives as folders](docs/files/archives.md) |

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
| [Files](docs/files/README.md) | Copy, move, new folder, delete, undo, clipboard, drag and drop, batch rename, archives, pack and extract, passwords, properties, duplicates, ZFS snapshots |
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
| **2.28.1** | 2026-10-19 | **`cargo install coxswain --version X` installs X.** Installing an older version from crates.io built it with the newest Coxswain library instead of its own, so `coxswain --version` (and the app) were a mix of two versions. The app now asks for its own library's version exactly. Versions before this one still mix when installed by number; install this one or newer. [Updates](docs/reference/updates.md) |
| **2.28.0** | 2026-10-18 | **Scripts in the terminal app too.** The programs in your `scripts` folder next to `config.toml` are now in the terminal app's **F2** menu as well, after your own entries, run on the marked files (or the file under the cursor) as full paths, on the real terminal, so a script can ask questions and its output stays until you press **Enter**. A new row in **F2** in both apps, *Open the scripts folder*, takes the active panel there and makes the folder when it is missing. Only files that really are in that folder run; a link to elsewhere is refused. [Scripts](docs/commands/scripts.md) |
| **2.27.9** | 2026-10-18 | **draw.io diagrams with maths show again.** A diagram with maths turned on stayed blank in the desktop app's preview: draw.io's viewer waited for its maths library, which Coxswain does not ship. It now draws the diagram at once, with the formulas as their TeX text. The draw.io viewer is now v32.0.2 and KaTeX, which draws maths in Markdown and notebooks, 0.19. [Diagrams](docs/previews/diagrams.md#why-does-a-formula-in-a-drawio-diagram-show-as-tex) |
| **2.27.8** | 2026-10-18 | **The desktop app asks before deleting where there is no bin.** On illumos, from another ZFS dataset than your home folder's, **F8** in the desktop app now does what the terminal app does: it says *No bin on this disk: delete "photo.jpg" for good?* and asks, even with *Ask before deleting* off, instead of failing to move the files to the bin. Both apps now copy, move, delete, pack and extract the same way underneath, and the desktop app measures the sizes of the folders a copy or extract changed again when it ends. [Delete](docs/files/delete.md) |
| **2.27.7** | 2026-10-18 | **A busy search helper that still answers.** While the helper makes the vectors for search by meaning (hours on the processor for a big home folder), it stops every minute to read the files you saved, so they are found by their words within a minute, not when it is done. A search by meaning during that time no longer holds up the helper's other answers, so Find, Settings and the terminal app no longer give up waiting for it. The helper counts the files for Settings and the terminal app only when the store has changed, the desktop app answers hints from memory instead of reading `state.json` each time the cursor moves, and a folder's summary in the preview counts the folder without building the whole listing. Nothing is read or made again. [Search by meaning](docs/search/meaning.md) |
| **2.27.6** | 2026-10-18 | **A lighter terminal app.** It no longer starts git for both panels at every change on disk: only a panel whose folder or repository changed reads its git line again, and never twice at once for the same folder, so a build in a big repository no longer piles up `git status` runs. Drawing the screen no longer looks at the disk, so a network share that stops answering no longer freezes it; the panel title and a link's target are read once, when you enter the folder or reach the link. Settings no longer walks the built-in models' folders twice at every key, and Find works out whether Ask is slow once a second instead of at every redraw. Less work for the processor while you wait. [When it updates](docs/panels/git.md#when-it-updates) |
| **2.27.5** | 2026-10-18 | **Fixes for quitting, hidden files, database previews and model servers.** In the terminal app, **F10** while a copy or move is still running says so, and a second **F10** quits at once, so a copy stuck on a network disk that no longer answers cannot keep you in; *Show hidden files* in Settings now shows or hides them straight away; swapping the panels with **Ctrl+U** no longer lets one panel's folder sizes stop the other's; and a closed terminal no longer leaves it spinning after **Ctrl+O**. A database preview stops counting after two and a half seconds for the whole file, not per table, so a big one no longer holds up **F3**. A config.toml from 1.x that writes `keys = { … }` or `search = { … }` on one line is updated like the others. Search by meaning stops a pass at the first expired key, missing model, *too many requests* or server error, instead of asking again for every file, and the error goes away once a search is answered again. In the desktop app a slow folder can no longer show up after you have moved on to another, nor a folder's summary in the preview under the wrong one. The ZFS snapshot difference is capped in time and size. [Copy](docs/files/copy.md) · [Servers](docs/search/servers.md) |
| **2.27.4** | 2026-10-18 | **Nothing lost to a near miss.** Duplicates no longer calls two folders identical when one has a `.git` or `.env` the other lacks (with hidden files off they were not compared), so a folder can no longer go to the bin with its repository. A file changed twice within a second is hashed again, not taken for its old contents. Batch rename stops instead of writing over a file that appeared after the preview. Copy and move refuse a folder into itself also through a link (FreeBSD's `/home`), and so does a rename inside an archive. `state.json` is saved safely when the terminal and the desktop app save at once; one that cannot be read is no longer replaced by defaults, and a second damaged one no longer replaces the first `.bad`. The desktop app no longer undoes hint counts, *Reset hints* or notices dismissed by the terminal app. inout 0.2.4 replaces the withdrawn 0.2.3. [Duplicates](docs/files/duplicates.md) |
| **2.27.3** | 2026-10-18 | **File names stay file names, whatever your shell.** F2's entries, **F3** and **F4** with a viewer or editor of your own, **Enter** on a program and a command of your own in `[git] tool` now run in `sh` in both apps, not in your `$SHELL`: with fish (or nushell, xonsh, elvish) a file named `x\';echo hi;#` could end the quoting and run `echo hi`. The line you type on the terminal app's command line still runs in your `$SHELL`, and **Ctrl+Enter** now quotes the name it adds the fish way when that shell is fish. On Windows a quoted path such as `"C:\Users\First Last\notes.txt"` reaches the program whole, a file named `%USERPROFILE%.txt` is no longer read as another name. And `%f` gives a name that starts with `-` as `./-name`, so no program takes it for an option. [User menu](docs/commands/user-menu.md#settings-and-configtoml) |
| **2.27.2** | 2026-10-18 | **Previews that cannot be turned against you.** A file named `.pdf` that is really a web page no longer runs as one in the desktop app's preview: the app's file protocol never serves HTML or XML, shows such a file as plain text, opens only real PDFs in the PDF viewer and sandboxes everything else. A Markdown file, notebook, Word document or book can no longer restyle the window or cover it with its own content: its style sheets are dropped and what it draws stays inside the preview. A file over 512 MiB is never read whole into memory, and a document too large to preview is refused before it is read. [Safety, speed and limits](docs/previews/safety.md#what-a-preview-never-does) |

All versions, back to 1.0.0: [the full changelog](docs/changelog.md).

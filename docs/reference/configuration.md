[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Configuration: every key

Both apps read one TOML file, `config.toml`. Set only what you want to change; everything else
keeps its default. This page lists every key, with its type, its default, what it does, which
app reads it and which Settings item writes it.

![Settings at Looks: the languages under their regions, the For switch on Desktop app, and the theme swatches with Cyber chosen](../screenshots/settings-looks.png)
*The desktop app's Settings (**Ctrl+,**) write single values into `config.toml`. The path is shown at the bottom: "Settings are stored in /home/demo/.config/coxswain/config.toml".*

## Contents

- [The file](#the-file)
- [Renamed in 2.0](#renamed-in-20)
- [Top-level keys](#top-level-keys)
- [Glyphs](#glyphs)
- [`[keys]`](#keys)
- [`[themes.<name>]`](#themesname)
- [`[[user_menu]]`](#user_menu)
- [`[search]`](#search)
- [`[preview]`](#preview)
- [`[gui]`](#gui)
- [`[git]`](#git)
- [What has no key](#what-has-no-key)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [An example](#an-example)
- [Questions](#questions)

## The file

| | |
|---|---|
| Where | `~/.config/coxswain/config.toml` on Linux, `~/Library/Application Support/coxswain/config.toml` on macOS, `%APPDATA%\coxswain\config.toml` on Windows. `coxswain --config-path` prints it |
| Every option with its default | `coxswain --dump-config` (redirect it to a file to start from the full list) |
| Made by | You, the desktop app's [Settings](../customise/settings.md), or the `coxswain --meaning` flags ([Command-line flags](command-line-flags.md#--meaning)). Settings and the flags write single values and keep your comments and layout |
| Read by | The terminal app when it starts; the desktop app when it starts and after each change in Settings; the [search helper](../search/helper.md) when it starts (`[search]`) |

**Mistakes.** A file that is not valid TOML, a value of the wrong type (`show_hidden = "yes"`),
an unknown action in `[keys]` or a key name that does not exist (`"Ctlr+P"`) is refused:

- The terminal app does not start and says why: `coxswain: config: …` or
  `coxswain: unknown key 'Ctlr+P'` (exit status 2).
- The desktop app starts with every default and prints `coxswain: config: …; using defaults`
  on the terminal it was started from.
- Settings never write a file that would be refused: a change that would break it is not saved.

An unknown top-level or table key (a typo such as `show_hiden = false`) is ignored without a word.

## Renamed in 2.0

Coxswain 2.0 gave a few keys the words the apps use: *mark*, not select; *folder*, not
directory; and the name index's folders the names Settings shows.

| Table | 1.x | 2.0 |
|---|---|---|
| `[keys]` | `select_group` | `mark_group` |
| `[keys]` | `unselect_group` | `unmark_group` |
| `[keys]` | `invert_selection` | `invert_marks` |
| `[keys]` | `mkdir` | `new_folder` |
| `[keys]` | `dir_sizes` | `folder_sizes` |
| `[search]` | `roots` | `name_roots` |
| `[search]` | `exclude` | `name_exclude` |

**What happens on the first start of 2.0.** Both apps rewrite `config.toml` once, before they
read it: each old key gets its new name in the same place, with its value and its comments.
Where the new name is there already, it wins and the old line goes. A notice then lists what
changed: *Coxswain 2.0 renamed keys in config.toml, your comments kept: [keys] mkdir →
new_folder, [search] exclude → name_exclude.* The desktop app shows it under *Settings →
Overview* (**Show me** opens *Keys*), the terminal app once in the status line. A file kept as a link (with your dotfiles)
is rewritten where the link points.

**When the file cannot be written** (read-only, owned by another user), nothing is changed and
the app prints on the terminal it was started from which keys to rename by hand:
`coxswain: config.toml could not be updated for 2.0 (…/config.toml: Permission denied (os error 13)). Rename these keys
yourself: [keys] mkdir → new_folder.` The old names are not read any more: an old action name in
`[keys]` is an unknown action, so the terminal app does not start and the desktop app starts with
the defaults; an old `roots` or `exclude` in `[search]` is ignored, like any unknown key.

## Top-level keys

| Key | Type | Default | Does | Read by |
|---|---|---|---|---|
| `language` | string | `"auto"` | `"auto"` follows the system; or a code: `en-GB`, `en-AU`, `en-CA`, `en-NZ`, `da`, `sv`, `fi`, `et`, `lv`, `lt`, `de`, `de-AT`, `de-CH`, `fr`, `it`, `nl`, `es-AR`, `ca`, `eu`, `he`, `pl`, `cs`, `uk`, `el`, `ja`, `ko` ([Languages](../customise/languages.md)) | Both |
| `theme` | string | `"nc"` | The terminal app's theme: a built-in name or one of your `[themes.<name>]` ([Themes](../customise/themes.md)). The desktop app's is `[gui] theme` | Terminal |
| `glyphs` | string | `"nerd"` | `"nerd"` for Nerd Font glyphs, `"ascii"` for plain characters | Both |
| `glyph_set` | table | none | Your own glyphs; see [Glyphs](#glyphs). When set, it wins over `glyphs` | Both |
| `show_hidden` | bool | `true` | Show hidden files at start; **Alt+.** switches | Both |
| `folder_sizes` | bool | `true` | Measure folders in the background. The desktop app starts from it, then keeps its own switch ([Folder sizes](../panels/folder-sizes.md)) | Both |
| `editor` | string | none | The program for **F4**. Terminal app: else `$VISUAL`, `$EDITOR`, `vi` (`notepad` on Windows). Desktop app: else the default application ([View and edit](../commands/view-and-edit.md)) | Both |
| `viewer` | string | none | The terminal app's **F3**; else `$PAGER`, `less` (`more` on Windows) | Terminal |
| `bom_viewer` | bool | `true` | **F3** on a CycloneDX BOM opens the BOM viewer; `false` opens the pager ([Cryptography bills of materials](../previews/bom.md)) | Terminal |
| `confirm_delete` | bool | `true` | Ask before moving to the trash, deleting, or taking something out of an archive | Both |
| `check_updates` | bool | `true` | Look for a newer release once a day ([Update checks](updates.md)) | Both |

## Glyphs

`glyphs = "ascii"` swaps the Nerd Font glyphs for plain characters. For a set of your own, give
`[glyph_set]` the glyphs you want; a missing one takes the Nerd Font default:

```toml
[glyph_set]
branch = "git:"
ahead = "^"
behind = "v"
staged = "+"
modified = "~"
untracked = "?"
deleted = "-"
renamed = ">"
conflict = "!"
ignored = "."
stash = "$"
clean = "="
folder = "/"
file = " "
symlink = "@"
```

These are the ASCII set. The first twelve are [git](../panels/git.md); `folder`, `file` and
`symlink` mark entries. More: [Glyphs and fonts](../customise/glyphs-and-fonts.md).

## `[keys]`

Action → the keys that run it, as a list of strings:

```toml
[keys]
quit = ["F10", "Ctrl+Q"]    # listing an action replaces its default keys
search = ["Ctrl+P"]
tag = []                    # [] unbinds it
```

**Key names:** a letter, digit or sign (`a`, `+`, `.`, `,`), `F1` to `F24`, `Enter` (or
`Return`), `Esc` (`Escape`), `Tab`, `Backspace`, `Delete` (`Del`), `Insert` (`Ins`), `Home`,
`End`, `PageUp` (`PgUp`), `PageDown` (`PgDn`), `Up`, `Down`, `Left`, `Right`, `Space`; with
`Ctrl+`, `Alt+` and `Shift+` in front, in any case. `A` alone means Shift+A; `Ctrl+F` means
Ctrl and the F key.

**Rules:**

- Listing an action replaces all its default keys; list the defaults again to keep them.
- A plain character bound to an action (`+`, `-`, `*`) only works while the command line is
  empty.
- A key bound to two actions runs one of them; bind each key once.
- An Alt+letter that is bound is no longer a [quick search](../panels/quick-search.md).

**The actions**, with their defaults (what each does: [Every default key](../panels/keys.md);
changing them: [Changing keys](../customise/keys.md)):

| Action | Default | Action | Default |
|---|---|---|---|
| `help` | `F1` | `goto_left` | `Alt+F1` |
| `user_menu` | `F2` | `goto_right` | `Alt+F2` |
| `view` | `F3` | `same_dir` | `Alt+O` |
| `edit` | `F4` | `sort_name` | `Ctrl+F3` |
| `copy` | `F5` | `sort_ext` | `Ctrl+F4` |
| `move` | `F6` | `sort_time` | `Ctrl+F5` |
| `new_folder` | `F7` | `sort_size` | `Ctrl+F6` |
| `delete` | `F8`, `Delete` | `copy_path` | `Ctrl+Enter`, `Ctrl+J` |
| `delete_forever` | `Shift+F8`, `Shift+Delete` | `new_tab` | `Ctrl+T` |
| `menu` | `F9` | `close_tab` | `Ctrl+W` |
| `quit` | `F10` | `next_tab` | `Ctrl+Tab` |
| `up` | `Up` | `prev_tab` | `Ctrl+Shift+Tab` |
| `down` | `Down` | `toggle_preview` | `Space` |
| `page_up` | `PageUp`, `Left` | `toggle_view` | `Alt+V` |
| `page_down` | `PageDown`, `Right` | `toggle_sidebar` | `Ctrl+B` |
| `home` | `Home` | `edit_path` | `Ctrl+L` |
| `end` | `End` | `folder_sizes` | none |
| `open` | `Enter` | `batch_rename` | `Ctrl+M` |
| `parent` | `Ctrl+PageUp`, `Backspace` | `tag` | `Alt+T` |
| `switch_panel` | `Tab` | `notes` | `Alt+N` |
| `mark` | `Insert`, `Shift+Down` | `back` | `Alt+Left` |
| `mark_group` | `+` | `forward` | `Alt+Right` |
| `unmark_group` | `-` | `clip_copy` | `Ctrl+C` |
| `invert_marks` | `*` | `clip_cut` | `Ctrl+X` |
| `search` | `Alt+F7`, `Ctrl+F` | `paste` | `Ctrl+V` |
| `search_text` | `Shift+F7`, `Ctrl+Shift+F` | `properties` | `Alt+Enter` |
| `ask` | `Ctrl+F7` | `extract` | `Ctrl+E` |
| `refresh` | `Ctrl+R` | `pack` | `Alt+F5` |
| `swap_panels` | `Ctrl+U` | `columns` | none |
| `toggle_panels` | `Ctrl+O` | `duplicates` | `Ctrl+D` |
| `toggle_hidden` | `Alt+.` | `settings` | `Ctrl+,` |
| `history` | `Ctrl+G` | `branches` | `Alt+B` |
| `worktrees` | `Alt+W` | `switch_branch` | `Alt+S` |
| `new_branch` | none | `snapshots` | `Alt+Z` |
| `flags` | none | `package` | none |

Keys inside dialogs (Enter, Esc, Tab and Shift+Tab in Find, the digits in *Colour tag*) are fixed.

## `[themes.<name>]`

Your own colour themes, one table per theme, with a `look` and up to 31 colour slots, each a
table with `fg`, `bg` (a colour name or `#rrggbb`) and `bold`. A slot you leave out keeps the
NC theme's colour. A table named like a built-in theme (`[themes.nord]`) replaces that theme
whole: its unset slots then come from NC too, not from the built-in Nord. See
[Your own theme and the colour slots](../customise/own-theme.md).

```toml
[themes.harbour]
look = "modern"
[themes.harbour.panel]
fg = "#e0e0e0"
bg = "#101820"
```

## `[[user_menu]]`

The entries of **F2**, one table each:

| Key | Type | Default | Does |
|---|---|---|---|
| `key` | string | – | The key that picks it in the menu |
| `label` | string | – | What the menu shows |
| `command` | string | – | The shell command; `%f` the file, `%d` the folder, `%s` the marked files (or the file), `%%` a `%`, all shell-quoted |
| `wait` | bool | `false` | Terminal app: wait for Enter afterwards. The desktop app always shows the output |

Without any entries the menu has four git commands: `git status` (**s**), `git log` (**l**),
`git diff (file)` (**d**), `git blame (file)` (**b**). Your entries replace all four. See
[The user menu](../commands/user-menu.md).

## `[search]`

Read by the [search helper](../search/helper.md) when it starts; Settings and the `--meaning`
flags start a new one.

| Key | Type | Default | Does |
|---|---|---|---|
| `name_roots` | list of paths | `[]` | The folders in the name index (1.x: `roots`). Empty: `/` on Linux and macOS, every drive on Windows |
| `name_exclude` | list of strings | `["/proc", "/sys", "/dev", "/run", "/tmp/.X11-unix"]` | Left out of the name index: a path skips that tree, a bare name skips every folder of that name (1.x: `exclude`) |
| `max_results` | number | `10000` | Most name hits per search (the desktop app shows 500 at most) |
| `watch` | bool | `true` | Follow changes live with the file watcher; `false`: only the hourly rebuild |
| `text` | bool | `true` | Keep the text of files, for Find's *In files* group ([Text in files](../search/text.md)) |
| `text_roots` | list of paths | `[]` | The folders whose files are read. Empty: your home folder ([Choosing the folders](../search/folders.md)) |
| `text_exclude` | list of strings | `["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash"]` | Folder names left out of reading, wherever they are. Hidden folders and folders with a `.nosearch` file always are |
| `names_only` | list of paths | `[]` | Folders found by name and counted in sizes, never read |
| `cloud` | string | `"local-only"` | Files only in OneDrive, Dropbox, Google Drive, Proton Drive, iCloud or a cloud mount: `"local-only"` finds them by name and never reads (downloads) them; `"all"` reads them ([Cloud files](../search/cloud-files.md)) |
| `cloud_read` | list of paths | `[]` | Cloud folders whose online-only files are read anyway |
| `text_max_size` | number (bytes) | `20971520` (20 MB) | Larger files are not read |
| `archives` | bool | `true` | Look inside the zip, 7z and tar archives in the folders read, less caches and build output: their files are found by name and by their text ([Inside archives](../search/archives.md)) |
| `archives_everywhere` | bool | `false` | Look inside every archive the name index sees, caches and programs' folders too, for their names ([Which archives](../search/archives.md#which-archives)) |
| `meaning` | bool | `false` | Search by meaning ([Search by meaning](../search/meaning.md)) |
| `meaning_engine` | string | `"builtin"` | `"builtin"` (the downloaded model, on this machine: a Mac's GPU through Metal when it can, the CPU elsewhere), `"ollama"` (an Ollama server's `/api/embed`) or `"openai"` (any `/v1/embeddings`: Lemonade, LM Studio, llama.cpp, vLLM) ([on a server](../search/servers.md)) |
| `meaning_url` | string | `""` | The server. Empty for Ollama on this machine (`http://localhost:11434`); for `"openai"` the base URL, for example `http://localhost:8000/api/v1` |
| `meaning_model` | string | `""` | The server's embedding model, for example `bge-m3` (the Ollama suggestion) |
| `meaning_key_env` | string | `""` | The name of the environment variable that holds the server's API key. The key itself is never in this file |
| `meaning_device` | string | `"auto"` | Where the built-in model runs: `"auto"` (a Mac's GPU through Metal when it has one whose results match the CPU's, the CPU elsewhere) or `"cpu"` (the CPU only). Only Macs have a choice ([Search by meaning](../search/meaning.md#on-a-macs-gpu)) |
| `ask_model` | string | `""` | The chat model that answers in [Ask](../search/ask.md), on the server above (Ollama on this machine with the built-in model), e.g. `qwen3:8b`. Empty: Ask is not set up |
| `ask_think` | bool | `false` | Let Ask's chat model think before it answers (Qwen3, DeepSeek-R1 …). Off: it is asked not to, so the first word comes at once ([Thinking](../search/ask.md#thinking)) |
| `history` | bool | `true` | Keep the history of the git repositories in the folders read: commit messages, authors and changed paths of the newest 2000 commits of each, found by Find's words in files and by meaning ([History in search](../search/history.md)) |

Changing `meaning_engine` or `meaning_model` makes the helper work out the vectors again,
since vectors of two models cannot be compared.

## `[preview]`

Previews made by tools, in the desktop app ([Previews made by tools](../previews/tools.md),
[Containers](../previews/containers.md), [LaTeX projects](../previews/latex.md)).

| Key | Type | Default | Does |
|---|---|---|---|
| `prefer` | string | `"auto"` | `"auto"`: an installed program first, else a container; `"local"`: installed programs (a container is listed but never the default); `"container"`: containers first |
| `prefer_tool` | table | `{}` | Per tool, overrides `prefer`: `prefer_tool = { latex = "container" }` |
| `container` | string | `"auto"` | `"auto"` (podman, else docker), `"podman"`, `"docker"`, or `"off"` for no containers |
| `images.latex` | string | `"docker.io/texlive/texlive:latest"` | The LaTeX image (about 5 GB; `:latest-medium` about 2 GB) |
| `images.plantuml` | string | `"docker.io/plantuml/plantuml:latest"` | The PlantUML image |
| `images.pandoc` | string | `"docker.io/pandoc/core:latest"` | The pandoc image, for reStructuredText |
| `images.libreoffice` | string | `""` | No official image; set one you trust that has `soffice` |
| `images.duckdb` | string | `""` | No official image; set one whose entry point is `duckdb` |
| `timeout` | number (seconds) | `120` | Seconds before a conversion is stopped; pulling an image is not counted |
| `latex_auto` | bool | `true` | Build a LaTeX document by itself when it is shown and its sources have changed |

An empty image means that tool never runs in a container. Setting one image keeps the others'
defaults. The tool names are `latex`, `libreoffice`, `plantuml`, `pandoc` and `duckdb`.

## `[gui]`

The desktop app's own settings.

| Key | Type | Default | Does |
|---|---|---|---|
| `theme` | string | `"cyber"` | The desktop app's theme |
| `font` | string | `"Inter, 'Segoe UI Variable', 'Segoe UI', system-ui, -apple-system, 'Noto Sans', sans-serif"` | The interface font, as a CSS font list. Cyber and the Windows and Mac themes bring their own |
| `icon_font` | string | `"'Symbols Nerd Font Mono', 'JetBrainsMono Nerd Font', 'MesloLGS Nerd Font', 'MesloLGM Nerd Font Mono', 'FiraCode Nerd Font', 'CaskaydiaCove Nerd Font', 'Hack Nerd Font', monospace"` | The Nerd Font for icons and git glyphs: the first installed one is used |
| `mono_font` | string | `"'JetBrains Mono', 'Cascadia Code', 'MesloLGS Nerd Font', Menlo, Consolas, monospace"` | Code, the command line, and everything in Cyber and NC |
| `font_size` | number | `13` | Text size in pixels (Settings allows 9 to 28) |
| `line_height` | number | `1.9` | Row height, as a multiple of the font size (Settings allows 1.2 to 3) |

## `[git]`

Read by both apps when they start; the desktop app also after a change in Settings.

| Key | Type | Default | Does |
|---|---|---|---|
| `last_commit` | bool | `true` | Last commit of each file: the *Last commit* column and the preview pane in the desktop app, the info line in the terminal app ([Git in the panels](../panels/git.md#last-commit-per-file)) |

The [git history](../panels/git-history.md) itself has no switch: it is there when you press
**Ctrl+G** (`history` in `[keys]`).

## What has no key

Some choices are not in `config.toml`, and that is on purpose:

| Choice | Where it lives |
|---|---|
| The LaTeX engine (XeLaTeX, LuaLaTeX, pdfLaTeX) | Picked per document: a `% !TEX program = …` line, else by the packages it loads. The engine button you last pressed in the preview (installed or a container) is remembered in the session in `state.json`. `prefer_tool = { latex = … }` sets which kind comes first ([LaTeX projects](../previews/latex.md)) |
| Archives and their passwords | Nothing to set. Passwords are kept in memory until the app quits and never written anywhere ([Passwords](../files/archive-passwords.md)) |
| Notices you dismissed | `state.json`, `notices_dismissed` ([Notices](../search/notices.md)) |
| The session, favourites, tags, notes | `state.json` ([What the apps remember](../panels/session.md)) |
| Starting the helper with your session | The system's own registration: `coxswain --index-service on` ([The search helper](../search/helper.md)) |

## Settings and config.toml

Each Settings item and the key it writes, area by area ([The Settings window](../customise/settings.md)):

| Settings area | Item | Key |
|---|---|---|
| *Finding files* | *How far should Find look?*: the four levels | `[search] text`, `meaning`, `ask_model` (a level that needs a model opens the setup guide) |
| *Finding files → Details → What is read* | *Words inside files* | `[search] text` |
| | *Look inside archives* | `[search] archives` |
| | *Archives everywhere* | `[search] archives_everywhere` |
| | *Git history* | `[search] history` |
| | *Read files that are only online* | `[search] cloud` |
| | *Cloud folders read anyway* | `[search] cloud_read` |
| | *Largest file read (MB)* | `[search] text_max_size` |
| | *Most hits* | `[search] max_results` |
| | *Programs that read more* | none: each missing program with the line that installs it and **Copy** ([Scans and pictures](../search/scans.md#installing-what-is-missing)) |
| *Finding files → Details → Folders* | *Folders read* | `[search] text_roots` |
| | *Names only* | `[search] names_only` |
| | *Left out everywhere* | `[search] text_exclude` |
| | *Where names are found* | `[search] name_roots` |
| | *Never indexed* | `[search] name_exclude` |
| | *Follow changes as they happen* | `[search] watch` |
| *Finding files → Details → Meaning* | *Turn on*, *Turn off*, *Download the model and turn on* | `[search] meaning` |
| | *Made by* | `[search] meaning_engine` |
| | *Server*, *Model*, *API key from the environment variable* | `[search] meaning_url`, `meaning_model`, `meaning_key_env` |
| | *Use the CPU only* (on a Mac) | `[search] meaning_device` |
| *Finding files → Details → Ask* | *Chat model* | `[search] ask_model` |
| | *Let the model think first* | `[search] ask_think` |
| *Finding files → Details → Background reading* | *Start with my session* | none: the session registration |
| *Previews* | *How previews are made* | `[preview] prefer` |
| | *Container runtime* | `[preview] container` |
| | *LaTeX image* | `[preview] images.latex` |
| | *Build LaTeX by itself* | `[preview] latex_auto` |
| | *Timeout (seconds)* | `[preview] timeout` |
| *Looks* | *Language* | `language` |
| | *Theme of the desktop app* (*For*: *Desktop app*) | `[gui] theme` |
| | *Theme of the terminal app* (*For*: *Terminal app*) | `theme` |
| | *Icons and git glyphs* | `glyphs` |
| | *Font*, *Monospaced font*, *Icon font* | `[gui] font`, `[gui] mono_font`, `[gui] icon_font` |
| | *Text size* | `[gui] font_size` |
| | *Row height* | `[gui] line_height` |
| *Behaviour* | *Show hidden files when Coxswain starts* | `show_hidden` |
| | *Ask before deleting* | `confirm_delete` |
| | *Measure folder sizes* | `folder_sizes` |
| | *Last commit of each file* | `[git] last_commit` |
| | *Editor* | `editor` |
| | *Viewer (terminal app)* | `viewer` |
| | *CBOM viewer on F3 (terminal app)* | `bom_viewer` |
| *Privacy and updates* | *Check for a new version* | `check_updates` |

*Keys* lists every action and its keys, read-only, with **Open config.toml**. Every other key
(`[keys]`, `glyph_set`, `[themes.*]`, `[[user_menu]]`, `prefer_tool`, the other images) has no
Settings item: edit the file.

## In the terminal app

The terminal app reads the same file, once, when it starts. It uses the top-level keys,
`glyph_set`, `[keys]`, `[themes.*]` (colours only, not `look`), `[[user_menu]]`, `[git]` and,
through the helper, `[search]`. It ignores `[preview]` and `[gui]`. Its Settings (**F9** → *Settings*,
[In the terminal app](../customise/settings.md#in-the-terminal-app)) write the file the way the
desktop app's do; or edit it, or use `coxswain --meaning …` for search by meaning
([Command-line flags](command-line-flags.md#--meaning)).

## An example

```toml
language = "auto"           # or "en-GB", "da", "de", "es-AR", "he", ...
theme = "nc"                # terminal app; or "cyber", "win95", ... or your own [themes.<name>]
glyphs = "nerd"             # or "ascii"
folder_sizes = true
editor = "hx"               # else $VISUAL / $EDITOR

[keys]
quit = ["F10", "Ctrl+Q"]
search = ["Ctrl+P"]

[themes.mine.panel]         # unset slots fall back to the NC scheme
fg = "#e0e0e0"
bg = "#101820"

[[user_menu]]               # F2; %f file, %d folder, %s the marked files (shell-quoted)
key = "t"
label = "cargo test"
command = "cargo test"
wait = true

[search]
name_exclude = ["/proc", "/sys", "/dev", "/run", "node_modules"]
names_only = ["/home/me/Mail"]
meaning = true
meaning_engine = "openai"
meaning_url = "http://localhost:8000/api/v1"
meaning_model = "nomic-embed-text-v1-GGUF"

[preview]
prefer = "container"        # use podman/docker even when a tool is installed
images.latex = "docker.io/texlive/texlive:latest-medium"
latex_auto = false

[gui]
theme = "win95"
font_size = 15
```

## Questions

#### Coxswain 2.0 changed my `config.toml`. What happened?

It renamed seven keys to the words the apps use (`mkdir` → `new_folder`, `[search] exclude` →
`name_exclude` …), once, keeping your values, comments and order. The notice lists them. See
[Renamed in 2.0](#renamed-in-20).

#### I keep my config in git for 1.x and 2.0 machines. What now?

2.0 does not read the old names, and 1.x does not know the new ones: a 1.x app ignores
`name_roots`, and stops (terminal) or uses the defaults (desktop) on `new_folder`. Update every
machine to 2.0, or keep the `[keys]` you change on the 1.x machines out of the shared file.

#### I edited `config.toml` and nothing changed.

The terminal app reads it when it starts: quit with **F10** and start it again. The desktop app
reads it at start and after a change in Settings: restart it. `[search]` is read by the helper
when it starts: see [The search helper](../search/helper.md#questions).

#### How do I go back to the defaults?

Delete the key (or the whole file). `coxswain --dump-config` shows what the default is.

#### Is it safe to put `--dump-config`'s output in my config?

Yes, but you then fix every default, including the full built-in themes and every key; later
versions' new defaults (a new action's key, a changed theme) will not reach you. Better: keep
only what you change.

#### I misspelt a key and Coxswain said nothing.

Only mistakes that make the file unreadable are refused: bad TOML, a wrong type, an unknown action
or key name in `[keys]`. A misspelt setting name such as `show_hiden` is ignored and the default
stays. Compare with `coxswain --dump-config`.

#### Will Settings mess up my comments?

No. Settings and the `--meaning` flags change only the one value, in place, and keep your
comments, order and blank lines. A new key is added at the end of its table.

#### Where do I choose the LaTeX engine?

Not in `config.toml`. The document chooses (`% !TEX program = xelatex`, or its packages), and in
the preview you pick installed or container with the engine buttons; that choice is remembered in
the session. See [What has no key](#what-has-no-key).

#### Can one config serve both apps with different themes?

Yes: `theme` is the terminal app's, `[gui] theme` the desktop app's. Everything else is shared.

---
[← Previous: Command-line flags](command-line-flags.md) · [Next: Privacy →](privacy.md)

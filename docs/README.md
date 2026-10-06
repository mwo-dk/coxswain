[← README](../README.md)

# Coxswain documentation

Every feature of both apps, the terminal app (`coxswain`) and the desktop app (`coxswain-gui`), has its own page: what it does, how to use it with the exact keys in both apps, what you see on screen, its settings and `config.toml` keys, how the terminal app differs, and the questions people ask. Each area has an index of its own. Pages end with *Previous* and *Next* links in the order below, so the docs can be read from start to end.

New here? Start with [The first-run guide](panels/first-run.md), [The screen](panels/the-screen.md) and [Every default key](panels/keys.md). Coming from 1.x? Read [What's new in 2.0](whats-new-2.md). Puzzled? See [Questions, collected](faq.md).

## [Panels and keys](panels/README.md)

| Page | What it covers |
|---|---|
| [Panels and keys](panels/README.md) | The area's pages and the main keys of both apps |
| [The first-run guide](panels/first-run.md) | The four steps shown on the first start (panels and keys, how far Find looks, looks with the Nerd Font check, privacy), skipping, opening it again (F1, Settings → Overview) |
| [The screen](panels/the-screen.md) | Panels, active panel, title bar with the version, status line and notices, F-key bar, archives shown in the pane, starting in a folder |
| [Moving around and going to a folder](panels/moving.md) | Cursor keys, Enter, parent, other panel, refresh, Alt+F1/Alt+F2, Ctrl+L, cd |
| [Quick search](panels/quick-search.md) | Alt+letter to jump to a name |
| [Marking files](panels/marking.md) | Insert, + - *, patterns, marking with the mouse, what uses the marks |
| [Sorting and hidden files](panels/sorting.md) | Ctrl+F3 to Ctrl+F6, headers, the rules, Alt+. and show_hidden |
| [Tabs, back and forward, one pane or two](panels/tabs-and-panes.md) | Tabs, back and forward, one pane or two (Ctrl+O), the splitter |
| [Views: details, columns, thumbnails](panels/views.md) | Details, Miller columns, thumbnails, the columns menu, the age chip |
| [Folder sizes](panels/folder-sizes.md) | Where they come from, how fresh, measuring again, switching off |
| [Git in the panels](panels/git.md) | The git line (worktree, detached HEAD, merge or rebase under way), file glyphs, the last commit per file, the diff, recent repositories, user menu commands |
| [Git history as folders](panels/git-history.md) | Ctrl+G, the commits of a file or folder, the files at a commit, preview and diff, F3, F5, read-only, limits |
| [Git branches and worktrees](panels/git-branches.md) | Alt+B, the branches, a branch's files, switching (Alt+S) and new branches, Alt+W, the worktrees, limits |
| [The mouse](panels/mouse.md) | Clicks, marks, drags, path bar, tabs, splitters, in both apps |
| [The command list (F9) and Help (F1)](panels/command-list.md) | The command list (F9) and Help (F1) |
| [What the apps remember](panels/session.md) | The desktop session, state.json, what is forgotten |
| [Every default key](panels/keys.md) | Every default key of both apps, keys that are not actions, keys inside dialogs |

## [Tags, notes, favourites and the sidebar](organise/README.md)

| Page | What it covers |
|---|---|
| [Tags, notes, favourites and the sidebar](organise/README.md) | The area, and its keys in both apps |
| [Colour tags](organise/tags.md) | Alt+T, seven colours, the dot after the name, tags belong to a path |
| [Folder notes](organise/notes.md) | Alt+N, the notes field in the preview pane, saved on leaving it, the footer icon |
| [Favourites](organise/favourites.md) | Named groups in the sidebar, add with +, remove with ×, rename and delete groups |
| [The sidebar](organise/sidebar.md) | Ctrl+B, places, drives with free space, favourites, twelve recent git repositories |

## [Commands, the user menu and scripts](commands/README.md)

| Page | What it covers |
|---|---|
| [Commands, the user menu and scripts](commands/README.md) | The area, and its keys in both apps |
| [The command line and its output](commands/command-line.md) | The command line: typing, cd, Ctrl+Enter, the shell, output in the terminal (Ctrl+O) and in the preview pane |
| [The user menu, F2](commands/user-menu.md) | The user menu (F2): [[user_menu]] entries, keys, %f %d %s %%, wait, the four git defaults |
| [Scripts](commands/scripts.md) | The desktop app's scripts folder, run from F2 with the marked files as arguments |
| [View and edit, F3 and F4](commands/view-and-edit.md) | View and edit (F3, F4): viewer, editor, $PAGER, $VISUAL, $EDITOR, the preview pane in the desktop app |
| [Opening files](commands/opening-files.md) | Enter and double-click on folders, archives, programs and files, default applications |

## [Search](search/README.md)

| Page | What it covers |
|---|---|
| [Search](search/README.md) | The groups of Find, all pages, keys at a glance |
| [Smart search in a few minutes](search/setup.md) | The guided setup (Settings → Set up…, `coxswain --setup-search`): parts, hardware table, Ollama, Lemonade, LM Studio, llama.cpp, the GPU check, start with my session |
| [Find](search/find-file.md) | One field for names, words, meaning and Ask: the groups and their order, the kinds (Tab) and prefixes, the scope (Ctrl+F inside Find), the Ask row and the answer in place, every key and state |
| [Names everywhere](search/names.md) | The name index, how it stays current and fast, the scope chip for this folder, exclude/roots/watch |
| [Name syntax](search/name-syntax.md) | Everything's words, !, |, wildcards, ext:, file:, folder:, case:, paths, quotes |
| [Text in files](search/text.md) | The *In files* group: how words match (every word, then any of them), the passages, the scope, what is read and when, search.db |
| [Documents it reads](search/documents.md) | PDF, Word, RTF, spreadsheets, slides, mail, books, notebooks, diagrams, Markdown |
| [Scans, pictures and older Office files](search/scans.md) | Tesseract, pdftoppm, LibreOffice and how to install them |
| [Diagrams read as sentences](search/diagrams.md) | Draw.io, Mermaid, Graphviz, PlantUML, with examples and limits |
| [Git history in search](search/history.md) | Commit messages, authors and changed paths of your repositories, found by text and meaning, opened at the commit |
| [Inside archives](search/archives.md) | Files in zip, 7z and tar found by name, text, meaning and Ask; which archives, changes, locked ones, limits |
| [Choosing the folders: folders read, names only, .nosearch](search/folders.md) | Folders read, Names only, .nosearch, text_exclude |
| [Cloud files: OneDrive, Dropbox, Google Drive, Proton Drive, iCloud](search/cloud-files.md) | Online-only files found by name and never downloaded, the cloud glyph, Download and preview, `cloud` and `cloud_read`, per system |
| [Removable disks](search/removable-disks.md) | Text kept while unplugged, found again at any mount point |
| [Search by meaning](search/meaning.md) | The built-in multilingual model, turning it on and off, what you see, how it works |
| [Search by meaning on a server: Ollama, Lemonade, LM Studio](search/servers.md) | Ollama, Lemonade, LM Studio, any OpenAI-compatible server, API keys, privacy |
| [Ask: questions answered from your files](search/ask.md) | The Ask row of Find (Ctrl+Enter): your chat model answers from the closest passages, citing them, in place of the list |
| [Ask without a server: the built-in chat model](search/ask-builtin.md) | Qwen3 1.7B or 4B Instruct built in: the download, Metal on a Mac and the processor elsewhere, loaded in the helper and let go when idle, speed, `builtin:` in `ask_model` |
| [The search helper](search/helper.md) | One process for all windows, privacy of its connection, starting with the session, taken over after an upgrade moves the program |
| [Battery](search/battery.md) | Reading pauses on battery, Read now reads anyway |
| [Notices and what's new](search/notices.md) | The version in the title, the tips, the count on Settings, *What's new* and `coxswain --whats-new` |
| [Search settings](search/settings.md) | Every item of Settings → Finding files (the status, the levels, Details), its config.toml key, and the terminal flags |

## [The preview pane](previews/README.md)

| Page | What it covers |
|---|---|
| [The preview pane](previews/README.md) | Opening and closing it, what it shows, the switches, every format with its page, keys; desktop-only and why |
| [Text and code](previews/text-and-code.md) | Highlighted code, logs, hex dumps, the git diff (File / Diff), Markdown with Mermaid and math |
| [Documents: PDF, Word, EPUB, notebooks, mail, calendars](previews/documents.md) | PDF, Word .docx, EPUB, Jupyter notebooks, e-mail, AsciiDoc, reStructuredText, calendars and contacts |
| [HTML pages, sandboxed](previews/html.md) | HTML files shown as sandboxed pages: what is allowed and blocked, scripts, the network, Rendered / Source |
| [PowerPoint and Office: quick view and LibreOffice's exact view](previews/office.md) | PowerPoint quick view drawn in the app, LibreOffice's exact view after it, and every other Office format |
| [Data: trees, spreadsheets, databases and certificates](previews/data.md) | JSON/YAML/TOML trees, JSON Lines, spreadsheets and CSV, SQLite, Parquet, DuckDB, certificates, property lists |
| [Cryptography bills of materials](previews/bom.md) | CycloneDX CBOMs as a rated tree or sunburst, with filters and a compare of two scans, in both apps |
| [Media and files: pictures, video, audio, fonts, archives, folders](previews/media.md) | Pictures, video, audio, fonts, archives listed (and files inside archives), folders, facts under a file |
| [Diagrams: draw.io, Mermaid, Graphviz and PlantUML](previews/diagrams.md) | Draw.io, Mermaid, Graphviz and PlantUML in the preview |
| [LaTeX projects](previews/latex.md) | LaTeX: which file is the document, the project, the engine, building by itself, another engine when one fails, errors |
| [Previews made by tools](previews/tools.md) | The engine buttons, how rendering works, the preview cache and Clear, [preview] |
| [Containers](previews/containers.md) | Podman or docker: what a container may do, pulling and removing images, images in Settings |
| [Safety, speed and limits](previews/safety.md) | What a preview never does, what reaches the network, speed, and every size limit |

## [Files](files/README.md)

| Page | What it covers |
|---|---|
| [Files](files/README.md) | The area, every page, the file keys in both apps |
| [Copy (F5), and when something goes wrong](files/copy.md) | Copy (F5): targets, never overwriting, merged folders, what a failure says |
| [Move and rename (F6)](files/move-and-rename.md) | Moving, renaming in place, across disks, into archives |
| [New folder (F7)](files/new-folder.md) | One folder or a whole path, also inside archives |
| [Delete: trash (F8) or for good (Shift+F8)](files/delete.md) | Trash (F8), for good (Shift+F8), taking out of an archive, turning the question off |
| [Clipboard: Ctrl+C, Ctrl+X, Ctrl+V](files/clipboard.md) | Ctrl+C / Ctrl+X / Ctrl+V shared with other file managers, name (2) |
| [Drag and drop](files/drag-and-drop.md) | Out to other apps, between panes, in; the copy/move menu |
| [Batch rename (Ctrl+M)](files/batch-rename.md) | Batch rename (Ctrl+M, desktop): regex, groups, {n:3} counter, live preview, conflicts |
| [Archives as folders](files/archives.md) | Zip/tar/7z opened with Enter; copy in, out, between; formats; what is rewritten; why not RAR; limits |
| [Pack (Alt+F5) and extract (Ctrl+E)](files/pack-and-extract.md) | Pack (Alt+F5) into any archive format by its name, extract (Ctrl+E) into a new folder |
| [Passwords for encrypted zip and 7z](files/archive-passwords.md) | Passwords for locked zip and 7z: when asked, kept in memory for the app run, never saved |
| [Properties and permissions](files/properties.md) | Properties (Alt+Enter, both apps): size, dates, owner, permissions and read-only; ZFS dataset, FreeBSD package, file flags (chflags) |
| [Finding duplicates](files/duplicates.md) | Files and whole folders, across disks, safe removal to the trash |
| [ZFS snapshots as folders](files/zfs-snapshots.md) | Alt+Z: a dataset's snapshots, newest first, browsed read-only; diff against now, F5 copies back; the ZFS line in the footer |

## [Customising](customise/README.md)

| Page | What it covers |
|---|---|
| [Customising](customise/README.md) | The Settings window, themes, looks, languages, keys, glyphs and fonts, with the main keys |
| [The Settings window](customise/settings.md) | Opening it (Ctrl+, F9, --settings=search|search_meaning), its areas from Overview to Privacy and updates, Find a setting, every item with its config.toml key, how saving keeps comments, the terminal app's Settings (F9, --settings) and flags |
| [Themes](customise/themes.md) | The 18 built-in themes, picking one in each app (Settings, F9 "Theme: …", `theme` / `[gui] theme`), what the terminal app takes |
| [Looks](customise/looks.md) | The desktop app's corners, bevels, title bars and era fonts per theme, `look =` in your own theme |
| [Your own theme and the colour slots](customise/own-theme.md) | Your own theme: `[themes.<name>]`, starting from --dump-config, colour names vs #rrggbb, every colour slot and where each app uses it |
| [Languages](customise/languages.md) | The 29 languages with flags, by region, how Automatic picks one, right to left in Hebrew and Persian, what is translated, improving a translation |
| [Changing keys](customise/keys.md) | `[keys]`, key names, rules, every action with its default, keys that cannot be changed, terminal limits |
| [Glyphs and fonts](customise/glyphs-and-fonts.md) | Nerd Font or ASCII glyphs, `[glyph_set]`, file icons, the interface/monospaced/icon fonts, text size and line height |

## [Reference](reference/README.md)

| Page | What it covers |
|---|---|
| [Reference](reference/README.md) | The area index, its pages and the keys that reach them |
| [The terminal app](reference/terminal-app.md) | What it has, what only the desktop app has and why, differences, archives and search by meaning in the terminal, the terminal it needs |
| [Command-line flags](reference/command-line-flags.md) | Every flag of coxswain and coxswain-gui: folders, --paths, --dump-config, --config-path, --meaning, --index-service, --settings, --duplicates, --index-helper, exit status |
| [Configuration: every key](reference/configuration.md) | Config.toml: every key with type, default and reader; which Settings item writes it; what has no key |
| [Privacy: what stays, what can leave](reference/privacy.md) | What stays on your machine, every case where something can leave it, and how to stop it |
| [Update checks](reference/updates.md) | The daily update check: what it sends, what each app shows, the upgrade command, turning it off |
| [Where things are kept](reference/where-things-are-kept.md) | Config, state and cache folders on each system, every file in them, what is safe to delete, removing everything |
| [Security](reference/security.md) | Dependencies: how they are chosen and updated, advisory and licence checks on every change and weekly, the bundled viewers, checking a download, reporting a problem |
| [Licences and bills of materials](reference/bills-of-materials.md) | The SBOMs, the CBOM and the third-party notices each release carries, the licence checks, the cryptography both apps use |
| [Performance](reference/performance.md) | What keeps each app quick, the numbers for 100,000 files and a million names, and how to measure again |
| [FreeBSD](reference/freebsd.md) | Both apps on FreeBSD: one-line install, packages, the search helper without systemd (autostart, rc.d, login shell), search by meaning, ZFS snapshots and facts, a file's package, file flags, boot environments and jails, differences, updating, the desktop app's experimental status, other BSDs |
| [Flatpak](reference/flatpak.md) | The desktop app as a Flatpak (io.github.mwo_dk.Coxswain): --filesystem=host and flatpak-spawn, the editor, commands and preview programs on the host, the search helper inside the sandbox, autostart, its own config folder, updating, building with flatpak-builder |
| [Termux on Android](reference/termux.md) | The terminal app in Termux: `pkg install coxswain` (or `cargo install coxswain` until the package is in), the phone's folders with termux-setup-storage, F-keys on the extra keys row, updating, the package recipe and its CI build |
| [macOS](reference/macos.md) | The folder prompts (TCC) and what to answer, what is never read on a Mac (~/Library but iCloud Drive, Photos and Music libraries), Full Disk Access, ad-hoc signing and asking again after updates, search by meaning on the GPU without a server |

## [Questions, collected](faq.md)

The common questions from every page, linked to their full answers.

Installing, and building from source: [the README](../README.md#install) and [install/INSTALL.md](../install/INSTALL.md).

---
[← Previous: README](../README.md) · [Next: Panels and keys →](panels/README.md)

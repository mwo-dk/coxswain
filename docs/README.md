[← README](../README.md)

# Coxswain documentation

Every feature of both apps, the terminal app (`coxswain`) and the desktop app (`coxswain-gui`), has its own page: what it does, how to use it with the exact keys in both apps, what you see on screen, its settings and `config.toml` keys, how the terminal app differs, and the questions people ask. Each area has an index of its own. Pages end with *Previous* and *Next* links in the order below, so the docs can be read from start to end.

New here? Start with [The screen](panels/the-screen.md) and [Every default key](panels/keys.md). Puzzled? See [Questions, collected](faq.md).

## [Panels and keys](panels/README.md)

| Page | What it covers |
|---|---|
| [Panels and keys](panels/README.md) | The area's pages and the main keys of both apps |
| [The screen](panels/the-screen.md) | Panels, active panel, title bar with version and search depths, status line and notices, F-key bar, archives shown in the pane, starting in a folder |
| [Moving around and going to a folder](panels/moving.md) | Cursor keys, Enter, parent, other panel, reread, Alt+F1/Alt+F2, Ctrl+L, cd |
| [Quick search](panels/quick-search.md) | Alt+letter to jump to a name |
| [Marking files](panels/marking.md) | Insert, + - *, patterns, marking with the mouse, what uses the marks |
| [Sorting and hidden files](panels/sorting.md) | Ctrl+F3 to Ctrl+F6, headers, the rules, Alt+. and show_hidden |
| [Tabs, back and forward, one pane or two](panels/tabs-and-panes.md) | Tabs, back and forward, one pane or two (Ctrl+O), the splitter |
| [Views: details, columns, thumbnails](panels/views.md) | Details, Miller columns, thumbnails, the columns menu, the age chip |
| [Folder sizes](panels/folder-sizes.md) | Where they come from, how fresh, measuring again, switching off |
| [Git in the panels](panels/git.md) | The git line, file glyphs, the diff, recent repositories, user menu commands |
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
| [Search](search/README.md) | The three depths, all pages, keys at a glance |
| [Find file](search/find-file.md) | Opening it, the three depth buttons, keys, count line, hints and the meaning tip |
| [Names everywhere](search/names.md) | The name index, how it stays current and fast, names in this folder, exclude/roots/watch |
| [Name syntax](search/name-syntax.md) | Everything's words, !, |, wildcards, ext:, file:, folder:, case:, paths, quotes |
| [Text in files](search/text.md) | How words match, the passages, what is read and when, search.db |
| [Documents it reads](search/documents.md) | PDF, Word, RTF, spreadsheets, slides, mail, books, notebooks, diagrams, Markdown |
| [Scans, pictures and older Office files](search/scans.md) | Tesseract, pdftoppm, LibreOffice and how to install them |
| [Diagrams read as sentences](search/diagrams.md) | Draw.io, Mermaid, Graphviz, PlantUML, with examples and limits |
| [Inside archives](search/archives.md) | Files in zip, 7z and tar found by name, text, meaning and Ask; which archives, changes, locked ones, limits |
| [Choosing the folders: folders read, names only, .nosearch](search/folders.md) | Folders read, Names only, .nosearch, text_exclude |
| [Removable disks](search/removable-disks.md) | Text kept while unplugged, found again at any mount point |
| [Search by meaning](search/meaning.md) | The built-in multilingual model, turning it on and off, what you see, how it works |
| [Search by meaning on a server: Ollama, Lemonade, LM Studio](search/servers.md) | Ollama, Lemonade, LM Studio, any OpenAI-compatible server, API keys, privacy |
| [Ask: questions answered from your files](search/ask.md) | The fourth depth of Find file: your chat model answers from the closest passages, citing them |
| [The search helper](search/helper.md) | One process for all windows, privacy of its connection, starting with the session |
| [Battery](search/battery.md) | Reading pauses on battery, Index now reads anyway |
| [Notices and the window title](search/notices.md) | Version and search depths in the title, the four notices |
| [Search settings](search/settings.md) | Every item of Search inside files and Search by meaning, its config.toml key, and the terminal flags |

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
| [Copy (F5), and when something goes wrong](files/copy.md) | Copy (F5): targets, never overwriting, merged folders, the "Something went wrong" list |
| [Move and rename (F6)](files/move-and-rename.md) | Moving, renaming in place, across disks, into archives |
| [New folder (F7)](files/new-folder.md) | One folder or a whole path, also inside archives |
| [Delete: trash (F8) or for good (Shift+F8)](files/delete.md) | Trash (F8), for good (Shift+F8), taking out of an archive, turning the question off |
| [Clipboard: Ctrl+C, Ctrl+X, Ctrl+V](files/clipboard.md) | Ctrl+C / Ctrl+X / Ctrl+V shared with other file managers, name (2) |
| [Drag and drop](files/drag-and-drop.md) | Out to other apps, between panes, in; the copy/move menu |
| [Batch rename (Ctrl+M)](files/batch-rename.md) | Batch rename (Ctrl+M, desktop): regex, groups, {n:3} counter, live preview, conflicts |
| [Archives as folders](files/archives.md) | Zip/tar/7z opened with Enter; copy in, out, between; formats; what is rewritten; why not RAR; limits |
| [Pack (Alt+F5) and extract (Ctrl+E)](files/pack-and-extract.md) | Pack (Alt+F5) into any archive format by its name, extract (Ctrl+E) into a new folder |
| [Passwords for encrypted zip and 7z](files/archive-passwords.md) | Passwords for locked zip and 7z: when asked, kept in memory for the app run, never saved |
| [Properties and permissions](files/properties.md) | Properties (Alt+Enter, desktop): size, dates, owner, permissions and read-only |
| [Finding duplicates](files/duplicates.md) | Files and whole folders, across disks, safe removal to the trash |

## [Customising](customise/README.md)

| Page | What it covers |
|---|---|
| [Customising](customise/README.md) | The Settings window, themes, looks, languages, keys, glyphs and fonts, with the main keys |
| [The Settings window](customise/settings.md) | Opening it (Ctrl+, F9, --settings=search|meaning), every section and item with its config.toml key, how saving keeps comments, terminal-app flags |
| [Themes](customise/themes.md) | The 18 built-in themes, picking one in each app (Settings, F9 "Theme: …", `theme` / `[gui] theme`), what the terminal app takes |
| [Looks](customise/looks.md) | The desktop app's corners, bevels, title bars and era fonts per theme, `look =` in your own theme |
| [Your own theme and the colour slots](customise/own-theme.md) | Your own theme: `[themes.<name>]`, starting from --dump-config, colour names vs #rrggbb, every colour slot and where each app uses it |
| [Languages](customise/languages.md) | The 18 languages with flags, how Automatic picks one, right to left in Hebrew, what is translated, improving a translation |
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

## [Questions, collected](faq.md)

The common questions from every page, linked to their full answers.

Installing, and building from source: [the README](../README.md#install) and [install/INSTALL.md](../install/INSTALL.md).

---
[← Previous: README](../README.md) · [Next: Panels and keys →](panels/README.md)

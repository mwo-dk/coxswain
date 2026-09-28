# The preview pane

The desktop app's preview pane (**Space**, or F3) shows the file or folder under the cursor
and follows it as you move. It waits a moment before loading, so holding an arrow key stays
smooth. This page covers everything it can show, the switches at its top, and the limits. The
terminal app has no preview pane: F3 opens the file in your viewer (`viewer` in the config,
else `$PAGER`, else `less`).

![Markdown with a Mermaid diagram and math, a Jupyter notebook, a spreadsheet, a Word document and a font](screenshots/gui-previews.png)
![A YAML tree, a SQLite database, a certificate and an e-mail](screenshots/gui-previews-data.png)
![A calendar, a git diff, a log file and an EPUB book](screenshots/gui-previews-more.png)

## Switches

Where a file can be shown more than one way, buttons at the top right of the preview choose.
Your choice sticks: it applies to the next file of that kind too, and survives a restart.

| Switch | Shown for | Choices |
|---|---|---|
| **File / Diff** | Files git sees as changed (modified, staged, renamed, deleted, conflicted) | The file, or `git diff HEAD` for it: staged and unstaged changes together, added lines green, removed lines red |
| **Rendered / Source** | Markdown, Mermaid, calendars, contacts | The rendered view, or the highlighted source |
| **Tree / Source** | JSON, YAML, TOML | A collapsible tree (the first two levels open), or the source |
| **Table / Source** | JSON Lines | A table, or the source |
| **Sheet names** | Workbooks with more than one sheet | One button per sheet |

## Formats

### Text and code

| Files | Preview |
|---|---|
| Source code, config, plain text | Syntax highlighted by [highlight.js](https://highlightjs.org/) (its "common" set of about 36 languages). The language comes from the extension, otherwise it is detected. Colors come from the current theme, so every theme matches |
| `.log` | Each line colored by its level: errors red, warnings yellow, info green, debug grey |
| Binary files | A hex dump |

The first 512 KB is shown. Files under 200 KB are highlighted; larger ones stay plain so
scrolling stays smooth.

### Documents

| Files | Preview | Done by |
|---|---|---|
| `.md`, `.markdown` | Rendered Markdown, with ```` ```mermaid ```` blocks drawn as diagrams and `$…$` / `$$…$$` math typeset | [marked](https://marked.js.org/), [Mermaid](https://mermaid.js.org/), [KaTeX](https://katex.org/) |
| `.mmd`, `.mermaid` | The diagram | Mermaid |
| `.ipynb` | Jupyter notebook: Markdown cells, highlighted code with its `In [n]` number, and outputs (text, errors, tables, PNG and SVG plots) | marked, highlight.js |
| `.docx` | The Word document: headings, paragraphs, lists, tables and embedded images | [mammoth](https://github.com/mwilliamson/mammoth.js) |
| `.pdf` | The PDF, with paging and zoom | The webview's own PDF viewer |
| `.epub` | The book's title and its first chapter with real text (covers and title pages are skipped) | Read in Rust |
| `.eml` | Subject, sender, recipients, date, attachments with sizes, and the text of the message | [mail-parser](https://crates.io/crates/mail-parser) |

### Data

| Files | Preview | Done by |
|---|---|---|
| `.json`, `.geojson`, `.yaml`, `.yml`, `.toml` | A collapsible tree with counts per level; strings, numbers and booleans colored | Built in, [yaml](https://eemeli.org/yaml/), [smol-toml](https://github.com/squirrelchat/smol-toml) |
| `.jsonl`, `.ndjson` | The first 200 lines as a table, one column per key | Built in |
| `.csv`, `.tsv`, `.xlsx`, `.xlsm`, `.xls`, `.ods` | The first 200 rows as a table, with a button per sheet | [SheetJS](https://sheetjs.com/) |
| `.db`, `.sqlite`, `.sqlite3`, `.db3` | Every table and view with its row count; click a name for its schema | [SQLite](https://sqlite.org/), opened read-only |
| `.ics` | The events: title, start and end, place, description | Built in |
| `.vcf` | The contact cards: name, title, organization, e-mail, phone | Built in |
| `.plist` | Binary or XML property lists, as XML | [plist](https://crates.io/crates/plist) |

### Media and files

| Files | Preview |
|---|---|
| Images (`png jpg gif webp bmp ico svg avif`) | The image on a checkerboard, plus its EXIF facts |
| Video (`mp4 webm mkv mov m4v ogv`) | A player |
| Audio (`mp3 flac wav ogg m4a opus aac`) | A player, plus its tags |
| Fonts (`.ttf`, `.otf`, `.woff`, `.woff2`) | Sample text at several sizes, the alphabet, digits and some accented letters |
| `.pem`, `.crt`, `.cer`, `.der` | Each certificate in the file (a chain shows all of them): subject, issuer, validity, the names it covers, serial number. Expiry within 30 days shows in yellow, an expired certificate in red |
| `.zip`, `.jar`, `.apk`, `.nupkg`, `.whl`, `.vsix`, `.tar`, `.tar.gz`, `.tgz` | The list of files inside. **Ctrl+E** extracts into a new folder in the other pane; it never writes over an existing folder or outside it |
| Folders | Number of folders and files, the size of the files directly inside, the newest change, the total size on request (Ctrl+Space), and the git line |

### Facts

Under the preview, some files get a short list of facts:

- **Photos:** camera, lens, date taken, exposure, aperture, ISO, focal length, and GPS
  position as decimal degrees (paste them into any map).
- **Audio:** title, artist, album, year, track, genre, length, bitrate, sample rate, channels.
- **Programs and libraries:** the platform and CPU they are built for (Linux ELF, Windows PE,
  macOS Mach-O including universal binaries; x86, x86-64, ARM, ARM64, RISC-V and more) and
  what they are (program, windowed or console program, shared library, object file).

## Columns and folder sizes

Right-click the column header in the details view, or pick **Columns and folder sizes** in
F9, to choose the columns:

| Column | Shows |
|---|---|
| Type | The extension, or "Folder" |
| Size | File size; for folders, once measured |
| Files | For folders: how many files are inside, all levels down |
| Modified | The age chip (red within the hour, fading to grey after a year) and the date |
| Created | The creation date, where the file system records it |

**Measure folder sizes automatically**, in the same menu, fills in Size and Files for every
folder as a folder opens. Folders are measured one at a time in the background, so the first
ones appear quickly; leaving the folder stops the work. Without it, Ctrl+Space measures the
marked folders (or all of them) on request. Narrow panes drop columns: Type, Files and
Created first, then the date part of Modified, then Size.

![The details view with every column and measured folder sizes](screenshots/gui-folder-sizes.png)

## Safety

Previews show other people's files, so:

- Every rendered result is HTML sanitized with [DOMPurify](https://github.com/cure53/DOMPurify)
  before it reaches the page. That covers Markdown, notebooks, Word documents, EPUB chapters
  and HTML notebook outputs. Scripts never run.
- Mermaid runs with `securityLevel: "strict"`.
- SQLite databases open read-only, and counting rows stops after about a second per table.
- Previews never write anything. The only thing that does is extracting an archive, and only
  when you press Ctrl+E.
- Nothing is sent over the network; every library is bundled with the app.

## Speed

The libraries behind the heavier previews (Mermaid, KaTeX, SheetJS, mammoth, the YAML and
TOML parsers) load the first time a file needs them, so they add nothing to startup. Files
bigger than 25 MB are not rendered as documents or spreadsheets.

## Adding a format

- A format that is easy in the browser goes into `gui/src/renderers.js`: a function that
  returns sanitized HTML or plain data.
- One that needs native code goes into `gui/src-tauri/src/preview.rs`, as a Tauri command.
- `previewKind` in `gui/src/lib.js` maps extensions to a kind.
- `gui/src/Preview.svelte` shows each kind.

For screenshots, `docs/screenshots/demo-docs.py` writes a demo file for each format, and
[the README](../README.md) describes the sandbox the screenshots are taken in.

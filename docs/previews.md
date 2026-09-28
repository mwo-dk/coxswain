# The preview pane

The desktop app's preview pane (**Space**, or F3) shows the file or folder under the cursor
and follows it as you move. It waits a moment before loading, so holding an arrow key stays
smooth. This page covers everything it can show, the switches at its top, and the limits. The
terminal app has no preview pane: F3 opens the file in your viewer (`viewer` in the config,
else `$PAGER`, else `less`).

![Markdown with a Mermaid diagram and math, a Jupyter notebook, a spreadsheet, a Word document and a font](screenshots/gui-previews.png)
![A YAML tree, a SQLite database, a certificate and an e-mail](screenshots/gui-previews-data.png)
![A calendar, a git diff, a log file and an EPUB book](screenshots/gui-previews-more.png)
![A PlantUML sequence diagram, a Graphviz graph and an AsciiDoc guide](screenshots/gui-previews-tools.png)
![A LaTeX document built in a container and shown in the preview pane](screenshots/gui-latex.png)

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
| **Engine** | Files made by an external tool (LaTeX, Office, PlantUML, ...) | The installed programs and the container, e.g. **latexmk · tectonic · podman texlive**; see [Previews made by tools](#previews-made-by-tools) |

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
| `.adoc`, `.asciidoc` | Rendered AsciiDoc (headings, lists, tables, admonitions, table of contents); **Rendered / Source** | [Asciidoctor.js](https://asciidoctor.org/), in secure mode (no file includes) |
| `.dot`, `.gv` | The Graphviz graph | [Graphviz](https://graphviz.org/) compiled to WebAssembly ([viz.js](https://github.com/mdaines/viz-js)); nothing to install |

### Data

| Files | Preview | Done by |
|---|---|---|
| `.json`, `.geojson`, `.yaml`, `.yml`, `.toml` | A collapsible tree with counts per level; strings, numbers and booleans colored | Built in, [yaml](https://eemeli.org/yaml/), [smol-toml](https://github.com/squirrelchat/smol-toml) |
| `.jsonl`, `.ndjson` | The first 200 lines as a table, one column per key | Built in |
| `.csv`, `.tsv`, `.xlsx`, `.xlsm`, `.xls`, `.ods` | The first 200 rows as a table, with a button per sheet | [SheetJS](https://sheetjs.com/) |
| `.db`, `.sqlite`, `.sqlite3`, `.db3` | Every table and view with its row count; click a name for its schema | [SQLite](https://sqlite.org/), opened read-only |
| `.parquet`, `.pq` | Row and column counts, the first 200 rows, and the schema (click **Schema**) | [hyparquet](https://github.com/hyparam/hyparquet), with Snappy, Gzip, Zstd, Brotli and LZ4; nothing to install |
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

## Previews made by tools

Some formats need a real program: a TeX distribution, LibreOffice, PlantUML. Coxswain uses one
that is installed, or runs it in a **container** (podman or docker), so nothing has to be
installed for a preview you only need now and then.

| Files | Tool | Installed programs it looks for | Container image (default) | Result |
|---|---|---|---|---|
| `.tex`, `.ltx` | LaTeX | `latexmk`, `tectonic`, `pdflatex` | `docker.io/texlive/texlive:latest` (about 5 GB) | PDF |
| `.doc` `.docm` `.dotx` `.odt` `.ott` `.rtf` `.ppt` `.pptx` `.pps` `.ppsx` `.pot` `.potx` `.odp` `.otp` `.odg` `.vsd` `.vsdx` `.pub` `.wpd` `.wps` | LibreOffice | `soffice` (also its usual macOS and Windows install folders) | none; set one you trust | PDF |
| `.puml`, `.plantuml`, `.pu`, `.iuml`, `.wsd` | PlantUML | `plantuml` | `docker.io/plantuml/plantuml:latest` | SVG |
| `.rst`, `.rest` | pandoc | `pandoc` | `docker.io/pandoc/core:latest` | HTML |
| `.drawio`, `.dio` | draw.io | `drawio` (also the macOS and Windows apps) | none; set one you trust | SVG |
| `.duckdb`, `.ddb` | DuckDB | `duckdb` | none; set one you trust | Tables, row estimates, column counts |

### How it works

1. **Pick an engine.** The buttons at the top of the preview list the installed programs and
   the container, e.g. **latexmk · podman texlive:latest**. Unavailable ones are greyed out;
   hover for the reason ("tectonic is not installed", "No image for drawio"). Your pick is
   remembered per tool.
2. **Render.** LaTeX, LibreOffice and draw.io wait for **Build PDF** / **Render**, because
   they take seconds and run someone else's document. PlantUML, pandoc and DuckDB run by
   themselves, since they are quick, unless their container image still has to be pulled.
3. **Reuse.** Results are cached in the cache folder (`~/.cache/coxswain/previews` on Linux),
   keyed by the file's path, size, modification time and the engine. A file renders once and
   shows at once afterwards; editing it makes the next render fresh.
4. **Errors** show in the preview. For LaTeX that is the first `!` error from the log with
   its context, e.g. `! Undefined control sequence.` and the offending line.

### Containers

- **Chosen by the config.** Containers run with podman, or docker where podman is missing
  (`container` below).
- **No network:** `--network=none`.
- **The file's folder is mounted read-only at `/src`.** LaTeX can still `\input` its sibling
  files, and nothing in your folder can be changed.
- **Results only go to a fresh folder in the cache,** mounted at `/out`. With rootful docker
  the container runs as your user, so the cache gets no root-owned files.
- **No SELinux relabeling** (`--security-opt label=disable`), so your folders' labels are
  never changed.
- **The first run pulls the image.** The engine button says so beforehand ("First run pulls
  docker.io/texlive/texlive:latest (about 5 GB)").
- **A run that takes longer than the timeout** (120 s by default) is stopped and its
  container killed. Pulling is not counted.
- LaTeX runs without `-shell-escape`, so a document cannot run commands.

### Configuration

```toml
[preview]
prefer = "container"     # "auto" (installed program first, else container), "local", "container"
container = "podman"     # "auto" (podman, else docker), "podman", "docker", "off"
timeout = 120            # seconds per conversion

[preview.prefer_tool]    # per tool, overrides prefer
libreoffice = "local"

[preview.images]         # "" means that tool never uses a container
latex = "docker.io/texlive/texlive:latest"   # or :latest-medium, about 2 GB
plantuml = "docker.io/plantuml/plantuml:latest"
pandoc = "docker.io/pandoc/core:latest"
libreoffice = ""
drawio = ""
duckdb = ""
```

LibreOffice, draw.io and DuckDB have no official images. If you set one, it must provide the
command Coxswain runs: `soffice` for LibreOffice. For draw.io and DuckDB, the image's entry
point must be the tool itself, which is how community images such as
`rlespinasse/drawio-desktop-headless` are built (not tested with Coxswain).

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
- Nothing is sent over the network; every library is bundled with the app. Tool previews run
  local programs or network-less containers ([above](#containers)); only pulling a container
  image, which the engine button announces first, downloads anything.

## Speed

The libraries behind the heavier previews (Mermaid, KaTeX, SheetJS, mammoth, the YAML and
TOML parsers, Graphviz, Asciidoctor, hyparquet) load the first time a file needs them, so they
add nothing to startup. The preview pane can be dragged to 60% of the window; documents and
PDFs open fitted to its width. Files
bigger than 25 MB are not rendered as documents or spreadsheets.

## Adding a format

- A format that is easy in the browser goes into `gui/src/renderers.js`: a function that
  returns sanitized HTML or plain data.
- One that needs native code goes into `gui/src-tauri/src/preview.rs`, as a Tauri command.
- One that needs an external program goes into `gui/src-tauri/src/convert.rs`: add the tool,
  its programs, its container command and its output, and map the extensions to it in
  `CONVERTER` in `gui/src/lib.js`.
- `previewKind` in `gui/src/lib.js` maps extensions to a kind.
- `gui/src/Preview.svelte` shows each kind.

For screenshots, `docs/screenshots/demo-docs.py` writes a demo file for each format, and
[the README](../README.md) describes the sandbox the screenshots are taken in.

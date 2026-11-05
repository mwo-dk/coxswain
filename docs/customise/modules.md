[← README](../../README.md) · [Docs index](../README.md) · [Customising](README.md)

# Modules: choose the viewers

Coxswain shows many kinds of file in their own way: a PDF as its pages, a game on its board, a
spreadsheet as a table, a diagram drawn. Each of those viewers belongs to a **module**, and you
can turn off the modules you do not want loaded, in both apps. A file whose module is off is
shown as text or hex, like any file with no viewer of its own. All modules are on until you
turn one off, so nothing changes until you do.

![The desktop app's Settings at Modules, in the Cyber theme: Modules outlined between Previews and Games on the left; on the right each module with its checkbox, its endings, what it uses and Show me, the ones that may download a container image with a downloads badge, and Chess games unticked](../screenshots/gui-settings-modules.png)
*Settings at Modules with Chess games turned off, opened with `coxswain-gui --settings=modules`.*

## Contents

- [How to use it](#how-to-use-it)
- [The modules](#the-modules)
- [What you see when a module is off](#what-you-see-when-a-module-is-off)
- [What stays whatever is off](#what-stays-whatever-is-off)
- [config.toml](#configtoml)
- [Questions](#questions)

## How to use it

| Where | Desktop app | Terminal app |
|---|---|---|
| Settings | **Ctrl+,** or **F12**, then *Modules* on the left: a checkbox per module | **F12**, then *Modules* (**Tab** moves between the areas): **Space** or **Enter** turns the module under the cursor on or off |
| Straight there | `coxswain-gui --settings=modules` | `coxswain --settings=modules` |
| First-run guide | Its last step says viewers can be turned off; **Choose modules** opens Settings at *Modules* | Its last step says *Viewers you do not need can be turned off: F12 → Modules* (your Settings key) |
| By hand | `[modules] off = ["games"]` in `config.toml` | The same file |

Each module's row shows its name, the endings it shows, the libraries and programs it uses, and
*may download a container image* where it can draw a file with a container when its program is
not installed (the desktop app marks those with the *downloads* badge). In the desktop app
**Show me** opens the module's own page; the terminal app names the page in the line at the
bottom. A change is saved to `config.toml` at once and used straight away, by the panel you are
in as much as by the next file.

![The terminal app's Settings at Modules, Classic blue: Modules chosen on the left; on the right the heading Viewers and every module with [x], the cursor on [ ] Chess games; at the bottom its endings, uses cozy-chess, stockfish and docs/previews/chess.md](../screenshots/tui-settings-modules.png)

## The modules

| Module | `[modules] off` name | Shows | Uses |
|---|---|---|---|
| [Documents](../previews/documents.md) | `documents` | PDF, Word (`.docx`), EPUB, Jupyter notebooks, XPS, reStructuredText, AsciiDoc, Typst, PostScript, mail (`.eml`, `.msg`), calendars, contacts | mammoth, Asciidoctor.js, marked, KaTeX, mail-parser, cfb; pandoc, typst, Ghostscript, asciidoctor-pdf |
| [PowerPoint and Office](../previews/office.md) | `office` | What LibreOffice opens and Coxswain cannot draw itself (`.doc`, `.pptx`, `.odt` …) | pptx-to-html; LibreOffice |
| [E-books](../previews/ebooks.md) | `ebooks` | MOBI, Kindle, FictionBook, LIT, comics (`.cbz`) | jszip; calibre |
| [LaTeX projects](../previews/latex.md) | `latex` | `.tex`, `.ltx` | latexmk, tectonic, pdflatex |
| [HTML pages](../previews/html.md) | `html` | `.html`, `.htm`, `.xhtml`, drawn sandboxed | DOMPurify |
| [Diagrams](../previews/diagrams.md) | `diagrams` | Graphviz, Mermaid, PlantUML, D2, ditaa, Svgbob, Pikchr, nomnoml, WaveDrom, Vega, DBML, ERD, bytefield, draw.io | Viz.js, Mermaid; dot, plantuml, d2, drawio, Kroki |
| [OpenSCAD](../previews/openscad.md) | `openscad` | `.scad` models | openscad |
| [KiCad](../previews/kicad.md) | `kicad` | Schematics and boards | kicad-cli |
| [Data files](../previews/data.md) | `data` | Spreadsheets, SQLite, dBase, Parquet, DuckDB, property lists, certificates | SheetJS, rusqlite, parquet, hyparquet, plist, x509-parser; duckdb |
| [Media and files](../previews/media.md) | `media` | Pictures, video, sound, HEIF, camera raw, fonts | lofty, kamadak-exif; ffmpeg, ImageMagick |
| [Old pictures and files](../previews/old-formats.md) | `old_formats` | TIFF, TGA, Netpbm, DDS, HDR, EXR, QOI, JPEG XL and the like; tracker music | image, jxl-oxide |
| [Geodata](../previews/geodata.md) | `geodata` | Shapefiles, GeoJSON, KML, GPX and the rest of GDAL's vector formats | GDAL |
| [Neural network models](../previews/neural-networks.md) | `neural_networks` | ONNX, TensorFlow Lite, Core ML, Keras; local models (GGUF, safetensors) | Netron |
| [Chess games](../previews/chess.md) | `games` | Chess, Go, draughts, shogi, xiangqi and bridge files, opening books | cozy-chess; stockfish |
| [Cryptography bills of materials](../previews/bom.md) | `bom` | CycloneDX BOMs and CBOMs | |
| [Build provenance](../previews/provenance.md) | `provenance` | In-toto attestations, Sigstore bundles | x509-parser |
| [Disk images](../files/disk-images.md) | `disk_images` | The facts of `.iso`, `.vhd`, `.vmdk` and other disk images | 7-Zip |

Each module is named in Settings after its entry in F1 → Features (*E-books: MOBI, Kindle, FictionBook*, *Disk images, CHM and CBR* …), in your language; the first column links its page. No module
sends your files anywhere; the ones that may download a container image say so, and what can
leave the machine is listed under [Settings → Privacy and updates](settings.md#privacy-and-updates).

## What you see when a module is off

- **The preview (desktop app)** shows the file's text, or its first bytes in hex when it is no
  text. A PDF shows its first bytes; a PGN file its moves as text.
- **F3** in either app opens the file in the text viewer instead of its own viewer.
- **Enter** on a games file, an opening book or a disk image does what it does with any file
  (its program), and does not open the games board or the image.
- **Open games** (the chess action) says that the module is off.
- **The first time** a file of a module that is off is shown, the status line says
  *Shown as text: Chess games is off. Turn it on in Settings (F12) → Modules.* (with your Settings
  key). It is said once per module.

## What stays whatever is off

Folders, archives and everything inside them, text and code, logs, data files (JSON, YAML,
TOML), Markdown and hex belong to no module: they are what every file falls back to. Find reads
what it reads either way: a module decides only what the apps show, not what search reads.
Actions on files (*Convert video or audio*, *Export the model*, *Make searchable* …) stay in the
action menu too.

## config.toml

```toml
[modules]
# The modules left out, by name (see the table above). Empty, or no [modules]: all are on.
off = ["games", "neural_networks"]
```

A name Coxswain does not know is left alone: Settings writes only the names it knows when you
next change a module there. Both apps read `[modules]` when they start and after every change
in Settings. See [Configuration: every key](../reference/configuration.md#modules).

The release builds have every module. They are all in one program; turning one off means it is
not used, not that it is left out of the build.

## Questions

#### Why does a PDF or a game show as text now?

The module that shows it is off. Turn it on again in Settings (**F12** in both apps, **Ctrl+,**
in the desktop app) → *Modules*, and the file shows as before; the panel you are in shows it at
once.

#### Does turning a module off stop Find reading those files?

No. Modules decide what the apps show; Find still reads the words in those files when reading
words is on (Settings → *Finding files*).

#### Does any module send my files anywhere?

No. A few may download a container image to draw a file when its program is not installed and
containers are on (Settings → *Previews* → *Container images*); Settings marks those with the
*downloads* badge, and the image comes from its registry, not your file going anywhere.

#### I turned off Chess games: why does Enter on a PGN file open my other chess program now?

With the module off, Enter does with a games file what it does with any file: it opens it with
the program your system gives it. Turn the module on again to open it on Coxswain's board.

#### Can I leave a module out of the build?

Not yet: every build has every module, and turning one off means it is not used. Leaving
modules out when building from source is planned.

#### Where do I see which module a kind of file belongs to?

In the table above, and in Settings → *Modules*, where each module lists the endings it shows.

---
[← Previous: The Settings window](settings.md) · [Next: Themes →](themes.md)

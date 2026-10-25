[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Containers

When a tool a preview needs is not installed, Coxswain can run it from a container image with
podman or docker: the TeX Live image builds any LaTeX document, Typst's builds Typst, Gotenberg's
LibreOffice turns office files and clip art into PDFs, Kroki's draws D2, ditaa and ten more
diagram kinds, the PlantUML and pandoc images draw diagrams and reStructuredText, OCRmyPDF's
makes scanned PDFs searchable and has the Ghostscript that shows PostScript, ffmpeg's shows and
converts video and sound and HEIC photos, ImageMagick's shows GIMP pictures and converts and
edits photos, GDAL's draws and converts geodata, OpenSCAD's makes models into pictures, STL
and 3MF, KiCad's shows and exports schematics and boards, Asciidoctor's builds
[PDFs of AsciiDoc](../files/asciidoc-pdf.md) and draw.io's [exports diagrams](../files/drawio-export.md)
(opt-in), and ClamAV's [scans for viruses](../files/virus-scan.md) when you ask. Apache Tika's [reads other formats for search](../search/tika.md) when you turn it on,
and calibre's reads [e-books](ebooks.md) when you turn them on. A container gets no network (only ClamAV's database update has it, and it sees nothing but
the database) and sees your files read-only. Nothing is downloaded until you click, except calibre's image, which
turning on *E-books with calibre* with text search on pulls ([E-books](ebooks.md#turning-it-on)).

![The desktop app in the Cyber theme: flight7.tex under the cursor in projects/paper, and the preview pane with the engine buttons podman texlive:latest (lit) and tectonic, and under them Rendering with podman texlive:latest… and Pulling docker.io/texlive/texlive:latest… with podman's progress line, the bar at the bottom of the pane running](../screenshots/previews-container-pull.png)

## Contents

- [How to use it](#how-to-use-it)
- [The images](#the-images)
- [What a container may do](#what-a-container-may-do)
- [Pulling and removing images](#pulling-and-removing-images)
- [Keeping images up to date](#keeping-images-up-to-date)
  - [Pinned images](#pinned-images)
- [Removing unused images](#removing-unused-images)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Install podman (or docker). Nothing else: the default images are set. On FreeBSD, the other
   BSDs and illumos neither runs these Linux images: the engine button's note and *Settings →
   Previews* say *No podman or docker here, and the preview images are Linux ones: install the
   program itself instead, such as `pkg install texlive-full`*, with that system's line
   ([Installing what is missing](tools.md#installing-what-is-missing)).
2. Put the cursor on a file whose tool is not installed, e.g. a `.tex` file, and open the pane
   (**Space** or **F3**). The engine button reads **podman texlive:latest**.
3. The first time, the image has to be downloaded. The preview says *First run pulls
   docker.io/texlive/texlive:latest (about 2.8 GB)* under **Build PDF**; press it. Or pull ahead
   of time: **Pull** on the kind's row in *Settings → Previews → [Preview tools](tools.md#preview-tools-in-settings)*.
4. From then on the container runs by itself like an installed program.
5. To free the room again: **Remove** on the same row.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane | – |
| **Ctrl+,** | Settings → *Previews*, with *Preview tools* (**Pull**, **Remove**), *Keep images up to date* and *Remove images not used for 90 days* | **F12** → *Previews*: the runtime, the LaTeX image, *E-books with calibre*, *Preview tools* (**P** pulls the image of the kind under the cursor, **Delete** removes it) and the two image options; Apache Tika's image is pulled by the search helper when [its option](../search/tika.md) is ticked |

## The images

Each image in *Settings → Previews → Preview tools* (when you hover its *Container:* line) says what it gives, how big it is and
what it costs, and so does its row in [Disk use](../reference/disk-use.md) in both apps. Sizes
are for 64-bit Intel and AMD (amd64), measured in October 2026; a newer version of a tag can be
a little larger or smaller.

| Image | What you get | Download | On disk | What it costs |
|---|---|---|---|---|
| `docker.io/texlive/texlive:latest` (default for LaTeX) | `.tex` and `.ltx` documents built into the PDF the preview shows, with all of TeX Live's packages and fonts | about 2.8 GB | about 5.9 GB | A few seconds of processor time per build, up to a minute for a long book |
| `docker.io/texlive/texlive:latest-medium` | The same, with fewer packages: a document that loads a rare one fails | about 960 MB | about 2.2 GB | As above |
| `docker.io/plantuml/plantuml:latest` (default for PlantUML) | `.puml` and `.plantuml` diagrams drawn as the picture the preview shows | about 141 MB | about 353 MB | Java starts for each diagram: a second or two |
| `docker.io/pandoc/core:latest` (default for pandoc) | `.rst` files shown as a formatted page | about 57 MB | about 246 MB | Light: under a second for most documents |
| `docker.io/gotenberg/gotenberg:8-libreoffice` (default for LibreOffice) | Word, Works, Write, WordPerfect, Lotus 1-2-3, PowerPoint, Visio, OpenDocument and other older office files, and Windows clip art (`.wmf`, `.emf`), turned into the PDF the preview shows ([Office](office.md)) | about 440 MB | about 1 GB | LibreOffice starts for each document: a second or two of processor time |
| `ghcr.io/typst/typst:latest` (default for Typst) | `.typ` documents built into the PDF the preview shows ([Typst](documents.md#typst)) | about 29 MB | about 65 MB | Light: a fraction of a second for most documents |
| `docker.io/yuzutech/kroki:latest` (default for Kroki's diagram kinds) | D2, ditaa, svgbob, Pikchr, nomnoml, WaveDrom, Vega, Vega-Lite, DBML, erd and bytefield diagrams drawn as the picture the preview shows ([Diagrams](diagrams.md#d2-ditaa-svgbob-and-the-others)) | about 1.2 GB | about 2.8 GB | Light: each diagram takes well under a second; the image is large because it holds the programs of two dozen kinds |
| `docker.io/jbarlow83/ocrmypdf-alpine:latest` (default for OCRmyPDF and for Ghostscript) | [Make PDF searchable](../files/searchable-pdf.md), and PostScript (`.ps`, `.eps`) shown as a PDF with its Ghostscript ([PostScript and XPS](documents.md#postscript-and-xps)): one pull for both | about 310 MB | about 850 MB | OCR: about a second of processor time per page, only when you ask; PostScript: under a second |
| `docker.io/mwader/static-ffmpeg:latest` (default for ffmpeg) | Videos and sound the app's player cannot play shown as a frame or a waveform, a frame as each video's thumbnail and poster, a waveform above each song's player, the facts of videos (length, size, codecs, tracks) ([Media](media.md#video-and-sound-with-ffmpeg)), and [Convert video or audio](../files/convert-media.md) | about 126 MB | about 315 MB | Previews: well under a second each. An MP4 keeps the processor busy for a while, often about as long as the video plays; only when you ask |
| `ghcr.io/mwo-dk/coxswain-imagemagick:latest` (default for ImageMagick) | HEIC and cameras' RAW photos, GIMP (`.xcf`) and the other pictures only ImageMagick reads, with thumbnails ([Media](media.md#photos-only-imagemagick-or-ffmpeg-reads)), and [Convert or edit pictures](../files/edit-pictures.md) | about 21 MB | about 52 MB | Light: a second or two per picture |
| `ghcr.io/osgeo/gdal:alpine-small-3.13.3` (default for GDAL, pinned to its digest) | A map of GeoPackage, shapefile, GeoJSON, KML and GeoTIFF files from their own shapes, with thumbnails, their layers, coordinate system and extent ([Geodata](geodata.md)), and Convert geodata | about 40 MB | about 98 MB | Light: under a second per file for most; a large GeoPackage or GeoTIFF takes a few seconds |
| `ghcr.io/mwo-dk/coxswain-stockfish:latest` (default for the chess engine) | Stockfish for the chess view's best lines and whole-game analysis when none is installed ([The engine](chess-play.md#the-engine)) | about 70 MB | about 80 MB | Light while you look at a position; a whole game analysed keeps one core busy for about a minute |
| `docker.io/openscad/openscad:trixie.2026-10-19` (default for OpenSCAD, pinned to its digest) | A picture of each `.scad` model's part, with thumbnails ([OpenSCAD models](openscad.md)), and Export 3D model as STL or 3MF | about 241 MB | about 635 MB | Light to heavy: most models take a second with Manifold; a large one with many curves keeps the processor busy for a while |
| `docker.io/apache/tika:4.1.0` (pinned to its digest, for [Read other formats with Apache Tika](../search/tika.md)) | The words of OneNote, XPS, CHM, Outlook data files, MHTML, Hangul and Access files for Find, and of older Office files when LibreOffice is not installed | about 190 MB | about 440 MB | Its server runs only while such files are read and stops after 5 idle minutes: about 350 MB of memory, a fraction of a second of processor time per file |
| `docker.io/linuxserver/calibre:9.16.0`, pinned by its digest (default for calibre, only with [E-books with calibre](ebooks.md) on) | MOBI, Kindle (AZW, AZW3), FictionBook, LIT and Palm books shown with their cover, title, authors and first pages, their words in Find, and *Convert e-book* to EPUB, AZW3 or PDF | about 1.3 GB | about 3.6 GB | A second or two of processor time per book, a few more for a PDF; reading a library for search runs in the background at the lowest priority |
| `docker.io/kicad/kicad:9.0.9` (default for KiCad, pinned to its digest; amd64 only) | KiCad schematics and boards shown as PDF, with thumbnails ([KiCad](kicad.md)), and Export from KiCad: PDF, SVG, a bill of materials, Gerbers | about 476 MB | about 1.7 GB | Light: a second or two per schematic or board; the download is the cost |
| `ghcr.io/ggml-org/whisper.cpp:main` (pinned to its digest, for [Transcripts of speech](../search/transcripts.md)) | The speech of recordings and videos you choose, written down for Find and the preview, when no whisper.cpp is installed | about 505 MB | about 1.1 GB | Only while it transcribes: half the processor, about 10 minutes for an hour of sound with the small model on a recent processor |
| `docker.io/asciidoctor/docker-asciidoctor:latest` (default for Asciidoctor) | [Build PDF of AsciiDoc](../files/asciidoc-pdf.md): a PDF of an `.adoc` document next to it, when you ask; the preview needs no image | about 665 MB | about 1.5 GB | A few seconds of processor time per document, only when you ask |
| `docker.io/rlespinasse/drawio-desktop-headless:latest` (default for draw.io, used only with *Export draw.io diagrams* on) | [Export draw.io diagram](../files/drawio-export.md): PNG, SVG or PDF of a diagram next to it, and its thumbnail in the grid view | about 560 MB | about 1.3 GB | draw.io starts a browser for each export: a second or two of processor time and a few hundred MB of memory |
| `docker.io/clamav/clamav-debian:1.5_base@sha256:dfe9…` (default for ClamAV) | [Scan for viruses](../files/virus-scan.md), once turned on: only the files you choose, only when you ask | about 80 MB | about 240 MB | Busy only when you ask: about 1.5 GB of memory while it scans, and its virus database (about 110 MB) in the cache |
| `ghcr.io/mwo-dk/coxswain-duckdb:latest` (default for DuckDB) | The tables of `.duckdb` files, with their columns and row counts, when no DuckDB is installed | about 25 MB | about 76 MB | Light: one quick read of the file |
| one you set for GhostXPS (`gxps`) | XPS and OpenXPS documents shown as a PDF | depends on the image | depends on the image | Light |

Where each image comes from, and why it is the one:

| Image | Made by | Licence of what runs in it | Kept up to date |
|---|---|---|---|
| `gotenberg/gotenberg:8-libreoffice` | The Gotenberg project; Coxswain runs only the LibreOffice in it, never Gotenberg's server | LibreOffice MPL-2.0, Gotenberg MIT | A new build every few weeks; LibreOffice publishes no image of its own |
| `yuzutech/kroki:latest` | The Kroki project; Coxswain runs the diagram programs in it one per call, never Kroki's server | MIT (Kroki), each program its own (D2 MPL-2.0, svgbob and Pikchr MIT or 0BSD, …) | Every few weeks |
| `jbarlow83/ocrmypdf-alpine:latest` | OCRmyPDF's author, with each release | OCRmyPDF MPL-2.0, Tesseract Apache-2.0, Ghostscript AGPL-3.0 | With every OCRmyPDF release (weekly in 2026) |
| `ghcr.io/typst/typst:latest` | The Typst project | Apache-2.0 | With every Typst release |
| `mwader/static-ffmpeg:latest` | Mattias Wadman's static builds of ffmpeg and ffprobe, the most used of their kind; Coxswain runs `/ffmpeg` and `/ffprobe` in it | ffmpeg GPL-2.0-or-later (with x264 and the other libraries built in), the build scripts MIT | With every ffmpeg release, and when a library in it changes (a new build every few days in 2026) |
| `osgeo/gdal:alpine-small-3.13.3` | The GDAL project's own images, built with each release; the small Alpine one has the drivers Coxswain uses and PROJ. Pinned to its digest: a Coxswain release moves it on | GDAL MIT, PROJ MIT, the libraries in it theirs (SQLite public domain, libtiff, libgeotiff, expat …) | With every GDAL release (every few weeks in 2026) |
| `kicad/kicad:9.0.9` | The KiCad project's own image of its newest release (the one without 3D models of parts; `-full` adds them at several GB). Pinned to its digest: a Coxswain release moves it on | KiCad GPL-3.0-or-later, its libraries CC-BY-SA-4.0 with an exception for what you make with them | With every KiCad release (every month or two in 2026) |
| `mwo-dk/coxswain-stockfish:latest` | Coxswain itself (`packaging/images/stockfish`): Stockfish 17.1's released source and its two evaluation networks, each checked by its digest, built statically into an image that holds nothing else (no shell) | Stockfish GPL-3.0 | Built again every week; a new Stockfish release with a Coxswain release |
| `openscad/openscad:trixie.2026-10-19` | The OpenSCAD project's own image of a dated snapshot (its last release, 2021.01, has no Manifold and no image of its own worth taking). Pinned to its digest: a Coxswain release moves it on | OpenSCAD GPL-2.0-or-later, the libraries in it theirs (CGAL GPL-3.0-or-later, Manifold Apache-2.0, Qt LGPL-3.0 …) | A snapshot every week in 2026 |
| `mwo-dk/coxswain-duckdb:latest` | Coxswain itself (`packaging/images/duckdb`): DuckDB 2.0.0's released command line for Linux, checked by its digest and unchanged, with musl, libstdc++ and libgcc from Alpine, and nothing else (no shell); DuckDB publishes no image of its own | DuckDB MIT, musl MIT, libstdc++ and libgcc GPL-3.0-or-later with the GCC runtime exception | Built again every week; a new DuckDB release with a Coxswain release |
| `mwo-dk/coxswain-imagemagick:latest` | Coxswain itself (`packaging/images/imagemagick`): Alpine's ImageMagick 7 with libheif (HEIC, AVIF) and LibRaw (RAW), Coxswain's policy built in, and no shell or package manager; ImageMagick publishes no image of its own | ImageMagick's licence (Apache-2.0 style), libheif LGPL-3.0, LibRaw LGPL-2.1 or CDDL-1.0, the rest of Alpine's packages theirs | Built again every week for Alpine's security updates |
| `apache/tika:4.1.0` | The Apache Tika project. Pinned to its digest: a Coxswain release moves it on | Apache-2.0 | With every Tika release (monthly in 2026) |
| `linuxserver/calibre:9.16.0` | [LinuxServer.io](https://www.linuxserver.io/), an organisation that rebuilds it for every calibre release, for amd64 and arm64; calibre publishes no image of its own. Coxswain runs only calibre's `ebook-convert` and `ebook-meta` in it, never the desktop it serves, and more tightly than the others ([How calibre runs](ebooks.md#how-calibre-runs)) | calibre GPL-3.0, the image's own files GPL-3.0 | Weekly; Coxswain pins the release it was tested with, and a newer Coxswain moves the pin |
| `ghcr.io/ggml-org/whisper.cpp:main` | The whisper.cpp project (ggml-org), built from every change; pinned to one build by its digest: a Coxswain release moves it on | MIT (whisper.cpp), ffmpeg LGPL/GPL | Daily; 64-bit Intel and AMD only |
| `asciidoctor/docker-asciidoctor:latest` | The Asciidoctor project; Coxswain runs only its `asciidoctor-pdf` | MIT (Asciidoctor, Asciidoctor PDF) | With every Asciidoctor release |
| `rlespinasse/drawio-desktop-headless:latest` | Romain Lespinasse, who keeps it in step with draw.io's desktop releases; no image of draw.io's own exports | MIT (the image), Apache-2.0 (draw.io) | With every draw.io desktop release (weekly in 2026) |
| `clamav/clamav-debian:1.5_base` | Cisco Talos, ClamAV's makers; for x86-64, ARM64 and POWER. Pinned by its digest; its database is fetched apart from `database.clamav.net` | GPL-2.0 | Rebuilt every few days; Coxswain moves to a newer digest in its releases |

The programs run as separate programs in their containers; Coxswain ships none of their code, so
Ghostscript's AGPL does not reach it.

In Settings the line under each image reads, for the default LaTeX one: *Builds LaTeX documents
(.tex, .ltx) into the PDF the preview shows, with TeX Live's packages and fonts. About 2.8 GB to
download, 5.9 GB on disk. Each build keeps the processor busy for a few seconds, up to a minute
for a long book.* An image you set yourself says *Its size depends on the image you set*; once
it is pulled, its size on disk shows beside it (*Pulled, 1.1 GB*).

## What a container may do

Every run is `podman run` (or `docker run`) with:

| Option | Why |
|---|---|
| `--rm` | The container is removed when it ends |
| `--network=none` | No network: a document cannot fetch or send anything |
| `--mount type=bind,source=<folder>,target=/src,readonly` | The file's folder, read-only: nothing in it can be changed. A folder whose name has a `:` (*Meeting 10:30*) mounts too, which `-v` could not do. For LaTeX, the whole [project](latex.md#what-coxswain-works-out), so chapters, pictures and the bibliography are found. ffmpeg and ImageMagick get the one file only (`--mount type=bind,source=<file>,target=/src/in.<ending>,readonly`); GDAL too, as `/src/<name>` (a name with other signs than letters, digits, `.`, `_` and `-` made plain, with an `f` before it), but a shapefile's whole folder, since a shapefile is several files side by side; OpenSCAD the model's folder, since a model includes others beside it; KiCad the schematic's or board's folder, for its sheets and project |
| `--mount type=bind,source=<cache>,target=/out` | A fresh folder in the preview cache, the only place it can write; for the conversions a fresh folder under `~/.cache/coxswain/media/`; ffprobe's run has none |
| `--security-opt label=disable` | No SELinux relabelling, so your folders' labels are never changed |
| `--user <you>` (docker, and podman run as root, on Linux and macOS) | Rootful docker or podman would leave root-owned files in the cache |
| `-e HOME=/tmp` | The image cannot be written to, and your user has no home in it, so programs that keep a profile or a cache (LibreOffice, TeX Live, Java) keep it in the container's `/tmp`, which goes with the container |
| `--user 0:0` (rootless podman) | In rootless podman the container's root is you, so an image whose own user is another one (Gotenberg's, OCRmyPDF's) can write its result to `/out`. Whether podman runs rootless is asked once, with `podman info` |
| `--name coxswain-preview-<hash>` | So a run past the timeout can be killed (`coxswain-media-<n>`, `coxswain-probe-<n>` for ffmpeg's, ImageMagick's, GDAL's, OpenSCAD's and KiCad's runs, `coxswain-stockfish-<n>` for the chess engine's) |
| `--read-only --tmpfs /tmp:rw,size=1g --memory=3g --pids-limit=256 --cap-drop=ALL --security-opt no-new-privileges` | Nothing in the image can be changed, only a small `/tmp` in memory and the output folder can be written, memory and processes are bounded, and the program has no capabilities and cannot gain privileges. Every image runs so, LaTeX, LibreOffice and Kroki's as much as ffmpeg's, Whisper's, Tika's, calibre's and the chess engine's; where podman cannot bound memory and processes (its cgroup controllers not delegated), those two are left out |

Inside, the commands are: `latexmk -pdf -interaction=nonstopmode -outdir=/out` for LaTeX,
`typst compile --root /src` for Typst, `soffice -env:UserInstallation=file:///tmp/lo --headless --convert-to pdf --outdir /out`
for LibreOffice, `gs -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pdfwrite` for PostScript (`--entrypoint gs`),
`ocrmypdf --skip-text -l <languages>` for [Make PDF searchable](../files/searchable-pdf.md), the
PlantUML and pandoc images' own entry points reading the file on standard input (`-tsvg -pipe`,
`-f rst -t html5`), Kroki's programs one per call, reading the diagram on standard input
(`--entrypoint d2` with `- -`, `ditaa --svg - -`, `svgbob`, `pikchr --svg-only -`, …), `asciidoctor-pdf --safe-mode server` for [Build PDF of
AsciiDoc](../files/asciidoc-pdf.md), draw.io's own `-x` export behind an Xvfb display for
[Export draw.io diagram](../files/drawio-export.md) (both held like ffmpeg's below, draw.io
seeing only a copy of the diagram), and
`duckdb -readonly -json` for DuckDB, `/ffmpeg` and `/ffprobe` with `-protocol_whitelist file`
and `-format_whitelist` (real media formats only) for ffmpeg, and `magick` under Coxswain's
policy (`MAGICK_CONFIGURE_PATH`: no helper programs, picture formats only, bounded memory and
time) for ImageMagick, and `gdalinfo`, `ogrinfo`, `ogr2ogr` and `gdal_translate` for GDAL, each
held to the driver of the file's kind (`-if GPKG`, `-if GTiff` …) with `GDAL_PAM_ENABLED=NO`,
`openscad -o /out/… --export-format binstl --backend=manifold` for OpenSCAD, and `kicad-cli sch export …`
and `kicad-cli pcb export …` for KiCad. The chess
engine (`/stockfish`) gets no folder at all: it talks UCI over standard input and output, runs
as the image's own user (nobody), and is named `coxswain-stockfish-<n>` so it is killed when the
position changes or the view closes. ffmpeg and ImageMagick see one file only, not its folder: it is mounted
read-only as `/src/in.<ending>`, so its name is never an argument. LaTeX runs without
`-shell-escape`, so a document cannot run commands. ClamAV's two runs are held tighter still,
and only its database update has network ([How it is sealed](../files/virus-scan.md#how-it-is-sealed)).
commands. No preview container starts a server or opens a port: each run does one job and ends.
Apache Tika's is the one server, for search: it listens only inside its container, has no
network, no folder of yours and no port on your machine, and stops itself when idle
([What the container may do](../search/tika.md#what-the-container-may-do)).

calibre's runs for [e-books](ebooks.md) are held tighter still: no mounts at all, the book on
standard input, `sh -c` running `ebook-convert` and `ebook-meta` as nobody, with the same limits
as ffmpeg's and ImageMagick's ([How calibre runs](ebooks.md#how-calibre-runs)).

A run longer than the timeout (120 s by default) is stopped and its container killed. Make PDF
searchable, Convert video or audio, Convert or edit pictures, Convert geodata, Export 3D model and Export from KiCad have none: **Stop** ends them. Pulling
the image is not counted.

## Pulling and removing images

- **The first run pulls the image,** and only after a click (calibre's also when you turn on
  [e-books](ebooks.md) with text search on). The engine button's note says so
  beforehand (*First run pulls docker.io/texlive/texlive:latest (about 2.8 GB)*, or *First run
  pulls docker.io/plantuml/plantuml:latest (about 141 MB)*; an image you set yourself shows no
  size), and while it downloads the preview shows
  *Pulling docker.io/texlive/texlive:latest…* with the runtime's own progress line.
- Once pulled, the note says *Runs docker.io/texlive/texlive:latest without network access*.
- **Settings lists the images** under [*Preview tools*](tools.md#preview-tools-in-settings), on the row of the kind they
  make (*PostScript and XPS* and *Searchable PDFs* share OCRmyPDF's): each with what it gives,
  its size and cost when you hover it ([The images](#the-images)), *Pulled, 5.9 GB* or
  *Not pulled (2.8 GB)* (the progress line while pulling), **Pull** to download it ahead of
  time or update it to the newest, and **Remove** to free the space.
- Unless you turn on one of the two options below, Coxswain never updates or removes an image
  by itself.

## Keeping images up to date

*Settings → Previews → Keep images up to date* (`[preview] image_updates`): **Off** (the
default), **Weekly** or **Monthly**.

- **What it updates:** only the images under `[preview.images]` that are already pulled, and
  the new [pin](#pinned-images) of a default image whose older build is there. One you never
  pulled is never downloaded by it.
- **When:** the desktop app looks once an hour while it runs (the first time ten minutes after
  it starts). When a week or a month has passed since the last round, it runs `podman pull`
  (or `docker pull`) for each of those images, in the background. Not on battery, and not with
  *Container runtime → No containers*. Each image is tried, even after one fails. When every
  pull fails (no network), the round stays due and the next hour tries again; when only some
  fail, the round is done and those wait for the next one.
- **What you see:** nothing pops up. While it pulls, *Settings → Previews → Preview tools*
  shows the runtime's progress line on that image's row, as for **Pull**, and the image's
  **Pull** and **Remove** wait.
- **The old copy goes at once.** When a pull brought a new version, the copy it replaced is
  removed by its own id (`podman image rm <id>`). Coxswain never runs `image prune`, which would
  also take untagged images of yours that have nothing to do with it. A copy that cannot be
  removed, because a container still uses it or another name points at it, stays; nothing is
  said and it is not tried again.
- **Where it goes:** the registry in the image's name, `docker.io` for most defaults, `ghcr.io` for Typst's, GDAL's, whisper.cpp's and
  Coxswain's own (ImageMagick, Stockfish).
  *Settings → Privacy and updates → What can leave this machine* then says *Container images for
  previews, when pulled and each week to keep them up to date* (or *each month*), with the
  registry. Apache Tika's image, once pulled for [search](../search/tika.md), is updated with
  them.
- **The terminal app** makes no container previews, so it updates nothing; its **F12** →
  *Previews* shows the option, and the desktop app does the updating.

### Pinned images

Every default image is named with its tag and the digest of the build Coxswain was tried with
(`docker.io/pandoc/core:latest@sha256:…`), as `[preview.images]` shows it. A pull fetches exactly
that build, for every processor the image is made for, never whatever the tag points at that
day; texts and Settings show the name without the digest.

- **A new version of Coxswain moves the pins.** Before a release, `tools/images-pin.py` asks
  each registry, anonymously, for the digest its tag has now, and writes the new ones into the
  code; the tests run on them.
- **An update takes the new pin.** With *Keep images up to date* on, the next round pulls the
  new pin of each image whose old one Coxswain ran in, and removes the copy of the old one by
  its name. A pull that fails keeps the old copy for the next round. Without it, the next
  preview that needs the image asks to pull it, as for a first run.
- **A former default goes the same way.** ImageMagick's default was
  `docker.io/dstmodders/imagemagick:latest` before Coxswain made its own image. With *Keep
  images up to date* on, a round that finds it pulls today's ImageMagick image and removes the
  old one by its name, under any tag, whether a preview ran in it or it was only pulled.
  Without the option it stays until you clear it in [Disk use](../reference/disk-use.md), which
  lists it as a former default. Not when `[preview.images] imagemagick` is set to an image of
  your own, or to that old one.
- **An image you set is yours.** One you set under `[preview.images]` (with or without a
  digest) is never rewritten nor replaced: Coxswain pulls and runs exactly that.

## Removing unused images

*Settings → Previews → Remove images not used for 90 days* (`[preview] image_prune`), off by
default.

- Both apps note, in `state.json`, when a preview, [Make PDF searchable](../files/searchable-pdf.md)
  or [Scan for viruses](../files/virus-scan.md) last ran in each image. Once an hour (not on battery) the desktop app removes a pulled image
  of `[preview.images]` that neither used (nor Apache Tika read a file with) for 90 days, by name, as **Remove** does.
- An image with no use noted yet, such as one pulled before you turned the option on, counts as
  used on the day the app first looks, so turning it on never removes all of them at once.
- The next preview that needs a removed image says *First run pulls …* again and waits for your
  click.
- Only Coxswain's own images are ever touched. The terminal app shows the option; the desktop
  app does the removing.
- Apache Tika's image counts as used on each day the search helper reads a file with it
  ([Other formats with Apache Tika](../search/tika.md)), so it is not removed while Tika is on and
  has files to read.

## What you see

- The container's engine button: runtime and image name, e.g. **podman texlive:latest**,
  **docker plantuml:latest**; greyed as **container** when it cannot run (hover: *Neither
  podman nor docker is installed*, or *No image for libreoffice: set [preview.images]
  libreoffice in the config*).
- While pulling: *Pulling …* and the progress line, in the preview and in Settings.
- A pull that fails: *Pulling docker.io/texlive/texlive:latest failed: …* with the runtime's last error line.

## Settings and config.toml

| Settings → Previews | config.toml | Choices, default |
|---|---|---|
| *Container runtime* | `[preview] container` | `"auto"` (podman, else docker), `"podman"`, `"docker"`, `"off"` (*No containers*) |
| *How previews are made* | `[preview] prefer` | `"auto"`, `"local"`, `"container"` |
| *LaTeX image* | `[preview.images] latex` | `"docker.io/texlive/texlive:latest@sha256:…"` (pinned) |
| – | `[preview.images] plantuml` | `"docker.io/plantuml/plantuml:latest@sha256:…"` (pinned) |
| – | `[preview.images] pandoc` | `"docker.io/pandoc/core:latest@sha256:…"` (pinned) |
| – | `[preview.images] libreoffice` | `"docker.io/gotenberg/gotenberg:8-libreoffice@sha256:…"` (pinned) |
| – | `[preview.images] kroki` | `"docker.io/yuzutech/kroki:latest@sha256:…"` (pinned) |
| – | `[preview.images] typst` | `"ghcr.io/typst/typst:latest@sha256:…"` (pinned) |
| – | `[preview.images] ghostscript` | `"docker.io/jbarlow83/ocrmypdf-alpine:latest@sha256:…"` (pinned) |
| – | `[preview.images] ocrmypdf` | `"docker.io/jbarlow83/ocrmypdf-alpine:latest@sha256:…"` (pinned) |
| – | `[preview.images] asciidoctor` | `"docker.io/asciidoctor/docker-asciidoctor:latest@sha256:…"` (pinned) |
| – | `[preview.images] drawio` | `"docker.io/rlespinasse/drawio-desktop-headless:latest@sha256:…"` (pinned) |
| – | `[preview.images] duckdb` | `"ghcr.io/mwo-dk/coxswain-duckdb:latest@sha256:…"` (pinned) ([Data](data.md)) |
| – | `[preview.images] gxps` | `""` (none) |
| – | `[preview.images] ffmpeg` | `"docker.io/mwader/static-ffmpeg:latest@sha256:…"` (pinned) |
| – | `[preview.images] imagemagick` | `"ghcr.io/mwo-dk/coxswain-imagemagick:latest@sha256:…"` (pinned) |
| – | `[preview.images] kicad` | `"docker.io/kicad/kicad:9.0.9@sha256:…"` (pinned) |
| – | `[preview.images] stockfish` | `"ghcr.io/mwo-dk/coxswain-stockfish:latest@sha256:…"` (pinned) ([The engine](chess-play.md#the-engine)) |
| – | `[preview.images] openscad` | `"docker.io/openscad/openscad:trixie.2026-10-19@sha256:…"` (pinned) |
| – | `[preview.images] gdal` | `"ghcr.io/osgeo/gdal:alpine-small-3.13.3@sha256:…"` (pinned) |
| – | `[preview.images] tika` | `"docker.io/apache/tika:4.1.0@sha256:…"` (pinned) ([Other formats with Apache Tika](../search/tika.md)) |
| – | `[preview.images] calibre` | `"docker.io/linuxserver/calibre:9.16.0@sha256:…"` ([E-books](ebooks.md)) |
| – | `[preview.images] whisper` | `"ghcr.io/ggml-org/whisper.cpp:main@sha256:…"` (pinned) ([Transcripts of speech](../search/transcripts.md)) |
| [*Preview tools*](tools.md#preview-tools-in-settings) | `[preview.tools]` | **Pull** and **Remove** for each kind's image, with what it gives, its size and cost; the choice per kind (`"local"`, `"container"`, `"off"`) |
| *Keep images up to date* | `[preview] image_updates` | `"off"`, `"weekly"`, `"monthly"`; default `"off"` |
| *Remove images not used for 90 days* | `[preview] image_prune` | `true`, `false`; default `false` |
| *Timeout (seconds)* | `[preview] timeout` | `120` |

An empty image means that tool never uses a container: Settings → Previews → Preview tools then
greys out *Container* for its kind and says *Container: no image; name one under
[preview.images] … in config.toml*, with **Open config.toml** (terminal app: **O**). GhostXPS has
no image Coxswain picks for you. An image you set must provide the command Coxswain runs: `soffice` for
LibreOffice, `typst` as the entry point for Typst, `gs` and `gxps` on its `PATH` (Coxswain sets
`--entrypoint`), `ocrmypdf` as the entry point for OCRmyPDF, Kroki's program names (`d2`,
`ditaa`, `svgbob`, `pikchr`, `nomnoml`, `wavedrom`, `vega`, `dbml`, `erd`, `bytefield`) on its
`PATH` for Kroki, `duckdb` as the entry point for DuckDB, `/ffmpeg` and `/ffprobe` for ffmpeg,
`magick` on its `PATH` for ImageMagick, GDAL's programs on its `PATH` for GDAL, `openscad`
on its `PATH` for OpenSCAD, `clamscan` and `freshclam` for ClamAV, `kicad-cli` on its `PATH` for KiCad, `sh`, `ebook-convert`, `ebook-meta` and `tar` on its `PATH` for
calibre, for Whisper `ffmpeg`, `whisper-cli` and `sh` on its `PATH`, and for Tika an `apache/tika` server image (`java` and Tika's jars in `/opt/tika-server`).

## In the terminal app

The terminal app makes no previews, so it starts no preview containers. Apache Tika's and whisper.cpp's run
for it through the search helper when [Read other formats with Apache Tika](../search/tika.md) or
[Transcribe speech with Whisper](../search/transcripts.md) is on.
The containers it runs itself are OCRmyPDF's, for [Make PDF searchable](../files/searchable-pdf.md) (**Alt+P**), and
ffmpeg's, ImageMagick's, GDAL's, OpenSCAD's and KiCad's, for [Convert video or audio](../files/convert-media.md),
[Convert or edit pictures](../files/edit-pictures.md), [Convert geodata](geodata.md#convert-geodata),
[Export 3D model](openscad.md#export-3d-model), [Export from KiCad](kicad.md#export-from-kicad), calibre's, for [e-books](ebooks.md)
(**F3** on a book, *Convert e-book*), Asciidoctor's, for [Build PDF of
AsciiDoc](../files/asciidoc-pdf.md), and draw.io's, for [Export draw.io
diagram](../files/drawio-export.md) (with `[preview] drawio` on), and ClamAV's, for [Scan for viruses](../files/virus-scan.md)
(**Alt+I**), the same way as the desktop app: asked
first, with the first pull and its size. The chess engine's image it runs when it is pulled (by
the desktop app's Settings, or `podman pull ghcr.io/mwo-dk/coxswain-stockfish:latest`), never
pulling it itself. **Properties** (**Alt+Enter**) reads a video's facts
with an installed ffprobe, else in ffmpeg's image when it is pulled, never pulling it, and
geodata's with GDAL the same way. Its
`config.toml` is the same file: of the `[preview]` section it uses the runtime, `prefer`,
`tools`, `ebooks`, `drawio`, `virus_scan` and the OCRmyPDF, ffmpeg, ImageMagick, GDAL, OpenSCAD, KiCad, calibre, Asciidoctor, draw.io and ClamAV images (and its helper `images.tika`). Its **F12** → *Previews* shows
[*Preview tools*](tools.md#preview-tools-in-settings), a line per kind, where **P** pulls a kind's image and **Delete**
removes it, and *Keep images up to date* and *Remove images not used for 90 days*, so you can
set them from there; the desktop app is what updates and removes unused images, while it runs. Its Disk use rows for the
images (`coxswain --disk`) say what each one gives, its size and cost, as the desktop app does.

## Questions

#### Is it safe to build someone else's LaTeX document in a container?

That is what the container is for. It has no network, sees the project read-only, can write
only to a fresh folder in the cache, and LaTeX runs without `-shell-escape`, so the document
cannot run commands.

#### Why does the first LaTeX preview take so long?

The TeX Live image is about 2.8 GB to download (5.9 GB on disk) and is downloaded the first
time. Set `[preview.images] latex = "docker.io/texlive/texlive:latest-medium"` (about 960 MB to
download, 2.2 GB on disk) for a smaller one, or install a TeX distribution.

#### The LaTeX container never starts by itself.

Its image is not pulled yet, and a 2.8 GB download never starts without a click. Press **Build
PDF**, or **Pull** it in Settings first.

#### Does Coxswain update the images?

Only when you turn it on: *Settings → Previews → Keep images up to date* → **Weekly** or
**Monthly** has the desktop app pull the images you already pulled, in the background and never
on battery, and remove the copy each one replaced. **Pull** in Settings fetches the newest
version of the tag at once.

#### Will an update leave old copies behind?

No. When a pull brings a new version, the old copy is removed straight away by its own id. If a
running container still uses it, or you gave it another name, it stays, and you can remove it
with `podman image rm` yourself. Coxswain never runs `image prune`, so your own untagged images
are safe.

#### Why did an image disappear?

*Remove images not used for 90 days* is on, and neither a preview, Make PDF searchable nor Scan for viruses used it (nor Apache Tika read a file with it) for that long. The next
preview that needs it says *First run pulls …* and waits for a click. Turn the option off in
*Settings → Previews* to keep every image.

#### I turned on *Remove images not used for 90 days*. Will it remove everything now?

No. An image with no use noted yet counts as used on the day the app first looks, so nothing
goes for 90 days unless a preview ran in it longer ago than that.

#### A preview in a folder with a `:` in its name failed with *incorrect volume format*.

Older versions mounted the folder with `-v`, which splits at every `:`. The folder is now
mounted with `--mount`, which takes any name, with `,` and `"` too. Press **Try again**.

#### Why is my home folder not visible to the container?

Only the file's folder (for LaTeX, the project) is mounted. A LaTeX project without a git
repository reaches as far up as its `../` paths go, three folders at most, and never your home
folder itself.

#### Can I use docker even though podman is installed?

Yes: *Container runtime → docker* (`container = "docker"`). `auto` takes podman when both are
there.

#### I use SELinux. Will my folders be relabelled?

No. Containers run with `--security-opt label=disable`, so no `:z` relabelling is ever done
to your folders.

#### Why Gotenberg's image for LibreOffice?

LibreOffice publishes no image of its own. Gotenberg's `8-libreoffice` is the most used one with
a current LibreOffice, rebuilt every few weeks, and small for what it holds (about 440 MB).
Coxswain runs only `soffice` in it, once per file, with no network; Gotenberg's own server never
starts.

#### Where does the DuckDB image come from?

DuckDB publishes no image, so Coxswain makes its own, `ghcr.io/mwo-dk/coxswain-duckdb`: DuckDB's
released command line, checked by its digest and unchanged, with the three libraries it needs
and nothing else. Its recipe is `packaging/images/duckdb/Dockerfile`. To use another, set it
under `[preview.images] duckdb`; set `""` for none.

#### Kroki's image is 1.2 GB. Do I need it for one D2 file?

No: an installed `d2` (or `svgbob`, `pikchr`, `erd`) is used first. The image holds the programs
of all the kinds at once, so it is pulled only when you click **Render** on one with no program
installed, after the note says how big it is.

#### Why that ffmpeg image?

FFmpeg publishes no image of its own. `mwader/static-ffmpeg` is the most used static build: one
ffmpeg and one ffprobe with the common codecs built in (H.264, H.265, VP9, AV1, Opus, MP3 and
more), no shell or other programs beside them, rebuilt with each release. It is small for what
it holds (about 126 MB). An installed ffmpeg is used first all the same.

#### Does ImageMagick's image show my HEIC and RAW photos?

Yes. Coxswain's own image (`packaging/images/imagemagick`) is Alpine's ImageMagick with libheif
and its HEVC decoder for HEIC and AVIF, and LibRaw for cameras' RAW photos: no other well-kept
ImageMagick image had both in 2026. It holds nothing else, no shell either, and runs under the
same policy as an installed ImageMagick ([Convert or edit pictures](../files/edit-pictures.md#how-a-picture-is-kept-from-doing-harm)).

#### Why is GDAL's image pinned to one version?

GDAL's images are its own, made by the GDAL project with each release; the small Alpine one is
about 40 MB and has every driver the [geodata](geodata.md) previews use. Coxswain names it by
its digest, as every default image is ([Pinned images](#pinned-images)), so the maps and
conversions stay the same until a Coxswain release moves it on, after it has been tried. Set
`[preview.images] gdal = "ghcr.io/osgeo/gdal:alpine-small-latest"` to follow GDAL's newest.

#### Why is ImageMagick's container used before an installed ImageMagick?

ImageMagick reads hundreds of formats, and a picture made to attack one of them is a real risk
with files from elsewhere. In its container it has no network and sees only the one file. An
installed one is used when the image is not pulled or cannot read the file, under a policy that
allows picture formats only. For ffmpeg the installed one comes first, as it is usually the
better build for your machine.

#### Does a container run a server?

A preview's never: Kroki and Gotenberg are servers by design, but Coxswain runs only the programs
inside their images, one per file: the container starts, makes the picture or PDF, and is
removed. No port is opened. The one server is Apache Tika's, for search, when you turn on
[Read other formats with Apache Tika](../search/tika.md): it has no network and no port on your
machine, gets each file on its standard input, and stops itself after 5 idle minutes.

---
[← Previous: Previews made by tools](tools.md) · [Next: Safety, speed and limits →](safety.md)

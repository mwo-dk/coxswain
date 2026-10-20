[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Containers

When a tool a preview needs is not installed, Coxswain can run it from a container image with
podman or docker: the TeX Live image builds any LaTeX document, Typst's builds Typst, Gotenberg's
LibreOffice turns office files and clip art into PDFs, Kroki's draws D2, ditaa and ten more
diagram kinds, the PlantUML and pandoc images draw diagrams and reStructuredText, and OCRmyPDF's
makes scanned PDFs searchable and has the Ghostscript that shows PostScript. A container gets no
network and sees your files read-only. Nothing is downloaded until you click.

<!-- screenshot: previews-container-pull.png: desktop app, Cyber theme: a .tex file whose engine is "podman texlive:latest", first run; the preview shows "Rendering with podman texlive:latest…" and under it "Pulling docker.io/texlive/texlive:latest…" with podman's progress line -->

## Contents

- [How to use it](#how-to-use-it)
- [The images](#the-images)
- [What a container may do](#what-a-container-may-do)
- [Pulling and removing images](#pulling-and-removing-images)
- [Keeping images up to date](#keeping-images-up-to-date)
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
   of time: *Settings → Previews → Container images → **Pull***.
4. From then on the container runs by itself like an installed program.
5. To free the room again: *Settings → Previews → Container images → **Remove***.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane | – |
| **Ctrl+,** | Settings → *Previews*, with *Container images*, *Keep images up to date* and *Remove images not used for 90 days* | **F12** → *Previews*: the runtime, the LaTeX image and the two image options, for the desktop app (no *Container images* list, no pulling) |

## The images

Each image under *Settings → Previews → Container images* says what it gives, how big it is and
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
| one you set for DuckDB | The tables of `.duckdb` files, with their columns and row counts | depends on the image | depends on the image | Light: one quick read of the file |
| one you set for GhostXPS (`gxps`) | XPS and OpenXPS documents shown as a PDF | depends on the image | depends on the image | Light |

Where each image comes from, and why it is the one:

| Image | Made by | Licence of what runs in it | Kept up to date |
|---|---|---|---|
| `gotenberg/gotenberg:8-libreoffice` | The Gotenberg project; Coxswain runs only the LibreOffice in it, never Gotenberg's server | LibreOffice MPL-2.0, Gotenberg MIT | A new build every few weeks; LibreOffice publishes no image of its own |
| `yuzutech/kroki:latest` | The Kroki project; Coxswain runs the diagram programs in it one per call, never Kroki's server | MIT (Kroki), each program its own (D2 MPL-2.0, svgbob and Pikchr MIT or 0BSD, …) | Every few weeks |
| `jbarlow83/ocrmypdf-alpine:latest` | OCRmyPDF's author, with each release | OCRmyPDF MPL-2.0, Tesseract Apache-2.0, Ghostscript AGPL-3.0 | With every OCRmyPDF release (weekly in 2026) |
| `ghcr.io/typst/typst:latest` | The Typst project | Apache-2.0 | With every Typst release |

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
| `--mount type=bind,source=<folder>,target=/src,readonly` | The file's folder, read-only: nothing in it can be changed. A folder whose name has a `:` (*Meeting 10:30*) mounts too, which `-v` could not do. For LaTeX, the whole [project](latex.md#what-coxswain-works-out), so chapters, pictures and the bibliography are found |
| `--mount type=bind,source=<cache>,target=/out` | A fresh folder in the preview cache, the only place it can write |
| `--security-opt label=disable` | No SELinux relabelling, so your folders' labels are never changed |
| `--user <you>` and `-e HOME=/tmp` (docker, and podman run as root, on Linux and macOS) | Rootful docker or podman would leave root-owned files in the cache; your user has no home in the image, so programs that keep a profile (LibreOffice) keep it in the container's `/tmp` |
| `--user 0:0` (rootless podman) | In rootless podman the container's root is you, so an image whose own user is another one (Gotenberg's, OCRmyPDF's) can write its result to `/out`. Whether podman runs rootless is asked once, with `podman info` |
| `--name coxswain-preview-<hash>` | So a run past the timeout can be killed |

Inside, the commands are: `latexmk -pdf -interaction=nonstopmode -outdir=/out` for LaTeX,
`typst compile --root /src` for Typst, `soffice -env:UserInstallation=file:///tmp/lo --headless --convert-to pdf --outdir /out`
for LibreOffice, `gs -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pdfwrite` for PostScript (`--entrypoint gs`),
`ocrmypdf --skip-text -l <languages>` for [Make PDF searchable](../files/searchable-pdf.md), the
PlantUML and pandoc images' own entry points reading the file on standard input (`-tsvg -pipe`,
`-f rst -t html5`), Kroki's programs one per call, reading the diagram on standard input
(`--entrypoint d2` with `- -`, `ditaa --svg - -`, `svgbob`, `pikchr --svg-only -`, …), and
`duckdb -readonly -json` for DuckDB. LaTeX runs without `-shell-escape`, so a document cannot run
commands. No container ever starts a server or opens a port: each run does one file and ends.

A run longer than the timeout (120 s by default) is stopped and its container killed. Pulling
the image is not counted.

## Pulling and removing images

- **The first run pulls the image,** and only after a click. The engine button's note says so
  beforehand (*First run pulls docker.io/texlive/texlive:latest (about 2.8 GB)*, or *First run
  pulls docker.io/plantuml/plantuml:latest (about 141 MB)*; an image you set yourself shows no
  size), and while it downloads the preview shows
  *Pulling docker.io/texlive/texlive:latest…* with the runtime's own progress line.
- Once pulled, the note says *Runs docker.io/texlive/texlive:latest without network access*.
- **Settings lists the images** under *Container images*: each with what it gives, its size and
  cost ([The images](#the-images)), *Pulled, 5.9 GB* or *Not pulled* (the progress line while
  pulling), **Pull** to download it ahead of time or update it to the newest, and **Remove** to
  free the space.
- Unless you turn on one of the two options below, Coxswain never updates or removes an image
  by itself.

## Keeping images up to date

*Settings → Previews → Keep images up to date* (`[preview] image_updates`): **Off** (the
default), **Weekly** or **Monthly**.

- **What it updates:** only the images under `[preview.images]` that are already pulled. One
  you never pulled is never downloaded by it.
- **When:** the desktop app looks once an hour while it runs (the first time ten minutes after
  it starts). When a week or a month has passed since the last round, it runs `podman pull`
  (or `docker pull`) for each of those images, in the background. Not on battery, and not with
  *Container runtime → No containers*. Each image is tried, even after one fails. When every
  pull fails (no network), the round stays due and the next hour tries again; when only some
  fail, the round is done and those wait for the next one.
- **What you see:** nothing pops up. While it pulls, *Settings → Previews → Container images*
  shows the runtime's progress line under that image, as for **Pull**, and the image's
  **Pull** and **Remove** wait.
- **The old copy goes at once.** When a pull brought a new version, the copy it replaced is
  removed by its own id (`podman image rm <id>`). Coxswain never runs `image prune`, which would
  also take untagged images of yours that have nothing to do with it. A copy that cannot be
  removed, because a container still uses it or another name points at it, stays; nothing is
  said and it is not tried again.
- **Where it goes:** the registry in the image's name, `docker.io` for the defaults, `ghcr.io` for Typst's.
  *Settings → Privacy and updates → What can leave this machine* then says *Container images for
  previews, when pulled and each week to keep them up to date* (or *each month*), with the
  registry.
- **The terminal app** makes no container previews, so it updates nothing; its **F12** →
  *Previews* shows the option, and the desktop app does the updating.

## Removing unused images

*Settings → Previews → Remove images not used for 90 days* (`[preview] image_prune`), off by
default.

- Both apps note, in `state.json`, when a preview or [Make PDF searchable](../files/searchable-pdf.md)
  last ran in each image. Once an hour (not on battery) the desktop app removes a pulled image
  of `[preview.images]` that neither used for 90 days, by name, as **Remove** does.
- An image with no use noted yet, such as one pulled before you turned the option on, counts as
  used on the day the app first looks, so turning it on never removes all of them at once.
- The next preview that needs a removed image says *First run pulls …* again and waits for your
  click.
- Only Coxswain's own images are ever touched. The terminal app shows the option; the desktop
  app does the removing.

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
| *LaTeX image* | `[preview.images] latex` | `"docker.io/texlive/texlive:latest"` |
| – | `[preview.images] plantuml` | `"docker.io/plantuml/plantuml:latest"` |
| – | `[preview.images] pandoc` | `"docker.io/pandoc/core:latest"` |
| – | `[preview.images] libreoffice` | `"docker.io/gotenberg/gotenberg:8-libreoffice"` |
| – | `[preview.images] kroki` | `"docker.io/yuzutech/kroki:latest"` |
| – | `[preview.images] typst` | `"ghcr.io/typst/typst:latest"` |
| – | `[preview.images] ghostscript` | `"docker.io/jbarlow83/ocrmypdf-alpine:latest"` |
| – | `[preview.images] ocrmypdf` | `"docker.io/jbarlow83/ocrmypdf-alpine:latest"` |
| – | `[preview.images] duckdb` | `""` (none) |
| – | `[preview.images] gxps` | `""` (none) |
| *Container images* | – | **Pull** and **Remove** for each image set, with what it gives, its size and cost |
| *Keep images up to date* | `[preview] image_updates` | `"off"`, `"weekly"`, `"monthly"`; default `"off"` |
| *Remove images not used for 90 days* | `[preview] image_prune` | `true`, `false`; default `false` |
| *Timeout (seconds)* | `[preview] timeout` | `120` |

An empty image means that tool never uses a container. DuckDB and GhostXPS have no image
Coxswain picks for you. An image you set must provide the command Coxswain runs: `soffice` for
LibreOffice, `typst` as the entry point for Typst, `gs` and `gxps` on its `PATH` (Coxswain sets
`--entrypoint`), `ocrmypdf` as the entry point for OCRmyPDF, Kroki's program names (`d2`,
`ditaa`, `svgbob`, `pikchr`, `nomnoml`, `wavedrom`, `vega`, `dbml`, `erd`, `bytefield`) on its
`PATH` for Kroki, and `duckdb` as the entry point for DuckDB.

## In the terminal app

The terminal app makes no previews, so it starts no preview containers. The one container it
runs is OCRmyPDF's, for [Make PDF searchable](../files/searchable-pdf.md) (**Alt+P**), the same
way as the desktop app: asked first, with the first pull and its size. Its `config.toml` is the
same file: of the `[preview]` section it uses the runtime, `prefer` and the OCRmyPDF image. Its **F12** → *Previews* still
shows *Keep images up to date* and *Remove images not used for 90 days*, so you can set them
from there; the desktop app is what pulls and removes, while it runs. Its Disk use rows for the
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

*Remove images not used for 90 days* is on, and neither a preview nor Make PDF searchable used it for that long. The next
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

#### Why is DuckDB not offered as a container?

DuckDB publishes no image, and Coxswain will not pick an unofficial one for you. Set one you
trust under `[preview.images] duckdb`.

#### Kroki's image is 1.2 GB. Do I need it for one D2 file?

No: an installed `d2` (or `svgbob`, `pikchr`, `erd`) is used first. The image holds the programs
of all the kinds at once, so it is pulled only when you click **Render** on one with no program
installed, after the note says how big it is.

#### Does a container run a server?

No. Kroki and Gotenberg are servers by design, but Coxswain runs only the programs inside their
images, one per file: the container starts, makes the picture or PDF, and is removed. No port is
opened.

---
[← Previous: Previews made by tools](tools.md) · [Next: Safety, speed and limits →](safety.md)

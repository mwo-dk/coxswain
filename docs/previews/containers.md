[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Containers

When a tool a preview needs is not installed, Coxswain can run it from a container image with
podman or docker: the TeX Live image builds any LaTeX document, the PlantUML and pandoc images
draw diagrams and reStructuredText. A container gets no network and sees your files read-only.

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
| one you set for LibreOffice | Word, PowerPoint, Visio and OpenDocument files turned into the PDF the preview shows | depends on the image | depends on the image | LibreOffice starts for each document: a few seconds |
| one you set for DuckDB | The tables of `.duckdb` files, with their columns and row counts | depends on the image | depends on the image | Light: one quick read of the file |

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
| `-v <folder>:/src:ro` | The file's folder, read-only: nothing in it can be changed. For LaTeX, the whole [project](latex.md#what-coxswain-works-out), so chapters, pictures and the bibliography are found |
| `-v <cache>:/out` | A fresh folder in the preview cache, the only place it can write |
| `--security-opt label=disable` | No SELinux relabelling, so your folders' labels are never changed |
| `--user <you>` (docker on Linux and macOS) | Rootful docker would leave root-owned files in the cache |
| `--name coxswain-preview-<hash>` | So a run past the timeout can be killed |

Inside, the commands are: `latexmk -pdf -interaction=nonstopmode -outdir=/out` for LaTeX,
`soffice --headless --convert-to pdf --outdir /out` for LibreOffice, the PlantUML and pandoc
images' own entry points reading the file on standard input (`-tsvg -pipe`, `-f rst -t
html5`), and `duckdb -readonly -json` for DuckDB. LaTeX runs without `-shell-escape`, so a
document cannot run commands.

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
- **Where it goes:** the registry in the image's name, `docker.io` for the defaults.
  *Settings → Privacy and updates → What can leave this machine* then says *Container images for
  previews, when pulled and each week to keep them up to date* (or *each month*), with the
  registry.
- **The terminal app** makes no container previews, so it updates nothing; its **F12** →
  *Previews* shows the option, and the desktop app does the updating.

## Removing unused images

*Settings → Previews → Remove images not used for 90 days* (`[preview] image_prune`), off by
default.

- The desktop app notes, in `state.json`, when a preview last ran in each image. Once an hour
  (not on battery) it removes a pulled image of `[preview.images]` that no preview used for 90
  days, by name, as **Remove** does.
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
| – | `[preview.images] libreoffice` | `""` (none) |
| – | `[preview.images] duckdb` | `""` (none) |
| *Container images* | – | **Pull** and **Remove** for each image set, with what it gives, its size and cost |
| *Keep images up to date* | `[preview] image_updates` | `"off"`, `"weekly"`, `"monthly"`; default `"off"` |
| *Remove images not used for 90 days* | `[preview] image_prune` | `true`, `false`; default `false` |
| *Timeout (seconds)* | `[preview] timeout` | `120` |

An empty image means that tool never uses a container. LibreOffice and DuckDB have no official
images. If you set one, it must provide the command Coxswain runs: `soffice` for LibreOffice;
for DuckDB, the image's entry point must be `duckdb` itself.

## In the terminal app

The terminal app starts no containers: it has no previews to make. Its `config.toml` is the
same file, so the `[preview]` section is simply not used there. Its **F12** → *Previews* still
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

*Remove images not used for 90 days* is on, and no preview used it for that long. The next
preview that needs it says *First run pulls …* and waits for a click. Turn the option off in
*Settings → Previews* to keep every image.

#### I turned on *Remove images not used for 90 days*. Will it remove everything now?

No. An image with no use noted yet counts as used on the day the app first looks, so nothing
goes for 90 days unless a preview ran in it longer ago than that.

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

#### Why are LibreOffice and DuckDB not offered as containers?

Neither project publishes an official image, and Coxswain will not pick an unofficial one for
you. Set one you trust under `[preview.images]`.

---
[← Previous: Previews made by tools](tools.md) · [Next: Safety, speed and limits →](safety.md)

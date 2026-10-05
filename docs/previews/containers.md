[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Containers

When a tool a preview needs is not installed, Coxswain can run it from a container image with
podman or docker: the TeX Live image builds any LaTeX document, the PlantUML and pandoc images
draw diagrams and reStructuredText. A container gets no network and sees your files read-only.

<!-- screenshot: previews-container-pull.png: desktop app, Cyber theme: a .tex file whose engine is "podman texlive:latest", first run; the preview shows "Rendering with podman texlive:latest…" and under it "Pulling docker.io/texlive/texlive:latest…" with podman's progress line -->

## How to use it

1. Install podman (or docker). Nothing else: the default images are set.
2. Put the cursor on a file whose tool is not installed, e.g. a `.tex` file, and open the pane
   (**Space** or **F3**). The engine button reads **podman texlive:latest**.
3. The first time, the image has to be downloaded. The preview says *First run pulls
   docker.io/texlive/texlive:latest (about 5 GB)* under **Build PDF**; press it. Or pull ahead
   of time: *Settings → Previews → Container images → **Pull***.
4. From then on the container runs by itself like an installed program.
5. To free the room again: *Settings → Previews → Container images → **Remove***.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane | – |
| **Ctrl+,** | Settings → *Previews*, with *Container images* | – |

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
  beforehand (*First run pulls docker.io/texlive/texlive:latest (about 5 GB)*, or *First run
  pulls docker.io/plantuml/plantuml:latest*), and while it downloads the preview shows
  *Pulling docker.io/texlive/texlive:latest…* with the runtime's own progress line.
- Once pulled, the note says *Runs docker.io/texlive/texlive:latest without network access*.
- **Settings lists the images** under *Container images*: each with *Pulled, 5.1 GB* or *Not
  pulled* (the progress line while pulling), **Pull** to download it ahead of time or update it
  to the newest, and **Remove** to free the space.
- Coxswain never updates or removes an image by itself.

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
| *Container images* | – | **Pull** and **Remove** for each image set |
| *Timeout (seconds)* | `[preview] timeout` | `120` |

An empty image means that tool never uses a container. LibreOffice and DuckDB have no official
images. If you set one, it must provide the command Coxswain runs: `soffice` for LibreOffice;
for DuckDB, the image's entry point must be `duckdb` itself.

## In the terminal app

The terminal app starts no containers: it has no previews to make. Its `config.toml` is the
same file, so the `[preview]` section is simply not used there.

## Questions

#### Is it safe to build someone else's LaTeX document in a container?

That is what the container is for. It has no network, sees the project read-only, can write
only to a fresh folder in the cache, and LaTeX runs without `-shell-escape`, so the document
cannot run commands.

#### Why does the first LaTeX preview take so long?

The TeX Live image is about 5 GB and is downloaded the first time. Set `[preview.images] latex
= "docker.io/texlive/texlive:latest-medium"` (about 2 GB) for a smaller one, or install a TeX
distribution.

#### The LaTeX container never starts by itself.

Its image is not pulled yet, and a 5 GB download never starts without a click. Press **Build
PDF**, or **Pull** it in Settings first.

#### Does Coxswain update the images?

No. **Pull** in Settings fetches the newest version of the tag when you press it. Nothing is
updated or removed by itself.

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

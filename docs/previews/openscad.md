[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# OpenSCAD models: pictures and Export 3D model

An OpenSCAD model (`.scad`) shows as a picture of the part it makes, in OpenSCAD's yellow, seen
from the front right and above, the way OpenSCAD first shows it; a flat (2D) model shows as its
outline. *Export 3D model* writes the part as STL or 3MF next to the model, for a slicer or
another CAD program. [OpenSCAD](https://openscad.org/) makes the part, as installed or from its
container image. Both apps; the picture is the desktop app's.

![The desktop app in the Cyber theme with engine-bell.scad under the cursor in Documents/Models: the preview shows the Rendered / Source switch, the engine buttons openscad and container, a yellow engine bell on its round mount with six bolt holes, seen from the front right and above, and under it the Export 3D model button](../screenshots/openscad-model.png)
*An OpenSCAD model: the part OpenSCAD makes of it, drawn by Coxswain.*

## Contents

- [How to use it](#how-to-use-it)
- [The picture](#the-picture)
- [Export 3D model](#export-3d-model)
- [What it needs](#what-it-needs)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a `.scad` file and press **Space** or **F3** for the preview. The picture
   comes once OpenSCAD has made the part (a second for most models), and is kept in the preview
   cache.
2. **Rendered** and **Source** at the top of the pane switch between the picture and the
   model's code. The choice sticks for the next file.
3. **Export 3D model** under the picture, or *Export 3D model* in the action menu
   (**Shift+F10**) or in **F9**, writes an STL or 3MF next to the model
   ([below](#export-3d-model)).

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane with the picture | **F3** opens the model's code in the built-in viewer |
| **Shift+F10**, **F9** | *Export 3D model* in the action menu (on a model, or with some marked) and in the command list | The same |
| **1**, **2**, **Enter** | Picks *As STL* or *As 3MF* in its menu | The same |

## The picture

OpenSCAD makes the model's part as STL, with Manifold (`--backend=manifold`) when it has it,
which takes a second where the older CGAL took minutes. Coxswain draws that STL itself: 1024 ×
768 pixels on a clear background, turned 25° and tipped as OpenSCAD's first view, every face lit
from the upper left and from the eye, in OpenSCAD's yellow. It does not use OpenSCAD's own view,
which needs a graphics card (OpenGL) that a container has not; so the picture looks the same
with an installed OpenSCAD and with its container.

A flat model (only 2D shapes: `square`, `circle`, `polygon` …) cannot be STL; it shows as
OpenSCAD's own SVG of its outline instead. A model that makes nothing says so: *OpenSCAD made an
empty model.* A model too large to draw (an STL over 200 MB, or faces that would take more than
about 20 seconds to draw) says *The model is too large to draw.*

`include` and `use` find the files beside the model: OpenSCAD runs in the model's folder (the
container sees that folder, read-only). A library installed in your OpenSCAD library folder is
found by an installed OpenSCAD only.

In the thumbnails view, a model shows its picture as its tile when OpenSCAD runs without a
pull.

## Export 3D model

1. Put the cursor on a model, or mark several (among other files, only the models are done).
2. Pick *Export 3D model* in the action menu (**Shift+F10**) or in **F9**, or click **Export 3D
   model** under the preview.
3. A menu offers **1** *As STL* and **2** *As 3MF*.
4. A question says what it writes (*Writes engine-bell.stl next to engine-bell.scad.
   engine-bell.scad itself is not changed.*), which program runs, and, when the image is not
   there yet, *First run pulls docker.io/openscad/openscad:trixie.2026-10-19 (about 241 MB)*.
   **Enter** (or **Convert**) starts it; **Esc** leaves everything as it is.
5. The status line says *Converting engine-bell.scad, 1 s*; **Stop** (desktop app), or the
   action again, stops it. Done: *Made engine-bell.stl*.

| Choice | The new file | How |
|---|---|---|
| *As STL* | `engine-bell.stl` | Binary STL (`--export-format binstl`), what every slicer reads |
| *As 3MF* | `engine-bell.3mf` | 3MF, which keeps units and is smaller; PrusaSlicer, Cura, Bambu Studio and Windows read it |

Each name is the first one free (`engine-bell 2.stl` when `engine-bell.stl` is there); the name
is taken only when the finished file is put there, so a stopped or failed run leaves nothing
behind. A flat model says *This model is flat (2D): STL and 3MF need a 3D one.*

## What it needs

One of these, in the order of *Settings → Previews → How previews are made* (`[preview]
prefer`, or `prefer_tool.openscad` for OpenSCAD alone):

| Way | What runs | Systems |
|---|---|---|
| OpenSCAD installed | `openscad -o <file> --export-format binstl <the model>` at the lowest priority, with `--backend=manifold` when its `--help` names it (snapshots from 2025 on; the 2021.01 release has CGAL only, and is slow on large models) | FreeBSD: `pkg install openscad`; NetBSD: `pkgin install openscad`; OpenBSD: `pkg_add openscad`; Linux: `sudo pacman -S openscad`, `sudo apt install openscad`, `sudo dnf install openscad`, `sudo zypper install openscad`; macOS: `brew install --cask openscad`; Windows: `winget install --id OpenSCAD.OpenSCAD -e`. On a Mac it is found in `/Applications/OpenSCAD.app`, on Windows in `C:\Program Files\OpenSCAD` (`openscad.com`), though the installers put it on no `PATH` |
| Its container | podman or docker runs `docker.io/openscad/openscad:trixie.2026-10-19`, pinned to its digest, with no network, seeing the model's folder read-only, on a read-only root with bounded memory and processes ([What a container may do](containers.md#what-a-container-may-do)) | Linux, macOS and Windows with podman or docker. Not on FreeBSD, the other BSDs and illumos: the image is a Linux one, so install OpenSCAD there instead |

The image is about 241 MB to download and 635 MB on disk: the OpenSCAD project's own build of a
dated snapshot, with Manifold and 3MF. It is pulled only when you click **Render** on the
preview or confirm an export; never by itself.

With neither, the preview says what is missing, with the line that installs it and a
**Settings** button, and the action says *Exporting a 3D model needs OpenSCAD. Install it (sudo
apt install openscad), or run it in a container: …*.

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| The preview of a model | **Rendered** / **Source**, the engine buttons (`openscad`, *container*), the picture, **Export 3D model** | – |
| First run of the container | *Pulling docker.io/openscad/openscad:trixie.2026-10-19…* with podman's progress line | The same, for an export |
| The status line | *Shift+F10 / Menu on an OpenSCAD model: export it as STL or 3MF*, a few times | The same hint |
| While it exports | *Converting engine-bell.scad, 1 s* and **Stop** | *… · F9 → Export 3D model stops* |
| Done | *Made engine-bell.stl* (several: *Made 3 new files next to their originals*) | The same |
| Failed | A dialog *Could not convert the file* with OpenSCAD's last lines (the line of the model that stopped it), per file | The same, full screen |

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *How previews are made* | `[preview] prefer` (`"auto"`, `"local"`, `"container"`) | `"auto"`: installed first, else the container |
| – | `[preview.prefer_tool] openscad` | `[preview] prefer` |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Container images* (Pull, Remove) | `[preview.images] openscad` | `"docker.io/openscad/openscad:trixie.2026-10-19@sha256:bbed…"`; `""` never uses a container. An image you set must have `openscad` on its `PATH` |
| *Timeout (seconds)* | `[preview] timeout` | `120`: a picture whose making takes longer (OpenSCAD's runs together) is stopped, and drawing it has 20 s of its own; an export has no time limit |
| *Keys* | `[keys] export_model` | none |

OpenSCAD works in a folder of its own in the cache (`~/.cache/coxswain/media/`), removed when
each run ends.

## In the terminal app

The same *Export 3D model*, menu and question, as dialogs. A terminal has no pictures, so there
is no preview of the part: **F3** shows the model's code, and the desktop app or your slicer
shows the STL.

## Questions

#### Why is the picture not OpenSCAD's own view?

OpenSCAD's view needs a graphics card's OpenGL, which a container has not and a server often
lacks. So OpenSCAD makes the part, and Coxswain draws it: from the front right and above, in
OpenSCAD's yellow, the same with an installed OpenSCAD and with its container. A flat model
shows as OpenSCAD's own outline.

#### Can a model reach my other files?

In its container OpenSCAD sees only the model's folder, read-only, and has no network, so an
`import` or `include` of anything else finds nothing. An installed OpenSCAD reads what the
model's `include`, `use` and `import` lines name, as when you open the model in OpenSCAD
yourself; OpenSCAD has no way to write files or run programs from a model.

#### Is my model changed when I export it?

No. The STL or 3MF is a new file next to it (`part.stl`, `part.3mf`, with ` 2` added when the
name is taken), never written over another, and the model itself is only read.

#### Why does a large model take long, or stop after two minutes?

With Manifold most models take a second. An installed OpenSCAD 2021.01 has only CGAL, which can
take minutes on many curves (a high `$fn`): the picture stops at the timeout (`[preview]
timeout`, 120 s), an export runs until done. Install a newer snapshot, or let the container
run.

#### Why is the image a dated snapshot?

OpenSCAD's last release (2021.01) is old and has no Manifold; the project publishes dated
snapshots, and its own image of them. Coxswain names one by its digest, so the pictures stay
the same until a Coxswain release moves it on. *Keep images up to date* leaves a pinned image as
it is.

#### Can I turn the part or see it in 3D?

Not yet: the picture is one fixed view. Open the model in OpenSCAD (**Enter**), or the STL in
your slicer.

---
[← Previous: Geodata](geodata.md) · [Next: Previews made by tools →](tools.md)

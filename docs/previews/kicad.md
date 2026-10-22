[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# KiCad: schematics, boards and Export from KiCad

A KiCad schematic (`.kicad_sch`) shows in the preview as a PDF of its sheets, a page each; a
board (`.kicad_pcb`) as a PDF of its outer layers, a page each, with the board's outline on every
page. In the thumbnails view a schematic shows its first sheet and a board a 3D view of it from
above. *Export from KiCad* writes a PDF, an SVG, a bill of materials (CSV) or the Gerber and
drill files for a board maker next to the original. KiCad's `kicad-cli` does the work, as
installed or from KiCad's container image. Both apps; the preview is the desktop app's.

![The desktop app in the Cyber theme with igniter.kicad_sch under the cursor in Documents/Electronics, igniter.kicad_pcb above it: the preview shows the engine buttons kicad-cli and container and the schematic as page 1 of 1 in the PDF viewer, the battery and resistor loop with its note at the top left of the sheet and the title block at the bottom right](../screenshots/kicad-schematic.png)
*A KiCad schematic: its sheets as pages, by kicad-cli.*

## Contents

- [How to use it](#how-to-use-it)
- [The preview](#the-preview)
- [Export from KiCad](#export-from-kicad)
- [What it needs](#what-it-needs)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a `.kicad_sch` or `.kicad_pcb` file and press **Space** or **F3** for the
   preview. The PDF comes after a second or two and is kept in the preview cache.
2. Page through the sheets or layers in the PDF viewer.
3. **Export from KiCad** under the preview, or *Export from KiCad* in the action menu
   (**Shift+F10**) or in **F9**, writes a file next to it ([below](#export-from-kicad)).

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane with the PDF | **F3** opens the file's text in the built-in viewer |
| **Shift+F10**, **F9** | *Export from KiCad* in the action menu (on a schematic or board, or with some marked) and in the command list | The same |
| **1** … **4**, **Enter** | Picks what to make in its menu | The same |

## The preview

| File | The preview | The thumbnail |
|---|---|---|
| Schematic `.kicad_sch` | `kicad-cli sch export pdf`: every sheet of its hierarchy, a page each, with its title block | The first (root) sheet, as SVG |
| Board `.kicad_pcb` | `kicad-cli pcb export pdf --mode-multipage`: front and back copper, silkscreen and solder mask (`F.Cu`, `B.Cu`, `F.Silkscreen`, `B.Silkscreen`, `F.Mask`, `B.Mask`), a page each, the outline (`Edge.Cuts`) on every page | `kicad-cli pcb render`: the board in 3D from above, 1024 × 768 |

kicad-cli runs in the file's folder, so a schematic finds its sub-sheets and a board its
project beside it. The 3D view uses the 3D models KiCad has: an installed KiCad shows its
parts, KiCad's container image has no models of parts, so the board shows bare (copper, pads,
silkscreen). Inner copper layers (`In1.Cu` …) are not in the preview; *Export from KiCad*'s
Gerbers have every layer.

## Export from KiCad

1. Put the cursor on a schematic or board, or mark several (among other files, only the
   schematics and boards are done).
2. Pick *Export from KiCad* in the action menu (**Shift+F10**) or in **F9**, or click **Export
   from KiCad** under the preview.
3. A menu offers **1** *As PDF*, **2** *As SVG*, **3** *Bill of materials (CSV)*, **4** *Gerber
   and drill files (zipped)*.
4. A question says what it writes (*Writes igniter.pdf next to igniter.kicad_sch.
   igniter.kicad_sch itself is not changed.*), which program runs, and, when the image is not
   there yet, *First run pulls docker.io/kicad/kicad:9.0.9 (about 476 MB)*. **Enter** (or
   **Convert**) starts it; **Esc** leaves everything as it is.
5. Done: *Made igniter.pdf*, next to the original.

| Choice | Schematic | Board |
|---|---|---|
| *As PDF* | `igniter.pdf`: every sheet, a page each | `igniter.pdf`: the outer layers, a page each, the outline on every page |
| *As SVG* | `igniter.svg.zip`: each sheet as an SVG, zipped | `igniter.svg`: front and back copper, front silkscreen and the outline in one picture |
| *Bill of materials (CSV)* | `igniter.bom.csv`: reference, value, footprint, how many, DNP | – (*A bill of materials comes from a schematic (.kicad_sch).*) |
| *Gerber and drill files (zipped)* | – (*Gerber files come from a board (.kicad_pcb).*) | `igniter.gerbers.zip`: a Gerber file per layer as the board's own plot settings say, the drill file (Excellon) and KiCad's Gerber job file, as board makers take them |

Each name is the first one free (`igniter 2.pdf` when `igniter.pdf` is there); the name is
taken only when the finished file is put there, so a stopped or failed run leaves nothing
behind.

## What it needs

One of these, in the order of the *KiCad* row in *Settings → Previews → Preview tools*
(`[preview.tools] kicad`), else *How previews are made* (`[preview] prefer`):

| Way | What runs | Systems |
|---|---|---|
| KiCad 9 installed (its `kicad-cli`; KiCad 8's lacks the 3D view and the board PDF of a page per layer) | `kicad-cli` at the lowest priority, with your KiCad's settings and libraries | FreeBSD: `pkg install kicad`; NetBSD: `pkgin install kicad`; OpenBSD: `pkg_add kicad`; Linux: `sudo pacman -S kicad`, `sudo apt install kicad`, `sudo dnf install kicad`, `sudo zypper install kicad`; macOS: `brew install --cask kicad`; Windows: `winget install --id KiCad.KiCad -e`. On a Mac it is found in `/Applications/KiCad`, on Windows in `C:\Program Files\KiCad\9.0\bin`, though the installers put it on no `PATH` |
| Its container | podman or docker runs `docker.io/kicad/kicad:9.0.9`, pinned to its digest, with no network, seeing the file's folder read-only, on a read-only root with bounded memory and processes ([What a container may do](containers.md#what-a-container-may-do)) | Linux, macOS and Windows with podman or docker, on x86-64 (the image is amd64 only; on an ARM machine the runtime must emulate it, which is slow). Not on FreeBSD, the other BSDs and illumos: install KiCad there instead |

The image is large: about 476 MB to download and 1.7 GB on disk (KiCad with its symbol and
footprint libraries). It is pulled only when you click **Render** on the preview or confirm an
export, after the note has said how large it is; never by itself. An installed KiCad is used
first and needs nothing more.

With neither, the preview says what is missing, with the line that installs it and a
**Settings** button, and the action says *Exporting from KiCad needs KiCad's kicad-cli. Install
it (sudo apt install kicad), or run it in a container: …*.

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| The preview | The engine buttons (`kicad-cli`, *container*), the PDF, **Export from KiCad** | – |
| First run of the container | *Pulling docker.io/kicad/kicad:9.0.9…* with podman's progress line | The same, for an export |
| The status line | *Shift+F10 / Menu on a KiCad schematic or board: export it as PDF, SVG, a bill of materials or Gerbers*, a few times | The same hint |
| While it exports | *Converting igniter.kicad_pcb, 2 s* and **Stop** | *… · F9 → Export from KiCad stops* |
| Done | *Made igniter.gerbers.zip* | The same |
| Failed | A dialog *Could not convert the file* with kicad-cli's last lines | The same, full screen |

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *How previews are made* | `[preview] prefer` | `"auto"`: installed first, else the container |
| *Preview tools* → *KiCad* | `[preview.tools] kicad` (`"local"`, `"container"`, `"off"`) | `[preview] prefer` |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Preview tools* → *KiCad* (Pull, Remove) | `[preview.images] kicad` | `"docker.io/kicad/kicad:9.0.9@sha256:e638…"`; `""` never uses a container. An image you set must have `kicad-cli` on its `PATH` |
| *Timeout (seconds)* | `[preview] timeout` | `120`: a preview that takes longer is stopped; an export has no time limit |
| *Keys* | `[keys] export_kicad` | none |

kicad-cli works in a folder of its own in the cache (`~/.cache/coxswain/media/`), removed when
each run ends.

## In the terminal app

The same *Export from KiCad*, menu and question, as dialogs. A terminal shows no PDF, so there
is no preview: export a PDF and open it with **Enter**, or use the desktop app.

## Questions

#### Is my KiCad project changed?

No. Every result is a new file next to it (`amp.pdf`, `amp.gerbers.zip`, with ` 2` added when
the name is taken), never written over another; kicad-cli is asked only to export, into a folder of its own in the
cache. In the container the project's folder is read-only.

#### Why does the container take so long the first time?

KiCad's image is large: about 476 MB to download and 1.7 GB on disk, since it carries KiCad and
its libraries. It is pulled only after the note or the question said so. An installed KiCad is
used first and needs no download; once pulled, a preview takes a second or two.

#### Which layers does the board preview show?

Front and back copper, silkscreen and solder mask, a page each, with the board's outline on
every page. Inner layers of a board of four layers or more are not shown; the Gerbers of
*Export from KiCad* have every layer.

#### Why does the 3D thumbnail show no parts?

KiCad's container image has no 3D models of parts, so the board is drawn bare: copper, pads,
holes and silkscreen. An installed KiCad with its 3D library draws the parts too.

#### What goes into the Gerber zip?

What `kicad-cli pcb export gerbers` and `pcb export drill` make with the board's own plot
settings: a Gerber file per layer (`-F_Cu.gtl`, `-B_Cu.gbl`, `-Edge_Cuts.gm1` …), the drill
file (`.drl`) and the Gerber job file (`-job.gbrjob`). Most board makers take the zip as it is.

#### Does it work on my ARM Mac or Raspberry Pi?

With an installed KiCad, yes. KiCad's image is built for x86-64 only: podman on an ARM machine
must emulate it, which is slow, and may refuse without emulation set up.

---
[← Previous: OpenSCAD models](openscad.md) · [Next: Previews made by tools →](tools.md)

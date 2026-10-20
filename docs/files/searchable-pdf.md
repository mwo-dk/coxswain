[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Make PDF searchable

A scanned PDF holds pictures of its pages, not their words: no PDF reader can search it, and you
cannot select a line to copy it. **Alt+P** writes a copy of it with the words in it, next to the
original: `report.pdf` gets `report.searchable.pdf`. [OCRmyPDF](https://ocrmypdf.readthedocs.io/)
reads the pages; the copy looks the same and has its words as an invisible layer, so any program
can search and select them, and Find reads them as it reads any PDF. Both apps.

<!-- screenshot: searchable-pdf.png: desktop app, Cyber theme: a scanned PDF in the preview pane with the line "No text in this PDF: it looks like a scan. Make it searchable, and Find reads its words." and the button Make PDF searchable with Alt+P beside it; the dialog Make PDF searchable over it saying "Writes scan.searchable.pdf next to scan.pdf … It runs OCRmyPDF in podman (docker.io/jbarlow83/ocrmypdf-alpine:latest), without network access. First run pulls … (about 310 MB)." -->

## Contents

- [How to use it](#how-to-use-it)
- [What it needs](#what-it-needs)
- [What you see](#what-you-see)
- [Languages](#languages)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a PDF, or mark several (PDFs among other files: only the PDFs are done).
2. Press **Alt+P**, or pick *Make PDF searchable* in the action menu (**Shift+F10**) or in
   **F9**. In the desktop app the preview pane of a PDF has a **Make PDF searchable** button
   too, with its key beside it.
3. A question says what it writes (*Writes scan.searchable.pdf next to scan.pdf, with the words
   OCRmyPDF reads on its pages. scan.pdf itself is not changed.*), which program runs, and, when
   the container image is not there yet, *First run pulls docker.io/jbarlow83/ocrmypdf-alpine:latest
   (about 310 MB)*. **Enter** (or **Make searchable**) starts it; **Esc** leaves everything as it is.
4. The status line says how far it is: *Making scan.pdf searchable (2 of 5), 14 s*, with what
   OCRmyPDF said last. **Stop** (desktop app) or **Alt+P** again (both apps) stops it.
5. When it is done: *Made scan.searchable.pdf: Find reads its words from now on*, and the panel
   shows the new file next to the old one.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Alt+P** | Asks, then makes the copies; while one runs, stops it | The same |
| **Shift+F10**, **F9** | *Make PDF searchable* in the action menu (on a PDF, or with a PDF marked) and in the command list | The same |
| **Space** / **F3** | The PDF's preview: the **Make PDF searchable** button, and on a scan the line that offers it | – |

The copy is never written over anything: when `scan.searchable.pdf` is there already, the next
one is `scan.searchable 2.pdf`. A page that has text already is left as it is
(`--skip-text`), so a PDF that is part scan, part text gets the words of its scanned pages.

## What it needs

One of these, tried in the order of *Settings → Previews → How previews are made*
(`[preview] prefer`, or `prefer_tool.ocrmypdf` for this alone):

| Way | What runs | Systems |
|---|---|---|
| OCRmyPDF installed | `ocrmypdf --skip-text -l <languages> in.pdf out.pdf`, at the lowest priority | Linux: `sudo pacman -S ocrmypdf`, `sudo apt install ocrmypdf`, `sudo dnf install ocrmypdf`; macOS: `brew install ocrmypdf`; OpenBSD: `pkg_add ocrmypdf`; on FreeBSD, NetBSD, illumos and Windows get it from [its site](https://ocrmypdf.readthedocs.io/en/latest/installation.html) (pip) |
| Its container | podman or docker runs `docker.io/jbarlow83/ocrmypdf-alpine:latest`, OCRmyPDF's own image (MPL-2.0, updated by its author with every release), with no network and the PDF's folder read-only ([What a container may do](../previews/containers.md#what-a-container-may-do)) | Linux, macOS and Windows with podman or docker. Not on FreeBSD, the other BSDs and illumos: the image is a Linux one, so install OCRmyPDF there instead |

The image is about 310 MB to download and 850 MB on disk. It is pulled only after the question
said so and you pressed **Enter**; never by itself. The same image has Ghostscript, so it also
makes the [PostScript previews](../previews/documents.md#postscript-and-xps): one pull serves both.

With neither, **Alt+P** says what is missing instead of asking: *Make PDF searchable needs
OCRmyPDF. Install it (sudo apt install ocrmypdf), or run it in a container: Neither podman nor
docker is installed*, or *… containers are off (Settings → Previews → Container runtime).*, or
*… How previews are made is set to installed programs only*. The desktop app's dialog has a
**Settings → Previews** button that opens Settings there (at *Container runtime* when containers
are off); the terminal app's says *Settings: F12 → Previews*.

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| The preview of a scanned PDF (no text found on its pages) | *No text in this PDF: it looks like a scan. Make it searchable, and Find reads its words.* above the PDF, with **Make PDF searchable** and **Alt+P** | – |
| The preview of any other PDF | Only the **Make PDF searchable** button and its key | – |
| The status line | *Alt+P makes a scanned PDF searchable: a copy with its words in it*, a few times on a PDF | The same hint |
| While it runs | *Making scan.pdf searchable, 14 s · Parsing 1 pages with HocrParser* and a **Stop** button at the end of the command line | *Making scan.pdf searchable, 14 s · … · Alt+P stops* |
| First run of the container | *Pulling docker.io/jbarlow83/ocrmypdf-alpine:latest…* with the runtime's progress line | The same |
| Done | *Made scan.searchable.pdf: Find reads its words from now on* (several: *Made 5 searchable copies: …*) | The same |
| Stopped | *Stopped: the PDF it was working on got no copy*; the ones done before stay | The same |
| Failed | A dialog *Could not make the PDF searchable* with OCRmyPDF's last lines, per file | The same, full screen |

Whether a PDF is a scan is worked out by the search's own PDF reader once the PDF has been shown
for a second, at most three seconds, for PDFs up to 15 MB, and the answer is kept for that file
until it changes: fewer than 20 letters on its pages counts as none. A larger PDF gets only the
button. A PDF locked with a password
counts as a scan too, as no text can be read of it; OCRmyPDF then says it cannot open it.

## Languages

OCRmyPDF reads English and the language Coxswain is set to (*Settings → Language*, or your
system's), when it has them:

- **Installed:** the languages your tesseract has (`tesseract --list-langs`); add more with your
  system's `tesseract-data-*` or `tesseract-ocr-*` packages.
- **The container:** English, German, French, Portuguese, Spanish and Simplified Chinese, the
  languages the image ships. A Danish or Japanese scan is read as English there: install
  OCRmyPDF with that language for better words.

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *How previews are made* | `[preview] prefer` (`"auto"`, `"local"`, `"container"`) | `"auto"`: installed first, else the container |
| – | `[preview.prefer_tool] ocrmypdf` | `[preview] prefer` |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Container images* (Pull, Remove) | `[preview.images] ocrmypdf` | `"docker.io/jbarlow83/ocrmypdf-alpine:latest"`; `""` never uses a container. An image you set must have `ocrmypdf` as its entry point |
| *Keep images up to date*, *Remove images not used for 90 days* | `[preview] image_updates`, `image_prune` | Off; a run counts as a use |
| *Keys* | `[keys] searchable_pdf` | `"Alt+P"` |

There is no time limit: a long book takes minutes, and **Stop** is there for that. OCRmyPDF
works in a folder of its own in the cache (`~/.cache/coxswain/ocr/`), removed when it ends.

## In the terminal app

The same action, key and question, as a dialog; the status line shows how far it is, and
**Alt+P** again stops it (the status line says so). It has no preview pane, so there is no
button or scan line there: the hint on a PDF and the action menu are where it is found. The
container runs the same way as in the desktop app, so a terminal app over ssh can make a scan on
a server searchable.

## Questions

#### Is my PDF changed?

No. The copy is `name.searchable.pdf` next to it (`name.searchable 2.pdf` when that is taken).
Nothing is ever overwritten, and a stopped or failed run leaves no copy: the name is taken only
when the finished copy is written. Quitting while it runs stops it first (the terminal app waits
for it; a second **F10** leaves at once), so its container and work folder go too.

#### Does Coxswain read scans in the background?

Only with tesseract installed, and only for Find ([Scans, pictures and older Office
files](../search/scans.md)). **Alt+P** is what makes a copy you can search, select and copy
from in any program; Find reads that copy like any PDF, all its pages. Nothing runs OCRmyPDF
by itself.

#### What does it need?

OCRmyPDF installed, or podman or docker: its container image (about 310 MB to download, 850 MB on
disk) is pulled on the first run, once the question before it has said so. On FreeBSD, the
other BSDs and illumos only an installed OCRmyPDF works.

#### Why is the button there on a PDF that has text?

So it can be found on every PDF. On one with text on every page, the copy is the same document
again: OCRmyPDF leaves pages with text as they are. The line above the PDF says *it looks like a
scan* only when no text was found.

#### The words come out wrong.

OCR guesses, and it guesses best in the languages it has ([Languages](#languages)). A clean
scan at 300 dpi reads far better than a photo of a page. Look at the copy in your PDF reader:
select the text to see what it read.

#### Can I keep the copy only and drop the original?

Yes, it is an ordinary file: delete the original with **F8** and rename the copy with
**Shift+F6**. Coxswain never does that for you.

#### Does anything leave my machine?

Only the image, downloaded from Docker Hub when you allow the first run; *Settings → Privacy
and updates → What can leave this machine* names it. The container has no network, and an
installed OCRmyPDF reads and writes only here.

---
[← Previous: ZFS snapshots as folders](zfs-snapshots.md) · [Next: Customising →](../customise/README.md)

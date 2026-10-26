[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Disk use

Every place Coxswain keeps things on the disk, in one list: what each is, its size, where it is,
what clearing it costs, and a button that clears it. What Coxswain caused outside its own folder
(Tectonic's cache, container images it pulled for previews) is listed too, and cleared only when
you ask for it by name, and your own files Coxswain keeps aside (the quarantine, find and
replace's undo copies, what a folder sync set aside) last, each removed only on its own click.
[Where things are kept](where-things-are-kept.md) has every path on each
system; this page is about the room they take and getting it back.

![Settings at Privacy and updates with Disk use open, 62.0 MB in all: search.db 1.7 MB with "read again on its own, at half speed", index.bin, previews/, peek/ and libreoffice-profile/, each with its size, what clearing costs, Show in panel and Clear](../screenshots/settings-disk-use.png)
*Settings → Privacy and updates → Disk use in the desktop app.*

## Contents

- [How to reach it](#how-to-reach-it)
- [The rows](#the-rows)
- [Clearing](#clearing)
- [Outside Coxswain's folder](#outside-coxswains-folder)
- [In the terminal app](#in-the-terminal-app)
- [On the command line](#on-the-command-line)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to reach it

| Where | Desktop app | Terminal app |
|---|---|---|
| The command list | **F9** → *Disk use* | **F9** → *Disk use* |
| Settings | **Ctrl+,** or **F12** → *Privacy and updates* → *Disk use* (click the heading to open it; it shows the total beside it) | **F12** → *Privacy and updates*, rows under *Disk use* |
| Starting at it | `coxswain-gui --settings=disk` | `coxswain --settings=disk` |
| From the file itself | The preview of `index.bin` or `search.db` → **Settings at Disk use** ([Data previews](../previews/data.md#coxswains-own-index-and-store)) | **F3** on either file names the way there |
| Command line | | `coxswain --disk`, `coxswain --disk clear …` |

*Disk use* has no key of its own; it is an entry of the command list (**F9**) and F1 → Features.

## The rows

| Row | What it holds | Clearing it costs | In *Clear everything* |
|---|---|---|---|
| `search.db` (with `-wal`, `-shm`) | The search store: the text of your files, every file's size and date, folder totals, duplicate hashes, meaning vectors, and the transcripts of speech, which stay when it is cleared | It is read again on its own, at half speed; until then Find finds fewer words in files, and search by meaning has to make its vectors again | Yes |
| *search.db: changes of commits* | The text of what each commit changed, inside `search.db`, when [Text of each commit's changes](../search/history.md#the-text-of-the-changes) is on; shown only then | The option is turned off: the text goes when the search helper next starts, `search.db` gives the room back, and Settings says how much was freed. Commit messages stay | No |
| `index.bin` | The name index, saved so Find has every name at once when the helper starts | Built again on its own: Find by name waits a few minutes on a large disk | Yes |
| `previews/` | PDFs, SVGs and pages made by LaTeX, LibreOffice, PlantUML and pandoc, and the EPUBs and covers calibre made of e-books; those not shown for 30 days go by themselves, and past 4 GB the ones shown longest ago | Made again the next time each file is shown | Yes |
| `peek/` | Files copied out of archives to be previewed or viewed, and archives inside archives you opened (`nested/`), and what compressed tars hold and the places they can be unpacked from (`archive-index/`) | Copied again the next time | Yes |
| `scratch/` (with `media/`, `geo/`, `ocr/`, `export/`, `clamscan/`, `drag/`, `provenance/`) | The work of conversions, searchable PDFs, virus scans, exports and drags, one folder per run | Nothing: only runs that ended are cleared, a run working now keeps its folder | Yes |
| `chess/` (with `cbv/`) | Chess analyses kept (the newest 1000), and ChessBase archives unpacked to be read (up to 16 GB each) | A game is analysed again, an archive unpacked again when opened; an archive a running app has open stays | Yes |
| `inside-…/` | Left-overs of the search helper reading files inside archives; shown only when there are some | Nothing. A folder a running helper is using now is kept | Yes |
| `libreoffice-profile/` (and `libreoffice-index-profile/`) | LibreOffice's own profiles for previews and for reading old Office files | Made again the next time LibreOffice runs | Yes |
| `clamav/` | ClamAV's virus database, for [Scan for viruses](../files/virus-scan.md); shown when there is one | Fetched again from database.clamav.net before the next virus scan | Yes |
| `helper.log` | When the search helper started and left, and what failed; the whole text of an error the status line said short | The record is gone. It is emptied, not deleted, since the helper writes to it | No |
| A built-in model (*Qwen3 4B*, *multilingual-e5-small*, *Whisper*, a left-over) | Its weights ([Built-in models](../search/models.md)) | Downloaded again when needed; the one in use gives way to the recommended choice (*Whisper* in use: transcribing is turned off) | Only when ticked, and never the one in use |
| *Tectonic* | Tectonic's cache of TeX packages, made by a LaTeX preview | Downloaded again the next time tectonic builds a document, yours too | Only when ticked |
| *Typst* | Typst's cache of packages, made by a Typst preview | Downloaded again the next time typst builds a document that uses a package, yours too | Only when ticked |
| *KiCad* | kicad-cli's cache, made by a KiCad preview or export | Made again the next time kicad-cli runs, yours too | Only when ticked |
| A container image | The image of `[preview] images` for LaTeX, Typst, LibreOffice, PlantUML, Kroki's diagrams, pandoc, OCRmyPDF, ffmpeg, ImageMagick, GDAL, OpenSCAD, KiCad, calibre, Asciidoctor, draw.io …; its line says what it gives, its size to download and on disk, and what it costs ([The images](../previews/containers.md#the-images)) | Pulled again the next time a preview needs it | Only when ticked |
| `quarantine/` (under *Your own files*) | Files you moved into quarantine after a [virus scan](../files/virus-scan.md#the-quarantine-folder), with `quarantine.log`; shown when it holds any | Not removed here: it has **Show in panel** only. They are yours: delete them in a panel once you are sure | No |
| `kicad/` (under *Your own files*) | kicad-cli's settings (`~/.config/kicad` on the BSDs and Linux, `~/Library/Preferences/kicad` on macOS, `%APPDATA%\kicad` on Windows); shown once a KiCad preview or export has run an installed kicad-cli | Not removed here: it has **Show in panel** only. An installed KiCad keeps your own settings there, so Coxswain never clears it | No |
| `coxswain-replace-…/` (under *Your own files*) | Your files as they were before a [find and replace](../files/find-and-replace.md), so it can be undone (256 MB at most per replace); shown when there are some | **Remove** takes only those of apps that have ended (their finds and replaces can no longer be undone); a running app keeps its own | No |
| `.coxswain-sync-…/` (under *Your own files*) | What a [folder sync](../files/compare-folders.md#where-did-my-replaced-files-go) that has ended replaced or deleted, where the bin could not take it (Windows, a disk without a bin); one row per folder, its line naming the synced folder | **Remove** deletes it for good | No |

![Disk use in the desktop app, Cyber, with the row search.db: changes of commits, its size, what clearing it costs and Clear, under it what it holds](../screenshots/disk-use-changes-gui.png)

Each row shows its name, its size, what clearing costs, **Show in panel** (the active pane opens
its folder, the cursor on the file; hover it for the path) and **Clear**. The small line under it
says what it holds. The total of all rows is beside the heading *Disk use*.

Your own files come last, under *Your own files, kept aside by Coxswain*: the quarantine,
KiCad's settings, the undo copies of find and replace, and what a folder sync set aside. They are
never in *Clear everything*; each goes only by its own **Remove** (the quarantine and KiCad's
settings have none), whose first click
turns into *Remove /path for good? Click again*.

Sizes are measured when the area opens: the folders on disk at once, the container images a
moment later (podman or docker is asked about each image by its exact name).

## Clearing

- **Clear** on a row: the button turns into *Clear /home/me/.cache/coxswain/previews? Click
  again*; the second click clears it and the line under the list says *previews/ cleared: 12.4 MB
  freed.* Moving away (the button loses the focus) cancels it.
- **Clear everything that can be built again (…)** clears the rows marked *Yes* above. Tick *Also
  the built-in models not in use* or *Also what Coxswain caused outside its folder* to take those
  too; the size on the button follows. The first click lists exactly what goes (*This goes:
  /home/me/.cache/coxswain/search.db, …*) and turns the button into *Clear these? Click again*.

![Disk use scrolled down: Typst 4.1 MB under Outside Coxswain's folder, the group Your own files, kept aside by Coxswain with quarantine/ 342 KB and only Show in panel, the box Also what Coxswain caused outside its folder (5.7 MB) ticked, the line This goes: with each path (scratch and chess among them, tectonic and typst last), and the button Clear these? Click again](../screenshots/settings-disk-clear-all.png)

How each is cleared:

- **The search store while the search helper runs** is emptied by the helper itself (the same as
  *Delete what was read* under Finding files → Details → Background reading): the store in use is
  never deleted from under it, and its write-ahead log is emptied too, so the room is free at
  once. With no helper running, the files are deleted.
- **The name index** is deleted, then a running helper is asked to make way for a new one, which
  builds the index again and saves it.
- **A folder of runs' folders** (`peek/`, `nested/`, `scratch/`, `media/` …, `cbv/`): only the
  folders of runs that ended go. Each running app and search helper holds its own list in
  `staging/`, so a copy it shows now or a conversion it runs now is never taken from under it.
- **A built-in model in use** gives way first: Ask or search by meaning takes the recommended
  choice, saved, and the line says which (as under [Built-in models](../search/models.md)).
- **A container image** is removed with `podman image rm <name>` (or docker's). No container is
  ever started. An image a container still uses is refused by podman, and its words are shown.
  What it says was freed is only what really went: the layers no other image uses, measured
  with `podman system df` (or docker's) before and after. Its size in the list counts the shared layers too.

## Outside Coxswain's folder

Coxswain lists only what it caused, matched exactly:

- **Tectonic's cache** (`~/.cache/tectonic` on the BSDs and Linux, `~/.cache/Tectonic` with older tectonic, `~/Library/Caches/Tectonic`
  on macOS, `%LOCALAPPDATA%\TectonicProject\Tectonic` on Windows, or `TECTONIC_CACHE_DIR`), and
  only when Coxswain made it: when a LaTeX preview runs tectonic and its cache folder was not
  there before, Coxswain notes the folder in `made-outside.txt` in its cache folder. A Tectonic
  cache you had before is never listed.
- **Typst's package cache** (`typst/` in the system's cache folder: `~/.cache/typst` on the BSDs
  and Linux, `~/Library/Caches/typst` on macOS, `%LOCALAPPDATA%\typst` on Windows, or
  `TYPST_PACKAGE_CACHE_PATH`) and **kicad-cli's cache** (`kicad/` in the same folder), the same
  way: only when an installed typst or kicad-cli run by a preview or export made it. kicad-cli's
  settings (`~/.config/kicad`) are listed under *Your own files* once a preview has run it,
  whoever made them, with **Show in panel** and no Clear: an installed KiCad keeps your own
  settings there.
- **Container images** named in `[preview] images` (by default
  `docker.io/texlive/texlive:latest`, `docker.io/plantuml/plantuml:latest`,
  `docker.io/pandoc/core:latest`, `docker.io/gotenberg/gotenberg:8-libreoffice`,
  `docker.io/yuzutech/kroki:latest`, `ghcr.io/typst/typst:latest`,
  `docker.io/jbarlow83/ocrmypdf-alpine:latest`, listed once though OCRmyPDF and Ghostscript
  share it, `docker.io/mwader/static-ffmpeg:latest`, `ghcr.io/mwo-dk/coxswain-imagemagick:latest`, `ghcr.io/osgeo/gdal:alpine-small-3.13.3`, `docker.io/openscad/openscad:trixie.2026-10-19`, `docker.io/kicad/kicad:9.0.9`, `docker.io/apache/tika:4.1.0` calibre's `docker.io/linuxserver/calibre:9.16.0`, pinned by its digest, and `ghcr.io/ggml-org/whisper.cpp:main`) that podman or docker has, and a former default no
  tool uses any more, `docker.io/dstmodders/imagemagick:latest` (ImageMagick's before Coxswain
  made its own), its line saying *A former default image for imagemagick previews, which
  Coxswain no longer uses*. It goes only when you clear it here, or by itself with *Keep images
  up to date* on. Other images, alike, are not listed. *Remove images not used for 90 days* in *Settings → Previews* removes
  these by itself when no preview used one (nor Apache Tika nor Whisper used it) for that long, and *Keep images up to date* removes
  the copy an update replaced ([Containers](../previews/containers.md#keeping-images-up-to-date)).

Nothing outside is cleared by *Clear everything* unless its box is ticked, and every clear shows
the exact path or image name before the second click.

## In the terminal app

**F9** → *Disk use* opens Settings at *Privacy and updates* with the cursor on the first row of
*Disk use*: each row shows its name, size and what clearing costs; the bottom says what it holds,
what clearing costs and its path, and the key line *Enter: show in panel · Delete: clear*.

![The terminal app's Settings at Privacy and updates: Disk use with search.db, index.bin (under the cursor), previews/, peek/, libreoffice-profile/, helper.log, clamav/, scratch/ 45.8 MB and chess/ 2.0 MB, the rows Clear everything that can be built again (55.8 MB) and Also what Coxswain caused outside its folder, then Outside Coxswain's folder with Tectonic and Typst, and Your own files, kept aside by Coxswain with quarantine/](../screenshots/tui-settings-disk.png)

| Key | Does |
|---|---|
| **↑** **↓** | Moves between the rows |
| **Enter** | On a row: Settings closes and the active panel opens its folder, the cursor on the file |
| **Delete** | On a row: the bottom turns red, *This goes: /path (size) · what it costs. Enter clears it, Esc keeps it*; on a row under *Your own files* it removes (the key line says *Delete: remove*), and on `quarantine/` it does nothing (*Enter: show in panel*) |
| **Enter** (asked) | Clears it; the key line says *previews/ cleared: 12.4 MB freed.* |
| **Esc** (asked) | Keeps it |
| **Space** or **Enter** | On *Also the built-in models not in use* or *Also what Coxswain caused outside its folder*: ticks it |
| **Enter** | On *[ Clear everything that can be built again ]*: lists what goes, red, and asks |

The container images come a moment after the area opens: the terminal app asks podman or docker
on a thread of its own.

## On the command line

`coxswain --disk` prints every row: its name to clear it by, its size, its file name, its path,
what it holds and what clearing it costs, under *In Coxswain's cache folder*, *Built-in models*
and *Outside Coxswain's folder, caused by it*; then the total that is built again on its own.

```
$ coxswain --disk
In Coxswain's cache folder
  store               284 KB  search.db
                              /home/me/.cache/coxswain/search.db
                              The search store: the text of your files, …
                              Clearing it: read again on its own, at half speed; …
  index               16.4 KB  index.bin
…
Built again on their own: 12.9 MB, coxswain --disk clear rebuildable
```

| Command | Does |
|---|---|
| `coxswain --disk clear store` | Empties the search store, through the helper when one runs: *search.db cleared: 284 KB freed.* |
| `coxswain --disk clear index` (`previews`, `peek`, `inside`, `libreoffice`, `log`, `clamav`, `work`, `chess`) | Clears that row |
| `coxswain --disk clear qwen3-4b-a06e946b` | A model, by the name in the first column; the same as `coxswain --models delete`, which asks *[y/N]* for the one in use |
| `coxswain --disk clear tectonic`, `coxswain --disk clear images`, `coxswain --disk clear docker.io/pandoc/core:latest` | Prints *This goes:* with the exact path or image and its size, then asks *[y/N]* |
| `coxswain --disk clear replace`, `coxswain --disk clear sync:.coxswain-sync-…` | Your own files: prints *This goes:* with the exact path and its size, then asks *[y/N]*. `quarantine` and `kicad-settings` are refused: delete those files yourself |
| `coxswain --disk clear rebuildable` | Every row that is built again on its own: *Cleared: 12.9 MB freed.* |

A value it does not know is refused with the names it takes and exit status 2, like every other
option ([Command-line flags](command-line-flags.md)); a clear that fails exits with 1.

## Settings and config.toml

Disk use has no options. The images it lists are `[preview] images`; which runtime it asks is
`[preview] container` (`auto`, `podman`, `docker`, or `off` for none). The folders follow the
system's ([Where things are kept](where-things-are-kept.md#on-each-system)).

## Questions

#### What can I clear without losing anything?

Everything in the list but your models, what is outside and your own files comes back by itself. **Clear
everything that can be built again** takes the search store, the name index, previews, archive
copies, LibreOffice profiles and ClamAV's virus database. The cost is time: the search store is read again at half speed
(**Read now** in Settings → *Finding files* reads at full speed), and Find by name waits for the
index.

#### Will it delete container images or a Tectonic cache that are mine?

No. Only the images named in `[preview] images` are listed, by their exact names, and a Tectonic
cache only when Coxswain's own LaTeX preview made it (it is noted in `made-outside.txt`). Each
goes only on its own click (or `[y/N]` on the command line), after the exact path or name is
shown.

#### I cleared search.db and it is large again a minute later. Why?

The helper reads your files again at once. The size grows back to what your files need; clearing
it frees room only for as long as the reading takes. To keep it small for good, read fewer
folders (Settings → *Finding files* → *Details* → *Folders*) or turn off search inside files.

#### Is it safe to clear search.db while Coxswain runs?

Yes. While the search helper runs, it empties the store itself, so nothing is cut from under it;
the desktop app's duplicate finder waits for it. With no helper, the file is deleted.

#### Why is Tectonic listed but my Docker images are not?

Only images named in `[preview] images` are Coxswain's. Tectonic's cache is listed when a LaTeX
preview made it; one that was there before Coxswain first ran tectonic is yours and not shown.
Typst's and kicad-cli's caches are listed the same way.

#### Why are the quarantine and a sync's folder listed, when nothing here is my work?

Everything else in the list is Coxswain's and comes back by itself. The rows under *Your own
files, kept aside by Coxswain* are yours: files you put in quarantine, KiCad's settings (which a
preview's kicad-cli uses, and an installed KiCad shares), your files as they were
before a find and replace, and what a folder sync replaced or deleted where the bin could not
take it. They are shown so the room they take is not hidden, and they go only when you remove
each one; *Clear everything* never takes them.

#### Why does helper.log not go with Clear everything?

It is not built again: it is the record of what the helper did, which you may want when
something failed. Clear it on its own row; it is emptied, not deleted, since the helper writes to
it.

#### The container images do not show.

Podman or docker is asked only when it is there and `[preview] container` is not `off`, and only
about the images in `[preview] images`. An image that was never pulled has no size and no row.

#### What does the terminal app show that the desktop app does not, or the other way?

The same rows and the same clearing in both. The desktop app has **Show in panel** and the two
clicks; the terminal app has **Enter** and **Delete**, and `coxswain --disk` on the command line.

#### How do I get there?

In both apps **F9** → *Disk use*, or Settings (**Ctrl+,** or **F12**) → *Privacy and updates* →
*Disk use*; in the terminal app also `coxswain --disk` on the command line ([How to reach it](#how-to-reach-it)).

---
[← Previous: Where things are kept](where-things-are-kept.md) · [Next: Security →](security.md)

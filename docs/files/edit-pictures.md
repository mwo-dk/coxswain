[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Convert or edit pictures

*Convert or edit pictures* makes a new copy of a photo or a picture, next to the original: as
JPEG, PNG or WebP, smaller, without its location and the camera's other notes, or turned.
[ImageMagick](https://imagemagick.org/) does the work, in its container first, else as
installed. The original is never changed and nothing is ever written over. Both apps.

![The desktop app in the Cyber theme with Pictures/Holiday as thumbnails, three photos marked; the menu Convert or edit pictures is open with To JPEG, To PNG, To WebP, Smaller…, Without its location and camera data, Turned 90° clockwise, Turned 270° clockwise and Turned 180° clockwise, numbered 1 to 8](../screenshots/edit-pictures.png)
*Three photos marked, and what Convert or edit pictures can make of them.*

## Contents

- [How to use it](#how-to-use-it)
- [What it makes](#what-it-makes)
- [What it needs](#what-it-needs)
- [How a picture is kept from doing harm](#how-a-picture-is-kept-from-doing-harm)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a picture, or mark several (among other files, only the pictures are
   done): JPEG, PNG, WebP, GIF, BMP, TIFF, AVIF, JPEG XL, Photoshop, TGA, HEIC, GIMP (`.xcf`)
   and the other kinds only ImageMagick reads, and cameras' RAW photos.
2. Pick *Convert or edit pictures* in the action menu (**Shift+F10**) or in **F9**. In the
   desktop app the preview pane of every picture has a **Convert or edit pictures** button too.
3. A menu lists what it can make, each with a number: **1** *To JPEG*, **2** *To PNG*, **3**
   *To WebP*, **4** *Smaller…*, **5** *Without its location and camera data*, **6** *Turned 90°
   clockwise*, **7** *Turned 270° clockwise*, **8** *Turned 180° clockwise*.
4. *Smaller…* asks for the longest side in pixels first (`1600` by default). A picture already
   that small is copied at its size: it is never made larger.
5. A question says what it writes (*Writes IMG_0042.resized.jpg next to IMG_0042.jpg.
   IMG_0042.jpg itself is not changed.*), which program runs, and, when the container image is
   not there yet, *First run pulls docker.io/dstmodders/imagemagick:latest (about 60 MB)*.
   **Enter** (or **Convert**) starts it; **Esc** leaves everything as it is.
6. The status line says which picture it is at: *Converting IMG_0043.jpg (2 of 3), 1 s*.
   **Stop** (desktop app), or the action again from **F9** (both apps), stops it.
7. When it is done: *Made 3 new files next to their originals*.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Shift+F10**, **F9** | *Convert or edit pictures* in the action menu (on a picture, or with one marked) and in the command list; while one runs, it stops it | The same |
| **1** … **8**, **Enter** | Picks what to make in its menu | The same |
| **Space** / **F3** | The preview: the **Convert or edit pictures** button under every picture | – |

The action has no key of its own; give it one in [Changing keys](../customise/keys.md)
(`[keys] edit_pictures`).

## What it makes

| Choice | The new file | How |
|---|---|---|
| *To JPEG* | `photo.jpg` | Quality 92 |
| *To PNG* | `photo.png` | Lossless |
| *To WebP* | `photo.webp` | Quality 90 |
| *Smaller…* | `photo.resized.jpg` | No side longer than the pixels you typed, the proportions kept |
| *Without its location and camera data* | `photo.no-location.jpg` | Every note in the file goes (the place, the camera, the date taken, the thumbnail), except its colour profile, so the colours stay right. The picture is turned upright first, since the note that said which way up goes too |
| *Turned …* | `photo.rotated.jpg` | Turned clockwise by 90, 180 or 270 degrees, after it is turned upright as the camera noted |

Every picture is turned upright first (the camera's orientation note), so the result looks as
the photo does in the preview. A file of several pages or frames (a TIFF, an animated GIF) gives
its first one; a GIMP file its layers merged. A JPEG, PNG, WebP, GIF, BMP or TIFF keeps its
kind when made smaller, turned or stripped of its location; a HEIC or RAW photo comes out as
JPEG, the other kinds as PNG. Each name is the first one free: when `photo.png` is there already,
the next is `photo 2.png`. A JPEG saved again loses a little quality, as any JPEG does.

## What it needs

One of these, in this order unless *Settings → Previews → How previews are made*
(`[preview] prefer`), or the *Pictures* row of [*Preview tools*](../previews/tools.md#preview-tools-in-settings) (`[preview.tools] pictures`) for this kind alone, says otherwise:

| Way | What runs | Systems |
|---|---|---|
| Its container, first | podman or docker runs `docker.io/dstmodders/imagemagick:latest` (ImageMagick 7 on Alpine, rebuilt with every ImageMagick release) with no network, seeing only that one file, read-only | Linux, macOS and Windows with podman or docker. Not on FreeBSD, the other BSDs and illumos: the image is a Linux one, so install ImageMagick there instead |
| ImageMagick installed | `magick`, at the lowest priority, under Coxswain's own [policy](#how-a-picture-is-kept-from-doing-harm) | FreeBSD: `pkg install ImageMagick7`; NetBSD: `pkgin install ImageMagick`; OpenBSD: `pkg_add ImageMagick`; Linux: `sudo pacman -S imagemagick`, `sudo apt install imagemagick`, `sudo dnf install ImageMagick`, `sudo zypper install ImageMagick`; macOS: `brew install imagemagick`; Windows: `winget install --id ImageMagick.ImageMagick -e`. It must be ImageMagick 7, which has `magick` |

The image is about 60 MB to download and 152 MB on disk. It is pulled only after the question
said so and you pressed **Enter**; never by itself. It reads every kind above except HEIC and
RAW photos, for which its build has no reader: those go to an installed ImageMagick with libheif
and LibRaw (Arch, Fedora, Debian 13, Homebrew and FreeBSD's ship it so) when there is one, while
the container does the rest. Without one, a HEIC or RAW photo says *This ImageMagick cannot read
.heic files…* and the others are done all the same.

With neither, the action says what is missing instead of asking: *Converting pictures needs
ImageMagick. Install it (sudo apt install imagemagick), or run it in a container: …*. The
desktop app's dialog has a **Settings → Previews** button; the terminal app's says *Settings:
F12 → Previews*.

## How a picture is kept from doing harm

A picture file can be made to attack the program that reads it, and ImageMagick reads many
formats, some of which can name other files or run helper programs. So:

- **In the container** it has no network, sees only the one file (mounted read-only as
  `/src/in.jpg`) and a fresh folder to write into, runs on a read-only root with no
  capabilities, at most 3 GB of memory and 256 processes, and under the policy below as well.
- **Installed** it runs under a policy of Coxswain's own (`~/.cache/coxswain/magick/policy.xml`,
  through `MAGICK_CONFIGURE_PATH`): no helper programs (delegates), no format but the picture
  kinds above (no SVG, MSL, text or URL readers), no `@file` arguments, and limits of 1 GiB of
  memory, 256 megapixels and five minutes per picture.
- **Names are never arguments:** the program reads a link to the file (a copy where no link can
  be made) named `in.<ending>`, so a file called `-rf.jpg` or `msl:x.png` is just a picture.
- **The result** is written in a folder of its own in the cache and put next to the original
  under a name no file has (`create_new`), so nothing is ever written over.

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| The preview of a picture | The **Convert or edit pictures** button under it, with the photo's facts | – |
| The status line | *Shift+F10 / Menu on a video, a song or a photo: convert it, cut a clip, make it smaller or turn it*, a few times | The same hint |
| While it runs | *Converting IMG_0043.jpg (2 of 3), 1 s* and a **Stop** button at the end of the command line | *Converting IMG_0043.jpg (2 of 3), 1 s · F9 → Convert or edit pictures stops* |
| First run of the container | *Pulling docker.io/dstmodders/imagemagick:latest…* with the runtime's progress line | The same |
| Done | *Made photo.no-location.jpg* (several: *Made 3 new files next to their originals*) | The same |
| Stopped | *Stopped: the file it was working on got no result*; the ones done before stay | The same |
| Failed | A dialog *Could not convert the file* with ImageMagick's last lines, per file | The same, full screen |

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *How previews are made* | `[preview] prefer` (`"auto"`, `"local"`, `"container"`) | `"auto"`: for ImageMagick, the container first, else installed |
| *Preview tools* → *Pictures* | `[preview.tools] pictures` (`"local"`, `"container"`, `"off"`) | `[preview] prefer`; *Off* turns off only its previews, not this |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Preview tools* → *Pictures* (Pull, Remove) | `[preview.images] imagemagick` | `"docker.io/dstmodders/imagemagick:latest"`; `""` never uses a container. An image you set must have `magick` on its `PATH` |
| *Keep images up to date*, *Remove images not used for 90 days* | `[preview] image_updates`, `image_prune` | Off; a run counts as a use |
| *Keys* | `[keys] edit_pictures` | none |

ImageMagick works in a folder of its own in the cache (`~/.cache/coxswain/media/`), removed when
each run ends; the result is then linked (or, on another disk, copied) next to the original.

## In the terminal app

The same action, menu and question, as dialogs; the status line shows how far it is, and the
action again (from **F9**, or from the action menu on a picture) stops it. The container runs
the same way as in the desktop app, so a terminal app over ssh can make a server's photos smaller.

## Questions

#### Is my photo changed?

No. Each result is a new file next to it (`photo.resized.jpg`, `photo.no-location.jpg`, with
` 2` added when the name is taken). Nothing is ever overwritten, and a stopped or failed run
leaves nothing behind.

#### Is it safe with a picture someone sent me?

ImageMagick runs in a container with no network that sees only that one file, read-only. An
installed ImageMagick runs under a policy that allows picture formats only and no helper
programs ([How a picture is kept from doing harm](#how-a-picture-is-kept-from-doing-harm)).

#### Why can it not read my HEIC or RAW photo?

The container image reads neither: its ImageMagick is built without libheif and LibRaw. Install
ImageMagick with them, as most systems ship it: HEIC and RAW photos then go to it by themselves,
the rest to the container. The [preview](../previews/media.md#photos-only-imagemagick-or-ffmpeg-reads)
shows HEIC photos with ffmpeg all the same.

#### Does *Without its location* keep the date and the camera?

No: it removes every note the file holds, as the location can hide in more than one of them.
Only the colour profile stays. The photo itself is the same, turned upright.

#### Why is the WebP or PNG larger than the JPEG?

PNG keeps every pixel exactly, which takes room for a photo; WebP at quality 90 is usually a
little smaller than the JPEG. For a smaller photo, use *Smaller…*.

#### Can I turn a photo without saving it again?

Not here: a turned JPEG is saved again, which loses a little quality. A viewer that only changes
the orientation note does not.

#### Does anything leave my machine?

Only the image, downloaded from Docker Hub when you allow the first run; *Settings → Privacy and
updates → What can leave this machine* names it. The container has no network, and an installed
ImageMagick reads and writes only here.

---
[← Previous: Convert video or audio](convert-media.md) · [Next: Checksums (Alt+H) →](checksums.md)

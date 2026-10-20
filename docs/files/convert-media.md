[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Convert video or audio

*Convert video or audio* makes a copy of a video or a song in another form, next to the
original: an MP4 that plays everywhere, an MP3 or a FLAC of its sound, its sound track as it is,
or a clip of it. [ffmpeg](https://ffmpeg.org/) does the work, as installed or from its container
image. The original is never changed and nothing is ever written over. Both apps.

![The desktop app in the Cyber theme with launch.avi under the cursor in Videos: the preview shows a frame of the video under the line "A frame of the video: the app's player cannot play this kind…", its facts Format AVI, Length 0:06, Size in pixels 1280 × 720, Video mpeg4, 25 fps, Sound mp2, mono, and the menu Convert video or audio is open with To MP4 (H.264 and AAC), To MP3, To FLAC, Its sound only, as it is and A clip…, numbered 1 to 5](../screenshots/convert-media.png)
*An AVI the player cannot play: ffmpeg's frame, its facts, and what Convert video or audio makes of it.*

## Contents

- [How to use it](#how-to-use-it)
- [What it makes](#what-it-makes)
- [What it needs](#what-it-needs)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a video or a song, or mark several (among other files, only the videos and
   songs are done).
2. Pick *Convert video or audio* in the action menu (**Shift+F10**) or in **F9**. In the desktop
   app the preview pane of every video and song has a **Convert video or audio** button too.
3. A menu lists what it can make, each with a number: **1** *To MP4 (H.264 and AAC)*, **2** *To
   MP3*, **3** *To FLAC*, **4** *Its sound only, as it is*, **5** *A clip…*. Press the number, or
   move to it and press **Enter**.
4. *A clip…* asks for the start and the length first: `1:30 0:45` takes 45 seconds from 1
   minute 30 (seconds, `m:ss` and `h:mm:ss` all work).
5. A question says what it writes (*Writes holiday.mp4 next to holiday.avi. holiday.avi itself
   is not changed.*), which program runs, and, when the container image is not there yet, *First
   run pulls docker.io/mwader/static-ffmpeg:latest (about 126 MB)*. **Enter** (or **Convert**)
   starts it; **Esc** leaves everything as it is.
6. The status line says how far it is: *Converting holiday.avi (2 of 5), 14 s · 63 %*.
   **Stop** (desktop app), or the action again from **F9** (both apps), stops it.
7. When it is done: *Made holiday.mp4*, and the panel shows the new file next to the old one.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Shift+F10**, **F9** | *Convert video or audio* in the action menu (on a video or a song, or with one marked) and in the command list; while one runs, it stops it | The same |
| **1** … **5**, **Enter** | Picks what to make in its menu | The same |
| **Space** / **F3** | The preview: the **Convert video or audio** button under every video and song | – |

The action has no key of its own; give it one in [Changing keys](../customise/keys.md)
(`[keys] convert_media`).

## What it makes

| Choice | The new file | How |
|---|---|---|
| *To MP4 (H.264 and AAC)* | `holiday.mp4` | The picture as H.264, every sound track as AAC at 160 kbit/s, for any player and the web; subtitles are left out. A video that is an MP4 already gets `holiday 2.mp4` |
| *To MP3* | `song.mp3` | The first sound track, as MP3 at about 190 kbit/s (`-q:a 2`) |
| *To FLAC* | `song.flac` | The first sound track, lossless |
| *Its sound only, as it is* | `holiday.audio.m4a` | The first sound track copied without encoding it again, in the file kind its codec belongs in: `.m4a` for AAC, `.mp3`, `.opus`, `.ogg` for Vorbis, `.flac`, `.ac3`, else `.mka` |
| *A clip…* | `holiday.clip.avi` | The stretch you typed, every track copied as it is, in the same kind of file |

Each name is the first one free: when `song.mp3` is there already, the next is `song 2.mp3`.
The name is taken only when the finished file is put there, so a stopped or failed run leaves
nothing behind. A song without a picture gets *To MP4* as an MP4 of its sound.

## What it needs

One of these, tried in the order of *Settings → Previews → How previews are made*
(`[preview] prefer`, or `prefer_tool.ffmpeg` for ffmpeg alone):

| Way | What runs | Systems |
|---|---|---|
| ffmpeg installed | `ffmpeg … -i file:<the file> …`, at the lowest priority, reading real media formats only (`-format_whitelist`, `-protocol_whitelist file`) | FreeBSD: `pkg install ffmpeg`; NetBSD: `pkgin install ffmpeg7`; OpenBSD: `pkg_add ffmpeg`; OmniOS: `pkg install ooce/multimedia/ffmpeg`; Linux: `sudo pacman -S ffmpeg`, `sudo apt install ffmpeg`, `sudo dnf install ffmpeg-free`, `sudo zypper install ffmpeg`; macOS: `brew install ffmpeg`; Windows: `winget install --id Gyan.FFmpeg -e` |
| Its container | podman or docker runs `docker.io/mwader/static-ffmpeg:latest` with no network, seeing only that one file, read-only ([What a container may do](../previews/containers.md#what-a-container-may-do)) | Linux, macOS and Windows with podman or docker. Not on FreeBSD, the other BSDs and illumos: the image is a Linux one, so install ffmpeg there instead |

The image is about 126 MB to download and 315 MB on disk. It is pulled only after the question
said so and you pressed **Enter**; never by itself. The same image makes the
[previews](../previews/media.md#video-and-sound-with-ffmpeg) of videos and sound the app's
player cannot play.

With neither, the action says what is missing instead of asking: *Converting video and sound
needs ffmpeg. Install it (sudo apt install ffmpeg), or run it in a container: Neither podman nor
docker is installed*. The desktop app's dialog has a **Settings → Previews** button; the
terminal app's says *Settings: F12 → Previews*.

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| The preview of a video or a song | The **Convert video or audio** button under it, and the facts: *Format*, *Length*, *Size in pixels*, *Video* (codec and frames a second), *Sound* (each track's codec, channels and language), *Subtitles*, *Bitrate* | **Alt+Enter** (Properties) lists the same facts |
| The status line | *Shift+F10 / Menu on a video, a song or a photo: convert it, cut a clip, make it smaller or turn it*, a few times | The same hint |
| While it runs | *Converting holiday.avi, 14 s · 63 %* and a **Stop** button at the end of the command line | *Converting holiday.avi, 14 s · 63 % · F9 → Convert video or audio stops* |
| First run of the container | *Pulling docker.io/mwader/static-ffmpeg:latest…* with the runtime's progress line | The same |
| Done | *Made holiday.mp4* (several: *Made 5 new files next to their originals*) | The same |
| Stopped | *Stopped: the file it was working on got no result*; the ones done before stay | The same |
| Failed | A dialog *Could not convert the file* with ffmpeg's last lines, per file | The same, full screen |

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *How previews are made* | `[preview] prefer` (`"auto"`, `"local"`, `"container"`) | `"auto"`: installed first, else the container |
| – | `[preview.prefer_tool] ffmpeg` | `[preview] prefer` |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Container images* (Pull, Remove) | `[preview.images] ffmpeg` | `"docker.io/mwader/static-ffmpeg:latest"`; `""` never uses a container. An image you set must have `/ffmpeg` and `/ffprobe`, as this one does |
| *Keep images up to date*, *Remove images not used for 90 days* | `[preview] image_updates`, `image_prune` | Off; a run counts as a use |
| *Keys* | `[keys] convert_media` | none |

There is no time limit: a long film takes a while, and **Stop** is there for that. ffmpeg works
in a folder of its own in the cache (`~/.cache/coxswain/media/`), removed when each run ends;
the result is then linked (or, on another disk, copied) next to the original.

## In the terminal app

The same action, menu and question, as dialogs; the status line shows how far it is, and the
action again (from **F9**, or from the action menu on a video) stops it, as the status line
says. It has no preview pane, so the facts are in **Properties** (**Alt+Enter**). The container
runs the same way as in the desktop app, so a terminal app over ssh can convert a video on a
server.

## Questions

#### Is my file changed?

No. The result is a new file next to it (`song.mp3`, `film.clip.mkv`, with ` 2` added when the
name is taken). Nothing is ever overwritten, and a stopped or failed run leaves nothing behind:
the name is taken only when the finished file is put there. Quitting while it runs stops it
first (the terminal app waits for it; a second **F10** leaves at once).

#### What does it need?

ffmpeg installed, or podman or docker: its container image (about 126 MB to download, 315 MB on
disk) is pulled on the first run, once the question before it has said so. On FreeBSD, the
other BSDs and illumos only an installed ffmpeg works.

#### Why does a clip start a little early?

A clip is copied as it is, without being encoded again: quick, and no quality is lost. A video
can only start at a key frame, so the clip starts at the key frame just before the time you
typed, often a second or two earlier. Convert the clip to MP4 afterwards for an exact start.

#### How long does an MP4 take?

It encodes the whole picture again, which keeps the processor busy: about as long as the video
plays on an ordinary laptop, much less on a fast one. Its sound only and a clip copy the data and
take seconds. The percent on the status line follows ffmpeg's own count.

#### Can I pick the quality or another codec?

Not here: the choices are the ones most asked for, with settings that suit most files. For
anything else, run ffmpeg yourself from the command line, or add it to the [user menu](../commands/user-menu.md).

#### It says it has no sound to take.

The file has no sound track (a screen recording without sound, say): *To MP3*, *To FLAC* and
*Its sound only* need one. *To MP4* still works.

#### Does anything leave my machine?

Only the image, downloaded from Docker Hub when you allow the first run; *Settings → Privacy and
updates → What can leave this machine* names it. The container has no network and sees only the
one file, and an installed ffmpeg reads and writes only here.

---
[← Previous: Make PDF searchable (Alt+P)](searchable-pdf.md) · [Next: Convert or edit pictures →](edit-pictures.md)

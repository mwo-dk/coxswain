[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Privacy: what stays, what can leave

Coxswain has no telemetry and no account. What it learns about your files (names, text, sizes,
hashes, meaning vectors) stays in files on your own disk, readable by you alone. This page lists
every time something can go over the network, and how to stop each one.

![Settings with Details → Meaning open: Made by a server with the OpenAI API, Server http://192.0.2.10:8000/api/v1 marked can leave this machine, and in bold: The text of your files, with their names and folders, is sent to 192.0.2.10:8000 to be read for meaning](../screenshots/reference-privacy-remote.png)

## Contents

- [How to use it](#how-to-use-it)
- [What stays on your machine](#what-stays-on-your-machine)
- [What can leave it](#what-can-leave-it)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

To keep everything on this machine:

1. Turn the update check off: Settings (**Ctrl+,**) → *Privacy and updates* → untick *Check for
   a new version*, or `check_updates = false` in `config.toml`.
2. For search by meaning, use the built-in model (`coxswain --meaning builtin`) or a server on
   this machine (`localhost`). Downloading the built-in model is the one request it makes, once.
3. Leave `[preview] container = "off"` if you do not want podman or docker to pull images, and
   *Settings → Previews → Keep images up to date* on **Off** (the default), and *Read other formats
   with Apache Tika* and *Transcribe speech with Whisper* off (the default).
4. Leave *Settings → Previews → Scan for viruses with ClamAV* off (the default), or know that a
   scan fetches ClamAV's virus database from `database.clamav.net` when it is more than a day old.

With those, nothing leaves the machine. Previews load nothing from the web, not even a picture a
Markdown file links: the app's own content security policy allows only the app and your disk.

## What stays on your machine

| What | Where |
|---|---|
| Your settings | `config.toml` |
| The session, favourites, tags, notes, recent repositories, dismissed notices, the last update check, the Subversion and CVS servers you said yes to | `state.json` |
| The name index: every file name in the folders indexed | `index.bin` in the cache folder |
| The search store: the text of your files, their sizes, hashes and meaning vectors | `search.db` in the cache folder, permissions `0600` (you alone) on the BSDs, illumos, Linux and macOS |
| The model for search by meaning | `models/` in the cache folder |
| Previews made by tools | `previews/` in the cache folder |
| Passwords of encrypted archives | Nowhere on disk: in the app's memory until it quits ([Passwords](../files/archive-passwords.md)) |

All paths: [Where things are kept](where-things-are-kept.md).

The apps talk to the [search helper](../search/helper.md) over a loopback connection on your own
machine. Its port and a random token are in `index.addr`, a file only you can read, so other
users of the machine cannot ask it anything.

Reading your documents for search is done by Coxswain itself, with no network. Installed
programs it may use (tesseract, pdftoppm, LibreOffice, and calibre for e-books, installed or in a
container with `--network=none`) run locally and write only into a
work folder in Coxswain's cache (`scratch/`), removed afterwards. Containers for previews run with
`--network=none`. Files copied out of an archive on the way to another archive pass through a work
folder of their own there, removed afterwards.

**Files only in the cloud** (OneDrive, Dropbox, Google Drive, Proton Drive, iCloud, cloud mounts
on Linux) are never read unless you open one: reading them would make the cloud app download
them. They are found by name only ([Cloud files](../search/cloud-files.md)).

**Other apps' data on a Mac** (`~/Library` but iCloud Drive and the cloud folders, the Photos and
Music libraries) is never opened by Coxswain's walks, and Coxswain needs no Full Disk Access.
macOS may ask once about Desktop, Documents, Downloads and removable or network volumes
([macOS](../systems/macos.md#the-folder-prompts)).

**HTML files** are shown as pages in a sandbox: the page runs no scripts and may load only
pictures, styles, fonts and media from your own disk (beside the file). A page that links a
script or a picture on the web gets nothing from the web ([HTML pages](../previews/html.md)).

## What can leave it

| When | What goes where | Who starts it | To stop it |
|---|---|---|---|
| **Update check**, when the app starts, then once a day while it is open | A request to `api.github.com` for the latest release's version number, with `User-Agent: coxswain/X.Y.Z` | Both apps, by themselves | `check_updates = false`, or untick *Settings → Privacy and updates → Check for a new version* ([Update checks](updates.md)) |
| **Model download**, once | The three files of multilingual-e5-small (about 465 MB) from `huggingface.co` | You: *Download the model* in Settings, or `coxswain --meaning on` | Do not turn search by meaning on, or use a server |
| **Search by meaning on another machine** | The text of your files (all of it, up to 256 passages each, every passage of long files set to *Everything*, with the file's name and two folders) and everything you type in Find while search by meaning is on, to that server | You: a *Server* that is not on this machine | Use the built-in model or a server on `localhost`. Settings warns: *The text of your files, with their names and folders, is sent to my-server to be read for meaning.* |
| **Chat model download**, once | The Qwen3 model and its tokenizer (1.0 GB, or 2.3 GB for the 4B one) from `huggingface.co` | You: *Download … and use* in Settings or the guide, or `coxswain --meaning ask builtin:qwen3-1.7b` | Use a chat model on a server, or none |
| **Ask** | With the [built-in chat model](../search/ask-builtin.md): nothing. Otherwise the question, the questions and answers before it in this Find, and the excerpts of the closest files with their paths (for a question about the folder as a whole also its README, project files, folder tree and docs index), as much as `ask_context` holds, to the chat model on the server (Ollama on this machine with the built-in model). Nothing is stored | You: asking in Find (**Ctrl+Enter** / **Alt+Enter**, or **Enter** on the *Ask* row) ([Ask](../search/ask.md)). With Find's scope on a folder, only excerpts of that folder are sent | Leave *Chat model* empty, or use a server on `localhost` |
| **Ollama pull** | Ollama downloads the model from its registry | You: *Pull bge-m3 with Ollama* or `coxswain --meaning ollama` | Do not pull |
| **Container images** | podman or docker downloads the image from its registry (`docker.io`; `ghcr.io` for Typst's and GDAL's) | You: the first build or render with a container, the first *Make PDF searchable* that needs OCRmyPDF's image, the first *Scan for viruses*, the first *Convert video or audio*, *Convert or edit pictures*, *Convert geodata*, *Export 3D model* or *Export from KiCad* that needs ffmpeg's, ImageMagick's, GDAL's, OpenSCAD's or KiCad's, turning on *E-books with calibre* with text search on (or calibre's first preview or *Convert e-book*), the first *Build PDF of AsciiDoc* or *Export draw.io diagram* that needs its image, or *Pull* in Settings. By itself only when you turn on *Keep images up to date* (weekly or monthly, the desktop app, the images already pulled) | `[preview] container = "off"`, or *Keep images up to date* → **Off** ([Containers](../previews/containers.md#keeping-images-up-to-date)) |
| **Virus database** | `freshclam`, in a container that sees only the database, fetches ClamAV's signatures (about 110 MB the first time, the day's changes after) from `database.clamav.net`; nothing of yours is sent | You: *Scan for viruses* (**Alt+I**), when the database is more than a day old, with *Scan for viruses with ClamAV* turned on ([Scan for viruses](../files/virus-scan.md)) | Leave *Scan for viruses with ClamAV* off (the default) |
| **Subversion or CVS history** | `svn log` or `cvs log` asks the server the working copy names for its history: which revisions touched the file or folder, with their authors, dates and messages. Inside a commit you open, `svn list`, `svn cat`, `svn diff` (or `cvs update -p`, `cvs diff`) ask the same server for the files and changes you look at or copy out. Your own `svn` or `cvs` config (saved passwords, proxies, `CVS_RSH`) holds | You: **Ctrl+G** in a working copy, once you said yes to that server in the question it asks first ([Subversion](../panels/subversion.md#history-from-the-server), [CVS](../panels/cvs.md#history-from-the-server)) | Say no; *Settings → Privacy and updates → History from servers* → **Forget** |
| **Links you click** | The release notes, a docs page under *Settings → Overview → What's new* (on GitHub), a feature's page from **Show me** in [Features and questions](../panels/features.md), a web link in a rendered Markdown or AsciiDoc file: each opens in your browser. *What's new* itself is built into the app | You | – |

On a machine with search by meaning on the built-in model, Coxswain also asks
`http://localhost:11434`, now and then, whether Ollama runs there, to suggest it. That request
never leaves the machine.

Every request Coxswain itself sends (the update check, a download, a server) names the app as
`User-Agent: coxswain/X.Y.Z` (its version), and no more about you or the machine.

Nothing else: no crash reports, no usage counts, no fonts or libraries fetched at run time.
Every preview library ships inside the app, and draw.io's viewer runs offline.

## What you see

- Settings → *Privacy and updates* → *What can leave this machine* lists every request your
  settings allow, each with where it goes, and marks the ones that stay on this machine; the
  *Privacy and updates* line in *Overview* names those that leave it.
- With a remote server chosen, Settings → *Finding files* → *Details* → *Meaning* shows under it
  *The text of your files, with their names and folders, is sent to my-server to be read for meaning.*, with the server's host name.
- An update found by the check shows as a button in the desktop app and a line in the terminal
  app's status line: [Update checks](updates.md#what-you-see).
- Nothing shows for requests that do not happen: there is no "sending data" indicator because
  nothing is sent in the background apart from the update check, the image updates when you
  turned them on, and Apache Tika's image and the Whisper model, once each, after you turn them on.

## Settings and config.toml

| Settings | Key | Default | Network when |
|---|---|---|---|
| *Privacy and updates* → *Check for a new version* | `check_updates` | `true` | On: when the app starts, then once a day while it is open, to GitHub |
| *Finding files* → *Details* → *Meaning* → *Made by* | `[search] meaning_engine` | `"builtin"` | `"ollama"` or `"openai"` with a remote `meaning_url` |
| *Finding files* → *Details* → *Meaning* → *Server* | `[search] meaning_url` | `""` | Not `localhost` |
| *Finding files* → *Details* → *Meaning* → *API key from the environment variable* | `[search] meaning_key_env` | `""` | The key is sent to the saved server only (unencrypted over `http://`), and never stored in `config.toml` |
| *Previews* → *Container runtime* | `[preview] container` | `"auto"` | Pulling an image, when you ask |
| *Finding files* → *Transcribe speech with Whisper* | `[search] whisper` | `false` | Once, when turned on: the Whisper model from `huggingface.co`, and whisper.cpp's image from `ghcr.io` when it is not installed ([Transcripts of speech](../search/transcripts.md)) |
| *Finding files* → *Read other formats with Apache Tika (container)* | `[search] tika` | `false` | Once, when turned on: Tika's image from `docker.io` ([Other formats with Apache Tika](../search/tika.md)) |
| *Previews* → *Scan for viruses with ClamAV* | `[preview] virus_scan` | `false` | On: before a scan, when the database is more than a day old, to `database.clamav.net` |
| *Previews* → *Keep images up to date* | `[preview] image_updates` | `"off"` | `"weekly"` or `"monthly"`: the desktop app pulls the images already pulled, from their registry |

## In the terminal app

The same. The terminal app's update check also runs when it starts, then once a day while it is
open (the result shared with the desktop app through `state.json`). `check_updates = false` turns
it off in both.

## Questions

#### Does anything leave my machine?

Only what the table above lists. With update checks and image updates off, and search by meaning
either off or on the built-in model (after its one download), nothing does. A picture a Markdown file links on
the web stays an empty box in the preview; the webview itself is held to the app and your disk,
so no preview of any kind can reach out.

#### Can other users on the same machine see my index?

No. At every start both apps (and the search helper) make the cache folder (`index.bin`,
`search.db`, previews, archive copies, the model) and the state folder (`state.json`: notes,
tags, favourites, recent folders) readable by you alone: the folders `700`, the files `600`.
Installs from before 1.26.4, whose files could be readable by others, are corrected at their first
start after the update. The helper answers only to a connection that knows its token, which is in
a file only you can read. (On Windows these folders are inside your own profile, which only you
can open.)

#### What does a remote embedding server get exactly?

The passages of each file with text in the folders read (the whole text, up to 256 passages of
about 120 words; every passage of long files set to *Everything*, [Long files](../search/long-files.md)), each with a first line naming the file, the two folders it is in and, in
Markdown, the heading above it (`budget.md · rocket/notes · Fuel`); and what you type in Find
while search by meaning is on (from two letters on, as you type). Not the full path. Before 1.39.0 it got
the first eight passages, without the line.

#### Is my archive password stored anywhere?

No. The box says so: *Its password (kept in memory while the app runs, never saved):*. It is kept in memory
while the app runs, so you are not asked again inside the same archive, and forgotten when the
app quits.

#### Can an HTML file I preview phone home?

No. It is shown in a sandbox with no scripts, and it may load only files from its own folder.
Tracking pixels and web fonts on the web are blocked, and a link in it goes nowhere: press
**Enter** to open the page in your browser.

#### Does the update check send anything about me?

Only what any web request sends: your IP address to GitHub, and `User-Agent: coxswain/X.Y.Z`.
No machine ID, no file names, no settings.

#### Does Coxswain talk to Ollama without asking?

Only to `localhost:11434`, on your own machine, to see whether Ollama runs there. It uses Ollama
only once you choose it.

---
[← Previous: Configuration: every key](configuration.md) · [Next: Update checks →](updates.md)

[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Where things are kept

Coxswain keeps its settings, its memory and its caches in the usual folders of each system. Use
this page to find a file, to see what takes room, or to remove everything.

![Settings, Search inside files: the search store's size and its path /home/demo/.cache/coxswain/search.db under the switch](../screenshots/gui-settings-search.png)
*Settings → Search inside files shows where the search store is and how large it is; Search by meaning shows the model's folder the same way.*

## Contents

- [How to use it](#how-to-use-it)
- [Every file and folder](#every-file-and-folder)
- [On each system](#on-each-system)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Removing everything](#removing-everything)
- [Questions](#questions)

## How to use it

`coxswain --paths` prints them for your machine:

```
$ coxswain --paths
config        /home/me/.config/coxswain/config.toml
state         /home/me/.local/share/coxswain/state.json
cache         /home/me/.cache/coxswain
name index    /home/me/.cache/coxswain/index.bin
search store  /home/me/.cache/coxswain/search.db
model         /home/me/.cache/coxswain/models/multilingual-e5-small-614241f6
previews      /home/me/.cache/coxswain/previews
archive looks /home/me/.cache/coxswain/peek
```

In the desktop app, the bottom of Settings (**Ctrl+,**) says *Settings are stored in …* with the
path of `config.toml`; *Search inside files* shows the search store and *Search by meaning* the
model's folder.

## Every file and folder

| What | Path (Linux) | Holds | Safe to delete? |
|---|---|---|---|
| `config.toml` | `~/.config/coxswain/config.toml` | Your settings ([Configuration](configuration.md)) | Yes: everything goes back to its default |
| `scripts/` | `~/.config/coxswain/scripts/` | Your scripts for the desktop app's **F2** ([Scripts](../commands/scripts.md)) | They are yours |
| `state.json` | `~/.local/share/coxswain/state.json` | The desktop app's session (panes, tabs, views, the preview engine you picked), favourites, colour tags, folder notes, recent repositories; the last update check; notices dismissed | Only if you want to lose tags and notes. A damaged file is moved aside to `state.json.bad`, never overwritten |
| `index.bin` | `~/.cache/coxswain/index.bin` | The name index, saved so it loads at once | Yes: it is built again |
| `search.db` (with `-wal`, `-shm`) | `~/.cache/coxswain/search.db` | The text of your files, every file's size and date, folder totals, duplicate hashes, meaning vectors | Yes, with no Coxswain running: it fills again. Or *Delete the index* in Settings |
| `models/` | `~/.cache/coxswain/models/multilingual-e5-small-614241f6/` | The built-in model for search by meaning, about 488 MB | Yes: *Delete the model* in Settings or `coxswain --meaning delete` does it for you |
| `peek/` | `~/.cache/coxswain/peek/<run>/` | The one file inside an archive being previewed or viewed, copied out for the look; replaced by the next, and left-overs of earlier runs go after a day | Yes, any time |
| `previews/` | `~/.cache/coxswain/previews/` | PDFs, SVGs and pages made by tools (LaTeX, LibreOffice, PlantUML, pandoc), one folder per file and engine | Yes: Settings → *Previews made by tools* → *Previews made so far* → *Clear* does it |
| `index.addr`, `index.lock` | `~/.cache/coxswain/` | The helper's port and token (readable by you alone), and its lock | Only while no helper runs |
| `libreoffice-profile/`, `libreoffice-index-profile/` | `~/.cache/coxswain/` | LibreOffice's own profiles for previews and for reading old Office files, so your open LibreOffice is left alone | Yes |
| Session registration | `~/.config/systemd/user/coxswain-index.service` (Linux), `~/Library/LaunchAgents/dk.mwo.coxswain.index.plist` (macOS), the `coxswain-index` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (Windows) | Only with *Start with my session* | Use `coxswain --index-service off` instead |
| `coxswain-extract-…` | The system's temporary folder | A few seconds' work of tesseract or LibreOffice while a file is read | Removed by itself |
| `coxswain-archive-…` | The system's temporary folder | Files on their way from one archive to another | Removed by itself |

Archive passwords are not in any file: they live in the app's memory until it quits.

## On each system

| Folder | Linux | macOS | Windows |
|---|---|---|---|
| Config (`config.toml`, `scripts/`) | `~/.config/coxswain/` (or `$XDG_CONFIG_HOME/coxswain/`) | `~/Library/Application Support/coxswain/` | `%APPDATA%\coxswain\` |
| State (`state.json`) | `~/.local/share/coxswain/` (or `$XDG_DATA_HOME/coxswain/`) | `~/Library/Application Support/coxswain/` | `%APPDATA%\coxswain\` |
| Cache (index, store, model, previews) | `~/.cache/coxswain/` (or `$XDG_CACHE_HOME/coxswain/`) | `~/Library/Caches/coxswain/` | `%LOCALAPPDATA%\coxswain\` |

On macOS and Windows the config and state share one folder.

## What you see

- `coxswain --paths` prints seven lines, the name left-aligned in 13 characters, then the path.
  A line with no path means the system has no such folder (rare: a user without a home folder).
- Settings → *Search inside files*: *Searchable: 31 files · still to read: 0 · 76.0 KB on disk*,
  then the path of `search.db`.
- Settings → *Previews made by tools*: *Previews made so far*: *12.4 MB in the cache; made again
  when a source changes*, with *Clear*.

## Settings and config.toml

None of these places can be changed in `config.toml`. They follow the system's folders; on Linux
the `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` variables move them.

## In the terminal app

The same folders and files: both apps share `config.toml`, `state.json`, the cache and the
helper. The terminal app writes to `state.json` only for the update check and notices; it does
not use `scripts/` or `previews/`.

## Removing everything

Uninstall the app the way you installed it ([README → Install](../../README.md#install)), then:

1. Turn off *Start with my session* if it was on: `coxswain --index-service off`.
2. Close every Coxswain window and the terminal app, and wait ten minutes for the helper to
   leave (or end the `coxswain --index-helper` process).
3. Delete the three folders above: config, state and cache.
4. Container images Coxswain pulled stay in podman or docker until you remove them there (or with
   *Remove* in Settings → *Container images* beforehand). A model pulled with Ollama goes with
   `ollama rm bge-m3`.

## Questions

#### The cache folder is large. What takes the room?

Usually `search.db` (the text of your files) and `models/` (488 MB, when search by meaning is on
with the built-in model), then `previews/`. Settings shows the size of each.

#### Can I move the cache to another disk?

Coxswain uses the system's cache folder. On Linux, set `XDG_CACHE_HOME` for the session; on other
systems, a symbolic link from `coxswain` in the cache folder to the other disk works.

#### Is my config shared between the terminal app and the desktop app?

Yes: one `config.toml` for both, and one search helper and cache.

#### How do I copy my tags, notes and favourites to another machine?

Copy `state.json` while no Coxswain runs. Tags and notes are kept by full path, so they show only
where the same paths exist.

#### I deleted `search.db` and Find file finds no text.

It fills again from the start: the helper reads your files anew, at half speed. Settings →
*Search inside files* → *Index now* reads at full speed.

#### What is `state.json.bad`?

A `state.json` that could not be read when an app started, moved aside so a new one could be
saved. Your tags and notes are in it; mend it by hand or delete it.

---
[← Previous: Update checks](updates.md) · [Next: Security →](security.md)

[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Search settings

Two sections of the desktop app's Settings hold everything about search: *Search inside files*
and *Search by meaning*. Every item is also a key under `[search]` in `config.toml`, which is how
the terminal app sets them.

![Settings, Search inside files: Keep the text of files ticked, Searchable: 107 files · still to read: 0 · 284 KB on disk, the path /home/demo/.cache/coxswain/search.db, Index now, Delete the index, Folders read and Names only](../screenshots/gui-settings-search.png)
*Settings → Search inside files.*
<!-- screenshot: search-settings-meaning.png: desktop app, Cyber theme, Settings → Search by meaning with the built-in model on: the hint, Vectors made by "Built-in model, on this machine (465 MB once)", "Understood: … files · still to go: …", builtin:multilingual-e5-small@614241f6, the model's folder, Turn off and Delete the model -->

## How to use it

| To open | Desktop app | Terminal app |
|---|---|---|
| Settings | **Ctrl+,**, the *Settings* button at the right of the command line, or *Settings* in the F9 command list | No Settings window: edit `config.toml` (`coxswain --config-path` prints where) |
| At *Search inside files* | `coxswain-gui --settings=search`, or a notice about tesseract | |
| At *Search by meaning* | `coxswain-gui --settings=meaning`, the notice, or *turn on search by meaning* in Find file | `coxswain --meaning …` |

A change in either section is saved at once and starts a new [helper](helper.md) with it.

## Search inside files

| Item | Key | Type, default | Does |
|---|---|---|---|
| *Keep the text of files, so Find file can search in it (Shift+F7)* | `text` | bool, `true` | Off: no [text search](text.md), no search by meaning, and folder sizes and duplicates lose the store's help. The rest of the section is hidden |
| *Search inside archives: the files in zip, 7z and tar archives are found by name and by their text* | `archives` | bool, `true` | See [Inside archives](archives.md). Off: an archive is found by its own name only |
| *Look inside archives everywhere the names are indexed* (under the one above, while it is on) | `archives_everywhere` | bool, `false` | On: the entries of every archive on the machine are found by name, caches included; their text is still read only in the folders read. See [Which archives](archives.md#which-archives) |
| Status | | | *Searchable: 31,208 files · still to read: 412 · 1.1 GB on disk*, where `search.db` is, and on battery *Paused while the machine runs on its battery. Index now reads anyway.* Without a helper: *The search helper is not running, so text cannot be searched now.* |
| *Start the search helper with my session, so it reads while no window is open* | Not in `config.toml` | off | See [The search helper](helper.md) |
| **Index now** | | | Reads the backlog at full speed, on battery too. Greyed out when nothing is left to read |
| **Delete the index** | | | Empties the store: text, sizes, hashes, vectors. A second click confirms (*Click again to delete*). It fills again from the start |
| *Folders read* | `text_roots` | list, `[]` = *Your home folder* | See [Choosing the folders](folders.md) |
| *Names only* | `names_only` | list, `[]` = *None* | See [Choosing the folders](folders.md) |
| *Search the history of git repositories too: commit messages, authors and changed paths* | `history` | bool, `true` | The newest 2000 commits of each repository in the folders read; see [Git history in search](history.md) |
| *Cloud files: read files that are only online (downloads them)* | `cloud` | `"local-only"` or `"all"`, `"local-only"` | Off: files only in OneDrive, Dropbox, Google Drive, Proton Drive or iCloud are found by name, never read or downloaded. See [Cloud files](cloud-files.md) |
| *Read anyway* | `cloud_read` | list, `[]` | Clouds found, each with **Read its files**, and folders added: their online-only files are read (downloaded). See [Cloud files](cloud-files.md) |
| *Programs that read more* | | | tesseract, pdftoppm and LibreOffice, each ✓ or ✗ *not installed*: see [Scans](scans.md) |

Keys with no item: `text_exclude` (folder names left out), `text_max_size` (20 MB), and for the
name index `roots`, `exclude`, `watch`, `max_results`.

## Search by meaning

| Item | Key | Type, default | Does |
|---|---|---|---|
| **Set up…** | | | Opens the guided setup, [Smart search in a few minutes](setup.md), which sets the items below for you |
| *Vectors made by* | `meaning_engine` | `"builtin"` / `"ollama"` / `"openai"`, `"builtin"` | *Built-in model, on this machine (465 MB once)*, *Ollama*, or *A server with the OpenAI API (Lemonade, LM Studio, llama.cpp …)* |
| *Server* | `meaning_url` | string, `""` | The server's base URL; empty is Ollama on this machine |
| *Embedding model* | `meaning_model` | string, `""` | The server's model; empty is `bge-m3` for Ollama |
| **Pull bge-m3 with Ollama** | | | Shown when Ollama lacks the model; fetches it with a progress bar |
| *API key from the variable* | `meaning_key_env` | string, `""` | OpenAI API only: the environment variable holding the key |
| *Use the CPU only* | `meaning_device` | `"auto"` / `"cpu"`, `"auto"` | On a Mac only: keeps the built-in model off the GPU ([on a Mac's GPU](meaning.md#on-a-macs-gpu)) |
| *Ask → Chat model* | `ask_model` | string, `""` | The chat model that writes [Ask](ask.md)'s answers, on the same server (Ollama here with the built-in model). Empty: Ask is not set up |
| **Download the model (465 MB) and turn on** | `meaning` | bool, `false` | Downloads the built-in model, then sets `meaning = true`. **Cancel** stops the download |
| **Turn on** / **Turn off** | `meaning` | | On or off, keeping the model |
| **Delete the model** | | | Turns it off and deletes the built-in model |
| Status | | | *Understood: 8,120 files · still to go: 23,088*, where the built-in model runs (*Built-in model · on the GPU (Metal)*), the model in use, the model's folder, an error, the battery line |

Details on [Search by meaning](meaning.md) and [servers](servers.md).

## What you see

Everything above updates every few seconds while Settings is open: the counts go down as the
helper reads. Errors from the helper or a server show in red under the section.

## Settings and config.toml

The whole `[search]` table with its defaults:

```toml
[search]
roots = []                 # the name index: empty = the whole machine
exclude = ["/proc", "/sys", "/dev", "/run", "/tmp/.X11-unix"]
max_results = 10000
watch = true
text = true
text_roots = []            # empty = your home folder
text_exclude = ["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash"]
names_only = []
text_max_size = 20971520
archives = true            # look inside zip, 7z and tar archives
archives_everywhere = false # true: every archive on the machine, caches too (names only)
cloud = "local-only"       # "all": read files only in the cloud too (downloads them)
cloud_read = []            # cloud folders read anyway
meaning = false
meaning_engine = "builtin" # or "ollama", "openai"
meaning_url = ""
meaning_model = ""
meaning_key_env = ""
ask_model = ""
```

See also [Configuration](../reference/configuration.md).

## In the terminal app

No Settings window. The flags do what the buttons do:

| Flag | Does |
|---|---|
| `coxswain --meaning on` / `off` / `delete` | Download and turn on / turn off / turn off and delete the model |
| `coxswain --meaning ollama [MODEL]` | Ollama on this machine, the model pulled when missing |
| `coxswain --meaning server URL MODEL` | A server with the OpenAI API |
| `coxswain --meaning builtin` | Back to the built-in model |
| `coxswain --meaning` | Prints `on` or `off` |
| `coxswain --index-service on` / `off` | The helper with your session, or not |
| `coxswain --paths` | Where the config, state, index, search store and model are |

There is no *Index now* or *Delete the index* flag: delete `search.db` while no Coxswain runs to
start afresh. More in [Command-line flags](../reference/command-line-flags.md).

## Questions

#### Why is most of *Search inside files* hidden?
*Keep the text of files* is unticked. Tick it and the status, buttons and lists come back.

#### Why are *Index now* and *Delete the index* greyed out?
The helper cannot be reached, or (for *Index now*) nothing is left to read.

#### I edited `config.toml` while Settings was open. Which wins?
Settings writes only the key you change, into the same file. The helper takes the file as it is
when it starts; make a change in Settings to start it again.

#### Where is the checkbox for starting with my session kept?
Not in `config.toml`: it is the systemd unit, LaunchAgent or *Run* entry itself. Settings shows
whether it is there.

#### How do I open Settings right at search?
`coxswain-gui --settings=search` or `--settings=meaning`. Inside the app, the notices and Find file's
link open *Search by meaning*.

#### Can I set these per folder or per window?
No. The search settings are for the machine: one helper serves every window.

---
[← Previous: Notices and what's new](notices.md) · [Next: The preview pane →](../previews/README.md)

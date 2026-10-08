[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Command-line flags

Every flag of both programs, `coxswain` (the terminal app) and `coxswain-gui` (the desktop app).
Use them to start in certain folders, to find where Coxswain keeps its files, and to change the
settings that do more than set a value (search by meaning, the search helper) from a script.
Both apps also open their Settings from the command line (`--settings`). For the command line *inside* Coxswain, where you type shell commands, see
[The command line and its output](../commands/command-line.md).

![A terminal in the demo sandbox after coxswain --version (coxswain 1.28.3) and coxswain --paths: config, state, cache, name index, search store, model, previews and archive looks, each with its path under /home/demo](../screenshots/reference-paths.png)

## Contents

- [How to use it](#how-to-use-it)
- [The terminal app: `coxswain`](#the-terminal-app-coxswain)
- [`--setup-search`](#--setup-search)
- [`--meaning`](#--meaning)
- [`--index-service`](#--index-service)
- [The desktop app: `coxswain-gui`](#the-desktop-app-coxswain-gui)
- [The helper: `--index-helper`](#the-helper---index-helper)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to use it

1. Open a shell. Both programs are on your `PATH` after an install ([README → Install](../../README.md#install)).
2. Type the program, then the flag: `coxswain --paths`, `coxswain-gui --settings=search_meaning`.
3. A flag is read only as the **first** argument, and one at a time. `coxswain --paths --version`
   prints the paths and ignores `--version`.

| Want | Terminal app | Desktop app |
|---|---|---|
| Start in two folders | `coxswain ~/src ~/Downloads` | `coxswain-gui ~/src ~/Downloads` |
| Start on a file | `coxswain notes.md` | `coxswain-gui notes.md` |
| The version | `coxswain --version` | The title of *Help* (**F1**), or the window title |
| Settings, at an area or an option | `coxswain --settings=search` | `coxswain-gui --settings=search` |
| Where things are kept | `coxswain --paths`, or `coxswain --settings=privacy` | `coxswain-gui --settings=privacy`: *Where things are kept* |
| Every default | `coxswain --dump-config` | – |
| Which languages there are | `coxswain --languages` | `coxswain-gui --settings=language` |
| The status-line hints again | `coxswain --hints reset` | `coxswain-gui --settings=hints`, then *Show the hints again* |
| What the new version brought | `coxswain --whats-new` | `coxswain-gui --settings=overview`, or click *⚙ Settings*: *What's new* is in the Overview |
| Search by meaning on | `coxswain --meaning on` | `coxswain-gui --settings=search`, then the level *Names, text and meaning* |
| Find duplicates in a folder | – | `coxswain-gui --duplicates ~/Pictures` |

## The terminal app: `coxswain`

`cox` is the same program under a shorter name. `coxswain --help` prints:

```
coxswain [LEFT] [RIGHT]      a folder, or a file to open its folder with the cursor on it
  --settings[=AREA|OPTION]  start with Settings open (also F9 → Settings): search, previews,
                  looks, behaviour, keys, privacy, or an option such as show_hidden
  --dump-config   print the full default config (redirect it to the config file to customise)
  --config-path   print where the config file is read from
  --paths         print where everything is kept: config, state, index, search store, models
  --setup-search  set up search inside files, by meaning and Ask, step by step: finds the model
                  servers on this machine and what suits it
  --index-service on|off   start the search helper with your session, or stop doing so
  --meaning on|off|delete  search by meaning: download the model and turn it on, turn it off,
                           or turn it off and delete the model
  --meaning ollama [MODEL] meaning read by Ollama here (bge-m3 unless named; pulled if missing)
  --meaning server URL MODEL  meaning read by a server with the OpenAI API (Lemonade, LM Studio)
  --meaning builtin        back to the built-in model
  --meaning cpu|auto       the built-in model on the CPU only, or on the Mac's GPU (Metal)
                           when it has one (auto, the default)
  --meaning ask MODEL|off  Ask in Find: the chat model on that server (Ollama here with the
                           built-in model) that answers questions from your files, or one
                           built in: builtin:qwen3-1.7b, builtin:qwen3-4b or
                           builtin:qwen3-14b (downloaded once);
                           delete: the built-in ones deleted
  --models                 the built-in models on the disk: what each is for, its size and
                           folder, whether it is in use and loaded, when it was last used
  --models delete NAME|unused  delete one (by the name --models shows), or all not in use
  --hints reset            show the hints on the command line again, each a few times
  --languages              the languages, by region, and how to help improve a new translation
  --whats-new [all]        what the versions since you last looked brought (all: every version)
  --version
  --help
```

| Flag | Does |
|---|---|
| `LEFT`, `RIGHT` | The folders of the left and right panel. A relative path is from the current folder, `~` is home. A file opens its folder with the cursor on it. Without them: the current folder in both |
| `--settings[=AREA\|OPTION] [LEFT] [RIGHT]` | Starts the app with [Settings](../customise/settings.md#in-the-terminal-app) open: at Overview, at an area (`overview`, `search`, `previews`, `looks`, `behaviour`, `keys`, `privacy`) or with the cursor on an option (`show_hidden`, `search_meaning`, `ask_model`, `language`, `max_results` …). **Esc** closes it to the panels, which show `LEFT` and `RIGHT` as without the flag. A name it does not know, such as 1.x's `meaning`, `ask`, `news` or `cloud`, opens Overview |
| `--help`, `-h` | Prints the text above |
| `--version`, `-V` | Prints `coxswain 1.20.0` |
| `--dump-config` | Prints every option with its default, as TOML, including every built-in theme and the full `[keys]` table. `coxswain --dump-config > ~/.config/coxswain/config.toml` gives you the full list to edit |
| `--config-path` | Prints where `config.toml` is read from |
| `--paths` | Prints where everything is kept, one line each: `config`, `state`, `cache`, `name index`, `search store`, `models` (the folder of the built-in models), `previews`, `archive looks`. See [Where things are kept](where-things-are-kept.md) |
| `--setup-search` | The guided setup of search inside files, by meaning and Ask: see [below](#--setup-search) |
| `--index-service [on\|off]` | The search helper with your session: see [below](#--index-service) |
| `--meaning [on\|off\|delete\|ollama\|server\|builtin\|cpu\|auto]` | Search by meaning: see [below](#--meaning) |
| `--models` | Lists the [built-in models](../search/models.md) on the disk, leftovers too: for each its folder's name, its name, *for Ask · 8.4 GB · in use · loaded on the CPU, about 8.4 GB of memory · last used 2026-10-08*, and its folder; then *Delete all not in use (…): coxswain --models delete unused* when there is something to free |
| `--models delete NAME` | Deletes one, named by its folder's name, its id (`qwen3-14b`) or its name; the one in use asks *[y/N]* first and says what comes instead (*Ask now uses qwen3:8b.*). No such model: exit status 1 |
| `--models delete unused` | Deletes every model not in use, unfinished downloads and older versions included: *Deleted 2 not in use: 9.4 GB freed.* |
| `--index-helper` | Runs as the search helper instead of the app: see [below](#the-helper---index-helper) |
| `--hints reset` | Every hint on the status line shows three times again, in both apps (they share `state.json`), and says so. See [The action menu and hints](../panels/action-menu.md#hints-on-the-status-line) |
| `--languages` | Prints every language under its region, one line each: code, own name, *new* for a fresh translation and *(current)* for the one in use, then where `language` is set and where to suggest a better word. In your language. See [Languages](../customise/languages.md) |
| `--whats-new` | Prints the changes of the versions you have not read yet, newest first, or of the version you run when you have read them all, then counts them as read (in both apps). Each version is a line `1.29.0  2026-10-04`, then its changes, with each docs link as `Find <https://github.com/mwo-dk/coxswain/blob/master/docs/search/find-file.md>`. See [Notices and what's new](../search/notices.md) |
| `--whats-new all` | Prints every version's changes, newest first |

An option it does not know is not taken for a folder: it is said, with the closest one, and
nothing starts (exit status 2):

```
$ coxswain --model
coxswain: unknown option --model
did you mean --models?
coxswain --help lists them
$ coxswain --meaning of
coxswain: unknown value of for --meaning: it takes on, off, delete, ollama, server, builtin, cpu, auto, ask
did you mean --meaning off?
$ coxswain nowhere
coxswain: no such folder or file: nowhere
```

A value the option does not take (`--index-service maybe`) is said the same way, and
`--hints` alone says *--hints needs one of: reset*.

### `--meaning`

These write `config.toml` the way Settings does (keeping your comments and layout), then start a
new search helper with the new settings. See [Search by meaning](../search/meaning.md) and
[Search by meaning on a server](../search/servers.md).

| Flag | Does |
|---|---|
| `--meaning` | Prints `on` when `[search] meaning` is on and its engine is ready (the built-in model downloaded, or a server set), else `off`. With the built-in model on, a second line says where it runs: `Built-in model · on the GPU (Metal)`, `Built-in model · on the CPU`, or on a Mac that could not use its GPU `Built-in model · on the CPU (this Mac has no Metal GPU to use)` |
| `--meaning cpu` | The built-in model on the CPU only, on a Mac too: sets `meaning_device = "cpu"` |
| `--meaning auto` | The built-in model on the Mac's GPU (Metal) when it can, the CPU elsewhere: sets `meaning_device = "auto"`, the default |
| `--meaning on` | Downloads the built-in model (multilingual-e5-small, about 488 MB) if it is not there, showing `Downloading: 42 %`, then sets `meaning = true`. It does not change `meaning_engine` |
| `--meaning off` | Sets `meaning = false`. The model stays on disk |
| `--meaning delete` | Sets `meaning = false` and deletes the model folder |
| `--meaning ollama [MODEL]` | Uses Ollama on this machine (`http://localhost:11434`) with `MODEL`, default `bge-m3`. When Ollama lacks the model it pulls it first: `Pulling bge-m3 with Ollama…`. Sets `meaning_engine = "ollama"`, `meaning_model` and `meaning = true` |
| `--meaning server URL MODEL` | Uses a server with the OpenAI API at `URL` (for example `http://localhost:8000/api/v1` for Lemonade) with `MODEL`. It first asks the server for its models, so a wrong address fails here, not later. Sets `meaning_engine = "openai"`, `meaning_url`, `meaning_model` and `meaning = true` |
| `--meaning builtin` | Back to the built-in model: sets `meaning_engine = "builtin"`, then does `--meaning on` |
| `--meaning ask MODEL` | [Ask](../search/ask.md)'s chat model, on the vectors' server (Ollama here with the built-in model). On Ollama a missing model is pulled first; an OpenAI-style server must list it. Sets `ask_model`. `--meaning ask off` empties it |
| `--meaning ask builtin:qwen3-1.7b` | [The built-in chat model](../search/ask-builtin.md) instead, with no server: downloads it from huggingface.co once (1.0 GB; `builtin:qwen3-4b` is 2.3 GB, `builtin:qwen3-14b` 8.4 GB), showing `Downloading: 42 %`, then sets `ask_model`. `--meaning ask delete` deletes the built-in chat models, and empties `ask_model` when it named one |

`--meaning server` without both a URL and a model prints
`usage: coxswain --meaning server URL MODEL, e.g. http://localhost:8000/api/v1 nomic-embed-text-v1-GGUF`.
A server that needs an API key reads it from the environment variable named in
`meaning_key_env` ([Configuration](configuration.md#search)); there is no flag for it.

### `--setup-search`

Asks, step by step, what the desktop app's **Set up…** (in *Settings → Overview* or *Finding files*) shows: search
inside files on or off, where the vectors come from (the servers found on this machine, the
built-in model, or a server you name), the model for the vectors, Ask's chat model with a test
question, whether the server uses the graphics card, and *Start with my session*. A number
chooses, **Enter** takes the default (marked `*`), **s** skips; downloads ask *[y/N]* first. See
[Smart search in a few minutes](../search/setup.md).

### `--index-service`

| Flag | Does |
|---|---|
| `--index-service` | Prints `on` or `off`: whether the search helper starts with your session |
| `--index-service on` | Registers the helper with the system (a systemd user unit on Linux, a LaunchAgent on macOS, a Run entry on Windows, an XDG autostart entry on FreeBSD, the other BSDs and illumos) and starts it. The same as Settings → *Finding files* → *Details* → *Background reading* → *Start with my session* |
| `--index-service off` | Removes the registration; the running helper makes way for one that leaves with the apps |

See [The search helper](../search/helper.md).

### Exit status

| Status | When |
|---|---|
| `0` | Done, or the app quit normally |
| `1` | A `--meaning` or `--index-service` flag failed; it prints `coxswain: …` with the reason |
| `2` | `config.toml` could not be read or has a mistake: `coxswain: config: …` or `coxswain: unknown key 'Ctlr+P'`. The app does not start |

## The desktop app: `coxswain-gui`

```
coxswain-gui [--settings[=AREA|OPTION]] [--duplicates [FOLDER …] | [LEFT] [RIGHT]]
```

| Flag | Does |
|---|---|
| `LEFT`, `RIGHT` | As in the terminal app. The last session is restored, and `LEFT` is added to the left pane as a new tab when no tab there shows it. `RIGHT` is used only when there is no saved session. Started from a desktop menu, where the current folder is `/`, it takes your home folder |
| `--settings` | Opens with the Settings window open, at *Overview* |
| `--settings=search` | …at *Finding files* |
| `--settings=search_meaning` | …at *Finding files*, with *Details → Meaning* open and scrolled to |
| `--settings=ask_model` | …at *Finding files*, with *Details → Ask* open and scrolled to |
| `--settings=language` | …at *Looks*, scrolled to *Language* ([Languages](../customise/languages.md)) |
| `--settings=overview` | …at *Overview*, where *What's new* is ([Notices and what's new](../search/notices.md)) |
| `--settings=AREA` | …at that area: `overview`, `search`, `previews`, `looks`, `behaviour`, `keys`, `privacy` |
| `--settings=OPTION` | …at the area of that option, with its group open, scrolled to it and lit up for a moment: an option's name, such as `check_updates` or `folder_sizes` |
| `--duplicates [FOLDER …]` | Opens *Find duplicates* and scans these folders at once; without folders, the current one. The panes open as usual behind it |
| `--index-helper` | Runs as the search helper, with no window: see [below](#the-helper---index-helper) |

`--settings` must come first, then `--duplicates`; everything else is taken as folders. Given
both, *Find duplicates* opens (there is one window at a time); Settings is a **Ctrl+,** away.
Any other word after `--settings=` opens it at *Overview*; so do the section names of 1.x
(`meaning`, `ask`, `news`, `cloud`), which 2.0 no longer knows.

The desktop app has no `--help`, `--version`, `--paths` or `--meaning`: use the terminal app's
flags, which work on the same `config.toml`, the same cache and the same helper. A
`config.toml` it cannot read does not stop it: it starts with every default and prints
`coxswain: config: …; using defaults` on the terminal it was started from.

## The helper: `--index-helper`

`coxswain --index-helper` and `coxswain-gui --index-helper` make the program the
[search helper](../search/helper.md) instead of an app: no window, no panels. The apps start it
by themselves when none runs; you never need to. It leaves ten minutes after the last app has
gone. With `--stay` after it, it does not leave; that is how the session registration
(`--index-service on`) starts it.

## What you see

- The printing flags (`--help`, `--version`, `--paths`, `--config-path`, `--dump-config`, and
  `--meaning` or `--index-service` without a word after them) print to the terminal and exit.
  No window opens.
- `--meaning on` shows the download percentage on one line while it runs; `--meaning ollama`
  shows `Pulling bge-m3 with Ollama…`. Both print nothing when they succeed.
- `coxswain-gui --settings=…` and `--duplicates` open the main window with that dialog on top.

## Settings and config.toml

The flags write these keys:

| Flag | Keys it writes |
|---|---|
| `--meaning on`, `off`, `delete` | `[search] meaning` |
| `--meaning ollama` | `[search] meaning_engine`, `meaning_model`, `meaning` |
| `--meaning server` | `[search] meaning_engine`, `meaning_url`, `meaning_model`, `meaning` |
| `--meaning ask` | `[search] ask_model` |
| `--meaning builtin` | `[search] meaning_engine`, `meaning` |
| `--index-service` | None: it writes the system's session registration, not `config.toml` |

Every other flag changes nothing. See [Configuration: every key](configuration.md).

## Questions

#### How do I find out which version I have?

`coxswain --version` prints `coxswain 1.20.0`. In the desktop app, the title of *Help* (**F1**)
and the window title show it: `Coxswain 1.20.0`.

#### `coxswain-gui --help` opened a window.

The desktop app takes arguments it does not know as folders. Use `coxswain --help`.

#### `coxswain --meaning ollama` says it cannot connect.

Ollama must be running on this machine (`ollama serve`) on its usual port 11434. The flag always
talks to Ollama on this machine. For Ollama on another machine, set `meaning_url` in
`config.toml` or use Settings → *Finding files* → *Details* → *Meaning* → *Server*.

#### Can I set the API key for a server on the command line?

No, on purpose: a key on the command line ends up in your shell history. Put it in an
environment variable and name that variable in `[search] meaning_key_env`. The key itself is
never written to `config.toml`.

#### I gave the desktop app two folders and the right pane ignored mine.

With a saved session, the desktop app restores it and only adds `LEFT` as a tab on the left.
`RIGHT` is used on a first start, when there is no session yet.

#### How do I open Settings straight at search by meaning?

`coxswain-gui --settings=search_meaning`: *Finding files*, with *Details → Meaning* open.
`--settings=search` opens *Finding files* at the top, `--settings=ask_model` at *Details → Ask*.
`coxswain --settings=search_meaning` does the same in the terminal app. A desktop menu entry or
a script can use these.

#### Why does `--settings=meaning` open the Overview now?

2.0 dropped the section names of 1.x: `--settings=` takes an area or an option's name only, and
anything else opens the Overview. Use `search_meaning` for `meaning`, `ask_model` for `ask`,
`overview` for `news` and `search_cloud` for `cloud`; `language` still works, as an option's name.
The names are listed in [Settings and config.toml](../customise/settings.md#settings-and-configtoml).

#### Does `--dump-config` change my config?

No. It prints to the terminal only. Redirect it with `>` yourself if you want a file; see
[Configuration](configuration.md#questions) before you do.

#### Why is a flag after a folder ignored?

A flag is looked at only as the first argument. In `coxswain ~/src --paths`, `--paths` is taken
as the right panel's folder, not as a flag. Put the flag first, on its own: `coxswain --paths`.

---
[← Previous: The terminal app](terminal-app.md) · [Next: Configuration: every key →](configuration.md)

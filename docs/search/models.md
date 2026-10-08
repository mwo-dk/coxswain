[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Built-in models: where they are, and freeing them

Coxswain downloads a model only when you choose one: the embedding model for
[search by meaning](meaning.md) (multilingual-e5-small, 465 MB) and the chat models for
[Ask without a server](ask-builtin.md) (Qwen3 1.7B, 4B Instruct, 14B). They live in the cache
folder, and the [search helper](helper.md) loads the one in use when it is needed. *Built-in
models* lists every one on the disk, with what is left over: an unfinished download, or a model
of an older version that a new release replaced. You see where each is, whether it is loaded and
where it runs, and you can unload it, delete it, or delete everything not in use.

Models a server runs (Ollama, Lemonade, LM Studio) are not listed: the server keeps and loads
them itself (`ollama list`, `ollama rm`).

![Settings → Finding files → Details → Built-in models in the desktop app: Qwen3 14B, in use, for Ask · 8.4 GB · in use · not loaded · last used 2026-10-07, with its folder, Show in panel and Delete; multilingual-e5-small, in use, for search by meaning · 465 MB; Qwen3 1.7B, an unfinished download · 977 KB · not in use; Qwen3 14B, an older version · 2.9 MB · not in use; then Delete all not in use (3.8 MB)](../screenshots/settings-models.png)

## Contents

- [What it shows](#what-it-shows)
- [Where to find it](#where-to-find-it)
- [Unload, delete, delete all not in use](#unload-delete-delete-all-not-in-use)
- [Deleting the model in use](#deleting-the-model-in-use)
- [On the command line](#on-the-command-line)
- [The terminal app and the desktop app](#the-terminal-app-and-the-desktop-app)
- [Questions](#questions)

## What it shows

One entry per folder in the cache folder's `models` folder, those in use first:

| Part | Example | Says |
|---|---|---|
| Name | *Qwen3 14B*, *multilingual-e5-small* | The model; a leftover by the model it was |
| What it is for | *for Ask*, *for search by meaning*, *an unfinished download*, *an older version*, *not one of Coxswain's models* | Leftovers are never in use |
| Size | *8.4 GB* | On the disk, all its files |
| In use | *in use* / *not in use* | `ask_model` names it, or search by meaning is on with the built-in model |
| Loaded | *loaded on the CPU, about 8.4 GB of memory*, *loaded on the GPU (Metal)*, *not loaded* | Whether the search helper (or the app, without a helper) holds it now, and where |
| Last used | *last used 2026-10-08*, *not used yet* | When it was last loaded or asked |
| Folder | `~/.cache/coxswain/models/qwen3-14b-a04a82c4` | With **Show in panel** (desktop) or **Enter** (terminal) |

The memory is about the size of the model's weights; a chat model holds a little more for the
question it reads. A chat model is let go by itself five minutes after its last answer.

## Where to find it

| App | Where |
|---|---|
| Desktop app | **Ctrl+,** → *Finding files* → *Details* → *Built-in models* (or `coxswain-gui --settings=models`). Each model has its line, its folder with **Show in panel**, **Unload now** (a chat model that is loaded) and **Delete**; **Delete all not in use (8.4 GB)** under the list |
| Terminal app | **F9** → *Settings* → *Finding files*, the *Built-in models* heading near the end (or `coxswain --settings=models`). One row per model: its name, then its line. The bottom line names the folder and the keys: *Enter: show its folder · Delete: delete it · U: unload it*. The last row is **[ Delete all not in use (8.4 GB) ]** |
| Command line | `coxswain --models` ([below](#on-the-command-line)) |

## Unload, delete, delete all not in use

| Action | Desktop app | Terminal app | Does |
|---|---|---|---|
| Show its folder | **Show in panel** | **Enter** on its row | Settings closes; the active panel opens the folder |
| Unload now | **Unload now** | **U** on its row | The helper lets the chat model go at once: *Qwen3 14B is unloaded: its memory is free.* The next question loads it again. The embedding model stays loaded while search by meaning uses it: *It stays loaded while search by meaning uses it.* |
| Delete | **Delete** (a second click on the one in use: *Click again to delete Qwen3 14B*) | **Delete** on its row (the one in use asks: *Qwen3 14B is in use. Delete it anyway? Enter deletes it, Esc keeps it*) | The folder goes: *Deleted Qwen3 14B: 8.4 GB freed.* A chat model is unloaded first |
| Delete all not in use | **Delete all not in use (size)** | **Enter** on **[ Delete all not in use (size) ]** | Every model not in use goes, leftovers included: *Deleted 2 not in use: 9.4 GB freed.* |

## Deleting the model in use

You are asked first. Then the setting gives way to the recommended choice and the line says
which:

| Deleted | Then | You see |
|---|---|---|
| Ask's built-in chat model | Ask takes a server's chat model on the graphics card when one answers (Ollama here with the built-in vectors, else the server that makes them), else another built-in chat model that is downloaded, else Ask is off | *Qwen3 14B was Ask's model: Ask now uses qwen3:8b.* or *… Ask is off until you choose another.* |
| The embedding model, while search by meaning uses it | A server's embedding model on the graphics card, when one answers; every file's meaning is read again. Else search by meaning is off (and Ask with it) | *multilingual-e5-small read the meaning of your files: bge-m3:latest reads it now, every file again.* or *… search by meaning is off until you choose a model.* |

## On the command line

```sh
coxswain --models                     # the list
coxswain --models delete qwen3-14b    # one, by its id, its name or the folder's name
coxswain --models delete unused       # everything not in use, leftovers too
```

`coxswain --models` prints each model's folder name, its name, its line and its folder, then
*Delete all not in use (8.4 GB): coxswain --models delete unused* when there is something to
free. Deleting the one in use asks *Qwen3 14B is in use. Delete it anyway? [y/N]* and says what
comes instead. A name that matches nothing: *No built-in model is called …: coxswain --models
lists them.*, with exit status 1. It works in the desktop app's package too, as `coxswain` is the
terminal app.

## The terminal app and the desktop app

Both list the same models from `coxswain_core::models` and ask the same helper what is loaded.
The desktop app shows the folder with a **Show in panel** link and asks for a second click on
the model in use; the terminal app uses keys on the row and asks on its bottom line. Only the
terminal app has `coxswain --models`. There are no `config.toml` keys: the list is what is on the
disk, and deleting the model in use changes `ask_model`, `meaning_engine`, `meaning_url`,
`meaning_model` or `search_meaning` as the table above says.

## Questions

#### Where are the built-in models kept?
In the `models` folder of the cache folder: `~/.cache/coxswain/models/` on Linux and FreeBSD,
`~/Library/Caches/coxswain/models/` on a Mac, `%LOCALAPPDATA%\coxswain\models\` on Windows.
`coxswain --paths` prints it as *models*. Each model has a folder of its own, named after it and
the first eight characters of the revision it was downloaded at (`qwen3-1.7b-d7f544ee`).
*Settings → Finding files → Details → Built-in models* shows each folder with **Show in panel**
(desktop) or **Enter** on its row (terminal).

#### How do I free the disk space?
**Delete** on a model, or **Delete all not in use**, which also takes unfinished downloads and
older versions. In the terminal app, **Delete** on its row in Settings, or
`coxswain --models delete unused`. The models are only downloaded again when you choose them.

#### How do I free the memory now?
**Unload now** on the chat model (desktop), **U** on its row (terminal). It is let go by itself
five minutes after the last answer anyway. The embedding model stays loaded while search by
meaning is on; turn search by meaning off, or choose a server for it, to free it.

#### What happens when I delete the model in use?
You are asked first. Ask, or search by meaning, then takes the recommended choice: a server's
model on the graphics card when one answers, else another built-in chat model you have, else it
turns off. The line after the delete says which ([the table](#deleting-the-model-in-use)).

#### What is "an older version" or "an unfinished download"?
A new release can pin a newer revision of a model: it is downloaded into a new folder, and the
old folder stays until you delete it. A download that was cancelled or broke off leaves an
unfinished folder (files ending in `.part`). Neither is ever in use, so *Delete all not in use*
takes them.

#### Why is my Ollama model not listed?
Ollama, Lemonade and LM Studio keep their own models; Coxswain does not download or delete
them. Use the server's own list (`ollama list`, `ollama rm qwen3:8b`).

#### Does deleting a model send anything anywhere?
No. Only choosing a built-in model again downloads it, from huggingface.co.

---
[← Previous: Ask without a server](ask-builtin.md) · [Next: The search helper →](helper.md)

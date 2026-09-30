[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Search by meaning

Words find the files that contain them. Search by meaning also finds the files that are *about*
what you type, whatever words they use and in whichever language: "what the rocket's fuel costs"
finds `Brændstofbudget.docx`. A small multilingual model does it on your own machine; it is off
until you turn it on.

<!-- screenshot: search-meaning-hits.png: desktop app, Cyber theme, Find file in Text in files with "rocket fuel cost" typed: word hits first, then hits whose passage starts with "similar to:", one of them a Danish document -->

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [How it works](#how-it-works)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

**Turn it on** (once):

| Where | How |
|---|---|
| Desktop app | *Settings → Search by meaning → Download the model (465 MB) and turn on*. A bar shows *Downloading the model: 120 MB of 465 MB*; **Cancel** stops it |
| Terminal app | `coxswain --meaning on`: prints *Downloading the model for search by meaning: 42%*, then turns it on |
| A link | *turn on search by meaning* under Find file's text depth, and the [notice](notices.md) *New: search by meaning finds files about your words, in any language. Turn it on* |

It needs *Search inside files* on: the button is greyed out otherwise. Then:

1. **Alt+F7** or **Ctrl+F**, and **Tab** twice for *Text in files*.
2. Type a question or a few words, in any language.
3. Files with your words come first; files found by meaning follow.

**Turn it off:** *Turn off* in Settings (or `coxswain --meaning off`) stops it and keeps the model.
**Delete the model** (or `coxswain --meaning delete`) turns it off and deletes the model.
`coxswain --meaning` alone prints `on` or `off`.

## What you see

**In Find file**, a hit found by meaning shows the start of the passage that was close (up to 24
words), marked *similar to:*. Only files close to the best match are shown (within a tenth of its
score, and above what unrelated text scores), so unrelated files stay out. A file found by both its
words and its meaning is shown once, as a word hit.

**In Settings → Search by meaning**, from top to bottom:

- The hint: *Finds files about what you type, not only files with its words, in any language:
  “rocket fuel cost” finds a Danish budget. A small language model (multilingual-e5-small) runs on
  this machine, slowly and never on battery; nothing leaves it. In Find file, Tab to text: such
  files show as “similar to”.*
- *Vectors made by*: *Built-in model, on this CPU (465 MB once)*, *Ollama*, or *A server with the
  OpenAI API (Lemonade, LM Studio, llama.cpp …)*. The last two are on [their own page](servers.md).
- While on: *Understood: 8,120 files · still to go: 23,088*, the model in use
  (`builtin:multilingual-e5-small@614241f6`), the model's folder, any error in red, and on battery
  *Paused while the machine runs on its battery. Index now reads anyway.* Buttons **Turn off** and
  **Delete the model**.
- While off, with the model there: **Turn on** and **Delete the model**. Without it:
  **Download the model (465 MB) and turn on**.

**In the title**: `Coxswain 1.16.0 · search: names · text · meaning` once it runs
([Notices and the window title](notices.md)).

## How it works

- **The model** is [multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small),
  downloaded once from Hugging Face at a pinned version: three files, 488 MB (465 MiB), each checked
  against its SHA-256 before it is used. It is kept in Coxswain's cache folder under
  `models/multilingual-e5-small-614241f6`.
- **It runs on the CPU**, with [candle](https://github.com/huggingface/candle) in pure Rust, on two
  threads, so the machine stays yours. No GPU is needed; for one, see [servers](servers.md).
- **What gets vectors:** the first eight passages of about 120 words of each file with text (the
  start of a document says what it is about). A passage with fewer than 20 letters is skipped.
  Each vector (384 numbers) is packed into bytes and kept in `search.db`.
- **When:** the [helper](helper.md) makes them a few files at a time, resting as long as the work
  took, and not on [battery](battery.md) unless you press *Index now*. New and changed files get
  theirs at the helper's next pass, within ten minutes; their words are searchable at once.
- **A search:** your question gets a vector too. A first sieve over one bit per number keeps the
  400 closest passages; their full vectors are then scored, and the best passage of each file
  counts.

## Settings and config.toml

| Settings item | Key under `[search]` | Type | Default |
|---|---|---|---|
| *Turn on* / *Turn off* | `meaning` | bool | `false` |
| *Vectors made by* | `meaning_engine` | `"builtin"`, `"ollama"` or `"openai"` | `"builtin"` |
| *Server*, *Embedding model*, *API key from the variable* | `meaning_url`, `meaning_model`, `meaning_key_env` | strings | `""` |

The last three are for [servers](servers.md). Search by meaning also needs `text = true`.

## In the terminal app

The same search: hits by meaning come after word hits, with *similar to:* in front of the
passage. Turn it on, off or delete it with `coxswain --meaning on|off|delete`. There is no status
of *still to go*; the desktop app's Settings shows it, or wait for hits. Find file's text depth
says *Also find files about your words, in any language: coxswain --meaning on* while it is off.

## Questions

#### Why is search by meaning off?
It needs a 465 MB model that Coxswain does not ship, and CPU time to read your files with it. You
choose whether that is worth it: turn it on in Settings or with `coxswain --meaning on`.

#### Why does it find nothing yet?
The vectors are made in the background, a few files at a time: Settings shows *still to go*. On a
laptop's CPU that takes hours for a large home folder, and it waits while on battery. A
[server with a GPU](servers.md) is much faster.

#### Why is a document not found by what its last chapter is about?
Only the start of each file gets vectors: eight passages of about 120 words. Search by words still
finds the whole text.

#### Why does it show files that have nothing to do with my question?
It shows the files closest to your question, above a floor. When nothing in your files is about
it, the closest may still pass. Word hits always come first; use more specific words.

#### Where is the model kept and how do I delete it?
In Coxswain's cache folder under `models/` (`coxswain --paths` prints it as `model`; Settings shows
it under the status). **Delete the model** in Settings, or `coxswain --meaning delete`.

#### Does the model send anything anywhere?
No. The download is the only network use; after it, the model runs on your machine and nothing you
have leaves it. See [Privacy](../reference/privacy.md).

#### Can I type the question in English and find Danish files?
Yes. The model is multilingual: a question and a passage about the same thing get close vectors
whatever their languages.

#### Does it slow my machine down?
It works on two threads, in batches with rests, and not on battery. Searching is quick; making the
vectors the first time is the slow part.

#### Why is the button greyed out?
*Search inside files* is off. Search by meaning works on the text the helper keeps, so turn that
on first.

---
[← Previous: Removable disks](removable-disks.md) · [Next: Search by meaning on a server →](servers.md)

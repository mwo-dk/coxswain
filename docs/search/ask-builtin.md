[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask without a server: the built-in chat model

[Ask](ask.md) can answer with a chat model built into Coxswain, so you need no Ollama, LM Studio
or other model server. It is downloaded once, when you choose it, and runs on this machine:
on a Mac with Apple silicon on its GPU (Metal), elsewhere on the processor. Your questions and
your files never leave the machine.

It answers like a server's model: from the passages of your files closest to the question,
citing them as **[1]**, **[2]**, word by word. Everything in [Ask](ask.md) (the keys, follow-ups,
the scope, the sources) works the same.

<!-- screenshot: settings-ask-builtin.png: Settings → Finding files → Details → Ask with "Built-in chat models": Qwen3 1.7B (recommended) with Download (1.0 GB) and use, Qwen3 4B Instruct below it -->

## Contents

- [The models](#the-models)
- [What it needs](#what-it-needs)
- [Turning it on](#turning-it-on)
- [What you see](#what-you-see)
- [How it runs](#how-it-runs)
- [How quick it is](#how-quick-it-is)
- [The estimate for this processor](#the-estimate-for-this-processor)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## The models

| Model | `ask_model` | Download | Memory while loaded | Suggested when |
|---|---|---|---|---|
| Qwen3 1.7B | `builtin:qwen3-1.7b` | 1.0 GB | about 3 GB | A Mac's GPU with less than 16 GB; a processor only when [the estimate](#the-estimate-for-this-processor) has its first word within 10 s |
| Qwen3 4B Instruct (2507) | `builtin:qwen3-4b` | 2.3 GB | about 5 GB | A Mac with Apple silicon and 16 GB or more: better answers, on the GPU |

Both are Qwen3 models by Alibaba's Qwen team, under the Apache 2.0 licence, in 4-bit GGUF form
(`Q4_K_M`). They know over a hundred languages, so a Danish question about English notes is
answered in Danish. The files come from Hugging Face at a fixed revision
([unsloth/Qwen3-1.7B-GGUF](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF),
[unsloth/Qwen3-4B-Instruct-2507-GGUF](https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF),
and the tokenizer from [Qwen/Qwen3-1.7B](https://huggingface.co/Qwen/Qwen3-1.7B)), and each file
is checked against its SHA-256 before it is used; a file that does not match is deleted.

## What it needs

- [Search by meaning](meaning.md) on, with the built-in model or a server. Ask finds its
  passages by meaning, as with any chat model.
- The download, once, from `huggingface.co`.
- The memory in the table, while it answers and for five minutes after.

## Turning it on

| App | How |
|---|---|
| Desktop app, the guide | **Ctrl+,** → **Set up…** → step 4 *Ask: the chat model*: choose *Qwen3 1.7B, built in (1.0 GB download, on the CPU; nothing leaves the machine)* in the list and press **Download Qwen3 1.7B (1.0 GB) and use it**. It is marked **recommended** (and its button is the main one) when no server answers and it suits the machine: on a Mac's GPU always, on a processor only when [the estimate](#the-estimate-for-this-processor) is under 10 s to the first word. Under the list: *On this processor: about 74 s to the first word, then 7.4 words a second (too slow to recommend; a model server answers more quickly)*. When the download is done, Ask is set to it and a test question follows: *It answered: the first word came after 8.6 s.* |
| Desktop app, Settings | **Ctrl+,** → *Finding files* → *Details* → *Ask* → *Built-in chat models*: each model with the estimate for this processor under it, **Download (1.0 GB) and use** next to the model; **Use** once it is there; **Delete the model** frees the disk |
| Terminal app, the guide | `coxswain --setup-search`, step 4: the built-in models come first in the list (`1  Qwen3 1.7B, built in (…)`), each with the estimate on the line below; the last number is *Skip Ask for now*, which turns Ask off. The default (`*`) is never the skip: Ask's model as set, else the server's suggestion, else the built-in model for this machine. Type its number, answer **y** to *Download Qwen3 1.7B (1.0 GB) and use it*; a test question follows |
| Terminal app, Settings | **F9** → *Settings* → *Finding files* → *Ask* (or `coxswain --settings=ask`): under *Chat model*, a line per built-in model with the estimate for this processor (it shows a moment after Settings opens, the first time). **Enter** on an empty *Chat model* offers `builtin:qwen3-1.7b` (on a Mac's GPU the one for its memory) to keep with **Enter** or change |
| Desktop app, Settings, Ask off | *Chat model* shows a server's first chat model, else the built-in one for this machine, with **Use** next to it (downloading a built-in one first) |
| Terminal app, a flag | `coxswain --meaning ask builtin:qwen3-1.7b` downloads it (*Downloading: 42 %*) and sets it; `coxswain --meaning ask off` turns Ask off again; `coxswain --meaning ask delete` deletes the built-in chat models |

The model in *Chat model* can also be typed: `builtin:qwen3-1.7b` or `builtin:qwen3-4b`.

## What you see

- **While it downloads**: Settings shows *Downloading the model: 312 MB of 1.0 GB* with a bar and
  **Cancel**; the guide shows *Downloading: 31 %*. A download that fails says why in red, for
  example *huggingface.co: …* with the cause (no network, a proxy that blocks huggingface.co, a
  full disk); press the button again to try again. Files already done are kept.
- **In Find**: *Qwen3 1.7B, built in answers from the passages of your files closest to the
  question, citing them as [1], [2]. Nothing is kept.* Until the first word: *Waiting for Qwen3
  1.7B, built in to answer: a model that is not loaded yet takes a while…*
- **Not downloaded** (a config set by hand, or the model deleted): the *Ask* row says in red
  *Qwen3 1.7B is not downloaded yet: download it in Settings under Ask, or in Set up…*
- **Settings → Finding files**: the *Ask* line of the status reads *Qwen3 1.7B, built in*, with
  **Try it**. Under *Privacy and updates*, *The built-in chat model, downloaded once* →
  *huggingface.co* is listed until it is downloaded.
- **The guide, step 5**: *The built-in chat model runs on the GPU (Metal) when it can* on a Mac,
  *… on the CPU* elsewhere.

## How it runs

- **In the search helper.** The [search helper](helper.md) loads the model on the first
  question, so every window and every terminal app share one copy in memory. Without a helper
  (it could not start) the app loads it itself.
- **Let go when idle.** Five minutes after the last answer the model is dropped and its memory
  freed; the next question loads it again (a second or two from disk).
- **On a Mac** it runs on the GPU through Metal. If the GPU cannot load it, or fails before the
  first word, the answer comes from the processor instead. `meaning_device = "cpu"` (*Use the CPU
  only* under *Meaning*) keeps it on the processor, as it does the embedding model.
- **Elsewhere** it runs on the processor, with all its cores while it answers. A graphics card
  on Linux or Windows is not used: for that, run Ollama ([servers](servers.md)).
- **No thinking.** Qwen3 1.7B can think before it answers; it is told not to, so the first word
  comes as soon as the prompt is read. *Let the model think first* does not apply.
- **The prompt** has the rules, the closest passages and the turns before: up to 7,000 tokens
  on a GPU (ten passages and a few follow-ups), about 1,000 on a processor (the closest four or
  five passages). When it is too long, the oldest turns go first, then the last passages.
- **Stop** (**Esc**, a new question, closing Find) is heard between parts of the prompt and
  between words.

## How quick it is

Measured on an Intel Core Ultra 9 185H (22 threads, no GPU used), a short question over two
small files:

| Model | First word | Then |
|---|---|---|
| Qwen3 1.7B | 9–13 s | 6–11 words (tokens) a second |
| Qwen3 4B Instruct | about 40 s | 3–4 tokens a second |

With the prompt at the processor's cap (about 1,000 tokens: the rules and five passages) and
the model loaded, Qwen3 1.7B took 65–70 s to its first word on that machine (with other work on it), reading
14–15 tokens a second and writing 8–10. The estimate said 74–76 s. Qwen3 4B Instruct took 271 s; the estimate said 251 s.

On a processor the prompt is read at about the speed the answer is written, so a question with
five passages of your files waits a minute or more for its first word. A Mac's GPU reads the
prompt in one go and writes many times faster. With a graphics card, a model server is quicker
still and can run larger models ([Smart search in a few minutes](setup.md)).

## The estimate for this processor

How quick the built-in model is on a processor depends on that processor far more than on a
Mac's GPU, so Coxswain measures it before it recommends anything. The first time the guide's
step 4 or *Settings → Ask* shows the built-in models on a machine without Metal, it runs a short
probe: the work that takes nearly all of the model's time (a 4-bit `Q4_K` matrix of Qwen3 1.7B's
size times a few rows of a prompt, as candle does it), for 0.2 seconds, with no download. From
that it works out how many tokens a second the model reads and writes, and how long a question
with a full prompt (about 1,000 tokens on a processor) waits for its first word once the model is
loaded:

*On this processor: about 74 s to the first word, then 7.4 words a second (too slow to recommend;
a model server answers more quickly)*

| Where | What you see |
|---|---|
| Desktop app, the guide, step 4 | The line under the list, for the model chosen in it |
| Desktop app, *Settings → Ask → Built-in chat models* | The line under each model |
| Terminal app, `--setup-search`, step 4 | The line under each built-in model in the list |
| Terminal app, *Settings → Finding files → Ask* | A line per built-in model, under *Chat model* |

- **Recommended only when quick.** Under 10 s to the first word, Qwen3 1.7B is marked
  **recommended** when no server answers, as on a Mac. Otherwise it is still chosen in the list
  when no server offers a chat model, so the choice is never empty, but without the badge and
  with the estimate's *(too slow to recommend; a model server answers more quickly)*. **Skip Ask
  for now** (a button in the desktop guide, the last number in the terminal app's) leaves Ask
  off.
- **Words, not tokens.** A token is about three quarters of an English word; the line counts
  words.
- **Kept.** The quickest result so far is kept in `chat-speed.txt` in the data folder
  (`~/.local/share/coxswain/` on Linux and FreeBSD, `%APPDATA%\coxswain\` on Windows), under
  the processor's name, its thread count and the version of Coxswain. Each start of an app
  measures once more, the first time the estimate is shown, and a quicker result replaces the
  kept one: a measurement taken while other programs kept the processor busy is too slow, and
  the quickest is the processor's own. A new processor or a new version starts afresh;
  deleting the file does too.
- **On a Mac's GPU** there is no probe and no line: the GPU reads a prompt in one go, and the
  guide recommends as before. With *Use the CPU only* on, a Mac is measured like any processor.
- **How close it is.** On the Core Ultra 9 185H above it said 74–76 s where the real
  answer took 65–70 s, and 251 s for Qwen3 4B where it took 271 s. It is an estimate: other programs busy on the processor, a laptop
  on battery or a hot processor make the model slower than it says, and the first question waits
  a few seconds more while the model loads.

## Settings and config.toml

| Item (*Settings → Finding files → Details*) | Key | Value | Does |
|---|---|---|---|
| *Ask* → *Chat model* | `[search] ask_model` | `"builtin:qwen3-1.7b"`, `"builtin:qwen3-4b"` | Ask answers with that built-in model |
| *Ask* → *Built-in chat models* | none | | The estimate for this processor, **Download (size) and use**, **Use**, **Delete the model** |
| *Meaning* → *Use the CPU only* | `[search] meaning_device` | `"auto"` / `"cpu"` | On a Mac, `"cpu"` keeps both built-in models off the GPU |

```toml
[search]
meaning = true
ask_model = "builtin:qwen3-1.7b"
```

The models are kept next to the embedding model, in the cache folder's `models` folder:
`~/.cache/coxswain/models/qwen3-1.7b-d7f544ee` on Linux and FreeBSD,
`~/Library/Caches/coxswain/models/…` on a Mac, `%LOCALAPPDATA%\coxswain\models\…` on Windows
([Where things are kept](../reference/where-things-are-kept.md)).

## Questions

#### Is anything sent anywhere?
Only the download, once, from huggingface.co, when you press the button or answer **y**. The
questions and the passages stay on this machine. See [Privacy](../reference/privacy.md).

#### Which of the two should I take?
Qwen3 1.7B on any machine, and always on a processor (if at all: see the question above). On a Mac with Apple silicon and 16 GB or
more, Qwen3 4B Instruct answers better and is quick on the GPU; the guide picks it there, and
Settings marks it *recommended*.

#### Why is the built-in model not recommended on my PC?
On a PC it runs on the processor, and the [estimate](#the-estimate-for-this-processor) for this
one has the first word more than 10 s away: the line under the model in step 4 says how long,
for example *about 74 s to the first word … (too slow to recommend; a model server answers more
quickly)*. You can still choose it: pick it in the list (desktop) or type its number (terminal)
and download it. For quick answers on a PC, run Ollama or Lemonade with a graphics card
([servers](servers.md)); the guide recommends their chat model when one answers.

#### Why is it so slow on my PC?
It runs on the processor there, and the processor reads the prompt about as fast as it writes
the answer. Coxswain gives it fewer passages for that reason. A model server with a graphics
card (Ollama with CUDA, Lemonade on AMD) answers in a fraction of the time; the guide finds one
when it runs.

#### Does it use my graphics card?
On a Mac, yes (Metal). On Linux, Windows and FreeBSD, no: the built-in model runs on the
processor. Ollama or Lemonade use the card.

#### How good are its answers?
Good for finding and quoting what your files say: *what does the fuel cost?* gets *The fuel
costs 40,000 euros per flight [1].* A small model reasons less well than `qwen3:8b` or larger on
a server, and with few passages on a processor it sees less of your files.

#### It says the model is not downloaded. Why?
`ask_model` names a built-in model that is not on disk, set by hand or deleted with **Delete the
model**. Download it in *Settings → Finding files → Details → Ask*, or run
`coxswain --meaning ask builtin:qwen3-1.7b`.

#### How do I free the memory or the disk?
The memory frees itself five minutes after the last answer. **Delete the model** in Settings
frees the disk; in the terminal app `coxswain --meaning ask delete` deletes both built-in chat
models. If it was Ask's model, Ask is turned off.

#### Can I use it with vectors from a server?
Yes. The chat model and the embedding model are chosen apart: vectors from Ollama with answers
from the built-in model work, and the other way round.

---
[← Previous: Ask](ask.md) · [Next: The search helper →](helper.md)

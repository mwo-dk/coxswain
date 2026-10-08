[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask without a server: the built-in chat model

[Ask](ask.md) can answer with a chat model built into Coxswain, so you need no Ollama, LM Studio
or other model server. It is downloaded once, when you choose it, and runs on this machine:
on a Mac with Apple silicon on its GPU (Metal), elsewhere on the processor. Your questions and
your files never leave the machine.

It answers like a server's model: from the passages of your files closest to the question,
citing them as **[1]**, **[2]**, word by word. Everything in [Ask](ask.md) (the keys, follow-ups,
the scope, the sources) works the same.

![Settings → Finding files → Details → Ask on a PC with an RTX 4070 and Ollama: Chat model set to Qwen3 14B · 8.4 GB · about 691 s to the first word, with the red line Qwen3 14B runs on the processor here: about 691 s to the first word. qwen3:8b on Ollama runs on your graphics card (NVIDIA GeForce RTX 4070 Laptop GPU) and answers far sooner, and the Use qwen3:8b button](../screenshots/settings-ask-builtin.png)

## Contents

- [The models](#the-models)
- [What it needs](#what-it-needs)
- [Turning it on](#turning-it-on)
- [What you see](#what-you-see)
- [How it runs](#how-it-runs)
- [When it is a poor choice here](#when-it-is-a-poor-choice-here)
- [On a Mac: a tip when a server has more](#on-a-mac-a-tip-when-a-server-has-more)
- [How quick it is](#how-quick-it-is)
- [The estimate for this processor](#the-estimate-for-this-processor)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## The models

| Model | `ask_model` | Download | Memory while loaded | Suggested when |
|---|---|---|---|---|
| Qwen3 1.7B | `builtin:qwen3-1.7b` | 1.0 GB | about 3 GB | A Mac's GPU with less than 16 GB; a processor only when [the estimate](#the-estimate-for-this-processor) has its first word within 10 s |
| Qwen3 4B Instruct (2507) | `builtin:qwen3-4b` | 2.3 GB | about 5 GB | A Mac with Apple silicon and 16 to 31 GB: better answers, on the GPU |
| Qwen3 14B | `builtin:qwen3-14b` | 8.4 GB | about 12 GB | A Mac with Apple silicon and 32 GB or more: knows more, reasons better, reads twice as much of your files |

Only the models the machine has the memory for are offered (Qwen3 1.7B always): a machine with
8 GB sees one, with 16 GB two, with 32 GB or more all three. The *Memory* column is the
machine's memory the guide and Settings go by.

All three are Qwen3 models by Alibaba's Qwen team, under the Apache 2.0 licence, in 4-bit GGUF form
(`Q4_K_M`). They know over a hundred languages, so a Danish question about English notes is
answered in Danish. The files come from Hugging Face at a fixed revision
([unsloth/Qwen3-1.7B-GGUF](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF),
[unsloth/Qwen3-4B-Instruct-2507-GGUF](https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF),
[unsloth/Qwen3-14B-GGUF](https://huggingface.co/unsloth/Qwen3-14B-GGUF),
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
| Desktop app, Settings | **Ctrl+,** → *Finding files* → *Details* → *Ask* → *Chat model*: one list with the server's chat models under *On Ollama at localhost:11434 · on the graphics card (…)* and the built-in ones under *Built in · on the CPU*, each with its size and, on a processor, *about 74 s to the first word*; one not downloaded yet says *downloaded from huggingface.co when chosen*. Picking it downloads it (*Downloading the model: 312 MB of 1.0 GB*, **Cancel**) and makes it Ask's. The recommended one says *recommended*, one that is slow here *slow here*. Deleting is under [Built-in models](models.md) |
| Terminal app, the guide | `coxswain --setup-search`, step 4: the built-in models come first in the list (`1  Qwen3 1.7B, built in (…)`), each with the estimate on the line below; the last number is *Skip Ask for now*, which turns Ask off. The default (`*`) is never the skip: Ask's model as set, else the server's suggestion, else the built-in model for this machine. Type its number, answer **y** to *Download Qwen3 1.7B (1.0 GB) and use it*; a test question follows |
| Terminal app, Settings | **F9** → *Settings* → *Finding files* → *Ask* (or `coxswain --settings=ask_model`): **Enter** or **Space** on *Chat model* opens one list: the server's models under its heading, the built-in ones under *Built in · on the CPU* with their size and estimate (it shows a moment after Settings opens, the first time), then *Another model…* (type a name) and *Off*. **↑ ↓** and **Enter** take one; a built-in one not downloaded yet is downloaded on the plain terminal first (`coxswain --meaning ask builtin:…`). With Ask off the cursor starts on the recommended one |
| Desktop app, Settings, Ask off | *Chat model* shows *Off*, with **Use qwen3:8b** (or the built-in one for this machine) under it: the recommended one, a server's on the graphics card first, downloading a built-in one first |
| Terminal app, a flag | `coxswain --meaning ask builtin:qwen3-1.7b` downloads it (*Downloading: 42 %*) and sets it; `coxswain --meaning ask off` turns Ask off again; `coxswain --meaning ask delete` deletes the built-in chat models |

The model in *Chat model* can also be typed: `builtin:qwen3-1.7b`, `builtin:qwen3-4b` or
`builtin:qwen3-14b`.

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
- **When it is slow here**: a red line in *Settings → Finding files* (on the *Ask* line of the
  status) and under *Ask*, and a row under Find's Ask row ([below](#when-it-is-a-poor-choice-here)).
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
  on Linux or Windows is not used: for that, run Ollama ([servers](servers.md)). When one runs a
  model on the card, Coxswain recommends it and says so wherever the built-in one is chosen
  ([below](#when-it-is-a-poor-choice-here)).
- **Thinking, when asked, on a Mac's GPU.** Qwen3 1.7B and 14B can think before they answer.
  They are told not to, so the first word comes as soon as the prompt is read, unless *Let the
  model think first* is ticked (`ask_think = true`): then, on a Mac's GPU, they think first,
  unseen, and up to 4,096 tokens are kept for the thinking and the answer (1,024 without). On a
  processor they never think: it would take minutes. Qwen3 4B Instruct cannot think.
- **The prompt** has the rules, the sources and the turns before: up to 7,000 tokens on a GPU
  (about 2,500 words of excerpts and a few follow-ups), 15,000 with Qwen3 14B (twice that), about
  1,000 on a processor (the closest three or four passages), so the first word does not take
  minutes. Thinking takes 3,000 more of them for itself. *Context for a server's
  model* does not change it. When it is too long, the oldest turns go first, then the last
  sources. See [How much it reads](ask.md#how-much-it-reads).
- **Stop** (**Esc**, a new question, closing Find) is heard between parts of the prompt and
  between words.

## When it is a poor choice here

A built-in model on the processor is a poor choice when something far quicker is at hand, and
Coxswain says so for as long as the choice stands, not once:

| The choice | Said when | The better one |
|---|---|---|
| A built-in chat model for Ask | It runs on the processor (not a Mac's GPU) and a server Ask can use runs its models on the graphics card: Ollama here with the built-in vectors, else the server that makes the vectors | That server's chat model: the one named like the size that suits the card (`qwen3:8b` with 8–15 GB), else a Qwen, else its first |
| A built-in chat model for Ask, no such server | Its [estimate](#the-estimate-for-this-processor) has the first word 10 s or more away (Qwen3 14B or 4B on a PC) | The largest smaller one that is quick here, if one is; and a model server on a graphics card |
| The built-in embedding model for search by meaning | It runs on the processor and a server here runs an embedding model on the graphics card (only Ask's server, when Ask uses a server's model) | That server's embedding model (`bge-m3` first); every file's meaning is read again, said and asked first |

*On the graphics card* is known when the server has a model loaded there (Ollama's `/api/ps`
with memory on the card, Lemonade's devices); with nothing loaded yet, a machine with a graphics
card or an NPU counts it as one. A server whose loaded model sits on the processor never
counts. On a Mac's GPU (Metal) the built-in models are a good choice: nothing is said in red,
only a tip when a server there has more ([below](#on-a-mac-a-tip-when-a-server-has-more)).

What you see, with Ask set to Qwen3 14B on a PC with an RTX 4070 and Ollama's `qwen3:8b`:

| Where | What |
|---|---|
| *Settings → Finding files*, the status | Under *Ask: Qwen3 14B, built in*, in red: *Qwen3 14B runs on the processor here: about 110 s to the first word. qwen3:8b on Ollama runs on your graphics card (NVIDIA GeForce RTX 4070 Laptop GPU) and answers far sooner.* and **Use qwen3:8b** (desktop: a link; terminal: **Enter** on the *Ask* line, whose bottom line shows the text) |
| *Settings → Ask* | The same line in red under *Chat model*, with the **Use qwen3:8b** button (terminal: a row *! …* with *[Use qwen3:8b]*, **Enter** takes it). For the vectors, the line is under *Meaning* |
| Find | Under the Ask row: *Qwen3 14B is slow here, on the processor* and **Use qwen3:8b** (terminal: *· Enter: Use qwen3:8b*). Without a quicker model the step is **Settings**, which opens at *Ask* |
| What's new | Once: the notice with the same text, **Show me** opening *Settings → Ask* (terminal: the status line) |
| The guide, step 4 | The server's model is preselected and *recommended*; the built-in ones say *slow here* |

**Use qwen3:8b** saves `ask_model = "qwen3:8b"` at once, and the line goes. When no model of
the server is quicker, or the smaller built-in model is not downloaded, there is no button: the
line names what to do (*Qwen3 1.7B answers sooner here (about 9 s); a model server on a graphics
card sooner still.*), and the list under *Chat model* downloads it.

The servers are asked in the background, at most every two minutes, the first time Settings or
Find needs it: only the servers on this machine, nothing else.

## On a Mac: a tip when a server has more

On a Mac the built-in models run on the GPU through Metal, and so do Ollama, LM Studio (with
its MLX models) and llama.cpp. A server is no quicker by being a server there, but it can run
models the built-in ones cannot, such as `qwen3:30b-a3b`, a mixture of experts. So on a Mac
Coxswain still compares, and says a **tip**, not a warning:

| The choice | Said when | The tip |
|---|---|---|
| A built-in chat model for Ask | A server Ask can use (Ollama here with the built-in vectors, else the server that makes the vectors) has a chat model with more weights than the built-in one | *qwen3:30b-a3b on Ollama runs on Metal too, larger and quicker than Qwen3 14B.* for a mixture of experts whose words go through fewer weights than the built-in one's; *qwen3:32b on Ollama runs on Metal too, and is larger than Qwen3 14B: better answers.* for a larger dense one |
| The built-in embedding model for search by meaning | A server here has a stronger embedding model: `bge-m3`, `qwen3-embedding` or `snowflake-arctic-embed2` | *bge-m3:latest on Ollama runs on Metal too, and finds by meaning better than the built-in model.* Every file's meaning is read again, said and asked first |

The size is the one the server gives (Ollama's `parameter_size`, such as *30.5B*), else the one in
the model's name (`Qwen3-30B-A3B-MLX-4bit`: 30 billion, 3 billion a word), else, for the model the
hardware advice names (`qwen3:14b` with 32 GB or more), the advice's. Of several larger ones the
tip names a mixture of experts first, else the smallest larger one. A server whose models are
the same size or smaller (`qwen3:8b` against Qwen3 14B, `nomic-embed-text`) says nothing, and
with no server the built-in models stay the recommendation, as before.

| Where | What |
|---|---|
| *Settings → Finding files*, the status | Under *Ask: Qwen3 14B, built in*, in grey (not red): the tip and **Use qwen3:30b-a3b** (desktop: a link; terminal: **Enter** on the *Ask* line) |
| *Settings → Ask* | The tip in grey under *Chat model*, with the **Use qwen3:30b-a3b** button (terminal: a row *· …* with *[Use qwen3:30b-a3b]*, not red, **Enter** takes it). For the vectors, the tip is under *Meaning* |
| Find | Under the Ask row: the tip and **Use qwen3:30b-a3b** (terminal: *· Enter: Use qwen3:30b-a3b*) |
| What's new | Once: the notice with the tip, **Show me** opening *Settings → Ask* (terminal: the status line) |
| The guide, step 4 | The server's chat models are listed first (desktop: the server's group above *Built in*; terminal: the numbers before the built-in ones), and the one the tip names is preselected and *recommended*. The built-in ones are never *slow here* on a Mac |

**Use qwen3:30b-a3b** saves `ask_model = "qwen3:30b-a3b"` at once, and the tip goes. Keep the
built-in model and the tip stays on its line, plain, as long as the server has the larger model.

## How quick it is

Measured on an Intel Core Ultra 9 185H (22 threads, no GPU used), a short question over two
small files:

| Model | First word | Then |
|---|---|---|
| Qwen3 1.7B | 9–13 s | 6–11 words (tokens) a second |
| Qwen3 4B Instruct | about 40 s | 3–4 tokens a second |
| Qwen3 14B | about 110 s | 1.5 tokens a second |

With the prompt at the processor's cap (about 1,000 tokens: the rules and five passages) and
the model loaded, Qwen3 1.7B took 65–70 s to its first word on that machine (with other work on it), reading
14–15 tokens a second and writing 8–10. The estimate said 74–76 s. Qwen3 4B Instruct took 271 s; the estimate said 251 s. Qwen3 14B took 561 s, reading 1.8 tokens a
second and writing 1.4; the estimate said 578 s. On a processor Qwen3 14B proves that it works,
not that it is worth the wait: it is meant for a Mac's GPU.

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
| Desktop app, *Settings → Ask → Chat model* | In each built-in model's entry of the list: *about 74 s to the first word* |
| Terminal app, `--setup-search`, step 4 | The line under each built-in model in the list |
| Terminal app, *Settings → Finding files → Ask* | In each built-in model's entry of the *Chat model* list |

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
  answer took 65–70 s, 251 s for Qwen3 4B where it took 271 s, and 578 s for Qwen3 14B where it took 561 s. It is an estimate: other programs busy on the processor, a laptop
  on battery or a hot processor make the model slower than it says, and the first question waits
  a few seconds more while the model loads.

## Settings and config.toml

| Item (*Settings → Finding files → Details*) | Key | Value | Does |
|---|---|---|---|
| *Ask* → *Chat model* | `[search] ask_model` | `"builtin:qwen3-1.7b"`, `"builtin:qwen3-4b"`, `"builtin:qwen3-14b"` | Ask answers with that built-in model |
| *Ask* → *Chat model*, the list | none | | The server's chat models and the built-in ones that fit, each with where it runs, its size and the estimate for this processor; a pick sets `ask_model`, downloading a built-in one first |
| *Built-in models* | none | | The models on the disk, to unload or delete ([Built-in models](models.md)) |
| *Ask* → *Let the model think first* | `[search] ask_think` | bool, `false` | Qwen3 1.7B and 14B think first, on a Mac's GPU |
| *Meaning* → *Use the CPU only* | `[search] meaning_device` | `"auto"` / `"cpu"` | On a Mac, `"cpu"` keeps the built-in models off the GPU (and stops them thinking) |

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

#### Which one should I take?
Qwen3 1.7B on any machine, and always on a processor (if at all: see the question below). On a
Mac with Apple silicon and 16 GB or more, Qwen3 4B Instruct answers better and is quick on the
GPU; with 32 GB or more, Qwen3 14B knows more, follows code and long documents better and reads
twice as much of your files, at about a third of the 4B's speed. The guide picks the largest the
Mac has the memory for, and Settings marks it *recommended*. You can take a smaller one there
(**Use** or **Download (size) and use** next to it), or type its `builtin:` name in *Chat model*.

#### Should I let it think?
For a question that needs reasoning (*why does this function fail on an empty list?*), yes:
tick *Let the model think first* under *Settings → Finding files → Details → Ask* (desktop), or
**Space** on it in the terminal app's *Settings → Finding files → Ask*. Qwen3 14B and 1.7B then
think first on a Mac's GPU; *Waiting for Qwen3 14B, built in to answer…* stays while they do,
often 20 to 60 seconds, and the thinking is not shown. For finding and quoting, leave it off.

#### Why not a larger model, such as Qwen3 30B-A3B?
Qwen3 30B-A3B is a mixture of experts: it knows as much as a 30B model but works as fast as a
3B one, and would suit a Mac with 48 GB or more. candle, the library Coxswain runs the models
with, runs its experts on NVIDIA's CUDA only, not on a Mac's GPU or a processor, so it is not
offered. With a model server (Ollama, LM Studio) it runs today: `qwen3:30b` ([servers](servers.md)),
and once the server has it, Settings and Find say so in a grey tip with **Use qwen3:30b-a3b**
([On a Mac](#on-a-mac-a-tip-when-a-server-has-more)).

#### Why is Qwen3 14B not offered on my machine?
It needs 32 GB of memory: 8.4 GB for the model, the rest for the 16,000 tokens it reads and
for macOS and your apps. Below that, only the models that fit are listed. On a PC it is
offered with 32 GB or more, but on a processor it is slow (see [How quick it is](#how-quick-it-is)).

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
when it runs, and Settings says in red which one is quicker, with a **Use** button
([When it is a poor choice here](#when-it-is-a-poor-choice-here)).

#### Settings says my model "runs on the processor here" in red. Why?
The built-in model you chose runs on the processor while a server on this machine runs a model
on your graphics card, or it is too large to answer within 10 s here. The line names the quicker
one; **Use qwen3:8b** (desktop) or **Enter** on the line (terminal) takes it. It stays as long as
the choice does: choose another model, or keep this one and read past it.

#### Why does Coxswain choose Ollama over the built-in model?
When Ollama (or Lemonade, LM Studio, llama.cpp) runs a chat model on your graphics card, it
answers in seconds where a built-in model on the processor takes a minute or more. So the guide's
step 4, *Settings → Ask* with Ask off, and deleting the built-in model in use all take the
server's model first. On a Mac the built-in models run on the GPU and stay a good first choice
when no server answers; when one does, its models are listed first, and a larger one is a tip.

#### On my Mac, Settings shows a grey line about Ollama or LM Studio. What is it?
A tip, not a fault: the server runs on Metal too, and has a chat model larger than the built-in
one you chose (*qwen3:30b-a3b on Ollama runs on Metal too, larger and quicker than Qwen3 14B.*).
**Use qwen3:30b-a3b** (desktop) or **Enter** on the line (terminal) takes it; or keep yours and
read past it. It shows under *Ask* in Settings, under Find's Ask row, and once in *What's new*.

#### Why does the tip say "quicker" for a larger model?
`qwen3:30b-a3b` is a mixture of experts: it holds 30 billion weights but each word goes through
3 billion, fewer than Qwen3 14B's 14. It knows more and writes sooner. A larger dense model
(`qwen3:32b`) is only *larger*, and the tip says *better answers*, not quicker.

#### Why is there no tip with my server's qwen3:8b?
It is smaller than the built-in Qwen3 14B you chose, and both run on the Mac's GPU: the built-in
one is the better choice. The tip shows only for a model with more weights than yours, by the
size the server gives, else the one in its name.

#### I use LM Studio's MLX models with the built-in vectors. Why no tip for Ask?
With the built-in model making the vectors, Ask uses Ollama here, not LM Studio. A stronger
embedding model in LM Studio (a `qwen3-embedding` one) shows a tip under *Meaning*; once LM
Studio makes the vectors, its larger chat models show one under *Ask*.

#### Does it use my graphics card?
On a Mac, yes (Metal). On Linux, Windows and FreeBSD, no: the built-in model runs on the
processor. Ollama or Lemonade use the card, and Coxswain recommends them when they answer.

#### How good are its answers?
Good for finding and quoting what your files say: *what does the fuel cost?* gets *The fuel
costs 40,000 euros per flight [1].* A small model reasons less well than `qwen3:8b` or larger on
a server, and with few passages on a processor it sees less of your files. Qwen3 14B on a Mac
with 32 GB or more is as large as the models people run on a server with 16 GB of graphics
memory, and reasons better still when it may think first.

#### It says the model is not downloaded. Why?
`ask_model` names a built-in model that is not on disk, set by hand or deleted. Pick it in the
list under *Settings → Finding files → Details → Ask → Chat model*, which downloads it, or run
`coxswain --meaning ask builtin:qwen3-1.7b`.

#### How do I free the memory or the disk?
The memory frees itself five minutes after the last answer, or at once with **Unload now**
under *Settings → Finding files → Details → Built-in models* (**U** on its row in the terminal
app). **Delete** there frees the disk; `coxswain --models delete unused` deletes all that are not
in use. Deleting Ask's model gives Ask the recommended one ([Built-in models](models.md)).

#### Can I use it with vectors from a server?
Yes. The chat model and the embedding model are chosen apart: vectors from Ollama with answers
from the built-in model work, and the other way round.

---
[← Previous: Ask about code](ask-code.md) · [Next: Built-in models →](models.md)

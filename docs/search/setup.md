[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Smart search in a few minutes

Coxswain finds files four ways: by **name**, by the **words** inside them, by what they are
**about**, and by answering a **question** about them. The first works from the start; the
other three need a little setting up. The guided setup does it with you, in both apps. It finds
the model servers on your machine, says what suits your hardware, and asks before it downloads
anything.

![The desktop app's Set up smart search window in Cyber: search inside files on, This machine: NVIDIA GeForce RTX 4070 Laptop GPU (8 GB), Ollama recommended, bge-m3:latest for the vectors, qwen3:8b for Ask with It answered: the first word came after 31.7 s., and The server runs the model on the graphics card.](../screenshots/setup-guide.png)
*The guide after a walk through it: Ollama found and chosen, both models set, the test question answered on the graphics card.*

## Contents

- [What each part does](#what-each-part-does)
- [What to use on your machine](#what-to-use-on-your-machine)
- [Starting the guide](#starting-the-guide)
- [The steps](#the-steps)
- [Servers one by one](#servers-one-by-one)
- [Is it using the graphics card?](#is-it-using-the-graphics-card)
- [Keep reading in the background](#keep-reading-in-the-background)
- [Where your data goes](#where-your-data-goes)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## What each part does

| Part | What you get | What it needs |
|---|---|---|
| Names | Every file and folder on the machine by name, in milliseconds | Nothing; always on |
| [Text in files](text.md) | Files whose text has your words: PDFs, Word, mail, notes, code | *Words inside files* (on by default). Coxswain reads your files in the background and keeps the text in a store on this machine |
| [Meaning](meaning.md) | Files *about* what you type, whatever words they use, in any language. *a dessert with apples* finds `apple-cake.md`, and a Danish question finds an English report | A model that turns every passage into a **vector** (numbers for what it means): the built-in one, or a [model server](servers.md) |
| [Ask](ask.md) | An answer to a question, written from the closest passages of your files, with numbered sources | Meaning, and a **chat model** on a model server |

A model server is a program on your machine (or one you run elsewhere) that runs AI models:
Ollama, Lemonade, LM Studio, llama.cpp's server, Jan, LocalAI or vLLM. Coxswain treats them all
the same. With a graphics card or an NPU, a server makes vectors many times faster than the
built-in model, which runs on the processor (or, on a Mac with Apple Silicon, on its GPU; the guide says which).

## What to use on your machine

The guide detects the hardware and recommends one of these. It says why in one line.

| Your machine | Server | Vectors | Ask | Why |
|---|---|---|---|---|
| **NVIDIA**, 8 GB (e.g. RTX 4070) | Ollama with CUDA | `bge-m3` | `qwen3:8b` | CUDA is Ollama's quickest path; both models fit in 8 GB together |
| NVIDIA, 16 GB or more | Ollama with CUDA | `bge-m3` | `qwen3:14b` | More memory, a better model for answers |
| NVIDIA, 6 GB or less | Ollama with CUDA | `bge-m3` | `qwen3:4b` | A small model still fits next to the vectors |
| **AMD Ryzen AI** (NPU, Radeon 780M/890M) or a **Radeon** card | Lemonade | its embedding model (`nomic-embed-text-v1-GGUF`) | `Qwen3-8B-GGUF`, or a Hybrid/NPU model | Lemonade runs models on AMD's NPU and Radeon graphics, which Ollama mostly does not |
| AMD with less than 32 GB and no NPU | Lemonade | as above | `Qwen3-4B-GGUF` | The Radeon in a Ryzen shares the system's memory |
| **Apple silicon**, 16 GB | Ollama or LM Studio | `bge-m3` | `qwen3:8b` | Both use the GPU through Metal |
| Apple silicon, 32 GB or more | Ollama or LM Studio | `bge-m3` | `qwen3:14b` | Half the shared memory can go to the model |
| **Processor only**, or no server | none | the built-in model (465 MB to download) | `qwen3:4b` on Ollama, slowly, or none | The built-in model needs nothing installed; a chat model on a processor takes seconds per sentence |

When a server is already running, the guide recommends that one if it suits the machine. If it
does not, it recommends the running server with models for both vectors and answers. Every server
it finds is listed and you can pick any of them.

## Starting the guide

| App | How |
|---|---|
| Desktop app | **Ctrl+,** → **Set up…** in *Overview* or *Finding files*; or in *Finding files* a level that needs a model, or the **Set up…** button on the *Meaning* or *Ask* line of the status. Or the *set it up* link that Find file shows when Ask or meaning is not set up (**Ctrl+F7** opens Find file at Ask) |
| Terminal app | `coxswain --setup-search` in a terminal. It asks step by step: a number chooses, **Enter** takes the default (marked `*`), **s** skips, and a yes/no question wants **y** |

Find file's hints say where to start. In the desktop app: *Ask answers questions from your files.
It needs search by meaning, and a chat model on your server: set it up*. In the terminal app:
*Set it up step by step: coxswain --setup-search*.

## The steps

Each step says what it does and how it stands now, and you can skip any of them. In the desktop
app all steps are on one page; the terminal app asks them in order.

1. **Search inside files.** On or off. **Turn it on** starts reading; the count of files read so
   far follows.
2. **Where the vectors come from.** First the machine: *This machine: NVIDIA GeForce RTX 4070
   Laptop GPU (8 GB), 63 GB of memory.*, with the recommendation's one line under it. Then every
   server that answered, for example *Ollama at http://localhost:11434: 2 models for vectors, 2
   for answers*, with **recommended** on the one that suits. *The built-in model* is listed last.
   For a server on another machine, type its address (`http://evo:13305/api/v1`) and press
   **Look**; a server elsewhere is marked in bold: *The text of your files is sent to … to get its
   vectors.* **Look again** asks once more, after you have started a server.
3. **The model for the vectors.** Only models that make vectors are listed. Chat models are left
   out. The suggestion is `bge-m3` (or the server's `nomic-embed-text`). When it is not on the
   server, **Download bge-m3** fetches it (Ollama and Lemonade), with a percentage. **Use** saves
   it. When files already have vectors from another model, Coxswain first says *Changing the
   embedding model re-reads the meaning of 3437 files (about 2 hours on this machine)*, with
   **Change and re-read** and **Keep the current model**. With the built-in model:
   **Download the built-in model (465 MB)**, or **Use the built-in model** once it is there.
4. **Ask: the chat model.** Only models that can answer are listed. Embedding models such as
   `bge-m3` are never offered. The suggestion follows the table above; **Download** fetches it.
   **Use and ask a test question** saves it and asks it about a made-up file: *It answered: the
   first word came after 2.4 s.* The first answer takes longest while the model loads.
5. **Speed.** After the test question, Coxswain asks the server where the model runs. *The server
   runs the model on the graphics card*, or what to do when it runs on the processor (next section).
6. **Keep reading in the background.** *Start with my session* (below).
7. **What is set.** One line each for search inside files, search by meaning (the model and
   server), Ask and *Start with my session*. **Done** closes the guide; the search helper starts
   again with the new settings.

Nothing is downloaded until you press a download button or answer **y**.

## Servers one by one

### Ollama

[ollama.com](https://ollama.com). It listens on `http://localhost:11434`. The guide reads its
list (`/api/tags`) and each model's capabilities (`/api/show`: `embedding`, `completion`), and
downloads with `/api/pull`.

- Recommended models: `bge-m3` for vectors (568 M parameters, 100+ languages), `qwen3:8b` for Ask
  (`qwen3:4b` with less memory, `qwen3:14b` with 16 GB or more). By hand: `ollama pull bge-m3`.
- NVIDIA: Ollama uses CUDA when its CUDA build is installed. On Arch Linux that is
  `sudo pacman -S ollama-cuda` (AMD: `ollama-rocm`), then `systemctl restart ollama`. Other
  systems: [Ollama's GPU guide](https://docs.ollama.com/gpu).
- Check: `ollama ps` shows *100% GPU* in the PROCESSOR column while a model is loaded.

### Lemonade

[lemonade-server.ai](https://lemonade-server.ai), for AMD Ryzen AI and Radeon. It listens on
`http://localhost:13305/api/v1` (older versions on port 8000); the guide tries both. Its model
list labels each model (`embeddings`, `reranking`, the rest chat), and `/system-info` tells
which NPU and Radeon it can use; the guide reads both. Downloads go through `/api/v1/pull`.

- Recommended: an embedding model from its list (`nomic-embed-text-v1-GGUF`) and `Qwen3-8B-GGUF`
  for Ask. On a Ryzen AI machine, the Hybrid (NPU + GPU) and NPU models in Lemonade's model
  manager use the NPU.
- When `/api/v1/health` shows a model on `cpu` while there is a GPU or NPU, the guide says so:
  choose a Hybrid or NPU model, or a GGUF model with the Vulkan or ROCm backend.

### LM Studio

[lmstudio.ai](https://lmstudio.ai). Turn on its server (*Developer* → *Start server*); it
listens on `http://localhost:1234/v1`. Its own API (`/api/v0/models`) says whether a model is
`llm`, `vlm` or `embeddings`, and the guide lists them by that.

- LM Studio downloads models in its own window. When a suggested model is missing, the guide
  says: *LM Studio downloads models in its own window: find bge-m3 there, download it, then look
  again here.* Search for `text-embedding-bge-m3` or `nomic-embed-text` under *Discover*.
- Speed: LM Studio does not say where a model runs. Set *GPU offload* to the maximum in the
  model's settings.

### llama.cpp, Jan, LocalAI, vLLM and others

llama.cpp's `llama-server` listens on `http://localhost:8080/v1` (LocalAI uses the same port;
the guide tells them apart by llama.cpp's `/props`), Jan on `http://localhost:1337/v1`. These do
not say what a model can do, so the guide sends each model one word both ways: to
`/embeddings` (does it make vectors?) and to `/chat/completions` (does it answer?). Start
`llama-server` with `--embedding` for a model that makes vectors. vLLM and other servers on
other ports: type the address under *A server on another machine*.

## Is it using the graphics card?

A model on the processor answers ten or more times slower. The guide checks after the test
question:

| Server | How the guide knows | How you check by hand |
|---|---|---|
| Ollama | `/api/ps`: `size_vram` is 0 for a model on the processor | `ollama ps`: *100% GPU* |
| Lemonade | `/api/v1/health`: the loaded model's `device` | Lemonade's status page |
| LM Studio | It does not say; the guide reminds you of *GPU offload* | The model's settings in LM Studio |
| NVIDIA in general | | `nvidia-smi` shows the server's process and its memory |

The graphics card is found with `nvidia-smi` (NVIDIA), the kernel's `/sys/class/drm` and
`/dev/accel` (AMD on Linux), Lemonade's `/system-info` (AMD on any system) and the processor
type (Apple silicon).

## Keep reading in the background

Coxswain reads your files and makes their vectors only while an app runs, and for ten minutes
after. With **Start with my session**, the [search helper](helper.md) starts when you log in,
so the store follows your files between launches and the first pass is done while no window is
open. It uses a little processor time when files change and pauses on battery
([Battery](battery.md)). No administrator rights are needed.

| System | What is registered | Check that it runs | Turn it off |
|---|---|---|---|
| Linux | A systemd user service, `coxswain-index.service` | `systemctl --user status coxswain-index` | Untick it, or `coxswain --index-service off` |
| FreeBSD | An XDG autostart entry, `~/.config/autostart/coxswain-index.desktop` ([other ways](../reference/freebsd.md#the-search-helper)) | `pgrep -lf index-helper` | Untick it, or `coxswain --index-service off` |
| macOS | A LaunchAgent, `~/Library/LaunchAgents/dk.mwo.coxswain.index.plist` | `launchctl list \| grep coxswain` | The same |
| Windows | A *Run* entry for your user in the registry | Task Manager → *Startup apps* lists Coxswain | The same, or disable it in Task Manager |

Desktop app: the guide's step 6, or *Settings → Finding files → Details → Background
reading → Start with my session*. Terminal app: the guide, or `coxswain --index-service on|off`.

## Where your data goes

Nothing leaves the machine unless you choose a server elsewhere. The guide only asks the
servers on `localhost` what they have. A server on another machine gets your search words and
the text of your files (for vectors), and your questions and the passages they are answered
from (for Ask). Settings and the guide say so in bold, with the server's address. See
[Privacy](../reference/privacy.md).

## Settings and config.toml

The guide writes the same keys as Settings:

| Step | `config.toml` |
|---|---|
| 1 | `[search] text = true` |
| 2–3 | `meaning = true`, `meaning_engine = "builtin" \| "ollama" \| "openai"`, `meaning_url`, `meaning_model` |
| 4 | `ask_model` |
| 6 | (the system's registration, not a key) |

```toml
[search]
text = true
meaning = true
meaning_engine = "openai"
meaning_url = "http://localhost:13305/api/v1"
meaning_model = "nomic-embed-text-v1-GGUF"
ask_model = "Qwen3-8B-GGUF"
```

**Desktop app and terminal app:** the steps, the checks and the advice are the same (the logic
lives in `coxswain-core`). The desktop app shows all steps on one page; the terminal app asks
them in order on the command line.

## Questions

#### The guide finds no server, but Ollama is running. Why?
The guide asks `localhost` on the usual ports and waits less than a second for each. An Ollama
started with `OLLAMA_HOST` on another port, or in a container without the port published, is
not found: type its address under *A server on another machine*, e.g. `http://localhost:11500`.

#### Why does it not offer my model for Ask?
Only models that can answer are listed. On Ollama a model whose capabilities lack `completion`
(an embedding model such as `bge-m3`) is left out. On llama.cpp, Jan and LocalAI, a model that
did not answer the one-word test is left out. Load it in the server, then press **Look again**.

#### Which is better, the built-in model or a server?
On a processor alone, the built-in model: it needs nothing installed. With a graphics card or an
NPU, a server: `bge-m3` finds more and is many times quicker. The [search quality
numbers](../reference/performance.md#search-quality) compare them.

#### It says the model runs on the processor. What do I do?
Do what the line says for your server: on Arch Linux `sudo pacman -S ollama-cuda` (or
`ollama-rocm`) and `systemctl restart ollama`; elsewhere [Ollama's GPU
guide](https://docs.ollama.com/gpu); in Lemonade a Hybrid or NPU model; in LM Studio *GPU
offload*. Then run the guide again and ask the test question.

#### Does changing the model start over?
Only a real change of the vectors' model does. You are told first how many files are read again
and about how long. `bge-m3` and `bge-m3:latest` are the same model. See
[Servers](servers.md#why-does-switching-the-model-start-over).

#### Can I run the guide again later?
Yes, as often as you like. It shows what is set now, and a step you skip keeps its setting.

#### My Lemonade is on port 8000. Is that a problem?
No. The guide tries 13305 and 8000. Settings takes whatever address you give it.

---
[← Previous: Search](README.md) · [Next: Find file →](find-file.md)

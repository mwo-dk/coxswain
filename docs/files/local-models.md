[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Your local models, by name

Ollama keeps its models as files named `sha256-…`, and the Hugging Face cache keeps folders named
`models--org--name`. Coxswain shows the model each one is, beside its name, in the panels and in
[Space](../panels/space.md), so you can see which gigabytes are `llama3.2:3b`. Put the cursor on a
model file and its facts say what it holds: architecture, parameters, quantisation, context
length, licence, its tensors by layer, the memory it needs to load, and whether that fits this
machine. Nothing is renamed, nothing is sent anywhere, and only a file's header is read.

![The desktop app in Ollama's blobs folder: each sha256 file has its model name in front of it, such as llama3.2:3b, qwen3:0.6b · chat template and llama3.2:1b, llama3.2:3b · licence; the preview pane shows the facts of llama3.2:3b's weights: Model llama3.2:3b, Format GGUF v3, Architecture llama, Parameters 3.2 billion, Quantisation Q4_K_M, Context length 131072 tokens, Layers 28 × blk: 2.8 billion, Memory to load about 2.4 GB, with 4096 tokens of context, On this machine fits in memory (32 GB); the status line says Alt+Enter shows what this model holds and whether it fits this machine](../screenshots/gui-local-models.png)
*Ollama's blobs, by model, and the facts of one.*

## Contents

- [How to use it](#how-to-use-it)
- [Where the names come from](#where-the-names-come-from)
- [The facts](#the-facts)
- [Memory, and whether it fits](#memory-and-whether-it-fits)
- [PyTorch pickles are not opened](#pytorch-pickles-are-not-opened)
- [In Space](#in-space)
- [In the terminal app](#in-the-terminal-app)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to use it

1. Go to a model store: Ollama's `~/.ollama/models/blobs` (or `blobs` under `OLLAMA_MODELS`),
   the Hugging Face cache `~/.cache/huggingface/hub` (or where `HF_HUB_CACHE` or `HF_HOME`
   puts it), or LM Studio's `~/.lmstudio/models`.
2. Each blob shows the model it is in front of its own name: `llama3.2:3b [sha256-dde5…]` in the
   terminal app, `llama3.2:3b sha256-dde5…` (the hash dimmed) in the desktop app. A Hugging Face
   folder shows `Qwen/Qwen3-0.6B`.
3. Put the cursor on a model file. The desktop app's preview pane shows its facts under the
   preview; **Alt+Enter** (Properties) shows them in both apps. The first few times, the status
   line says so: *Alt+Enter shows what this model holds and whether it fits this machine*.
4. **Alt+U** (Space) on the store shows where the bytes go, named by model.

| | Desktop app | Terminal app |
|---|---|---|
| Names | Beside each name, in every view | Beside each name |
| Facts | Preview pane, and Properties (**Alt+Enter**) | Properties (**Alt+Enter**) |
| Space | Rectangles and *Largest files* named by model | The same |

## Where the names come from

| Store | Used by | What you see |
|---|---|---|
| Ollama, `blobs/` next to `manifests/` | Ollama | `llama3.2:3b` for the weights; `llama3.2:3b · chat template`, `· licence`, `· parameters`, `· system prompt`, `· vision projector`, `· configuration` for the other parts. A blob several models share names them all: `llama3.2:1b, llama3.2:3b · licence` |
| Hugging Face cache, `hub/` | Lemonade, llama.cpp, vLLM, `transformers` | `models--Qwen--Qwen3-0.6B` as `Qwen/Qwen3-0.6B` (datasets and spaces too) |
| A Hugging Face repository's `blobs/` | the same | Each blob as `Qwen/Qwen3-0.6B · model.safetensors`: the file in a snapshot that links to it |
| LM Studio, `~/.lmstudio/models` | LM Studio | Its names are plain already: only the facts are added |

Ollama's names come from its manifests, `manifests/<registry>/<namespace>/<model>/<tag>`: a model
from Ollama's own library is `llama3.2:3b`, one from another namespace `user/model:tag`, one from
another registry `hf.co/org/repo:Q4_K_M`, as `ollama list` shows them. The store is found by its
shape, `blobs/` beside `manifests/`, so a system-wide store (`/usr/share/ollama/.ollama/models`)
or one moved with `OLLAMA_MODELS` is named too. The names are read again a few seconds after
the last time, so a model pulled meanwhile shows when you come back to the folder.

In a Hugging Face snapshot (`snapshots/<revision>/`) the files have their real names already;
each one is a link to a blob, and the facts are those of the file behind the link.

## The facts

| Fact | GGUF | safetensors |
|---|---|---|
| Model | The name from the store, when it is in one | The same |
| Format | `GGUF v3` | `safetensors`, or *safetensors, in shards* for a `model.safetensors.index.json` |
| Name, architecture | `general.name`, `general.architecture` | The header's `name`, when it has one |
| Shards | | How many, or *2 of 4 here* while a download is not done |
| Parameters | The tensors' elements added up, in billions or millions | The same |
| Quantisation | `general.file_type`: `Q4_K_M`, `Q8_0`, `F16` … | |
| Context length | `<architecture>.context_length`, in tokens | |
| Licence | `general.license` | |
| Chat template | Folded to one line: how many lines it has and how it starts | |
| Tensors, Types | How many tensors, and the bytes of each type (`Q4_K 1.6 GB, Q6_K 308 MB`) | How many, and the bytes of each dtype (`BF16 1.2 GB`) |
| Layers | Numbered tensors by layer, `28 × blk: 3.0 billion`, then the rest by name, `token_embd: 394 million`, the largest first | `28 × model.layers: …`, `model.embed_tokens: …` |
| Memory to load, On this machine | [Below](#memory-and-whether-it-fits) | [Below](#memory-and-whether-it-fits) |

A sharded model's `model.safetensors.index.json` lists its shards; when all of them are here,
their headers are read too and the facts are those of the whole model.

Only the header is read: a GGUF's keys and tensor list (up to 256 MB of header, a few
megabytes in practice), a safetensors file's JSON header (up to the format's own 100 MB). Every
length, count and offset in it is checked before it is used, and a header that claims more than
the file holds, or more than these limits, is not read further. ONNX files show no facts yet.

## Memory, and whether it fits

*Memory to load* is the weights (the tensors' bytes) and, for a GGUF, the cache of keys and
values for 4,096 tokens of context, what Ollama and llama.cpp start with: *about 2.4 GB, with 4096
tokens of context*. A longer context takes more.

*On this machine* compares that with the graphics card and the memory, found the same way as
for [the setup guide's model choice](../search/setup.md#what-to-use-on-your-machine):

| It says | When |
|---|---|
| *fits on the graphics card (NVIDIA GeForce RTX 4070, 12 GB)* | It fits in the card's own memory |
| *fits in memory (32 GB), not on the graphics card (…, 8 GB)* | Too big for the card, it runs on the processor, slower |
| *fits in memory (32 GB)* | No card with memory of its own (Apple silicon shares its memory) |
| *more than this machine's memory (16 GB)* | It will not load, or only very slowly |

## PyTorch pickles are not opened

A `.pt`, `.pth`, `.ckpt` or PyTorch `.bin` (`pytorch_model.bin`, or a `.bin` in a Hugging Face
snapshot) is a Python pickle, and reading one can run code. Coxswain names it,
*PyTorch pickle — not opened (opening one can run code)*, and never parses or loads it. Prefer
the safetensors or GGUF file of the same model.

## In Space

[Space](../panels/space.md) (**Alt+U**) on `~/.ollama/models` or the Hugging Face cache
labels each rectangle by its model, and *Largest files* lists `llama3.2:3b` (the desktop app's
tooltip has the blob's path). The line under the map names the model and the path, in both apps.

![Space in the desktop app on Ollama's blobs folder: three rectangles labelled llama3.2:3b 2.0 GB, llama3.2:1b 798 MB and qwen3:0.6b 387 MB; Largest files lists them by model, then the chat templates, configurations, parameters and the licence; the line under the map reads llama3.2:3b · ~/.ollama/models/blobs/sha256-925b… · 2.0 GB · Other](../screenshots/gui-local-models-space.png)
*Space on Ollama's store: the gigabytes, by model.*

## In the terminal app

The names and the facts are the same: the panel shows `llama3.2:3b [sha256-…]`, and
**Alt+Enter** opens Properties with the facts under the file's own (a later line of the
tensor list has no label of its own). The terminal app has no preview pane.

![The terminal app with Ollama's blobs in the left panel, each named by its model (qwen3:0.6b · chat template [sha256-0…], llama3.2:3b [sha256-925b2ca…]) and the Hugging Face cache in the right one (bartowski/SmolLM2-360M-Instruct-GGUF and the others); Properties is open on llama3.2:3b's weights with the same facts as the desktop app, from Model llama3.2:3b to On this machine fits in memory (32 GB)](../screenshots/tui-local-models.png)
*Properties (**Alt+Enter**) on a blob, and the Hugging Face cache by name in the other panel.*

## Settings and config.toml

None: the names and facts are always on, and nothing leaves the machine.

## Questions

#### Why does a sha256 file show a model's name?

In Ollama's `blobs` folder Coxswain reads Ollama's manifests, which say which blob is which
model, and shows the name in front of the file's own. Nothing is renamed: the file keeps its
name on the disk, and copying it (**F5**) copies the blob as it is.

#### Where do I see what a model holds?

Put the cursor on it: the desktop app's preview pane shows its facts, and **Alt+Enter**
(Properties) shows them in both apps.

#### Why is my .pt or .bin file not opened?

A PyTorch pickle can run code when it is read, so Coxswain names it and never opens it. A GGUF
or safetensors file holds only data, and only its header is read.

#### Does the memory include the context?

For a GGUF, yes: 4,096 tokens of it. A model run with a longer context needs more; one run with
a shorter one less. For safetensors only the weights are counted.

#### My graphics card is not named. Why?

The card is found by `nvidia-smi`, by the kernel's list of devices on Linux, and by `pciconf` on
FreeBSD. Without a card it found, *On this machine* compares with the memory alone.

#### Why are some blobs not named?

A blob no manifest names is left over from a model removed or a download not finished; Ollama
removes such blobs itself when it starts. A Hugging Face blob no snapshot links to is the same
kind of leftover.

#### Can I find a model by its name?

Not yet: Find (**Alt+F7**, **Ctrl+F**) searches the files' own names. Open the store's folder and the names
are there.

---
[← Previous: Container images](container-images.md) · [Next: Properties and permissions →](properties.md)

[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask: questions answered from your files

Ask is part of [Find](find-file.md). Type a question in your own words, such as *how does
our Entra sign-in flow work?* or *what did we decide about the fuel budget?*, and press
**Ctrl+Enter** (desktop app) or **Alt+Enter** (terminal app), or **Enter** on the *Ask* row at
the top of the list.
Coxswain finds the passages of your files closest in meaning to the question, from
anywhere in a document: the whole text has vectors, not only its start
([what is covered](meaning.md#how-it-works)). It gives the chat model as much as the model can
take: each passage with the ones before and after it, so it reads as a whole, and more from the
files that match best. A question about the folder as a whole (*what is this repository's code
about?*) gets its README, project files and folder tree too ([How much it reads](#how-much-it-reads)).
The chat model on your own server, or the one [built into Coxswain](ask-builtin.md), then writes
a short answer from only those sources, and cites them as **[1]**, **[2]**. The sources are
listed under the answer, numbered the same way, under a line that says how much was read, and
**Enter** on one takes you to the file.

The answer opens in place of the list; **Esc** goes back to the list. Nothing is kept: the
questions and answers exist only while Find is open, and closing it forgets them.

<picture><source media="(prefers-reduced-motion: reduce)" srcset="../screenshots/ask.png"><img src="../screenshots/ask.gif" alt="Ctrl+F opens Find, rocket fuel cost is typed and Ctrl+Enter asks; after a wait the answer appears in place, Rocket fuel costs 2,105 kEUR in April and 2,655 kEUR in June [1], with the numbered sources budget.txt and budget-da.txt under it and the line Enter ask, or go to the source"></picture>

*`qwen3:8b` on Ollama on the same machine; the wait for the first word is cut short.*

<!-- screenshot: ask.png: Ask's answer with the dim line "N excerpts from M files, about … words" over the numbered sources (and ask.gif retaken the same way) -->

## Contents

- [What it needs](#what-it-needs)
- [How to use it](#how-to-use-it)
- [How much it reads](#how-much-it-reads)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Thinking](#thinking)
- [Questions](#questions)

## What it needs

1. [Search by meaning](meaning.md) turned on, with the built-in model or [a server](servers.md).
   Ask looks for the passages the same way Find picks the files *About this*.
2. A **chat model**: the [built-in one](ask-builtin.md) (Qwen3, downloaded once, no server; on a
   Mac it runs on the GPU), or one on a server you run:
   - With the vectors from Ollama or an OpenAI-style server (Lemonade, LM Studio, llama.cpp,
     vLLM), the chat model is on **that same server**, with the same address and API key.
   - With the built-in model, the chat model is on **Ollama on this machine**
     (`http://localhost:11434`).

   For example `qwen3:8b`, `llama3.1:8b` or `gemma3:12b` on Ollama (`ollama pull qwen3:8b`), or
   the name Lemonade or LM Studio lists. A model that reasons aloud (`<think> … </think>`) is
   fine: only its answer is shown. Ask asks it not to think first, so the answer starts at once
   (see [Thinking](#thinking)).

Ask never uses a service on the internet unless you point the server address at one yourself.

## How to use it

The guided setup does all of it, with a test question: [Smart search in a few minutes](setup.md)
(**Set up…** in *Settings → Finding files* or *Overview*, or the level *Names, text, meaning
and Ask* there; or `coxswain --setup-search`). By hand:

1. **Set the chat model** once. Desktop app: **Ctrl+,** → *Finding files* → *Details* →
   *Ask* → *Chat model*: one list with the server's models under *On Ollama at localhost:11434 ·
   on the graphics card (…)* and the built-in ones under *Built in · on the CPU*, then *Another
   model…* and *Off*. Only models that can answer are listed: on Ollama, models that only make
   vectors, such as `bge-m3`, are left out. A pick sets it at once. Terminal app: **Enter** on
   *Chat model* in *Settings → Finding files → Ask* opens the same list, or
   `coxswain --meaning ask qwen3:8b`. On Ollama a missing model is pulled first. A model that
   cannot answer is refused with the reason (below), in both apps.
2. Open Find (**Ctrl+F**) and type the question. The first row reads **? Ask: "your
   question"** once there are two words or more, or a `?` at the end (a question ending in `?`
   puts the cursor on it). Press **Enter** on it, or **Ctrl+Enter** (desktop) / **Alt+Enter**
   (terminal) from any row. **Ctrl+F7** (the F9 command list calls it *Ask your files*) opens
   Find straight at the *Ask* kind, where **Enter** asks; inside Find it asks what is typed. A
   `?` typed first does the same: `? what does it cost`.
3. The list gives way to the answer, and the *Ask* kind is lit. The sources come first, then the
   answer, word by word. Until the first word comes, the answer says *Waiting for qwen3:8b to
   answer: a model that is not loaded yet takes a while…*, with whatever server it is on.
4. Ask a **follow-up** the same way: *and in Danish?*, *who wrote that?*. The questions and
   answers before it go along, and the passages are looked up with the question before it too,
   so short follow-ups work.
5. **Up** / **Down** move through the sources of the last answer. **Enter** with the field empty
   goes to that file: the active panel opens its folder with the cursor on it. In the desktop
   app a click on a source or on a **[n]** in the answer does the same. **F4** edits the source,
   and **F3** views it in the terminal app.
6. **Esc** goes back to the list (the conversation stays until Find closes); **Esc** there closes
   Find and forgets the questions. An answer still being written stops when Find closes,
   also while the server is still loading the model and has not said a word.

**Only one folder:** switch Find's scope to the active panel's folder with **Ctrl+F** inside Find
(*In rocket*): Ask then takes its passages from the files in that folder and below.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Ctrl+Enter** / **Alt+Enter** | **Ctrl+Enter**: ask what is typed, from any row | **Alt+Enter** (**Ctrl+Enter** where the terminal reports it) |
| **Ctrl+F7** | Open Find at *Ask*; inside Find, ask what is typed | The same |
| **Enter** on the *Ask* row, or with a question typed in the answer | Ask it | The same |
| **Enter** with the field empty, in the answer | Go to the source under the cursor | The same |
| **Up** / **Down** | Move through the last answer's sources | The same |
| **Ctrl+F** / **Alt+F7** | Switch the scope: everywhere or the panel's folder | The same |
| **F3** | Nothing: the preview pane is under Find | View the source |
| **F4** | Edit the source | The same |
| **Esc** | Back to the list; again: close and forget | The same |
| Mouse | Click a source or a **[n]** to go to it | None |

## How much it reads

Ask fills the chat model's context, not a fixed number of passages. What fits is the context
less the rules, the questions and answers before, the question and 1,024 tokens kept for the
answer (4,096 when the model thinks first, [Thinking](#thinking)); a token is counted as 3 bytes of text, on the safe side (English is nearer 4).

| Chat model | Context | Sources, about |
|---|---|---|
| On a server (Ollama, Lemonade, LM Studio, llama.cpp, vLLM) | `ask_context`, 8,192 tokens by default | 20 KB: 2,500–3,000 words |
| Built in, on a Mac's GPU | 8,192 tokens; Qwen3 14B 16,384 | 20 KB; 45 KB with Qwen3 14B |
| Built in, on the processor | 2,048 tokens | 2.5 KB: the closest three or four passages |

The processor keeps the small one because candle reads a prompt at 10 to 40 tokens a second
there: a full context would mean minutes before the first word.

**Excerpts, not loose passages.** The 48 passages closest to the question (four of a file at
most, from 16 files at most) are grouped by file, the file with the closest passage first. Each
hit comes with the passage before it and the one after it in the same file, when they fit; the
first file gives up to four hits, the second three, the third two, the rest one. Neighbouring
passages are joined into one excerpt without the 20 words two passages share where a paragraph
was cut, and a file's excerpts stay in their order, with `[…]` between them. Each file is one
source. Text that is the same in two files (a copy) is given once.

**A question about the whole folder.** When the question asks what the folder, the project, the
repository or the code is or is about (*what is this repository's code about?*, *explain this
project*, *give me an overview*, *hvad handler denne mappe om?*, *worum geht es in diesem
Projekt?*), or how it is built (*what is the architecture?*, *which libraries does it use?*,
*which crates?*), Ask first reads an overview of Find's scope or, with the scope on everywhere, of the
active panel's folder, in up to three fifths of the room:

1. the README at its top (`README.md`, `README`, `readme.txt`, …), up to half of that;
2. the tree of its folders, up to a quarter, two levels deep, 40 entries a folder, without hidden folders, the
   folders in *Left out everywhere* (`text_exclude`: `node_modules`, `target`, `build`, `dist`, …)
   and the plain names in its `.gitignore`;
3. for a project (a folder with `.git` or a project file), its map: each project file with what
   it uses, each workflow, each file of code with what it says it is and its public items, in up
   to half of the room, the README and the tree getting less then ([Ask about code](ask-code.md#the-map-of-a-project));
4. its project files up to three levels deep, up to an eighth each: `Cargo.toml`, `package.json`, `pyproject.toml`,
   `go.mod`, `pom.xml`, `build.gradle`, `*.csproj`, `CMakeLists.txt`, `flake.nix` and others;
5. its docs index: `docs/README.md` or `docs/index.md`.

A file that would get less than 200 bytes is left out, so a small context (the built-in model on
a processor) gets the start of the README and the tree rather than scraps of everything.

The closest excerpts fill the rest. Each of these is a source of its own and can be cited; the
tree's source is the folder itself, and **Enter** on it opens it. When nothing in the files is
close to the question but it names the folder or the code (*how is the code laid out?*), the
overview alone answers.

The words are recognised in the app's languages. A question they miss gets the closest excerpts
alone, as before.

## What you see

- **Not set up yet**, the *Ask* row says what is missing: *Ask your files a question · needs
  search by meaning first*, or *· choose a chat model*, with *Set up* (desktop app: a link that
  opens the [setup guide](setup.md); terminal app: **Enter** runs `coxswain --setup-search`).
  **Delete** (or **×**) sends the row away for good; **Ctrl+Enter** / **Ctrl+F7** still show what
  Ask needs.
- **A model that cannot answer**: when the chat model only makes vectors, the *Ask* row and the
  answer's place say so (in red in the answer), for example *bge-m3 only reads meaning and cannot answer: choose
  a chat model, e.g. qwen3:8b.*, with the link *set it up* (desktop app), and **Enter** does not
  send the question. A model the server does not have shows the server's message, such as
  *model 'qwen3' not found*. Settings shows the same line in red under *Chat model*.
- **Ready**, before the first question: *qwen3:8b answers from the passages of your files closest
  to the question, citing them as [1], [2]. Nothing is kept.*
- **Each question** is in bold (desktop app) or after `›` in the search colour (terminal app),
  and its answer is under it. While it is being written, a blinking **▍** ends the answer.
- **Citations** **[1]**, **[2]** are underlined links in the desktop app; hover one for the
  file's path.
- **How much was read**, over the sources, dim: *12 excerpts from 10 files, about 2600 words*.
  A question about code says so first: *Code question: 9 excerpts from 6 files, about 2000 words*
  ([A question about code](ask-code.md#a-question-about-code)).
- **The sources** are a numbered list under each answer: the file's name and its folder. The
  cursor marks the one **Enter** goes to.
- **Errors** show in red under the question: the server's own message, such as *model "qwen3:8b"
  not found*, or *Nothing in your files is close to the question.*
- **The footer**: *Enter ask, or go to the source · ↑↓ sources · F4 edit · Esc back to the list*
  (the terminal app adds *F3 view*).

## Settings and config.toml

| Item (*Settings → Finding files → Details*) | Key | Type, default | Does |
|---|---|---|---|
| *Chat model* | `[search] ask_model` | string, `""` | The model that writes the answers: a server's (`qwen3:8b`) or a built-in one (`builtin:qwen3-1.7b`, `builtin:qwen3-4b`, `builtin:qwen3-14b`, [Ask without a server](ask-builtin.md)). Empty: Ask is not set up |
| *Chat model*, the list | none | | Every model Ask can take, by where it runs, the recommended one and those *slow here* marked; a built-in one is downloaded when picked. Deleting is under [Built-in models](models.md) |
| *Model for code questions* | `[search] ask_code_model` | string, `""` | The chat model for questions about code, on the same server; empty (*Same as Ask*): Ask's own ([A model for code questions](ask-code.md#a-model-for-code-questions)) |
| *Let the model think first* | `[search] ask_think` | bool, `false` | Off: a model that thinks first (Qwen3, DeepSeek-R1, …) is asked not to. On: it thinks, many seconds before the first word ([Thinking](#thinking)) |
| *Context for a server's model (tokens)* | `[search] ask_context` | number, `8192` (2,048 to 131,072) | The tokens a server's chat model is given, sources, question and answer together. Ollama is asked for this context (`num_ctx`); an OpenAI-style server must be set to at least as much. Not for the built-in models ([How much it reads](#how-much-it-reads)) |
| *Meaning* → *Made by*, *Server*, *API key from the environment variable* | `meaning_engine`, `meaning_url`, `meaning_key_env` | | The server Ask talks to, as above |
| (none) | `[keys] ask` | list of keys, `["Ctrl+F7"]` | The keys that open Find at Ask ([Changing keys](../customise/keys.md)) |

```toml
[search]
meaning = true
ask_model = "qwen3:8b"
```

## In the terminal app

Ask is the same: type in Find and **Alt+Enter** (or **Ctrl+F7** for `ask: `, type, **Enter**). The answer wraps in the window and scrolls
so its end stays in sight. Set it up with:

```sh
coxswain --meaning ollama            # or: --meaning on, --meaning server URL MODEL
coxswain --meaning ask qwen3:8b      # pulled on Ollama when missing
coxswain --meaning ask off           # Ask off again
```

## Thinking

Models such as Qwen3 think before they answer. Coxswain never shows the thinking, so the user
would wait for nothing: with `qwen3:8b` on an RTX 4070 laptop GPU the first word came after
15–28 seconds. Ask therefore asks the model not to think:

| Server | How |
|---|---|
| Ollama (0.9 or newer) | `"think": false` with the question; a model that cannot think is not affected, an older Ollama ignores it |
| OpenAI-style (Lemonade, LM Studio, llama.cpp, vLLM) | ` /no_think` at the end of the question, for Qwen3's hybrid models only (not its *coder* or *instruct* models, which do not think) |

With it the first word comes in 0.3–0.4 seconds once the model is loaded.

**The context.** On Ollama, Ask also asks for a context of `ask_context` tokens (`num_ctx`,
8,192 by default), and fills it with the sources ([How much it reads](#how-much-it-reads)). Recent Ollama versions give
`qwen3:8b` 32,768 by default, and the cache for that pushed the model partly off an 8 GB graphics
card and the embedding model (`bge-m3`) out of it, so every question loaded both again: about 7
seconds before the first word. With 8,192 both stay loaded. Reading a full context takes a moment: with
`qwen3:8b` loaded on that card the first word came after 0.6–1.2 seconds, where ten passages took
0.5–1.0. The chat model is loaded with the same context
while the sources are looked up, so it is not loaded twice. A model that always
thinks (DeepSeek-R1, Qwen3's *thinking* models) cannot be stopped; *Waiting for … to answer*
stays until it has. To let the model think, for harder questions: tick *Let the model think
first* under *Settings → Finding files → Details → Ask* in the desktop app, or set `ask_think = true` under
`[search]` in `config.toml` (both apps). The built-in Qwen3 1.7B and 14B think too then, on a
Mac's GPU ([Ask without a server](ask-builtin.md#how-it-runs)).

## Questions

#### What is sent, and where?
With the [built-in chat model](ask-builtin.md): nothing; it answers on this machine. With a server's: the question, the questions and answers before it in this Find, and the excerpts with
their file paths (for a question about the folder as a whole, also its README, project files,
folder tree and docs index), as much as `ask_context` holds, to the chat model on the server set under *Settings → Finding files → Details → Meaning* (Ollama on this machine
with the built-in model). Nothing else, and nothing to anyone else. With a server on another
machine, Settings says so in bold, as for the vectors. See [Privacy](../reference/privacy.md).

#### How much of my files does Ask read?
As much as the chat model's context holds: with a server's model and the default 8,192 tokens,
about 2,500–3,000 words, from up to about a dozen files; with the built-in model on a processor,
the closest three or four passages. The line over the sources says what it was, for example
*12 excerpts from 10 files, about 2600 words*. Each hit comes with the passages before and
after it, and the files that match best give more. See [How much it reads](#how-much-it-reads).

#### Why does a question about the whole project read the README?
A broad question (*what is this repository's code about?*, *give me an overview*) has no
passage that answers it: the closest passages are scattered fragments. Ask reads the folder's
README, its project files (`Cargo.toml`, `package.json`, …), its folder tree and its docs index
first, so the model sees what the project is. They are listed and numbered with the other
sources. The folder is Find's scope (*In rocket*) or, with the scope on everywhere, the active
panel's folder: open the project's top folder before you ask.

#### Can Ask read more?
With a model on a server, yes: raise *Context for a server's model* under *Settings → Finding
files → Details → Ask*, or `ask_context` under `[search]` in `config.toml` (both apps; the
terminal app's Settings has it under *Ask*). 16,384 reads about twice as much. Each token holds
memory on the server: on an 8 GB graphics card `qwen3:8b` with more than 8,192 pushes
`bge-m3` out, and every question loads both again. The built-in models keep their own sizes.

#### My OpenAI-style server says the prompt is too long. Why?
Coxswain can ask Ollama for a context but not Lemonade, LM Studio, llama.cpp or vLLM: they use
the context the model was loaded with, and LM Studio's default is 4,096 tokens. Load the model
there with a context of at least `ask_context` (8,192), or set `ask_context` to what the server
has.

#### Does Ask see the whole of a long document?
Yes, since 1.39.0: every passage of a file has a vector, up to 256 of them (about 25,000 words), so
the passage that answers can come from the last chapter. Each passage is matched with the file's
name, its folder and its Markdown heading in front of it, so *what does the rocket plan say about
the launch window?* finds the section under *Launch window* in `rocket/plan.md`. A file longer
than that has its start, end, section starts and passages evenly between. Right after the update,
the vectors are being [renewed](meaning.md#why-is-search-by-meaning-re-reading-everything); a file
waiting for its new ones gives no passages yet.

#### Why does it say the sources do not hold the answer?
The model is told to answer from the sources only and to say so when they do not answer the
question. Ask with other words, or check with Find (**Shift+F7**, *In files*) that the file is read: folders
outside *Folders read*, *Names only* folders and files still waiting for their vectors are not
searched.

#### Can it answer from one folder only?
Yes. Switch Find's scope with **Ctrl+F** inside Find (*In rocket*, `[in rocket]` in the terminal
app) before you ask: the passages then come from that folder and below. With nothing close
there, it says *Nothing in the files in rocket is close to the question.*

#### Are the answers kept, for next time?
No. They live in the Find window only. Closing it forgets them, and nothing is written to
disk.

#### Can Ask work without Ollama or any server?
Yes: choose the [built-in chat model](ask-builtin.md) in step 4 of the guide, or in the
*Chat model* list in Settings, or run `coxswain --meaning ask builtin:qwen3-1.7b`. It is downloaded
once (1.0 GB) and runs on a Mac's GPU, or slowly on the processor elsewhere.

#### How do I switch Ask to another model?
Pick it in *Chat model*: **Ctrl+,** → *Finding files* → *Details* → *Ask* (terminal app:
**Enter** on *Chat model*, **↑ ↓**, **Enter**). The list holds the server's chat models (Ollama
here when search by meaning uses the built-in model, else the server that makes the vectors)
and the built-in ones, under where each runs; a pick saves `ask_model` at once. A server that
does not answer shows *Ollama does not answer at localhost:11434* in its group, with **Set up a
server…**. Before 2.15.0 the field was a text box whose suggestions only matched the name
already in it, so a built-in model there hid Ollama's.

#### Which chat model should I pick?
When a server runs a model on your graphics card, that one: Coxswain recommends it, and says
in red when a built-in model on the processor is chosen instead. With no server, the built-in Qwen3 1.7B, or Qwen3 4B Instruct on a Mac with 16 GB or more. On a server, one that fits your GPU's memory: `qwen3:8b` or `llama3.1:8b` with 8 GB, `gemma3:12b` or
`qwen3:14b` with 12–16 GB. Models of 3–4 B parameters answer on a CPU too, slowly. A
multilingual model answers questions in other languages; Ask tells it to answer in the
question's language.

#### The first answer takes long. Why?
The server loads the model into memory on the first question; that can take half a minute. On
Ollama, Ask has the model loaded while it looks the sources up, so the wait starts before the
question is sent. A server with the OpenAI API (Lemonade, LM Studio, llama.cpp) is sent nothing
before the question: it loads the model when the question comes. Either way *Waiting for … to
answer* shows under the question until the first word. Ask waits up to five minutes for the answer to start, and **Esc** stops the
wait at any moment. After that the words come as they are made. A model that thinks first
would add many seconds more; Ask asks it not to ([Thinking](#thinking)).

<a id="it-says-my-model-makes-vectors-and-cannot-answer-why"></a>

#### It says my model only reads meaning and cannot answer. Why?
The model in *Chat model* is an embedding model, such as `bge-m3` or `nomic-embed-text`: it turns
text into vectors for search by meaning and cannot write an answer. Pick a chat model instead,
such as `qwen3:8b`. On Ollama, Coxswain asks the server what each model can do (`/api/show`)
when Find opens at *Ask* and when Settings opens, and the *Chat model* list leaves embedding
models out. A server with the OpenAI API does not say: its whole list is offered, and the model
you choose is sent a one-word question when you save it (Settings, or `coxswain --meaning ask`);
its error, if any, shows under *Chat model*.

#### Why does a follow-up sometimes find other sources?
Each question looks up its own passages, together with the question before it. The numbers
under each answer are that answer's sources.

---
[← Previous: Search by meaning on a server](servers.md) · [Next: Ask about code →](ask-code.md)

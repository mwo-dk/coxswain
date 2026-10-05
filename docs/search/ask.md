[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask: questions answered from your files

Ask is part of [Find](find-file.md). Type a question in your own words, such as *how does
our Entra sign-in flow work?* or *what did we decide about the fuel budget?*, and press
**Ctrl+Enter** (desktop app) or **Alt+Enter** (terminal app), or **Enter** on the *Ask* row at
the top of the list.
Coxswain finds the ten passages of your files closest in meaning to the question, from
anywhere in a document: the whole text has vectors, not only its start
([what is covered](meaning.md#how-it-works)). The chat
model on your own server then writes a short answer from only those passages, and cites them
as **[1]**, **[2]**. The sources are listed under the answer, numbered the same way, and
**Enter** on one takes you to the file.

The answer opens in place of the list; **Esc** goes back to the list. Nothing is kept: the
questions and answers exist only while Find is open, and closing it forgets them.

<!-- screenshot: search-ask.png: the desktop app (Cyber), Find with the answer in place: the question, the answer citing [1] and [2], the numbered sources budget.txt and budget-da.txt -->

## Contents

- [What it needs](#what-it-needs)
- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Thinking](#thinking)
- [Questions](#questions)

## What it needs

1. [Search by meaning](meaning.md) turned on, with the built-in model or [a server](servers.md).
   Ask looks for the passages the same way Find picks the files *About this*.
2. A **chat model** on a server you run:
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
   *Ask* → *Chat model* (the list offers the models on the server that can answer: on Ollama, models
   that only make vectors, such as `bge-m3`, are left out). Terminal app:
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
- **The sources** are a numbered list under each answer: the file's name and its folder. The
  cursor marks the one **Enter** goes to.
- **Errors** show in red under the question: the server's own message, such as *model "qwen3:8b"
  not found*, or *Nothing in your files is close to the question.*
- **The footer**: *Enter ask, or go to the source · ↑↓ sources · F4 edit · Esc back to the list*
  (the terminal app adds *F3 view*).

## Settings and config.toml

| Item (*Settings → Finding files → Details*) | Key | Type, default | Does |
|---|---|---|---|
| *Chat model* | `[search] ask_model` | string, `""` | The model that writes the answers. Empty: Ask is not set up |
| *Let the model think first* | `[search] ask_think` | bool, `false` | Off: a model that thinks first (Qwen3, DeepSeek-R1, …) is asked not to. On: it thinks, many seconds before the first word ([Thinking](#thinking)) |
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

**The context.** On Ollama, Ask also asks for a context of 8,192 tokens (`num_ctx`): ten
passages, the rules and a few turns before fit with room to spare. Recent Ollama versions give
`qwen3:8b` 32,768 by default, and the cache for that pushed the model partly off an 8 GB graphics
card and the embedding model (`bge-m3`) out of it, so every question loaded both again: about 7
seconds before the first word. With 8,192 both stay loaded, and in the terminal app the first
word of a follow-up came after 0.06–0.3 seconds. The chat model is loaded with the same context
while the sources are looked up, so it is not loaded twice. A model that always
thinks (DeepSeek-R1, Qwen3's *thinking* models) cannot be stopped; *Waiting for … to answer*
stays until it has. To let the model think, for harder questions: tick *Let the model think
first* under *Settings → Finding files → Details → Ask* in the desktop app, or set `ask_think = true` under
`[search]` in `config.toml` (both apps).

## Questions

#### What is sent, and where?
The question, the questions and answers before it in this Find, and the ten passages with
their file paths, to the chat model on the server set under *Settings → Finding files → Details → Meaning* (Ollama on this machine
with the built-in model). Nothing else, and nothing to anyone else. With a server on another
machine, Settings says so in bold, as for the vectors. See [Privacy](../reference/privacy.md).

#### Why only ten passages?
They are the closest to the question, at most three from one file, so a long document does not
crowd out the rest. Ten passages of up to 120 words fit in the context of small local models and
keep the answer quick.

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

#### Which chat model should I pick?
One that fits your GPU's memory: `qwen3:8b` or `llama3.1:8b` with 8 GB, `gemma3:12b` or
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
[← Previous: Search by meaning on a server](servers.md) · [Next: The search helper →](helper.md)

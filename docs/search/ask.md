[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Ask: questions answered from your files

Ask is the fourth depth of Find file. Type a question in your own words, such as *how does our
Entra sign-in flow work?* or *what did we decide about the fuel budget?*, and press **Enter**.
Coxswain finds the ten passages of your files closest in meaning to the question. The chat
model on your own server then writes a short answer from only those passages, and cites them
as **[1]**, **[2]**. The sources are listed under the answer, numbered the same way, and
**Enter** on one takes you to the file.

Nothing is kept. The questions and answers exist only while Find file is open; **Esc** forgets
them.

![The desktop app's Find file on Ask: the question what does the rocket fuel cost? in bold, the answer The rocket fuel costs 2,105 kEUR in April and 2,655 kEUR in June [1]., and the numbered sources budget.txt and budget-da.txt, the second under the cursor](../screenshots/search-ask.png)

## Contents

- [What it needs](#what-it-needs)
- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## What it needs

1. [Search by meaning](meaning.md) turned on, with the built-in model or [a server](servers.md).
   Ask looks for the passages the same way *Text in files* finds files *similar to:* your words.
2. A **chat model** on a server you run:
   - With the vectors from Ollama or an OpenAI-style server (Lemonade, LM Studio, llama.cpp,
     vLLM), the chat model is on **that same server**, with the same address and API key.
   - With the built-in model, the chat model is on **Ollama on this machine**
     (`http://localhost:11434`).

   For example `qwen3:8b`, `llama3.1:8b` or `gemma3:12b` on Ollama (`ollama pull qwen3:8b`), or
   the name Lemonade or LM Studio lists. A model that reasons aloud (`<think> … </think>`) is
   fine: only its answer is shown.

Ask never uses a service on the internet unless you point the server address at one yourself.

## How to use it

The guided setup does all of it, with a test question: [Smart search in a few minutes](setup.md)
(**Set up…** at the top of *Settings → Search by meaning*, or `coxswain --setup-search`). By hand:

1. **Set the chat model** once. Desktop app: **Ctrl+,** → *Search by meaning* → *Ask* →
   *Chat model* (the list offers the models on the server that can answer: on Ollama, models
   that only make vectors, such as `bge-m3`, are left out). Terminal app:
   `coxswain --meaning ask qwen3:8b`. On Ollama a missing model is pulled first. A model that
   cannot answer is refused with the reason (below), in both apps.
2. Press **Ctrl+F7** (the F9 command list calls it *Ask your files*): Find file opens at *Ask*
   (desktop app: the fourth button lit; terminal app: the prompt `ask: `). Already in Find file,
   **Ctrl+F7** goes there too, as do **Shift+Tab** from *Everywhere* and a click on the *Ask*
   button.
3. Type the question and press **Enter**. The sources come first, then the answer, word by word.
   Until the first word comes, the answer says *Waiting for qwen3:8b to answer: a model that is
   not loaded yet takes a while…*, with whatever server it is on.
4. Ask a **follow-up** the same way: *and in Danish?*, *who wrote that?*. The questions and
   answers before it go along, and the passages are looked up with the question before it too,
   so short follow-ups work.
5. **Up** / **Down** move through the sources of the last answer. **Enter** with the field empty
   goes to that file: the active panel opens its folder with the cursor on it. In the desktop
   app a click on a source or on a **[n]** in the answer does the same. **F4** edits the source,
   and **F3** views it in the terminal app.
6. **Esc** closes Find file and forgets the questions. An answer still being written stops,
   also while the server is still loading the model and has not said a word.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Ctrl+F7** | Open Find file at Ask, or go to Ask from another depth | The same |
| **Tab** / **Shift+Tab** | From Ask, on to *Everywhere* / back to *Text in files*; what you typed stays | The same |
| **Shift+F7**, **Alt+F7** | From Ask, straight to *Text in files* / names everywhere | The same |
| **Enter** with a question typed | Ask it | The same |
| **Enter** with the field empty | Go to the source under the cursor | The same |
| **Up** / **Down** | Move through the last answer's sources | The same |
| **F3** | Nothing: the preview pane is under Find file | View the source |
| **F4** | Edit the source | The same |
| **Esc** | Close and forget; an answer being written stops | The same |
| Mouse | Click a source or a **[n]** to go to it | None |

## What you see

- **Not set up yet**, the line under the field says what is missing. With search by meaning off:
  *Ask answers questions from your files. It needs search by meaning, and a chat model on your
  server:* with the link *set it up*. With no chat model: *Ask needs a chat model on your server
  (Ollama, Lemonade, LM Studio):* with the same link. The link opens Settings at the right
  section. The terminal app says which command to run.
- **A model that cannot answer**: when the chat model only makes vectors, the line under the
  field says so in red instead, for example *bge-m3 makes vectors and cannot answer: choose a
  chat model, e.g. qwen3:8b.*, with the link *set it up* (desktop app), and **Enter** does not
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
- **The footer**: *Enter ask, or go to the source · ↑↓ sources · F4 edit · Tab names · Shift+F7
  text · Esc close and forget* (the terminal app adds *F3 view*).

## Settings and config.toml

| Item | Key | Type, default | Does |
|---|---|---|---|
| *Chat model* | `[search] ask_model` | string, `""` | The model that writes the answers. Empty: Ask is not set up |
| *Vectors made by*, *Server*, *API key from the variable* | `meaning_engine`, `meaning_url`, `meaning_key_env` | | The server Ask talks to, as above |
| (none) | `[keys] ask` | list of keys, `["Ctrl+F7"]` | The keys that open Find file at Ask ([Changing keys](../customise/keys.md)) |

```toml
[search]
meaning = true
ask_model = "qwen3:8b"
```

## In the terminal app

Ask is the same: **Ctrl+F7** for `ask: `, type, **Enter**. The answer wraps in the window and scrolls
so its end stays in sight. Set it up with:

```sh
coxswain --meaning ollama            # or: --meaning on, --meaning server URL MODEL
coxswain --meaning ask qwen3:8b      # pulled on Ollama when missing
coxswain --meaning ask off           # Ask off again
```

## Questions

#### What is sent, and where?
The question, the questions and answers before it in this Find file, and the ten passages with
their file paths, to the chat model on the server in *Search by meaning* (Ollama on this machine
with the built-in model). Nothing else, and nothing to anyone else. With a server on another
machine, Settings says so in bold, as for the vectors. See [Privacy](../reference/privacy.md).

#### Why only ten passages?
They are the closest to the question, at most three from one file, so a long document does not
crowd out the rest. Ten passages of up to 120 words fit in the context of small local models and
keep the answer quick.

#### Why does it say the sources do not hold the answer?
The model is told to answer from the sources only and to say so when they do not answer the
question. Ask with other words, or check with *Text in files* that the file is read: folders
outside *Folders read*, *Names only* folders and files still waiting for their vectors are not
searched.

#### Can it answer from one folder only?
Not yet. Ask looks in everything *Text in files* reads.

#### Are the answers kept, for next time?
No. They live in the Find file window only. Closing it forgets them, and nothing is written to
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
wait at any moment. After that the words come as they are made.

#### It says my model makes vectors and cannot answer. Why?
The model in *Chat model* is an embedding model, such as `bge-m3` or `nomic-embed-text`: it turns
text into vectors for search by meaning and cannot write an answer. Pick a chat model instead,
such as `qwen3:8b`. On Ollama, Coxswain asks the server what each model can do (`/api/show`)
when Find file opens at *Ask* and when Settings opens, and the *Chat model* list leaves embedding
models out. A server with the OpenAI API does not say: its whole list is offered, and the model
you choose is sent a one-word question when you save it (Settings, or `coxswain --meaning ask`);
its error, if any, shows under *Chat model*.

#### Why does a follow-up sometimes find other sources?
Each question looks up its own passages, together with the question before it. The numbers
under each answer are that answer's sources.

---
[← Previous: Search by meaning on a server](servers.md) · [Next: The search helper →](helper.md)

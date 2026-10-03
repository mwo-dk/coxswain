[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Find file

Find file is one window for four kinds of search: names on the whole machine, names in
this folder, the text inside your files, and [Ask](ask.md), questions answered from them. Open it, type, and go to the file with **Enter**.

![The desktop app's Find file over the panels: "engine" typed in Text in files, 18 matches in 1.3 ms in the text of 107 files, each name in bold with its folder and the passage with "engine" highlighted](../screenshots/gui-text-search.png)
*The desktop app searching the text of files. Each hit has its folder and the passage that matched.*
![The desktop app's Find file opened with nothing typed: the buttons Everywhere, In rocket, Text in files and Ask, Everywhere highlighted, and the hint Type to search every file name on this machine. Tab: only this folder, or the words inside your files.](../screenshots/search-find-file-scopes.png)

## How to use it

1. Press **Alt+F7** or **Ctrl+F** (the F9 command list calls it *Find file*). It opens at names
   everywhere, every time.
2. Type. Results update as you type; typing is never held up by a search, however large the
   index, and a search that a newer one replaces is dropped.
3. Press **Tab** to go to the next depth, and **Tab** again for the one after. From the last it
   goes back to the first.
4. Move to a result with **Up** / **Down** and press **Enter**: the active panel opens the
   result's folder with the cursor on it, and Find file closes.

| Depth | Terminal app prompt | Desktop app button | Finds |
|---|---|---|---|
| [Names everywhere](names.md) | `everywhere: ` | *Everywhere* | Files and folders by name, on the whole machine |
| [Names in this folder](names.md#names-in-this-folder) | `in ~/projects: ` | *In projects* | The same, in the active panel's folder and below |
| [Text in files](text.md) | `text: ` | *Text in files* | Files whose text has your words; with [search by meaning](meaning.md), also files about them |
| [Ask](ask.md) | `ask: ` | *Ask* | An answer to your question from the closest passages, with numbered sources |

| Key | Desktop app | Terminal app |
|---|---|---|
| **Up** / **Down** | Move through the results | The same |
| **PageUp** / **PageDown** | Fifteen results at a time | Ten results at a time |
| **Enter** | Go to the result | The same |
| **F3** | Nothing: the preview pane is under Find file | View the file in your viewer, then come back to the results |
| **F4** | Edit the file in your editor, Find file stays open | The same |
| **Tab** | The next depth | The same |
| **Backspace** | Delete the last character | The same |
| **Esc** | Close | The same |
| Mouse | A click selects a result, a double-click goes to it; a click on a depth button chooses it | None |

**F3** and **F4** are the *View* and *Edit* actions, so if you [change their keys](../customise/keys.md)
Find file follows. **Enter** on a folder opens the folder that holds it, with the cursor on it.

## What you see

**Where it can look.** In the desktop app the four depths are four buttons side by side at the
right of the search field, the current one highlighted: *Everywhere*, *In projects* (the
active panel's folder name), *Text in files* and *Ask*. The terminal app shows the depth as the prompt.

**Before you type**, the line under the field says what the depth does:

| Depth | Desktop app | Terminal app |
|---|---|---|
| Names | *Type to search every file name on this machine. Tab: only this folder, or the words inside your files.* | `452393 files indexed · Tab: only this folder, or the words inside your files.` |
| Text | *Type words to search inside your files: text, PDF, Word, spreadsheets, slides, mail and books.* | The count of files with text, then the same sentence |

In text, while [search by meaning](meaning.md) is off, a second line says *Also find files about
your words, in any language:*. The desktop app follows it with the link *turn on search by
meaning*, which opens *Settings → Search by meaning*; the terminal app follows it with the
command, `coxswain --meaning on` (shown while there are no hits).

**While you type**, the line counts: `128 matches in 3.1 ms · 1,402,311 files indexed` for names,
`7 matches in 2.4 ms · text of 31,208 files · 412 still to read` for text. While the first index is
built it adds ` · building index…`; while a fresh one replaces the saved one, ` · refreshing index`.
Nothing found: *No matches*.

**Each result** is the name (bold, in the desktop app) and its folder. A text hit adds the passage
that matched, your words highlighted: a marker in the desktop app, the `search_hit` colour on a
second line in the terminal app. A hit found by meaning starts its passage with *similar to:*.

**How many.** The desktop app lists the first 500 and says *showing the first 500, type more to
narrow*; the terminal app gets up to `max_results` (10,000) and draws what fits.

**The footer** lists the keys: *Enter go to · Tab everywhere/here/text/ask · F4 edit · Esc close*
(desktop app), `Enter go to · Tab everywhere/here/text/ask · F3 view · F4 edit · Esc close · syntax: F1`
(terminal app).

## Settings and config.toml

| Key | Type | Default | Does |
|---|---|---|---|
| `keys.search` | list of keys | `["Alt+F7", "Ctrl+F"]` | The keys that open Find file |
| `search.max_results` | number | `10000` | Most results of one search (the desktop app shows at most 500 of them) |

The rest are the depths' own: see [Search settings](settings.md).

## In the terminal app

The same window, depths, keys and results, in a frame over the panels titled *Find file*. It
differs in three things: **F3** views a result (the desktop app has the preview pane there
instead), there is no mouse, and the tip for search by meaning names the command rather than a
link. **F1** lists the [name syntax](name-syntax.md).

## Questions

#### How do I search only this folder?
Press **Tab** once. *In projects* (the active panel's folder; `in ~/projects:` in the terminal app)
searches that folder and everything below it, by name. See [Names in this folder](names.md#names-in-this-folder).

#### Why does F3 do nothing in the desktop app's Find file?
In the desktop app F3 is the preview pane, and Find file covers it. Press **Enter** to go to the
file, then **Space** for the [preview](../previews/README.md). **F4** edits a result in both apps.

#### Why does Find file start at names everywhere again?
It opens at the first depth every time, so **Alt+F7** then typing always searches names. One
**Tab** or two takes you to the others; in the desktop app, click the button.

#### Can I search the text of files in one folder only?
No. *In projects* narrows names only; *Text in files* covers every [folder read](folders.md). The
folder under each hit shows where it is.

#### Why does it say "showing the first 500"?
The desktop app lists at most 500 hits, so the list stays quick to draw. Type more words, or use
`ext:` or a path term (`src/`) to [narrow it](name-syntax.md).

#### Is there a quicker search for the panel I am in?
[Quick search](../panels/quick-search.md): **Alt+letter** jumps to names starting with what you
type, in the current panel only.

#### What is "similar to:" in front of a passage?
The file was found by [meaning](meaning.md), not by your words. Word hits come first; files
found by meaning come after them.

#### Can I open the file straight from Find file?
**F4** opens it in your editor. **Enter** takes you to it in the panel, where **Enter** again
[opens it](../commands/opening-files.md) with its program.

---
[← Previous: Search](README.md) · [Next: Names everywhere →](names.md)

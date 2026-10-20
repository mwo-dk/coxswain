[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# The built-in editor, Shift+F4

**Shift+F4** opens the file under the cursor in Coxswain's own editor, in both apps: the text
with the colours of the viewer, line numbers, undo, find and replace, and go to line. It is for
the quick change: a line in a config file, a typo in a README, a value in a `.env` file,
without leaving the panels. **F4** still starts your own editor
([View and edit](view-and-edit.md)); in the terminal app, when no editor is set at all, **F4**
opens this one too, rather than `vi`.

<!-- screenshot: editor-tui.png: the terminal app's built-in editor in Classic blue on src/engine.rs in ~/projects/rocket: line numbers, a selected word, the title naming engine.rs, Rust, UTF-8, LF and the line count with "modified", the line under the text saying Line 12, column 9, and the key line at the bottom -->
<!-- screenshot: editor-gui.png: the desktop app's editor window in Cyber over the panels on src/engine.rs: Save, Search, Replace and Go to line buttons at the top, highlighted code with line numbers, the find bar open at the bottom with "fuel" typed -->

## Contents

- [How to use it](#how-to-use-it)
- [Keys](#keys)
- [What you see](#what-you-see)
- [Saving: what is kept](#saving-what-is-kept)
- [What it opens](#what-it-opens)
- [The two apps](#the-two-apps)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to use it

1. Put the cursor on a file.
2. Press **Shift+F4**. Or open the action menu (**Shift+F10**) and choose *Edit here*, or find
   *Edit here* in the command list (**F9**).
3. Type. **Ctrl+S** saves.
4. **Esc** closes. With changes not saved yet, it asks first.

In [Find](../search/find-file.md), **Shift+F4** on a hit edits that file; when the editor
closes, the search is there as it was. In the terminal app's [viewer](view-and-edit.md#the-built-in-viewer),
**Shift+F4** goes from viewing to editing the same file.

## Keys

The keys inside the editor are fixed: `[keys]` changes only **Shift+F4** itself (`edit_here`).

| Key | Does |
|---|---|
| Letters, **Enter**, **Backspace**, **Delete** | Type. **Enter** starts the new line with the indent of the line before |
| **Tab** | Indents: a tab when the file indents with tabs, else four spaces |
| Arrows, **Home**, **End**, **PageUp**, **PageDown** | Move. **Home** goes to the first character that is no space, a second **Home** to the line's start |
| **Ctrl+Left**, **Ctrl+Right** | A word left or right |
| **Ctrl+Home**, **Ctrl+End** | The start or the end of the text |
| **Shift** with any of those | Selects |
| **Ctrl+A** | Selects all |
| **Ctrl+C**, **Ctrl+X**, **Ctrl+V** | Copy, cut, paste |
| **Ctrl+Z**, **Ctrl+Y** (or **Ctrl+Shift+Z**) | Undo, redo. Letters typed one after another are undone together |
| **Ctrl+S**, **F2** | Save |
| **Ctrl+F**, **F7** | Find: type the text, **Enter**. Case does not count; the search goes on past the end from the top |
| **Shift+F7** | The next hit |
| **Ctrl+R** | Replace: what to find, **Enter**, what to put in its place, **Enter**: every hit is replaced, as one step of undo |
| **Ctrl+G** | Go to a line: its number, **Enter** |
| **Esc**, **F10** | Close; with changes, the question below. **Esc** in a find or replace line closes that line only |
| **F1** | F1 → Features at *The built-in editor*; **Esc** there comes back to your text |

## What you see

- **Terminal app:** the file full screen over the panels, in a double frame. The title names
  the file, its language, its encoding, its line endings (*LF* or *CRLF*), its number of lines
  and, once you change it, *modified*. Line numbers run down the left, the current one brighter.
  A selection is marked in the cursor colour. The line under the text says where the cursor is
  (*Line 12, column 9*), or what was just done (*Saved engine.rs*, *Replaced 3 times*,
  *Not found: fuel*); a find, replace or go-to line is typed there too. The key line is at the
  bottom.
- **Desktop app:** a window over the panels, with the file's name (and a dot once it has
  changes), its language, encoding and line endings, and the buttons *Save*, *Search*,
  *Replace*, *Go to line* and *Close*, each with its key. The find, replace and go-to fields
  open in a bar at the bottom, with *Next* and *Previous*, or *Replace all*. The line at the
  very bottom says where the cursor is.
- **Closing with changes:** the terminal app asks on the line under the text:
  *Save the changes to engine.rs? y save · n throw them away · Esc back to editing*. The desktop
  app shows the same question with the buttons *Save*, *Don't save* and *Cancel*.
- **Changed elsewhere:** when the file changed on the disk since it was opened (another program
  saved it, a `git pull`), saving does not write over that. It asks:
  *engine.rs changed on the disk since it was opened. o save over it · r read it again (your
  changes are lost) · Esc back*; the desktop app has *Save over it*, *Read it again* and
  *Cancel*.

When the editor closes after a save, the panels are read again, so the new size and time show.

## Saving: what is kept

A file is saved the way it was read, so a change of one line changes one line:

- **Encoding.** UTF-8, UTF-8 with its byte order mark, UTF-16 (little or big endian, by its
  mark) and Windows-1252 are written back as they were. A file that is not entirely valid UTF-8
  is read as Windows-1252, where every byte stands for a character, so the bytes you did not
  touch stay the same. A character Windows-1252 has no byte for (a `✓` typed into such a file)
  is refused with *Not saved: The file is in Windows-1252, which has no ✓*.
- **Line endings.** A file whose lines end in CRLF (most of them) keeps CRLF; others keep LF. A
  file that mixes them gets the one most of its lines had.
- **Permissions.** A script stays executable; the owner's, group's and others' bits are kept.
- **Links.** Saving a symbolic link saves the file it points to, and the link stays. A file
  with other hard links is written in place, so every name keeps seeing the new text.
- **No half files.** The text is written to a file beside it first, then renamed over the old
  one, so a full disk or a crash in the middle leaves the old file as it was.

## What it opens

- **Text files up to 16 MB.** Bigger files are refused with *Too big to edit here*; the viewer
  (**F3**) reads any size, and your own editor (**F4**) edits them.
- **Not binary files:** a file with a zero byte in it (and no UTF-16 mark) is refused with
  *Not text: the built-in editor edits text files only.*
- **Not inside an archive, a git history or a ZFS snapshot.** In an archive the status line
  says to copy the file out first (**F5**); a history and a snapshot are read-only.

## The two apps

Both apps open and save through the same code, so encodings, line endings, permissions and the
question about a file changed elsewhere are the same. The differences:

| | Terminal app | Desktop app |
|---|---|---|
| Where | Full screen over the panels | A window over the panels |
| Colours | The viewer's, for files of any size the editor opens | The preview's, for files up to about 60,000 characters; longer ones stay plain while you type |
| **F4** with no editor set | Opens the built-in editor | Opens the file in its default application ([View and edit](view-and-edit.md)) |
| Paste from outside | The terminal's own paste (**Ctrl+Shift+V** in most) comes in as it is, without indents added; **Ctrl+V** pastes what **Ctrl+C** or **Ctrl+X** took in the editor | **Ctrl+V** pastes from the system clipboard |
| Copy to outside | **Ctrl+C** also puts the text on the system clipboard, in terminals that allow it (OSC 52) | **Ctrl+C** puts it on the system clipboard |

## Settings and config.toml

The editor has no settings of its own. Its key is an action:

| Key | Type | Default | Used by |
|---|---|---|---|
| `[keys]` `edit_here` | list of keys | `["Shift+F4"]` | Both apps |

`editor` (Settings → *Behaviour* → *Editor*) decides what **F4** starts; when it is empty and
`$VISUAL` and `$EDITOR` are not set either, the terminal app's **F4** opens the built-in
editor ([View and edit](view-and-edit.md#settings-and-configtoml)).

## Questions

#### Why does F4 not open the built-in editor?

**F4** starts your own editor: `editor` in `config.toml`, else `$VISUAL` or `$EDITOR`. In the
desktop app, without `editor`, it opens the file in its default application. **Shift+F4** always
opens the built-in one. In the terminal app **F4** opens the built-in editor only when none of
those is set.

#### What if the file changed on the disk while I was editing it?

Saving asks first: **o** saves over it, **r** reads it again (your changes are lost), **Esc**
goes back to editing (the desktop app: *Save over it*, *Read it again*, *Cancel*). Nothing is
written until you choose. Coxswain notices by the file's size and time of change, taken when it
was opened.

#### Does saving change the file's encoding or line endings?

No. UTF-8 (with or without its byte order mark), UTF-16 and Windows-1252 are written back as
they were read, CRLF stays CRLF, and the file keeps its permissions. See
[Saving: what is kept](#saving-what-is-kept).

#### Pasting into the terminal app indents every line further. Why?

It does not any more when the terminal sends a paste as one piece (bracketed paste), which
nearly every terminal does: the text then goes in as it is. In a terminal without it, each
pasted line comes in as typed keys, and **Enter** adds the indent of the line before.

#### Can I edit a file inside an archive?

Not in place: the editor would change a copy, and the change would be lost. Copy it out with
**F5** first ([Archives as folders](../files/archives.md)).

#### Why is a big file refused?

The editor holds the whole text in memory, with what undo needs. Up to 16 MB that is quick;
past it, the [viewer](view-and-edit.md#the-built-in-viewer) (**F3**) reads any size a screen at
a time, and your own editor (**F4**) edits it.

#### How do I get vi back on F4?

Set `editor = "vi"` in `config.toml`, or in Settings → *Behaviour* → *Editor*, or set
`$EDITOR` in your shell. **F4** then starts it, and **Shift+F4** still opens the built-in
editor.

---
[← Previous: View and edit, F3 and F4](view-and-edit.md) · [Next: Opening files →](opening-files.md)

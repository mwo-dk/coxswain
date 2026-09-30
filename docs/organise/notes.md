[← README](../../README.md) · [Docs index](../README.md) · [Tags, notes, favourites and the sidebar](README.md)

# Folder notes

A note is free text kept for a folder: to-dos, reminders, where the backup is, which script
builds it. It shows at the bottom of the preview pane whenever you look at that folder.

![The desktop app with the preview pane showing launch-report.pdf; under the PDF page, the notes field labelled Notes for this folder, still empty, with its grey hint To-dos, reminders… saved when you leave the field (Esc)](../screenshots/gui-pdf.png)
*The notes field at the bottom of the preview pane, empty. The cursor is on a file, so the note is for the folder, Documents.*

<!-- screenshot: organise-notes.png: desktop app, Cyber theme, in /home/demo/projects/rocket: the preview pane open on the right with the folder src under the cursor, the notes field at its bottom labelled "Notes for src" and filled with two lines of to-dos; the left pane's footer shows the note icon next to "7 items" -->

## How to use it

1. Put the cursor on a folder to write its note. On a file, or on `..`, the note is for the
   folder you are in.
2. Press **Alt+N**, or pick *Folder notes* in the command list (**F9**). The
   [preview pane](../previews/README.md) opens if it was closed, and the cursor is in the notes field.
3. Type. Lines, blank lines and any script work; text is written in the direction you type,
   so Hebrew and English notes both read correctly.
4. Leave the field: press **Esc**, or click anywhere else. That saves the note.

To read a note, just move the cursor onto the folder with the preview pane open (**Space** opens
it): the field at the bottom shows its note. To delete a note, empty the field and leave it: a
note of only spaces and blank lines is removed.

While the cursor is in the field, keys type text, except **Esc** (leaves the field) and the
function keys, which keep working (**F3** still closes the preview, **F5** still copies).

| Key | Desktop app | Terminal app |
|---|---|---|
| **Alt+N** | Preview pane opens, cursor in the notes field | Status line: *Folder notes is available in the desktop app (coxswain-gui)* |
| **Esc** (in the field) | Leaves the field and saves | — |

## What you see

- **The field** sits at the bottom of the preview pane, under the file or folder preview. Its
  label says whose note it is: *Notes for src* when the cursor is on the folder `src`, *Notes for
  this folder* when it is on a file or `..`. Empty, it reads *To-dos, reminders… saved when you
  leave the field (Esc)*.
- **The note icon** in the pane's footer, next to the item count, shows when the folder that
  pane is in has a note. Hover it: *This folder has notes (Alt+N)*. Folders in the list with a
  note get no mark; move the cursor onto them to see it.
- When the preview pane shows [command output](../commands/command-line.md#what-you-see), the
  field is not there; **Alt+N** switches the pane back to the preview first.

## Settings and config.toml

No Settings item. The key is `notes` under `[keys]`, default `["Alt+N"]`. The notes themselves
are kept in `state.json` (`notes`, a map from folder path to text), not in `config.toml`; see
[Where things are kept](../reference/where-things-are-kept.md).

## In the terminal app

There are no notes in the terminal app; **Alt+N** says *Folder notes is available in the desktop
app (coxswain-gui)*. The terminal app has no preview pane to show them in. Notes written in the
desktop app stay in `state.json` meanwhile.

## Questions

#### Where did my note go after I moved the folder?

A note belongs to a path. Move or rename the folder, in Coxswain or elsewhere, and the note
stays at the old path; a new folder there shows it again. Move the folder back, or copy the
text into the note at the new place.

#### Is my note saved while I type?

No, when you leave the field: **Esc**, a click elsewhere, or moving the focus. Press **Esc**
before you close the window, to be sure the last words are kept.

#### I pressed Alt+N on a file. Whose note is it?

The folder you are in: the label says *Notes for this folder*. Files have no notes of their
own; use a [colour tag](tags.md) to mark a file.

#### How do I see which folders have notes?

The pane footer shows the note icon for the folder you are in. Folders in the list are not
marked. There is no list of all notes; they are in the `notes` part of `state.json`, which is
plain JSON you can open in an editor.

#### Can I search the text of my notes?

No. [Find file](../search/find-file.md) searches file names and the text of files, not notes.
If you want a note to be found, keep it as a file in the folder, such as `NOTES.md`.

#### Why does the preview pane open when I press Alt+N?

The notes field is part of the preview pane. **Alt+N** opens the pane and puts the cursor in
the field; **Esc** leaves the field, a second **Esc** closes the pane.

#### Can I write a note for a folder inside an archive?

Yes. While you [browse an archive](../files/archives.md), its folders have paths such as
`…/backup.zip/photos`, and the note is kept for that path. Nothing is written into the archive.

---
[← Previous: Colour tags](tags.md) · [Next: Favourites →](favourites.md)

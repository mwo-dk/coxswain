[← README](../../README.md) · [Docs index](../README.md) · [Tags, notes, favourites and the sidebar](README.md)

# Colour tags

A colour tag is a coloured dot after a file's or folder's name: red for urgent, green for done,
whatever you like. Use it to find things again at a glance in a busy folder.

![The details view of the desktop app with tag dots: a grey dot after the folder src, a red dot after TODO.txt, a yellow dot after budget.xlsx and a red dot after launch-report.pdf](../screenshots/gui-details.png)
*Tags in the details view: grey on `src`, red on `TODO.txt` and `launch-report.pdf`, yellow on `budget.xlsx`.*

## How to use it

1. Put the cursor on a file or folder, or mark several ([Marking files](../panels/marking.md)).
2. Press **Alt+T**, or pick *Colour tag* in the command list (**F9**).
3. The *Colour tag* dialog opens. Press a digit, or click a colour:

| Key | Tag |
|---|---|
| **1** | Red |
| **2** | Orange |
| **3** | Yellow |
| **4** | Green |
| **5** | Blue |
| **6** | Purple |
| **7** | Grey |
| **0** | none: removes the tag |
| **Esc** | Closes the dialog and changes nothing |

The dialog closes at once and the panel is read again. Marked files all get the same colour; when
nothing is marked, the file under the cursor gets it. The `..` entry cannot be tagged.

To change a tag, tag the file again with the other colour: a file has one tag at most.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Alt+T** | Opens *Colour tag* | Status line: *Colour tag is available in the desktop app (coxswain-gui)* |

## What you see

- **The dialog:** titled *Colour tag*, with one button per colour, each a dot, its digit and its
  name (*1 Red* … *7 Grey*), and a last button *0 none* with an empty dot.
- **In the panels:** a small round dot in the tag's colour right after the name. It shows in all
  three views: [details, Miller columns and thumbnails](../panels/views.md). In the details view,
  hover the dot for the colour's name.
- **The colours** are fixed and the same in every theme: red `#ef5350`, orange `#ffa726`, yellow
  `#fdd835`, green `#66bb6a`, blue `#42a5f5`, purple `#ab47bc`, grey `#9e9e9e`. Your own theme
  cannot change them ([Your own theme](../customise/own-theme.md)).

## Settings and config.toml

There is no Settings item. The key is `tag` under `[keys]`, default `["Alt+T"]`:

```toml
[keys]
tag = ["Ctrl+Shift+T"]
```

The tags themselves are kept in `state.json` (`tags`, a map from full path to colour name),
not in `config.toml`. See [Where things are kept](../reference/where-things-are-kept.md).

## In the terminal app

The terminal app has no tags. It does not show the dots, and **Alt+T** only says *Colour tag is
available in the desktop app (coxswain-gui)* in the status line. Tags you set in the desktop app
are still there when you come back to it. Tags, like notes, favourites and the sidebar, were
built for the desktop app and have not come to the terminal app yet.

## Questions

#### I renamed a file and its tag is gone.

A tag belongs to a path, not to the file. Renaming or moving a file, in Coxswain or anywhere
else, leaves the tag behind at the old path; a new file with the old name gets the old tag
back. Tag the file again under its new name.

#### Can I tag many files at once?

Yes. Mark them (**Insert**, **Shift+Down**, or **+** for a pattern), then press
**Alt+T** and a digit. Every marked file gets that colour. **0** removes the tag from all of them.

#### Can I search for tagged files, or sort by tag?

No. [Find](../search/find-file.md) does not search tags, and the panels do not sort by
them. Tags are for the eye, in the folder you are in.

#### Are these the tags of macOS Finder or of my Linux file manager?

No. Coxswain's tags are its own, kept in its `state.json`. Finder tags, KDE Dolphin tags and
GNOME's emblems are not read or written, and other programs do not see Coxswain's.

#### Do tags go with a file when I copy it to another computer or a USB stick?

No. Nothing is written into the file or next to it. The copy has no tag; the original keeps its own.

#### Can I tag a file inside an archive?

Yes. While you [browse an archive as a folder](../files/archives.md), its files have paths such
as `…/photos.zip/2024/beach.jpg`, and the tag is kept for that path. It is lost when the
archive is renamed or moved, like any other tag.

#### The dot is hard to see. Can I make it bigger or pick my own colours?

No. The seven colours and the dot's size are fixed, so a tag means the same in every theme.

#### Does the terminal app show tags I set in the desktop app?

No. See [In the terminal app](#in-the-terminal-app).

---
[← Previous: Tags, notes, favourites and the sidebar](README.md) · [Next: Folder notes →](notes.md)

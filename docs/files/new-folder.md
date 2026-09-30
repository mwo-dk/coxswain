[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# New folder (F7)

**F7** makes a new folder in the active panel's folder, or a whole path of folders at once. It
also makes folders inside an archive.

<!-- screenshot: files-new-folder.png: desktop app, Cyber theme: the New folder dialog with 'Name:' and '2026/09/receipts' typed in -->

## How to use it

1. Press **F7**. The dialog *New folder* opens with an empty field.
2. Type a name, or a path: `2026/09/receipts` makes `2026`, `09` in it and `receipts` in that.
3. Press **Enter** (or *OK*). An empty name, or **Esc**, cancels.

| | Desktop app | Terminal app |
|---|---|---|
| Key | **F7** | **F7** |
| Dialog label | *Name:* | *Create the folder:* |
| Confirm / cancel | **Enter** or *OK* / **Esc** or *Cancel* | **Enter** / **Esc** |

A relative name starts in the active panel's folder; `~/…` starts in your home folder, and an
absolute path makes the folder there.

## What you see

The status line says `Created 2026/09/receipts` (the terminal app names the full path,
`Created /home/me/2026/09/receipts`), the panel is read again and the cursor lands
on the new folder (for a path, on its first folder, `2026`). A folder that already exists is
not an error: nothing changes.

Inside an archive the new folder is written into the archive, which is written anew
([Archives as folders](archives.md#what-changes-and-how-safely)); a name that is already there
is refused with `… exists`.

## Settings and config.toml

None. The key is `mkdir` in `[keys]`; the F-key bar calls it *Mkdir*.

## In the terminal app

The same, with the label *Create the folder:*, and the status line names the full path.
**Ctrl+U** clears the field.

## Questions

#### Can I make several nested folders at once?

Yes: type the whole path, `photos/2026/summer`. Every folder on the way that is missing is made.

#### Why did nothing happen when the folder exists?

Making a folder that is already there is not treated as an error outside archives: the status
line still says `Created …` and the cursor goes to it. Inside an archive it is refused.

#### Can I make a new, empty file?

Not with a key of its own. Use the [command line](../commands/command-line.md): `touch notes.txt`
(Linux, macOS), or a [user menu](../commands/user-menu.md) entry.

#### Can I make a folder inside a zip?

Yes. Open the zip with **Enter**, press **F7** and type the name. The zip gets a folder entry,
so the folder stays even while it is empty.

---
[← Previous: Move and rename (F6)](move-and-rename.md) · [Next: Delete →](delete.md)

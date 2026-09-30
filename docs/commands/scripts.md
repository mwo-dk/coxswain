[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# Scripts

In the desktop app, every file in the `scripts` folder next to your config is an entry of the
**F2** menu. A script gets the marked files as arguments, so a longer job (resize these
pictures, convert these documents, upload these files) is one file you drop in a folder, with
no quoting to get right in `config.toml`.

<!-- screenshot: commands-scripts.png: desktop app, Cyber theme, in /home/demo/Pictures with three pictures marked: the F2 menu titled "Scripts" with the four git entries at the top and, under them, the entries "Make thumbnails" and "Upload" with the script icon and no key; the cursor on "Make thumbnails" -->

## How to use it

1. Find the folder: `coxswain --config-path` prints where `config.toml` is; the scripts go in a
   folder named `scripts` next to it.

   | System | Folder |
   |---|---|
   | Linux | `~/.config/coxswain/scripts/` |
   | macOS | `~/Library/Application Support/coxswain/scripts/` |
   | Windows | `%APPDATA%\coxswain\scripts\` |

2. Put a script there. On Linux and macOS it must start with a `#!` line and be executable
   (`chmod +x`). On Windows it must be a program Windows starts by itself, such as a `.exe`,
   `.bat` or `.cmd` file.

   ```sh
   #!/bin/sh
   # ~/.config/coxswain/scripts/Make thumbnails
   for f in "$@"; do magick "$f" -resize 400x "thumb-$(basename "$f")"; done
   ```

3. In the desktop app, mark the files, press **F2**, move to the script with **Up** and
   **Down** and press **Enter**, or click it.

The menu is read afresh on every **F2**, so a new script shows at once, without a restart.

## How a script is run

| | |
|---|---|
| Arguments | The marked files, as full paths; when nothing is marked, the file under the cursor, as a full path; on `..` with nothing marked, none |
| Folder | The active panel's folder |
| Shell | None: the script is run as a program. No `%f` or `%s`; use `"$@"` |
| Input | None (standard input is empty) |
| Output | Standard output, then standard error, then `[exit status: 1]` if it failed, in the preview pane |

Only files inside the `scripts` folder run this way. A link in it that points to a file
elsewhere is followed and then refused with *Not a Coxswain script*.

## What you see

- **In the menu** (titled *Scripts*): after your [`[[user_menu]]`](user-menu.md) entries, one row
  per file, sorted by file name, labelled with the name without its extension (`resize.sh` shows
  as *resize*), with a script icon and no key.
- **While it runs:** the status line says *Running Make thumbnails…*.
- **Afterwards:** the [preview pane](../previews/README.md) shows the output, headed with the
  script's label and *Command output*, and both panels reread. The `×` (*Back to preview*) goes
  back to the file preview.
- **When it cannot start:** the status line shows why, for example *Permission denied (os error
  13)* when it is not executable, or *Exec format error (os error 8)* when the `#!` line is missing.

## Settings and config.toml

No Settings item and no config key: the folder is fixed, next to `config.toml`. **F2** is
`user_menu` under `[keys]`.

## In the terminal app

The terminal app does not have scripts; its **F2** shows `[[user_menu]]` entries only. To run a
script there, add it as a user-menu entry with the marked files:

```toml
[[user_menu]]
key = "m"
label = "Make thumbnails"
command = "~/.config/coxswain/scripts/'Make thumbnails' %s"
wait = true
```

Note that `%s` gives the name under the cursor when nothing is marked, where a script run by the
desktop app gets the full path.

## Questions

#### My script is in the menu but nothing happens.

Look at the status line: the error is there. Usually the script is not executable (`chmod +x`),
or its first line is not a `#!` line. The folder must be exactly `scripts`, next to `config.toml`.

#### Why does my README (or .DS_Store) show up as a script?

Every file in the folder is listed, whatever its name or permissions. Keep only scripts in it;
put notes and helpers in a subfolder, which is not listed.

#### Can I give a script a key, like the user-menu entries?

No, script files have no key. Add a `[[user_menu]]` entry that runs the script (as in
[In the terminal app](#in-the-terminal-app)) and give that entry a key.

#### Can a script use a shell pipe, or my aliases?

The script's `#!` line picks its interpreter, so inside it you can use anything that shell has.
Aliases from your interactive shell's rc files are not loaded.

#### Can a script ask me something, or show a progress bar?

No. It gets no input, and its output is shown only when it ends. For anything interactive,
open a terminal, or use the terminal app with a user-menu entry.

#### In which order are the scripts listed?

By file name, upper-case letters before lower-case. Put a number in front to choose the order
(`1 Resize`, `2 Upload`); the label shows the number too.

#### Why is a link to a script in my ~/bin refused?

Only files that really are inside the `scripts` folder may run from **F2**, so that a link
cannot be used to run something else. Copy the script into the folder instead.

---
[← Previous: The user menu, F2](user-menu.md) · [Next: View and edit, F3 and F4 →](view-and-edit.md)

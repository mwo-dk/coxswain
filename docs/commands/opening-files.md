[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# Opening files

**Enter**, or a double-click, opens what is under the cursor: a folder or an archive opens in
the panel, and a file opens in the application your desktop uses for it, as `xdg-open`, `open`
or `start` would. The terminal app runs a program instead of opening it.

<!-- screenshot: commands-opened.png: desktop app, Cyber theme, in /home/demo/Documents with the cursor on budget.xlsx after Enter: the status line at the bottom reads "Opened budget.xlsx" -->

## How to use it

1. Put the cursor on a file, folder or archive.
2. Press **Enter** (*Open*), or double-click it. When the command line has text, **Enter** runs
   the line instead ([The command line](command-line.md)); press **Esc** first to clear it.

| On | Desktop app | Terminal app |
|---|---|---|
| A folder, or `..` | Goes into it | Goes into it |
| An archive (`.zip`, `.tar.gz`, `.7z` and the others) | Opens it as a folder ([Archives as folders](../files/archives.md)) | Same |
| A file inside an archive | Status line: *photos.zip is an archive: F5 copies this file out of it* | Same |
| A program (a file with the executable bit on Linux and macOS; `.exe`, `.bat`, `.cmd`, `.ps1`, `.com` on Windows) | Opened like any other file, in its default application | Run in the terminal as `./name`, in the panel's folder, then `-- press Enter --` |
| Any other file | Opened in its default application | Same |

The default application is the one your desktop picks: `xdg-open` on Linux, `open` on macOS,
`start` on Windows. Coxswain starts it and goes on; it does not wait for the application.

## What you see

- **Opened:** the status line says *Opened budget.xlsx* and the application opens in its own
  window. In the terminal app too, the application opens outside the terminal.
- **Could not start the opener:** in the terminal app the status line says *Could not open:*
  and the error; in the desktop app it shows the error alone.
- **A program, terminal app:** the panels step aside, the terminal shows
  `/home/demo/bin> ./build.sh` and the program's output, then `-- press Enter --`. **Enter** brings
  the panels back, reread.
- **Double-click** works in the details, columns and thumbnails views of the desktop app and in the
  terminal app's panels (two clicks within 400 ms on the same spot).

## Settings and config.toml

No Settings item. The key is `open` under `[keys]`, default `["Enter"]`. Which application opens
which file is your desktop's choice, not Coxswain's: change it in your system settings, or with
`xdg-mime default` on Linux. To open files in a program of your own choosing from Coxswain, add a
[user-menu](user-menu.md) entry (`command = "gimp %s"`).

## In the terminal app

Folders, archives and files open as in the desktop app. The one difference is programs: the
terminal app runs them in the terminal, as Norton Commander did, and waits for **Enter** so you
can read what they printed. The desktop app has no terminal to run them in, so it hands them to
the desktop like any file. On a machine without a desktop (an SSH session, a server), `xdg-open`
is often missing or finds nothing, and files do not open; use **F3** and **F4** there
([View and edit](view-and-edit.md)).

## Questions

#### Enter on a file does nothing, and there is no error.

Coxswain started `xdg-open` (or `open`), which found no application for that type, or failed
without saying so. The status line still says *Opened …*, since the opener did start. Set a
default application for the type in your system settings, or open the file with **F4** or a
user-menu entry.

#### How do I run a script from the desktop app?

**Enter** hands it to the desktop, which may open it in a text editor instead of running it. Run
it from the [command line](command-line.md) (`./build.sh`), or put it in the
[scripts folder](scripts.md) so that **F2** runs it with the marked files.

#### How do I open a file with another application than the default?

There is no *Open with* menu. Add a [user-menu](user-menu.md) entry for that application, such
as `command = "gimp %s"`, or type it on the command line with **Ctrl+Enter** to add the name.

#### Why does Enter on a .zip go into it instead of opening my archive program?

Archives are browsed as folders in both apps ([Archives as folders](../files/archives.md)). To
open one in another program, use a user-menu entry, or `xdg-open photos.zip` on the command line.

#### Enter on a file inside an archive only shows a message.

A file inside an archive is not on disk, so no application can open it. Copy it out with **F5**
([Copy](../files/copy.md)), or look at it in the desktop app's preview pane (**F3**).

#### Can Enter open several marked files at once?

No, **Enter** opens the one under the cursor. For several, use a [user-menu](user-menu.md)
entry with `%s`: an application that takes many files (`command = "gimp %s"`), or a loop,
since `xdg-open` takes one file at a time: `command = "for f in %s; do xdg-open \"$f\"; done"`.

#### I opened a program in the terminal app and the panels do not come back.

The program is still running, or it has ended and waits at `-- press Enter --`. Press **Enter**.
If the program itself waits for input, answer it or end it first.

---
[← Previous: View and edit, F3 and F4](view-and-edit.md) · [Next: Search →](../search/README.md)

[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Pick mode: `coxswain --pick`

Another program can use Coxswain to choose files or folders, instead of the system's file
dialog. It starts `coxswain --pick`, you choose in the two panels as always, and the paths come
back to it. Pick mode is in the terminal app; the desktop app does not have it yet.

## Contents

- [Starting it](#starting-it)
- [Choosing](#choosing)
- [What comes back](#what-comes-back)
- [Questions](#questions)

## Starting it

```
coxswain --pick[=FILE] [--folder] [--multiple] [--filter EXT,EXT] [--start DIR] [--title TEXT]
```

| Flag | Does |
|---|---|
| `--pick` | Pick mode. The paths go to standard output, and only they do: the panels are drawn on the terminal itself (`/dev/tty`, `CONOUT$` on Windows), so `x=$(coxswain --pick)` works |
| `--pick=FILE` | The paths go to `FILE` instead |
| `--folder` | Folders are chosen, not files |
| `--multiple` | Several may be chosen, the marked ones |
| `--filter EXT,EXT` | Only files with these extensions can be chosen: `--filter pgn,pgn.gz`. A name before a colon, `"Games:pgn,pgn.gz"`, is ignored. Case does not matter |
| `--start DIR` | The folder both panels start in; the current folder without it |
| `--title TEXT` | Replaces the status line's *Choose a file* with your own text |

A flag it does not know, `--start` with a folder that is not there, or no terminal to draw on
(a cron job, for one) is said and nothing starts (exit status 2).

## Choosing

The status line says what is asked for, and the key line at the bottom shows the keys of pick
mode instead of the F-keys:

| Key | Does |
|---|---|
| **Enter** | Chooses the marked entries, or the one under the cursor. On a folder (or an archive) while files are asked for, it opens the folder as usual |
| **Esc**, **F10** | Cancels |
| the *Mark* key (**Insert** by default) | Marks, with `--multiple`. Marks work as always: see [Marking](../panels/marking.md) |
| **Ctrl+PageDown** | Opens the folder or archive under the cursor, also when `--folder` makes **Enter** choose it |
| the *Parent folder* key | Up a folder |

An entry that does not fit (a file with `--folder`, a folder among the marked files, a file
without the extension asked for, two with no `--multiple`) is not chosen: the status line says
why. A picker only chooses, so it changes nothing: every action that copies, moves, renames, deletes,
writes or edits a file, runs a command (the command line, the user menu) or writes settings is
refused with *The picker only chooses: it changes no files*, whether you reach it by its key,
the action menu (**Shift+F10**) or the command list (**F9**). Looking, marking, searching,
sorting, the preview and moving about work as always. A double-click on a file chooses it, as
**Enter** does.

## What comes back

- Each chosen path is one line, UTF-8, in `FILE` or on standard output (nothing else is
  printed there), and the exit status is `0`.
- Cancelling, or closing the terminal, prints nothing and exits `1`; a `FILE` left by an earlier run is not touched, so read the exit status first.
- A file chosen **inside an archive** is first unpacked into the cache folder (`picked/` in
  [the cache](where-things-are-kept.md)), and the path of that copy is what comes back, since the
  other program cannot read into an archive. Those copies are not deleted by Coxswain: the other
  program owns them, and [Disk use](disk-use.md) lists the cache. A folder inside an archive
  cannot be chosen. An archive itself (a `.zip`) is opened by **Enter**; mark it with the *Mark* key to choose it.
- Nothing is sent anywhere: the paths go to your file or your terminal.

Not in this version: asking for a new file name to save under (`--save`), dimming the files the
filter excludes, and the desktop app's window. They are a separate issue.

## Questions

#### How does the program that calls Coxswain get the paths?

Read its standard output (`x=$(coxswain --pick)`), or give it `--pick=FILE` and read the file
after Coxswain exits: one path per line, UTF-8. Exit status `0` means something was chosen, `1`
that it was cancelled.

#### Where are the panels drawn when standard output is a pipe?

On the terminal itself, as `fzf` does, so only the paths are printed on standard output. With no
terminal (no controlling terminal in a cron job) it says so and exits with `2`.

#### Why does the picker say it changes no files?

You pressed something that would copy, move, delete, edit or run a command. A program that asked
to choose files must not find them changed; use the normal `coxswain` for that.

#### How do I choose several files, or a folder?

For several, start with `--multiple`, mark the files with **Insert** and press **Enter**. For a
folder, start with `--folder`, put the cursor on it and press **Enter**; open a folder to look
inside it with **Ctrl+PageDown**.

#### Enter opened the folder instead of choosing a file. Why?

While files are asked for, **Enter** on a folder (or an archive) opens it, so you can look for
the file. On a file it chooses. Choosing a folder needs `--folder`.

#### What path comes back for a file inside an archive?

A copy: Coxswain unpacks the file into its cache (`picked/`, a folder of its own per run) and
gives that path. The copy stays until you delete it or clear the cache.

#### Can I limit it to some kinds of files?

Yes: `--filter pgn,pgn.gz`. A file with another extension cannot be chosen (the status line says
so), though it is still listed.

---
[← Previous: Command-line flags](command-line-flags.md) · [Next: Configuration: every key →](configuration.md)

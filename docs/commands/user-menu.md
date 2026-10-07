[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# The user menu, F2

**F2** opens a menu of commands, each with a key of its own: one keystroke opens a terminal in
the active panel's folder, shows a file's SHA-256 or runs `git status`, `cargo test` or a zip of
the marked files. It comes with three entries made for your system; the last row, *Add your own
command…* (`+`), opens `config.toml` where your own go. Placeholders put the file under the
cursor or the marked files into a command.

![The desktop app's F2 menu titled Scripts in ~/projects/rocket: Open a terminal here (t), SHA-256 of the file (h) and git status (s), the scripts Make thumbnails and Upload with a script icon and no key, and Add your own command… with the key +](../screenshots/commands-user-menu.png)
![The terminal app in Classic blue (NC): the User menu box over the panels with Open a terminal here, SHA-256 of the file, git status and Add your own command…, and their keys t, h, s and +](../screenshots/commands-user-menu-tui.png)

- [How to use it](#how-to-use-it)
- [The entries it comes with](#the-entries-it-comes-with)
- [Your own entries](#your-own-entries)
- [Ready-made entries to copy](#ready-made-entries-to-copy)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a file, or mark some ([Marking files](../panels/marking.md)).
2. Press **F2**, or pick *Menu* in the command list (**F9**).
3. Press an entry's key to run it at once, or move with **Up** and **Down** and press **Enter**.
   A click runs an entry in the desktop app. **Esc** closes the menu.

| Key | Desktop app | Terminal app |
|---|---|---|
| **F2** | Opens *Scripts*: the entries, then the [script files](scripts.md), then *Add your own command…* | Opens *User menu*: the entries, then *Add your own command…* |
| An entry's key | Runs it | Runs it |
| **+** | Opens `config.toml` at `[[user_menu]]` | Opens `config.toml` at `[[user_menu]]` |
| **Up**, **Down**, **Enter** | Move, run | Move, run |
| **Esc** | Closes | Closes |

## The entries it comes with

Until you have entries of your own, F2 offers these. Each is made for the system it runs on and
for what is installed there, and an entry that does not fit is left out, not shown broken: no
terminal without a desktop, no hash on a folder or `..`, no `git status` outside a repository.

| Key | Label | Does | Shown |
|---|---|---|---|
| `t` | Open a terminal here | A new terminal window in the active panel's folder | When a terminal is found (below) |
| `h` | SHA-256 of the file | The SHA-256 of the file under the cursor, shown with its name (`wait`) | On a file |
| `s` | git status | `git status` (`wait`) | In a git repository, with git installed |

The commands per system (the first one installed is taken):

| System | Open a terminal here | SHA-256 of the file |
|---|---|---|
| FreeBSD | `$TERMINAL`, else the first of `x-terminal-emulator`, foot, Alacritty, kitty, WezTerm, Ghostty, Konsole, GNOME Terminal, Ptyxis, Console (`kgx`), Xfce Terminal, MATE Terminal, LXTerminal, QTerminal, Terminator, Tilix, urxvt, st, xterm | `sha256 -- %f` (else `sha256sum`) |
| OpenBSD, NetBSD | the same | `sha256 -- %f`, else `sha256sum`, else `cksum -a sha256 -- %f` |
| illumos | the same | `digest -v -a sha256 -- %f`, else `sha256sum` |
| Linux | the same | `sha256sum -- %f`, else `shasum -a 256 -- %f` |
| Android (Termux) | not offered: Termux is the terminal | `sha256sum -- %f` |
| macOS | `open -a Terminal .` | `shasum -a 256 -- %f` |
| Windows | `start "" wt -d .` (Windows Terminal), else `start "" powershell` | `certutil -hashfile %f SHA256` |

On the BSDs, illumos and Linux the terminal is only offered on a desktop (`$WAYLAND_DISPLAY` or
`$DISPLAY` set), not on a text console or over ssh. It is started with `nohup … &`, so it stays
open when Coxswain quits. `$TERMINAL` is taken when the program it names is installed.

## Your own entries

Each entry is a `[[user_menu]]` table in `config.toml` ([Configuration](../reference/configuration.md)):

The quickest way in is **F2** then **+** (*Add your own command…*): `config.toml` opens in your
editor at its `[[user_menu]]`. When it has none yet, a commented example is written at its end
first: the entries F2 comes with on this system, and one more to start from. Take the `#` off
the lines of an entry to use it. Nothing else in the file is touched, comments included.

```toml
[[user_menu]]
key = "c"                 # the key that runs it inside the menu
label = "cargo test"      # what the menu shows
command = "cargo test"    # run with the shell in the active panel's folder
wait = true               # wait for Enter afterwards (terminal app), show the output (desktop app)
when = "file"             # optional: only on a file ("file") or in a git repository ("git")
```

Your own list **replaces** the entries F2 comes with. To keep them, take them over from the
commented example (or `coxswain --dump-config`) next to yours. `user_menu = []` leaves F2 with
only *Add your own command…* (and the scripts, in the desktop app).

The editor is the one **F4** uses ([Viewing and editing](view-and-edit.md)): in the terminal app
`editor`, `$VISUAL`, `$EDITOR` or `vi` (`notepad` on Windows); in the desktop app `editor`, or
the default application for `.toml` files. Editors that take `+line` (vi, Vim, Neovim, nano,
Emacs, micro, mg, joe, kak, mcedit and others) open at the `[[user_menu]]` line; others open
the file at the top. Once you save, the next **F2** lists the new entries; no restart.

In `command`:

| Placeholder | Becomes |
|---|---|
| `%f` | The name of the file under the cursor: the name only, since the command runs in its folder. Empty on `..` |
| `%d` | The panel's folder, as a full path |
| `%s` | The marked files as full paths, separated by spaces; when nothing is marked, the name under the cursor |
| `%%` | A single `%` |

Each is quoted for the shell when it needs it: `'it'\''s here.txt'` on Linux and macOS,
`"My File.txt"` on Windows. Names of only letters, digits and `-_./+,:@` stay as they are. So
names with spaces or quotes are safe; do not put your own quotes around a placeholder.

## Ready-made entries to copy

Paste any of these into `config.toml` (**F2**, **+**). Pick keys that are free in your menu.

```toml
[[user_menu]]
key = "z"
label = "Zip the marked files"
command = "zip -r marked.zip %s"
wait = true

[[user_menu]]
key = "o"
label = "Open the folder in VS Code"
command = "code %d"

[[user_menu]]
key = "a"
label = "tar.gz the folder"
command = "tar -czf ../\"$(basename \"$PWD\")\".tar.gz ."
wait = true

[[user_menu]]
key = "c"
label = "cargo test"
command = "cargo test"
wait = true
```

*tar.gz the folder* writes `<folder>.tar.gz` next to the folder, in its parent; it uses the
shell's `$PWD` and `$(…)`, so it is for Linux, the BSDs and macOS. On Windows, `tar` is there
too (Windows 10 and later): `command = "tar -czf ..\\folder.tar.gz ."`.

## What you see

- **The menu:** titled *Scripts* in the desktop app and *User menu* in the terminal app. Each row
  is the label with its key at the right; in the desktop app entries have a terminal icon, script
  files a script icon and *Add your own command…* a plus.
- **Running:** the same as a line typed on the [command line](command-line.md#what-you-see). In the
  terminal app the panels step aside, the terminal shows `folder> command` and its output, and
  with `wait = true` it ends with `-- press Enter --`. In the desktop app the status line says
  *Running git status…* and the preview pane then shows the output under the label and *Command output*.
  An entry without `wait` that prints nothing (*Open a terminal here*) shows no output.
- **Add your own command…:** the terminal app steps aside for the editor and comes back when it
  closes; the desktop app starts the editor in its own window.
- Afterwards both panels are read again.

## Settings and config.toml

No Settings item: the menu is edited in `config.toml`, reached with **F2**, **+**.

| Key | Type | Default | Means |
|---|---|---|---|
| `[[user_menu]]` `key` | text, one character | required (`""` for none) | The key that runs the entry inside the menu |
| `[[user_menu]]` `label` | text | required | What the menu shows |
| `[[user_menu]]` `command` | text | required | The shell command, with `%f` `%d` `%s` `%%` |
| `[[user_menu]]` `wait` | true/false | `false` | Terminal app: wait for **Enter** before the panels return. Desktop app: show the output even when there is none |
| `[[user_menu]]` `when` | `"file"` or `"git"` | none (always) | Offer the entry only on a file, or only in a git repository |
| `user_menu` left out | – | the entries above | No `[[user_menu]]` at all: the entries F2 comes with |
| `[keys]` `user_menu` | list of keys | `["F2"]` | The key that opens the menu |

The shell is the same as the command line's: `$SHELL -c` in the terminal app, `sh -c` in the
desktop app, `cmd /C` on Windows.

## In the terminal app

The same entries, the same keys and placeholders. Differences: the menu is titled *User menu*;
it lists no script files ([Scripts](scripts.md) are desktop-only); the command has the real
terminal, so `less`, `vim` and prompts work; and `wait = true` holds the output on screen until
you press **Enter**. The desktop app shows a command's output in the preview pane, and with
`wait = false` only when there is some. *Add your own command…* uses the terminal app's editor
there and the desktop app's `editor` (or the default application) here.

## Questions

#### Where did the git entries go?

*git log*, *git diff (file)* and *git blame (file)* used to be in F2. Git is built into both
apps now: **Ctrl+G** opens the history of the file or folder under the cursor as folders, one
per commit, with each commit's diff in the preview ([Git history as folders](../panels/git-history.md));
**Alt+B** lists the branches and **Alt+W** the worktrees ([Git branches](../panels/git-branches.md));
the git line at the bottom of the panel shows the branch and what changed ([Git in the panels](../panels/git.md)).
*git status* stays, as `s`, inside a repository. To have the others back, add them yourself:
**F2**, **+**, then for example `command = "git log --oneline --graph --decorate -50"`.

#### My entries replaced the built-in ones. How do I keep both?

A `[[user_menu]]` in your config replaces the whole built-in list. **F2**, **+** on a config
without entries writes them into it, commented out: take the `#` off those you want. Or copy
them from `coxswain --dump-config`.

#### Why is *Open a terminal here* not in my menu?

It is left out when there is nothing to open: on Linux, the BSDs and illumos when no terminal
from the list above is installed and `$TERMINAL` names none, or when there is no desktop
(neither `$WAYLAND_DISPLAY` nor `$DISPLAY` is set: a text console, ssh); in Termux, which is the
terminal already. Set `$TERMINAL` to yours (`export TERMINAL=footclient`), or add your own entry
with the command your terminal needs.

#### Why does *SHA-256 of the file* not show?

It is offered only with the cursor on a file: not on a folder, not on `..`, and not inside an
archive or a history, where the file is not on disk. And only when the system has a SHA-256 tool
(table above).

#### Why do *git status* and my own entries come and go?

An entry with `when = "git"` shows only when the panel's folder is in a git repository (a
`.git` in it or a folder above), and one with `when = "file"` only with the cursor on a file.
Entries without `when` are always there.

#### I added an entry but F2 does not show it.

Save the file: the next **F2** reads `config.toml` again. If the menu stays the same, the file
does not parse (the terminal app says why when it starts); a commented `# [[user_menu]]` line
does nothing until the `#` is gone. And once you have one entry of your own, the built-in ones
are gone unless you list them too.

#### Why does %f give only the name?

The command runs in the panel's folder, where the name is enough. Use `%d/%f` for the full
path, or `%s`, which gives full paths of the marked files.

#### What does %s give when nothing is marked?

The name under the cursor, as `%f` does. With files marked it gives their full paths, whatever
the cursor is on.

#### Should I put quotes around %f?

No. Coxswain already quotes each name when it needs it; your own quotes would end up inside the
command as part of the name. Write `git diff -- %f`, not `git diff -- "%f"`.

#### Can an entry have an upper-case key?

Yes: `key = "T"` runs with **Shift+T** in both apps, and `t` stays free for another entry. Keys
typed with **Shift** on your keyboard, such as `+` on many layouts, run as typed.

#### Can two entries have the same key, or none?

With the same key, the first one runs. With an empty `key = ""`, the entry has no key and is
picked with the arrows and **Enter**.

#### Why does `git blame` show all at once in the desktop app?

The desktop app gives commands no terminal, so `less` has nothing to page on and passes the text
through; the preview pane shows it all and you scroll there. In the terminal app `less` pages it.

#### Can I run a command on the other panel's folder?

There is no placeholder for it. Swap the panels (**Ctrl+U**) or make the other panel active
(**Tab**) first, or use an absolute path in the command.

---
[← Previous: The command line and its output](command-line.md) · [Next: Scripts →](scripts.md)

[← README](../../README.md) · [Docs index](../README.md) · [Commands, the user menu and scripts](README.md)

# The user menu, F2

**F2** opens a menu of commands, each with a key of its own: one keystroke opens a terminal in
the active panel's folder, shows a file's SHA-256 or runs `git status`, `cargo test` or a zip of
the marked files. It comes with three entries made for your system; the last row, *Add your own
command…* (`+`), opens `config.toml` where your own go. Placeholders put the file under the
cursor or the marked files into a command.

![The desktop app's F2 menu titled Scripts in ~/projects/rocket with the cursor on Cargo.toml: SHA-256 of the file (h) and git status (s), the scripts Make thumbnails and Upload with a script icon and no key, Open the scripts folder with a folder icon, and Add your own command… with the key +; there is no desktop in the sandbox, so Open a terminal here is left out](../screenshots/commands-user-menu.png)
![The terminal app in Classic blue (NC): the User menu box over the panels with SHA-256 of the file (h), git status (s), the scripts Make thumbnails and Upload with no key, Open the scripts folder, and Add your own command… (+); with no desktop in the sandbox, Open a terminal here is left out](../screenshots/commands-user-menu-tui.png)

- [How to use it](#how-to-use-it)
- [The entries it comes with](#the-entries-it-comes-with)
- [Choosing the terminal](#choosing-the-terminal)
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
| **F2** | Opens *Scripts*: the entries, then the [script files](scripts.md), *Open the scripts folder* and *Add your own command…* | Opens *User menu*: the same rows |
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
| `t` | Open a terminal here | A new terminal window in the active panel's folder: the one you chose ([below](#choosing-the-terminal)), else the system's | When a terminal is found (below) |
| `h` | SHA-256 of the file | The SHA-256 of the file under the cursor, shown with its name (`wait`) | On a file |
| `s` | git status | `git status` (`wait`) | In a git repository, with git installed |

The commands per system when no terminal is chosen (the first one installed is taken):

| System | Open a terminal here | SHA-256 of the file |
|---|---|---|
| FreeBSD | `$TERMINAL`, else the first of `x-terminal-emulator`, foot, Alacritty, kitty, WezTerm, Ghostty, Konsole, GNOME Terminal, Ptyxis, Console (`kgx`), Xfce Terminal, MATE Terminal, LXTerminal, QTerminal, Terminator, Tilix, urxvt, st, xterm | `sha256 -- %f`, else `sha256sum -- %f`, else `cksum -a sha256 -- %f` |
| NetBSD, OpenBSD | the same | the same |
| illumos | the same | `digest -v -a sha256 -- %f`, else `sha256sum` |
| Linux | the same | `sha256sum -- %f`, else `shasum -a 256 -- %f` |
| Android (Termux) | not offered: Termux is the terminal | `sha256sum -- %f` |
| macOS | `open -a Terminal %d` | `shasum -a 256 -- %f` |
| Windows | Windows' *Default terminal application*: Windows Terminal (`start "" wt -d .`) or the console (`start "" powershell`); when it is *Let Windows decide*, Windows Terminal when installed, else the console | `certutil -hashfile %f SHA256` |

On the BSDs, illumos and Linux the terminal is only offered on a desktop (`$WAYLAND_DISPLAY` or
`$DISPLAY` set), not on a text console or over ssh. It is started with `nohup … &`, so it stays
open when Coxswain quits. `$TERMINAL` is taken when the program it names is installed. A
terminal Coxswain knows is told the folder with its own flag (below), so one that keeps a
single process running (kitty, WezTerm, GNOME Terminal) opens there too.

## Choosing the terminal

*Open a terminal here*, and *Open in your git tool* (**Alt+G**) when it opens a terminal, use the
terminal you choose under **Settings → Behaviour → Programs → Terminal**, in both apps. The list
holds *Automatic: …* (what opens when none is chosen, named), the terminals found on this
machine, and *Another command…*, which shows a field (desktop app) or a line to type in
(terminal app) for a command of your own. In `config.toml` it is the top-level key `terminal`:
a name from the table below (any case; the name Settings shows works too), or a command, `{dir}`
being the folder. Leave it out for *Automatic*.

| System | Found | Name | Opens in the folder with |
|---|---|---|---|
| FreeBSD, the other BSDs, illumos, Linux | on `PATH` | `foot` | `foot --working-directory=<folder>` |
| | | `alacritty` | `alacritty --working-directory <folder>` |
| | | `kitty` | `kitty --directory <folder>` |
| | | `wezterm` | `wezterm start --cwd <folder>` |
| | | `ghostty` | `ghostty --working-directory=<folder>` |
| | | `konsole`, `qterminal` | `--workdir <folder>` |
| | | `gnome-terminal`, `ptyxis`, `kgx`, `xfce4-terminal`, `mate-terminal`, `lxterminal`, `terminator`, `tilix` | `--working-directory=<folder>` |
| | | `urxvt` | `urxvt -cd <folder>` |
| | | `st` | `st -d <folder>` |
| | | `xterm`, `x-terminal-emulator` | started in the folder |
| macOS | in `/Applications` or `~/Applications` (Terminal always) | `terminal` (Terminal) | `open -a Terminal <folder>` |
| | | `iterm2` (iTerm2) | `open -a iTerm <folder>` |
| | the app there, else on `PATH` | `ghostty`, `alacritty`, `kitty`, `wezterm` | `open -na Ghostty --args --working-directory=<folder>` and the like, or the program on `PATH` with the flags above |
| Windows | on `PATH` (cmd and Windows PowerShell always) | `wt` (Windows Terminal) | `wt -d .` |
| | | `pwsh` (PowerShell 7) | `pwsh -WorkingDirectory .` in a new console |
| | | `powershell` (Windows PowerShell), `cmd` | started in the folder, in a new console |
| | | `alacritty`, `wezterm` | `alacritty --working-directory .`, `wezterm start --cwd .` |

On Windows the folder is always `.`, the folder the terminal starts in, so no folder name is
ever on a line that `cmd.exe` or Windows Terminal reads: a name with `&` in it could run what
follows. A command of your own gets `.` for `{dir}` there too; elsewhere `{dir}` is the folder,
quoted for the shell.

When *Open in your git tool* runs a terminal tool (lazygit, tig) in a new window, it is put after
the terminal's own flag for a program (`-e`, `--`, `-x`, or none for kitty and foot); on a Mac
Terminal and iTerm2 type the line into their new window. With a command of your own, the tool
follows your command after the flag its first word's terminal takes (`-e` for one Coxswain does
not know).

```toml
terminal = "ghostty"                      # by name
# terminal = "footclient -D {dir}"        # a command of your own
```

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
`editor`, `$VISUAL` or `$EDITOR`, else the [built-in editor](editor.md); in the desktop app `editor`, or
the default application for `.toml` files. Editors that take `+line` (vi, Vim, Neovim, nano,
Emacs, micro, mg, joe, kak, mcedit and others) open at the `[[user_menu]]` line; others open
the file at the top. Once you save, the next **F2** lists the new entries; no restart.

In `command`:

| Placeholder | Becomes |
|---|---|
| `%f` | The name of the file under the cursor: the name only, since the command runs in its folder. A name that starts with `-` comes as `./-name` (`.\-name` on Windows), so the program does not take it for an option. Empty on `..` |
| `%d` | The panel's folder, as a full path; on Windows `.`, the same folder, as the command runs there |
| `%s` | The marked files as full paths, separated by spaces; when nothing is marked, the name under the cursor |
| `%%` | A single `%` |

Each is quoted for the shell when it needs it: `'it'\''s here.txt'` on the BSDs, illumos, Linux and macOS.
Names of only letters, digits and `-_./+,:@` stay as they are. On Windows a name that needs
quotes goes in as `"%COXSWAIN_0%"`, a variable that holds it: cmd.exe expands a `%PATH%` even
inside quotes, so a file named `%USERPROFILE%.txt` would otherwise become another name. The
terminal app shows the line with the names in. So
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
shell's `$PWD` and `$(…)`, so it is for the BSDs, illumos, Linux and macOS. On Windows, `tar` is there
too (Windows 10 and later): `command = "tar -czf ..\\folder.tar.gz ."`.

## What you see

- **The menu:** titled *Scripts* in the desktop app and *User menu* in the terminal app. Each row
  is the label with its key at the right; in the desktop app entries have a terminal icon, script
  files a script icon, *Open the scripts folder* a folder and *Add your own command…* a plus.
  [Script files](scripts.md) and *Open the scripts folder* have no key.
- **Running:** the same as a line typed on the [command line](command-line.md#what-you-see). In the
  terminal app the panels step aside, the terminal shows `folder> command` and its output, and
  with `wait = true` it ends with `-- press Enter --`. In the desktop app the status line says
  *Running git status…* and the preview pane then shows the output under the label and *Command output*.
  An entry without `wait` that prints nothing (*Open a terminal here*) shows no output.
- **Add your own command…:** the terminal app steps aside for the editor and comes back when it
  closes; the desktop app starts the editor in its own window.
- Afterwards both panels are read again.

## Settings and config.toml

The menu is edited in `config.toml`, reached with **F2**, **+**. The terminal *Open a terminal
here* opens is the one Settings item: **Settings → Behaviour → Programs → Terminal**, the key
`terminal` ([Choosing the terminal](#choosing-the-terminal)).

| Key | Type | Default | Means |
|---|---|---|---|
| `[[user_menu]]` `key` | text, one character | required (`""` for none) | The key that runs the entry inside the menu |
| `[[user_menu]]` `label` | text | required | What the menu shows |
| `[[user_menu]]` `command` | text | required | The shell command, with `%f` `%d` `%s` `%%` |
| `[[user_menu]]` `wait` | true/false | `false` | Terminal app: wait for **Enter** before the panels return. Desktop app: show the output even when there is none |
| `[[user_menu]]` `when` | `"file"` or `"git"` | none (always) | Offer the entry only on a file, or only in a git repository |
| `user_menu` left out | – | the entries above | No `[[user_menu]]` at all: the entries F2 comes with |
| `[keys]` `user_menu` | list of keys | `["F2"]` | The key that opens the menu |
| `terminal` | text | none (automatic) | The terminal *Open a terminal here* opens: a name (`"ghostty"`, `"wt"`) or a command, `{dir}` the folder |

The shell is `sh -c` on Linux, the BSDs, illumos, Termux and macOS, in both apps, whatever your
`$SHELL` is, and `cmd /S /C` on Windows: the names are quoted for `sh`, and fish, nushell, xonsh
or elvish read quotes otherwise, so a file name could end the quoting and run what follows it.
Only the line you type on the terminal app's [command line](command-line.md#the-shell) runs in
your `$SHELL`.
The same holds for **F3** and **F4** with a viewer or editor of your own, **Enter** on a program,
and a command of your own in `[git] tool`.

## In the terminal app

The same entries, the same keys and placeholders. Differences: the menu is titled *User menu*;
it lists the [script files](scripts.md) too, without icons; the command has the real
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
**Alt+C** shows the git config ([Git config](../panels/git-config.md));
the git line at the bottom of the panel shows the branch and what changed ([Git in the panels](../panels/git.md)).
*git status* stays, as `s`, inside a repository. To have the others back, add them yourself:
**F2**, **+**, then for example `command = "git log --oneline --graph --decorate -50"`.

#### My entries replaced the built-in ones. How do I keep both?

A `[[user_menu]]` in your config replaces the whole built-in list. **F2**, **+** on a config
without entries writes them into it, commented out: take the `#` off those you want. Or copy
them from `coxswain --dump-config`.

#### Why is *Open a terminal here* not in my menu?

It is left out when there is nothing to open: on the BSDs, illumos and Linux when no terminal
from the list above is installed and `$TERMINAL` names none, or when there is no desktop
(neither `$WAYLAND_DISPLAY` nor `$DISPLAY` is set: a text console, ssh); in Termux, which is the
terminal already. Choose yours under **Settings → Behaviour → Programs → Terminal**
(*Another command…* takes any command, `{dir}` the folder), or set `$TERMINAL`
(`export TERMINAL=footclient`).

#### How do I make Coxswain open Ghostty?

**Settings → Behaviour → Programs → Terminal**, then *Ghostty* from the list: it is there when
Ghostty is installed (on a Mac in `/Applications` or `~/Applications`, elsewhere on `PATH`). The
setting is saved at once; **F2**, **t** then opens Ghostty in the panel's folder
(`--working-directory=`), and *Open in your git tool* opens lazygit or tig in Ghostty too. In
`config.toml` it is `terminal = "ghostty"`. Any other terminal works the same way, or with
*Another command…* and a command of your own, `{dir}` being the folder.

#### I chose a terminal and Windows still opens another one.

A console program (PowerShell, cmd) opens in whatever Windows' *Default terminal application*
says (Settings → System → For developers → Terminal in Windows 11): with *Windows Terminal* there,
choosing `powershell` opens PowerShell in a Windows Terminal tab. Choose *Windows Console Host*
there for a console window of its own, or choose Alacritty or WezTerm in Coxswain, which open
their own windows.

#### Why does my terminal open in my home folder and not in the panel's folder?

A terminal that keeps one process running for all its windows (a `footclient` server, kitty
with `single-instance`, WezTerm with a GUI already open) starts each window in its own folder
unless it is told the folder. Coxswain tells the terminals it knows with their own flag (the
table above); for a command of your own put `{dir}` where your terminal takes the folder, as in
`footclient -D {dir}`.

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

#### My entry uses fish (or zsh) syntax and fails.

Entries run in `sh`, whatever your `$SHELL` is: the names that `%f`, `%d` and `%s` put in are
quoted for `sh`, and another shell would read those quotes otherwise. Call your shell yourself
and pass the names as arguments: `command = "fish -c 'for f in $argv; echo $f; end' -- %s"`.

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

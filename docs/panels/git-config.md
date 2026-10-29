[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Git config

**Alt+C** shows the git config as git uses it in the active panel's folder: every key with
the value that counts, its scope (system, global, local, worktree, command line) and the file
it comes from. Values set earlier are dimmed below the one git uses, and passwords and tokens
are hidden until you ask. Coxswain only reads the config; **Alt+G** opens your own git tool to
change it. Both apps do this.

![The desktop app's Git config of rocket dialog in Cyber: [core], [include], [init], [remote "github"], [remote "origin"], [sendemail] with smtppass shown as •••• and a Show button, and [user] with email demo@rocket.example.org from local above a dimmed demo@example.com, global · set earlier; each line with its scope, its file, Copy and Open in editor, and under the list Coxswain only reads the config: Alt+G opens your git tool to change it](../screenshots/git-config.png)

![The terminal app in Classic blue: Git config of rocket, full screen, the line •••• hides a password or token under the title, [alias] lines from the included .gitconfig-aliases, the scope and file of each line on the right, a hidden smtppass, the dimmed global email under the local one, and the key line Enter show or hide · F4 open its file · F5 copy the line · Alt+G open in your git tool · Esc close](../screenshots/tui-git-config.png)

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Hidden secrets](#hidden-secrets)
- [Opening a file, copying a line](#opening-a-file-copying-a-line)
- [Changing the config](#changing-the-config)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. In a folder, press **Alt+C**. It is also *Git config* in **F9**, in the action menu
   (**Shift+F10**) in a repository, in the git line's menu (a right-click on the git line in the
   desktop app's pane footer, whose tooltip names **Alt+C** too) and under *Git* in the
   desktop app's preview pane.
2. The dialog (the desktop app) or the full screen (the terminal app) is titled *Git config of
   rocket*, or *Git config outside a repository* in a folder of no repository.
3. **Up** and **Down** move the cursor from line to line. **Enter** shows or hides a hidden
   value; **F4** opens the line's file in your editor; **Ctrl+C** (desktop app) or **F5**
   (terminal app) copies the line.
4. **Esc** closes it.

The config is read once, when it opens, with
`git config --list --show-origin --show-scope`. Press **Alt+C** again after a change to see it
anew.

## What you see

The keys are grouped under their section, sorted: `[branch "master"]`, `[core]`,
`[remote "origin"]`, `[user]`. Each line has:

| Part | What it is |
|---|---|
| The name | The key's last part: `url` under `[remote "origin"]` |
| The value | What it is set to; a key written without `=` (`bare`) shows `true`, as git reads it |
| The scope | *system*, *global*, *local*, *worktree* or *command line*, with *included* when it comes from a file another one includes (`include.path`, `includeIf`) |
| The file | The file it was read from, in full (hover it in the desktop app); *the command line* for `git -c` and `GIT_CONFIG_COUNT` values |

The line that counts comes first. When a key is set more than once (in your global config and
again in the repository's, say), the earlier values follow it **dimmed**, with *set earlier* in
their scope: git uses the last one it reads, and it reads system, global, local, worktree and
the command line in that order. For a key with several values, such as `remote.origin.fetch`
or `include.path`, git uses every one; they are listed the same way.

Outside a repository there is no local or worktree config: the view says *Not in a repository:
the system and global config only*.

## Hidden secrets

A value that looks like a secret shows as `••••`:

| What | Shown as |
|---|---|
| The user and password in an http(s) URL, since a token can stand as either (`https://ghp_…:x-oauth-basic@github.com`) | `https://••••@example.org/r.git` |
| The password in any other URL | `ftp://ada:••••@example.org/f` |
| A token in a URL's query (a key with *token*, *secret*, *pass*, *key*, *sig*, *auth*, *jwt* or *ticket* in it, or ending in *pat*) | `https://h/r.git?access_token=••••` |
| An `http.extraHeader` value (also `http.<url>.extraHeader`) | `Authorization: ••••`: the header's name stays |
| A key whose last part ends in *pass*, *password*, *token*, *secret*, *apikey*, *credential*, *pat* or *bearer* | `••••` |
| A `credential.helper` written out as a shell line holding a password, a token, `--pat`, a bearer or a long word of letters and digits that looks like one | `••••` |

A URL with a user or password in a section's name (`url.https://ada:pw@host/.insteadOf`) has it
hidden in the heading too. **Enter** on the line (or its *Show* button in the desktop app) shows the
value; **Enter** again (*Hide*) hides it. The first few times a hidden value is there, a line
under the title says so.

## Opening a file, copying a line

- **F4** (or *Open in editor* on the line) opens the line's file in your editor, the one F4 uses
  everywhere ([View and edit](../commands/view-and-edit.md)). The terminal app closes the view
  while the editor runs. A value from the command line or a blob has no file: the status line
  says *This value comes from the command line, not from a file: there is nothing to open in an
  editor*, and the desktop app's button is greyed out.
- **Ctrl+C** in the desktop app (or *Copy*), **F5** in the terminal app, copies the line as
  `key=value`, the form `git -c` takes. A hidden value is copied hidden (`••••`) until you show
  it; the status line says *Copied:* and the line, hidden. The terminal app copies through the
  terminal (OSC 52), over ssh too.

## Changing the config

Coxswain never writes the git config: it shows version control and leaves the writing to your
own tool ([Open in your git tool](git.md#open-in-your-git-tool)). The line under the list says
*Coxswain only reads the config: Alt+G opens your git tool to change it*, and **Alt+G** (or the
*Open in your git tool* button) does that from the view. **F4** on a line opens its file to
edit by hand.

## Settings and config.toml

| What | config.toml | Default |
|---|---|---|
| The git config view | `git_config` in `[keys]` | `["Alt+C"]` |

Bound to *Git config*, **Alt+C** no longer starts a [quick search](quick-search.md) for "c";
set `git_config = []` to free it. It needs `git` on your `PATH`.

## In the terminal app

The same, full screen: the headings in brackets (`[core]`), each line's scope and file on the
right, the dimmed earlier values, and at the bottom the line about changing it and the keys:
*Enter show or hide · F4 open its file · F5 copy the line · Alt+G open in your git tool · Esc
close*. **F1** in it opens Features at *Git config*. There is no git line to right-click: use
**Alt+C**, **F9** or the action menu.

## Questions

#### Where does this value come from?

The right side of its line: the scope (*global*, *local*, …) and the file. *included* means a
file another one includes; *the command line* means `git -c` or `GIT_CONFIG_COUNT` in
Coxswain's environment.

#### Why are some lines dimmed?

They were set earlier, in a lower scope or an included file, and the line above them is the
one git uses. A key with several values, such as `remote.origin.fetch`, uses them all; those
are dimmed the same way.

#### Can I change a value here?

No. Coxswain only reads the config. **F4** on the line opens its file in your editor, and
**Alt+G** opens your own git tool. Press **Alt+C** again afterwards to see the change.

#### Why does a value show as ••••?

It looks like a password or token: in a URL, an `extraHeader`, or a key named like *password*,
*token* or *secret*. **Enter** (or *Show* in the desktop app) shows it on that line. A copy takes
it hidden until then.

#### Why do I only see system and global values?

The folder is in no repository, so there is no local or worktree config; the view says so
under its title. Open a folder of the repository and press **Alt+C** there.

#### Why does F4 do nothing on a line?

That value comes from the command line (or a blob), not a file, so there is nothing to open;
the status line says so. Unset `GIT_CONFIG_COUNT` or the `-c` you started Coxswain with to be
rid of it.

#### Alt+C used to start a quick search for "c".

It is bound to *Git config*. Set `git_config = []` in `[keys]` to give it back to quick search,
or start the quick search with another letter.

---
[← Previous: Git branches and worktrees](git-branches.md) · [Next: Fetch and pull →](fetch-and-pull.md)

[← README](../README.md) · [Docs index](README.md)

# What's new in 2.0

Coxswain 2.0 sums up the 1.x series: what changed between 1.0 and 2.0, in plain words, with a
link to each feature's page. The full list, version by version, is the changelog at the bottom of
the [README](../README.md#changelog).

## Contents

- [Coxswain runs on FreeBSD](#coxswain-runs-on-freebsd)
- [One Find](#one-find)
- [Settings by task, in both apps](#settings-by-task-in-both-apps)
- [Guided setup](#guided-setup)
- [Search by meaning reads whole documents](#search-by-meaning-reads-whole-documents)
- [The Mac's GPU](#the-macs-gpu)
- [Git branches and worktrees](#git-branches-and-worktrees)
- [26 languages](#26-languages)
- [Cloud files are safe](#cloud-files-are-safe)
- [New in 2.0 itself](#new-in-20-itself)
- [Changed in 2.0: renamed keys and Settings names](#changed-in-20-renamed-keys-and-settings-names)
- [Questions](#questions)

## Coxswain runs on FreeBSD

Coxswain runs on FreeBSD: the terminal app and the desktop app. Both are built for FreeBSD 14 and
15 on amd64 with every release, and one line installs them:

```sh
fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh
```

The script is plain `sh`. It installs under `/usr/local` (or `~/.local` for you alone), shows each
`pkg install` before it runs it, and checks every download against its SHA-256 sum. You get
`coxswain(1)` as a manual page, and with `/usr/local` an rc.d script, `coxswain_index`, that runs
the search helper from boot once you `sysrc coxswain_index_enable=YES`. The terminal app needs
nothing beyond the base system and is a first-class build; the desktop app needs WebKitGTK from
packages and is marked experimental for now. Hints that name a package say `pkg install`.

[FreeBSD](reference/freebsd.md)

## One Find

**Ctrl+F** (or **Alt+F7**) opens one field for everything: file names on the whole machine, the
words inside your files, files about your words in any language, and your git history. The hits
come in groups (*Names*, *In files*, *About this*, *History*) ranked by words and meaning together.
**Tab** picks one kind, one key limits the search to this folder, and **Ctrl+Enter** asks your
files a question, answered in place by your own chat model with numbered sources.

[Find](search/find-file.md) · [Ask](search/ask.md)

## Settings by task, in both apps

Settings is ordered by what you come for: *Overview*, *Finding files*, *Previews*, *Looks*,
*Behaviour*, *Keys*, *Privacy and updates*, with *Find a setting* above them. Every option says
what it does and what it costs (disk, processor, network, data that leaves the machine), and is
saved to `config.toml` at once with your comments kept. The terminal app has the same Settings,
full screen: **F9** → *Settings*, or `coxswain --settings`.

[The Settings window](customise/settings.md)

## Guided setup

Search by meaning and Ask are set up with a guide, in both apps: it finds the model servers on
your machine (Ollama, Lemonade, LM Studio, llama.cpp and others), suggests models by what they can
do and by your hardware, tests a question, checks the GPU, and can start the search helper with
your session. Settings → *Overview* → *Set up search by meaning and Ask*, or
`coxswain --setup-search`.

[Smart search in a few minutes](search/setup.md)

## Search by meaning reads whole documents

Every part of a file gets read for meaning (up to about 25,000 words), not only its beginning, so
a fact on page 40 of a PDF is found and Ask can answer from it.

[Search by meaning](search/meaning.md)

## The Mac's GPU

On Apple Silicon the built-in model runs on the GPU through Metal, several times faster, after a
check that its results match the CPU's.

[Search by meaning](search/meaning.md)

## Git branches and worktrees

**Alt+B** lists a repository's branches as folders, **Enter** browses a branch's files,
**Alt+S** switches to it (git refuses to lose your changes), **Alt+W** lists the worktrees. The
git line names a worktree, a detached HEAD and a merge or rebase under way. **Ctrl+G** still lists
a file's or a folder's history.

[Git branches and worktrees](panels/git-branches.md) · [Git history](panels/git-history.md)

## 26 languages

From Danish to Korean, with flags in Settings, right to left in Hebrew, and wide letters lined up
in the terminal app.

[Languages](customise/languages.md)

## Cloud files are safe

OneDrive, Dropbox, Google Drive, Proton Drive and iCloud files that are only online are found by
name, marked with a cloud, and never downloaded unless you open one.

[Cloud files](search/cloud-files.md)

## New in 2.0 itself

- **A first-run guide** in both apps: the two panels and their keys, how far Find looks, theme,
  language and icons (with a Nerd Font check), and what can leave the machine. Shown once,
  skippable on every step, and opened again from Help (**F1**) or Settings → *Overview*.
  [The first-run guide](panels/first-run.md)
- **Install lines for what is missing.** When tesseract, pdftoppm, LibreOffice, LaTeX, PlantUML or
  pandoc is missing, the hint gives the exact command for your system (`pkg install` on FreeBSD,
  `pacman`, `apt`, `dnf` or `zypper` on Linux, `brew` on macOS, `winget` on Windows) and a **Copy**
  button. Coxswain never runs it. [Scans and pictures](search/scans.md) ·
  [Previews made by tools](previews/tools.md)
- **Dialogs say what they do.** The title says what and how many ("Copy 3 items"), the button is
  the verb (*Copy*, *Move*, *Create*, *Pack*, *Extract*), and one key line under it reads the same
  in both apps: "Enter Copy · Esc Cancel". An error says what failed ("Could not copy
  budget.txt"), its cause in one line, and the full text under *Details*. [Copy](files/copy.md)
- **One word for one thing**: you *mark* files (never select them), everything is a *folder*,
  the search box is *Find*, Ctrl+R is *Refresh*. The desktop app's key bar says *Move*, *New
  folder* and *Commands*; the terminal app keeps Norton Commander's *RenMov*, *Mkdir* and
  *PullDn*.

## Changed in 2.0: renamed keys and Settings names

A few names in `config.toml` now use the word the apps use. **On the first start of 2.0, both apps
rewrite your `config.toml` once**, keeping your comments and the order of your keys, and a notice
lists what changed. Nothing else in the file is touched.

| Where | Before 2.0 | From 2.0 |
|---|---|---|
| `[keys]` | `select_group` | `mark_group` |
| `[keys]` | `unselect_group` | `unmark_group` |
| `[keys]` | `invert_selection` | `invert_marks` |
| `[keys]` | `mkdir` | `new_folder` |
| `[keys]` | `dir_sizes` | `folder_sizes` |
| `[search]` | `roots` | `name_roots` |
| `[search]` | `exclude` | `name_exclude` |

The old names are not read any more. A config that cannot be written (read-only, or kept in a
store that is) is left as it is, and the app says on its error output which keys to rename.

`--settings=NAME` takes an area (`overview`, `search`, `previews`, `looks`, `behaviour`, `keys`,
`privacy`) or an option's name. The section names of before the areas are gone:

| Before 2.0 | From 2.0 |
|---|---|
| `--settings=meaning` | `--settings=search_meaning` |
| `--settings=ask` | `--settings=ask_model` |
| `--settings=news` | `--settings=overview` |
| `--settings=cloud` | `--settings=search_cloud` |

[Configuration: every key](reference/configuration.md#renamed-in-20) ·
[Command-line flags](reference/command-line-flags.md)

## Questions

**Do I have to do anything to update to 2.0?** No. Start it: your `config.toml` is updated once and
a notice says what changed. Scripts that start the apps with `--settings=meaning`, `ask`, `news`
or `cloud` need the new names above.

**I keep my config.toml in a dotfiles repository.** The rewrite follows a link to the real file
and changes only the renamed keys; commit the change. A machine still on 1.x does not know the new
names: update it too.

**My config is read-only (Nix, a managed home).** Rename the keys of the table above in its
source. Until then the terminal app stops with the name of the first old key, and the desktop app
starts with the defaults and says why on its error output.

**Where do I see what else is new?** Settings → *Overview* in the desktop app; `coxswain
--whats-new` (or `--whats-new all`) in the terminal app; the changelog in the
[README](../README.md#changelog).

---
[← Previous: README](../README.md) · [Next: Docs index →](README.md)

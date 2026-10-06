[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Flatpak

The desktop app, `coxswain-gui`, comes as a Flatpak with the id `io.github.mwo_dk.Coxswain`,
built from source on the GNOME runtime (which brings the WebKitGTK it draws with). It is the
same app as the `.deb`, `.rpm` and AppImage, with the same keys and the same `config.toml` keys;
this page says what the sandbox changes and why. The terminal app is not in the Flatpak: it is
one static binary that needs nothing, so the [install table](../../README.md#install) has the
ways to get it.

The Flatpak is **not on Flathub yet**. Until it is, build it yourself
([below](#building-it-yourself)); it takes about twenty minutes.

## Contents

- [Installing](#installing)
- [What the sandbox lets it do, and why](#what-the-sandbox-lets-it-do-and-why)
- [Your programs run on the host](#your-programs-run-on-the-host)
- [The search helper](#the-search-helper)
- [Where its files are kept](#where-its-files-are-kept)
- [How it differs from the other Linux packages](#how-it-differs-from-the-other-linux-packages)
- [Updating](#updating)
- [Uninstalling](#uninstalling)
- [Building it yourself](#building-it-yourself)
- [Questions](#questions)

## Installing

Once Flathub lists it:

```sh
flatpak install flathub io.github.mwo_dk.Coxswain
flatpak run io.github.mwo_dk.Coxswain
```

or *Coxswain* in GNOME Software, KDE Discover or another software centre that has Flathub. It
then sits in your desktop's menu as *Coxswain*, and `flatpak run io.github.mwo_dk.Coxswain
~/projects` starts it in a folder, the way `coxswain-gui ~/projects` does.

## What the sandbox lets it do, and why

A Flatpak gets only what its manifest asks for. Coxswain asks for these:

| Permission | Why |
|---|---|
| `--filesystem=host` | A file manager works on all your files: the panels, copy and move, the trash, Find, previews. Without it every folder would need a portal dialog first |
| `--talk-name=org.freedesktop.Flatpak` | `flatpak-spawn --host`: your editor, shell commands, scripts and the programs previews use run on the host ([next section](#your-programs-run-on-the-host)) |
| `--share=network` | The daily [update check](updates.md) (it can be turned off) and, when you turn them on, the model download and a server for [search by meaning](../search/meaning.md). Nothing else goes out ([Privacy](privacy.md)) |
| `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--share=ipc` | The window, drawn on the GPU |
| `--socket=pulseaudio` | Sound and video in the preview pane |

`--filesystem=host` is your home, `/media`, `/mnt`, `/run/media` and the rest of the file
system, but not the system's own `/usr`, `/etc` or `/tmp`: the app sees the GNOME runtime there.
The host's `/usr` is visible under `/run/host/usr`, which is how Coxswain finds your programs.

## Your programs run on the host

Inside the sandbox are only the GNOME runtime, Coxswain and git. Everything you start yourself
runs on the host instead, through `flatpak-spawn --host`, so it is your program with your
`PATH`, your configuration and your terminal:

| What | Key | Runs |
|---|---|---|
| The editor | **F4** | `editor` from `config.toml`, through `sh -c` on the host; without `editor`, the file opens in its default application |
| A command typed in the command line | **Enter** | `sh -c` on the host, in the active panel's folder; its output shows as it does outside a Flatpak |
| [The user menu](../commands/user-menu.md) and [scripts](../commands/scripts.md) | **F2** | On the host, in the active panel's folder, with the marked files as arguments |
| Deleting to the trash | **F8** | `gio trash` on the host, so the files land in your desktop's trash; the Flatpak's own data folder would hold a trash nothing shows |
| Opening a file | **Enter**, **F3** → *Open in its app* | The desktop's portal, which opens the default application or asks which one |
| tesseract, pdftoppm, LibreOffice, pandoc, LaTeX, PlantUML, podman or docker, duckdb | Previews, Find inside files | On the host when they are installed in `/usr/bin` or `/usr/local/bin` there, at the lowest priority, and stopped with Coxswain when they run past their time limit |
| git | The panels' status, **History**, branches | Inside the sandbox: Coxswain bundles it, so the settings that keep a repository's own config from running programs still hold |

A preview tool installed somewhere else (Homebrew, `~/.local/bin`) is not found from the
Flatpak; the preview pane says it is not installed, as it does when it is missing. PlantUML gets
its allowlist (`PLANTUML_SECURITY_PROFILE=ALLOWLIST`, only the diagram's own folder) on the host
too. Temporary files go to `~/.var/app/io.github.mwo_dk.Coxswain/cache/tmp`, which the host's
programs see at the same path; the sandbox's `/tmp` is its own.

## The search helper

The [search helper](../search/helper.md) runs inside the sandbox, started by the first window as
`coxswain-gui --index-helper`, and leaves a while after the last window has gone, as it does
outside a Flatpak.

*Settings → Finding files → Details → Background reading → Start with my session* cannot reach
systemd from the sandbox, so it writes an XDG autostart entry on the host instead,
`~/.config/autostart/coxswain-index.desktop`, which runs
`flatpak run --command=coxswain-gui io.github.mwo_dk.Coxswain --index-helper --stay` when you
log in. It starts the version that is installed, so an update needs nothing. Unticking it
removes the entry; the helper running then stays until you log out, as on FreeBSD.
`flatpak ps` lists `io.github.mwo_dk.Coxswain` while it runs.

## Where its files are kept

A Flatpak keeps its files in its own folder, not in `~/.config` and `~/.cache`:

| What | In the Flatpak | Outside it |
|---|---|---|
| `config.toml`, scripts | `~/.var/app/io.github.mwo_dk.Coxswain/config/coxswain/` | `~/.config/coxswain/` |
| Name index, search store, previews | `~/.var/app/io.github.mwo_dk.Coxswain/cache/coxswain/` | `~/.cache/coxswain/` |
| The model for search by meaning, notes, tags | `~/.var/app/io.github.mwo_dk.Coxswain/data/coxswain/` | `~/.local/share/coxswain/` |

So the Flatpak and a terminal app installed another way do not share a config, an index or
tags. To use the config you have, copy it once:
`cp ~/.config/coxswain/config.toml ~/.var/app/io.github.mwo_dk.Coxswain/config/coxswain/`.
*Settings → Privacy and updates* and [Where things are kept](where-things-are-kept.md) show the
paths the running app uses.

## How it differs from the other Linux packages

| | Flatpak | `.deb`, `.rpm`, AppImage |
|---|---|---|
| WebKitGTK, GStreamer | From the GNOME runtime, updated by Flathub | From your distribution (the AppImage brings its own) |
| Updates | `flatpak update`, or the software centre | The package manager, or a new download |
| The update notice | *Coxswain 2.3.0 is available: flatpak update io.github.mwo_dk.Coxswain* | Points to the releases page |
| Start with my session | An XDG autostart entry | A systemd user service |
| Config and index | Under `~/.var/app/io.github.mwo_dk.Coxswain/` | `~/.config/coxswain/`, `~/.cache/coxswain/` |
| Preview programs | The host's, from `/usr/bin` and `/usr/local/bin` | Anything on `PATH` |
| The terminal app | Not included | Not included either: it has packages of its own |

## Updating

Flathub publishes each release a little after it is on the [releases
page](https://github.com/mwo-dk/coxswain/releases/latest). Update it with everything else:

```sh
flatpak update
```

or only Coxswain with `flatpak update io.github.mwo_dk.Coxswain`, which is the command the
update notice shows at the right of the command line. GNOME Software and Discover update it on
their own schedule.

## Uninstalling

```sh
flatpak uninstall io.github.mwo_dk.Coxswain
```

leaves your config and index in `~/.var/app/io.github.mwo_dk.Coxswain/`; add `--delete-data`
to remove them too. Untick *Start with my session* first, or delete
`~/.config/autostart/coxswain-index.desktop`, so nothing tries to start it at login.

## Building it yourself

The manifest is `packaging/flatpak/io.github.mwo_dk.Coxswain.yml`. It builds the checkout it is
in with no network: Cargo's and npm's downloads come from two files that
`packaging/flatpak/sources.sh` writes from the lock files (it needs `python3` and `git`).

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Sdk//51 org.gnome.Platform//51 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08 org.freedesktop.Sdk.Extension.node24//26.08 \
  org.flatpak.Builder
git clone https://github.com/mwo-dk/coxswain && cd coxswain
sh packaging/flatpak/sources.sh
flatpak run org.flatpak.Builder --user --install --force-clean \
  packaging/flatpak/build packaging/flatpak/io.github.mwo_dk.Coxswain.yml
flatpak run io.github.mwo_dk.Coxswain
```

CI builds it the same way on every change that goes into it (the *Flatpak* workflow), runs
Flathub's linter on it, installs it and starts it on a display of its own.

## Questions

**Why does it need `--filesystem=host`? Is that not the whole point of a sandbox?**
A file manager's job is your files, all of them: a portal dialog for every folder would make the
panels useless. What the sandbox still gives you is a known runtime and no access to the
system's `/usr` and `/etc`. Coxswain itself never sends your files anywhere
([Privacy](privacy.md)).

**F4 does nothing, or says *sh: hx: not found*.**
The editor runs on the host, so it has to be installed there and on the host's `PATH`. A
terminal editor (`hx`, `vim`, `nano`) needs a terminal around it: set
`editor = "kitty -e hx"` (or your terminal's equivalent) in `config.toml`, as outside a Flatpak.

**A preview says *tesseract is not installed*, but it is.**
The Flatpak looks for the host's programs in `/usr/bin` and `/usr/local/bin` only. One that is
installed elsewhere (Homebrew under `/home/linuxbrew`, `~/.local/bin`) is not found; install it
with your distribution's package manager, or use the `.deb`, `.rpm` or AppImage.

**My settings and tags are gone after switching to the Flatpak.**
They are where they were, in `~/.config/coxswain/` and `~/.local/share/coxswain/`; the Flatpak
keeps its own under `~/.var/app/io.github.mwo_dk.Coxswain/`. Copy `config.toml` across
([Where its files are kept](#where-its-files-are-kept)); the index is built again on its own.

**Does search by meaning work in the Flatpak?**
Yes. The built-in model downloads into the Flatpak's data folder when you turn it on, and
Ollama or a server on this machine (`http://localhost:11434`) is reached as usual.

**Can it open a terminal in the current folder?**
Through the [user menu](../commands/user-menu.md) (**F2**). This entry runs on the host, so the
terminal opens in the active panel's folder with your shell, not inside the sandbox:

```toml
[[user_menu]]
key = "t"
label = "Terminal here"
command = "gnome-terminal --working-directory=%d"
```

**Why is git inside the Flatpak and not the host's?**
Coxswain runs git with settings that stop a repository you did not make (a download, an
unpacked archive) from running its own hooks, filters or file system monitor. Those settings are
passed in git's environment, which `flatpak-spawn` would not carry to the host; the bundled git
gets them.

---
[← Previous: FreeBSD](freebsd.md) · [Next: Termux on Android →](termux.md)

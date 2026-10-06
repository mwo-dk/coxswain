[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# OpenBSD

Both apps run on OpenBSD. The terminal app, `coxswain`, needs nothing beyond the base system and
is built and tested on every change with OpenBSD's own Rust (Rust supports OpenBSD at tier 3:
there is no `rustup` toolchain, the `rust` package is the way). The desktop app, `coxswain-gui`,
is **experimental** on OpenBSD: it builds, its tests pass and it starts under X11 in CI, with
WebKitGTK from packages, but it has had little use on real desktops yet.

Release builds are for **OpenBSD 7.9 on amd64**, built on 7.9. OpenBSD changes its libraries'
major numbers between releases, so a build for 7.9 runs on 7.9 only: on another release, build
from source ([below](#building-from-source)).

## Contents

- [Install with one line](#install-with-one-line)
- [What the script does](#what-the-script-does)
- [Install by hand](#install-by-hand)
- [The packages, and what each is for](#the-packages-and-what-each-is-for)
- [Starting the apps](#starting-the-apps)
- [The search helper](#the-search-helper)
- [OpenBSD's limits, and what Coxswain does about them](#openbsds-limits-and-what-coxswain-does-about-them)
- [How OpenBSD differs from Linux here](#how-openbsd-differs-from-linux-here)
- [Updating and uninstalling](#updating-and-uninstalling)
- [Building from source](#building-from-source)
- [The desktop app: what experimental means](#the-desktop-app-what-experimental-means)
- [Questions](#questions)

## Install with one line

```sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
```

`ftp(1)` is in the base system and speaks HTTPS. The script is plain `sh`; the same script
installs on NetBSD and illumos. It asks before it does anything that needs root, gets root
rights from `doas` (or `sudo`), shows each `pkg_add` line before it runs it, and checks every
download against its SHA-256 sum (`sha256 -q`). To read it first:

```sh
ftp https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh
less install-unix.sh
sh install-unix.sh
```

| Option | Does |
|---|---|
| `--terminal-only` | Only the terminal app; no packages are needed |
| `--prefix DIR` | Install into `DIR`. Without it the script uses the place Coxswain is already in, or asks: `/usr/local` for every user, or `~/.local` for you alone |
| `--version v2.7.0` | That release instead of the latest |
| `--from DIR` | Install from release archives already in `DIR`, without downloading |
| `--yes` | Answer yes to every question |
| `--uninstall` | Remove what the script installed |

Options go after `sh -s --` when the script comes through a pipe:
`ftp -o - …/install-unix.sh | sh -s -- --terminal-only`.

`doas` needs a rule first if you have not made one: as root,
`echo 'permit persist :wheel' > /etc/doas.conf`.

## What the script does

1. Asks GitHub for the latest release and downloads
   `coxswain-terminal-<version>-x86_64-unknown-openbsd.tar.gz` and its `.sha256`, and stops if
   the sum does not match.
2. Installs `bin/coxswain`, the `cox` link and the manual page `coxswain(1)` under the prefix
   (`/usr/local/man/man1`, where `man` looks). With the prefix `/usr/local` it also installs
   the rc.d script `/etc/rc.d/coxswain_index`, which stays off until you turn it on
   ([the search helper](#the-search-helper)).
3. For the desktop app, checks which packages are missing, prints the `pkg_add` line, and runs
   it through `doas` when you say yes. Without WebKitGTK it leaves the desktop app out and says
   so; the terminal app is installed either way.
4. Downloads `coxswain-desktop-<version>-x86_64-unknown-openbsd.tar.gz` the same way and
   installs `bin/coxswain-gui`, `share/applications/coxswain.desktop` and the icon.
5. Lists the optional packages that are not installed, with what each adds.

## Install by hand

```sh
v=v2.7.0
t=x86_64-unknown-openbsd
ftp https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz
ftp https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz.sha256
sha256 -q coxswain-terminal-$v-$t.tar.gz; cat coxswain-terminal-$v-$t.tar.gz.sha256
tar xzf coxswain-terminal-$v-$t.tar.gz
cd coxswain-terminal-$v-$t
doas install -m 755 coxswain /usr/local/bin/
doas ln -sf coxswain /usr/local/bin/cox
doas install -m 644 coxswain.1 /usr/local/man/man1/
doas install -m 555 coxswain_index /etc/rc.d/
```

The two sums printed must be the same. Every download can also be checked against the release's
`SHA256SUMS` and its signed provenance ([Security](security.md)).

## The packages, and what each is for

The terminal app needs no packages.

| Package | For | Needed by |
|---|---|---|
| `webkitgtk41` | The desktop app's window content: WebKitGTK with the 4.1 API; brings `gtk+3` and `libsoup3` | Desktop app, required |
| `xdg-utils` | `xdg-open`, which opens a file in its program | Desktop app; the terminal app uses it for the same |
| `gstreamer1-plugins-good` | Video and sound in the preview pane | Desktop app; without it a notice says what to install |
| `symbolsonly-nerd-fonts` (or another `*-nerd-fonts`) | The file icons and git glyphs | Optional; or `glyphs = "ascii"` ([Glyphs and fonts](../customise/glyphs-and-fonts.md)) |
| `noto-cjk` | Japanese and Korean letters in the desktop app | Optional |
| `tesseract` | Search the words in scans and pictures | Optional ([Scans and pictures](../search/scans.md)) |
| `poppler-utils` | The same for scanned PDFs | Optional, with `tesseract` |
| `libreoffice` | Search older Office files | Optional |
| `pandoc` | Previews of formats it converts | Optional |
| `git` | Git status, history and branches | Optional |

Where an optional program is missing, both apps show the `pkg_add` line that installs it.
Coxswain never runs it; run it with `doas` in front.

## Starting the apps

| App | Start | Notes |
|---|---|---|
| Terminal app | `coxswain` or `cox` | Any terminal with 256 colours; `xterm` from the base system works. On the console, `glyphs = "ascii"` |
| Desktop app | `coxswain-gui`, or *Coxswain* in the desktop's menu | An X11 session (Xenocara, the base system's X) |

## The search helper

By default the [search helper](../search/helper.md) starts with the first Coxswain app and
leaves ten minutes after the last. To have it read while no app is open:

### From boot, with rcctl

The script installs `/etc/rc.d/coxswain_index` (with the prefix `/usr/local`). It runs the helper
as the user you name, with that user's home and login class; what it says goes to syslog
(`/var/log/daemon`):

```sh
doas rcctl enable coxswain_index
doas rcctl set coxswain_index user alice
doas rcctl start coxswain_index
rcctl check coxswain_index
```

Until a user is set it refuses to start and says how to set one. `doas rcctl disable
coxswain_index` turns it off again. One script serves one user; for a second, copy it as
`/etc/rc.d/coxswain_index_bob`.

### With your desktop session, or your login shell

*Settings → Finding files → Details → Background reading → Start with my session* (or
`coxswain --index-service on`) writes an XDG autostart entry,
`~/.config/autostart/coxswain-index.desktop`, which Xfce, KDE Plasma, GNOME and MATE start at
login. With `cwm`, `fvwm` or another window manager, add to `~/.xsession` or `~/.profile`:

```sh
coxswain --index-helper --stay >/dev/null 2>&1 &
```

## OpenBSD's limits, and what Coxswain does about them

OpenBSD's defaults are strict, on purpose. What touches Coxswain:

| Limit | Default | What Coxswain does |
|---|---|---|
| Open files per process (`openfiles-cur` in `login.conf`) | 512 for users (1024 at most); 128 for daemons | The helper raises its own limit to the class's maximum, and watches at most three quarters of it in folders, less 64 for reading files: about 700 folders. Folders past those are read again at the hourly rebuild |
| Memory per process (`datasize-cur`) | 1.5 GB for the `default` and `staff` classes | Enough for the apps and the helper. The built-in models for search by meaning and Ask are large: if they stop with *out of memory*, raise `datasize-cur` in your class (`staff` allows `ulimit -d unlimited`) |
| `kern.maxfiles` | 7030 for the whole system | The watch budget above keeps the helper well under it |

For a large home folder, more open files help the helper notice changes at once. In
`/etc/login.conf`, a class of your own:

```
coxswain:\
	:openfiles-cur=4096:openfiles-max=8192:\
	:tc=default:
```

then `doas usermod -L coxswain alice`, log in again, and `rcctl set coxswain_index class coxswain`
for the rc.d script.

## How OpenBSD differs from Linux here

| Feature | On OpenBSD |
|---|---|
| File watching | kqueue, folders only, within the open-file budget above |
| Trash (F8) | `~/.local/share/Trash`, the freedesktop.org layout |
| Drives in the sidebar | The mounted file systems, from getmntinfo(3) |
| Battery | `sysctl hw.power`: `0` means on battery, and the helper pauses ([Battery](../search/battery.md)) |
| Memory in the setup guide | `sysconf(_SC_PHYS_PAGES)` |
| Removable disks | Known by their path only |
| ZFS | None on OpenBSD |
| The update notice | Names the install line ([Updating](#updating-and-uninstalling)) |

## Updating and uninstalling

The update notice names the install line again; it replaces Coxswain where it is installed:

```sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --uninstall
```

Uninstalling removes the programs, the manual page, the menu entry, the icon, the rc.d script
(after `rcctl disable`) and the autostart entry. Your settings and the index stay:
`rm -rf ~/.config/coxswain ~/.local/share/coxswain ~/.cache/coxswain`.

## Building from source

```sh
doas pkg_add git rust
git clone https://github.com/mwo-dk/coxswain.git && cd coxswain
cargo build --release --locked -p coxswain
```

For the desktop app as well (Node.js for the frontend):

```sh
doas pkg_add node webkitgtk41 librsvg
(cd gui && npm ci && npm run build)
cargo build --release --locked -p coxswain-gui
```

The build needs Rust 1.94 or later (OpenBSD 7.9 has 1.94.1). If linking stops with *out of
memory*, raise the data size first: `ulimit -d unlimited` in a `staff` login.

## The desktop app: what experimental means

The desktop app is a [Tauri](https://tauri.app) app, and Tauri does not support OpenBSD
officially. It builds and runs because its window layer is GTK on the BSDs as on Linux, with the
same two patched crates as on [FreeBSD](freebsd.md#the-desktop-app-what-experimental-means).
On OpenBSD the drives in the sidebar come from Coxswain's own getmntinfo(3) reading, since the
library used elsewhere does not know OpenBSD.

Tested in CI on OpenBSD 7.9 (amd64): it builds, its tests pass, and it starts under X11 and
draws its window. Browsing, the preview pane, Find and the file operations are the same code as
on Linux and FreeBSD, tested by the core's tests on OpenBSD; they have not had use on a real
OpenBSD desktop yet. Please say what works and what does not in an
[issue](https://github.com/mwo-dk/coxswain/issues).

## Questions

**Is there a port?** Not yet. Until then, the script, the archives or `cargo install --locked
coxswain`.

**Why does the helper miss a new file deep in my tree until later?** Its folder is past the
watch budget ([OpenBSD's limits](#openbsds-limits-and-what-coxswain-does-about-them)). A login
class with more open files, or leaving large trees out of the index (`name_exclude` in
`[search]`), fixes it.

**Search by meaning says it is out of memory.** The built-in model and its work can need more
than the 1.5 GB a process gets by default: `ulimit -d unlimited` in a `staff` login, or
`datasize-cur` raised in `login.conf`. A model server elsewhere needs no memory here
([Servers](../search/servers.md)).

**Does the terminal app need `wxallowed`?** No: it never writes and executes the same memory.

**Why does the script ask for root?** To install packages with `pkg_add`, to write under
`/usr/local`, and to put the rc.d script in `/etc/rc.d`. With `--prefix ~/.local` and
`--terminal-only` it needs no root.

**Will the 7.9 build run on 7.8 or -current?** No: OpenBSD's library majors change. Build from
source there.

---
[← Previous: NetBSD](netbsd.md) · [Next: DragonFly BSD →](dragonfly.md)

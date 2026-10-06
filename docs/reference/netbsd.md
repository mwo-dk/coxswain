[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# NetBSD

The terminal app, `coxswain`, runs on NetBSD: one binary that needs nothing beyond the base
system, built and tested on every change, with pkgsrc's Rust. The desktop app is not shipped for
NetBSD yet: [why](#the-desktop-app).

Release builds are for **NetBSD 10 and 11 on amd64**, built on NetBSD 10.1. NetBSD keeps its
libraries' interfaces, so the 10.1 build runs on 11 as well.

## Contents

- [Install with one line](#install-with-one-line)
- [What the script does](#what-the-script-does)
- [Install by hand](#install-by-hand)
- [Packages for the extras](#packages-for-the-extras)
- [Starting the app](#starting-the-app)
- [The search helper](#the-search-helper)
- [How NetBSD differs from Linux here](#how-netbsd-differs-from-linux-here)
- [Updating and uninstalling](#updating-and-uninstalling)
- [Building from source](#building-from-source)
- [The desktop app](#the-desktop-app)
- [Questions](#questions)

## Install with one line

```sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
```

NetBSD's `ftp(1)` speaks HTTPS. The script is plain `sh`, the same one that installs on OpenBSD
and illumos. It asks before it does anything that needs root, gets root rights from `sudo` or
`doas` (either from pkgsrc) when you are not root, and checks every download against its
SHA-256 sum (`cksum -a sha256`). To read it first:

```sh
ftp https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh
less install-unix.sh
sh install-unix.sh
```

| Option | Does |
|---|---|
| `--terminal-only` | Only the terminal app (on NetBSD this is all there is for now) |
| `--prefix DIR` | Install into `DIR`. Without it the script uses the place Coxswain is already in, or asks: `/usr/local` for every user, or `~/.local` for you alone |
| `--version v2.7.0` | That release instead of the latest |
| `--from DIR` | Install from release archives already in `DIR`, without downloading |
| `--yes` | Answer yes to every question |
| `--uninstall` | Remove what the script installed |

Options go after `sh -s --` when the script comes through a pipe:
`ftp -o - …/install-unix.sh | sh -s -- --prefix ~/.local`.

## What the script does

1. Asks GitHub for the latest release and downloads
   `coxswain-terminal-<version>-x86_64-unknown-netbsd.tar.gz` and its `.sha256`, and stops if
   the sum does not match.
2. Installs `bin/coxswain`, the `cox` link and the manual page `coxswain(1)` under the prefix
   (`/usr/local/man/man1`). Coxswain is not a pkgsrc package, so it stays out of `/usr/pkg`,
   which belongs to `pkgin` and `pkg_add`.
3. With the prefix `/usr/local` it also installs the rc.d script `/etc/rc.d/coxswain_index`,
   which stays off until rc.conf turns it on ([the search helper](#the-search-helper)).
4. Lists the optional packages that are not installed, with what each adds.

`man coxswain` finds the manual: NetBSD's `/etc/man.conf` looks in `/usr/local/man`. With the
prefix `~/.local`, set `MANPATH="$HOME/.local/man:"` (the trailing colon keeps the system's).

## Install by hand

```sh
v=v2.7.0
t=x86_64-unknown-netbsd
ftp https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz
ftp https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz.sha256
cksum -a sha256 coxswain-terminal-$v-$t.tar.gz; cat coxswain-terminal-$v-$t.tar.gz.sha256
tar xzf coxswain-terminal-$v-$t.tar.gz
cd coxswain-terminal-$v-$t
install -m 755 coxswain /usr/local/bin/
ln -sf coxswain /usr/local/bin/cox
install -m 644 coxswain.1 /usr/local/man/man1/
install -m 555 coxswain_index /etc/rc.d/
```

(as root). The two sums printed must be the same. Every download can also be checked against
the release's `SHA256SUMS` and its signed provenance ([Security](security.md)).

## Packages for the extras

The terminal app needs no packages. What adds to it, from pkgsrc with `pkgin install`:

| Package | For |
|---|---|
| `nerd-fonts-Symbols` (in your terminal's font list) | The file icons and git glyphs; or `glyphs = "ascii"` ([Glyphs and fonts](../customise/glyphs-and-fonts.md)) |
| `tesseract` | Search the words in scans and pictures ([Scans and pictures](../search/scans.md)) |
| `poppler-utils` | The same for scanned PDFs |
| `libreoffice` | Search older Office files |
| `pandoc-cli` | Previews of formats it converts, in the desktop app |
| `git` | Git status, history and branches in the panels |
| `xdg-utils` | `xdg-open`, which opens a file in its program |

Where one of them is missing, the app names the `pkgin install` line. Coxswain never runs it.

## Starting the app

`coxswain` or `cox`, optionally with two folders. Any terminal with 256 colours works; on the
console (`wscons`), set `glyphs = "ascii"`. `coxswain --help` lists the command-line flags.

## The search helper

By default the [search helper](../search/helper.md) starts with the first Coxswain app and
leaves ten minutes after the last. To have it read while no app is open:

### From boot, with rc.d

The script installs `/etc/rc.d/coxswain_index` (with the prefix `/usr/local`). It runs the helper
as the user you name, with that user's home; what it says goes to syslog
(`/var/log/messages`, tagged `coxswain_index`). In `/etc/rc.conf`:

```sh
coxswain_index=YES
coxswain_index_user=alice
```

then `/etc/rc.d/coxswain_index start` (as root), and `status` or `stop` the same way. Without a
user it refuses to start and says what to set. NetBSD has no daemon(8) to start the helper again
if it ends; it rarely does, and `/etc/rc.d/coxswain_index start` brings it back.

### With your desktop session, or your login shell

*Start with my session* in the desktop app's Settings, or `coxswain --index-service on`, writes
an XDG autostart entry (`~/.config/autostart/coxswain-index.desktop`) that a desktop session
reads. Without one, in `~/.profile`:

```sh
coxswain --index-helper --stay >/dev/null 2>&1 &
```

## How NetBSD differs from Linux here

| Feature | On NetBSD |
|---|---|
| File watching | kqueue, folders only. Each watched folder holds a file descriptor: the helper raises its limit to the hard one (`ulimit -Hn`, 3404 by default) and watches at most three quarters of it, about 2,500 folders, shallowest first. Folders past them are read again at the hourly rebuild |
| Trash (F8) | `~/.local/share/Trash`, the freedesktop.org layout |
| Battery | envstat(8): an `acpiacad` adapter with `connected: FALSE` means on battery, and the helper pauses ([Battery](../search/battery.md)) |
| Memory in the setup guide | `sysconf(_SC_PHYS_PAGES)` |
| Removable disks | Known by their path only ([Removable disks](../search/removable-disks.md)) |
| ZFS | NetBSD's ZFS is not read; **Alt+Z** says the folder is not on ZFS |
| The update notice | Names the install line ([Updating](#updating-and-uninstalling)) |

## Updating and uninstalling

The update notice names the install line again, which replaces Coxswain where it is installed:

```sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
ftp -o - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --uninstall
```

Uninstalling removes the program, the manual page, the rc.d script and the autostart entry; take
the two lines out of `/etc/rc.conf` yourself. Your settings and the index stay:
`rm -rf ~/.config/coxswain ~/.local/share/coxswain ~/.cache/coxswain`.

## Building from source

```sh
pkgin install git rust
git clone https://github.com/mwo-dk/coxswain.git && cd coxswain
cargo build --release --locked -p coxswain
```

On another architecture that pkgsrc's Rust supports (aarch64 too) this is the way, as is
`cargo install --locked coxswain`. The build needs Rust 1.93 or later.

## The desktop app

The desktop app needs WebKitGTK with the 4.1 API, which pkgsrc has as `www/webkit-gtk41`, and
it builds from the same code as on FreeBSD and OpenBSD. It is not shipped yet because the binary
packages for NetBSD 10.1 and 11.0 could not be installed in CI: `webkit-gtk41`'s dependencies
`dbus`, `libxml2` and others were missing from the package repository. When the binary packages
are complete again, NetBSD gets the desktop app as OpenBSD has it. Until then, with pkgsrc built
from source (`cd /usr/pkgsrc/www/webkit-gtk41 && make install`), the
[build from source](#building-from-source) can add
`(cd gui && npm ci && npm run build) && cargo build --release --locked -p coxswain-gui`; it is
untested.

## Questions

**Why is it not in pkgsrc?** Not yet; a package is the natural next step. Until then, the script,
the archives or `cargo install --locked coxswain`.

**Does it run on NetBSD 9?** It is built on 10.1 and tested on 10.1 and 11.0 only. On 9, build from
source if pkgsrc's Rust there is 1.93 or later.

**Why does the helper miss new files deep in my tree for a while?** Their folders are past the
2,500 or so it can watch ([above](#how-netbsd-differs-from-linux-here)). A higher
`kern.maxfiles` and hard limit help (`ulimit -Hn` is set by the login class in
`/etc/login.conf`), as does leaving large trees out of the index (`name_exclude` in `[search]`).

**The battery is not noticed.** envstat must list an `acpiacad` adapter: `envstat -d acpiacad0`.
Machines without one count as on mains.

---
[← Previous: FreeBSD](freebsd.md) · [Next: OpenBSD →](openbsd.md)

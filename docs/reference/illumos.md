[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# illumos: OmniOS and OpenIndiana

The terminal app, `coxswain`, runs on illumos: one binary for OmniOS, OpenIndiana and the other
illumos distributions on amd64, built and tested on OmniOS on every change. ZFS is where illumos
shines, and Coxswain's [ZFS snapshots as folders](../files/zfs-snapshots.md) work there as on
FreeBSD, tested end to end on a pool in every CI run. The desktop app is not built for illumos:
[why](#the-desktop-app).

Release builds are made on **OmniOS r151058**.

## Contents

- [Install with one line](#install-with-one-line)
- [What the script does](#what-the-script-does)
- [Install by hand](#install-by-hand)
- [Starting the app](#starting-the-app)
- [The search helper](#the-search-helper)
- [ZFS snapshots and datasets](#zfs-snapshots-and-datasets)
- [How illumos differs from Linux here](#how-illumos-differs-from-linux-here)
- [Updating and uninstalling](#updating-and-uninstalling)
- [Building from source](#building-from-source)
- [The desktop app](#the-desktop-app)
- [Questions](#questions)

## Install with one line

```sh
curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
```

The script is plain `sh` (illumos's `/bin/sh` is ksh93, which runs it), and the same script
installs on NetBSD and OpenBSD. It asks before it does anything that needs root, gets root
rights from `pfexec`, `sudo` or `doas`, and checks every download against its SHA-256 sum
(`digest -a sha256`). To read it first:

```sh
curl -fsSLO https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh
less install-unix.sh
sh install-unix.sh
```

| Option | Does |
|---|---|
| `--terminal-only` | Only the terminal app (on illumos this is all there is) |
| `--prefix DIR` | Install into `DIR`. Without it the script uses the place Coxswain is already in, or asks: `/usr/local` for every user, or `~/.local` for you alone |
| `--version v2.6.0` | That release instead of the latest |
| `--from DIR` | Install from release archives already in `DIR`, without downloading (a zone without network) |
| `--yes` | Answer yes to every question |
| `--uninstall` | Remove what the script installed |

Options go after `sh -s --` when the script comes through a pipe:
`curl -fsSL …/install-unix.sh | sh -s -- --prefix ~/.local`.

## What the script does

1. Asks GitHub for the latest release and downloads
   `coxswain-terminal-<version>-x86_64-unknown-illumos.tar.gz` and its `.sha256`, and stops if
   the sum does not match.
2. Installs `bin/coxswain`, the `cox` link and the manual page under `share/man/man1`.
3. With the prefix `/usr/local` it also imports the SMF service `application/coxswain-index`,
   disabled until you name a user and enable it ([the search helper](#the-search-helper)).

No packages are needed: the terminal app links only against the base system's libraries
(`libc`, `libsocket`, `libnsl`, `libumem`).

## Install by hand

```sh
v=v2.6.0
t=x86_64-unknown-illumos
curl -fsSLO https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz
curl -fsSLO https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz.sha256
digest -a sha256 coxswain-terminal-$v-$t.tar.gz; cat coxswain-terminal-$v-$t.tar.gz.sha256
gzip -dc coxswain-terminal-$v-$t.tar.gz | tar xf -
cd coxswain-terminal-$v-$t
pfexec mkdir -p /usr/local/bin /usr/local/share/man/man1
pfexec cp coxswain /usr/local/bin/ && pfexec ln -sf coxswain /usr/local/bin/cox
pfexec cp coxswain.1 /usr/local/share/man/man1/
```

The two sums printed must be the same. Every download can also be checked against the
release's `SHA256SUMS` and its signed provenance ([Security](security.md)).

## Starting the app

`coxswain` or `cox`, optionally with two folders: `coxswain /export/home/alice /rpool/data`.
Any terminal with 256 colours works; `TERM=xterm-256color` over SSH is the usual one. On the
console, choose plain characters (`glyphs = "ascii"`, [Glyphs and fonts](../customise/glyphs-and-fonts.md)).
`/usr/local/bin` is not on OmniOS's default `PATH`: add it in `~/.profile`
(`PATH=/usr/local/bin:$PATH; export PATH`), and `MANPATH=/usr/local/share/man:$MANPATH` for
`man coxswain`.

## The search helper

Find reads names and words from an index that the [search helper](../search/helper.md) keeps
current. By default it starts with the terminal app and leaves ten minutes after the last one.
To have it read while no app is open:

### From boot, with SMF

The script imports `application/coxswain-index` (from `/var/svc/manifest/site/coxswain-index.xml`)
disabled. It runs the helper as the user you name, with that user's home; SMF starts it again if
it ends:

```sh
pfexec svccfg -s application/coxswain-index setprop start/user = astring: alice
pfexec svcadm refresh application/coxswain-index
pfexec svcadm enable application/coxswain-index
svcs -p application/coxswain-index
```

Until a user is named it refuses to start (it would run as `nobody`), and `svcs -x` says why.
What the helper says goes to the service's log: `svcs -L application/coxswain-index`.
`svcadm disable application/coxswain-index` stops it.

### From your login shell

For one user, in `~/.profile`:

```sh
coxswain --index-helper --stay >/dev/null 2>&1 &
```

A second login finds the first helper running and its own one ends at once.
`coxswain --index-service on` writes an XDG autostart entry
(`~/.config/autostart/coxswain-index.desktop`), which only a desktop session reads; on a server
use SMF.

## ZFS snapshots and datasets

Everything in [ZFS snapshots as folders](../files/zfs-snapshots.md) works on illumos, with the
`zfs` command of the base system and no root:

| What | Where |
|---|---|
| A folder's snapshots | **Alt+Z** on it: newest first, each a read-only folder as it was (`.zfs/snapshot`) |
| How a file differs now | **Enter** on a file inside a snapshot shows the diff in your pager |
| A file back | **F5** copies it out of the snapshot to the other panel; **F4**, **F6**, **F7** and **F8** are refused in a snapshot |
| The dataset | The panel's bottom border: `rpool/export/home/alice · 1.52x · 12.3 GB used · 88.1 GB free` |
| ZFS facts | **Alt+Enter** (Properties): dataset, mountpoint, compression and ratio, used, available, referenced, quota, refquota, snapshots |

The dataset of a folder comes from `/etc/mnttab`. The diff uses GNU diff
(`/usr/gnu/bin/diff`, in OmniOS's and OpenIndiana's base) for its headers and for files that are
gone now; without it the system's `diff -u` is used.

Boot environments (`beadm`) and zones are not listed; that is FreeBSD's `bectl` and jails only.

## How illumos differs from Linux here

| Feature | On illumos |
|---|---|
| File watching | Event ports (`port_associate`), folder by folder: a folder fires when an entry in it is added, removed or renamed. No file descriptor is held per folder, so up to 20,000 are watched, shallowest first; folders past them are read again at the hourly rebuild |
| Trash (F8) | `~/.local/share/Trash`, the freedesktop.org layout |
| Battery | `kstat -p acpi_drv:0:power:power`: `battery` means on battery, and the helper pauses ([Battery](../search/battery.md)). Servers have no such kstat and count as on mains. Not yet tried on a laptop |
| Memory in the setup guide | `sysconf(_SC_PHYS_PAGES)` |
| Removable disks | Known by their path only ([Removable disks](../search/removable-disks.md)) |
| Installing tools | Hints say `pkg install …` where OmniOS has a package (`ooce/application/texlive` for LaTeX); Tesseract, Poppler and LibreOffice have none in OmniOS's repositories |
| The update notice | Names the install line ([Updating](#updating-and-uninstalling)) |

## Updating and uninstalling

Both apps check once a day for a newer release and name the command to update
([Update checks](updates.md)). On illumos that is the install line again:

```sh
curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh
```

To remove it:

```sh
curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-unix.sh | sh -s -- --uninstall
```

This removes the program, the manual page and the SMF service. Your settings and the index stay:
`rm -rf ~/.config/coxswain ~/.local/share/coxswain ~/.cache/coxswain` removes them
([Where things are kept](where-things-are-kept.md)).

## Building from source

OmniOS packages Rust in its extra repository:

```sh
pfexec pkg install build-essential ooce/developer/rust
git clone https://github.com/mwo-dk/coxswain.git && cd coxswain
PATH=/opt/ooce/bin:$PATH cargo build --release --locked -p coxswain
```

`rustup` works on illumos too. `cargo install --locked coxswain` builds the terminal app from
crates.io. Linking the tests of a debug build needs a lot of memory (illumos reserves swap for all of it):
give the machine or the zone 8 GB or more, or build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`.

## The desktop app

The desktop app draws its window with WebKitGTK 4.1 and GTK 3. OmniOS packages neither, and
Tauri's window layer has not been ported to illumos, so there is no desktop build for illumos.
From a desktop elsewhere, the desktop app works on an illumos file server through NFS or SMB,
and the terminal app over SSH does the ZFS work on the server itself.

## Questions

**Does it run in a zone?** It needs nothing from the global zone, so a non-global zone runs it
the same way. There, `zfs` lists only the datasets delegated to the zone, so **Alt+Z** works on
those, and other folders show no ZFS line. (CI tests the global zone only.)

**Does it run on OpenIndiana?** Yes: CI installs the OmniOS-built archive on OpenIndiana 2026.04
with the same script and runs it. Other illumos distributions with the same base libraries
should run it too; they are not tested.

**Why does Diff say the file is new or gone without names in its header?** GNU diff is not
installed; the system's `diff` has no `-N` or `-L`. `pfexec pkg install text/gnu-diffutils`.

**Why is there no tesseract hint?** OmniOS has no package for it. Search by words in scans needs
Tesseract; without it, scans are found by name only ([Scans and pictures](../search/scans.md)).

**Is the helper heavy on a file server with millions of files?** It watches at most 20,000
folders and reads text with the lowest priority. Leave out what you never search:
`name_exclude` in `[search]`, or index only your own shares ([Choosing the folders](../search/folders.md)).

**Search by meaning?** The built-in model runs on the processor, the same code as on Linux
(`coxswain --meaning on`); the model (488 MB) is downloaded once after you say so. Its tests run
on OmniOS in CI; the model itself has not been tried on illumos yet.

---
[← Previous: DragonFly BSD](dragonfly.md) · [Next: TrueNAS →](truenas.md)

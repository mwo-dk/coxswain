[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# TrueNAS

TrueNAS keeps your files on ZFS, and Coxswain's terminal app is a good way to look after them
over SSH: browse the pools, **Alt+Z** into any dataset's snapshots, compare a file with an older
version and copy it back, all without root and without writing anything into a snapshot. Nothing
special is needed: TrueNAS runs the same builds as Linux and FreeBSD.

| TrueNAS | What it is | Which build |
|---|---|---|
| **Community Edition** (SCALE until 25.04) | Debian Linux with OpenZFS | The static Linux build, `x86_64-unknown-linux-musl` |
| **CORE** (13.x) | FreeBSD 13 with ZFS | The FreeBSD build, `x86_64-unknown-freebsd`, with `--terminal-only` |

The desktop app does not belong on a NAS (it has no desktop). Use it on your own computer, on the
shares, and use the terminal app on the NAS for what only the NAS can do: the snapshots and the
dataset facts.

## Contents

- [Before you start: a home of your own](#before-you-start-a-home-of-your-own)
- [Install on Community Edition (SCALE)](#install-on-community-edition-scale)
- [Install on CORE](#install-on-core)
- [Snapshots](#snapshots)
- [Leaving the system alone](#leaving-the-system-alone)
- [The search helper on a NAS](#the-search-helper-on-a-nas)
- [Questions](#questions)

## Before you start: a home of your own

TrueNAS replaces its system files on every update, and the package tools are switched off:
`apt` on Community Edition and `pkg` on CORE say so if you try them. So Coxswain goes into your
own home folder, on a pool, where updates do not touch it.

1. In the web interface, *Credentials → Users*: edit your user (or add one), give it a *Home
   Directory* on a data pool (for example `/mnt/tank/home/alice`) and *SSH* access with a shell
   (`bash` or `zsh`).
2. Turn on SSH under *System → Services*.
3. Log in: `ssh alice@nas`.

Coxswain keeps its settings and its index under that home (`~/.config/coxswain`,
`~/.cache/coxswain`). With the default home `/var/empty` nothing can be saved there.

## Install on Community Edition (SCALE)

The static Linux build needs nothing from the system:

```sh
v=$(curl -fsSL https://api.github.com/repos/mwo-dk/coxswain/releases/latest | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
t=x86_64-unknown-linux-musl
curl -fsSLO https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz
curl -fsSLO https://github.com/mwo-dk/coxswain/releases/download/$v/coxswain-terminal-$v-$t.tar.gz.sha256
sha256sum -c coxswain-terminal-$v-$t.tar.gz.sha256
tar xzf coxswain-terminal-$v-$t.tar.gz
mkdir -p ~/.local/bin && cp coxswain-terminal-$v-$t/coxswain ~/.local/bin/
ln -sf coxswain ~/.local/bin/cox
```

Then add `~/.local/bin` to your `PATH` (in `~/.bashrc` or `~/.zshrc`:
`export PATH="$HOME/.local/bin:$PATH"`) and start it with `coxswain`. To update, run the same
lines again; the update notice points at the releases page.

## Install on CORE

The FreeBSD script, for the terminal app, into your home:

```sh
fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh -s -- --terminal-only --prefix ~/.local
```

It needs no root and no packages. The FreeBSD build is made on FreeBSD 14 and starts on
FreeBSD 13.5 (checked in CI); CORE 13.3 is based on FreeBSD 13.3. CORE no longer gets new
features from iXsystems, and FreeBSD 13 reached its end of life in April 2026, so this is
tested less than the rest.

## Snapshots

Everything in [ZFS snapshots as folders](../files/zfs-snapshots.md) works, for every user, with
the `zfs` command TrueNAS has:

| What | How |
|---|---|
| A dataset's snapshots | `cd` into a share (`/mnt/tank/media`), put the cursor on a folder, **Alt+Z**: the snapshots your periodic snapshot tasks made (`auto-2026-10-06_00-00` …), newest first |
| A file as it was | **Enter** on a snapshot, then into its folders; **F3** shows a file in your pager |
| What changed | **Enter** on a file inside a snapshot: its diff against now |
| A file back | **F5** copies it out of the snapshot to the other panel |
| The dataset | The panel's bottom border: `tank/media · 1.02x · 3.1 TB used · 1.4 TB free`; **Alt+Enter** for compression, quota and the number of snapshots |

You need read access to the folder now; the snapshot keeps the permissions the files had then.
*Snapshot directory: Invisible* (`snapdir=hidden`, the default) is fine: Coxswain goes to
`.zfs/snapshot` by name.

## Leaving the system alone

Coxswain only writes where you tell it to, and into its own folders in your home. On a NAS, a
few places are best left to TrueNAS:

| Place | What it is | What Coxswain does |
|---|---|---|
| Snapshots (`.zfs/snapshot/…`) | Read-only | **F4**, **F6**, **F7** and **F8** are refused there, always; nothing is written into one |
| `boot-pool` (`/` and the system) | TrueNAS's own, replaced on updates | Nothing; do not install into `/usr/local` there |
| `<pool>/.system` | TrueNAS's logs, configuration database and reports | Nothing; leave it out of the index (below) |
| `<pool>/ix-applications`, `<pool>/ix-apps` | Apps and their containers | Nothing; leave it out of the index |

Coxswain never creates, destroys or rolls back a snapshot, and never changes a dataset's
properties: that stays with the web interface and its tasks.

## The search helper on a NAS

Find in the terminal app uses the [search helper](../search/helper.md). Out of the box its name
index covers the whole machine but `/proc`, `/sys`, `/dev` and `/run`, which on a NAS means
every pool, and it reads the words in the files of your home folder. On a NAS, say which
datasets to cover and leave TrueNAS's own out, in `~/.config/coxswain/config.toml`:

```toml
[search]
name_roots = ["/mnt/tank"]
name_exclude = ["/proc", "/sys", "/dev", "/run", ".system", "ix-applications", "ix-apps"]
text_roots = ["/mnt/tank/home/alice", "/mnt/tank/documents"]
```

`name_exclude` replaces the default list, so the system folders stay in it
([Choosing the folders](../search/folders.md), [every key](configuration.md#search)). No walk of
Coxswain's goes into a dataset's `.zfs` folder, even with *Snapshot directory: Visible*: it would
hold the whole dataset again for each snapshot. On a large pool the first reading takes a while,
at the lowest priority. The helper starts with the app and leaves ten minutes after it. To have
it read while you are logged out, add a cron job in the web interface (Community Edition:
*System → Advanced Settings → Cron Jobs*; CORE: *Tasks → Cron Jobs*) that runs
`/mnt/tank/home/alice/.local/bin/coxswain --index-helper --stay` as your user, say every hour:
while one helper runs, a second one ends at once, so the job only starts it again after a
restart. Do not edit the system's crontab by hand; TrueNAS rewrites it.

## Questions

**Can I run the desktop app on TrueNAS?** No: there is no desktop to run it in. Run it on your
computer and open the NAS's shares (SMB or NFS) in it.

**Can I browse snapshots through an SMB share from the desktop app?** **Alt+Z** needs the `zfs`
command, which runs only on the NAS. Windows' *Previous Versions* shows them over SMB; with
Coxswain, SSH to the NAS and use the terminal app.

**Do I need root, or the `truenas_admin` user?** No. Listing and reading snapshots works for any
user who can read the folder.

**Will a TrueNAS update remove it?** Not when it is in your home on a data pool, as above. A copy
in `/usr/local` on the boot pool would be lost at the next update.

**Does it change anything on my pools?** Only what you do with it: copying, moving or deleting
files where you have the right to. Its own state is in your home.

---
[← Previous: illumos](illumos.md) · [Next: Flatpak →](flatpak.md)

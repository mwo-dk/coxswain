[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Linux on ARM

Both apps have release builds for 64-bit ARM Linux (`aarch64`, also called `arm64`): a
Raspberry Pi with the 64-bit Raspberry Pi OS, an Apple Silicon Mac with Asahi Linux, an ARM
laptop, an ARM server (Ampere, AWS Graviton) or an ARM Chromebook's Linux. They are the same apps
as on an Intel or AMD PC, with every feature, built natively on ARM and tested there on every
change.

## Contents

- [Which file to download](#which-file-to-download)
- [What it needs](#what-it-needs)
- [Installing](#installing)
- [Raspberry Pi](#raspberry-pi)
- [Asahi Linux](#asahi-linux)
- [ARM servers](#arm-servers)
- [Search by meaning on ARM](#search-by-meaning-on-arm)
- [Questions](#questions)

## Which file to download

From the [releases page](https://github.com/mwo-dk/coxswain/releases/latest):

| You have | Desktop app | Terminal app |
|---|---|---|
| Raspberry Pi OS 64-bit, Debian, Ubuntu | `Coxswain_<version>_arm64.deb` | `coxswain-terminal-<version>-aarch64-unknown-linux-musl.tar.gz` |
| Fedora, Asahi Fedora Remix, openSUSE | `Coxswain-<version>-1.aarch64.rpm` | the same |
| Any other | `Coxswain_<version>_aarch64.AppImage` | the same |
| Arch Linux ARM | `coxswain-bin` from the AUR (both apps) | the same |

`uname -m` says `aarch64` on these machines (`dpkg --print-architecture` says `arm64`). If it
says `armv7l` or `armhf`, the system is 32-bit: there is no build for that.

The terminal app is one static file that needs nothing from the system. Homebrew's formula
installs it on ARM Linux too (`brew install mwo-dk/coxswain/coxswain`); the Homebrew cask for
the desktop app is for x86-64 only.

## What it needs

The desktop app needs **glibc 2.35 or newer**, **WebKitGTK 4.1** and **GTK 3**: Debian 12 and
Raspberry Pi OS Bookworm, Ubuntu 22.04 and newer, Fedora 36 and newer. The `.deb` and `.rpm`
name what they need (`libwebkit2gtk-4.1-0`, `libgtk-3-0`), so the package manager installs it.
The AppImage carries its own libraries but needs FUSE 2 to start (`sudo apt install libfuse2`,
or `libfuse2t64` on Ubuntu 24.04 and newer).

The builds are made on Ubuntu 22.04 for ARM (GitHub's ARM runners), so they run on any
distribution at least that new. Video and sound in the preview need GStreamer's good plugins
(`gstreamer1.0-plugins-good`), as on any Linux ([Media](../previews/media.md)).

## Installing

```sh
sudo apt install ./Coxswain_<version>_arm64.deb        # Raspberry Pi OS, Debian, Ubuntu
sudo dnf install ./Coxswain-<version>-1.aarch64.rpm    # Fedora, Asahi
chmod +x Coxswain_<version>_aarch64.AppImage && ./Coxswain_<version>_aarch64.AppImage
```

The `.deb` and `.rpm` add *Coxswain* to the applications menu and `coxswain-gui` to the path.
For the terminal app, unpack the archive and put `coxswain` and `cox` on your `PATH`
(`~/.local/bin`, for example).

Both apps check once a day for a newer release and name the file to get
([Update checks](updates.md)).

## Raspberry Pi

A Raspberry Pi 3, 4, 5 or 400 with the **64-bit** Raspberry Pi OS (Bookworm) runs both apps:
the `.deb` for the desktop app, the archive for the terminal app. Reading the words inside many
files for search takes longer than on a PC; a Pi has no battery, so it is never paused for one
([Battery](../search/battery.md)).

Raspberry Pi OS Lite has no desktop: use the terminal app, over SSH too.

## Asahi Linux

On an Apple Silicon Mac with the Asahi Fedora Remix, install the `.rpm`: it uses Fedora's own
WebKitGTK. Asahi's kernel uses 16 KB memory pages; the binaries are laid out for pages of up to
64 KB, so they load there. The AppImage carries WebKitGTK built on Ubuntu and has not been tried
on Asahi.

## ARM servers

On a server without a desktop, the terminal app is the one to use, over SSH: copy it to the
server, or `brew install mwo-dk/coxswain/coxswain`. The search helper runs as a systemd user
service with *Start with my session* ([The search helper](../search/helper.md)); for it to keep
running after you log out, `loginctl enable-linger $USER` once.

## Search by meaning on ARM

The built-in model runs on the processor, with ARM's vector instructions (NEON). There is no
GPU support for it on Linux. It works, but making the vectors for many files takes far longer on a
Pi than on a desktop. Two ways round it:

- Point Coxswain at a model server on a faster machine: *Settings → Finding files → Set up…*
  (`coxswain --setup-search`) finds the ones on your network ([Model servers](../search/servers.md)).
- Leave only *Words inside files* on: it needs no model.

## Questions

#### Which file do I download for a Raspberry Pi?

With the 64-bit Raspberry Pi OS: `Coxswain_<version>_arm64.deb` for the desktop app and
`coxswain-terminal-<version>-aarch64-unknown-linux-musl.tar.gz` for the terminal app. Check with
`uname -m`: it must say `aarch64`.

#### Does it run on the 32-bit Raspberry Pi OS?

No. There is no `armhf` build. A Pi 3 or newer can run the 64-bit Raspberry Pi OS instead.

#### The AppImage does not start and mentions FUSE. What now?

Install FUSE 2: `sudo apt install libfuse2` (`libfuse2t64` on Ubuntu 24.04 and newer). Or run it
once with `--appimage-extract-and-run`. The `.deb` needs neither.

#### Why does the desktop app say a library is missing, or a GLIBC version?

The system is older than Debian 12 or Ubuntu 22.04 (glibc 2.35 and WebKitGTK 4.1). Use the
terminal app there: it needs nothing from the system.

#### Can I install the desktop app with Homebrew on ARM Linux?

Not yet: the cask is for x86-64 Linux. The formula for the terminal app works on ARM.

#### Why is search by meaning so slow on my Pi?

The built-in model runs on the Pi's processor. Use a model server on another computer, or let it
run overnight; *Words inside files* needs no model and is quick.

---
[← Previous: Flatpak](flatpak.md) · [Next: Nix →](nix.md)

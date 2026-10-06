[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# ChromeOS

On a Chromebook, both apps run in ChromeOS's Linux (the *Linux development environment*, a
Debian container). There is no separate ChromeOS build: the Linux `.deb` or AppImage for the
Chromebook's processor is the one, and the terminal app runs in the *Terminal* app. Linux sees
its own home folder, *Linux files*, and the ChromeOS folders you share with it.

## Contents

- [Turning on Linux](#turning-on-linux)
- [Which file to download](#which-file-to-download)
- [Installing the desktop app](#installing-the-desktop-app)
- [The terminal app](#the-terminal-app)
- [Your ChromeOS files](#your-chromeos-files)
- [Search, the helper and the graphics card](#search-the-helper-and-the-graphics-card)
- [What was checked](#what-was-checked)
- [Questions](#questions)

## Turning on Linux

*Settings → About ChromeOS → Developers → Linux development environment → Set up*. ChromeOS
downloads a Debian container (Debian 12 or newer) and opens a terminal in it. School
and work Chromebooks may have it turned off by their administrator.

## Which file to download

In the Linux terminal, `dpkg --print-architecture` says which:

| It says | Desktop app | Terminal app |
|---|---|---|
| `amd64` (Intel, AMD) | `Coxswain_<version>_amd64.deb` | `coxswain-terminal-<version>-x86_64-unknown-linux-musl.tar.gz` |
| `arm64` (MediaTek, Snapdragon) | `Coxswain_<version>_arm64.deb` | `coxswain-terminal-<version>-aarch64-unknown-linux-musl.tar.gz` |

They are on the [releases page](https://github.com/mwo-dk/coxswain/releases/latest). ARM
Chromebooks are covered in more detail under [Linux on ARM](linux-arm.md).

## Installing the desktop app

Download the `.deb` in Chrome. In the Files app, double-click it in *Downloads* and choose
**Install** (*Install app with Linux*). Or, after moving it to *Linux files*, in the terminal:

```sh
sudo apt install ./Coxswain_<version>_amd64.deb
```

Either way apt brings what it needs, WebKitGTK 4.1 and GTK 3, from Debian. *Coxswain* then
appears in the launcher under *Linux apps*, and `coxswain-gui` starts it from the terminal.

The AppImage works too: move it to *Linux files*, then `sudo apt install libfuse2`, `chmod +x`
and start it. The `.deb` is simpler, and apt keeps its libraries current.

## The terminal app

Unpack the archive in *Linux files* and put `coxswain` and `cox` on the path:

```sh
tar xzf coxswain-terminal-*-linux-musl.tar.gz
mkdir -p ~/.local/bin && cp coxswain-terminal-*/coxswain coxswain-terminal-*/cox ~/.local/bin/
```

It runs in the *Terminal* app (or any terminal installed in Linux). The Terminal app's default
font has no Nerd Font glyphs: pick one in its settings, or set `glyphs = "ascii"`
([Glyphs and fonts](../customise/glyphs-and-fonts.md)).

## Your ChromeOS files

Linux sees only *Linux files* (its home, `/home/<you>`) until you share more. In the Files app,
right-click a folder (*My files*, *Downloads*, a Google Drive folder, a USB stick) and choose
**Share with Linux**. It then appears under `/mnt/chromeos`:

| Shared from | Path in Linux |
|---|---|
| *My files* or a folder in it | `/mnt/chromeos/MyFiles/…` (`/mnt/chromeos/MyFiles/Downloads`) |
| Google Drive | `/mnt/chromeos/GoogleDrive/MyDrive/…` |
| A USB stick or SD card | `/mnt/chromeos/removable/<name>/…` |
| Android's *Play files* | `/mnt/chromeos/PlayFiles/…` |

Open one in either app as any folder: type the path (**Alt+F1** / **Alt+F2** in the terminal app,
the path bar in the desktop app), or start in it: `coxswain-gui /mnt/chromeos/MyFiles/Downloads`.
*Settings → About ChromeOS → Developers → Linux development environment → Manage shared folders*
takes a share back.

To search a shared folder by name and content, add it under *Settings → Finding files → Folders*
([Folders to search](../search/folders.md)). Changes made from ChromeOS's side of a shared folder
reach the Linux container late or not at once, so the index may lag behind them; **Ctrl+R**
rereads a panel.

## Search, the helper and the graphics card

- **Start with my session** works: the container runs systemd, and the helper is a systemd user
  service. It starts with the container (when you open the terminal or a Linux app), not when you
  sign in to ChromeOS.
- **Search by meaning** runs the built-in model on the processor. ChromeOS's *GPU acceleration*
  for Linux draws windows faster but gives no GPU for computing, so neither the built-in model nor
  a model server inside the container uses the graphics card. A model server on another computer
  does it faster ([Model servers](../search/servers.md)).
- **The window** is drawn by WebKitGTK: on the graphics card where ChromeOS gives Linux GPU
  acceleration, else on the processor. Both work.

## What was checked

The release builds need nothing ChromeOS's Debian 12 container lacks:

| Build | Needs | Debian 12 has |
|---|---|---|
| `.deb`, x86-64 and ARM64 | `libwebkit2gtk-4.1-0`, `libgtk-3-0` (and through them libsoup 3, D-Bus, cairo) | All of them, from apt |
| Desktop app binary | glibc 2.35 (built on Ubuntu 22.04) | glibc 2.36 |
| AppImage | FUSE 2 | `libfuse2` from apt |
| Terminal app | Nothing: a static binary | |

The desktop app uses no tray icon and no system service, which the container may not offer.

## Questions

#### Which `.deb` does my Chromebook need?

Run `dpkg --print-architecture` in the Linux terminal: `amd64` takes `Coxswain_<version>_amd64.deb`,
`arm64` takes `Coxswain_<version>_arm64.deb`.

#### Why does Coxswain not see my Downloads folder?

Linux sees only *Linux files* until a folder is shared. In the Files app, right-click
*Downloads* and choose **Share with Linux**; it is then at `/mnt/chromeos/MyFiles/Downloads`.

#### Can I open Google Drive files?

Yes, once Google Drive (or a folder in it) is shared with Linux: `/mnt/chromeos/GoogleDrive/MyDrive`.

#### Does search by meaning use the Chromebook's graphics?

No. Linux on ChromeOS has no GPU for computing; the built-in model runs on the processor.

#### Does the helper keep reading when Linux is closed?

No. The helper runs inside the container, which stops when you sign out, shut down or choose
*Shut down Linux*. It carries on where it was when the container starts again.

---
[← Previous: Linux on ARM](linux-arm.md) · [Next: macOS →](macos.md)

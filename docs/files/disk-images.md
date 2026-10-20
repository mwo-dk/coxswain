[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Disk images, CHM help files and CBR comics

A disk image (`.iso`, `.dmg`, `.vhd`, `.vhdx`, `.vmdk`, `.qcow2`, `.vdi`, `.wim`, a raw `.img`)
shows its facts in the preview pane: its format, the size of the disk it holds, what it takes
on your disk, whether it is fixed or grows, its partitions and its label. With *Open disk images,
CHM and CBR like folders* turned on, **Enter** opens it like a folder, read-only, and so does a
`.chm` help file or a `.cbr` comic. **F5** copies files out of it. An installed 7-Zip reads them;
Coxswain never mounts an image and never writes into one.

![The desktop app with setup.iso under the cursor in Downloads; the preview pane shows Format ISO 9660, Disk size 372 KB, Size on disk 372 KB, Label SETUP_1_0, and below them the line Open it like a folder: turn on Open disk images, CHM and CBR like folders in Settings → Behaviour, with a Settings → Behaviour button](../screenshots/gui-disk-image.png)
*A disk image with the option off: its facts, and how to look inside.*

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Formats](#formats)
- [What it needs](#what-it-needs)
- [Read-only, and never mounted](#read-only-and-never-mounted)
- [Search](#search)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Put the cursor on a disk image. The desktop app's preview pane shows its facts at once; in
   the terminal app **Alt+Enter** (Properties) shows them.
2. To look inside, turn the option on once, in either way:
   - **Shift+F10** (the action menu) or **F9** (the command list) → *Open disk images, CHM and
     CBR like folders*;
   - *Settings* (**Ctrl+,** or **F12**) → *Behaviour* → *Open disk images, CHM and CBR like
     folders*. The desktop app's preview has a *Settings → Behaviour* button that opens Settings
     right there.
3. **Enter** on the image opens it like a folder. The panel title shows the image's path, and
   folders inside open with **Enter** as usual; `..` at the top leads back out.
4. Put the cursor on a file inside and the preview shows it (a page of a CHM in the sandboxed
   HTML preview, a page of a CBR as a picture); **F3** views it.
5. **F5** copies the file or folder under the cursor (or the marked ones) out to the other
   panel. **Ctrl+E** on the image unpacks all of it into a folder of its own.

| | Desktop app | Terminal app |
|---|---|---|
| Facts | Preview pane, and Properties (**Alt+Enter**) | Properties (**Alt+Enter**) |
| Turn it on | **Shift+F10**, **F9**, *Settings → Behaviour*, or the button under the facts | **Shift+F10**, **F9**, or **F12** → *Behaviour* |
| Open | **Enter**, double-click | **Enter** |
| Copy out | **F5** | **F5** |

## What you see

Under the facts, one line says what more you can do, and how. It has three states:

| State | The line | What to do |
|---|---|---|
| The option is off | *Open it like a folder: turn on Open disk images, CHM and CBR like folders in Settings → Behaviour. Coxswain reads it with 7-Zip and never mounts it.* The desktop app adds a *Settings → Behaviour* button; the terminal app names its Settings key and **Shift+F10** | Turn it on ([How to use it](#how-to-use-it)) |
| On, and no 7-Zip | *Install it:* with the line for your system and a *Copy* button (the desktop app); *To look inside it, install 7-Zip: pkg install 7-zip* in the terminal app's Properties. A [notice](../panels/notices.md) says it once too | Run the line ([What it needs](#what-it-needs)) |
| On, and 7-Zip is there | *Enter opens it like a folder, read-only: copy files out of it to the other panel.* The desktop app lists what is inside below it | **Enter** |

Off, **Enter** on an image does what it does with any file: the desktop app hands it to your
system (which may mount it), the terminal app runs or views it as before.

The first few times the cursor is on a disk image with the option off, the status line says
*Shift+F10 → Open disk images, CHM and CBR like folders looks inside it*; inside one it says
*Read-only here: F5 copies files out to the other panel*.

The facts, line by line:

| Line | Says | Read from |
|---|---|---|
| *Format* | VHD, VHDX, VMDK, QCOW2, DMG, VDI, WIM, ISO 9660, UDF, *Raw disk image*, *CHM help file* or *CBR comic (RAR)*, with *fixed size*, *grows as it fills* or *changes to a parent image* where the format says | The image's header or footer |
| *Disk size* | The size of the disk it holds (a dynamic VHD of 40 GB may take 2 GB) | VHD footer, VHDX metadata, VMDK header or descriptor, QCOW2 and VDI headers, DMG trailer, ISO volume |
| *Size on disk* | What the file takes on your disk (less than its length when it is sparse) | The file system |
| *Label* | An ISO volume's name, a FAT or ext file system's label | The volume descriptor or boot sector |
| *File system* | FAT12, FAT16, FAT32, NTFS, exFAT, ext2/3/4, HFS+ or APFS, when it starts right at the image's beginning | The boot sector or superblock |
| *Partition table*, *Partition 1* … | MBR or GPT, and each partition's type (EFI, Linux, FreeBSD ZFS, Microsoft basic data, …), size, start and GPT name | The first MiB of a raw image, an ISO or a fixed VHD |

The facts need no program and are read even with the option off.

## Formats

7-Zip reads every one of these, and the file systems inside them (FAT, NTFS, ext, HFS+, APFS,
ISO 9660 and UDF):

| Format | Endings | Facts |
|---|---|---|
| ISO 9660 and UDF | `.iso` | Label, size, partitions of a hybrid image |
| Apple disk image | `.dmg` | Disk size (an encrypted one cannot be opened) |
| Virtual PC, Hyper-V | `.vhd`, `.vhdx` | Disk size, fixed, dynamic or differencing; a fixed VHD's partitions |
| VMware | `.vmdk` (sparse, or the descriptor of a flat one) | Disk size, fixed or dynamic |
| QEMU | `.qcow2`, `.qcow` | Disk size, with a backing file or not |
| VirtualBox | `.vdi` | Disk size, fixed or dynamic |
| Windows imaging | `.wim` | Format |
| Raw disk | `.img` | Partition table, file system, label |
| Compiled HTML help | `.chm` | Format; inside, its pages, pictures and table of contents |
| Comic book (RAR) | `.cbr` | Format; inside, its pages as pictures |

A `.vhd` is also VHDL source code, and an `.img` can be anything. Such a file counts as a disk
image only when its content says so: a VHD's `conectix` footer, or for an `.img` a partition table,
a file system or an ISO volume. VHDL source stays code, in the preview and in search.

A disk with several partitions opens to one file per partition (`0.fat`, `1.img`), as 7-Zip
shows them; copy one out and open that.

## What it needs

An installed 7-Zip (`7zz`, or `7z`). Coxswain looks for it on `PATH`, and on Windows also in
`C:\Program Files\7-Zip`:

| System | Install |
|---|---|
| FreeBSD | `pkg install 7-zip` |
| NetBSD | `pkgin install 7-zip` |
| OpenBSD | `pkg_add p7zip` |
| illumos | `pkg install compress/7zip` (OmniOS), or 7-Zip from [7-zip.org](https://www.7-zip.org) |
| Linux | `sudo pacman -S 7zip` (Arch), `sudo apt install 7zip` (Debian, Ubuntu), `sudo dnf install 7zip` (Fedora), `sudo zypper install 7zip` (openSUSE) |
| macOS | `brew install sevenzip` |
| Windows | `winget install 7zip.7zip` |

`7za` alone is not enough: it reads none of these formats. Coxswain shows the line for your
system and never runs it. 7-Zip itself (`7zz`, version 21 or later) is best: a folder comes out at once.
Any other 7-Zip, the older p7zip (OpenBSD's package) included, or one whose version Coxswain
cannot read, copies a folder out file by file, which is slower for big folders, because older
ones make the links an image holds even when told not to.

## Read-only, and never mounted

Coxswain asks 7-Zip to list the image and to write single files out of it; it never mounts
anything, needs no administrator rights, and never writes into an image. Inside one, **F6**,
**Shift+F6**, **F7** and **F8** are left out of the action menu, and pressed anyway they say
*Read-only: Coxswain never writes into a disk image, a CHM help file, a CBR comic or a mailbox. Copy its
files out instead.* **F5** into an open image says the same.

Names are given to 7-Zip as they are, after `--`, with its wildcards off: a file called `*` or
`-ao` is just a name. Copied out, a file can never land outside the folder you copy to, and no
symbolic link inside an image is ever made (`-snl-`, and any that came out anyway is removed),
so nothing is written through one. A file that 7-Zip gives back with another size than it
listed is an error, not a short copy. A locked image is not opened: no password is asked for or
given.

A file inside, previewed or viewed with **F3**, is copied to Coxswain's cache folder first (256
MiB at most), as for [archives](archives.md#limits).

An image is listed by 7-Zip once, when you first look at it, and the list is kept while the
image does not change; the facts are read once too. An image with more entries than 64 MiB of
7-Zip's listing holds (some 300,000) shows only those, and the preview counts them with a *+* (*2000+ entries*), as for a big archive.
An image only in the cloud is not read at all, not even for its facts, so a cursor move never
downloads it. An `.iso`, `.dmg` or other image whose header Coxswain does not know still shows
*Its format was not recognised from its header; 7-Zip may still read it.*, with the line of
[What you see](#what-you-see).

## Search

Search does not read inside disk images, CHM help files or CBR comics: they can be many
gigabytes, and need 7-Zip. Find finds the image itself by its name, as any file. A disk image's
own bytes are never read as text either: a VHD named `.vhd` is not taken for VHDL source.

## Settings and config.toml

| Setting | Key | Default |
|---|---|---|
| *Settings → Behaviour → Open disk images, CHM and CBR like folders* | `[archive] disk_images` | `false` |

```toml
[archive]
disk_images = true
```

Each app reads it when it starts and after a change in its own Settings or from the action menu.

## In the terminal app

The same option, keys and 7-Zip. The facts are in Properties (**Alt+Enter**), with the line of
[What you see](#what-you-see) below them, naming the terminal app's keys: *turn on Open disk
images, CHM and CBR like folders in Settings (F12) → Behaviour, or with Shift+F10 → Open disk
images, CHM and CBR like folders*. Running that entry turns it on and the status line then says
what to do next. **Enter** on a file inside says *demo.iso is read-only: F5 copies this file out
of it*.

## Questions

#### Why does Enter not open my .iso?

Opening disk images is off by default. Turn it on with **Shift+F10** → *Open disk images, CHM
and CBR like folders*, or in *Settings → Behaviour*; 7-Zip must be installed
([What it needs](#what-it-needs)). Off, the preview still shows the image's facts.

#### Why is it off by default?

Images are often opened by your system (a `.dmg` mounts on a Mac, an `.iso` with a double-click
on Windows), and Enter does that today. Opening them in Coxswain needs 7-Zip, which not every
machine has, so you choose it.

#### Can I change files inside a disk image?

No. Coxswain never writes into a disk image and never mounts one: **F5** copies files out of it,
and adding, renaming, deleting or making folders inside it is refused.

#### Does search find files inside disk images?

No. Search does not read inside disk images, CHM help files or CBR comics, which can be large;
it finds the file itself by its name.

#### My .vhd is VHDL code. Will Coxswain take it for a disk image?

No. A `.vhd` counts as a disk image only when it ends with a VHD footer (`conectix`). Source code
stays code: highlighted in the preview, read by search, opened by **Enter** as before.

#### Why does a disk show 0.fat and 1.img instead of my files?

The image holds several partitions, and 7-Zip shows each as a file. Copy the one you want out
with **F5**, then open that copy (it is a disk image too).

#### Why can a .dmg not be opened?

An encrypted DMG needs its password, and Coxswain gives 7-Zip none. Some newer compressions need
a recent 7-Zip (`7zz`, version 21 or later); the error line under the facts says what 7-Zip said.

#### Why are RAR archives not opened, but CBR comics are?

Coxswain's own archive code is pure Rust and reads no RAR ([Why not RAR](archives.md#why-not-rar)).
A `.cbr` is read by the 7-Zip you installed, with this option on, the same way as a disk image.

---
[← Previous: Passwords for encrypted zip and 7z](archive-passwords.md) · [Next: Properties and permissions →](properties.md)

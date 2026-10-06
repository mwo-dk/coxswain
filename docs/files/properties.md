[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Properties and permissions

**Alt+Enter** shows what a file or folder is: its size, dates, owner and permissions, and where
the system has them, its ZFS dataset, the FreeBSD package it belongs to and its file flags. In
the desktop app you can change the permission bits there (on Windows the read-only flag) and
your own file flags; the terminal app shows the same facts in a window of text.

![The Properties dialog of deploy.sh: Location /home/demo/projects/rocket, Type File, Size 69 B, Created, Modified and Accessed, Owner uid 1000, gid 1000, Permissions 755 with rwxr-xr-x next to it, Apply and Close](../screenshots/files-properties.png)

## How to use it

1. Put the cursor on a file or folder (not `..`; in the terminal app `..` means the folder you
   are in).
2. Press **Alt+Enter** (or *Properties* in the F9 command list). For a folder, the status line
   says *Reading properties…* while its size is measured.
3. To change the permissions, type three or four octal digits into the field (`755`, `0600`),
   or tick or untick *Read-only* on Windows.
4. Press **Enter** or *Apply*. *Close* (or **Esc**) closes without changing anything.

## What you see

The dialog is titled with the entry's name.

| Item | Shows |
|---|---|
| *Location* | The folder it is in |
| *Type* | *File*, *Folder*, or *Symbolic link* `→` its target |
| *Size* | The size; from 10 KB on also the exact bytes; for a folder, *in 1,234 files* |
| *Created*, *Modified*, *Accessed* | Dates, where the file system records them |
| *Owner* | `uid 1000, gid 1000` (Linux, macOS) |
| *Permissions* | An octal field, `644`, with `rw-r--r--` next to it, updated as you type (Linux, macOS) |
| *Attributes* | *Read-only* (Windows) |

A value that is not octal says *Permissions are 3 or 4 octal digits, e.g. 644* under the list,
and nothing is changed. After *Apply* the dialog closes and the pane is read again.

## ZFS, packages and file flags

Below the permissions, where they apply:

![Properties of README.md on FreeBSD 14.5: Owner uid 1001, Permissions 644, Flags uarch with tick boxes for nodump and hidden, ZFS dataset tank/home/demo mounted at /tank/home/demo, Compression lz4 ratio 1.24x, Space 1001 KB used, 499 MB available, 424 KB referenced, Quota 500 MB with descendants, Snapshots 3 (Alt+Z lists them)](../screenshots/files-properties-zfs.png)

| Item | Shows | Where |
|---|---|---|
| *Flags* | The file flags set, by the names `ls -lo` uses: `uarch`, `nodump`, `hidden`, `schg` …; *none* when there are none. Under it, a tick box for each flag you may set (when you own the file) | FreeBSD, macOS; on Linux the attributes `lsattr` shows that matter here: `immutable`, `append-only`, `nodump` |
| *Package* | The installed package the file belongs to, `git-2.56.0`, and *Files of this package*, which lists its files as a folder | FreeBSD, files of packages (`pkg which`) |
| *ZFS dataset*, *Mounted at* | The dataset the file is on, and where it is mounted | ZFS |
| *Compression* | The dataset's compression and its ratio: `lz4, ratio 1.52x` | ZFS |
| *Space* | Used, available and referenced space of the dataset | ZFS |
| *Quota* | `quota` (with the datasets below it) and `refquota` (this dataset alone), when either is set | ZFS |
| *Snapshots* | How many snapshots the dataset has, and the key that lists them: `12 (Alt+Z lists them)` ([ZFS snapshots as folders](zfs-snapshots.md)) | ZFS |

**Flags you can set.** The user flags, on a file you own: `uchg` (user immutable), `uappnd`
(user append-only), `nodump` (left out by dump(8)) and `hidden` (hidden in Finder and Samba).
Tick or untick them and press *Apply*; the other flags stay as they are. ZFS on FreeBSD keeps
`nodump` and `hidden` but no `uchg` or `uappnd`, so on ZFS only those two are offered.

**System flags** (`schg`, `sappnd`, `sunlnk`, `arch`) are shown, not offered: only root sets
them, and while the system runs at securelevel 1 or higher (`sysctl kern.securelevel`) not even
root can clear them. The dialog says so under the flags when one is set. On Linux the
attributes are shown only: `chattr` sets them, as root.

**Files of this package** (*F9 → Files of this package*, or the button next to the package)
opens `<file>/@package`: every file the package installed, by its full path, with its size and
date, as `pkg info -l` lists them. **Enter** on one goes to its folder with the cursor on it;
**F3** views it and **F5** copies it, as anywhere. The list is read-only, and its badge names
the package. **Backspace** leads back to the file's folder.

## Settings and config.toml

None. The keys are `properties` (**Alt+Enter**), `flags` and `package` (no key of their own:
**F9**) in `[keys]`.

## In the terminal app

**Alt+Enter** (or *F9 → Properties*) opens a window with the same facts as text: location,
type, size, modified, owner, permissions, and the ZFS, package and flag lines. A folder's size
is measured on a thread; the status line says *Reading properties…* meanwhile. Any key closes
it. Nothing is changed there; instead:

| To | In the terminal app |
|---|---|
| Change the permissions | The command line: `chmod 755 deploy.sh` ([The command line](../commands/command-line.md)) |
| Change your file flags | *F9 → File flags* on the file: a line with the flags set, to edit (`nodump hidden`); the flags you may set are named above it, and the others you leave out are cleared |
| List a package's files | *F9 → Files of this package* on the file |

## Questions

#### Can I change the owner?

No, only the permission bits (or the read-only flag on Windows). Changing the owner needs
administrator rights; use `sudo chown` on the command line.

#### Does changing a folder's permissions change the files in it?

No, only the folder itself. For everything inside, use `chmod -R` on the command line.

#### What do the four-digit values mean?

The first digit holds the special bits: `4` setuid, `2` setgid, `1` sticky. `1777` is a shared
folder like `/tmp`; `0644` is the same as `644`.

#### Why is Created missing?

Not every file system records when a file was made. Where it does not (some Linux file systems,
network shares), the line is left out.

#### What happens when I change the permissions of a symbolic link?

The permissions of what it points at change: links themselves have no permissions of their own
on Linux.

#### Why can I not tick uchg on ZFS?

ZFS on FreeBSD stores `nodump` and `hidden` for users, and immutability only as the system flag
`schg`. Setting `uchg` there fails with *Operation not supported*, so the dialog offers only
the two that work. As root: `chflags schg file`.

#### Why does every file on ZFS show `uarch`?

ZFS on FreeBSD sets the archive flag (`uarch`, from Windows' "archive" attribute) on files it
writes, for Samba. `ls -lo` shows it too. It is harmless; `chflags nouarch` clears it.

#### Which package does a file belong to, from the command line?

`pkg which /usr/local/bin/git`, and `pkg info -l git` lists its files: the same two commands
Coxswain runs.

#### Why does the dialog show an error for a file inside an archive?

A file inside an archive is not on disk, so it has no properties the system can report. Copy it
out with **F5** first ([Archives as folders](archives.md#limits)).

---
[← Previous: Passwords for encrypted zip and 7z](archive-passwords.md) · [Next: Finding duplicates →](duplicates.md)

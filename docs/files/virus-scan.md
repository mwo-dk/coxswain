[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Scan for viruses

**Alt+I** scans a file, the marked files or a folder for viruses with
[ClamAV](https://www.clamav.net/), in a container. Only when you ask: Coxswain never scans by
itself, in the background or when files arrive. What it finds is marked in the panel and listed,
and for each file you choose what happens: leave it, put it in the bin, or move it into
quarantine. Nothing is ever deleted by itself. Both apps.

It is off until you turn it on (*Settings → Previews → Scan for viruses with ClamAV*), because
ClamAV's virus database is fetched from ClamAV's server, `database.clamav.net`.

![The desktop app's dialog Found by ClamAV over the Downloads folder: eicar.com set to Quarantine and test-files.zip to Leave it, each with Eicar-Test-Signature and its path, the quarantine folder named, the line The virus database is from …, and the buttons Do it and Close; behind it both files marked in the panel](../screenshots/virus-scan.png)

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [What it needs](#what-it-needs)
- [How it is sealed](#how-it-is-sealed)
- [The quarantine folder](#the-quarantine-folder)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Turn it on once: *Settings* (**Ctrl+,** or **F12**) → *Previews* → *Scan for viruses with
   ClamAV*. Until then **Alt+I** says it is off and offers Settings.
2. Put the cursor on a file or a folder, or mark several. On `..` with nothing marked, the
   panel's own folder is scanned. Folders are scanned with everything in them.
3. Press **Alt+I**, or pick *Scan for viruses* in the action menu (**Shift+F10**) or in **F9**.
4. A question says what it reads and what happens first: *Scans the folder Downloads for viruses
   (files: 1204). ClamAV runs in podman without network access and reads the files without
   being able to change them. The virus database is from 19/10/2026 09:12. The database is more
   than a day old: first it is brought up to date from database.clamav.net, in a container of its
   own that sees nothing but the database.* On the first scan it also says the image is pulled
   and its size, and that the database (about 110 MB) is fetched. **Enter** (or **Scan**)
   starts; **Esc** changes nothing.
5. The status line says how far it is: *Updating the virus database · daily.cvd updated …, 9 s*,
   then *Scanning for viruses: 312/1204, 21 s*. **Stop** (desktop app) or **Alt+I** again (both
   apps) stops it; nothing has been changed then.
6. Nothing found: *No viruses found (files scanned: 1204)* on the status line. Something found:
   the files are marked in the panel, and the dialog *Found by ClamAV* lists each with what was
   found (*Eicar-Test-Signature*) and a choice, *Leave it* to start with.
7. Choose for each, then **Do it** (**Enter**). The status line says *Done: in the bin: 1, in
   quarantine: 1, left as they were: 0*. **Close** (**Esc**) leaves every file as it is.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Alt+I** | Asks, then scans; while a scan runs, stops it | The same |
| **Shift+F10**, **F9** | *Scan for viruses* in the action menu (on files and folders on disk) and in the command list | The same |
| In *Found by ClamAV*: **Up** / **Down** | The file to choose for (the mouse too) | The same |
| **N** / **B** / **Q** | *Leave it* / *Bin* / *Quarantine* for that file (or its list) | The same |
| **Enter** | Do what was chosen | The same |
| **Esc** | Close, changing nothing | The same |

## What you see

| Where | Desktop app | Terminal app |
|---|---|---|
| Turned off | A dialog: *Scan for viruses is off. Turn it on in Settings → Previews → Scan for viruses with ClamAV; until then nothing is scanned or fetched.*, with a **Settings → Previews** button that opens Settings at that switch | The same text, with *Settings: F12 → Previews* |
| No podman or docker | *Scan for viruses runs ClamAV in a container (podman or docker): Neither podman nor docker is installed* (or *containers are off …*) | The same |
| The question | What it reads (the count of files), how ClamAV runs, the database's date, its update, the first pull, and *Nothing is deleted or moved* | The same |
| While it runs | *Updating the virus database · …*, then *Scanning for viruses: 312/1204, 21 s*, and a **Stop** button at the end of the command line | The same text, with *· Alt+I stops* |
| First scan | *Pulling docker.io/clamav/clamav-debian:1.5_base@sha256:…* with the runtime's progress | The same |
| Nothing found | *No viruses found (files scanned: 1204)*; when the update failed, or some files could not be read or were read only in part, a dialog says so too and lists them | The same |
| Nothing read | Never *No viruses found*: the dialog says *Nothing was scanned, so nothing can be said about viruses*, with why (the files it could not read, or *there are no files in it* for an empty folder or one of links only) | The same |
| Found | The files marked in the panel (those in it), and the dialog *Found by ClamAV*: a row a file, with its path, what was found and a list *Leave it* / *Bin* / *Quarantine*; under it the database's date, the quarantine folder, and what was not read | The same, as a box: the choice, what was found, the path (from the panel's folder); **N**, **B**, **Q** set the choice |
| Stopped | *Stopped: nothing was changed* | The same |
| Failed | *The virus scan failed* with ClamAV's last lines | The same |

Files only in the cloud (an online-only OneDrive or Dropbox file) are not downloaded to be
scanned: the dialog counts them under *Not scanned*. Links found inside a folder are not
followed, and pipes and devices are left out; a file or folder you marked that is itself a link
(a `Downloads` that points to another disk) is scanned as what it points to, and what is found
is named by that real path. A file ClamAV could not read (*Access denied*) is not counted as
scanned: the dialog lists it under *Could not be read*. A name with a line break in it is scanned like any other (up to a hundred
of them a scan).

## What it needs

| Way | What runs | Systems |
|---|---|---|
| Its container | podman or docker runs `docker.io/clamav/clamav-debian:1.5_base`, ClamAV's own image, made by Cisco Talos with every ClamAV release and rebuilt as Debian updates (GPL-2.0), for x86-64, ARM64 and POWER. The `_base` image has no database in it: Coxswain keeps the database apart and brings it up to date itself | Linux, macOS and Windows with podman or docker. Not on FreeBSD, the other BSDs and illumos: the image is a Linux one. There **Alt+I** says so; an installed `clamscan` can be run from the command line or the [user menu](../commands/user-menu.md) instead |

The image is about 80 MB to download and 240 MB on disk, pinned by its digest, so every scan
runs the same ClamAV until Coxswain itself moves to a newer one. The database is about 110 MB
(`~/.cache/coxswain/clamav/`, listed in [Disk use](../reference/disk-use.md)). A scan needs
about 1.5 GB of memory while it runs; loading the database takes some seconds before the first
file is read.

ClamAV's own limits apply: a file over 100 MB, or an archive that unpacks to more than 400 MB
(or holds more files, or is packed deeper, than ClamAV goes), is read only as far as those
limits. Such a file is never taken for clean: ClamAV is asked to say so
(`--alert-exceeds-max`), and the dialog lists it under *Read only in part, as they are larger or
packed deeper than ClamAV's limits allow*. That is a note, not a finding: nothing is offered
to move.

## How it is sealed

Two runs, each in a container of its own, both removed when they end:

| Run | When | Network | Sees |
|---|---|---|---|
| `freshclam` | Before a scan, when the database is missing or more than a day old | Yes, the runtime's usual network; freshclam is set to fetch only from `database.clamav.net`, ClamAV's server | Only the database folder (which it writes), and a config file of Coxswain's, read-only |
| `clamscan` | The scan | None (`--network=none`) | The folder scanned (or the folder the marked files are in), read-only, and each file with a line break in its name on its own; the database, read-only; the list of files Coxswain made |

Both have a read-only root with a small `/tmp` in memory, no capabilities, no way to gain
privileges, bounded memory and processes (where the runtime can bound them), and run as you: your own user id inside in rootless podman (`--userns=keep-id`), your user and
group ids (`--user`) with docker or podman run as root, never those of a folder's owner. Run as
root itself, Coxswain does not scan. Nothing ClamAV writes is trusted: a finding
counts only for a file Coxswain itself put on the list it scanned, and its lines are shown with
control characters taken out. When the update fails (offline, or ClamAV's server asks to wait),
the scan uses the database there is and says so; with no database at all it does not scan.

## The quarantine folder

*Quarantine* moves the file into `~/.local/share/coxswain/quarantine/` (macOS:
`~/Library/Application Support/coxswain/quarantine/`, Windows:
`%APPDATA%\coxswain\quarantine\`) under a name of its own, the time and its name
(`1792515148-eicar.com`), so nothing there is written over. On Linux and macOS it is made
readable by you alone and runnable by nobody, before it is moved; a file that belongs to another
user cannot be made so, and stays where it is (*… it belongs to another user, so it cannot be
locked in quarantine and was left where it is*; *Bin* may still take it). On Windows its permissions stay as they were. Each move adds a line to `quarantine.log` in that folder: when, what was
found, where it came from and where it went, so a file that was wrongly found can be put back
by hand (copy it back, then give it its permissions again).

Settings → *Privacy and updates* → [Disk use](../reference/disk-use.md#the-rows) lists the folder
as *quarantine/* under *Your own files, kept aside by Coxswain*, with its size and **Show in
panel**, and no Clear: what is in it is yours, deleted in a panel once you are sure.

*Bin* puts it in the bin, from where your desktop puts it back. *Leave it* does nothing.

Both act only on the file that was found. Coxswain notes each one as the scan finds it (which
file it is, its size and time); when the file has been replaced since, by a link, a folder or
another file, it is left alone, and the dialog *Some files could not be moved* says *… it is not the file that was found any more*. On Windows this needs the file's own id, which NTFS, ReFS and FAT give; where none can be read, the file is left alone, saying so.

## Settings and config.toml

| Settings → Previews | config.toml | Default |
|---|---|---|
| *Scan for viruses with ClamAV* | `[preview] virus_scan` | `false` |
| *Container runtime* | `[preview] container` | `"auto"` |
| *Container images* (Pull, Remove) | `[preview.images] clamav` | `"docker.io/clamav/clamav-debian:1.5_base@sha256:dfe9…"`; one you set must have `clamscan` and `freshclam` |
| *Remove images not used for 90 days* | `[preview] image_prune` | Off; a scan counts as a use |
| *Keys* | `[keys] scan_viruses` | `"Alt+I"` |

*Settings → Privacy and updates → What can leave this machine* names `database.clamav.net` while
it is on. [Keep images up to date](../previews/containers.md#keeping-images-up-to-date) does not
change a pinned image; the database is what keeps the scan current.

## In the terminal app

<!-- screenshot: tui-virus-scan.png: The terminal app's box Found by ClamAV over the Downloads panel: Quarantine for eicar.com and Leave it for test-files.zip, each with Eicar-Test-Signature, the quarantine folder above and the key line Up/Down a file · N leave it · B bin · Q quarantine · Enter Do it · Esc Close; both files marked in the panel -->

The same action, key, question and list, as dialogs; the status line shows how far it is, and
**Alt+I** again stops it. Quitting while it runs stops the scan first, so its container goes
too (a second **F10** leaves at once). A terminal app over ssh can scan a server's folders the
same way.

## Questions

#### Does Coxswain scan by itself, or send my files anywhere?

No. It scans only when you press **Alt+I** (or pick it in **Shift+F10** or **F9**), in a
container with no network, your files read-only. The one download is ClamAV's virus database
from `database.clamav.net`, before a scan when it is more than a day old, by a container that
sees only the database. No file, name or finding is sent.

#### What happens to a file it finds?

Nothing until you choose: leave it, put it in the bin, or move it into the
[quarantine folder](#the-quarantine-folder), where it cannot be run. Nothing is ever deleted by
itself, and **Esc** in the list changes nothing. The files stay marked in the panel, so **F8**
or **F6** work on them too.

#### What does it need?

Podman or docker, and *Scan for viruses with ClamAV* turned on in Settings → Previews. ClamAV's
image (about 80 MB) is pulled on the first scan, and its database (about 110 MB) fetched, once
the question before it has said so. On FreeBSD, the other BSDs and illumos it does not run.

#### It found the EICAR test file. Is that a virus?

No. EICAR is a harmless line of text every scanner reports on purpose, to show it works. Make
one to try it: a file holding only
`X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*` is found as
*Eicar-Test-Signature*.

#### A file I trust was found. What now?

Choose *Leave it*. ClamAV's signatures sometimes match a harmless file, mostly tools and
packed programs. When it was moved into quarantine already, `quarantine.log` says where it came
from: copy it back.

#### Why does the first scan take longer?

It pulls the image and fetches the whole database first; later scans fetch only the day's
changes, once a day. Each scan also loads the database (some seconds) before it reads files.

#### Does it scan inside archives?

Yes: ClamAV opens zip, 7z, tar, rar and more and reads what is inside, within its limits. A
finding names the archive, as that is the file you can act on. Files inside an archive that
Coxswain shows as a folder are scanned by scanning the archive itself.

#### It says some files were read only in part. Are they safe?

Not known: ClamAV stopped at its limits (100 MB a file, 400 MB unpacked from an archive, or an
archive packed too deep), so what lies past them was not looked at. The dialog lists those
files under *Read only in part*; nothing is offered to move, as nothing was found. Scan what
is inside such an archive by unpacking it first, or check it with another scanner.

#### Why does it say *Nothing was scanned* and not *No viruses found*?

Because no file was read, so nothing can be said about viruses. The dialog says why: the files
listed under *Could not be read* (often *Access denied*: files of another user), those only in
the cloud, or *there are no files in it*, for an empty folder or one that holds only links,
pipes or devices, which are never followed.

#### Why is it off by default?

Because the database comes from ClamAV's server, and nothing leaves your machine unless you
ask. Turning it on is that asking; Settings names the server.

---
[← Previous: Export draw.io diagram](drawio-export.md) · [Next: Customising →](../customise/README.md)

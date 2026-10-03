[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Cloud files: OneDrive, Dropbox, Google Drive, Proton Drive, iCloud

OneDrive, Dropbox, Google Drive, Proton Drive and iCloud can keep a file on your disk by its name
and size only, with its contents left in the cloud ("online-only", "Files On-Demand", "streamed",
"Optimise Mac storage"). Anything that reads such a file makes the cloud app download it. An
indexer that reads every file would download the whole cloud: gigabytes over the network, a full
disk, a machine that crawls. Versions before 1.28.0 could do that while indexing.

**Coxswain never downloads a file that is only online unless you ask for that one file.** It
finds online-only files by name, shows them with a cloud glyph, and leaves their contents in the
cloud. Files that are on your disk, inside a cloud folder or not, are read as before.

<!-- screenshot: search-cloud-files.png: desktop app, Cyber theme, a OneDrive folder in Details view with three files marked by the cloud glyph after their names, and the preview pane on one of them saying "Online only: not downloaded. Press Enter to download and open it, or:" with the button "Download and preview" -->

## How to use it

Nothing to do: it is on from the start, in both apps, on Windows, macOS and Linux.

- **To see one file:** in the desktop app the preview says *Online only: not downloaded. Press
  Enter to download and open it, or:* with the button **Download and preview**, which downloads it
  and previews it. In the terminal app **F3** views it (and downloads it). **Enter**, **F4**,
  **F5** (copy) and **F6** (move to another disk) work on it as on any file, and download it,
  because you asked.
- **To search inside a cloud folder:** let the cloud app keep it on this machine: in OneDrive,
  right-click the folder and choose *Always keep on this device*; in Dropbox and Google Drive,
  *Make available offline* (*Available offline*); in iCloud Drive, *Keep Downloaded*; in Proton
  Drive, *Available offline*. The cloud app downloads it on its own schedule, and Coxswain reads
  the files from disk at its next pass.
- **To read every online-only file anyway** (they are downloaded): *Settings → Search inside files
  → Cloud files: read files that are only online (downloads them)*, or `cloud = "all"` under
  `[search]` in `config.toml`.
- **To read one cloud folder anyway** (a cloud mount on Linux, say): under *Read anyway*, press
  **Read its files** next to the cloud Coxswain found, or add the folder. In `config.toml`:
  `cloud_read = ["/home/me/gdrive"]`.

## What you see

- **A cloud glyph** after the name of an online-only file: in the desktop app's Details, Columns
  and Thumbnails views (tooltip *Online only: not downloaded*), and in the terminal app's git
  column (`*` with `glyphs = "ascii"`). The terminal app's info line adds *online only · F3
  downloads it*. Folders get no glyph.
- **No thumbnail** for an online-only picture in the Thumbnails view: its icon instead.
- **The preview pane** says *Online only: not downloaded.* and reads nothing: no text, no picture,
  no file facts (EXIF, tags), no git diff, until you press **Download and preview**.
- **A notice, once**, the first time the helper finds online-only files: *Found OneDrive, Dropbox:
  files that are only online are found by name only, so nothing is downloaded. Change in
  Settings*. Clicking it opens *Settings → Search inside files*. The terminal app's says
  *… cloud = "all" in config.toml reads them*.
- **Settings → Search inside files** has the box *Cloud files: read files that are only online
  (downloads them)*, off; under *Read anyway*, each cloud found (*OneDrive*,
  `C:\Users\me\OneDrive`) with **Read its files**, and the folders you added.

## What is read, and what is not

| | A file only online | A file on the disk (kept on this device, or downloaded earlier) |
|---|---|---|
| Found by name, in both apps | yes | yes |
| Listed in panels, with size and date | yes | yes |
| Counted in [folder sizes](../panels/folder-sizes.md) | yes, from its size on record: nothing is opened | yes |
| Its [text](text.md), [meaning](meaning.md) and [Ask](ask.md) | **no** | yes |
| The files inside it, when it is an [archive](archives.md) | **no** | yes |
| Hashed for [duplicates](../files/duplicates.md) | **no**: never a duplicate | yes |
| [Thumbnail](../panels/views.md), [preview](../previews/README.md), file facts | **no**, until you press **Download and preview** | yes |
| Git status, last commit, history in search, in a repository whose `.git` is online-only | **no** | yes |
| **F3**, **F4**, **Enter**, copy, move, pack, by you | yes: you asked, it downloads | yes |

When a file goes back to the cloud (OneDrive's *Free up space*, macOS evicting it), its text,
vectors and hash leave the index at the next pass, and it is found by name only. When a file comes
back to the disk (you opened it, or chose *Always keep on this device*), it is read at the next
pass. Reading stays as gentle as for any file: one at a time, with rests, never on battery unless
*Index now* ([Battery](battery.md)).

## How it is told

Only from the file's metadata, the same that a folder listing gives: Coxswain never opens a file
to find out, and never reads a link's target to find out. On Windows the attributes come from the
folder's listing (or `GetFileAttributesW`), never from opening the file, since opening one marked
`RECALL_ON_OPEN` would download it.

In the desktop app the guard is in two places: the page shows no preview of an online-only file,
and the app's own readers (text, archives, databases, books, mail, certificates, bills of
materials, previews made by tools, file facts, git diff) refuse one until you press **Download and
preview** or go into it yourself (an archive you open, say). That includes what the page loads
straight from the disk, pictures, thumbnails, PDFs, fonts, video, Office files and a page's own
stylesheet: they come through the app's own file protocol, which answers *forbidden* for an
online-only file that was not asked for.

| System | Clouds | Online-only when |
|---|---|---|
| **Windows** | OneDrive, Dropbox, Google Drive for desktop, Proton Drive, iCloud for Windows, Nextcloud: every app on the Cloud Files API | The file's attributes have `RECALL_ON_DATA_ACCESS` (0x400000), `RECALL_ON_OPEN` (0x40000) or `OFFLINE` (0x1000). *Always keep on this device* (`PINNED`) with the data on disk is read |
| **macOS** | iCloud Drive, and OneDrive, Dropbox, Google Drive and others under `~/Library/CloudStorage` (File Provider) | The file's flags have `SF_DATALESS` |
| **Linux** | Cloud mounts: rclone, google-drive-ocamlfuse, onedriver, GNOME's Google Drive (gvfs), WebDAV through davfs2, sshfs, and other FUSE file systems that are not local, FUSE without a subtype included (`/proc/self/mountinfo`) | Every file on such a mount, as the mount itself fetches what is read. Local FUSE file systems (the document portal, AppImages, mergerfs, gocryptfs, ntfs-3g and the like) are ordinary disks |

NFS and SMB shares are not cloud mounts: reading their files downloads nothing to keep, so they
are read like a disk (slowly; leave one out with *Names only* if it bites).

On Linux, the Dropbox, Insync, Proton and other clients that sync a normal folder keep every file
on disk: those folders are read like any other. A cloud mount whose files are not read is not
walked by the search store either, as listing it may take the network; its names are in the
[name index](names.md) as before.

The name of the cloud in the notice comes from its folder: `OneDrive…`, `Dropbox`, `Google
Drive`, `My Drive`, `Proton Drive`, `iCloud…`, a folder under `~/Library/CloudStorage`, or the
mount's type on Linux (`rclone`).

## Settings and config.toml

Under `[search]`:

| Key | Type | Default | Settings item |
|---|---|---|---|
| `cloud` | `"local-only"` or `"all"` | `"local-only"` | *Cloud files: read files that are only online (downloads them)* |
| `cloud_read` | list of paths | `[]` | *Read anyway* |

```toml
[search]
cloud = "local-only"                 # "all" reads online-only files too, downloading them
cloud_read = ["/home/me/gdrive"]     # read this cloud mount anyway
```

The [helper](helper.md) reads the setting when it starts; Settings restarts it.

## In the terminal app

The same files are left in the cloud, with the `*` or cloud glyph and the info line. There is no
Settings window: set `cloud` and `cloud_read` in `config.toml`. **F3** on an online-only file
downloads it and views it.

## Questions

#### Will Coxswain download my OneDrive?
No. Since 1.28.0 a file that is only online is found by name and left in the cloud: not read for
search, not hashed, no thumbnail, no preview, until you open that one file yourself. Files OneDrive
already keeps on your disk are read, as they cost no download.

#### An older version downloaded my OneDrive. How do I free the space again?
Update to 1.28.0 or later first. Then in Explorer right-click the OneDrive folder (or a part of it)
and choose *Free up space*: the files become online-only again, and Coxswain drops their text at
its next pass. On macOS, *Remove Download* in Finder does the same for iCloud Drive and
`~/Library/CloudStorage`.

#### How do I search inside my Dropbox files?
Make the folder available offline in Dropbox (*Make available offline*): Dropbox downloads it in
its own time, and Coxswain reads the files once they are on disk. Or let Coxswain download them:
*Settings → Search inside files → Read anyway → Read its files* next to Dropbox, or the box *Cloud
files: read files that are only online* for every cloud.

#### Does a file I opened once stay searchable?
As long as the cloud app keeps it on disk. If it frees the space again, the file goes back to
being found by name only.

#### Does anything break without a network?
No. Nothing waits on the network, since nothing online-only is read: names, listings, sizes and
the text of what is on disk all work offline. On Linux, a cloud mount itself may be slow to list
when offline; Coxswain does not walk one for search unless you read it.

#### I set `cloud = "all"`. Will Coxswain download everything at once?
It reads one file at a time with rests, as it does for local files, and never on battery unless
you press *Index now*: the cloud app downloads each as it is read. A big cloud still means a big
download and a full disk, so prefer *Read its files* on one folder, or the cloud app's own
*Always keep on this device*.

#### Does this cover Google Drive's "streamed" files and Proton Drive?
On Windows, every cloud app built on Windows' Cloud Files API marks its online files the same way,
and Coxswain reads that mark: Google Drive for desktop, Proton Drive, Nextcloud and iCloud for
Windows included. On macOS the File Provider apps (under `~/Library/CloudStorage`) do the
same. A cloud app with its own virtual drive that does not mark files is not recognised; add its
folder to *Names only* ([Choosing the folders](folders.md)).

#### Can I see which files are online-only?
In the panels: the cloud glyph after the name. Find file shows no glyph; the panel does once you
go to the file.

---
[← Previous: Choosing the folders](folders.md) · [Next: Removable disks →](removable-disks.md)

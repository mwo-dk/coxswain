[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Removable disks

A folder on a USB stick or an external drive can be one of the *Folders read*. Coxswain knows it
by its disk, not only by its path, so its text is kept while the disk is away and found again
wherever the disk is mounted next, without being read afresh.

<!-- screenshot: search-removable-away.png: desktop app, Cyber theme, Settings → Search inside files, Folders read listing /home/demo and /media/demo/BACKUP1, the second with "412 MB in the index, kept while its disk is not plugged in. Remove forgets it." -->

## How to use it

1. Plug the disk in and open a folder on it in the active panel.
2. *Settings → Search inside files → Folders read*: press **Add** with the field empty (or type
   the path, `/media/me/BACKUP1/papers`). In the terminal app, add it to `text_roots`.
3. Add your home folder too, if the list was empty: listing folders replaces the default.

From then on:

| The disk is | What happens |
|---|---|
| **Unplugged** | Its text stays in the index but out of search results. Settings shows it as away |
| **Plugged in again**, at the same place or another (`/media/me/BACKUP1` one day, `E:\` the next) | At the helper's next pass it is searched again, without being read afresh; only what changed is read |
| **Replaced** by another disk with files at the same place | The new disk is taken as it is, and read |
| **Removed** from *Folders read* | Its text is forgotten |

The disk is told apart by the file system's UUID on Linux and macOS, and by the volume serial on
Windows, with the folder's place inside that file system. A disk that cannot be told apart (a
network share, tmpfs) is known by its path only.

## What you see

In Settings, under *Folders read*: *412 MB in the index* while the disk is there, and *412 MB in
the index, kept while its disk is not plugged in. Remove forgets it.* while it is away. In Find
file, hits from an away disk are left out of both text and meaning results.

## Settings and config.toml

| Key | Type | Default | Does |
|---|---|---|---|
| `search.text_roots` | list of paths | `[]` | The folders read, a folder on a removable disk among them |

Nothing else: the disk is recognised by itself.

## In the terminal app

The same, with `text_roots` in `config.toml`. There is no list showing which disks are away.

## Questions

#### Why did my disk's files disappear from search?
The disk is not plugged in (or not mounted). Its text is kept, out of sight, and comes back when it
is mounted again, wherever. Settings shows it as away.

#### I plugged the disk back in; when do its files come back?
At the helper's next pass over the folders, within ten minutes. **Index now** in Settings starts
one at once.

#### I replaced the disk with a new one at the same place.
A folder with files in it on another disk is taken as it is: the new disk is read.

#### It was mounted at `/media/me/BACKUP1`, now at `/run/media/me/BACKUP1`. Is it read again?
No. The helper finds it by its UUID at the new place and moves what it knows there; only files
that changed are read.

#### Does this work for network shares?
A share has no UUID Coxswain can read, so it is known by its path. While the path is gone its text
is kept out of sight like a disk's, but it is not found at another path.

#### How do I get rid of an old disk's text?
**Remove** next to it under *Folders read*, or take it out of `text_roots`.

---
[← Previous: Choosing the folders](folders.md) · [Next: Search by meaning →](meaning.md)

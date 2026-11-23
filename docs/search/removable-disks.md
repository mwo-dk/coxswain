[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Removable disks

A folder on a USB stick or an external drive can be one of the *Folders read*. Coxswain knows it
by its disk, not only by its path, so its text is kept while the disk is away and found again
wherever the disk is mounted next, without being read afresh.

![Settings → Finding files → Details → Folders: Folders read lists /home/demo (14.3 MB in the index) and /run/media/demo/BACKUP1 with 412 MB in the index, kept while its disk is not plugged in. Remove forgets it., each with Remove](../screenshots/search-removable-away.png)

## How to use it

1. Plug the disk in and open a folder on it in the active panel.
2. *Settings → Finding files → Details → Folders → Folders read*: type `.` and press **Add** (or type
   the path, `/media/me/BACKUP1/papers`). In the terminal app: **F12** → *Finding files* →
   *Folders* → *Folders read*, **Enter**, and type `.` or the path on the *+ Add* line.
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
network share, tmpfs) is known by its path only, and so is every disk on
[FreeBSD](../systems/freebsd.md#how-freebsd-differs-from-linux-here), NetBSD, OpenBSD and illumos for now.

## What you see

In Settings, under *Folders read*: *412 MB in the index* while the disk is there, and *412 MB in
the index, kept while its disk is not plugged in. Remove forgets it.* while it is away. In Find,
hits from an away disk are left out of both text and meaning results.

## Settings and config.toml

| Key | Type | Default | Does |
|---|---|---|---|
| `search.text_roots` | list of paths | `[]` | The folders read, a folder on a removable disk among them |

Nothing else: the disk is recognised by itself.

## In the terminal app

The same, with *Folders read* in its Settings (**F12** → *Finding files* → *Folders*) or
`text_roots` in `config.toml`. Its Settings list the folders but do not say which disks are away.

## Questions

#### Why did my disk's files disappear from search?

The disk is not plugged in (or not mounted). Its text is kept, out of sight, and comes back when it
is mounted again, wherever. Settings shows it as away.

#### I plugged the disk back in; when do its files come back?

At the helper's next pass over the folders, within ten minutes. **Read now** in Settings starts
one at once.

#### I replaced the disk with a new one at the same place.

A folder with files in it on another disk is taken as it is: the new disk is read.

#### It was mounted at `/media/me/BACKUP1`, now at `/run/media/me/BACKUP1`. Is it read again?

No. The helper finds it by its UUID at the new place and moves what it knows there; only files
that changed are read.

#### Does this work for network shares?

A share has no UUID Coxswain can read, so it is known by its path. While the path is gone its text
is kept out of sight like a disk's, but it is not found at another path.

#### A network folder inside my home folder dropped. Is it read again when it is back?

No. A folder the helper cannot list for now (an sshfs or NFS mount that dropped, a folder the system
refuses, a read error) keeps its text and its vectors, and its files are still found. When it
answers again, only the files that changed are read. Only a folder that is not there any more
loses its text.

#### A disk is mounted inside my home folder. What happens when it is unplugged?

Its text and vectors stay, out of sight, as for a disk that is one of the *Folders read*. The
helper notes each folder inside the folders read where another disk is mounted (an external
drive at `~/backup`, an encrypted folder at `~/Private`). When such a folder is later empty and
on its parent's disk, the disk is taken as away: Find leaves its files out until it is mounted
there again, and then only the files that changed are read. An empty folder there cannot be told
from one emptied by hand: to let its text go, delete the empty folder. Links left in it (a locked
ecryptfs folder holds two) and a Finder's `.DS_Store` do not count as files. A folder that goes
with its disk (one an automounter made under `/run/media`) is gone, not away: when it sits inside
a folder read, its text goes. On Windows a disk mounted in a folder is not walked into, so this
does not arise there.

#### How do I get rid of an old disk's text?

**Remove** next to it under *Folders read*, or take it out of `text_roots`. For a disk that was
mounted inside one of the folders read, delete the empty folder it was mounted on: at the next
pass its text goes.

---
[← Previous: Cloud files](cloud-files.md) · [Next: Search by meaning →](meaning.md)

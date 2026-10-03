[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Choosing the folders: folders read, names only, left out, .nosearch

You choose which folders have their text read for [Text in files](text.md), and which are found
by name only. By default it is your home folder, less hidden folders and build output. Use this
to add a data disk, or to keep a mail store or other people's papers out of the index.

![Settings, Search inside files: the box "Keep the text of files", Searchable: 107 files · still to read: 0 · 284 KB on disk, the path of search.db, Index now and Delete the index, Folders read "Your home folder" and Names only "None", each with a field and Add](../screenshots/gui-settings-search.png)
*Settings → Search inside files, with the lists of folders.*

## How to use it

**Desktop app.** Open Settings (**Ctrl+,**, or start with `coxswain-gui --settings=search`) and go
to *Search inside files*. It has three lists, each with a field and **Add**:

1. Type or paste a folder in the field under *Folders read* or *Names only*. A relative path is
   taken from the active panel's folder; an empty field adds the active panel's folder itself
   (it is shown as the placeholder).
2. Press **Add** (or **Enter** in the field).
3. **Remove** next to a folder takes it off the list.
4. Under *Left out*, type a folder name (`Mail`) or a file pattern (`*.log`, `secret*`,
   `notes.txt`) and press **Add**: it is never read, wherever it is. The **×** on an entry takes it
   off. The list starts with `node_modules`, `target`, `build`, `dist`, `out`, `vendor`,
   `__pycache__` and `Trash`.

**Any app, any folder.** Put an empty file named `.nosearch` in a folder:
`touch ~/Private/.nosearch`. It does the same as *Names only*, for that folder and all below it.
Delete the file and the folder is read again.

**Terminal app.** Set the lists in `config.toml` (below).

| List | Key | Does |
|---|---|---|
| **Folders read** | `text_roots` | The folders whose files are read. None listed means your home folder. |
| **Names only** | `names_only` | Folders whose files are found by name and counted in folder sizes, but never read: mail stores, archives of other people's documents, anything whose text you do not want in the index |
| `.nosearch` | a file | The same as *Names only*, set from the folder itself |
| **Left out** | `text_exclude` | Folder names and file patterns left out wherever they are. A plain name (`build`) leaves out folders of that name; an entry with `*`, `?` or a dot (`*.log`, `notes.txt`) leaves out files too. `*` stands for any run of characters, `?` for one; the case counts |

Files that are only online in OneDrive, Dropbox, Google Drive, Proton Drive or iCloud are never
read either, wherever they are: they are found by name and left in the cloud, unless you say
otherwise. See [Cloud files](cloud-files.md).

Hidden folders (their names start with a dot) are always left out of reading, and so is
Coxswain's own cache folder (`search.db`, previews, copies looked at) on every system. All of them
are still in the [name index](names.md), and a file left out still counts in folder sizes.

## What you see

Under *Folders read*, each folder shows how much of it is in the index: *412 MB in the index*.
With none listed it says *Your home folder · 412 MB in the index*. A folder on a disk that is not
plugged in says *412 MB in the index, kept while its disk is not plugged in. Remove forgets it.*
(see [Removable disks](removable-disks.md)). *Names only* says *None* while it is empty.

Changing a list starts a new [helper](helper.md) with the new settings: files newly left out go
from the index, files newly included are read, and *still to read* counts them.

Under *Left out*: *Folder names (node_modules) and file patterns (*.log, secret*) never read,
wherever they are; they are still found by name. To leave out one folder, add it under Names only,
or put an empty file named .nosearch in it.*

Above the lists, the path of the store, `search.db`, with **Show in panel**: Settings closes and
the active panel opens its folder with the cursor on it, so the preview (or **F3**) shows its
tables. With the built-in model for search by meaning, its folder has a **Show in panel** too.

The hint under the lists: *Names-only folders are found by name and counted in folder sizes, but
never read. A file named .nosearch in a folder does the same. Normally files are read at half
speed, one at a time; Index now reads the backlog at full speed.*

## Settings and config.toml

Under `[search]`:

| Key | Type | Default | Settings item |
|---|---|---|---|
| `text_roots` | list of paths | `[]` (your home folder) | *Folders read* |
| `names_only` | list of paths | `[]` | *Names only* |
| `text_exclude` | list of strings | `["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash"]` | *Left out* |
| `text_max_size` | bytes | `20971520` | None |

`text_exclude` replaces the default list, so repeat what you want to keep:

```toml
[search]
text_roots = ["/home/me", "/mnt/data"]
names_only = ["/home/me/Mail"]
text_exclude = ["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash", "Mail", "*.log"]
text_max_size = 52428800   # 50 MB
```

## In the terminal app

There is no Settings window: edit `config.toml` (its path: `coxswain --config-path`). The helper
reads the config when it starts, so a change made by hand takes effect at its next start (see the
questions). `.nosearch` works the same from any app.

## Questions

#### How do I make a folder names-only?
Desktop app: *Settings → Search inside files → Names only*, type or paste the folder and press
**Add** (or open the folder in the active panel and press **Add** with the field empty). From
anywhere: put an empty file named `.nosearch` in it. In `config.toml`:
`names_only = ["/home/me/Mail"]` under `[search]`.

#### How do I leave out files like `*.log`, or one file?
Add the pattern under *Settings → Search inside files → Left out* (`*.log`, `*.csv`, `secret*`), or
the file's name (`passwords.txt`): every file that matches, anywhere, loses its text from the
index at the next scan and is never read again. It is still found by name. In `config.toml`, add it
to `text_exclude`.

#### Where is `search.db`, and can I look inside?
In Coxswain's cache folder: `~/.cache/coxswain/search.db` on Linux, `~/Library/Caches/coxswain` on
macOS, `%LOCALAPPDATA%\coxswain` on Windows (`coxswain --paths` prints it). *Settings → Search
inside files → Show in panel* opens it in the active panel; the preview shows its tables and their
row counts, and **F3** in the terminal app their first rows too, read-only, also while the helper
writes to it. The cache
folder itself is never read into the index, so the store does not index itself.

#### I added a folder outside my home folder, and now my home folder is not read.
Listing folders replaces the default. Add your home folder to *Folders read* too.

#### I changed `[search]` in `config.toml` by hand and nothing happened.
The helper reads the config when it starts. Changes made in Settings restart it; a change made by
hand takes effect when it next starts. Make any change under *Settings → Search inside files*, or
close every Coxswain window and wait ten minutes. With the helper started with your session, run
`coxswain --index-service off` and then `on`.

#### Is a names-only folder still in folder sizes?
In [folder sizes](../panels/folder-sizes.md), yes: its files are counted, never read. Its names are
found by [names everywhere](names.md) as before.

#### What happens to text already in the index when I leave a folder out?
It goes from the index when the new helper walks the folders read.

#### Can I read a hidden folder, like `~/.notes`?
No. Hidden folders are always left out of reading. Their files are still found by name.

#### Can I read `node_modules` or `build` after all?
Yes: press its **×** under *Left out*, or set `text_exclude` without that name. In `config.toml`
the list replaces the default one, so list the others you still want left out.

#### Will adding my OneDrive folder download it?
No. Its files that are only online are found by name and never read, so nothing is downloaded;
the files OneDrive keeps on your disk are read. See [Cloud files](cloud-files.md).

#### Does `.nosearch` hide a folder from name search too?
No, only from reading. To leave a folder out of the name index, use `exclude` (see
[Names everywhere](names.md#settings-and-configtoml)).

---
[← Previous: Inside archives](archives.md) · [Next: Cloud files →](cloud-files.md)

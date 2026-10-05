[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Folder sizes

Folders show their size without being asked, in both apps, so you can see at a glance where
the space goes. This page explains where the sizes come from, how fresh they are, how to
measure again, and how to switch it off.

![The details view of the home folder with every column on: each folder's size and the number of files in it](../screenshots/gui-folder-sizes.png)
*The desktop app with the Files and Created columns on: `projects` holds 3.1 MB in 102 files.*

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Where the sizes come from](#where-the-sizes-come-from)
- [A folder in the preview pane and in Properties](#a-folder-in-the-preview-pane-and-in-properties)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Open a folder. Coxswain measures the folders in it in the background and fills in their
   sizes as they come: the first ones first, one at a time, so they appear quickly.
2. To measure again now, pick **Folder sizes** in the command list (**F9**, type `folder`).
   It measures afresh, whatever is remembered and whether or not automatic measuring is on:
   - Terminal app: every folder in the active panel.
   - Desktop app: the marked folders, or every folder in the tab when none is marked.
3. To switch automatic sizes off: in the desktop app untick *Measure folder sizes
   automatically* in the columns menu (right-click a column header, or **F9** → *Columns and
   folder sizes*); in the terminal app set `folder_sizes = false`.

**Folder sizes** has no key by default (the natural one, Ctrl+Space, belongs to the system on
a Mac). Give it one: `[keys] folder_sizes = ["Ctrl+Q"]` (`dir_sizes` before 2.0, renamed on the
first start; see [Renamed in 2.0](../reference/configuration.md#renamed-in-20)).

## What you see

| | Terminal app | Desktop app |
|---|---|---|
| Before it is measured | `SUB-DIR` in the Size column | An empty Size cell |
| Measured | The size in the Size column | The size in the Size column; with the *Files* column on, the number of files inside, all levels down |
| While measuring on request | – | Status line: `Measuring 12 folders…` |
| Marked folders | Not counted in `… in 3 marked` | Counted in `3 marked (…)` once measured |

## Where the sizes come from

- **Inside the folders search reads** (your home folder unless you chose others; see
  [Choosing the folders](../search/folders.md)): at once. The
  [search helper](../search/helper.md) already knows every file's size from its last walk, so
  a folder's size is a sum it has at hand. This needs *Words inside files* on (Settings → *Finding files*).
- **Everywhere else:** measured by walking the folder, on two threads at most, so the machine
  stays yours. Leaving the folder stops the walk.

**How fresh they are.** A measured size is believed for five minutes, so going back to a
folder shows its sizes at once. Copying, moving, deleting, pasting, packing or extracting with
Coxswain forgets the sizes of the folders involved, and they are measured again. In both apps,
a change the file watcher sees in a folder on screen does the same. The helper's
sizes follow its own watcher, and its walk every ten minutes.

**What is counted:** the bytes of every file below the folder, as the file system reports its
length. Symbolic links are counted as links, never followed. Folders that cannot be read are
skipped. `/proc`, `/sys`, `/dev` and `/run` are never measured.

## A folder in the preview pane and in Properties

With the cursor on a folder, the desktop app's [preview pane](../previews/README.md)
(**Space**) shows *Contents* (how many folders and files it holds directly), *Files here*
(the size of those files), *Newest*, its **Total size** (a *Calculate* link when it has not
been measured yet) and its *Git* line.

**Properties** (**Alt+Enter**, desktop app) walks the folder afresh and shows its size in
bytes and the number of files in it; see [Properties and permissions](../files/properties.md).

## Settings and config.toml

| Setting | config.toml | Type, default |
|---|---|---|
| *Measure folder sizes automatically* (columns menu, desktop app) | – (session) | on |
| Settings → *Behaviour* → *Measure folder sizes* | `folder_sizes` | bool, `true` |
| *Files (in folders)* column | – (session) | off |
| Folder sizes command | `[keys] folder_sizes` | no key |

`folder_sizes` is where the desktop app starts; after that the columns menu switch is kept in
its session. The terminal app reads `folder_sizes` at every start.

## In the terminal app

The same sizes from the same places, in the Size column in place of `SUB-DIR`. It has no Files
column, and nothing on screen says it is measuring: sizes simply fill in. It does not watch
folders, so a change made outside it is noticed after five minutes, on **Ctrl+R** after
that, or with **Folder sizes** in the **F9** list.

## Questions

#### Why is a folder's size not shown?

- It is still being measured: large folders take a while, and they are done one at a time.
- You left the folder: the walk stops, and starts again when you come back.
- Measuring is off: *Measure folder sizes* in Settings → *Behaviour* or `folder_sizes = false`
  (both apps), or the columns menu switch (desktop app).
- It is `/proc`, `/sys`, `/dev` or `/run`, which are never measured.
- Desktop app: the Size column is hidden, or the pane is too narrow for it
  ([Views](views.md#what-you-see)).

#### Why are sizes in my home folder instant, and slow elsewhere?

The search helper keeps every file's size for the folders it reads (your home folder by
default), so those are sums. Other folders are walked. Add a folder under *Settings → Finding
files → Details → Folders → Folders read* to have its sizes at once too, or to *Names only*, which keeps
its sizes without reading its files.

#### A size is out of date. Why?

A change made outside Coxswain deep inside a folder that is not on screen goes unnoticed for
up to five minutes, the time a measured size is believed. Pick **Folder sizes** in the **F9**
list to measure again now.

#### Why does Coxswain's size differ from `du`?

`du` counts the disk blocks a file takes; Coxswain counts the bytes in it, as a file's own
size column does. Sparse files, compression and many small files make the two differ. A file
with two hard links is counted twice, and a symbolic link is not followed.

#### Does measuring slow my machine?

It uses two threads at most, and stops when you leave the folder. On a slow network share the
walk takes long; switch measuring off while you work there.

#### Can I sort by folder size?

No. **Ctrl+F6** sorts files by size; folders keep name order among themselves
([Sorting](sorting.md#why-are-folders-not-sorted-by-their-size)).

#### My `dir_sizes` key stopped working in 2.0. Why?

The action is called `folder_sizes` since 2.0, the same word as the `folder_sizes` switch. The
first start of 2.0 renames it in `config.toml` and lists it in a notice; an old name typed in
afterwards makes the config invalid. See [Renamed in 2.0](../reference/configuration.md#renamed-in-20).

#### Why do folders inside an archive show 0 B?

A folder inside an archive is not on disk, so the walk finds nothing in it. The archive
file itself shows its real size in the folder that holds it; open it to see its files' sizes
([Archives as folders](../files/archives.md)).

---
[← Previous: Views: details, columns, thumbnails](views.md) · [Next: Git in the panels →](git.md)

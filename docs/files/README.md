[← README](../../README.md) · [Docs index](../README.md)

# Files

Copy, move, rename, make folders and delete, in both apps with the same F-keys. Archives open
like folders, so the same keys copy files into them, out of them and between them, and
**Alt+F5** packs files into a new one. The desktop app adds the system clipboard, drag and
drop, batch rename with a regular expression and a duplicate finder. On ZFS, **Alt+Z** browses a
folder's snapshots.

All operations work on the **marked** files, or on the file under the cursor when nothing is
marked ([Marking files](../panels/marking.md)). `..` is never a target.

![The desktop app with a tar.gz archive under the cursor in the right pane and its contents listed in the preview pane](../screenshots/gui-archive.png)
*An archive under the cursor: the preview pane lists what is in it. **Enter** opens it like a
folder; **Ctrl+E** extracts it.*

| Page | What it covers |
|---|---|
| [Copy (F5), and when something goes wrong](copy.md) | Copying files and folders, the target field, never overwriting, the error list |
| [Move and rename (F6)](move-and-rename.md) | Moving, renaming in place (**Shift+F6**), moves across disks |
| [New folder (F7)](new-folder.md) | Making a folder, or a whole path of folders at once |
| [Delete: trash (F8) or for good (Shift+F8)](delete.md) | The trash, deleting for good, the question and how to skip it |
| [Clipboard: Ctrl+C, Ctrl+X, Ctrl+V](clipboard.md) | Copy and cut through the system clipboard, shared with other file managers |
| [Drag and drop](drag-and-drop.md) | Dragging files out to other applications, between panes and in |
| [Batch rename (Ctrl+M)](batch-rename.md) | Renaming many files with a regular expression and a counter, previewed |
| [Archives as folders](archives.md) | Opening zip, tar and 7z like folders; copying in, out and between; formats; why not RAR |
| [Pack (Alt+F5) and extract (Ctrl+E)](pack-and-extract.md) | Making a new archive, and unpacking one into a folder of its own |
| [Passwords for encrypted zip and 7z](archive-passwords.md) | The password question, and how long Coxswain keeps a password |
| [Properties and permissions](properties.md) | Sizes, dates, owner, and changing the permission bits |
| [Finding duplicates](duplicates.md) | Duplicate files and whole duplicate folders, across disks, removed safely |
| [ZFS snapshots as folders](zfs-snapshots.md) | **Alt+Z**: a dataset's snapshots browsed read-only, a file compared with now, copied back with **F5** |

## Keys at a glance

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| **F5** | yes | yes | [Copy](copy.md) to the other panel (or anywhere you type) |
| **F6** | yes | yes | [Move or rename](move-and-rename.md) |
| **Shift+F6** | yes | yes | [Rename](move-and-rename.md) in place |
| **Shift+F10**, **Menu** | yes | yes | [What can I do with this?](../panels/action-menu.md): the actions that fit |
| **F7** | yes | yes | [New folder](new-folder.md) |
| **F8**, **Delete** | yes | yes | [Move to the trash](delete.md); inside an archive, take out of it |
| **Shift+F8**, **Shift+Delete** | yes | yes | [Delete for good](delete.md) |
| **Enter** on an archive | yes | yes | [Open it like a folder](archives.md) |
| **Backspace** | yes | yes | Go up, also out of an archive |
| **Ctrl+E** | yes | yes | [Extract](pack-and-extract.md) the archive into a new folder |
| **Alt+F5** | yes | yes | [Pack](pack-and-extract.md) the files into a new archive |
| **Ctrl+C**, **Ctrl+X**, **Ctrl+V** | yes | no | [Clipboard](clipboard.md) copy, cut, paste |
| Drag with the mouse | yes | no | [Drag and drop](drag-and-drop.md) |
| **Ctrl+M** | yes | no | [Batch rename](batch-rename.md) |
| **Alt+Enter** | yes | yes (shown, not changed) | [Properties and permissions](properties.md), with ZFS, package and file flags |
| **Alt+Z** | yes | yes | [ZFS snapshots](zfs-snapshots.md) of the folder's dataset |
| *F9 → File flags* | in Properties | yes | Your own [file flags](properties.md#zfs-packages-and-file-flags) (FreeBSD, macOS) |
| *F9 → Files of this package* | yes | yes | The files of the [package](properties.md#zfs-packages-and-file-flags) a file belongs to (FreeBSD) |
| **Ctrl+D** | yes | no | [Find duplicates](duplicates.md) |

Every key can be changed ([Changing keys](../customise/keys.md)). In the terminal app a key that
only the desktop app has says so on the status line: *Batch rename is available in the desktop
app (coxswain-gui)*.

---
[← Previous: Safety, speed and limits](../previews/safety.md) · [Next: Copy (F5) →](copy.md)

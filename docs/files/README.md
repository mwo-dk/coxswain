[← README](../../README.md) · [Docs index](../README.md)

# Files

Copy, move, rename, make folders and delete, in both apps with the same F-keys. Archives open
like folders, so the same keys copy files into them, out of them and between them, and
**Alt+F5** packs files into a new one. The desktop app adds the system clipboard, drag and
drop (the terminal app takes drops only), batch rename with a regular expression and a duplicate finder. On ZFS, **Alt+Z** browses a
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
| [Undo (Ctrl+Z)](undo.md) | Undoing the last copies, moves, renames, new folders, trashings, packs, extracts, find-and-replaces and changes of permissions and dates; what is refused and why |
| [Clipboard: Ctrl+C, Ctrl+X, Ctrl+V](clipboard.md) | Copy and cut through the system clipboard, shared with other file managers |
| [Drag and drop](drag-and-drop.md) | Dragging files out to other applications (out of archives too), between panes, onto folders and in |
| [Batch rename (Ctrl+M)](batch-rename.md) | Renaming many files with a regular expression and a counter, previewed |
| [Find and replace across files (Alt+R)](find-and-replace.md) | Replacing text or a regular expression in many files, every change previewed first, undone in one step |
| [Archives as folders](archives.md) | Opening zip, tar and 7z like folders; copying in, out and between; formats; why not RAR |
| [Pack (Alt+F5) and extract (Ctrl+E)](pack-and-extract.md) | Making a new archive, and unpacking one into a folder of its own |
| [Passwords for encrypted zip and 7z](archive-passwords.md) | The password question, and how long Coxswain keeps a password |
| [Disk images, CHM help files and CBR comics](disk-images.md) | An image's facts in the preview; with the option on, **Enter** opens .iso, .dmg, .vhd, .vmdk, .qcow2, .chm and .cbr like folders, read-only, through 7-Zip |
| [Container images](container-images.md) | **Enter** opens a `docker save` / `podman save` tar or an OCI image layout as one read-only folder of its merged layers; its tags, platforms and layers in the preview |
| [Your local models, by name](local-models.md) | Ollama's sha256 blobs and the Hugging Face cache named by model; a GGUF's or safetensors file's facts, the memory it needs and whether it fits |
| [Properties and permissions](properties.md) | Sizes, dates, owner, and changing the permission bits |
| [Permissions and dates in bulk (Alt+M)](permissions-and-dates.md) | Permission bits and modified dates of many files at once, into folders if asked, planned first, undone in one step |
| [Finding duplicates](duplicates.md) | Duplicate files and whole duplicate folders, across disks, removed safely |
| [Compare files (Shift+F2)](compare-files.md) | Two files side by side: changed lines and words of texts and documents, pictures by their pixels, other files byte by byte; a change copied across and saved |
| [Compare folders (Ctrl+F2)](compare-folders.md) | The left folder against the right, recursively, by size and date or byte by byte: marks in both panels, differences only, mark and copy, make one side like the other after a plan |
| [ZFS snapshots as folders](zfs-snapshots.md) | **Alt+Z**: a dataset's snapshots browsed read-only, a file compared with now, copied back with **F5** |
| [Make PDF searchable (Alt+P)](searchable-pdf.md) | A copy of a scanned PDF with its words in it, by OCRmyPDF, installed or in a container; next to the original, never over it |
| [Convert video or audio](convert-media.md) | MP4, MP3 or FLAC copies, the sound alone or a clip, by ffmpeg; next to the original, never over it |
| [Convert or edit pictures](edit-pictures.md) | JPEG, PNG or WebP copies, smaller, without the location, turned, by ImageMagick; next to the original, never over it |
| [Checksums (Alt+H)](checksums.md) | A SHA256SUMS, SHA512SUMS or B3SUMS of the marked files and folders; **Enter** on such a list checks it: OK, changed, missing |
| [Split and join (Alt+X)](split-and-join.md) | A big file in parts of a chosen size (`name.001` …) with its SHA-256; **Enter** on `name.001` joins them back and checks the result |
| [Build PDF of AsciiDoc](asciidoc-pdf.md) | A PDF of an `.adoc` document next to it, by Asciidoctor PDF installed or in its container, its diagrams drawn here; nothing from the web |
| [Export draw.io diagram](drawio-export.md) | PNG or SVG of each page, or one PDF, next to a `.drawio` (or `.drawio.svg`, `.drawio.png`) diagram, by draw.io only in its sealed container (podman or docker); off until turned on |
| [Scan for viruses (Alt+I)](virus-scan.md) | ClamAV in a container without network, on a file, the marked files or a folder, only when asked; for each file found: leave it, bin or quarantine |

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
| **Ctrl+Z** | yes | yes | [Undo](undo.md) the last file operation |
| **Enter** on an archive | yes | yes | [Open it like a folder](archives.md) |
| **Backspace** | yes | yes | Go up, also out of an archive |
| **Ctrl+E** | yes | yes | [Extract](pack-and-extract.md) the archive into a new folder |
| **Alt+F5** | yes | yes | [Pack](pack-and-extract.md) the files into a new archive |
| **Ctrl+C**, **Ctrl+X**, **Ctrl+V** | yes | no | [Clipboard](clipboard.md) copy, cut, paste |
| Drag with the mouse | yes | drops in only | [Drag and drop](drag-and-drop.md) |
| **Ctrl+M** | yes | no | [Batch rename](batch-rename.md) |
| **Alt+R** | yes | yes | [Find and replace](find-and-replace.md) across the marked files or Find's results |
| **Alt+Enter** | yes | yes (shown, not changed) | [Properties and permissions](properties.md), with ZFS, package and file flags |
| **Alt+M** | yes | yes | [Permissions and dates](permissions-and-dates.md) of the marked files, in bulk |
| **Alt+Z** | yes | yes | [ZFS snapshots](zfs-snapshots.md) of the folder's dataset |
| *F9 → File flags* | in Properties | yes | Your own [file flags](properties.md#zfs-packages-and-file-flags) (FreeBSD, macOS) |
| *F9 → Files of this package* | yes | yes | The files of the [package](properties.md#zfs-packages-and-file-flags) a file belongs to (FreeBSD) |
| **Ctrl+D** | yes | no | [Find duplicates](duplicates.md) |
| **Ctrl+F2** | yes | yes | [Compare folders](compare-folders.md): the two panels' folders, every entry marked, the differences marked for F5 |
| **Shift+F2** | yes | yes | [Compare files](compare-files.md): the two cursors' files, or two marked ones, side by side |
| **Alt+P** | yes | yes | [Make PDF searchable](searchable-pdf.md): a copy with the words of a scan in it |
| *Shift+F10*, *F9 → Convert video or audio* | yes | yes | [Convert video or audio](convert-media.md): MP4, MP3, FLAC, the sound, a clip |
| *Shift+F10*, *F9 → Convert or edit pictures* | yes | yes | [Convert or edit pictures](edit-pictures.md): JPEG, PNG, WebP, smaller, without the location, turned |
| *Shift+F10*, *F9 → Export from KiCad* | yes | yes | [Export from KiCad](../previews/kicad.md#export-from-kicad): PDF, SVG, a bill of materials, Gerbers |
| *Shift+F10*, *F9 → Export 3D model* | yes | yes | [Export 3D model](../previews/openscad.md#export-3d-model): STL or 3MF of an OpenSCAD model |
| *Shift+F10*, *F9 → Convert geodata* | yes | yes | [Convert geodata](../previews/geodata.md#convert-geodata): GeoPackage, GeoJSON, a shapefile, KML, GPX, COG, PNG |
| **Alt+H** | yes | yes | [Make checksums](checksums.md#making-a-list): SHA256SUMS, SHA512SUMS or B3SUMS |
| **Enter** on a SHA256SUMS, `.sha256` … | yes | yes | [Verify checksums](checksums.md#checking-a-list): OK, changed, missing |
| **Alt+X** | yes | yes | [Split file](split-and-join.md#splitting-a-file) into parts for a USB stick, a CD or DVD |
| **Enter** on `name.001` | yes | yes | [Join parts](split-and-join.md#joining-the-parts) and check them against `name.sha256` |
| *Shift+F10 → Build PDF of AsciiDoc* | yes | yes | [A PDF of an AsciiDoc document](asciidoc-pdf.md) next to it |
| *Shift+F10 → Export draw.io diagram* | yes | yes | [PNG, SVG or PDF of a diagram](drawio-export.md) next to it |
| **Alt+I** | yes | yes | [Scan for viruses](virus-scan.md) with ClamAV, once turned on in Settings |

Every key can be changed ([Changing keys](../customise/keys.md)). In the terminal app a key that
only the desktop app has says so on the status line: *Batch rename is available in the desktop
app (coxswain-gui)*.

---
[← Previous: Safety, speed and limits](../previews/safety.md) · [Next: Copy (F5) →](copy.md)

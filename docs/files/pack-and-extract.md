[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Pack (Alt+F5) and extract (Ctrl+E)

**Alt+F5** packs the marked files and folders into a new archive: zip, 7z, or tar plain or
compressed, chosen by the name you give it. **Ctrl+E** unpacks an archive into a new folder named
after it. Both apps have both keys.

![The desktop app with website-0.3.0.tar.gz under the cursor in the right pane; the preview pane lists its five entries and says "Ctrl+E extracts to the other pane"](../screenshots/gui-archive.png)
*An archive under the cursor. The preview pane (Space) lists what is in it and reminds you that
**Ctrl+E** extracts it to the other pane.*

<!-- screenshot: files-pack.png: desktop app, Cyber theme: the Pack dialog with 'Pack 3 items into (.zip, .tar or .tar.gz):' and the field '/home/demo/Documents/rocket.tar.zst' -->

## How to use it

### Pack

1. Mark the files and folders to pack, or put the cursor on one.
2. Press **Alt+F5** (or *Pack into an archive* in the F9 command list). The dialog *Pack* opens
   with *Pack "photos" into (.zip, .tar or .tar.gz):* and a name in the other panel's folder:
   - one item: its name without its extension, `photos.zip` for a folder `photos`, `report.zip`
     for `report.pdf`;
   - several items: the name of the folder you are in, `rocket.zip`.
3. Change the name or its ending to choose the format.
4. For a zip or a 7z, type a password if you want one, and the same again below it; leave both
   empty for none. A 7z also has *Hide the file names too*, on by default. For a tar the fields
   are not there, and the dialog says *tar archives have no passwords: pack into .zip or .7z for
   one*. More in [Passwords](archive-passwords.md#locking-a-new-archive).
5. Press **Enter** (or *OK*). While the two passwords differ, *OK* is greyed out and the dialog
   says *The passwords do not match*.

| You type | You get |
|---|---|
| `photos.zip` (or `.jar` and the other zip endings) | A zip, Deflate-compressed |
| `photos.7z` | A 7z, LZMA2-compressed |
| `photos.tar` | A plain tar |
| `photos.tar.gz` or `.tgz` | A tar with gzip |
| `photos.tar.bz2`, `.tbz2`, `.tbz` | A tar with bzip2 |
| `photos.tar.xz`, `.txz` | A tar with xz (level 6) |
| `photos.tar.zst`, `.tzst` | A tar with zstd (its fastest level) |
| any other ending | Refused: `… is not an archive Coxswain reads` |

All of these can be packed, whatever the dialog's label lists. A relative name is taken from the
active panel's folder, `~/…` from your home folder.

### Extract

1. Put the cursor on an archive, or mark several.
2. Press **Ctrl+E** (or *Extract archive* in the F9 command list). The dialog *Extract* asks
   *Extract "photos.zip" into a new folder in:*, filled in with the other panel's folder.
3. Press **Enter**. Coxswain makes a folder named after the archive there (`photos` for
   `photos.zip`, `backup` for `backup.tar.gz`, `site` for `site.7z`) and unpacks into it. Several
   archives each get their own folder.

| Key | Desktop app | Terminal app |
|---|---|---|
| Pack | **Alt+F5** | **Alt+F5** |
| Extract | **Ctrl+E** | **Ctrl+E** |
| Confirm / cancel | **Enter** or *OK* / **Esc** or *Cancel* | **Enter** / **Esc** |

To take out only some files, open the archive with **Enter** and copy them with **F5**
([Archives as folders](archives.md)).

## What you see

- The password fields show dots. After packing with a password, the new archive opens without
  asking until you close the app; its badge says *archive, locked*.
- After packing, the status line says `Packed "photos"` or `Packed 3 items`, and the new archive
  shows in the other panel.
- After extracting, it says `Extracted "photos.zip"`.
- **Ctrl+E** on something that is not an archive says `Not a zip, 7z or tar archive` on the
  status line.
- Errors show in *Something went wrong*: an archive or folder that already exists (`… exists`),
  an ending Coxswain cannot write.
- A locked zip or 7z asks for its password before extracting
  ([Passwords](archive-passwords.md)).

## What goes in, what comes out

**Packing** takes folders with everything in them, hidden files and empty folders included.
Symbolic links are left out, since what they point at may be anywhere. Files keep their
modification dates. The archive is written to a `.coxswain-tmp` file first and gets its name only
when it is complete, so a failure leaves no half archive. With a password, a zip's files are each
locked with AES-256, and a 7z's contents too (and its names, with *Hide the file names too*).

**Extracting** never writes over anything: if the folder named after the archive is already
there, it stops with `… exists`. Entries that would land outside the new folder (`../`, absolute
paths) are not written. If extracting fails half-way, the new folder is removed again.

## Settings and config.toml

None. The keys are `pack` and `extract` in `[keys]` ([Changing keys](../customise/keys.md)).

## In the terminal app

The same keys, formats and texts. **Ctrl+U** clears the field. For a zip or 7z target, the
target prompt is followed by a password prompt (stars; **Enter** alone packs without one) and,
when you typed one, a prompt to type it again; if the two differ, the status line says *The
passwords do not match* and nothing is packed. A 7z packed from the terminal app always hides its
file names too. There is no preview pane,
so the list of contents before extracting is seen by opening the archive with **Enter**.

## Questions

#### How do I extract into the folder I am in?

Change the target in the dialog to `.` (the active panel's folder); the new folder named after
the archive is still made there.

#### Can I extract without the extra folder?

No: Coxswain always makes one, so an archive full of loose files never spills into a folder.
Open the archive with **Enter** instead, mark everything and copy it with **F5** where you want it.

#### How do I choose between zip, 7z and tar.gz?

By the ending you type in the Pack dialog. Zip opens everywhere, also on Windows and macOS
without extra programs. 7z and `.tar.xz` are usually the smallest. `.tar.zst` is the quickest to
make. Tar keeps Unix permissions.

#### The dialog says ".zip, .tar or .tar.gz". Can I really make a 7z?

Yes. The label is older than the other formats; every format in the table above can be packed.

#### Can I pack files into an archive with a password?

Yes, into a zip or a 7z: type it in the Pack dialog's password fields. See
[Locking a new archive](archive-passwords.md#locking-a-new-archive).

#### Why was a symbolic link not packed?

Links are left out on purpose: a link can point anywhere, also outside what you packed. Pack the
folder it points at, or use `tar -cf` on the command line if you need the link itself.

#### Can I pack files that are inside another archive?

Not directly: packing takes files on disk. Copy them out first with **F5**, then pack them. To
add them to an archive that already exists, open both archives and copy across with **F5**.

#### Extract says "exists". What now?

A folder of the archive's name is already in the target. Rename or remove it, or extract into
another folder.

---
[← Previous: Archives as folders](archives.md) · [Next: Passwords for encrypted zip and 7z →](archive-passwords.md)

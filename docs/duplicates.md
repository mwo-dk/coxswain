[← README](../README.md) · [Docs index](README.md)

# Finding duplicates

The desktop app finds duplicate files and whole duplicate folders: across folders, across
disks, in old backups. It shows how much space they waste and helps you remove the extra
copies safely.

![The Duplicates window: a duplicated Pictures folder, a PDF downloaded twice, a photo in three places and a document in an old backup](screenshots/gui-duplicates.png)

## Using it

1. Press **Ctrl+D**, or pick **Find duplicates** in the F9 list. From a terminal,
   `coxswain-gui --duplicates ~/Pictures /mnt/old-disk` starts straight in a scan of those
   folders.
2. **Look in:** tick the places to compare. Offered are both panes' folders, your favorites
   and every drive in the sidebar; type any other folder into *Add a folder…* and press Enter.
   Comparing across disks is the point: tick your home and an old backup disk to find what is
   stored twice.
3. **Options:**
   - *Ignore files under:* skips small files (1 KB by default).
   - *Include hidden files:* also looks inside dot-files and dot-folders.
   - *Find duplicate folders:* reports whole identical folders as one entry.
4. **Scan.** The bar shows the phase, the progress and how much has been read. **Cancel**
   stops at once.
5. **Review.** Groups are sorted by the space they waste, largest first. Each group lists its
   copies with their folder, modification date and, for pictures, a thumbnail. **Show** closes
   the window and puts the cursor on that copy in the pane, so you can preview it or look
   around.
6. **Mark** the copies to remove, with the tick boxes or a rule:
   - **Mark all but the newest:** keeps the most recently changed copy of each file.
   - **Mark all but the oldest:** keeps the original.
   - **Mark all but the one under** *folder*: keeps the copy inside the folder you choose,
     e.g. your real `Pictures` rather than the backup.
   - **Clear marks.**
7. **Move marked to trash.** After a confirmation, the marked copies go to the trash (the
   Recycle Bin on Windows), so you can still restore them. The footer shows how many copies
   are marked and how much space that frees.

**Every group always keeps at least one copy.** A rule never marks all of them, and ticking
the last unmarked copy of a group is refused.

## What counts as a duplicate

Everything is compared **by content**. Names, dates and locations never matter.

- **Files:** the same bytes. `IMG_2041.jpg` in `Pictures` and `wallpaper.jpg` in `Downloads`
  are duplicates when their bytes are equal.
- **Folders:** the same files by content, all the way down, however the files and subfolders
  inside are named or arranged. A backup whose photos were renamed or re-sorted into year
  folders still matches the original. A folder with even one file that exists nowhere else
  has no twin.
- **Only the outermost match is shown.** Two copies of `Pictures` show as one folder group,
  not as one group per photo inside, and a folder that holds nothing but one subfolder stands
  in for it. A file that also exists outside those folders still gets its own file group.
- **Hardlinks** to one file are not duplicates, because they share the same space on disk, so
  they are counted once.
- **Symbolic links are never followed.** A link to a folder could otherwise make every file
  in it look duplicated.
- Empty files and, by default, files under 1 KB are ignored.

## How it stays fast

Reading every file on a large disk would take hours, so Coxswain only reads what could match:

1. **Sizes first.** A parallel walk lists every file with its size. A file whose size occurs
   only once cannot have a duplicate, and is never opened.
2. **The first 16 KB.** Files that share a size are compared by a hash of their first 16 KB.
   Different files usually already differ there.
3. **The whole file,** only for the files still matching. Hashing uses
   [BLAKE3](https://github.com/BLAKE3-team/BLAKE3), which uses SIMD instructions (AVX2 and
   AVX-512 on x86, NEON on ARM) and runs on all cores.
4. **Kept.** Full hashes are kept in the search store, `search.db` in Coxswain's cache folder,
   keyed by path, size and modification time. Scanning the same disk again only reads files
   that changed. In the background the search helper hashes the home folder's files that
   share a size, so a scan there reads nothing it already knows, not even the first 16 KB. Hashes of files that are gone or have
   changed are dropped at its next pass.

On a developer's home folder of 64,856 files (30 GB), the first scan read 1.9 GB and took
1.0 s; the second took 0.5 s.

Run the same engine from a terminal to try it on your own disks:

```sh
cargo run --release -p coxswain-core --example dupes -- ~/Pictures /mnt/old-disk
```

## Limits

- **Only exact copies.** A photo that was resized, re-saved or edited is a different file. A
  "similar pictures" mode, comparing what images look like, is planned.
- **Spinning disks** are read by several threads at once, which makes their heads seek. On an
  old hard disk the full-hash phase is therefore slower than the drive's top speed.
- The terminal app does not have the duplicate finder yet; the engine
  (`crates/coxswain-core/src/dupes.rs`) is shared and ready for it.
- The results list shows the 500 groups that waste the most space.

---

← [The preview pane](previews.md) · [Docs index](README.md) · [Cryptography bills of materials](bom.md) →

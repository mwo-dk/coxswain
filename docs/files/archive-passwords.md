[← README](../../README.md) · [Docs index](../README.md) · [Files](README.md)

# Passwords for encrypted zip and 7z

A locked zip (ZipCrypto or AES) or 7z (AES) asks for its password when Coxswain needs to read
what is locked. The password is kept in the app's memory until you close it, so you are asked
once per archive, and it is never written anywhere.

<!-- screenshot: files-archive-password.png: desktop app, Cyber theme: the Locked archive dialog over a pane inside secret.zip (tinted, badge 'archive, locked'), the label 'Its password (kept only for this, never saved):' and a password field showing dots, OK and Cancel -->

## How to use it

1. Do what you want with the archive: open it (**Enter**), copy a file out (**F5**), move it out
   (**F6**), or extract it (**Ctrl+E**).
2. When a locked part is needed, the dialog *Locked archive* opens with *Its password (kept only
   for this, never saved):*.
3. Type the password and press **Enter**. The operation runs again with it.
4. A wrong password asks again, with *That password did not open it. Try again:*. **Esc** gives up.

| | Desktop app | Terminal app |
|---|---|---|
| The field | A password field: dots | Stars, one per character |
| Confirm / cancel | **Enter** or *OK* / **Esc** or *Cancel* | **Enter** / **Esc** (`Enter = OK   Esc = Cancel`) |
| Clear the field | select and type | **Ctrl+U** |

**When you are asked:**

| Archive | Asked when |
|---|---|
| Zip with locked files | You copy, move or extract a locked file. Its names are not locked, so it opens and lists without a password; the badge says *archive, locked* |
| 7z with locked contents | You copy, move or extract a file, or change the archive (add, rename, make a folder, take out): it is unpacked and packed again for that. The badge says *archive, locked* |
| 7z with locked contents and names | Already when you open it with **Enter**: without the password even the list of names cannot be read |

## What you see

- The dialog *Locked archive*, as above. After a wrong password the label changes to *That
  password did not open it. Try again:* (for copy, move, extract, delete and new folder; when
  opening a 7z, the first label is shown again).
- In the desktop app the pane inside a zip with locked files, or a 7z with locked contents,
  shows the badge *archive, locked*.
- While the password dialog is up, the pane says *Locked archive*.
- If you press **Esc** when opening a locked 7z, the pane shows the error
  `locked: a password is needed` and stays empty; open it again to be asked again.
- If you press **Esc** during a copy, nothing is copied from the locked part.

## How long the password is kept, and where

- **In memory only.** The password is held by the running app, for that archive's path. It is
  never written to `config.toml`, the session, the cache, a log or anywhere else on disk.
- **Until the app closes.** Every later copy, move or extract from the same archive uses it
  without asking. Closing the app forgets it. The desktop app and the terminal app each keep
  their own.
- **Kept only when it worked.** A password given for a copy or extract is kept only after it
  opened the archive; one given to open a 7z is replaced when you type another.
- **Per path.** A moved or renamed archive is a new path and asks again.
- It is held as ordinary text in the app's memory, not wiped. A program that can read the app's
  memory as your user (a debugger) could read it; nothing else can.

The dialog's text *kept only for this, never saved* is older than keeping it for the app run;
*never saved* is still exactly true.

## Settings and config.toml

None. There is no setting to store passwords, on purpose.

## In the terminal app

The same dialog, texts and rules. The field shows stars. The password is kept in the terminal
app's memory until you quit it (**F10**).

## Questions

#### Does Coxswain save my archive passwords?

No. A password lives in the running app's memory and is gone when you close it. It is never
written to disk.

#### Why was I not asked again for the second file?

The password that opened the archive is kept for the rest of the app run, for that archive.

#### Why could I see the names in a locked zip without a password?

In a zip only the contents of each file are locked, not the names or sizes. So Coxswain can list
it; the badge *archive, locked* tells you the password will be needed to copy those files.

#### Why does a 7z ask for the password before it even opens?

That 7z was made with its file names locked as well (`7z -mhe=on`). Without the password there is
nothing to list.

#### Some of the marked files were not locked. Are they copied twice?

No. Whatever was not locked is copied in the first try; when the rest failed only for want of a
password, you are asked for it and the operation runs again for just those files. If something
else failed as well (a name that exists, say), the errors are shown instead and nothing runs
again.

#### Can I remove a locked file from a zip without the password?

Yes. Taking out (**F8**) writes the zip anew with the other entries copied as they are, locked or
not; nothing needs to be unlocked. A 7z has to be unpacked and packed again, so it asks for the
password unless it is known from opening it or from a copy out, and it is then written back without one
([Limits](archives.md#limits)).

#### How do I make Coxswain forget a password?

Close the app (the desktop window, or **F10** in the terminal app). There is no other way.

#### What if a wrong ZipCrypto password looks right?

The old ZipCrypto method cannot always tell a wrong password at once; Coxswain notices when the
file's checksum does not match. The half-written file is removed and you are asked again.

---
[← Previous: Pack and extract](pack-and-extract.md) · [Next: Properties and permissions →](properties.md)

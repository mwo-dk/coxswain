[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# The search helper

The index lives in a helper process that every window and terminal app shares: one index in
memory, one scan of the disk, one `search.db`. It starts with the first app and leaves ten minutes
after the last, or it can start with your session so it reads while no window is open.

<!-- screenshot: search-helper-session.png: desktop app, Cyber theme, Settings → Search inside files showing the status line, the ticked box "Start the search helper with my session, so it reads while no window is open", and the buttons Index now and Delete the index -->

## How to use it

Nothing to do: the first app starts it and the others find it. A new window searches at once.

**Start it with my session:**

| Where | How |
|---|---|
| Desktop app | Tick *Settings → Search inside files → Start the search helper with my session, so it reads while no window is open* |
| Terminal app | `coxswain --index-service on`; `off` removes it; `coxswain --index-service` alone prints `on` or `off` |

This registers the helper with the system:

| System | Registration |
|---|---|
| Linux | A systemd user unit, `~/.config/systemd/user/coxswain-index.service`, with `Nice=10` and idle I/O |
| macOS | A LaunchAgent, `~/Library/LaunchAgents/dk.mwo.coxswain.index.plist`, with `Nice` 10 and low-priority I/O |
| Windows | A *Run* entry, `coxswain-index`, under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |

It then starts at login and stays, so the backlog is read before any window is opened. Unticking
it (or `off`) removes the registration and stops the running one; the helper goes back to
starting with the first app and leaving ten minutes after the last.

## What you see

Nothing of its own while it works. Signs of it:

- Find file's count line: ` · building index…`, ` · refreshing index`, ` · 412 still to read`.
- *Settings → Search inside files*: *Searchable: 31,208 files · still to read: 412 · 1.1 GB on disk*
  and where `search.db` is. When it cannot be reached: *The search helper is not running, so text
  cannot be searched now.*, and **Index now** and **Delete the index** are greyed out.
- In a process list: the app itself, started as `coxswain --index-helper` (or `coxswain-gui
  --index-helper`).

## How it works

- **One for all.** The helper is the app itself, started with `--index-helper`. It holds the
  [name index](names.md), reads [text](text.md), makes [vectors](meaning.md), and keeps
  [folder sizes](../panels/folder-sizes.md) and the hashes for [Duplicates](../files/duplicates.md).
- **Private.** The apps talk to it over a local connection (a loopback TCP port). Its port and a
  random token are in a file only you can read (`index.addr` in the cache folder); a connection
  without the token is dropped.
- **Versions.** A helper of another version steps down for the new one after an update.
- **Without it.** If the helper cannot be reached, each app indexes names by itself; text search
  and meaning then wait for the helper.
- **Settings.** A change under *Search inside files* or *Search by meaning* starts a new helper
  with the new settings.

## Settings and config.toml

| Settings item | Where it is kept | Default |
|---|---|---|
| *Start the search helper with my session…* | Not in `config.toml`: the systemd unit, LaunchAgent or *Run* entry itself | Off |

## In the terminal app

The same helper, shared with the desktop app. `coxswain --index-service on|off` does what the
checkbox does. `coxswain --paths` prints where the index, the search store and the model are.

## Questions

#### Is Coxswain running in the background after I close it?
Yes, the helper, for ten minutes, so the next window starts searching at once. Then it exits. With
*Start with my session* it stays for good.

#### How do I stop the helper?
Close every Coxswain window and wait ten minutes. With *Start with my session* on, untick it (or
`coxswain --index-service off`), which also stops the running one. Do not kill it by name: an app
still running would start another.

#### Why start it with my session?
So the first pass over your files, and the vectors for search by meaning, are done while no
window is open. Otherwise reading happens only while an app runs and ten minutes after.

#### Can another user on the machine search my files through it?
No. The port is on loopback only, and a connection must bring the token, which is in a file only
you can read.

#### Two windows are open. Are my files read twice?
No. Both use the one helper: one scan, one store, one index in memory.

#### After an update, the index was built again. Why?
The helper of the old version stepped down for the new one, which loaded the saved index and
refreshed it. The text in `search.db` stays.

#### I changed `[search]` by hand. How do I make the helper take it?
Make any change in Settings, or close every window and wait ten minutes; with the session helper,
`coxswain --index-service off` then `on`.

---
[← Previous: Ask](ask.md) · [Next: Battery →](battery.md)

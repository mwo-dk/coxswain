[← README](../../README.md) · [Docs index](../README.md)

# Tags, notes, favourites and the sidebar

Four ways to keep your bearings in the desktop app: colour tags on files, notes on folders,
named groups of favourite folders, and the sidebar that holds the favourites next to your
places, drives, clouds, the bin and recent git repositories. None of them change your files. They live in
Coxswain's own state file, `state.json` ([Where things are kept](../reference/where-things-are-kept.md)).
The terminal app has none of them but the clouds, which it lists under **Alt+F1** and **Alt+F2**:
the others' keys there say which app has them.

![The desktop app with the sidebar at the left: Places with Home, Drives with System and its free space, Clouds with Dropbox – Personal and Nextcloud, the Bin with 2 items, a favourites group named WORK with rocket, website and Documents, and Git repositories with website pinned, then paper, rocket and vision. In the panels, a grey tag dot after src, a red one after TODO.txt, a yellow one after budget.xlsx and a red one after launch-report.pdf](../screenshots/gui-details.png)
*The sidebar, a favourites group and colour tags in the details view (Cyber theme).*

| Page | What it covers |
|---|---|
| [Colour tags](tags.md) | Seven colours for files and folders with **Alt+T**, the dot after the name, and why a tag stays with a path |
| [Folder notes](notes.md) | Free text for a folder with **Alt+N**, in the preview pane, and the note icon in the pane footer |
| [Favourites](favourites.md) | Named groups of folders in the sidebar: add, go, remove, rename and delete groups |
| [The sidebar](sidebar.md) | **Ctrl+B**: places, drives with their free space, clouds, the bin, favourites and the last eight git repositories, by name, with pins |
| [Clouds](clouds.md) | One entry per cloud account (OneDrive, Google Drive, Dropbox, Nextcloud, iCloud …) found on the machine, in the sidebar and under **Alt+F1** in the terminal app; no logins |

## Keys at a glance

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| **Alt+T** | *Colour tag* dialog | Status line: *Colour tag is available in the desktop app (coxswain-gui)* | Tag the marked files, or the one under the cursor |
| **Alt+N** | Opens the preview pane, cursor in the notes field | *Folder notes is available in the desktop app (coxswain-gui)* | Write a note for a folder |
| **Ctrl+B** | Shows or hides the sidebar | *Sidebar is available in the desktop app (coxswain-gui)* | The sidebar |
| **Esc** | Leaves the notes field, which saves the note | — | |
| Click | On a favourite, place, drive or repository: the active tab goes there | — | |

All three keys can be changed under `[keys]` in `config.toml` (`tag`, `notes`, `toggle_sidebar`);
see [Changing keys](../customise/keys.md).

---
[← Previous: Every default key](../panels/keys.md) · [Next: Colour tags →](tags.md)

# Coxswain GUI v2 — OneCommander-grade

The GUI moves from "NC in a window" to a modern file manager on par with
[OneCommander](https://onecommander.com/). Norton Commander keys stay the default; the NC blue
look becomes a selectable theme. The TUI keeps its NC look.

## Requirements

**Look.** Modern dark theme by default (`gui.theme = "dark"`), plus `light`, `nord` and `nc`.
Every color is a theme slot in the shared config. File-type icons (Nerd Font glyphs with a
color per type, shared with the TUI). File age as a heat color: red within the hour, through
yellow and green, to blue within a year, gray after.

**Layout.**
- Left sidebar: drives with free space, home places (Home, Desktop, Documents, Downloads, …),
  favorite groups (add the current folder, rename, remove), and recent git repositories.
  Toggle with Ctrl+B.
- Two panes (one or two visible). Each pane has tabs (Ctrl+T new, Ctrl+W close, Ctrl+Tab /
  Ctrl+Shift+Tab switch). Tabs, pane sizes and view modes persist across sessions.
- Breadcrumb path bar. Click a segment to go there; Ctrl+L or a click on empty space edits the
  path as text.
- Two view modes per pane: details (name, git, size, modified, age) and Miller columns
  (Alt+V toggles). In columns mode, Left/Right move between columns.
- A preview pane (Space) on the right: images, video and audio, syntax-highlighted text and
  code, rendered Markdown, a hex view for binaries, and a folder summary with notes.
- Resizable splits between sidebar, panes and preview. Status bar with item count, selection
  size and the git prompt.

**Power tools.**
- Folder sizes on demand (Ctrl+Space), computed in parallel.
- Regex batch rename (Ctrl+M) with a live preview and conflict detection.
- Color tags (Alt+T, then 1–7, or 0 to clear), shown as a dot on the row.
- Scripts: F2 lists `[[user_menu]]` entries plus executables in `<config>/coxswain/scripts/`.
  They run on the selection, and their output shows in the preview pane.
- Per-folder notes (Alt+N), kept in Coxswain's state file, never written into the folder.

## Design

- `coxswain-core::state` — a JSON file in the data dir with the session (tabs, splits, modes),
  favorites, tags, notes and recent repos. Load it, change it, then save it atomically.
- `coxswain-core::icons` — maps a file name to a Nerd Font glyph and a color. Used by both UIs.
- `coxswain-core::rename` — `plan(names, pattern, replacement, flags)` returns the new name for
  each file and marks conflicts; `apply` runs the plan.
- `coxswain-core::fs::dir_size` — a parallel recursive size that does not follow symlinks.
- New `Action`s, with the same keys in both UIs (the TUI shows "GUI only" where needed):
  `new_tab`, `close_tab`, `next_tab`, `prev_tab`, `toggle_preview`, `toggle_view`,
  `toggle_sidebar`, `edit_path`, `dir_sizes`, `batch_rename`, `tag`, `notes`.
- Tauri: the asset protocol serves previews of images and video. `sysinfo` lists the disks.
- Svelte: `App` (shell, keys, dialogs), `Sidebar`, `Pane` (tabs + breadcrumb + view),
  `DetailsView`, `ColumnsView`, `Preview`, `Splitter`, `lib.js`. Uses Svelte 5 runes
  throughout; `highlight.js` for code and `marked` for Markdown.

## Steps

1. Core: new actions, `state`, `icons`, `rename`, `dir_size`, and GUI themes.
   Acceptance: `cargo test -p coxswain-core`.
2. Tauri commands: disks, places, state, sizes, rename, scripts, and the asset protocol.
   Acceptance: `cargo build -p coxswain-gui`.
3. Svelte shell: themes, sidebar, panes with tabs, breadcrumbs, splitters, status bar, and
   session persistence. Acceptance: `npm run build && npx svelte-check`, plus screenshots.
4. Details view polish (icons, age heat, tags, folder sizes) and the Miller columns view.
5. Preview pane.
6. Power-tool dialogs: batch rename, tags, notes, scripts.
7. Review, then screenshots of every theme.

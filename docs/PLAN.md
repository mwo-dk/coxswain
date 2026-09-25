# Coxswain plan

Coxswain is a two-panel file manager in the Norton Commander tradition: a Ratatui TUI and a
Tauri + Svelte 5 GUI, both on one shared Rust core.

## Decisions

- **Workspace:** `crates/coxswain-core` (everything that is not UI), `crates/coxswain-tui` (binary
  `coxswain`), `gui/` (Svelte 5 + Vite) with `gui/src-tauri` (binary `coxswain-gui`).
- **Git:** shell out to `git status --porcelain=v2 --branch -z`. It respects every git
  config, hook and attribute, needs no C dependency and is fast. Glyphs are Nerd Font
  (oh-my-posh style) by default with an ASCII set in config.
- **Search:** Everything's speed comes from keeping every file name in RAM in a compact
  layout and scanning it in parallel, not from a database. Core keeps one `Vec<u8>` of
  names plus a parent-index array; a query is a parallel `memmem` scan across all cores,
  full paths are built only for hits. The index is persisted to the cache dir and loads in
  well under a second; live changes come from the `notify` crate.
  - Linux: parallel walk to build, inotify via `notify` to update.
    Upgrade path: fanotify (`FAN_REPORT_DFID_NAME`) for whole-filesystem watches without
    the inotify watch limit.
  - macOS: same walk, FSEvents via `notify`.
  - Windows: same walk. Upgrade path: MFT enumeration (`FSCTL_ENUM_USN_DATA`) and the USN
    journal, which is what Everything itself does.
  - Query syntax (Everything subset): space = AND, `!term` = NOT, `*`/`?` wildcards,
    `ext:rs;toml`, a term containing `/` matches the full path, `case:` for case-sensitive.
- **Config:** one TOML file at `<config dir>/coxswain/config.toml`, shared by TUI and GUI.
  `[keys]` maps actions to key strings (`"F5"`, `"Ctrl+R"`, `"Alt+F7"`), defaults are
  Norton Commander's. `theme = "nc"` picks a built-in scheme, `[themes.<name>]` defines or
  overrides one. `coxswain --dump-config` prints the full default file.

## Steps

Each step ends with its acceptance command green.

1. Workspace + core `config` (key parsing, actions, themes, defaults) —
   `cargo test -p coxswain-core config`.
2. Core `fs` (list, sort, copy/move/delete/mkdir) — `cargo test -p coxswain-core fs`.
3. Core `git` (porcelain v2 parser, glyphs) — `cargo test -p coxswain-core git`.
4. Core `index` (build, save/load, query, watch) — `cargo test -p coxswain-core index`, then
   `cargo run --release -p coxswain-core --example bench -- /` for real timings.
5. TUI: panels, function-key bar, command line, dialogs, viewer/editor via `$PAGER`/`$EDITOR`,
   search dialog (Alt+F7) — `cargo build -p coxswain-tui` and a manual run.
6. GUI: Tauri commands over core, Svelte 5 dual panel with the same key map and theme —
   `npm run build` in `gui/` and `cargo build -p coxswain-gui`.

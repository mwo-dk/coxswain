# Working on Coxswain

These rules hold for every contributor and every Claude session in this repository.

## Shipping

- **Never a partial release: complete on every system and every channel, or not at all.** Every
  installer and archive, each with its `.sha256`, one `SHA256SUMS` over all of them, and every
  channel (crates.io, Homebrew, WinGet, the FreeBSD port), or the release does not go out. No
  `continue-on-error` on anything that makes a release file.
- **Releases are automatic, in one run.** The version lives only in `Cargo.toml`
  (`[workspace.package]`). Every push to `master` starts one *Release* run (`release.yml`): it
  runs CI and every system's workflow (FreeBSD, NetBSD, OpenBSD, illumos, Termux, Flatpak, Nix)
  as parts of that run; when all pass and the version has no GitHub release yet, it makes a draft,
  builds every platform, checks the draft against the full list of files
  (`packaging/release-assets.sh`; a gap deletes the draft), publishes (creating the `vX.Y.Z`
  tag) and then updates each channel, retrying passing faults; a channel that still fails turns
  the run red, named. A push to `master` without a new version only runs the tests. When the run
  ends, `release-check.yml` installs the release and checks every channel. A new build target
  goes into the matrix and into `packaging/release-assets.sh` together.
- **Pull requests run the virtual machines only for platform code** (files with BSD or illumos
  branches, `Cargo.toml`, `Cargo.lock`, `install/`, that system's `packaging/`), and Termux,
  Flatpak and Nix only when what goes into them changes; every push to `master` runs all of them,
  in its Release run. A new file with `cfg(target_os = …)` for those systems goes into
  their workflows' `paths`. Branches need not be up to date with `master`: master's own runs
  catch clashes.
- **Bump the version in the same commit as a user-visible change:** patch for fixes, minor for
  features, major for breaking config or key changes. Then run `cargo check` so `Cargo.lock`
  follows. Do not bump for docs, CI or refactors. Two open PRs that both bump: the one merged
  second takes the next number when it is rebased.
- **All changes go through pull requests.** `master` is protected: no direct pushes, CI must
  pass on Linux, macOS and Windows, squash merge only. Work on a branch, open the PR with
  `gh pr create`, then `gh pr merge --auto --squash` so it merges itself once CI is green.
  The squash commit title is what lands on `master`, so make the PR title a good commit title.
- **Watch every PR you open until it has merged.** Auto-merge hides failures: follow the checks
  (`gh pr checks <n> --watch`), and when one fails read `gh run view <id> --log-failed`, fix,
  push, and watch again. Windows is the usual one (path separators, `;` in path lists, files
  still open). A PR is done when it is merged, not when it is opened.
- **Commits:** plain messages. No `Co-Authored-By` or other Claude attribution.
- **Work in a git worktree** (`git worktree add -b <branch> .worktrees/<topic> origin/master`, inside the repository and
  ignored by git, so the folder above stays clean)
  when other work is under way, never by switching branches under uncommitted changes. Remove
  the worktree and its branch once its PR has merged.

## Building and checking

- **Local GUI builds:** `cd gui && npm run build`, then `cargo build --release -p coxswain-gui`
  embeds `gui/dist` (default feature `custom-protocol`). `npx tauri dev -- --no-default-features`
  for the live dev server; without that flag dev loads `dist` too.
- **Check before pushing:** `cargo test --workspace`, and for GUI changes
  `cd gui && npx svelte-check && npm test && npm run build`. For preview changes, build the
  release and run `uv run tools/preview-check.py` (Linux, needs `broadwayd`): it opens a file of
  each kind in the real app, on a display of its own, and fails when one shows nothing.
- **Bills of materials:** a dependency that does cryptography needs its use described in
  `tools/bom/crypto.toml`, and an npm licence outside `deny.toml`'s list needs reading first;
  the *bills of materials* check fails until then (`tools/bom/README.md`).
- **The terminal app ships as a static musl binary.** A dependency that compiles C must build
  for `x86_64-unknown-linux-musl` too; CI checks it. Prefer pure-Rust crates.

- **Dependabot PRs are not merged as they are**: their commits would make dependabot[bot] a
  contributor. Apply the update in a PR of our own (with the checks below), then close the
  Dependabot PR with a comment naming ours.
- **No legacy.** Dependencies stay current: Rust crates, npm packages, GitHub Actions and bundled
  viewers are upgraded (majors too, when the migration is contained) and every known advisory is
  fixed; each source or security review includes this. Old code paths, compatibility shims,
  deprecated APIs and dead options are removed, not carried along.

## Every feature

- **Both apps.** A feature goes to the terminal app and the desktop app; its logic lives in
  `coxswain-core`. Desktop-only needs a reason (a picture, a web view) and a word in the docs.
- **Keys:** never take over an existing default key for a new action; pick a free one.
- **Texts in every language.** User-facing strings go through `t!` / `t()` with keys in
  `crates/coxswain-core/locales/*.json`, translated into all of them (`en-AU`, `en-CA`, `en-NZ`,
  `de-AT` and `de-CH` only differ where they must; `de-CH` gets `de` with ss for ß by itself);
  the i18n test fails on a missing key.
- **Nothing hidden.** Every capability can be found without reading the docs, in both apps:
  - it is reachable from something visible: the action menu (**Shift+F10**,
    `coxswain_core::menu`), the F9 command list, a button or a menu, not only by a key;
  - its key is shown where it applies (a key line, a tooltip, the menu entry) and in F1;
  - nothing exists only as a gesture, a repeated key, a modifier+click, a typed syntax or a
    `config.toml` setting: each of those also has a visible way, and Settings names the config
    keys it does not cover, with a button that opens `config.toml` there;
  - what a user would not guess gets a hint (`coxswain_core` hints: shown a few times, then
    never again);
  - when it is off or needs something installed, a notice (`coxswain_core::notices`) says so
    once.
  A PR that adds or changes a capability checks these points; a review that finds a hidden
  feature fixes it.
- **Nothing leaves the machine unless the user asks** for it (the update check aside, and it can
  be turned off). Anything that sends data (a model download, a remote server) is opt-in, and
  Settings names where it goes.

## Documentation

The README is an **overview**: what Coxswain is, the highlights, install, first steps, a map of the
documentation, and the changelog at the bottom. **Every feature, all of them, is walked through in detail under `docs/`.** When
you add or change a feature, the docs change in the same PR.

- **The docs are a tree:** a folder per area (`docs/panels/`, `docs/search/`, `docs/previews/`,
  `docs/files/`, …), each with a `README.md` index of its pages, and **one page per feature**
  (`docs/search/meaning.md`, `docs/files/archives.md`). A new feature gets its own page in its
  area, a line in the area index and in `docs/README.md` (the full map); a new area gets a line
  in the README's documentation map.
- **Pages:** each page starts with
  `[← README](../../README.md) · [Docs index](../README.md) · [<Area>](README.md)`, has a table
  of contents when it is long, and ends with previous / next links in `docs/README.md`'s order.
  Screenshots are linked as `../screenshots/<name>.png`. Check every relative link and anchor
  before pushing.
- **Changelog:** every PR that bumps the version adds its row to the changelog table at the
  bottom of `README.md` (version, date, what a user gets in plain words, link to the docs
  page), newest first.
- **Each feature section** says what it does, how to reach it (keys, menu, Settings, command
  line), what it needs, how the terminal app and the desktop app differ, and its `config.toml`
  keys.
- **Questions:** under every feature, a "Questions" part with the whys and hows users really
  ask (three to eight, more where people get stuck), answered from the code. `docs/faq.md`
  collects the common ones and links to the full answers.
- **Keys and what you see:** every how, why and answer names the exact keys (in both apps, and
  where they differ) and what happens on screen: which pane changes, the badge or tint, the
  status line text, the dialog that opens, how a hit or a state is marked.
- **Pictures** from `docs/screenshots/` sit next to the text they show. A missing one is marked
  `<!-- screenshot: name.png: what it must show -->` and taken in the sandbox (below).
- **Systems in this order** wherever several are listed (install tables, docs, release notes, hints): FreeBSD first, then the other BSDs and illumos, then Linux (and Android/ChromeOS), then "Any" (cargo, from a clone), and macOS and Windows last. The owner's choice, on principle.
- Plain British English (README and docs: British or Canadian spelling where they differ from US: colour, licence as a noun, catalogue, grey, favourite, centre), short concrete sentences, tables for keys and options, no marketing.

## Screenshots

Screenshots in `docs/screenshots/` are taken in a sandbox with a fake home, so nothing personal
shows (Linux, needs `bwrap`): build release, then
`docs/screenshots/sandbox.sh coxswain-gui /home/demo/projects/rocket` or
`docs/screenshots/sandbox.sh alacritty -e coxswain`. `demo-home.sh` builds the demo files.
`coxswain-gui --settings=<section>` opens Settings at a section. `SANDBOX_ARGS` adds bwrap
arguments (a fake `/etc/hosts` so no server answers, a fake `/sys/class/power_supply`, a disk
under `/run/media`).

**Taking them without the desktop:** `docs/screenshots/cap.py` runs both apps in that sandbox on
displays of their own, so nothing shows on screen and no key can reach another window. The
desktop app runs on GTK's Broadway inside the sandbox (its pictures go through the sandbox's
`/dev/shm`), shown by a headless Chromium; keys and clicks go to that page over the DevTools
protocol, and pictures and GIF frames come from it. Broadway has no screen resolution, which
WebKitGTK turns into a negative zoom, so the app gets a small preload that says 96 dpi. The
terminal app runs in a tmux of its own, drawn in MesloLGM Nerd Font Mono. Each run gets its own
process namespace: stopping it stops all it started, the search helper too. See the top of
`cap.py` for the calls.

- Never run podman or docker inside the sandbox.
- Keys may be sent (`wtype`) only to the sandbox window, and only after checking that it has
  the focus. Never type into other windows, and never stop Coxswain processes by name: stop the
  sandbox's own processes.
- Cyber for the desktop app, Classic blue (NC) for the terminal app.

**Pictures stay current.** A PR that changes what a screen shows retakes the pictures and GIFs
of that screen in the same PR, or marks each one it cannot take with the placeholder above and
says so in the PR. Before a minor or major release is merged, check
`grep -rn "<!-- screenshot:" README.md docs` and take what can be taken.

**GIFs** show what moves: Find filling its groups, Ask writing its answer, marking and copying,
an archive opened as a folder, git branches. Record them in the same sandbox with `cap.py`
(`record()` takes the frames while the keys are sent, `gif()` makes the GIF with `ffmpeg`'s
`palettegen`/`paletteuse` and its first-frame still).

- At most 8 seconds, 800 px wide, 10–12 frames a second, under 1.5 MB, looping; start and end
  on the same calm frame.
- Every GIF has a still of its first frame next to it (`<name>.png`) and is shown as
  `<picture><source media="(prefers-reduced-motion: reduce)" srcset="../screenshots/<name>.png"><img src="../screenshots/<name>.gif" alt="…"></picture>`,
  with an alt text that says what happens.
- The README carries two at most (Find and Ask); the rest go on their feature's docs page.

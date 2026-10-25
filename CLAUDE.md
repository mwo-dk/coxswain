# Working on Coxswain

These rules hold for every contributor and every Claude session in this repository.

## Shipping

- **Never a partial release: complete on every system and every channel, or not at all.** Every
  installer and archive, each with its `.sha256` (the bills of materials and
  `THIRD-PARTY-NOTICES.md` have none: `SHA256SUMS` covers them), one `SHA256SUMS` over all of
  them, and every channel (crates.io, Homebrew, the FreeBSD port; WinGet within its week), or the
  release does not go out. No `continue-on-error` on anything that makes a release file.
- **Releases are automatic, in one run.** The version lives only in `Cargo.toml`
  (`[workspace.package]`). Every push to `master` starts one *Release* run (`release.yml`). It
  first decides whether the version has a GitHub release yet. If not, and this push is the one
  that bumped it (later pushes test only), it runs CI and every
  system's workflow (FreeBSD, NetBSD, OpenBSD, illumos, Termux, Flatpak, Nix) as parts of that
  run; when all pass it makes a draft, builds every platform, checks the draft against the full
  list of files (`packaging/release-assets.sh`; a gap deletes the draft), publishes (creating the
  `vX.Y.Z` tag) and then updates each channel, retrying passing faults; a channel that still
  fails turns the run red, named. A push without a new version runs only what its pull request
  ran: CI, and each system whose pull-request `paths` the push touches
  (`.github/changed-systems.py`). WinGet is not in that run: `winget.yml` files the newest
  release there on Mondays (one pull request per package, our older open ones closed as
  superseded; a package not yet listed has its New-Package pull request moved to the new version
  in place, keeping its place in the queue), because its moderators review each pull request by hand. When the run ends,
  `release-check.yml` installs the release and checks every channel, WinGet within that week (no
  release left more than 8 days without a pull request). A new build target goes into the matrix
  and into `packaging/release-assets.sh` together.
- **A failed release run: fix, then start Release by hand.** When the bump's release run fails on
  a fault of ours, fix it in a PR, then run `gh workflow run release.yml` on `master` (a rerun
  would build the old commit, and the fix's own push only tests). Until that version is
  published, every Release run of a later push and the nightly run go red naming it
  (`.github/unreleased.sh`), and `packaging/release-pr.py` refuses to bump past it (exit 3;
  `--unreleased-ok` skips it, the owner's decision only), so the Sunday release pull request
  waits for it.
- **Nightly, every system** (`nightly.yml`, 02:17 UTC): when `master` changed since the last
  nightly run that passed (and its head is not a release, which tested everything), every
  system's workflow runs on it, so a fault no pull request caught shows up the next morning. Fix
  a red nightly like a red master.
- **Pull requests run the virtual machines only for their own packaging**: a system's workflow
  (FreeBSD, NetBSD, OpenBSD, illumos, Termux, Flatpak, Nix) runs on a pull request only when that
  system's `packaging/` or the workflow itself changes (Nix also `flake.nix`, `flake.lock`). All
  other code, platform code included, is tested on every system by the nightly run and in every
  Release run, the Saturday rehearsal among them; a red nightly is fixed like a red master. The
  *Dependencies* workflow runs on a pull request only when a `Cargo.toml`, `Cargo.lock`,
  `deny.toml`, `gui/package*.json` or `tools/bom/` changes. A release pull
  request (branch `release/X.Y.Z`) starts none of them: its Release run tests every system. CI on
  a pull request runs Linux, macOS and Windows on x64 (macOS on Apple Silicon), only Linux when it changes texts alone (docs, README, `changelog.d/`,
  locales); the ARM machines
  (`ubuntu-24.04-arm`, `windows-11-arm`) run only on pushes to `master` and in Release runs. The
  only required check is *CI result*. Branches need not be up to date with `master`: master's
  own runs catch clashes.
- **Feature and fix PRs do not bump the version.** A user-visible change adds its changelog row as a
  file of its own, `changelog.d/<branch>.md` (below), and nothing else about the version, so PRs
  do not clash on `Cargo.toml` or the changelog and can run and merge in any order. A feature's
  row ends with ` <!-- feature -->` after its last `|`; a fix's has nothing there.
- **A release is a pull request of its own,** small and holding nothing else, on the branch
  `release/X.Y.Z`, titled `Release X.Y.Z`. `packaging/release-pr.py` makes it: it bumps the
  version (minor when any waiting row is marked a feature, patch otherwise; a major release, for
  breaking config or key changes, is bumped by hand) in `[workspace.package]` and in the
  `coxswain-core` line under `[workspace.dependencies]` (`=X.Y.Z`, the same), updates
  `Cargo.lock` with it, moves every `changelog.d/` row into one row for the new version in
  `docs/changelog.md` (version, date, the rows' texts and docs links joined, features first) and
  adds that row to the table at the bottom of `README.md` (dropping its oldest, so it keeps ten),
  and deletes the `changelog.d/` files. The row can be polished in the pull request. Its merge
  starts the release.
- **Releases are weekly, on Sunday, by themselves.** Saturday 06:17 UTC, when `changelog.d/` has
  rows, the Release run rehearses tomorrow's release: it makes the release pull request's changes
  on a branch `rehearsal/vX.Y.Z`, tests every system, builds every file into a draft, checks it
  against `packaging/release-assets.sh`, runs `cargo publish --dry-run` for both crates and
  `brew audit` on the formula and cask, then deletes the draft and the branch; nothing is
  published and no tag made. A red rehearsal gives a day to fix. Sunday 06:17 UTC `release-pr.yml`
  opens the release pull request with auto-merge on; it needs the secret `RELEASE_PR_TOKEN` (a
  fine-grained token for this repository with Contents and Pull requests read and write: a pull
  request made with `GITHUB_TOKEN` starts no checks) and fails naming it when missing. The Monday
  `winget.yml` run then takes the release at once. Between Sundays only a fix for a security
  hole, lost data or a crash gets a release of its own (a patch release PR, by hand, with just
  that fix's rows, made with `packaging/release-pr.py`).
- **All changes go through pull requests.** `master` is protected: no direct pushes, CI must
  pass on Linux, macOS and Windows, squash merge only. Work on a branch, open the PR with
  `gh pr create`, then `gh pr merge --auto --squash` so it merges itself once CI is green.
  The squash commit title is what lands on `master`, so make the PR title a good commit title.
- **Watch every PR you open until it has merged.** Auto-merge hides failures: follow the checks
  (`gh pr checks <n> --watch`), and when one fails read `gh run view <id> --log-failed`, fix,
  push, and watch again. Windows is the usual one (path separators, `;` in path lists, files
  still open). A PR is done when it is merged, not when it is opened.
- **Watch master, every Release run, the nightly run and the Saturday rehearsal to the end, and
  fix what fails without being asked.**
  A failure that is GitHub's or a mirror's (a cancelled job, HTTP 5xx, a hung download) is rerun
  (`gh run rerun <id> --failed`); one of ours is fixed in a PR. A workflow-only PR whose checks
  are not required (the virtual machines, Termux, Flatpak) merges only after those checks have
  passed on it, not on auto-merge alone.
- **A green job is not proof that a channel published.** After a release, check each channel
  itself: the GitHub release and its files with their `.sha256`, the version on crates.io, the
  Homebrew formula and cask, the FreeBSD port's artifact, and after the next weekly `winget.yml`
  run the WinGet pull request. Say a release is out only after that (WinGet may follow later).
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
- **No clock in a test's proof.** The BSD and illumos VMs are 5-10 times slower than Linux. A
  test proves "quick" by counting the work, by the state the code is in (still looking after a
  turn of the UI loop), or against a plain input timed on the same machine
  (`coxswain_core::assert_costs_at_most`, also n against 8n for linear work). A time limit only
  guards against a hang, and every one, waits for a thread too, goes through
  `coxswain_core::test_limit`.
- **Test platform code in a VM before CI:** `tools/vms/rebuild/<vm>.sh` makes a FreeBSD,
  OpenBSD, NetBSD or OmniOS virtual machine (unattended), and `tools/vms/test-in.sh <system>`
  runs a checkout's tests in it (`tools/vms/README.md`). Use one before pushing a change to BSD
  or illumos code, since pull requests run those systems only for platform files.
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
  the i18n test fails on a missing key. A new language is marked new in `i18n::LANGUAGES`
  (`Some("YYYY-MM-DD")`): the Sunday of the fourth weekly release after the one that brings it,
  when the mark goes by itself.
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
- **Every feature in F1 → Features.** A PR that adds or changes a feature adds or updates its
  entry in `coxswain_core::features::FEATURES` (id, docs page, the actions that are its keys)
  and its texts in every language: `feature.<id>.title`, `.what` (one line), and its two or three
  most asked questions with answers of a sentence or two (`.q1`/`.a1` …), taken from its docs
  page's *Questions*; keys in them as `{action}` placeholders, so they follow the keymap. A test
  fails when a docs feature page has no entry (reference pages that are no feature are listed
  in the test).
- **Work a user's machine must redo is a decision, not a side effect.** A change that makes
  existing stores redo work (vectors made again, files read again, an index rebuilt) is named in
  the PR title and in its `changelog.d/` row, and the release carries it into the version's row,
  saying what is redone and for which files. A passage
  change that concerns only some readers bumps that reader's version (`readers` in the store),
  not `meaning::SCHEME`, so only those files are redone; and an older version never throws
  away what a newer one made.
- **Nothing leaves the machine unless the user asks** for it (the update check aside, and it can
  be turned off). Anything that sends data (a model download, a remote server) is opt-in, and
  Settings names where it goes.

## Documentation

The README is an **overview**: what Coxswain is, the highlights, install, first steps, a map of the
documentation, and the changelog at the bottom. **Every feature, all of them, is walked through in detail under `docs/`.** When
you add or change a feature, the docs change in the same PR.

- **The docs are always current, and CI holds them to it.** The *docs* check
  (`tools/docs-check.py` and `crates/coxswain-core/tests/docs.rs`) runs on every pull request and
  blocks merge: links and anchors resolve, each `changelog.d/` file is one row whose links
  resolve, the changelog has the version's row, the README its last ten version rows, a release
  leaves `changelog.d/` empty, no old version stands as an example (write `coxswain X.Y.Z`; picture alt texts
  too), every question under *Questions* is a `####` heading (never a bold paragraph), every page has its
  *Previous* / *Next* links in `docs/README.md`'s order and a long one its contents, every theme,
  action, `config.toml` key, flag, `--settings=` name and feature is in `docs/` (every action in
  F1 → Features too, and `coxswain --help` word for word), F1's texts (`feature.*`, `help.*` in
  every locale) name an action's key only as `{action}`, never a default key written out, and a minor or major release has no
  screenshot placeholders left (except the few in `NEEDS_HARDWARE` in `tools/docs-check.py`, each
  naming the machine it needs). Docs-only pull requests run them too. Run
  `python3 tools/docs-check.py` and `cargo test -p coxswain-core --test docs` before pushing.
- **A change fixes every mention of the old way.** A change to a key, a way to reach something, a
  default, a behaviour or what a screen shows fixes every mention of the old one in `docs/`,
  `README.md`, the F1 texts in every locale and the pictures of that screen, in the same PR, not
  only the feature's own page. Before a feature PR is opened, a docs-sync review is run: a
  reviewer given the diff lists what changed and greps for the old wording, and every hit is
  fixed.
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
- **Changelog:** every PR with a user-visible change adds one file, `changelog.d/<branch>.md`,
  holding one line: `| **What a user gets.** In plain words. | [Docs page](page.md#anchor) |`,
  links written as from `docs/`, and ` <!-- feature -->` after it for a feature. One file per PR, because rows in one shared table clash whenever
  two PRs merge. `docs/changelog.md` and `README.md` are not touched: they show released versions
  only (the README the newest ten). The release PR (above) turns the `changelog.d/` rows into the
  version's row (version, date, what's new) in both tables and deletes the files.
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

## Testing the apps on a machine someone uses

- **A fake home, always.** Runs of the apps for testing (built-in models, the setup guide,
  Settings, Ask, screenshots) use a fake `HOME` and `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`,
  `XDG_DATA_HOME`, never the user's own `config.toml`, `state.json` or cache: one test once
  downloaded 12 GB of models into the owner's cache and left his Ask set to one of them. A local
  model server (Ollama) may be asked questions; never pull, delete or change its models.
- **Nothing left behind.** Anything built or downloaded for a task outside the repository (a
  virtual machine, a ports tree, a model, a toolchain) goes in the session's scratch folder or a
  `.worktrees/` folder and is removed when the task is done; never in `~/.cache` or the folder
  above the repository. Say what is left, where and how large, when it must stay.

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
- **Stop only what you started, by its own PID.** Save `$!` (or use `timeout`) and kill that
  PID. Never `kill -- -PGID` or `kill 0`, never `pkill`/`killall`/`pgrep … | kill` by name or
  pattern, and never `rm -rf` or `find … -exec rm` over a folder others share: a group or pattern
  kill once reached the owner's compositor and ended the whole desktop session.
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

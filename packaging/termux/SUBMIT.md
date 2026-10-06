# Submitting Coxswain to termux-packages

The recipe is `coxswain/build.sh` in this folder. CI (`.github/workflows/termux.yml`) builds it
from each commit with termux-packages' own builder for `aarch64`, then installs the `.deb` in
Termux's Docker image on an ARM runner and starts it. The pull request to Termux is opened by
hand, from the owner's account; nothing here opens it.

## AI-written work: what to know first

The recipe, this file and the CI job were drafted with an AI assistant (Claude). As of
2026-10-06, termux-packages has no written rule on AI-generated contributions: none in
CONTRIBUTING.md, in `.github/` (it has no pull request template) or on the wiki's front page.
Their contributing guide does say that contributors take all responsibility for what they
submit, and that low-quality work is closed. So:

- **Read and understand every line of `build.sh` before you submit it**, and build it yourself
  (step 3). You must be able to answer review questions about it in your own words.
- **Write the pull request text yourself.** Below are the facts it should carry, not a text to
  paste.
- **Say that an AI assistant helped**, in one sentence in the pull request (for example that
  the recipe was drafted with one and that you reviewed and tested it). If their rules have
  changed by the time you submit, follow them; check CONTRIBUTING.md again.

## Before you start

- The recipe must name a **released** version. `TERMUX_PKG_VERSION` and `TERMUX_PKG_SHA256`
  are those of the latest release; when a newer one is out, update both:

  ```sh
  V=2.1.0   # the release, without the v
  curl -sL https://github.com/mwo-dk/coxswain/archive/refs/tags/v$V.tar.gz | sha256sum
  ```

- The last *Termux* run on `master` is green (Actions → Termux).

## Steps

1. **Fork** <https://github.com/termux/termux-packages> on GitHub (one click, *Fork*). Leave
   *Allow edits by maintainers* on when you open the pull request: they often push small fixes
   themselves.
2. **Clone the fork and add the recipe:**

   ```sh
   git clone https://github.com/<you>/termux-packages
   cd termux-packages
   git checkout -b coxswain
   cp -r /path/to/coxswain/packaging/termux/coxswain packages/coxswain
   ```

3. **Build it once yourself** (Docker on a Linux machine, about 15 minutes):

   ```sh
   ./scripts/run-docker.sh ./build-package.sh -I -a aarch64 coxswain
   ./scripts/run-docker.sh ./build-package.sh -I -a arm coxswain
   ```

   The packages land in `output/`. If you have an Android phone, copy one over and install it in
   Termux with `apt install ./coxswain_<version>_aarch64.deb`, then run `coxswain`.
4. **Commit** with their message format (CONTRIBUTING.md, *Commit messages*):

   ```sh
   git add packages/coxswain
   git commit -m "addpkg(main/coxswain): two-panel file manager for the terminal, Norton Commander style"
   git push -u origin coxswain
   ```

5. **Open the pull request** from your fork's `coxswain` branch to `termux/termux-packages`
   `master`, with a title and text you write from the facts below.
6. **Answer the review.** Their CI builds the package for all four architectures (`aarch64`,
   `arm`, `i686`, `x86_64`). What reviewers usually ask, and the answers:
   - *Why not just `cargo install`?* Their policy prefers language package managers for
     libraries and modules; Coxswain is an end-user program, like `yazi`, `ripgrep`, `eza` and
     `broot`, which they package. It installs a manual page and the `cox` link, which cargo does
     not.
   - *Is it well known enough?* Say what it is used for and link the README. If they decline
     the main repository, the same `build.sh` goes to the Termux User Repository instead:
     fork <https://github.com/termux-user-repository/tur>, put it in `tur/coxswain/build.sh`
     (set `TERMUX_PKG_MAINTAINER="@<your GitHub name>"`), and open the pull request there; users then run `pkg install tur-repo && pkg install coxswain`.
   - *Maintainer:* new packages in the main repository use `@termux`; keep it unless they ask.
   - *The trash patch:* the `trash` crate leaves Android out. The recipe copies it into
     `vendor/trash`, lets Android take its freedesktop.org code and adds a `get_mount_points`
     for Android: the same sed and the same patch file as `packages/yazi`, so F8 moves files to
     `~/.local/share/Trash` as on Linux.
   - *Bundled C:* the build compiles SQLite and Oniguruma from their crates (`rusqlite`
     `bundled`, `onig`). If they want the system libraries, add `TERMUX_PKG_DEPENDS="oniguruma"`
     and `export RUSTONIG_SYSTEM_LIBONIG=1` in `termux_step_pre_configure`, build again, push.
7. When it is merged, the package reaches users within a day: `pkg install coxswain`.
   Change the README's install row and `docs/reference/termux.md` from *once it is in Termux's
   repository* to the plain command.

## The pull request

**Title** (their commit format): `addpkg(main/coxswain): <a short description>`, for example
`addpkg(main/coxswain): two-panel file manager for the terminal`.

**What the text should say**, in your words:

- What Coxswain is and who it is for: a two-panel, Norton Commander style file manager for the
  terminal, in Rust, MIT licensed; the homepage https://github.com/mwo-dk/coxswain.
- That only the terminal app is built (`cargo build -p coxswain`); the repository's desktop app
  is not packaged.
- What it installs: `bin/coxswain`, the `cox` link, the manual page `coxswain(1)`.
- That the `trash` crate is patched for Android the way `packages/yazi` does (same sed, same
  patch file).
- Its dependencies: none at run time; it recommends `git` (git column) and `termux-api`.
- That `TERMUX_PKG_AUTO_UPDATE=true` follows the GitHub releases, tagged `vX.Y.Z`.
- How you tested it: your own build (step 3), on a phone if you have one, and the project's CI,
  which builds and starts it in termux-docker on every change:
  https://github.com/mwo-dk/coxswain/actions/workflows/termux.yml
- That you are the upstream author and will look after build failures with new releases.
- The one sentence about the AI assistant (above).

## After it is in

Termux's auto-update bot reads `TERMUX_PKG_AUTO_UPDATE=true`, looks at the latest GitHub
release of `TERMUX_PKG_SRCURL`'s repository every few hours, and commits a new
`TERMUX_PKG_VERSION` and `TERMUX_PKG_SHA256` to termux-packages itself, which builds and
publishes it. Nothing is needed from us for a release. When an update fails to build there,
the bot reports it in an issue in termux/termux-packages naming the package: fix it here (CI's Termux
job shows the same failure first), release, and the next bot run picks it up; a change to the
recipe itself goes in as an `fix(main/coxswain): …` pull request to termux-packages.

Keep `packaging/termux/coxswain/build.sh` here the same as the one in termux-packages, apart
from the version, so the CI tests what Termux builds.

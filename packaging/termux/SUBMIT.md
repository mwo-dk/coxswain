# Submitting Coxswain to termux-packages

The recipe is `coxswain/build.sh` in this folder. CI (`.github/workflows/termux.yml`) builds it
from each commit with termux-packages' own builder for `aarch64`, then installs the `.deb` in
Termux's Docker image on an ARM runner and starts it. The pull request to Termux is opened by
hand, from the owner's account; nothing here opens it.

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
   `master`, with the title and text below.
6. **Answer the review.** Their CI builds the package for all four architectures (`aarch64`,
   `arm`, `i686`, `x86_64`). What reviewers usually ask, and the answers:
   - *Why not just `cargo install`?* Their policy prefers language package managers for
     libraries and modules; Coxswain is an end-user program, like `yazi`, `ripgrep`, `eza` and
     `broot`, which they package. It installs a manual page and the `cox` link, which cargo does
     not.
   - *Is it well known enough?* Say what it is used for and link the README. If they decline
     the main repository, the same `build.sh` goes to the Termux User Repository instead:
     fork <https://github.com/termux-user-repository/tur>, put it in `tur/coxswain/build.sh`
     (set `TERMUX_PKG_MAINTAINER="@<your GitHub name>"`), and open the pull request there with
     the same text; users then run `pkg install tur-repo && pkg install coxswain`.
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

**Title:** `addpkg(main/coxswain): two-panel file manager for the terminal, Norton Commander style`

**Text:**

> Coxswain is a two-panel file manager for the terminal in the tradition of Norton Commander
> and Midnight Commander, written in Rust: two panels, F-key actions (copy, move, delete,
> rename, new folder), a git column, branches and history, archives opened as folders, file
> search by name and inside files, and a command line. MIT licensed.
>
> - Homepage and source: https://github.com/mwo-dk/coxswain
> - Builds only the terminal app (`cargo build -p coxswain`); the repository also has a desktop
>   app, which is not packaged here.
> - Installs `bin/coxswain`, a `cox` link and the manual page `coxswain(1)`.
> - Patches the `trash` crate for Android the way `packages/yazi` does (same sed, same patch).
> - No runtime dependencies; recommends `git` (git column) and `termux-api` (clipboard and
>   battery through Termux:API).
> - `TERMUX_PKG_AUTO_UPDATE=true`: releases are tagged `vX.Y.Z` on GitHub.
> - Built and installed in termux-docker (aarch64) in the project's CI on every change:
>   https://github.com/mwo-dk/coxswain/actions/workflows/termux.yml
>
> I am the upstream author and will look after build problems with new releases.

## After it is in

Termux's auto-update bot reads `TERMUX_PKG_AUTO_UPDATE=true`, looks at the latest GitHub
release of `TERMUX_PKG_SRCURL`'s repository every few hours, and commits a new
`TERMUX_PKG_VERSION` and `TERMUX_PKG_SHA256` to termux-packages itself, which builds and
publishes it. Nothing is needed from us for a release. When an update fails to build there,
the bot opens an issue in termux/termux-packages naming the package: fix it here (CI's Termux
job shows the same failure first), release, and the next bot run picks it up; a change to the
recipe itself goes in as an `fix(main/coxswain): …` pull request to termux-packages.

Keep `packaging/termux/coxswain/build.sh` here the same as the one in termux-packages, apart
from the version, so the CI tests what Termux builds.

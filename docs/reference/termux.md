[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Termux on Android

The terminal app, `coxswain`, runs in [Termux](https://termux.dev), the Linux terminal for
Android. It is built for Termux from the same source as every other release, with a recipe for
termux-packages, Termux's own package collection. The desktop app does not run on Android: it
needs a desktop with WebKitGTK.

## Contents

- [Installing](#installing)
- [Your phone's folders](#your-phones-folders)
- [Updating](#updating)
- [Uninstalling](#uninstalling)
- [The package recipe](#the-package-recipe)
- [Questions](#questions)

## Installing

Once the package is in Termux's main repository:

```sh
pkg install coxswain
```

Until then, build it in Termux with Rust's package manager. It takes a few minutes on a phone:

```sh
pkg install rust git
cargo install coxswain
```

`cargo` puts the program in `~/.cargo/bin`; add that to your `PATH` in `~/.bashrc` if `coxswain`
is not found:

```sh
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
```

Start it with `coxswain`, or the shorter `cox` when it came from the package. The package also
installs the manual page: `man coxswain` (`pkg install man` first, if `man` is missing).

| Package | What it adds |
|---|---|
| `coxswain` | The terminal app, `cox`, `man coxswain` |
| `git` (recommended) | The git column, branches and history ([Git](../panels/git.md)) |
| `termux-api` (recommended) | The commands Termux:API brings, together with the Termux:API app from the same store as Termux |

## Your phone's folders

Termux starts in its own home folder, `/data/data/com.termux/files/home`, which other apps
cannot see. To reach the phone's shared storage (Downloads, DCIM, Documents), run once:

```sh
termux-setup-storage
```

Android asks whether Termux may reach your files; allow it. Termux then makes links in
`~/storage`: `shared` is the whole internal storage, `downloads`, `dcim`, `pictures`, `music`
and `movies` lead to their folders, and `external-1` to a memory card when there is one. Open
`~/storage/shared` in a panel like any other folder: type `cd ~/storage/shared` on the
command line, or walk there from the home folder.

## Updating

With the package, Termux updates Coxswain with everything else:

```sh
pkg upgrade
```

Installed with cargo, run `cargo install coxswain` again. The once-a-day update check tells you
when a new version is out ([Update checks](updates.md)).

## Uninstalling

`pkg uninstall coxswain`, or `cargo uninstall coxswain`. The config, state and cache stay in
`~/.config/coxswain`, `~/.local/state/coxswain` and `~/.cache/coxswain`
([Where things are kept](where-things-are-kept.md)); delete them by hand to remove everything.

## The package recipe

The recipe is [`packaging/termux/coxswain/build.sh`](../../packaging/termux/coxswain/build.sh).
It builds the terminal app with Termux's Rust toolchain (`cargo build -p coxswain --locked`)
and installs `bin/coxswain`, the `cox` link and the manual page `coxswain(1)`.

Every change to the Rust code, the lock file or the recipe builds it again in CI
(`.github/workflows/termux.yml`): termux-packages' own builder makes the `aarch64` package from
that commit, then a Termux image on an ARM machine installs it and runs `coxswain --version`,
`cox --version`, `coxswain --paths` and `coxswain --dump-config`. The `.deb` is kept with the
run as *coxswain-termux-aarch64*; it installs on a phone with `dpkg -i coxswain_*.deb` (or
`apt install ./coxswain_*.deb`) from inside Termux.

To build it yourself on a Linux machine with Docker:

```sh
git clone https://github.com/termux/termux-packages
cp -r coxswain/packaging/termux/coxswain termux-packages/packages/
cd termux-packages
./scripts/run-docker.sh ./build-package.sh -I -a aarch64 coxswain
```

The package lands in `output/`. `-a arm`, `-a x86_64` and `-a i686` build for the other
architectures Termux has.

How the recipe goes to Termux, and how versions are kept current after that:
[packaging/termux/SUBMIT.md](../../packaging/termux/SUBMIT.md).

## Questions

**Why is the desktop app not on Android?** It draws its window with WebKitGTK on a Linux
desktop. Android has neither; the terminal app needs only a terminal, and Termux is one.

**`pkg install coxswain` says *Unable to locate package*.** The package is not in Termux's
repository yet. Use `cargo install coxswain` meanwhile ([Installing](#installing)); `pkg` takes
over later, and `cargo uninstall coxswain` then removes the cargo copy.

**I see only Termux's own files, not my Downloads.** Run `termux-setup-storage` once and allow
the request; your folders are then in `~/storage` ([Your phone's folders](#your-phones-folders)).
If you refused, allow *Files and media* for Termux in Android's app settings and run it again.

**Which keys do I press without F-keys?** Give Termux's extra keys row the F-keys. In
`~/.termux/termux.properties`:

```
extra-keys = [['ESC','TAB','CTRL','ALT','UP','DOWN','LEFT','RIGHT'], \
              ['F2','F3','F4','F5','F6','F7','F8','F10']]
```

then `termux-reload-settings`. The two rows above the keyboard now carry Esc, Ctrl, Alt, the
arrows and the F-keys the panels use (**F5** copy, **F6** move, **F8** delete, **F10** quit). A
hardware keyboard's F-keys work as on a PC. **F9** opens the command list with every action
and its key ([The terminal app](terminal-app.md)).

**Which phones?** Termux runs on Android 7 and later. The CI builds and tests the `aarch64`
package, which almost every phone of the last years uses; the recipe builds for `arm`, `i686` and
`x86_64` too.

**Is anything sent from the phone?** No more than on any other system: the daily update check,
which can be turned off ([Privacy](privacy.md)).

---
[← Previous: FreeBSD](freebsd.md) · [Next: macOS →](macos.md)

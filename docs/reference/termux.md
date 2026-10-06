[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Termux on Android

The terminal app, `coxswain`, runs in [Termux](https://termux.dev), the Linux terminal for
Android. It is built for Termux from the same source as every other release, with a recipe for
termux-packages, Termux's own package collection. The desktop app does not run on Android: it
needs a desktop with WebKitGTK.

Inside Termux it works the Android way: the phone's folders are in the go-to list, files open
in their Android app, copied lines go to Android's clipboard and the search helper knows when
the phone is on its battery. It tells it is in Termux from `TERMUX_VERSION`, or from `$PREFIX`
being under `/data/data/com.termux/`; Termux sets both.

## Contents

- [Installing](#installing)
- [Your phone's folders](#your-phones-folders)
- [Opening files](#opening-files)
- [Clipboard and battery: Termux:API](#clipboard-and-battery-termuxapi)
- [The search helper](#the-search-helper)
- [Search by meaning](#search-by-meaning)
- [How Termux differs from Linux here](#how-termux-differs-from-linux-here)
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
| `termux-api` (recommended) | Android's clipboard and the battery state, together with the Termux:API app from the same store as Termux ([Termux:API](#clipboard-and-battery-termuxapi)) |


The static Linux build from the releases page
(`coxswain-terminal-<version>-aarch64-unknown-linux-musl.tar.gz`) starts in Termux too, but it
cannot look up names on the network: it looks for `/etc/resolv.conf`, which Android does not
have. Browsing and search work; the update check, the model download and model servers do not.
Prefer the package or `cargo install`.

Git glyphs want a Nerd Font: put one at `~/.termux/font.ttf` and run `termux-reload-settings`,
or set `glyphs = "ascii"` ([Glyphs and fonts](../customise/glyphs-and-fonts.md)).

## Your phone's folders

Termux starts in its own home folder, `/data/data/com.termux/files/home`, which other apps
cannot see. To reach the phone's shared storage (Downloads, DCIM, Documents), run once:

```sh
termux-setup-storage
```

Android asks whether Termux may reach your files; allow it. Termux then makes links in
`~/storage`: `shared` is the whole internal storage, `downloads`, `dcim`, `pictures`, `music`
and `movies` lead to their folders, and `external-1` to a memory card when there is one. They are a key away:

| Key | What you see |
|---|---|
| **Alt+F1** / **Alt+F2** | A list for the left / right panel: *Type a path…* first, then *Phone: shared*, *Phone: dcim*, *Phone: downloads* … **Enter** opens one |

Until `termux-setup-storage` has been run, the status line says once: *In Termux: run
termux-setup-storage once, and Alt+F1 lists your phone's folders (shared storage, Downloads,
DCIM …)*. A link to a memory card that was taken out is left out of the list.

## Opening files

**Enter** on a file that is not a program opens it with `termux-open`: Android shows its *Open
with* chooser, or opens the file in its app at once. The status line says *Opened …*. Programs
run in the terminal, as on Linux; **F3** views and **F4** edits in the terminal as everywhere.

## Clipboard and battery: Termux:API

Two things go through [Termux:API](https://wiki.termux.com/wiki/Termux:API): the Termux:API app
(from F-Droid, like Termux) and its commands:

```sh
pkg install termux-api
```

| With Termux:API | Without |
|---|---|
| A line copied in Settings (an install command) goes to Android's clipboard with `termux-clipboard-set` | It is sent to the terminal (OSC 52), which Termux puts on the clipboard too when it allows it |
| `termux-battery-status` tells the search helper when the phone runs on its battery, and reading waits ([Battery](../search/battery.md)) | The helper looks in `/sys/class/power_supply`, which Android usually hides, so it reads on battery too |

When the commands are missing, the status line says once: *In Termux: pkg install termux-api
and the Termux:API app put copied lines on Android's clipboard and tell the search helper when
the phone runs on its battery*. Without the app, the commands would wait for it for ever; Coxswain
gives each one a few seconds and then does without.

## The search helper

Android has no session service, so *Start with my session* (Settings → *Finding files*) cannot
be turned on in Termux. Ticking it says *Termux has no session service: the search helper starts
with Coxswain and stays ten minutes after it*. That is what happens: the first `coxswain` starts
the helper, and it reads in the background while Termux runs ([The search helper](../search/helper.md)).

Android stops apps it thinks are idle. To keep reading with the screen off, pull down Termux's
notification and choose *Acquire wakelock*, or run `termux-wake-lock`.

## Search by meaning

The built-in model runs on the phone's processor. It works, but making the vectors for many
files takes long on a phone and warms it. A model server on another computer does it faster:
*Settings → Finding files → Set up…* (`coxswain --setup-search`) finds one on your network
([Model servers](../search/servers.md)). Words inside files need no model.

## How Termux differs from Linux here

| | Linux | Termux |
|---|---|---|
| Opening a file | `xdg-open` | `termux-open` |
| Clipboard | OSC 52 | `termux-clipboard-set`, else OSC 52 |
| Battery | `/sys/class/power_supply` | `termux-battery-status`, else `/sys/class/power_supply` |
| Start with my session | A systemd user service | Not available; the helper stays ten minutes after the app |
| Certificates for HTTPS | The system's store | Termux's own, `$PREFIX/etc/tls/cert.pem` (`pkg install ca-certificates`) |
| The go-to list (Alt+F1) | Type a path | Type a path, and the phone's folders |
| F8 (Delete) | To the desktop's trash | Android has no trash: F8 says so, and **Shift+F8** deletes for good after asking. The static Linux binary keeps a trash in `~/.local/share/Trash` |
| Update hint | By where it is installed | `cargo install coxswain` for a cargo build; `pkg upgrade coxswain` for one installed with `pkg` |

Everything else, keys, config and the search index included, is the same. The config is in
`~/.config/coxswain` inside Termux's home ([Where things are kept](where-things-are-kept.md)).

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
the request; your folders are then in `~/storage`, and **Alt+F1** lists them ([Your phone's folders](#your-phones-folders)).
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


**Why does copying say nothing ends up on the clipboard?** Without Termux:API, the line is sent to the terminal (OSC 52). Install both the Termux:API app
and `pkg install termux-api`; then `termux-clipboard-set` puts it on Android's clipboard.

**Why can I not turn on *Start with my session*?** Android has no per-user service manager for Termux to register with. The helper starts with
Coxswain and stays ten minutes after the last one quits. `termux-wake-lock` keeps Android from
stopping it while the screen is off.

**Why does the update check never say anything?** With the static musl binary it cannot look up `api.github.com` on Android. Built with
`cargo install coxswain` it can. With either, `coxswain --version` says which version you have.

**Why does F8 say Android has no trash?** The Termux build (the package, or `cargo install coxswain`) has no desktop trash to move
files to. **Shift+F8** (*Delete permanently*) asks, then deletes for good.

**Will reading files drain my battery?** Not with Termux:API installed: the helper waits while the phone is unplugged. Without it,
Coxswain cannot tell, and reads on battery too.

---
[← Previous: Linux on ARM](linux-arm.md) · [Next: ChromeOS →](chromeos.md)

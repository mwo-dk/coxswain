[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Termux on Android

The terminal app, `coxswain`, runs in [Termux](https://termux.dev), the Linux terminal for
Android phones, tablets and Chromebooks with Android apps. It knows when it is in Termux and
works the Android way there: the phone's folders are in the go-to list, files open in their
Android app, copied lines go to Android's clipboard and the search helper knows when the phone
is on its battery. The desktop app needs a desktop and does not run in Termux.

## Contents

- [Installing](#installing)
- [The phone's folders](#the-phones-folders)
- [Opening files](#opening-files)
- [Clipboard and battery: Termux:API](#clipboard-and-battery-termuxapi)
- [The search helper](#the-search-helper)
- [Search by meaning](#search-by-meaning)
- [How Termux differs from Linux here](#how-termux-differs-from-linux-here)
- [Questions](#questions)

## Installing

Install Termux from [F-Droid](https://f-droid.org/packages/com.termux/) or its GitHub releases,
not from the Play Store (that build is old). Then, in Termux:

```sh
pkg install rust git
cargo install coxswain
```

This builds Coxswain on the phone, for Android itself (`aarch64-linux-android`), and takes a
while. `coxswain` (or `cox`) then starts it.

The static Linux build from the releases page
(`coxswain-terminal-<version>-aarch64-unknown-linux-musl.tar.gz`) runs in Termux too, but it
cannot look up names on the network: it looks for `/etc/resolv.conf`, which Android does not
have. Browsing and search work; the update check, the model download for search by meaning and
model servers do not. Prefer `cargo install`.

Coxswain tells it is in Termux from `TERMUX_VERSION`, or from `$PREFIX` being under
`/data/data/com.termux/`. Termux sets both.

Git glyphs want a Nerd Font: put one at `~/.termux/font.ttf` and run `termux-reload-settings`,
or set `glyphs = "ascii"` ([Glyphs and fonts](../customise/glyphs-and-fonts.md)).

## The phone's folders

Termux sees only its own home until it is allowed into the phone's storage. Run once:

```sh
termux-setup-storage
```

Android asks whether Termux may use your files; say yes. Termux then makes links under
`~/storage`: `shared` (the whole internal storage), `downloads`, `dcim`, `pictures`, `music`,
`movies`, `documents`, and `external-1` … for a memory card.

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
| F8 (Delete) | To the desktop's trash | In a `cargo install` build, Android has no trash: F8 says so, and **Shift+F8** deletes for good after asking. The static Linux binary keeps a trash in `~/.local/share/Trash` |
| Update hint | By where it is installed | `cargo install coxswain` for a cargo build; `pkg upgrade coxswain` for one installed with `pkg` |

Everything else, keys, config and the search index included, is the same. The config is in
`~/.config/coxswain` inside Termux's home ([Where things are kept](where-things-are-kept.md)).

## Questions

#### Why does Alt+F1 not show my phone's folders?

`termux-setup-storage` has not been run, or Android was told no. Run it again and allow access
to files; then **Alt+F1** lists *Phone: shared*, *Phone: downloads* and the others.

#### Why does copying say nothing ends up on the clipboard?

Without Termux:API, the line is sent to the terminal (OSC 52). Install both the Termux:API app
and `pkg install termux-api`; then `termux-clipboard-set` puts it on Android's clipboard.

#### Why can I not turn on *Start with my session*?

Android has no per-user service manager for Termux to register with. The helper starts with
Coxswain and stays ten minutes after the last one quits. `termux-wake-lock` keeps Android from
stopping it while the screen is off.

#### Why does the update check never say anything?

With the static musl binary it cannot look up `api.github.com` on Android. Built with
`cargo install coxswain` it can. With either, `coxswain --version` says which version you have.

#### Why does F8 say Android has no trash?

A build for Android itself (`cargo install coxswain` in Termux) has no desktop trash to move
files to. **Shift+F8** (*Delete permanently*) asks, then deletes for good.

#### Does the desktop app run in Termux?

No: it needs a desktop with WebKitGTK. On a Chromebook use its Linux instead
([ChromeOS](chromeos.md)).

#### Will reading files drain my battery?

Not with Termux:API installed: the helper waits while the phone is unplugged. Without it,
Coxswain cannot tell, and reads on battery too.

---
[← Previous: Linux on ARM](linux-arm.md) · [Next: ChromeOS →](chromeos.md)

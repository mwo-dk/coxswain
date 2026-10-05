[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Update checks

Once a day, each app asks GitHub whether a newer release exists, and tells you when it does,
with the command that upgrades your copy. Nothing is downloaded or installed by itself.

<!-- screenshot: reference-update.png: the desktop app (Cyber theme), the command line row at the bottom with the update button at its right reading "Coxswain 1.21.0 is available: brew upgrade coxswain" -->

## Contents

- [How to use it](#how-to-use-it)
- [What it does](#what-it-does)
- [What you see](#what-you-see)
- [The upgrade command](#the-upgrade-command)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Nothing to do: it is on by default.
2. When a newer version exists, a button appears at the right of the command line (desktop app)
   or a line in the status line (terminal app).
3. Run the command it names in a shell, or click the button for the release page.
4. After the upgrade, the desktop app's **⚙ Settings** button shows a count: click it for
   *Settings → What's new*, with the new version's changes marked *new*. The terminal app says
   once `Coxswain 1.21.0 is installed: coxswain --whats-new says what it brought`
   ([Notices and what's new](../search/notices.md)).

To turn it off: Settings (**Ctrl+,**) → *Behaviour* → untick *Check for a new version once a
day*, or `check_updates = false` in `config.toml`.

## What it does

- It asks `https://api.github.com/repos/mwo-dk/coxswain/releases/latest` for the latest
  release's version number, with `User-Agent: coxswain/1.20.0`. Nothing else is sent.
- It waits at most 5 seconds. Offline, or with GitHub unreachable, it simply finds nothing and
  says nothing.
- It uses the system's certificate store, so it works behind a company proxy that inspects TLS.
- The time of the check and the answer are kept in `state.json`, so several windows and the
  terminal app ask once a day between them.
- The desktop app looks again every hour while it is open (and asks GitHub only when a day has
  passed), so a window left open for weeks still hears of a release. The terminal app checks
  once, when it starts.
- Only a higher `major.minor.patch` counts as newer.

## What you see

| | Desktop app | Terminal app |
|---|---|---|
| Newer version, installed by a package manager | A button at the right of the command line: *Coxswain 1.21.0 is available: brew upgrade coxswain*. Its tooltip: *Upgrade with: brew upgrade coxswain. Click for the release notes.* | The status line: `Coxswain 1.21.0 is available: brew upgrade coxswain` |
| Newer version, installed from a download | *Coxswain 1.21.0 is available*, tooltip *Open the releases page* | `Coxswain 1.21.0 is available: https://github.com/mwo-dk/coxswain/releases/latest` |
| Click | Opens the release page in your browser | – |
| How long | Until you update; it has no `×` | Until the status line shows something else |
| After updating | A count on **⚙ Settings**; a click opens *Settings → What's new*, where the new version's changes are marked *new* | Once, in the status line: `Coxswain 1.21.0 is installed: coxswain --whats-new says what it brought` |

While the update button shows, the desktop app holds back a problem with search in the status
line ([Notices and what's new](../search/notices.md)).

## The upgrade command

It is worked out from where the program is installed:

| Installed with | Command shown |
|---|---|
| Homebrew formula (the terminal app) | `brew upgrade coxswain` |
| Homebrew cask (the desktop app) | `brew upgrade --cask coxswain-gui` |
| Cargo | `cargo install coxswain` |
| WinGet, the terminal app (`mwo-dk.Coxswain.Terminal`) | `winget upgrade mwo-dk.Coxswain.Terminal` |
| Anything else on FreeBSD (the install script, the archives) | `fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh \| sh`, which updates both apps where they are ([FreeBSD](freebsd.md#updating)) |
| A download, `.deb`, `.rpm`, AppImage, the Windows installer (also when WinGet ran it: `winget upgrade mwo-dk.Coxswain`), AUR, a source build | None: it points to the [releases page](https://github.com/mwo-dk/coxswain/releases/latest) |

The install tables in the [README](../../README.md#install) say how to update each.

## Settings and config.toml

| Settings | Key | Type | Default |
|---|---|---|---|
| *Behaviour* → *Check for a new version once a day* | `check_updates` | bool | `true` |

The last check is kept in `state.json` as `update_checked` (a time) and `latest_version`; the
version whose notice you saw is `seen_version`. None of these are for editing.

## In the terminal app

It checks once at start, on a thread of its own, so a slow network never holds up the panels.
The answer shows in the status line; there is no button and nothing to click. The same
`check_updates` key turns it off.

## Questions

#### Does Coxswain update itself?

No. It only tells you. Updating is left to the way you installed it, so your package manager
stays in charge.

#### Why does it show a URL instead of a command?

It could not tell a package manager from where the program runs: a download, a `.deb`, an
AppImage, or the Windows desktop installer, which lands in the same folder whether you ran it
yourself or WinGet did (then `winget upgrade mwo-dk.Coxswain`). Update it the way you installed
it; the README's install tables say how.

#### I updated, but it still says a newer version is available.

The running app is the old one: quit every Coxswain window and the terminal app, then start
again. `coxswain --version` shows the version you now have.

#### It never tells me about updates.

Check that `check_updates` is not `false`, and that the machine can reach `api.github.com`. The
check runs at most once a day, so a release out an hour ago may show tomorrow.

#### What does the check send?

Only the request for the latest version, with `User-Agent: coxswain/1.20.0`. See
[Privacy](privacy.md#what-can-leave-it).

#### How do I get rid of the update button?

Update. Or turn the check off in Settings → *Behaviour*; the button then goes at the next hourly look,
or when the app starts again.

---
[← Previous: Privacy](privacy.md) · [Next: Where things are kept →](where-things-are-kept.md)

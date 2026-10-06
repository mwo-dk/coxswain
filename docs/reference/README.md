[← README](../../README.md) · [Docs index](../README.md)

# Reference

These pages cover the parts of Coxswain that sit under the features. They say what the terminal
app can and cannot do, list every command-line flag and every `config.toml` key with its
default, say what can go over the network, explain the update check, and show where each file
Coxswain writes is kept, and how its dependencies are kept safe. Come here when a feature page tells you "the key is `…`" and you want
the full list.

![The terminal app: two blue panels, a git repository on the left, the F-key bar below](../screenshots/tui-panels.png)
*The terminal app, `coxswain`, in its default theme NC. It reads the same `config.toml` as the desktop app.*

| Page | What it covers |
|---|---|
| [The terminal app](terminal-app.md) | What `coxswain` has, what only the desktop app has and why, where the two behave differently, archives and search by meaning in the terminal, the terminal it needs |
| [Command-line flags](command-line-flags.md) | Every flag of `coxswain` and `coxswain-gui`: folders to start in, `--paths`, `--dump-config`, `--meaning`, `--index-service`, `--settings`, `--duplicates`, `--index-helper` |
| [Configuration: every key](configuration.md) | The `config.toml` file, every key with its type and default, which Settings item writes it, and which app reads it |
| [Privacy](privacy.md) | What stays on your machine, every case where something can leave it, and how to stop each one |
| [Update checks](updates.md) | The once-a-day version check, what you see in each app, the upgrade command, turning it off |
| [Where things are kept](where-things-are-kept.md) | The config, state and cache folders on Linux, macOS and Windows, every file in them, what is safe to delete, removing everything |
| [Security](security.md) | How dependencies are chosen and kept current, the advisory checks on every change and every week, licences, the bundled viewers, checking a download, reporting a problem |
| [Licences and bills of materials](bills-of-materials.md) | What each release carries (SBOMs, a CBOM, the third-party notices), how licences are checked, the cryptography both apps use, making the files yourself |
| [Performance](performance.md) | What keeps each app quick: rows on screen only, no disk work on the window's thread, background measuring, and the numbers for a folder of 100,000 files and an index of a million names |
| [FreeBSD](freebsd.md) | Both apps on FreeBSD: the one-line install, the packages and what each is for, the search helper from the session, rc.d or the login shell, search by meaning, how FreeBSD differs, updating, the desktop app's experimental status |
| [Nix](nix.md) | The flake: `nix run github:mwo-dk/coxswain`, `nix profile install`, NixOS and Home Manager, the desktop app on Linux, updating, the package prepared for nixpkgs |
| [macOS](macos.md) | The folder prompts macOS shows and what to answer, what Coxswain never reads on a Mac (`~/Library`, Photos and Music libraries), Full Disk Access (not needed), why a new version may ask again, search by meaning on the GPU with no server |

## Keys at a glance

These pages are mostly about files and flags, not keys. The keys that reach them:

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| **Ctrl+,** | Opens Settings | Says *Settings is available in the desktop app (coxswain-gui)* | Settings write `config.toml` ([Configuration](configuration.md)) |
| **F1** | Help, with the version in its title | Help | The keys in use, from your `[keys]` |
| **F9** | The command list | The command list | Every action and its key; the terminal app leaves out what it lacks |
| **Ctrl+O** | One pane or two | Shows the terminal with the last command's output | See [The terminal app](terminal-app.md#where-the-two-differ) |
| **F10** | Quit | Quit | |

---
[← Previous: Glyphs and fonts](../customise/glyphs-and-fonts.md) · [Next: The terminal app →](terminal-app.md)

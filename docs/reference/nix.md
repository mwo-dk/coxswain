[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Nix

The repository is a Nix flake. It builds the terminal app, `coxswain`, on Linux and macOS, and
the desktop app, `coxswain-gui`, on Linux, from the source of the commit you ask for. Nothing
is downloaded from the releases page: Nix compiles both apps itself, with the exact crates in
`Cargo.lock` and the npm packages in `gui/package-lock.json`. Both are built on every change
that touches them (the *Nix* workflow), on Linux and macOS.

A package for nixpkgs itself is prepared ([below](#nixpkgs-and-the-flake)); until it is
accepted, the flake is the way to get Coxswain with Nix.

## Contents

- [Try it without installing](#try-it-without-installing)
- [Install](#install)
- [NixOS and Home Manager](#nixos-and-home-manager)
- [The desktop app](#the-desktop-app)
- [What the packages hold](#what-the-packages-hold)
- [Updating](#updating)
- [nixpkgs and the flake](#nixpkgs-and-the-flake)
- [Questions](#questions)

## Try it without installing

```sh
nix run github:mwo-dk/coxswain                    # the terminal app
nix run github:mwo-dk/coxswain#coxswain-gui       # the desktop app (Linux)
nix run github:mwo-dk/coxswain/v2.1.0             # a given release
```

Arguments go after `--`: `nix run github:mwo-dk/coxswain -- ~/src ~/Downloads` opens those two
folders in the panels. Flakes must be on (`experimental-features = nix-command flakes` in
`nix.conf`).

## Install

```sh
nix profile install github:mwo-dk/coxswain                # coxswain and cox
nix profile install github:mwo-dk/coxswain#coxswain-gui   # coxswain-gui, its menu entry and icon
```

| Flake output | What | Systems |
|---|---|---|
| `packages.<system>.default`, `.coxswain` | The terminal app | `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `aarch64-darwin` |
| `packages.<system>.coxswain-gui` | The desktop app | `x86_64-linux`, `aarch64-linux` |
| `apps.<system>.default`, `.coxswain`, `.coxswain-gui` | The same, for `nix run` | As above |

## NixOS and Home Manager

Add the flake as an input and put the packages in your system or your home:

```nix
{
  inputs.coxswain.url = "github:mwo-dk/coxswain";
  # inputs.coxswain.inputs.nixpkgs.follows = "nixpkgs";  # build with your nixpkgs

  outputs = { nixpkgs, coxswain, ... }: {
    nixosConfigurations.mybox = nixpkgs.lib.nixosSystem {
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            coxswain.packages.${pkgs.stdenv.hostPlatform.system}.coxswain
            coxswain.packages.${pkgs.stdenv.hostPlatform.system}.coxswain-gui
          ];
        })
      ];
    };
  };
}
```

With Home Manager the same two lines go in `home.packages`. Coxswain's settings stay in
`~/.config/coxswain/config.toml` as on any Linux ([Where things are kept](where-things-are-kept.md));
Home Manager can write that file with `xdg.configFile."coxswain/config.toml".text`, but then
Settings cannot save to it, since the file is a read-only link into the store.

## The desktop app

`coxswain-gui` is built against nixpkgs' WebKitGTK 4.1 and wrapped by `wrapGAppsHook3`, so it
finds GTK's schemas, its icons, `glib-networking` (for the update check over HTTPS) and the
GStreamer plugins (base and good) that play video and sound in the preview pane. The package
adds a menu entry, *Coxswain*, and the icon. It is Linux only: on a Mac use the `.dmg` or
Homebrew's cask ([Install](../../README.md#install)).

The programs some previews and search use are not pulled in: tesseract (scans), pdftoppm,
LibreOffice, LaTeX, PlantUML, pandoc, git. Install the ones you want next to it; when one is
missing the preview says so and shows its install line
([Programs Coxswain uses](../previews/README.md)).

## What the packages hold

| Package | Files |
|---|---|
| `coxswain` | `bin/coxswain`, `bin/cox` (the same app, shorter), the manual page `coxswain(1)` |
| `coxswain-gui` | `bin/coxswain-gui` (wrapped), `share/applications/coxswain.desktop`, `share/icons/hicolor/128x128/apps/coxswain.png` |

The terminal app's build runs the tests of `coxswain` and `coxswain-core`, with git at hand
(the history and branch tests make small repositories) and a temporary home. The tokenizer for
search by meaning links nixpkgs' oniguruma rather than building its own copy.

## Updating

- `nix profile upgrade coxswain` (or the index `nix profile list` shows) for a profile install.
- `nix flake update coxswain` and a rebuild for NixOS and Home Manager.

The once-a-day update check cannot tell a Nix install from a download, so its notice points at
the releases page. Update with the commands above instead, or turn the check off with
`check_updates = false` in `config.toml` (Settings → *Privacy and updates* → *Check for a new
version*); see [Update checks](updates.md).

## nixpkgs and the flake

| | The flake (this repository) | nixpkgs (once accepted) |
|---|---|---|
| Source | The commit you name, `master` by default | The tagged release |
| Install | `nix profile install github:mwo-dk/coxswain` | `nix profile install nixpkgs#coxswain`, `environment.systemPackages = [ pkgs.coxswain ]` |
| Binary cache | None: Nix builds it on your machine | cache.nixos.org has it built |
| Updates | When you update the flake input | When nixpkgs' bot ([r-ryantm](https://github.com/nix-community/nixpkgs-update)) has opened its update and it is merged; then with your channel |
| Desktop app | `#coxswain-gui` | `pkgs.coxswain-gui`, if it is accepted too |

The files for nixpkgs are in
[`packaging/nix/nixpkgs`](../../packaging/nix/nixpkgs), and how they are submitted in
[`packaging/nix/SUBMIT.md`](../../packaging/nix/SUBMIT.md). The *Nix* workflow builds them too,
from the release they name.

## Questions

### Why does `nix run github:mwo-dk/coxswain` take so long the first time?

There is no binary cache for the flake, so Nix compiles the app and all its crates: a few
minutes for the terminal app, longer for the desktop app with its frontend. The result stays in
your store, and the next `nix run` of the same commit starts at once. Once Coxswain is in
nixpkgs, `nix run nixpkgs#coxswain` comes ready-built from cache.nixos.org.

### Why is there no desktop app for macOS in the flake?

The Mac app is a signed-ad-hoc `.app` bundle with its own Info.plist, folder-prompt texts and
Metal for search by meaning; Nix would give a bare binary without those. Use the `.dmg` or
`brew install --cask mwo-dk/coxswain/coxswain-gui`. The terminal app builds on both Macs.

### The update notice says a new version is out, but `nix profile upgrade` changes nothing.

The notice comes from GitHub's latest release; a profile installed from the flake follows
`master`, which has the release as soon as it is tagged, so `nix profile upgrade coxswain`
fetches it. With nixpkgs the new version arrives only after the update has been merged and has
reached your channel, which can take days. Turn the notice off with `check_updates = false`
if it gets ahead of your channel.

### Can Settings save when my `config.toml` comes from Home Manager?

No. Home Manager makes `~/.config/coxswain/config.toml` a link into the read-only Nix store, so
the desktop app's Settings and the first-run guide cannot write it, and their changes are lost.
Either let Coxswain own the file, or keep all settings in your Home Manager configuration and
make changes there. `coxswain --dump-config` prints every key with its default to start from.

### Does the search helper start with my session on NixOS?

Settings → *Finding files* → *Details* → *Background reading* → *Start with my session* (or
`coxswain --index-service on`) writes a systemd user unit under `~/.config/systemd/user`. It
names the program by its link on your PATH (`~/.nix-profile/bin/coxswain`,
`/run/current-system/sw/bin/coxswain`) when there is one, so after an upgrade it starts the new
version. The desktop app in Nix is a wrapper script around the real program, so when you turn
the option on there the unit names the program's store path, which a garbage collection after
an upgrade removes. Turn it on with `coxswain --index-service on` instead (the terminal app
runs the same helper), or turn it off and on again after each upgrade.
See [The search helper](../search/helper.md).

### Which nixpkgs does the flake build with?

The one in `flake.lock` (nixos-unstable at the time it was last locked). To build with yours,
set `inputs.coxswain.inputs.nixpkgs.follows = "nixpkgs"`; Coxswain needs a Rust toolchain that
supports edition 2024 (Rust 1.85 or newer).

---
[← Previous: Flatpak](flatpak.md) · [Next: Termux on Android →](termux.md)

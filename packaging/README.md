# Package managers

`release.yml` publishes each release to these, once their secret is set. Without the secret the
job just leaves a notice.

| Where | What | Secret |
|---|---|---|
| crates.io | `coxswain-core`, then `coxswain` (`cargo install coxswain`) | `CARGO_REGISTRY_TOKEN` |
| WinGet | `mwo-dk.Coxswain` (desktop installer) and `mwo-dk.Coxswain.Terminal` (portable zip) in [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), as pull requests opened by Komac | `WINGET_TOKEN` |
| Homebrew | `coxswain` formula (terminal) and `coxswain-gui` cask (desktop: .dmg on macOS, AppImage on Linux) in [mwo-dk/homebrew-coxswain](https://github.com/mwo-dk/homebrew-coxswain) | `TAP_TOKEN` |

The AUR packages in `aur/` are kept, but not published while AUR registration is closed.

The FreeBSD port in `freebsd/sysutils/coxswain` is submitted by hand for now; see
[freebsd/README.md](freebsd/README.md).

The Flatpak of the desktop app in `flatpak/` is built and tested by the *Flatpak* workflow and
not on Flathub; why, and what submitting it would take, is in [flatpak/SUBMIT.md](flatpak/SUBMIT.md).

The Nix flake (`flake.nix` at the root) builds from `nix/package.nix` and `nix/gui.nix`; the
packages for nixpkgs are in `nix/nixpkgs/`, submitted by hand: see [nix/SUBMIT.md](nix/SUBMIT.md).

The Termux recipe in `termux/coxswain/build.sh` is built and started in CI (`termux.yml`) and
submitted to termux-packages by hand; see [termux/SUBMIT.md](termux/SUBMIT.md). After that,
Termux's own bot follows our releases.

The recipes for illumos and pkgsrc are built in CI where that is cheap and submitted by hand:
[omnios/](omnios/SUBMIT.md) for omnios-extra (`omnios-extra.yml` builds it with their build
system on OmniOS), [openindiana/](openindiana/SUBMIT.md) for oi-userland (not built in CI), and
[pkgsrc/](pkgsrc/SUBMIT.md) for pkgsrc-wip, which covers NetBSD and SmartOS (`pkgsrc.yml` builds
it on NetBSD; `pkgsrc/update.py` moves it to a new release).

After each release, `release-check.yml` installs it the way a user does, from its published
downloads and with the install scripts of its tag: `install-freebsd.sh` on FreeBSD 14.5 and 15.0,
`install-unix.sh` on NetBSD 10.1, OpenBSD 7.9 and OmniOS r151058, the static musl archive and the
`.deb` (with apt, started on a virtual display) on Linux x86-64 and ARM64, and
`cargo install coxswain` in Termux. It checks the version each prints, the manual page and the
service file, and every checksum against `SHA256SUMS`. It runs after every Release run that
published a release, and by hand (*Actions → Release check → Run workflow*, a tag or `latest`).
Nothing waits for it: a red run means a release that does not install, fixed in the next one.

`taps/generate.sh` writes the Homebrew formula and cask from a release's `.sha256` assets.

**crates.io:** make a token at <https://crates.io/settings/tokens> with the
`publish-new` and `publish-update` scopes, then
`gh secret set CARGO_REGISTRY_TOKEN -R mwo-dk/coxswain`.

**WinGet:** the first version of each package was submitted by hand (microsoft/winget-pkgs
#446561 and #446560); the release job only adds later versions, and says so and skips while a
package is not listed yet. Make a classic token at <https://github.com/settings/tokens> with the
`public_repo` scope, from the account that owns the fork `mwo-dk/winget-pkgs`, then
`gh secret set WINGET_TOKEN -R mwo-dk/coxswain`. Every new package version is reviewed by
Microsoft before it is listed, usually within a day.

**Homebrew:** make a fine-grained token at <https://github.com/settings/personal-access-tokens>
limited to `mwo-dk/homebrew-coxswain`, with *Contents: read and write*, then
`gh secret set TAP_TOKEN -R mwo-dk/coxswain`.

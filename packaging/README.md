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

The Termux recipe in `termux/coxswain/build.sh` is built and started in CI (`termux.yml`) and
submitted to termux-packages by hand; see [termux/SUBMIT.md](termux/SUBMIT.md). After that,
Termux's own bot follows our releases.

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

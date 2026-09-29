# Package managers

`release.yml` publishes each release to these, once their secret is set. Without the secret the
job just leaves a notice.

| Where | What | Secret |
|---|---|---|
| crates.io | `coxswain-core`, then `coxswain` (`cargo install coxswain`) | `CARGO_REGISTRY_TOKEN` |
| Homebrew | `coxswain` formula (terminal) and `coxswain-gui` cask (desktop: .dmg on macOS, AppImage on Linux) in [mwo-dk/homebrew-coxswain](https://github.com/mwo-dk/homebrew-coxswain) | `TAP_TOKEN` |
| Scoop | manifest in [mwo-dk/scoop-coxswain](https://github.com/mwo-dk/scoop-coxswain) | `TAP_TOKEN` |
| winget | `mwo-dk.Coxswain` (the MSI) and `mwo-dk.Coxswain.Terminal` (the zip), as pull requests to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) | `WINGET_TOKEN` |

The AUR packages in `aur/` are kept, but not published while AUR registration is closed.

`taps/generate.sh` writes the Homebrew formula, cask and Scoop manifest from a release's `.sha256`
assets.

**crates.io:** make a token at <https://crates.io/settings/tokens> with the
`publish-new` and `publish-update` scopes, then
`gh secret set CARGO_REGISTRY_TOKEN -R mwo-dk/coxswain`.

**Homebrew and Scoop:** make a fine-grained token at <https://github.com/settings/personal-access-tokens>
limited to `mwo-dk/homebrew-coxswain` and `mwo-dk/scoop-coxswain`, with *Contents: read and write*,
then `gh secret set TAP_TOKEN -R mwo-dk/coxswain`.

**winget:** the release job only adds versions to packages that already exist, so the first
version of each was submitted by hand with [Komac](https://github.com/russellbanks/Komac)
(`komac new`). The terminal app's manifests are written by `winget/terminal.sh` each release instead of `komac update`, because the exe's path inside the zip contains the version. Each pull request is reviewed by winget's maintainers, which takes hours to
days; until it merges, `winget upgrade` does not see the new version. For the job, make a
classic token at <https://github.com/settings/tokens> with only the `public_repo` scope
(Komac pushes to your fork of winget-pkgs), then `gh secret set WINGET_TOKEN -R mwo-dk/coxswain`.

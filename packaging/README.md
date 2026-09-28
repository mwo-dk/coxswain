# Package managers

`release.yml` publishes each release to these, once their secret is set. Without the secret the
job just leaves a notice.

| Where | What | Secret |
|---|---|---|
| crates.io | `bosum-core`, then `bosum` (`cargo install bosum`) | `CARGO_REGISTRY_TOKEN` |
| Homebrew | `bosum` formula (terminal) and `bosum-gui` cask (desktop: .dmg on macOS, AppImage on Linux) in [mwo-dk/homebrew-bosum](https://github.com/mwo-dk/homebrew-bosum) | `TAP_TOKEN` |
| Scoop | manifest in [mwo-dk/scoop-bosum](https://github.com/mwo-dk/scoop-bosum) | `TAP_TOKEN` |
| winget | `mwo-dk.Bosum` (the MSI) and `mwo-dk.Bosum.Terminal` (the zip), as pull requests to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) | `WINGET_TOKEN` |

The AUR packages in `aur/` are kept, but not published while AUR registration is closed.

`taps/generate.sh` writes the Homebrew formula, cask and Scoop manifest from a release's `.sha256`
assets.

**crates.io:** make a token at <https://crates.io/settings/tokens> with the
`publish-new` and `publish-update` scopes, then
`gh secret set CARGO_REGISTRY_TOKEN -R mwo-dk/bosum`.

**Homebrew and Scoop:** make a fine-grained token at <https://github.com/settings/personal-access-tokens>
limited to `mwo-dk/homebrew-bosum` and `mwo-dk/scoop-bosum`, with *Contents: read and write*,
then `gh secret set TAP_TOKEN -R mwo-dk/bosum`.

**winget:** the release job only adds versions to packages that already exist, so the first
version of each was submitted by hand with [Komac](https://github.com/russellbanks/Komac)
(`komac new`). Each pull request is reviewed by winget's maintainers, which takes hours to
days; until it merges, `winget upgrade` does not see the new version. For the job, make a
classic token at <https://github.com/settings/tokens> with only the `public_repo` scope
(Komac pushes to your fork of winget-pkgs), then `gh secret set WINGET_TOKEN -R mwo-dk/bosum`.

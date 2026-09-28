# Package managers

`release.yml` publishes each release to these, once their secret is set. Without the secret the
job just leaves a notice.

| Where | What | Secret |
|---|---|---|
| crates.io | `coxswain-core`, then `coxswain` (`cargo install coxswain`) | `CARGO_REGISTRY_TOKEN` |
| Homebrew | `coxswain` formula (terminal) and `coxswain-gui` cask (desktop, macOS) in [mwo-dk/homebrew-coxswain](https://github.com/mwo-dk/homebrew-coxswain) | `TAP_TOKEN` |
| Scoop | manifest in [mwo-dk/scoop-coxswain](https://github.com/mwo-dk/scoop-coxswain) | `TAP_TOKEN` |

The AUR packages in `aur/` are kept, but not published while AUR registration is closed.

`taps/generate.sh` writes the Homebrew formula, cask and Scoop manifest from a release's `.sha256`
assets.

**crates.io:** make a token at <https://crates.io/settings/tokens> with the
`publish-new` and `publish-update` scopes, then
`gh secret set CARGO_REGISTRY_TOKEN -R mwo-dk/coxswain`.

**Homebrew and Scoop:** make a fine-grained token at <https://github.com/settings/personal-access-tokens>
limited to `mwo-dk/homebrew-coxswain` and `mwo-dk/scoop-coxswain`, with *Contents: read and write*,
then `gh secret set TAP_TOKEN -R mwo-dk/coxswain`.

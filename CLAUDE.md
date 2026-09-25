# Working on Coxswain

- **Releases are automatic.** The version lives only in `Cargo.toml` (`[workspace.package]`).
  When CI passes on `master` and that version has no GitHub release yet, `release.yml` builds
  every platform, publishes the release (creating the `vX.Y.Z` tag) and updates the AUR.
- **Bump the version in the same commit as a user-visible change:** patch for fixes, minor for
  features, major for breaking config or key changes. Then run `cargo check` so `Cargo.lock`
  follows. Do not bump for docs, CI or refactors.
- **Commits:** plain messages. No `Co-Authored-By` or other Claude attribution.
- **Check before pushing:** `cargo test --workspace`, and for GUI changes
  `cd gui && npx svelte-check && npm run build`.

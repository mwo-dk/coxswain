# Working on Coxswain

- **Releases are automatic.** The version lives only in `Cargo.toml` (`[workspace.package]`).
  When CI passes on `master` and that version has no GitHub release yet, `release.yml` builds
  every platform, publishes the release (creating the `vX.Y.Z` tag) and updates the AUR.
- **Bump the version in the same commit as a user-visible change:** patch for fixes, minor for
  features, major for breaking config or key changes. Then run `cargo check` so `Cargo.lock`
  follows. Do not bump for docs, CI or refactors.
- **All changes go through pull requests.** `master` is protected: no direct pushes, CI must
  pass on Linux, macOS and Windows, squash merge only. Work on a branch, open the PR with
  `gh pr create`, then `gh pr merge --auto --squash` so it merges itself once CI is green.
  The squash commit title is what lands on `master`, so make the PR title a good commit title.
- **Commits:** plain messages. No `Co-Authored-By` or other Claude attribution.
- **Check before pushing:** `cargo test --workspace`, and for GUI changes
  `cd gui && npx svelte-check && npm run build`.
- **Screenshots** in `docs/screenshots/` are taken in a sandbox with a fake home, so nothing
  personal shows (Linux, needs `bwrap`): build release, then
  `docs/screenshots/sandbox.sh coxswain-gui /home/demo/projects/rocket` or
  `docs/screenshots/sandbox.sh alacritty -e coxswain`. `demo-home.sh` builds the demo files.

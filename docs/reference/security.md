[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Security: dependencies, advisories and updates

Coxswain is built from about 750 Rust crates and 200 npm packages. This page says how they are
chosen, how they are kept current, what happens when one of them gets a security advisory, and
how to report a problem in Coxswain itself. What Coxswain sends over the network is on
[Privacy](privacy.md); the update check of the app itself is on [Update checks](updates.md).

## Contents

- [What is checked, and when](#what-is-checked-and-when)
- [How dependencies are chosen](#how-dependencies-are-chosen)
- [How updates arrive](#how-updates-arrive)
- [The bundled viewers](#the-bundled-viewers)
- [Checking a download](#checking-a-download)
- [Reporting a problem](#reporting-a-problem)
- [Questions](#questions)

## What is checked, and when

| Check | Tool | When |
|---|---|---|
| Known vulnerabilities in Rust crates, unmaintained crates | `cargo deny check advisories` against the [RustSec database](https://rustsec.org) | Every pull request, every change on `master`, and every Monday |
| Licences of every crate | `cargo deny check licenses`, allow list in `deny.toml` | Same |
| Crates from unknown registries or git | `cargo deny check sources` | Same |
| Known vulnerabilities in npm packages | `npm audit --audit-level=high` in `gui/` | Same |
| Builds and tests on Linux, macOS and Windows, and the static musl build | `cargo test`, `cargo clippy`, `svelte-check`, Vite | Every pull request |

The weekly run matters: an advisory published after the last change would otherwise go unseen
until someone touched the code. A failing check blocks the pull request; a failing weekly run
shows on the Actions page and is fixed in the next release.

Three warnings are accepted and listed in `deny.toml` with the reason: crates that are
unmaintained but only used inside build-time macros or font parsing. Each is reviewed when its
parent (candle, tauri, pdf-extract) moves.

## How dependencies are chosen

- **Pure Rust where it exists.** The terminal app ships as a static musl binary, so anything
  compiled from C has to build for musl too. Compression (zlib-rs, bzip2, xz, zstd, 7z, AES) and
  TLS (rustls) are pure Rust; SQLite and Oniguruma are the two C libraries left, both built
  from bundled sources, never taken from the system.
- **Default features off where they pull in code Coxswain does not use.** The terminal app
  takes only the widgets it draws from ratatui.
- **One version of a crate where the choice is ours.** Transitive duplicates (two `sha2`, two
  `nom`) are tolerated when the crates that pull them have not moved yet.
- **Licences:** MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 and the like. Nothing under GPL or
  LGPL, which could not be distributed with Coxswain's MIT licence. `deny.toml` holds the list;
  a new licence fails the check until someone reads it.
- **Nothing phones home.** A dependency that would send anything on its own is not taken. See
  [Privacy](privacy.md).

## How updates arrive

- **Dependabot** opens a pull request every Monday for the Cargo workspace, for the frontend
  under `gui/` and for the GitHub Actions the builds use. Minor and patch updates come grouped
  as one pull request per ecosystem; a major update comes on its own, so the migration can be
  looked at.
- Every pull request runs the full CI, so an update that breaks a build or a test does not
  merge.
- A release follows the version in `Cargo.toml`: dependency updates that matter to users get a
  patch version and a changelog row, so the release notes say what moved.
- The GitHub Actions the builds run are pinned to a commit, with the version in a comment, so
  a tag moved under a published action cannot change what the release pipeline runs.

## The bundled viewers

Two renderers are not npm packages but files kept in the repository:

| Viewer | Where | Version | Notes |
|---|---|---|---|
| draw.io viewer | `gui/public/vendor/drawio/` | tag `v31.5.3` of [jgraph/drawio](https://github.com/jgraph/drawio), unmodified | Every path it would take from diagrams.net (shape libraries, styles, the image proxy, maths) is pointed at a local folder that does not exist, so it fetches nothing. Diagrams that use draw.io's extra shape libraries show those shapes as plain boxes. |
| SheetJS Community Edition | `gui/vendor/xlsx-0.20.3.tgz` | 0.20.3 | The last version on npm is 0.18, so the package is kept as a file and installed from it. |

Both are updated by hand; the README next to each says the tag it came from.

## Checking a download

Every terminal build on the [releases page](https://github.com/mwo-dk/coxswain/releases) comes
with a `.sha256` file:

```sh
sha256sum -c coxswain-terminal-v1.23.1-x86_64-unknown-linux-musl.tar.gz.sha256
```

The desktop builds come from `tauri-action` in the same pipeline; the AUR, crates.io and
Homebrew packages are generated from the same tag by the same workflow, see
[Install](../../install/INSTALL.md).

## Reporting a problem

A security problem in Coxswain itself: use GitHub's private
[security advisory](https://github.com/mwo-dk/coxswain/security/advisories/new) form on the
repository, so the fix can ship before the details are public. Anything else: an
[issue](https://github.com/mwo-dk/coxswain/issues).

## Questions

**Why does `cargo audit` or `cargo deny` still name a crate?**
Those three warnings are the accepted ones above, each with its reason in `deny.toml`. A new one
fails CI and is fixed, or added there with a reason, in the next pull request.

**Why not take every new major version at once?**
Majors come one at a time so their migration can be read. Minors and patches arrive grouped,
weekly. The aim is no legacy: a major that is safe to take is taken in the same week.

**Why is the `tokenizers` crate not on its latest version?**
The `candle` crates (search by meaning) ask for it, and one copy is better than two. It moves
with candle.

**Does the draw.io viewer talk to diagrams.net?**
No. Its paths are set to a local folder before it loads, so a diagram is drawn from what is in
the file. Shapes from draw.io's downloadable libraries show as boxes; see [Diagrams](../previews/diagrams.md).

**Can I check the dependency tree myself?**
`cargo deny check` in a checkout, with `cargo install cargo-deny --locked`, and `npm audit` in
`gui/`. `cargo tree -d` lists duplicate crates; `cargo tree -e normal -f "{p} {f}"` the features
each one is built with.

---
[← Previous: Where things are kept](where-things-are-kept.md) · [Next: Questions, collected →](../faq.md)

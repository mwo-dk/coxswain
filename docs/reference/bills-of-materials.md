[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Licences and bills of materials

Every release of Coxswain comes with what it is made of: an SBOM for each app, a CBOM of the
cryptography both use, and the licence text of every component inside. This page says what each
file holds, how the licences are checked, and how to make the files yourself. How dependencies
are chosen and kept current is on [Security](security.md).

## Contents

- [What each release carries](#what-each-release-carries)
- [The licences](#the-licences)
- [The cryptography](#the-cryptography)
- [Making them yourself](#making-them-yourself)
- [Questions](#questions)

## What each release carries

The files are on the release's **Assets** list on the
[releases page](https://github.com/mwo-dk/coxswain/releases), next to the apps:

| File | What it is |
|---|---|
| `coxswain-terminal-<version>.sbom.cdx.json` | The terminal app's SBOM: every Rust crate in it, with version, licence, hashes and how they depend on each other. CycloneDX 1.6 |
| `coxswain-desktop-<version>.sbom.cdx.json` | The desktop app's SBOM: its Rust crates and the npm packages of its web frontend |
| `coxswain-<version>.cbom.cdx.json` | The CBOM: every algorithm and protocol the apps use, the crate that provides it, why Coxswain uses it, and where in the source |
| `THIRD-PARTY-NOTICES.md` | The licence text of every component, and of the code that came without a package manager |

`THIRD-PARTY-NOTICES.md` also ships inside the apps: next to `LICENSE` in every terminal
archive, in the desktop installers (in `/usr/lib/Coxswain/` from the `.deb`, in the app's
resources on macOS and Windows), and in `/usr/share/licenses/coxswain-bin/` from the Arch
package `coxswain-bin` (built with `makepkg` from `packaging/aur/`).

The files are made from the release's own commit. The same commit always gives the same files,
byte for byte, so anyone can make them again and compare.

## The licences

Coxswain itself is MIT. What it is built from is checked on every pull request, every change on
`master` and every Monday:

| What | Checked by | Against |
|---|---|---|
| Rust crates | `cargo deny check licenses` | The allow list in `deny.toml`: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 and the like |
| npm packages in the desktop app | `tools/bom/bom.py licenses`, on the npm SBOM | The same list, plus EPL-2.0 for one package (below) |
| C code a crate compiles in, files kept in the repository | By hand, listed in `tools/bom/licenses.toml` | Their licence texts go into the notices |

Nothing is under GPL or LGPL. The cases that needed a closer look, and what was found:

| Component | Licence | Why it is fine |
|---|---|---|
| elkjs (mermaid's graph layout) | EPL-2.0 | Weak copyleft, file by file. Used unmodified; the notices carry its licence and its source is where it is published |
| Graphviz, inside `@viz-js/viz` | EPL-1.0 | The same, compiled to WebAssembly; its licence text is in `tools/bom/licenses/` and the notices |
| dompurify, jszip | MPL-2.0 or Apache-2.0; MIT or GPL-3.0-or-later | Dual-licensed: Coxswain takes Apache-2.0 and MIT |
| r-efi | MIT, Apache-2.0 or LGPL-2.1-or-later | The same: MIT |
| libdbus-sys (Linux desktop app) | MIT; carries D-Bus's source, AFL-2.1 or GPL-2.0-or-later | The source is never built: without its `vendored` feature the crate links the system's libdbus, so none of it ships |
| Oniguruma, SQLite, ring's C code | BSD-2-Clause, public domain, ISC and Apache-2.0 | Compiled in from bundled sources; their texts are in the notices, beside the crates' own |
| The BOM viewer's rating catalogue and the code ported from cipherscape | MIT | cipherscape is Jimmy Tønners's own project, and he contributes this material under Coxswain's MIT licence; see `crates/coxswain-core/src/bom/README.md` |
| duck, khroma | BSD-2-Clause, MIT | Their `package.json` says "BSD" or nothing; the licence files say what is recorded in `tools/bom/licenses.toml` |

## The cryptography

The CBOM lists what Coxswain uses cryptography for, and rates it with Coxswain's own
[BOM viewer](../previews/bom.md). Open the release's `.cbom.cdx.json` in either app to see the tree
and where each algorithm is used.

| For | Algorithms | Notes |
|---|---|---|
| HTTPS: the update check, model downloads, model servers | TLS 1.3 and 1.2 with AES-GCM and ChaCha20-Poly1305, X25519 and the P-256 and P-384 curves; certificates checked against the system's trust store | rustls with ring. No post-quantum key exchange yet: ring does not offer one |
| Packing and opening archives with a password | 7z: AES-256-CBC, its key from SHA-256 in 2^19 rounds. ZIP: AES-256-CTR with HMAC-SHA1 and PBKDF2 (WinZip AE-2) | ZIP archives others locked with 128-bit AES, or the old ZipCrypto, can be opened; Coxswain never locks one with them |
| Reading encrypted PDFs, for search inside files | RC4, MD5, AES-128-CBC, AES-256-CBC | Read only: old PDFs use RC4 and MD5, which are broken, and the CBOM says so |
| Checking a downloaded model | SHA-256 | |
| Finding duplicate files | BLAKE3 | Not for security: it tells files apart |

`tools/bom/crypto.toml` is kept by hand, because no scanner reads Rust well enough yet. The
build of the CBOM checks it: a crate that does cryptography and is not described, a crate that
is gone, or a `found` line that no longer matches its file all fail it, in CI.

## Making them yourself

```sh
cargo install --locked cargo-cyclonedx@0.5.7 cargo-about@0.8.4
(cd gui && npm ci)
tools/bom/generate.sh            # into target/bom
```

It needs Rust, Node.js (for `npx @cyclonedx/cyclonedx-npm`, fetched at a fixed version) and
Python 3. It takes under a minute once the crates are downloaded, and ends by reading every file
back with Coxswain's own BOM reader. [tools/bom/README.md](../../tools/bom/README.md) has the
details for maintainers.

## Questions

#### Why are RC4 and MD5 in the CBOM, rated broken?
Because Coxswain uses them, to read the text of old encrypted PDFs for search inside files. That
is decryption of what someone else encrypted long ago, never protection of anything new. The
CBOM records the purpose with each algorithm, so a reader can tell the two apart.

#### Why are BLAKE3 and ZipCrypto rated unknown?
The rating catalogue does not list them. BLAKE3 is a fast, modern hash that Coxswain uses only to
find duplicate files; ZipCrypto is the old ZIP cipher, which Coxswain only opens and never writes.

#### Why are there two SBOMs and not one?
Because there are two products. The terminal app has no web frontend, and the desktop app has
crates the terminal app does not, such as Tauri. An SBOM per download says exactly what that
download holds.

#### Where are the licences of the apps I installed?
In `THIRD-PARTY-NOTICES.md`, next to `LICENSE`: in the terminal app's archive, in the desktop
app's install folder, or in `/usr/share/licenses/` from the Arch package. Every release also has it on its
Assets list.

#### A new dependency fails the "bills of materials" check. What now?
Read the message. A crypto crate needs a `[[library]]` in `tools/bom/crypto.toml`; an npm
licence outside the list needs reading, then an entry in `tools/bom/licenses.toml` with the
reason, or a different package.

---
[← Previous: Security](security.md) · [Next: Performance →](performance.md)

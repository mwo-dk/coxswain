# Bills of materials and licence notices

`generate.sh` makes what every release carries (users' side:
[Licences and bills of materials](../../docs/reference/bills-of-materials.md)):

| File | Made from |
|---|---|
| `coxswain-terminal-<version>.sbom.cdx.json` | `cargo cyclonedx` for the `coxswain` crate, every target |
| `coxswain-desktop-<version>.sbom.cdx.json` | `cargo cyclonedx` for `coxswain-gui`, and `cyclonedx-npm` for `gui/` without dev dependencies |
| `coxswain-<version>.cbom.cdx.json` | `crypto.toml`, checked against `cargo metadata` and the source |
| `THIRD-PARTY-NOTICES.md` | `cargo about` (Rust), the npm packages' licence files, and the `[[bundled]]` entries of `licenses.toml` |

```sh
cargo install --locked cargo-cyclonedx@0.5.7 cargo-about@0.8.4
(cd gui && npm ci)
tools/bom/generate.sh [OUT_DIR]        # default target/bom
```

`cyclonedx-npm` runs through `npx` at the version pinned in `generate.sh`, so the frontend's
lockfile does not carry it. All output is CycloneDX 1.6 JSON (cargo-cyclonedx writes 1.5, and
`bom.py` lifts it). Timestamps are the commit's and serial numbers derive from the version, so a
commit always gives the same bytes.

## Where it runs

- **CI:** the *bills of materials* job of the Dependencies workflow (`audit.yml`), on every pull
  request, on `master` and weekly, so a failure shows before a release.
- **Release:** the `boms` job of `release.yml` uploads the four files to the draft release. The
  terminal archives and the desktop installers then take `THIRD-PARTY-NOTICES.md` from there;
  the installers bundle it through a `--config` override, so other builds need no such file.
  The release is published only once `boms` has passed.

## What it checks, and what to do when it fails

| Message | Do |
|---|---|
| `X is in the build but not in crypto.toml` | A dependency brought in a crate from `watch`. Find out what it is used for (`cargo tree -i X`) and add a `[[library]]`, with `[[asset]]`s for what Coxswain really uses |
| `crypto.toml describes X, which is no longer in the build` | Remove its `[[library]]`, and assets nothing else provides |
| `"…" is no longer in <file>` | The code moved: update the `found` pattern to a line that still shows the use |
| `npm licences: … is not on the allowed list` | Read the package's licence. If it is acceptable, add it to `[npm].also_allow` with the reason; if the package metadata is wrong, add an override keyed by name and version |
| `bom_check: problem: …` | Coxswain's own reader found something wrong in a generated file, such as a dangling reference. Names it does not know (BLAKE3, ZipCrypto) are reported but do not fail |

`cargo deny` checks the Rust crates' licences on its own (`deny.toml`); `about.toml` accepts the
same list.

## Kept by hand

- **`crypto.toml`:** no scanner reads Rust's cryptography well enough yet. Each asset says what
  it is (CycloneDX `cryptoProperties`), why Coxswain uses it (`purpose`) and where (`found`,
  turned into file and line evidence). A protocol lists its cipher suites and the algorithms it
  uses.
- **`licenses.toml`:** components that come without a package manager (the draw.io viewer,
  Graphviz inside `@viz-js/viz`) and C code a crate compiles in under its own licence
  (Oniguruma, ring's), whose texts cargo-about does not report.
- **`licenses/EPL-1.0.txt`:** Graphviz's licence, which no package on disk carries; the SPDX
  text.

## Known noise

cargo-about logs errors about GPL-2.0 while scanning `libdbus-sys`, which carries D-Bus's C
source. That source is built only with the crate's `vendored` feature, which Coxswain does not
use (the system's libdbus is linked), so nothing of it ships. `generate.sh` runs cargo-about with
`-L off` for that reason; its real failures still stop the script.

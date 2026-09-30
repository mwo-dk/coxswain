# Design: a CBOM viewer (SBOMs later)

Status: **draft**, for review before any code. Branch `feature/cbom`.

## What and why

A CycloneDX **CBOM** (Cryptography Bill of Materials) lists the cryptography an application
uses: algorithms, keys, certificates, protocols, and where in the code each one is found.
Coxswain should show one as a readable picture instead of a JSON tree of a few thousand
lines. You select `app.cdx.json` and see which crypto is weak, where it is, and what changed
since the last scan.

The views come from the sibling project **cipherscape** (a browser CBOM visualizer, same
author). This document says which parts to port, how they fit coxswain's two apps, and in
what order to build them.

**First iteration**, in this order of importance:

1. **Tree:** the BOM's structure, each node colored by its worst crypto.
2. **Sunburst:** the same tree as rings, which shows where the risk sits.
3. **Filters and search:** by status, asset kind and primitive family, plus a text search.
4. **Compare:** two versions of a BOM: new risks, fixed risks, added and removed assets.

**Later, not in this design's steps:** SBOMs proper (licenses, vulnerabilities, component
graph), the relationship graph (cipherscape's Sigma view), the 3D galaxy, the Quantum
Countdown year slider, blast radius, the action plan, PNG/PDF export, SPDX.

## What cipherscape has, and what we take

Cipherscape is React + TypeScript. All its logic sits in a DOM-free core
(`web/src/core/{ingest,model,policy,analysis}`, about 1,500 lines), and that is what we
port. We port it **to Rust in `coxswain-core`**, so the terminal app and the desktop app share
one parser and one set of ratings.

| cipherscape | Lines | Coxswain | Notes |
|---|---|---|---|
| `core/ingest/cyclonedx.ts`, `cyclonedx-xml.ts` | 250 + ~150 | `bom::ingest` | JSON with serde_json, XML with quick-xml (both already in core) |
| `core/model/types.ts`, `hierarchy.ts` | 220 | `bom::model`, `bom::tree` | Same node and edge kinds, same three hierarchy modes |
| `core/model/status.ts`, `analysis/assess.ts` | | `bom::status` | Same status scale, reasons and roll-up |
| `core/policy/{resolve,alias,evaluate}.ts` + `policy/algorithms.yaml` | 400 + catalog | `bom::policy` + vendored `catalog.json` | See [Ratings](#ratings) |
| `core/analysis/filter.ts` | 140 | GUI: `bom.js`; TUI: `bom::filter` | A mask over nodes |
| `core/analysis/diff.ts` | 170 | `bom::diff` | Identity by what and where, never by bom-ref |
| `features/tree/TreeView.tsx` | | GUI `BomView.svelte` tree; TUI dialog | The keyboard model carries over as is |
| `render/sunburst/SunburstView.tsx` (Canvas + D3) | 300 | GUI: SVG, no library; TUI: half-block cells | See [Sunburst](#sunburst) |
| `features/filters/*`, `features/diff/*` | | Chips and a compare bar in the view | |
| `features/inspector/*` + `explain.ts` | | A small **details** box under the tree | Header, *Why* (from reasons), *Found in* (occurrences), raw fields. No breach simulation |

We do **not** take the store, Sigma, three.js, the Web Worker setup or the Go server.

## Data model (Rust, `crates/coxswain-core/src/bom/`)

We name the module **`bom`**, not `cbom`. Crypto is one layer of a BOM, and an SBOM will
reuse the same nodes, tree and diff later.

```rust
pub enum NodeKind { Application, Group, Component, Algorithm, Certificate, Protocol, Material }
pub enum EdgeKind { DependsOn, Provides, Contains, SignedWith, HasKey, UsesAlgorithm, Uses, OccursIn }

pub struct Node {
    pub kind: NodeKind,
    pub label: String,
    pub bom_ref: Option<String>,
    pub version: Option<String>,
    pub purl: Option<String>,
    pub crypto: Option<Crypto>,          // algorithm / certificate / protocol / material facts
    pub occurrences: Vec<Occurrence>,    // location, line, offset, additionalContext
    pub group: Option<GroupInfo>,        // synthetic directory / file / per-kind groups
}
pub struct Edge { pub from: u32, pub to: u32, pub kind: EdgeKind }
pub struct Bom {
    pub source: Source,                  // format (json|xml), specVersion, serial, timestamp
    pub nodes: Vec<Node>,                // nodes[0] is the root
    pub edges: Vec<Edge>,
    pub issues: Vec<Issue>,              // dangling-ref, duplicate-bom-ref, unsupported-spec-version, ...
}
pub struct Tree { pub parent: Vec<u32>, pub children: Vec<Vec<u32>>, pub order: Vec<u32>, pub mode: TreeMode }
pub enum TreeMode { Dependencies, Files, Flat }
```

We keep cipherscape's ingest rules. The root is `metadata.component`. If that is missing, as
in CBOMkit output, a root is made from `gitUrl` or the file name. Nested components give
`Contains` edges. A certificate's refs give `SignedWith` and `HasKey`. Cipher suites and
`algorithmRef` give `UsesAlgorithm`. We accept spec versions 1.6 and 1.7. Other versions
load, with an `unsupported-spec-version` warning.

Parsing is **lenient**. Only three things refuse a file: it is not CycloneDX, it is invalid
JSON or XML, or the XML has a DOCTYPE (XXE and entity-expansion guard). Anything else becomes
an `Issue`, which the view lists.

The tree follows `hierarchy.ts`. **Dependencies** mode is a BFS over
contains/dependsOn/provides. **Files** mode groups assets by the source path of their first
occurrence and collapses single-child directory chains. This is the best mode for CBOMkit
output, which has no dependencies. **Flat** mode groups by kind. The best available mode is
the default, and the view offers a switch.

## Ratings

The status scale is cipherscape's, in the same order and colors:

| Status | Color | Meaning |
|---|---|---|
| safe | green, with a quantum-safe mark | Approved, and not vulnerable to a quantum computer |
| acceptable | green | Approved now |
| unknown | grey | Could not be resolved, or a parameter is missing. **Never shown as green** |
| deprecated | yellow | Allowed, but on the way out |
| disallowed | red | Not permitted by the profile, e.g. an expired certificate |
| broken | red | Practically broken (MD5, SHA-1, DES, RC4, RSA-1024) |
| not rated | muted | KDFs, DRBGs, padding: no rule applies |

A node's display status is the worst of its own status and everything below it (the
roll-up). We take the lifecycle rule too: an expired certificate is disallowed, and one that
expires within 90 days is deprecated. This matches the 30-day warning the certificate preview
already shows.

**The catalog.** We vendor cipherscape's compiled catalog (`catalog.gen.json`, 32 KB, built
from `policy/algorithms.yaml`) as `crates/coxswain-core/src/bom/catalog.json`, loaded with
`include_str!`. The first line of `bom/README` records the cipherscape commit it came from.
This keeps one source of truth, and core needs no YAML crate. A small script,
`bom/sync-catalog.sh`, rebuilds and copies it.

We rate with the **NIST profile at the current year**. There is no profile picker or year
slider in this iteration; the evaluator takes both as arguments, so they are cheap to add
later.

> The catalog is an **unreviewed seed** (`last_reviewed: null`). The view says so in a
> footnote next to the legend, and the docs page does too. A rating is a hint, not an audit.

**Name resolution** is ported from `resolve.ts`. Three signals vote: OID, `algorithmFamily`
and the parsed name. On a tie the name wins, because OIDs in real files are often wrong, and
a disagreement adds an `oid-mismatch` issue. A composite such as `SHA256withRSA` is rated as
its worst part. Parameters come from name digits, the OID, `parameterSetIdentifier` and the
curve table. `policy/golden.yaml` becomes a Rust test table, so both projects rate alike.

## Detection

Today `previewKind` (`gui/src/lib.js:227`) goes by extension, and `.json` is `data`. A BOM is
recognized by:

1. **Name:** `*.cdx.json`, `*.cdx.xml`, `*.cbom.json`, `bom.json`, `bom.xml`.
2. **Content,** for any other `.json` or `.xml` under 64 MB: the first 8 KB contains
   `"bomFormat"` with `"CycloneDX"`, or `xmlns="http://cyclonedx.org/schema/bom/`.

Content sniffing needs the file's first bytes. In the GUI this is done in the text loader
that already calls `read_text`: when a `data` or `text` file turns out to be a BOM, the kind
is upgraded to `bom`. In the TUI, core offers `bom::sniff(path) -> bool`, which F3 calls.

A CycloneDX file with **no crypto assets** (a plain SBOM) opens in the same view. It shows
its components tree with every node *not rated*, and a line saying SBOM views are coming.
That is the place SBOM support grows into.

## Desktop app

**Backend** (`gui/src-tauri/src/preview.rs`, next to `cert_info`):

- `bom_info(path) -> BomView`: nodes (compact: kind, label, status, parent, and details on
  demand), tree order, counts per status/kind/family, and issues. It runs on a blocking
  thread. The largest cipherscape sample (50k nodes, 25 MB) must come back in under a second.
- `bom_node(path, idx) -> NodeDetails`: the details box, fetched when a node is selected, so
  `bom_info` stays small. The parsed `Bom` is cached per path and mtime (a single entry).
- `bom_diff(old, new) -> BomDiff`: a change code per node of `new` (unchanged, added,
  improved, worsened), the removed assets, and counts.

**Frontend:** a new `gui/src/BomView.svelte` takes `previewKind === "bom"`. Preview.svelte
gets only one `{:else if}`, because it is already 1,000 lines. Filtering and the sunburst
geometry live in `gui/src/bom.js`.

```
┌ app.cdx.json ─────────────────────── [Tree|Sunburst|Source] [⤢] ┐
│ ● 12 broken  ● 3 disallowed  ● 20 deprecated  ● 4 unknown  ● 88  │  ← status chips (toggle)
│ Algorithm  Certificate  Protocol  Key   · hash  signature  kem …  │  ← kind / family chips
│ [/ search……………………]   dim ○ hide ●         Compare with other pane │
├──────────────────────────────────────────────────────────────────┤
│ ▾ ● keycloak                                                     │
│   ▾ ● services/src/main/java/org/keycloak                        │
│     ▾ ● crypto/                                                  │
│         ● SHA1withRSA           broken    RsaSigner.java:88      │
│         ● AES-128-GCM           acceptable                       │
├──────────────────────────────────────────────────────────────────┤
│ SHA1withRSA · signature · broken                                 │
│ Why: SHA-1 is broken (collisions). Rated as its worst part.      │
│ Found in: RsaSigner.java:88, JwsBuilder.java:41 (Enter opens)    │
└──────────────────────────────────────────────────────────────────┘
```

- **Switch:** `Tree | Sunburst | Source`, following the existing Tree/Source switch. The
  choice sticks, as all preview switches do (`ui.previewSource`, a new value `"sunburst"`).
- **Tree:** cipherscape's keyboard model: ↑/↓, →/← to expand, collapse or go to the parent,
  Home/End, and Enter on an occurrence. The first two levels are open. Each node shows at most
  500 children, then a *show more* row. Every row shows a status dot and, for assets, the
  status word and the first occurrence.
- **Found in:** an occurrence is a path relative to the scanned repository. If that path
  exists next to the BOM (the usual layout, `repo/cbom.json`), Enter moves the pane's cursor
  to the file. This is what a file manager adds over cipherscape: you go from a red node to
  the source in one key.
- **Filters:** chips toggle statuses, kinds and families, each with its count. The search
  box (`/`) matches labels, OIDs and occurrence paths. *Dim* keeps the structure and greys
  what doesn't match, and *hide* removes it. Ancestors of a match always stay visible. Filters
  apply to the tree and the sunburst alike.
- **⤢** opens the same component full-window, as the Duplicates window does
  (`ui.modal = {kind: "bom", path}`), for large BOMs and the sunburst.
- **Compare:** coxswain has two panes, so *Compare with other pane* compares this BOM with
  the file under the cursor in the other pane. That file is the "before", and this one is the
  "after". A compare bar then shows **new risks · fixed · added · removed**, each clickable
  as a filter. Tree rows get a change mark (＋ added, ▲ worsened, ▼ improved), and removed
  assets are listed under the tree, because they have no row. If the other file is not a BOM,
  the button is disabled with a tooltip saying why.
- All strings from the file are shown as text, never as HTML.

### Sunburst

- It is drawn as **SVG** paths, with no chart library: about 150 lines in `bom.js`. The
  preview has no D3, and the few arcs we need don't justify it. SVG gives us hit testing,
  focus and theme colors through CSS variables for free.
- The rings follow the tree. The root is in the middle, and an arc's angle is proportional to
  the number of leaves under it. Arcs are colored by roll-up status.
- **Clicking an arc** zooms into it, and the centre or the breadcrumbs zoom out. Hover shows
  label, status and counts. Clicking also selects the node, so the details box and the tree
  follow.
- **Big BOMs:** arcs narrower than 0.5° are merged into one "n more" arc per parent, and at
  most 6 rings are drawn below the zoom root. This keeps a 50k-node BOM at a few thousand
  paths.

## Terminal app

The TUI has no preview pane. F3 runs `viewer`, else `$PAGER`, else `less`. So the terminal
viewer is a new full-screen dialog, modelled on the scrollable `Dialog::Help`
(`crates/coxswain/src/main.rs:154`).

- **F3 on a BOM** opens `Dialog::Bom`. Inside, **F3 again** (or `s`) opens the source in the
  pager as before, so nothing is lost. A config switch, `bom_viewer = false`, keeps the old
  behaviour.
- **Tree:** the same keys as the GUI. The status is shown as a colored `●` from the theme's
  palette, with the status word next to it, so colors are never the only signal.
- **Filters:** `1`–`6` toggle the statuses, `k` cycles the kind, `/` searches, and `h`
  toggles dim or hide.
- **Sunburst:** `Tab` switches to it. It is drawn with `▀` half-blocks: each cell holds two
  pixels, with foreground and background color, and each pixel is mapped to an arc by angle
  and radius. It is coarse but true to shape, and it needs no font support beyond `▀`. Enter
  zooms into the arc under a movable cursor, and Backspace zooms out.
- **Compare:** `c` compares with the file under the other panel's cursor, as in the GUI.
- **Details:** the bottom third of the dialog.

## Safety and limits

- The file is read-only and nothing is executed. XML with a DOCTYPE is refused.
- The size limit is 64 MB. Above it we fall back to the ordinary JSON or text preview, with a
  note.
- Tree rows are paged, and the sunburst merges thin arcs (see above). No view walks all nodes
  on every keystroke except the filter mask, which is O(n).
- Nothing leaves the machine. No network is used.

## Tests and fixtures

- The fixtures live in `crates/coxswain-core/src/bom/testdata/`, each with its upstream
  LICENSE file:
  - the CycloneDX `bom-examples` (CC0): algorithm, certificate, key, protocol,
    with-dependencies
  - CBOMkit `keycloak.cdx.json` (Apache-2.0), a real flat scanner output that keeps its
    errors (wrong OIDs, dangling refs)
  - a spec `cryptography-full-1.7` JSON+XML pair (Apache-2.0), which tests that both formats
    parse to the same `Bom`
  
  This departs from the repo's habit of building fixtures in a temp dir, because real files
  are the point here. The files total under 150 KB.
- The tests:
  - **Ingest:** every fixture parses, with the expected node, edge and issue counts, and each
    JSON/XML pair gives the same model.
  - **Ratings:** the cases in `policy/golden.yaml` become a table test.
  - **Tree:** each mode's shape on the with-dependencies and keycloak fixtures.
  - **Diff:** keycloak against a copy with one asset removed, one added and one weakened.
  - **Sunburst geometry:** angles sum to 360°, and thin arcs merge.
- The GUI checks are `npx svelte-check && npm run build`, and the Tauri command tests go in
  `preview.rs`, as for `cert_info`.
- Screenshots: `demo-home.sh` gets a `projects/rocket/cbom.cdx.json` generated to match the
  demo project's files, so *Found in* resolves in the screenshots.

## Strings and docs

- New catalogue keys go under `bom.*`: the switch labels, status names, kind and family names,
  filter labels, compare labels, the unreviewed-catalog footnote and the SBOM notice, about
  40 in all. en-GB is required, and every full locale gets them in the same PR, as is the
  convention.
- `docs/previews.md` gets a **CBOM** row under Data and a screenshot. `docs/bom.md` is a
  feature page in the style of `duplicates.md`: using it, what the colors mean, where
  ratings come from, compare.

## Steps

Each step is one PR to `master`, ends with its acceptance command green, and bumps the
version only when it is user-visible.

1. **Core model and ingest** (`bom::{model, ingest, tree}`, fixtures): JSON and XML, issues,
   the three tree modes. Acceptance: `cargo test -p coxswain-core bom`.
2. **Core ratings** (`bom::{policy, status}`, vendored catalog, golden table), plus
   `bom::sniff`. Acceptance: `cargo test -p coxswain-core bom`.
3. **Core diff** (`bom::diff`). Acceptance: `cargo test -p coxswain-core bom::diff`.
4. **GUI tree, filters and details** (`bom_info`, `bom_node`, `BomView.svelte`, detection,
   Found in, ⤢ window). This is the first user-visible step and bumps the **minor** version.
   Acceptance: `cargo test --workspace`, `cd gui && npx svelte-check && npm run build`, and a
   manual run on keycloak.
5. **GUI sunburst.** Acceptance: as step 4, plus the 50k synthetic BOM staying smooth.
6. **GUI compare** (`bom_diff`, compare bar, change marks). Acceptance: as step 4.
7. **TUI viewer** (`Dialog::Bom`: tree, filters, sunburst, compare). Acceptance:
   `cargo test --workspace` and a manual run in the sandbox.
8. **Docs, strings in every locale, screenshots.** Acceptance: the i18n completeness test.

## Open questions

1. **One PR or several?** The steps are cut to be separate PRs. Steps 1–3 are invisible on
   their own and could go to `master` early, or everything could stay on `feature/cbom` until
   step 6.
2. **Catalog ownership:** is vendoring cipherscape's compiled catalog right, or should the
   catalog move to a small shared repo or crate that both projects use?
3. **TUI sunburst:** is a half-block sunburst worth having in a terminal, or should the TUI
   get a per-ring summary bar instead?
4. **F3 in the TUI:** should a BOM open the built-in viewer by default (proposed), or only on
   a new key, leaving F3 alone?
5. **Year and profile:** should the NIST-at-this-year default be a setting from the start?

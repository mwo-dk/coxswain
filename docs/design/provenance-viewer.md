# Design: a build provenance viewer (SLSA, in-toto)

Status: **agreed**; being built. Branch `feature/provenance`.

## What and why

A **provenance** file says how a piece of software was built. It names the files that came out
of the build (the *subjects*, each with a digest), the builder that made them, and the inputs
the builder used: a source repository at a commit, a workflow, parameters and dependencies.
SLSA defines the format, and in-toto defines the statement that carries it. GitHub Actions
(`actions/attest-build-provenance`), slsa-github-generator, npm, PyPI and Google Cloud Build
all write it.

As a file, it is unreadable. The statement is base64 inside a DSSE envelope, and the envelope
sits inside a Sigstore bundle with a certificate and a transparency-log entry. A
`*.intoto.jsonl` file often holds several of these. People open it to find out three things:
*was this file built from that source, by whom, and is the file I have the file that was
built?*

Coxswain should answer this in the preview pane, without the user needing to decode anything.
A file manager can also do something a web viewer cannot: it can **check the claims against
the disk**. It can hash the release files next to the provenance and say which ones match. It
can find the source commit in a checkout you already have. And it can compare two builds.

**First iteration**, in this order of importance:

1. **Read every common form:** a bare statement, a DSSE envelope, a Sigstore bundle, and JSON
   Lines of any of these. Read SLSA provenance v1 and v0.2, and show other predicates
   generically.
2. **Flow:** inputs → build → outputs, as one picture, with the signer's identity at the top.
3. **Checks on disk:** do the subjects match the files here, and is the source commit in a
   checkout here?
4. **Compare:** two provenance files, to see what differs between two builds.

**Not in this design's steps:** verifying signatures (see [Verification](#verification): not in
this version), SLSA level ratings, PEP 740 attestation files,
fetching attestations from a registry or from GitHub and a Verify button.

## Formats

| Form | How it is recognised | What it holds |
|---|---|---|
| in-toto Statement | `"_type": "https://in-toto.io/Statement/v1"` (or `/v0.1`) | `subject[]`, `predicateType`, `predicate` |
| DSSE envelope | `"payloadType": "application/vnd.in-toto+json"` | `payload` (the statement, base64), `signatures[]` (`keyid`, `sig`) |
| Sigstore bundle | `"mediaType": "application/vnd.dev.sigstore.bundle…"` (v0.1 to v0.3) | `dsseEnvelope`, and `verificationMaterial`: `certificate` or `x509CertificateChain` or `publicKey`, plus `tlogEntries[]` |
| JSON Lines | One of the above per line | `slsa-github-generator`'s `*.intoto.jsonl`; `gh attestation download`'s `sha256:<hex>.jsonl` |

The predicates:

| `predicateType` | Shown as |
|---|---|
| `https://slsa.dev/provenance/v1` | The flow |
| `https://slsa.dev/provenance/v0.2` | The flow, after it is mapped to v1 (below) |
| `https://slsa.dev/verification_summary/v1` (VSA) | A result card: verifier, resource, policy, *PASSED* or *FAILED*, levels it vouches for |
| `https://cyclonedx.org/bom` | Subjects, plus **Open as BOM**, which hands the predicate to the BOM viewer ([bom-viewer.md](bom-viewer.md)) |
| Anything else (SPDX, vulns, test results, release) | Subjects, and the predicate as a JSON tree |

**v0.2 mapped to v1.** `materials` becomes `resolvedDependencies`, and
`invocation.configSource` becomes the first dependency, marked *config source*, with its
`entryPoint`. `invocation.parameters` becomes the external parameters, and
`invocation.environment` and `buildConfig` become the internal ones.
`metadata.buildStartedOn` and `buildFinishedOn` become the start and finish times.
`completeness` and `reproducible` are kept and shown as facts. Both versions then reach the
views as one model, and compare works across them.

## Data model (Rust, `crates/coxswain-core/src/provenance/`)

We name the module, the preview kind, the dialog, the strings and the docs page
**`provenance`**, not `slsa`: SLSA is the main predicate, but the same reader carries every
in-toto predicate, and it is provenance people come for.

```rust
pub struct Attestations {
    pub source: Source,               // path, size, mtime, form of the first entry
    pub entries: Vec<Entry>,          // one per statement; a .jsonl gives several
    pub issues: Vec<Issue>,
}
pub struct Entry {
    pub line: Option<usize>,          // in a .jsonl
    pub wrapping: Wrapping,           // Statement | Envelope | Bundle { media_type }
    pub signatures: Vec<Signature>,   // keyid and the signature's length, never "valid"
    pub signer: Option<Signer>,       // from a Fulcio certificate
    pub log: Vec<LogEntry>,           // Rekor: index, integrated time, kind, proof or promise present
    pub statement: Statement,
}
pub struct Statement {
    pub subjects: Vec<Resource>,
    pub predicate_type: String,
    pub predicate: Predicate,         // Provenance | Vsa | Bom(Value) | Other(Value)
    pub raw: serde_json::Value,       // the decoded statement, for the Statement view
}
pub struct Provenance {
    pub version: SlsaVersion,         // V1 | V0_2
    pub build_type: String,
    pub builder: Builder,             // id, version map, builderDependencies
    pub external: serde_json::Value,  // externalParameters
    pub internal: serde_json::Value,  // internalParameters
    pub dependencies: Vec<Resource>,  // resolvedDependencies
    pub invocation: Option<String>,
    pub started: Option<String>,
    pub finished: Option<String>,
    pub byproducts: Vec<Resource>,
    pub completeness: Option<Completeness>,
    pub reproducible: Option<bool>,
}
pub struct Resource {                 // in-toto ResourceDescriptor
    pub name: Option<String>,
    pub uri: Option<String>,
    pub digest: BTreeMap<String, String>, // sha256, sha512, gitCommit, …
    pub download: Option<String>,
    pub media_type: Option<String>,
    pub annotations: serde_json::Value,
}
pub struct Signer {
    pub identity: String,             // the SAN: a workflow URI or an e-mail address
    pub issuer: Option<String>,       // the OIDC issuer, e.g. token.actions.githubusercontent.com
    pub valid: (i64, i64),            // a Fulcio certificate lives about ten minutes
    pub claims: Vec<(Claim, String)>, // the Fulcio extensions, below
}
```

**The signer's claims** come from the Fulcio certificate's extensions under
`1.3.6.1.4.1.57264.1`. These are the source repository, ref and commit, the build config
(workflow) URI, the trigger, the runner environment, the run's URI and the repository's
visibility (`.8` to `.22`). The legacy `.1` to `.6` are read when the newer ones are missing.
For this, `x509-parser` moves into `coxswain-core`, so that the terminal app has it too. It is
pure Rust, builds for musl, and only parses: its `verify` feature stays off.
`tools/bom/crypto.toml` therefore needs no new library entry. The desktop app's `cert_info`
can then use core's parser as well.

Parsing is **lenient**, as for BOMs. Only three things refuse a file: it is none of the forms
above, it is not valid JSON, or it is over the size limit. Everything else becomes an
`Issue`, and the rest still shows. Examples are a payload that is not valid base64, a payload
type other than in-toto, a subject without a digest, an unknown digest algorithm, a
certificate that cannot be parsed, and the subjects of a bundle's statement disagreeing with
those in another line of the same file.

## The checks on disk

These are the parts a file manager adds. They are all local, and each one says plainly what it
proves.

### Do the outputs match?

For each subject with a `sha256` or `sha512` digest (`sha2` is already in core):

1. **Find the file.** Coxswain looks for the subject's `name` relative to the provenance's
   folder, then for its last path part in that folder, then in the folder of the other pane.
   The rule is `bom::view::on_disk`, moved to a shared place: never a path that climbs out
   with `..`, and never an absolute path.
2. **Hash it** in the background, cancelled when the cursor moves, as `dupes::hash_file`
   does. Files up to 256 MB are hashed when the provenance is shown, and larger ones on
   **Check** (Enter on the row). Results are kept per path, size and mtime.
3. **Mark it:** ✓ *matches*, ✗ *differs* (with both digests in the details), ? *not here*, or
   – *cannot check* (a container image, a digest algorithm we do not hash, or no digest).

The details box says what a ✓ means: *this file is the one the provenance names*. It does not
mean that the provenance is genuine. That needs [verification](#verification).

### Is the source commit here?

A dependency such as `git+https://github.com/owner/repo@refs/tags/v2.1.0` with a `gitCommit`
digest is a source. Coxswain looks for a checkout of that repository in the provenance's
folder or one above it, and in the other pane's folder. A checkout matches when one of its
remotes points to the same host and path. Then it runs `git cat-file -e <sha>^{commit}`, and
`git merge-base --is-ancestor` and `git rev-list --count` against HEAD, using
`git::command`. The result reads *in your checkout, 2 commits behind HEAD*, *in your checkout,
not on this branch*, or *not in your checkout*. Coxswain **never fetches**: a missing commit
is reported, not looked up.

## Verification

Showing a provenance is not verifying it. Verifying means checking the DSSE signature against
the certificate, the certificate against Fulcio's root, and the Rekor inclusion proof against
a log key, all from a trusted root that rotates and is distributed through Sigstore's TUF
repository. Doing that inside Coxswain would mean ECDSA and Ed25519 crates (new entries in
`crypto.toml`), and either network access or a root that goes stale.

So the **first iteration verifies nothing**, and says so. The header carries *Not verified:
this is what the file claims* in every view, next to the signer. No green mark is ever given
to the provenance as a whole, only to the on-disk checks, which are true facts about the disk.

There is **no Verify button in this version**, not even one that runs an installed verifier.
A later version may add one, opt-in, as an engine button that runs `slsa-verifier`, `cosign`
or `gh attestation verify`. Those contact Sigstore, so it would be off until turned on in
Settings, which would name where the data goes. Until then, the docs page says how to verify
with those tools by hand.

## Detection

1. **Name:** `*.intoto.jsonl`, `*.intoto.json`, `*.sigstore.json`, `*.sigstore`,
   `*.dsse.json`, `*.provenance.json`, `provenance.json`.
2. **Content,** for any other `.json`, `.jsonl` or `.ndjson` up to the size limit: the first
   8 KB contain `"payloadType"` with `application/vnd.in-toto+json`, or `"_type"` with
   `https://in-toto.io/Statement/`, or `"mediaType"` with
   `application/vnd.dev.sigstore.bundle`. This catches `gh attestation download`'s
   `sha256:<hex>.jsonl`.

Core offers `provenance::sniff(path)`, `sniff_name` and `sniff_head`, as `bom` does. In the
desktop app, `Preview.svelte`'s `sniffedBom` becomes `sniffed = { path, kind }`, so the text
loader can upgrade a `data` or `jsonl` file to `bom` or to `provenance`. The BOM sniff runs first:
a CycloneDX file is never an attestation. A `.jsonl` file whose first line is not an
attestation stays a JSON Lines table.

## Desktop app

**Backend** (`gui/src-tauri/src/provenance.rs`, next to `bom.rs`):

- `provenance_info(path) -> ProvenanceView`: entries with their subjects, the flow's columns, the
  signer, log entries and issues. Parameters and the decoded statement come on demand. It runs
  on a blocking thread, and the parsed file is cached per path and mtime.
- `provenance_check(path, entry) -> Vec<SubjectCheck>`: the on-disk subject checks, streamed as
  each hash finishes, and cancellable.
- `provenance_source(path, entry) -> Vec<SourceCheck>`: the checkout lookups.
- `provenance_diff(old, new) -> ProvenanceDiff`.

**Frontend:** a new `gui/src/ProvenanceView.svelte` takes `kind === "provenance"`. `Preview.svelte`
gets one `{:else if}` and the switch buttons.

```
┌ coxswain-2.1.0.intoto.jsonl ─────────────── [Flow|Statement|Source] [⤢] ┐
│ SLSA provenance v1 · statement 1 of 3 ‹ ›                                │
│ Signed by …/coxswain/.github/workflows/release.yml@refs/tags/v2.1.0      │
│ via token.actions.githubusercontent.com · Rekor #148203311, 14:19         │
│ ⚠ Not verified: this is what the file claims                             │
├───────── Inputs ──────────┬────── Build ───────┬────── Outputs ─────────┤
│ ● coxswain@a1b2c3d        │ GitHub Actions,    │ ✓ coxswain-x86_64.tgz   │
│   source · in your        ├─▶ hosted runner   ─┼▶ ✓ coxswain-arm64.tgz   │
│   checkout, HEAD~2        │ release.yml · push │ ✗ coxswain.msi  differs │
│ ○ 3 builder dependencies  │ 14:02 → 14:19      │ ? coxswain.dmg  not here│
│                           │ 4 parameters       │                         │
├───────────────────────────┴────────────────────┴─────────────────────────┤
│ coxswain.msi · sha256                                                     │
│ named   9f8e2c…41d0                                                       │
│ on disk 41aa07…be12  (coxswain.msi, 18.2 MB, changed 2026-10-06 15:40)    │
│ The file here is not the one this build made.                            │
└───────────────────────────────────────────────────────────────────────────┘
```

- **Switch:** `Flow | Statement | Source`. *Statement* is the decoded statement in the
  existing JSON tree, which alone is a large gain over base64. *Source* is the file as it is.
  The choice sticks (`ui.previewSource`, new value `"flow"`).
- **Flow:** three columns in a CSS grid, with an SVG layer behind them for the connectors. No
  library is needed. Inputs list sources first, then images, then the rest, with at most 8
  rows and then *n more*. The build box shows the builder (a short name for well-known
  builder IDs, else the ID), the build type, the workflow, trigger and runner from the signer
  or the parameters, the times, the invocation, and the number of parameters. Outputs list the
  subjects with their checks, and byproducts dimmed.
- **Keys:** ←/→ move between columns, ↑/↓ within one, Home/End. **Enter** on an output puts
  the other pane's cursor on the file, or starts **Check** when the file is large. Enter on a
  source goes to the checkout. Enter on the build box opens its parameters in the details. `[`
  and `]` move to the previous or next statement in a `.jsonl`.
- **Details box:** the row under the cursor in full: digests side by side, the URI, the
  annotations, the claim behind each fact ("workflow: from the certificate", or "from
  externalParameters"), and the issues for that row.
- **Links:** the repository, the run (`invocation`) and the Rekor entry are shown as text with
  an *Open* button. Nothing is fetched to draw the view.
- **⤢** opens the same component full-window, as for BOMs.
- All strings from the file are shown as text, never as HTML.

### Compare

*Compare with other pane*, as for BOMs: the other pane's file is "before" and this one is
"after". This works for v1 against v0.2 because both share the model. Statements are paired
by subject name. Within a pair:

| What | Matched by | Shown as |
|---|---|---|
| Builder | id, then version map | changed / same |
| Build type | the string | changed / same |
| Parameters | flattened JSON paths (`workflow.ref`) | added, removed, changed, each with both values |
| Dependencies | URI without its `@ref`, then name | added, removed, digest changed, ref changed |
| Subjects | name | added, removed, digest changed |
| Signer claims | claim | changed / same |

A bar shows **builder changed · n parameters · n dependencies · n outputs**, each a filter.
This answers *what is different between the v2.0.0 and the v2.1.0 build* and *are these two
builds of the same commit the same?*

## Terminal app

The same viewer as a full-screen dialog, `Dialog::Provenance`, modelled on `Dialog::Bom`.

- **F3 on an attestation** opens it. **F3 again** (or `s`) shows the source in the pager.
  `provenance_viewer = false` in the config keeps the pager.
- **Layout:** the three columns side by side at 100 columns or more, and stacked (inputs,
  build, outputs) when narrower. The details take the bottom third. Marks are characters
  (`✓ ✗ ? –`, `●` found, `○` not found) with the word next to them, so colour is never the
  only signal.

| Key | Does |
|---|---|
| ← → | Move between columns (stacked: between sections) |
| ↑ ↓ Home End PgUp PgDn | Move within a column |
| Enter | On an output: put the panel's cursor on the file, or check a large one. On a source: go to the checkout. On the build: its parameters in the details |
| `[` `]` | Previous / next statement |
| Tab | Flow or Statement (the decoded statement as an indented tree) |
| c | Compare with the file under the other panel's cursor; `c` again stops |
| n | While comparing: only what changed |
| o | Open as BOM, for a CycloneDX predicate |
| d / u | Scroll the details |
| F3 / s | The source in the pager |
| Esc | Close |

## Safety and limits

- The file is read-only and nothing is executed. The only programs run are `git`, read-only,
  in a checkout found as above.
- No network. Links open only when the user presses *Open*.
- The size limit is 64 MB, as for BOMs (real attestations are a few KB to a few hundred KB).
  Above it the file is shown as JSON or text, with a note. A base64 payload is decoded only
  when its encoded length fits in the limit, and serde_json's recursion limit stays on.
- Subject paths never leave the provenance's folder or the other pane's folder (the
  `on_disk` rule).
- Hashing runs in the background, cancelled on cursor move, and limited to 256 MB per file
  unless the user asks.
- The header says *Not verified* until [verification](#verification) exists and has run.

## Tests and fixtures

- The fixtures live in `crates/coxswain-core/src/provenance/testdata/`, each with its upstream
  LICENSE file:
  - slsa-verifier's testdata (Apache-2.0): slsa-github-generator generic v0.2 and v1
    `*.intoto.jsonl`, and a container provenance
  - a GitHub artifact attestation bundle (v0.3) from sigstore-js or sigstore-python testdata
    (Apache-2.0)
  - the in-toto attestation spec's examples (Apache-2.0): a bare statement, a VSA, a CycloneDX
    predicate
  - one made here: a `.jsonl` with a bad base64 line, a subject without a digest, and an
    unknown predicate, to test leniency
- The tests:
  - **Ingest:** every fixture parses, with the expected entries, subjects, dependencies,
    signer claims and issues. v0.2 and v1 of the same build map to equal fields.
  - **Sniff:** by name and by content, and a BOM is never sniffed as an attestation.
  - **Subject checks:** in a temp dir, a matching file, a differing one, a missing one, a
    `../` name that must not be followed, and cancellation.
  - **Source checks:** a temp git repo with a remote, a commit behind HEAD, one on another
    branch, and one missing.
  - **Diff:** two v1 fixtures with a changed parameter, dependency and subject.
- GUI: `npx svelte-check && npm test && npm run build`, and the Tauri command tests in
  `provenance.rs`.
- **Our own provenance:** a separate CI-only PR (no version bump) adds
  `actions/attest-build-provenance` to `release.yml`. Coxswain's own releases then have
  provenance, which is a real fixture we own and a good docs example.
- **Screenshots:** `demo-home.sh` gets `projects/rocket/dist/` with two release files and a
  `rocket-1.4.0.intoto.jsonl` whose subjects match one file and not the other, and whose
  source is the demo repository's HEAD~1. This shows ✓, ✗ and *in your checkout* in one
  picture. The signed-identity screenshot uses a testdata bundle.

## Strings and docs

- New catalogue keys go under `provenance.*`: the switch labels, column titles, check results,
  the *Not verified* line, claim names, compare labels and issues, about 45 in all, in every
  locale in the same PR.
- `docs/previews/provenance.md` is the feature page (*Build provenance and attestations*):
  what it shows, the checks and what they prove, compare, files it reads, safety, keys in both
  apps, `config.toml`, and Questions (*Does a ✓ mean the file is safe?*, *Why does it say not
  verified?*, *Why is my commit "not in your checkout"?*, *Can it read npm or PyPI
  provenance?*). It gets a line in `docs/previews/README.md`, `docs/README.md` and a row in
  the preview kinds table, and `docs/faq.md` gets the first two questions.

## Steps

All steps stay on `feature/provenance` and go to `master` as one PR, bumped **minor**, as
the BOM viewer was. Each step ends with its acceptance command green.

1. **Core ingest** (`provenance::{model, ingest, sniff}`, fixtures): every form, v1 and v0.2,
   VSA, the generic predicate, issues. Acceptance: `cargo test -p coxswain-core provenance`.
2. **Core signer** (`x509-parser` into core, the Fulcio claims; `cert_info` moved onto it).
   Acceptance: `cargo test --workspace`, and the musl build.
3. **Core checks** (`provenance::check`: subjects, sources; `on_disk` shared with `bom`).
   Acceptance: `cargo test -p coxswain-core provenance`.
4. **Core diff** (`provenance::diff`). Acceptance: `cargo test -p coxswain-core provenance::diff`.
5. **GUI flow, Statement view, checks, detection, ⤢.** This is the first user-visible step.
   Acceptance: `cargo test --workspace`, `cd gui && npx svelte-check && npm test && npm run
   build`, and a manual run on each fixture.
6. **GUI compare.** Acceptance: as step 5.
7. **TUI viewer** (`Dialog::Provenance`). Acceptance: `cargo test --workspace` and a manual run
   in the sandbox.
8. **Docs, strings in every locale, screenshots, changelog row, version bump.** Acceptance:
   the i18n test, `uv run tools/preview-check.py` with an attestation added to its files, and
   every link checked.

## Decisions

1. **Name:** `provenance` throughout: module, preview kind, dialog, strings and page.
2. **F3 switch:** a separate `provenance_viewer = true`, next to `bom_viewer`. It adds a key
   (minor) where one shared key would rename `bom_viewer` (a breaking config change), and each
   viewer can be turned off on its own.
3. **Known builders:** a small table gives short names to well-known builder IDs and build
   types (*GitHub Actions, hosted runner*); the full ID is always in the details. It claims no
   SLSA levels.
4. **Hashing limit:** files up to 256 MB are checked unasked; larger ones on Enter.
5. **Verify:** none in this version (see [Verification](#verification)).

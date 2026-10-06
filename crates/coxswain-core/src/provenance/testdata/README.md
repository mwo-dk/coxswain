# Provenance test fixtures

| File | Source | Commit | License |
|---|---|---|---|
| `slsa-verifier/envelope-v0.2-multi-subject.intoto.jsonl` | [slsa-framework/slsa-verifier](https://github.com/slsa-framework/slsa-verifier) `cli/slsa-verifier/testdata/binary-linux-amd64-multi-subject-first.intoto.jsonl` | `30d0be3` | Apache-2.0 ([LICENSE](slsa-verifier/LICENSE)) |
| `slsa-verifier/bundle-v0.3-generic-v0.2.intoto.jsonl` | the same, `gha_generic/v2.1.0/binary-linux-amd64-push-v14.intoto.jsonl` | `30d0be3` | Apache-2.0 |
| `slsa-verifier/bundle-v0.2-delegator-v1.build.slsa` | the same, `gha_delegator/v2.1.0/binary-linux-amd64-push-v14.build.slsa` | `30d0be3` | Apache-2.0 |
| `slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl` | the same, `bcr/MODULE.bazel.intoto.jsonl` | `30d0be3` | Apache-2.0 |
| `made/*` | Made here, after the examples in the in-toto and SLSA specifications | | as Coxswain |

The upstream files are copied unmodified apart from their names. Between them they cover a bare
DSSE envelope, Sigstore bundles v0.2 (certificate chain) and v0.3 (single certificate), SLSA
provenance v0.2 and v1, and GitHub's own build type.

`made/` holds what upstream lacks: a bare v1 statement, a verification summary, a CycloneDX
predicate, and `lenient.intoto.jsonl`, whose lines are broken in different ways on purpose
(bad base64, a subject without a digest, an empty line, a line that is not JSON, a payload that
is not in-toto).

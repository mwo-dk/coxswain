# BOM test fixtures

Real CycloneDX files, copied unmodified (apart from renaming) from the sample set of
cipherscape (commit `bc0ec16`). Some keep their errors on purpose, such as wrong OIDs and
dangling references, because the parser has to cope with exactly that.

| File | Source | Commit | License |
|---|---|---|---|
| `bom-examples/*.cdx.json` | [CycloneDX/bom-examples](https://github.com/CycloneDX/bom-examples) `CBOM/*/bom.json` | `7d9172d` | CC0-1.0 ([LICENSE](bom-examples/LICENSE)) |
| `cbomkit/{keycloak,kafka,flick}.cdx.json` | [cbomkit/cbomkit](https://github.com/cbomkit/cbomkit) `example/*-cbom.json` | `d2f20f2` | Apache-2.0 ([LICENSE](cbomkit/LICENSE)) |
| `cbomkit/theia-unknown-key-size.cdx.json` | [cbomkit/cbomkit-theia](https://github.com/cbomkit/cbomkit-theia) `testdata/unknown_key_size/bom.json` | `1571014` | Apache-2.0 |
| `spec/*.xml`, `spec/*.json` | [CycloneDX/specification](https://github.com/CycloneDX/specification) `tools/src/test/resources/{1.6,1.7}/valid-cryptography-*` | `db25df6` | Apache-2.0 ([LICENSE](spec/LICENSE)) |

Two of the spec "pairs" are not twins upstream: full-1.6 puts the protocol properties on
asset-2 in XML but asset-3 in JSON, and implementation-1.7 has different components in each.
Only their XML halves are here, with checks of their own.

// node --test (npm test): provenance detection, as coxswain-core's provenance::sniff_name and
// sniff_head have it (the same cases as their Rust test).
import { test } from "node:test";
import assert from "node:assert/strict";
import { isProvenanceName, looksLikeProvenance, looksLikeBom } from "./sniff.js";


test("provenance by name", () => {
  for (const n of ["x.intoto.jsonl", "X.INTOTO.JSON", "a.sigstore.json", "a.sigstore", "a.dsse.json", "rocket.provenance.json", "b.build.slsa", "provenance.json"])
    assert.ok(isProvenanceName(n), n);
  for (const n of ["bom.json", "a.json", "intoto.txt", "provenance.json.bak"]) assert.ok(!isProvenanceName(n), n);
});

test("provenance by its first bytes, never a BOM", () => {
  assert.ok(looksLikeProvenance('{"payloadType":"application/vnd.in-toto+json","payload":'));
  assert.ok(looksLikeProvenance('{"_type": "https://in-toto.io/Statement/v1", "subject"'));
  assert.ok(looksLikeProvenance('{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"'));
  assert.ok(!looksLikeProvenance('{"bomFormat": "CycloneDX", "specVersion": "1.6"}'));
  assert.ok(!looksLikeProvenance('{"payloadType": "text/plain"}'));
  // A statement with a CycloneDX predicate looks like a BOM too: the preview asks provenance first.
  const sbom = '{"_type": "https://in-toto.io/Statement/v1", "predicate": {"bomFormat": "CycloneDX"}}';
  assert.ok(looksLikeProvenance(sbom) && looksLikeBom(sbom));
});

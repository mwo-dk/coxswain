// node --test (npm test): provenance detection by a file's first bytes, as coxswain-core's
// provenance::sniff_head has it (the same cases as its Rust test). By name: viewers.test.js.
import { test } from "node:test";
import assert from "node:assert/strict";
import { looksLikeProvenance, looksLikeBom } from "./sniff.js";

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

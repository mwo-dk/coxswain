// What a file's first bytes say, for the kinds the preview recognises by content too:
// CycloneDX BOMs and in-toto attestations (by name: the core's `viewers::rules`). No imports, so node --test can load it.

/** Whether the start of a JSON or XML file is a CycloneDX BOM's (as coxswain-core's bom::sniff_head). */
export function looksLikeBom(text) {
  const head = text.slice(0, 8192);
  return (head.includes('"bomFormat"') && head.includes('"CycloneDX"')) || head.includes("http://cyclonedx.org/schema/bom/");
}

/** Whether the start of a JSON file is an in-toto envelope's, statement's or Sigstore bundle's
 *  (as coxswain-core's provenance::sniff_head). Asked before looksLikeBom: a statement may carry
 *  a CycloneDX predicate. */
export function looksLikeProvenance(text) {
  const head = text.slice(0, 8192);
  return (
    (head.includes('"payloadType"') && head.includes("application/vnd.in-toto+json")) ||
    (head.includes('"_type"') && head.includes("https://in-toto.io/Statement/")) ||
    head.includes("application/vnd.dev.sigstore.bundle")
  );
}

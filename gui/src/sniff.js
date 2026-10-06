// What kind of file a name or its first bytes say, for the kinds the preview recognises by
// content too: CycloneDX BOMs and in-toto attestations. No imports, so node --test can load it.

/** Names that say CycloneDX BOM. Other JSON and XML files are recognised by their first bytes (looksLikeBom). */
const BOM_NAME = /(\.(cdx|cbom)\.(json|xml)|^bom\.(json|xml))$/i;

/** Whether the start of a JSON or XML file is a CycloneDX BOM's (as coxswain-core's bom::sniff_head). */
export function looksLikeBom(text) {
  const head = text.slice(0, 8192);
  return (head.includes('"bomFormat"') && head.includes('"CycloneDX"')) || head.includes("http://cyclonedx.org/schema/bom/");
}

/** Names that say in-toto attestation (as coxswain-core's provenance::sniff_name). Other JSON and
 *  JSON Lines files are recognised by their first bytes (looksLikeProvenance). */
const PROVENANCE_NAME = /(\.intoto\.jsonl?|\.sigstore(\.json)?|\.dsse\.json|\.provenance\.json|\.build\.slsa|^provenance\.json)$/i;

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

/** Whether a name says CycloneDX BOM. */
export const isBomName = (name) => BOM_NAME.test(name);

/** Whether a name says in-toto attestation. */
export const isProvenanceName = (name) => PROVENANCE_NAME.test(name);

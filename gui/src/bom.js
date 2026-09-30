// The BOM view's logic without the DOM (see BomView.svelte and docs/design/bom-viewer.md):
// which rows match the filters, and which rows the tree shows.

/** Statuses from worst to best, as the chips show them. */
export const STATUSES = ["broken", "disallowed", "deprecated", "unknown", "acceptable", "safe", "not-rated"];
export const KINDS = ["algorithm", "certificate", "protocol", "material", "component"];
export const FAMILIES = ["signature", "asymmetric", "kem", "symmetric", "hash", "mac"];
const CRYPTO = new Set(["algorithm", "certificate", "protocol", "material"]);

export const isCrypto = (row) => CRYPTO.has(row.k);

/** The colour a status is shown in: green, grey, yellow, red or muted. */
export const COLOR = { safe: "green", acceptable: "green", unknown: "grey", deprecated: "yellow", disallowed: "red", broken: "red", "not-rated": "muted" };

/** Each row's children, siblings in order. */
export function childrenOf(rows, order) {
  const kids = rows.map(() => []);
  for (const i of order) if (rows[i].p >= 0) kids[rows[i].p].push(i);
  return kids;
}

/** Whether any filter is set. */
export const filtering = (f) => f.status.size > 0 || f.kind.size > 0 || f.family.size > 0 || f.query.trim() !== "";

/**
 * Which rows match the filters (`match`) and which have a match at or below them (`keep`), so
 * that what a match sits in stays visible. Status and family filters look at crypto assets only;
 * the search also finds groups and components by name. O(n).
 */
export function mask(rows, order, f, label) {
  const n = rows.length;
  const match = new Uint8Array(n);
  const keep = new Uint8Array(n);
  const q = f.query.trim().toLowerCase();
  const onlyAssets = f.status.size > 0 || f.family.size > 0;
  for (let i = 0; i < n; i++) {
    const r = rows[i];
    if (onlyAssets && !isCrypto(r)) continue;
    if (f.status.size && !f.status.has(r.s)) continue;
    if (f.kind.size && !f.kind.has(r.k)) continue;
    if (f.family.size && !(r.f ?? []).some((x) => f.family.has(x))) continue;
    if (q && !r.q.includes(q) && !label(i).toLowerCase().includes(q)) continue;
    match[i] = 1;
  }
  // Reverse pre-order: every row after everything below it.
  for (let k = order.length - 1; k >= 0; k--) {
    const i = order[k];
    if (match[i]) keep[i] = 1;
    const p = rows[i].p;
    if (keep[i] && p >= 0) keep[p] = 1;
  }
  return { match, keep };
}

/** How many children a row shows before a "more" row. */
export const PAGE = 500;

/**
 * The rows the tree shows, in order: `{ i, depth }` for a node, `{ more, depth, n }` for the rest
 * of a long list. `open` says which rows are expanded; with `hide`, rows outside `keep` are left out.
 */
export function visibleRows(rows, kids, open, shown, keep, hide) {
  const out = [];
  const stack = [[0, 0]];
  while (stack.length) {
    const [i, depth] = stack.pop();
    if (typeof i === "object") {
      out.push({ ...i, depth });
      continue;
    }
    out.push({ i, depth });
    if (!open(i)) continue;
    const list = hide ? kids[i].filter((c) => keep[c]) : kids[i];
    const limit = shown[i] ?? PAGE;
    const page = list.slice(0, limit);
    if (list.length > limit) stack.push([{ more: i, n: list.length - limit }, depth + 1]);
    for (let c = page.length - 1; c >= 0; c--) stack.push([page[c], depth + 1]);
  }
  return out;
}

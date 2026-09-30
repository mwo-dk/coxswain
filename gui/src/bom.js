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

// ------------------------------------------------------------ sunburst
//
// The tree as rings: the zoomed-in node in the middle, its children in the first ring, and so on.
// An arc's angle is proportional to the leaves beneath it. Arcs narrower than MIN_ANGLE are merged
// into one "more" arc per parent, and at most RINGS rings are drawn, so a 50k-node BOM stays at a
// few thousand paths.

export const RINGS = 6;
export const MIN_ANGLE = 0.5;
const RANK = Object.fromEntries(STATUSES.map((s, i) => [s, STATUSES.length - i]));

/** The worse of two statuses; not rated is neutral. */
export function worse(a, b) {
  if (!a || a === "not-rated") return b;
  if (!b || b === "not-rated") return a;
  return RANK[a] >= RANK[b] ? a : b;
}

/** Leaves beneath each row (a leaf counts itself). With `keep`, rows outside it count nothing. */
export function leafCounts(rows, order, kids, keep) {
  const leaves = new Float64Array(rows.length);
  for (let k = order.length - 1; k >= 0; k--) {
    const i = order[k];
    if (keep && !keep[i]) continue;
    let sum = 0;
    for (const c of kids[i]) sum += leaves[c];
    leaves[i] = kids[i].length ? sum : 1;
  }
  return leaves;
}

/**
 * The arcs below `root`: `{ i, depth, a0, a1 }` in degrees for a node, or `{ more, n, depth, a0,
 * a1, s }` for thin siblings merged (`s` their worst status). Depth 1 is the first ring.
 */
export function sunburstArcs(rows, kids, leaves, root, rings = RINGS, minAngle = MIN_ANGLE) {
  const arcs = [];
  const stack = [[root, 0, 0, 360]];
  while (stack.length) {
    const [p, depth, a0, a1] = stack.pop();
    if (depth >= rings || !leaves[p]) continue;
    const scale = (a1 - a0) / leaves[p];
    let at = a0;
    let merged = null;
    for (const c of kids[p]) {
      const span = leaves[c] * scale;
      if (!span) continue;
      if (span < minAngle) {
        merged ??= { more: p, n: 0, depth: depth + 1, a0: at, a1: at, s: null };
        merged.n++;
        merged.a1 += span;
        merged.s = worse(merged.s, rows[c].s);
      } else {
        arcs.push({ i: c, depth: depth + 1, a0: at, a1: at + span });
        stack.push([c, depth + 1, at, at + span]);
      }
      at += span;
    }
    if (merged) arcs.push(merged);
  }
  return arcs;
}

const point = (cx, cy, r, deg) => {
  const a = ((deg - 90) * Math.PI) / 180;
  return `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
};

/** An SVG path for the ring sector from `a0` to `a1` degrees (clockwise from the top), between radii. */
export function arcPath(cx, cy, r0, r1, a0, a1) {
  // A whole ring is two halves: one arc cannot start and end at the same point.
  if (a1 - a0 >= 359.99) return arcPath(cx, cy, r0, r1, a0, a0 + 180) + " " + arcPath(cx, cy, r0, r1, a0 + 180, a0 + 360);
  const large = a1 - a0 > 180 ? 1 : 0;
  return (
    `M${point(cx, cy, r1, a0)} A${r1} ${r1} 0 ${large} 1 ${point(cx, cy, r1, a1)} ` +
    `L${point(cx, cy, r0, a1)} A${r0} ${r0} 0 ${large} 0 ${point(cx, cy, r0, a0)} Z`
  );
}

/** The rows from the root to `i`, for breadcrumbs. */
export function pathTo(rows, i) {
  const out = [];
  for (let at = i; at >= 0; at = rows[at].p) out.unshift(at);
  return out;
}

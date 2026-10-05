// Find's keyboard helpers; the rows themselves come from coxswain-core (`find::rows`).

/** The kinds, in Tab's order. */
export const KINDS = ["all", "names", "in_files", "about", "ask"];

/** A prefix typed first (`text:`, `about:`, a leading `?`): its kind, and the query without it. */
export function prefix(query) {
  const m = /^\s*(text:|about:|\?)\s*/i.exec(query);
  if (!m) return { kind: null, rest: query };
  const kind = { "text:": "in_files", "about:": "about", "?": "ask" }[m[1].toLowerCase()];
  return { kind, rest: query.slice(m[0].length) };
}

/** The next kind (Tab), or the one before (Shift+Tab), round. */
export const nextKind = (kind, back) => KINDS[(KINDS.indexOf(kind) + (back ? KINDS.length - 1 : 1)) % KINDS.length];

/** The row `by` selectable rows from `at` (headings are skipped), within the list. */
export function step(rows, at, by) {
  const sel = rows.map((r, i) => (r.row === "head" ? -1 : i)).filter((i) => i >= 0);
  if (!sel.length) return 0;
  let here = sel.findIndex((i) => i >= at);
  if (here < 0) here = sel.length - 1;
  return sel[Math.max(0, Math.min(sel.length - 1, here + by))];
}

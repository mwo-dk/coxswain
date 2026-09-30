// node --test (npm test): the BOM view's logic, without a browser.
import { test } from "node:test";
import assert from "node:assert/strict";
import { childrenOf, mask, visibleRows, leafCounts, sunburstArcs, arcPath, worse, pathTo, changeIs, MIN_ANGLE } from "./bom.js";

/** A root with `n` leaves under one group, plus one more leaf of its own. */
function tree(n) {
  const rows = [{ p: -1, k: "application", s: "broken", q: "app" }, { p: 0, k: "group", s: "broken", q: "g" }];
  for (let i = 0; i < n; i++) rows.push({ p: 1, k: "algorithm", s: i === 0 ? "broken" : "acceptable", q: `alg${i}`, f: ["hash"] });
  rows.push({ p: 0, k: "certificate", s: "deprecated", q: "cert" });
  const order = [0, 1, ...rows.slice(2, -1).map((_, i) => i + 2), rows.length - 1];
  return { rows, order, kids: childrenOf(rows, order) };
}

test("the arcs of each ring cover the whole circle", () => {
  const { rows, order, kids } = tree(10);
  const leaves = leafCounts(rows, order, kids);
  assert.equal(leaves[0], 11);
  const arcs = sunburstArcs(rows, kids, leaves, 0);
  for (const depth of [1, 2]) {
    const sum = arcs.filter((a) => a.depth === depth).reduce((s, a) => s + a.a1 - a.a0, 0);
    assert.ok(Math.abs(sum - (depth === 1 ? 360 : (360 * 10) / 11)) < 1e-9, `ring ${depth}: ${sum}`);
  }
});

test("thin arcs merge into one per parent, with their worst status", () => {
  const { rows, order, kids } = tree(5000);
  const arcs = sunburstArcs(rows, kids, leafCounts(rows, order, kids), 0);
  const more = arcs.filter((a) => a.more !== undefined);
  assert.equal(more.length, 2); // one under the group, one for the thin certificate at the root
  assert.equal(more[0].n + more[1].n, 5001);
  assert.equal(worse(more[0].s, more[1].s), "broken");
  assert.ok(arcs.length < 10);
  assert.ok(arcs.every((a) => a.more !== undefined || a.a1 - a.a0 >= MIN_ANGLE));
});

test("only so many rings", () => {
  const rows = [{ p: -1, k: "application", s: "safe", q: "" }];
  for (let i = 1; i < 20; i++) rows.push({ p: i - 1, k: "group", s: "safe", q: "" });
  const order = rows.map((_, i) => i);
  const kids = childrenOf(rows, order);
  const arcs = sunburstArcs(rows, kids, leafCounts(rows, order, kids), 0, 6);
  assert.equal(Math.max(...arcs.map((a) => a.depth)), 6);
  assert.equal(sunburstArcs(rows, kids, leafCounts(rows, order, kids), 10, 6)[0].i, 11); // zoomed in
});

test("hidden rows take no room", () => {
  const { rows, order, kids } = tree(3);
  const f = { status: new Set(["deprecated"]), kind: new Set(), family: new Set(), query: "" };
  const { keep } = mask(rows, order, f, (i) => rows[i].q);
  const arcs = sunburstArcs(rows, kids, leafCounts(rows, order, kids, keep), 0);
  assert.deepEqual(arcs.map((a) => rows[a.i].q), ["cert"]);
  assert.equal(arcs[0].a1 - arcs[0].a0, 360);
});

test("a whole ring is drawn as two halves, a sector as one", () => {
  assert.equal((arcPath(50, 50, 10, 20, 0, 360).match(/M/g) ?? []).length, 2);
  assert.equal((arcPath(50, 50, 10, 20, 0, 90).match(/A/g) ?? []).length, 2);
  assert.match(arcPath(50, 50, 10, 20, 0, 90), /^M50\.00 30\.00 A20 20 0 0 1 70\.00 50\.00/);
});

test("filters keep what a match sits in; the tree pages long lists", () => {
  const { rows, order, kids } = tree(1200);
  const f = { status: new Set(), kind: new Set(), family: new Set(), query: "alg7" };
  const { match, keep } = mask(rows, order, f, (i) => rows[i].q);
  assert.equal(match[0], 0);
  assert.equal(keep[0], 1);
  assert.equal(keep[1], 1);
  const all = visibleRows(rows, kids, () => true, {}, keep, false);
  assert.equal(all.filter((v) => v.more !== undefined)[0].n, 700);
  const hidden = visibleRows(rows, kids, () => true, {}, keep, true);
  assert.ok(hidden.every((v) => v.i === undefined || keep[v.i]));
  assert.deepEqual(pathTo(rows, 5), [0, 1, 5]);
});

test("a compare filters by change, and new risks are what got worse or came in red", () => {
  const { rows, order } = tree(3);
  const changes = ["unchanged", "unchanged", "added", "worsened", "improved", "added"];
  const risk = { status: new Set(), kind: new Set(), family: new Set(), change: new Set(["risk"]), query: "" };
  // row 2 is broken and added, row 3 got worse; row 5 (the certificate) is added but only deprecated... still not green
  const { match } = mask(rows, order, risk, (i) => rows[i].q, changes);
  assert.deepEqual([...match].map((m, i) => (m ? i : -1)).filter((i) => i >= 0), [2, 3, 5]);
  assert.equal(changeIs({ s: "acceptable" }, "added", new Set(["risk"])), false);
  assert.equal(changeIs({ s: "acceptable" }, "improved", new Set(["improved"])), true);
});

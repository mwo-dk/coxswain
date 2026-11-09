// node --test (npm test): previewKind evaluates the core's table of viewers (`viewers::rules`)
// as the core does: viewer-kinds.json holds the table and each case with the core's answer
// (written by `COXSWAIN_WRITE_VARIANTS=1 cargo test -p coxswain-core viewers`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("./lib.js", import.meta.url), "utf8").replaceAll("\r\n", "\n");
const { rules, reads, max_whole, cases } = JSON.parse(readFileSync(new URL("./viewer-kinds.json", import.meta.url), "utf8"));

/** lib.js's `viewerRules`, `setViewerRules` and `previewKind`, as a module: lib.js imports the
 *  app's state, which node cannot load. */
async function load() {
  const lines = src.split("\n");
  const part = (n) => {
    const at = lines.findIndex((l) => new RegExp(`^(export )?(let|function) ${n}\\b`).test(l));
    assert.ok(at >= 0, n);
    let end = at + 1;
    while (end < lines.length && /^(\s|\}|$)/.test(lines[end]) && lines[end] !== "") end++;
    return lines.slice(at, end).join("\n").replace(/^export /, "");
  };
  const code = ["viewerRules", "readsByKind", "setViewerRules", "previewKind", "viewerReads"].map(part).join("\n");
  return import("data:text/javascript," + encodeURIComponent(`${code}\nexport { setViewerRules, previewKind, viewerReads };`));
}

test("previewKind gives the core's kind for every case", async () => {
  const { setViewerRules, previewKind } = await load();
  setViewerRules(rules);
  for (const c of cases) assert.equal(previewKind({ name: c.name, is_dir: c.is_dir }), c.kind, `${c.name}${c.is_dir ? "/" : ""}`);
  assert.equal(previewKind(null), "none");
});

test("viewerReads gives what the core says each kind's viewer needs inside an archive", async () => {
  const { setViewerRules, viewerReads } = await load();
  setViewerRules(rules, reads);
  for (const [kind, want] of Object.entries(reads)) assert.equal(viewerReads(kind), want, kind);
  assert.equal(viewerReads("text"), "stream");
  assert.equal(viewerReads("no such kind"), "copy");
  const limit = src.match(/^export const MAX_WHOLE = (.+);$/m);
  assert.ok(limit, "lib.js says MAX_WHOLE");
  assert.equal(Function(`return ${limit[1]}`)(), max_whole, "lib.js's MAX_WHOLE is the core's");
});

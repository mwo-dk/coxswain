// node --test (npm test): previewKind evaluates the core's table of viewers (`viewers::rules`)
// as the core does: viewer-kinds.json holds the table and each case with the core's answer
// (written by `COXSWAIN_WRITE_VARIANTS=1 cargo test -p coxswain-core viewers`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("./lib.js", import.meta.url), "utf8").replaceAll("\r\n", "\n");
const { rules, cases } = JSON.parse(readFileSync(new URL("./viewer-kinds.json", import.meta.url), "utf8"));

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
  const code = ["viewerRules", "setViewerRules", "previewKind"].map(part).join("\n");
  return import("data:text/javascript," + encodeURIComponent(`${code}\nexport { setViewerRules, previewKind };`));
}

test("previewKind gives the core's kind for every case", async () => {
  const { setViewerRules, previewKind } = await load();
  setViewerRules(rules);
  for (const c of cases) assert.equal(previewKind({ name: c.name, is_dir: c.is_dir }), c.kind, `${c.name}${c.is_dir ? "/" : ""}`);
  assert.equal(previewKind(null), "none");
});

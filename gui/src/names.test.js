// A name used but never declared (a constant lost in a merge) throws only when that code runs,
// and a throw in a Svelte effect stops every preview: the type checker finds them all at once.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";

test("every name in the frontend is declared", () => {
  let out = "";
  try {
    out = execFileSync("npx", ["svelte-check", "--tsconfig", "./names.jsconfig.json", "--output", "machine"], { encoding: "utf8", shell: process.platform === "win32", maxBuffer: 64 << 20 });
  } catch (e) {
    out = e.stdout ?? "";
  }
  const missing = out.split("\n").filter((l) => l.includes(" \"src/") && l.includes("Cannot find name"));
  assert.deepEqual(missing, []);
});

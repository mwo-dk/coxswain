import { test } from "node:test";
import assert from "node:assert/strict";
import { prefix, nextKind, step } from "./find.js";

test("prefixes turn their kind on; a ? inside stays a wildcard", () => {
  assert.deepEqual(prefix("text: rocket"), { kind: "in_files", rest: "rocket" });
  assert.deepEqual(prefix("About:brændstof"), { kind: "about", rest: "brændstof" });
  assert.deepEqual(prefix("? what costs"), { kind: "ask", rest: "what costs" });
  assert.deepEqual(prefix("a?c.txt"), { kind: null, rest: "a?c.txt" });
  assert.deepEqual(prefix("texture"), { kind: null, rest: "texture" });
});

test("Tab walks the kinds round, Shift+Tab back", () => {
  assert.equal(nextKind("all", false), "names");
  assert.equal(nextKind("ask", false), "all");
  assert.equal(nextKind("all", true), "ask");
});

test("the cursor steps over headings", () => {
  const rows = [{ row: "ask" }, { row: "head" }, { row: "hit" }, { row: "hit" }, { row: "head" }, { row: "off" }];
  assert.equal(step(rows, 0, 1), 2);
  assert.equal(step(rows, 3, 1), 5);
  assert.equal(step(rows, 5, 1), 5);
  assert.equal(step(rows, 2, -1), 0);
  assert.equal(step(rows, 2, -10), 0);
  assert.equal(step([], 0, 1), 0);
});

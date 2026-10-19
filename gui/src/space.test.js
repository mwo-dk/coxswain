import { test } from "node:test";
import assert from "node:assert/strict";
import { at, step } from "./space.js";

// A folder holding two files side by side, and a file beside the folder.
const tiles = [
  { x: 0, y: 0, w: 60, h: 40, kind: "folder" },
  { x: 2, y: 2, w: 28, h: 36, kind: "file" },
  { x: 30, y: 2, w: 28, h: 36, kind: "file" },
  { x: 60, y: 0, w: 40, h: 40, kind: "file" },
];

test("the deepest rectangle under the pointer; folders only where no file is", () => {
  assert.equal(at(tiles, 10, 10), 1);
  assert.equal(at(tiles, 1, 1), 0, "the folder's frame");
  assert.equal(at(tiles, 1, 1, true), -1);
  assert.equal(at(tiles, 200, 1), -1);
});

test("arrows go to the next file that way, and stay at the edge", () => {
  assert.equal(step(tiles, 1, 1, 0), 2);
  assert.equal(step(tiles, 2, 1, 0), 3);
  assert.equal(step(tiles, 3, -1, 0), 2);
  assert.equal(step(tiles, 3, 1, 0), 3);
  assert.equal(step(tiles, -1, 0, 0), 1, "no cursor yet: the top left");
});

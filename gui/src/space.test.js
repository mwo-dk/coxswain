import { test } from "node:test";
import assert from "node:assert/strict";
import { at, step, chain, fit, palette } from "./space.js";

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

test("the folder chain around a file, outermost first", () => {
  const nested = [{ x: 0, y: 0, w: 100, h: 100, kind: "folder", depth: 1 }, { x: 2, y: 2, w: 50, h: 50, kind: "folder", depth: 2 }, { x: 4, y: 4, w: 10, h: 10, kind: "file", depth: 3 }, { x: 60, y: 60, w: 30, h: 30, kind: "folder", depth: 2 }];
  assert.deepEqual(chain(nested, 2), [0, 1]);
  assert.deepEqual(chain(nested, -1), []);
});

test("a label fits whole, cut with at least eight letters, or not at all", () => {
  const width = (s) => s.length * 10;
  assert.equal(fit("launch.mp4", 200, width), "launch.mp4");
  assert.equal(fit("onboard-camera.mov", 120, width), "onboard-cam…");
  assert.equal(fit("06 Track six.flac", 60, width), "", "no crumbs");
});

const FIXED = { image: "#b07cff", video: "#ff5f6d", audio: "#ff9f40", archive: "#e8d44d", document: "#4ea8ff", code: "#3fd47a", data: "#26d0c4", program: "#ff6fd2", disk: "#c98a5a", other: "#8d99a6" };
const apart = (p) => {
  const v = Object.values(p).map((c) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)));
  return v.every((a, i) => v.every((b, j) => i === j || Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) >= 40));
};

test("the colours come from the theme, and no two kinds look alike", () => {
  const midnight = { red: "#f7768e", orange: "#ff9e64", yellow: "#e0af68", green: "#9ece6a", blue: "#2ac3de", magenta: "#bb9af7", dir: "#7aa2f7", dim: "#565f89", bg: "#1a1b26" };
  const p = palette(midnight, FIXED);
  assert.equal(p.video, "#f7768e");
  assert.equal(p.code, "#9ece6a");
  assert.ok(apart(p));
  // Cyber: greens, cyans and ambers only.
  const cyber = { red: "#ffa31a", orange: "#ffb000", yellow: "#d4ff3a", green: "#33ff66", blue: "#3fd0ff", magenta: "#3fffd2", dir: "#5cf2ff", dim: "#1c9c48", bg: "#020805" };
  assert.ok(apart(palette(cyber, FIXED)), JSON.stringify(palette(cyber, FIXED)));
  // A theme without these colours: the fixed ones.
  assert.deepEqual(palette({}, FIXED).image, FIXED.image);
});

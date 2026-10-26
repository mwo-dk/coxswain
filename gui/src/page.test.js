// An HTML file shown as a page: its frame loads it from the app's file protocol (`?page`), its
// folder written one segment at a time, so its own stylesheets and pictures resolve beside it.
// The protocol's policy (asset.rs) names the same folder and nothing more.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// renderers.js pulls in the app's libraries; pageUrl needs none of them.
// A Windows checkout may have CRLF line ends.
const src = readFileSync(new URL("./renderers.js", import.meta.url), "utf8").replaceAll("\r\n", "\n");
const body = src.slice(src.indexOf("export function pageUrl"), src.indexOf("\n}\n", src.indexOf("export function pageUrl")) + 2);
const { pageUrl } = await import("data:text/javascript," + encodeURIComponent(body));

test("an HTML page is asked for as a page, its links resolving in its own folder", () => {
  const url = pageUrl("http://asset.localhost/%2Fhome%2Fme%2Fsite%20v2%2Findex.html", "/home/me/site v2/index.html");
  assert.equal(url, "http://asset.localhost/%2Fhome/me/site%20v2/index.html?page");
  // A stylesheet beside it, or in a folder below, is asked for by its absolute path (the protocol drops the first `/`).
  assert.equal(decodeURIComponent(new URL("assets/style.css", url).pathname.slice(1)), "/home/me/site v2/assets/style.css");
  // `..` leaves the folder: the protocol's policy refuses it.
  assert.equal(decodeURIComponent(new URL("../x.css", url).pathname.slice(1)), "/home/me/x.css");
  // Windows: the drive is a segment of its own.
  assert.equal(pageUrl("http://asset.localhost/C%3A%5Cdocs%5Ca.html", "C:\\docs\\a.html"), "http://asset.localhost/C%3A/docs/a.html?page");
});

// A file's <style> would restyle the whole window: clean() forbids it unless asked to keep it.
// No DOM here, so DOMPurify is a stand-in that tells what clean() asks of it.
test("sanitized HTML keeps no style sheet", async () => {
  const at = src.indexOf("export const clean");
  const decl = src.slice(at, src.indexOf(";\n", at) + 2).replace(/^export /, "");
  const { clean } = await import("data:text/javascript," + encodeURIComponent(`const DOMPurify = { sanitize: (_, o) => o };\n${decl}\nexport { clean };`));
  assert.deepEqual(clean("<style>body{display:none}</style>").FORBID_TAGS, ["style"]);
  assert.deepEqual(clean("<svg><style></style></svg>", { styles: true }).FORBID_TAGS, []);
});

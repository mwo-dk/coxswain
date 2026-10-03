// An HTML file shown as a page: its frame's content security policy names the page's own
// folder, never the whole file protocol, so a downloaded page cannot show what is elsewhere
// on the disk.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// renderers.js pulls in the app's libraries; pageHead needs none of them.
const src = readFileSync(new URL("./renderers.js", import.meta.url), "utf8");
const body = src.slice(src.indexOf("export function pageHead"), src.indexOf("\n}\n", src.indexOf("export function pageHead")) + 2);
const { pageHead } = await import("data:text/javascript," + encodeURIComponent(body));

test("an HTML page may load from its own folder only", () => {
  const head = pageHead("http://asset.localhost/%2Fhome%2Fme%2Fsite%20v2%2Findex.html", "/home/me/site v2/index.html");
  const csp = head.match(/content="([^"]*)"/)[1];
  assert.equal(head.match(/<base href="([^"]*)"/)[1], "http://asset.localhost/home/me/site%20v2/");
  for (const what of ["img-src", "style-src", "font-src", "media-src"]) {
    assert.match(csp, new RegExp(`${what} http://asset.localhost/home/me/site%20v2/( |$)`), what);
  }
  assert.doesNotMatch(csp, /asset\.localhost[ ;]/, "never the whole file protocol");
  assert.match(csp, /default-src 'none'/);
  // Windows: the drive is a segment of its own.
  assert.match(pageHead("http://asset.localhost/C%3A%5Cdocs%5Ca.html", "C:\\docs\\a.html"), /img-src http:\/\/asset\.localhost\/C%3A\/docs\/ /);
});

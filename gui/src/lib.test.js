// node --test (npm test): the pure helpers of lib.js. lib.js imports the app's state, which
// node cannot load, so each helper is taken out of it by name, with what it uses.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("./lib.js", import.meta.url), "utf8").replaceAll("\r\n", "\n");
/** The top-level declarations `names` of lib.js, as a module: each runs to the next line that
 *  starts at the margin and does not close it. */
async function pick(...names) {
  const lines = src.split("\n");
  const parts = names.map((n) => {
    const at = lines.findIndex((l) => new RegExp(`^(export )?(const|let|function|async function) ${n}\\b`).test(l));
    assert.ok(at >= 0, n);
    let end = at + 1;
    while (end < lines.length && /^(\s|\}|\)|$)/.test(lines[end]) && !/^$/.test(lines[end])) end++;
    return lines.slice(at, end).join("\n").replace(/^export /, "");
  });
  return import("data:text/javascript," + encodeURIComponent(`${parts.join("\n")}\nexport { ${names.join(", ")} };`));
}

test("quote: single quotes for a Unix shell, double for Windows' cmd", async () => {
  const { quote } = await pick("quote");
  assert.equal(quote("a.txt"), "a.txt");
  assert.equal(quote("my notes.txt"), "'my notes.txt'");
  assert.equal(quote("it's"), "'it'\\''s'");
  assert.equal(quote("my notes.txt", true), '"my notes.txt"');
  assert.equal(quote('say "hi"', true), '"say ""hi"""');
  assert.equal(quote(""), "''");
});

test("globRe: whole name, any case, ? one character", async () => {
  const { globRe } = await pick("globRe");
  assert.ok(globRe("*.TXT").test("a.txt"));
  assert.ok(globRe("caf?").test("café"));
  assert.ok(!globRe("a.c").test("abc"), "a dot is a dot");
});

test("archive endings: the core's lists, the longest ending first", async () => {
  const { setArchiveEndings, isArchive, withEnding, takesPassword } = await pick("archive", "setArchiveEndings", "endingOf", "isArchive", "withEnding", "takesPassword");
  setArchiveEndings([[".tar.gz", ".tgz", ".tar", ".7z", ".zip"], [".7z", ".zip"]]);
  assert.ok(isArchive("A.TAR.GZ") && !isArchive("a.gz"));
  assert.equal(withEnding("b.tar.gz", ".zip"), "b.zip");
  assert.equal(withEnding("my.notes", ".zip"), "my.notes.zip");
  assert.ok(takesPassword("x.ZIP") && !takesPassword("x.tar"));
});

test("natord and sortItems: as coxswain-core's fs::natord and fs::sort", async () => {
  const { natord, sortItems } = await pick("isDigit", "byPoint", "natord", "sortItems");
  assert.ok(natord("file2", "file10") < 0);
  assert.equal(natord("a", "A"), 0);
  assert.ok(natord("a", "B") < 0);
  assert.equal(natord("01", "1"), 0);
  assert.ok(natord("x", "x1") < 0);
  const items = [
    { name: "b.txt", is_dir: false, modified: 1, size: 5 },
    { name: "..", is_dir: true },
    { name: "a10.md", is_dir: false, modified: 3, size: 1 },
    { name: "Docs", is_dir: true, modified: 2 },
    { name: "a2.md", is_dir: false, modified: null, size: 9 },
  ];
  const names = (key, reverse = false) => sortItems(items, key, reverse).map((e) => e.name);
  assert.deepEqual(names("name"), ["..", "Docs", "a2.md", "a10.md", "b.txt"]);
  assert.deepEqual(names("name", true), ["..", "Docs", "b.txt", "a10.md", "a2.md"]);
  assert.deepEqual(names("ext"), ["..", "Docs", "a2.md", "a10.md", "b.txt"]);
  assert.deepEqual(names("time"), ["..", "Docs", "a10.md", "b.txt", "a2.md"], "newest first, none last");
  assert.deepEqual(names("size"), ["..", "Docs", "a2.md", "b.txt", "a10.md"], "biggest first");
});

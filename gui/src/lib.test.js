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

test("disk images: by name, .vhd and .img only once their content says so", async () => {
  const { setDiskEndings, diskName, diskSure } = await pick("endingOf", "disk", "setDiskEndings", "diskName", "diskSure");
  setDiskEndings([".dmg", ".iso", ".chm", ".cbr", ".vhd", ".img"]);
  assert.ok(diskName("Mac.DMG") && diskSure("Mac.DMG") && diskSure("a.cbr"));
  assert.ok(diskName("blink.vhd") && !diskSure("blink.vhd") && !diskSure("sd.IMG"));
  assert.ok(!diskName("notes.txt"));
});

test("old pictures: the core's endings, any case", async () => {
  const { setPictureEndings, isOldPicture } = await pick("OLD", "setPictureEndings", "isOldPicture");
  setPictureEndings(["tif", "iff", "psd"]);
  assert.ok(isOldPicture("Scan.TIF") && isOldPicture("boing.iff") && !isOldPicture("photo.jpg"));
  setPictureEndings(["pcx"]);
  assert.ok(!isOldPicture("a.tif"), "the list replaced");
});

test("sortItems: as coxswain-core's fs::sort, names by their ranks from collate", async () => {
  const { sortItems } = await pick("sortItems");
  // What `name_ranks` gives for these: the language's order, numbers by their value.
  const order = ["", "a2.md", "a10.md", "b.txt", "Docs", "md", "txt"];
  const rank = (w) => order.indexOf(w);
  const items = [
    { name: "b.txt", is_dir: false, modified: 1, size: 5 },
    { name: "..", is_dir: true },
    { name: "a10.md", is_dir: false, modified: 3, size: 1 },
    { name: "Docs", is_dir: true, modified: 2 },
    { name: "a2.md", is_dir: false, modified: null, size: 9 },
  ];
  const names = (key, reverse = false) => sortItems(items, key, reverse, {}, rank).map((e) => e.name);
  assert.deepEqual(names("name"), ["..", "Docs", "a2.md", "a10.md", "b.txt"]);
  assert.deepEqual(names("name", true), ["..", "Docs", "b.txt", "a10.md", "a2.md"]);
  assert.deepEqual(names("ext"), ["..", "Docs", "a2.md", "a10.md", "b.txt"]);
  assert.deepEqual(names("time"), ["..", "Docs", "a10.md", "b.txt", "a2.md"], "newest first, none last");
  assert.deepEqual(names("size"), ["..", "Docs", "a2.md", "b.txt", "a10.md"], "biggest first");
  // As fs::tests::by_size_folders_go_by_their_measured_size: a folder by its measured size, the
  // ones not measured (or folder sizes off) last, by name, whichever way round.
  const d = (name, size) => ({ name, path: `/d/${name}`, is_dir: true, size });
  const all = [d("..", 4096), d("one", 4096), d("fifty", 4096), d("five-g", 4096), d("b-new", 9999), d("a-new", 0), { name: "x.txt", is_dir: false, size: 10 }, { name: "y.txt", is_dir: false, size: 20 }];
  const measured = { "/d/one": 2 ** 20, "/d/fifty": 50 * 2 ** 20, "/d/five-g": 5 * 2 ** 30 };
  const alpha = (w) => [...all.map((e) => e.name)].sort().indexOf(w);
  const bySize = (sizes, reverse) => sortItems(all, "size", reverse, sizes, alpha).map((e) => e.name);
  assert.deepEqual(bySize(measured, false), ["..", "five-g", "fifty", "one", "a-new", "b-new", "y.txt", "x.txt"]);
  assert.deepEqual(bySize(measured, true), ["..", "one", "fifty", "five-g", "a-new", "b-new", "x.txt", "y.txt"]);
  assert.deepEqual(bySize({}, false), ["..", "a-new", "b-new", "fifty", "five-g", "one", "y.txt", "x.txt"]);
});

test("inside: the folder itself and below it, not a sibling that starts the same", async () => {
  const { inside } = await pick("inside");
  assert.ok(inside("/a/b", "/a/b", "/"));
  assert.ok(inside("/a/b/c", "/a/b", "/"));
  assert.ok(!inside("/a/bc", "/a/b", "/"));
  assert.ok(inside("/a", "/", "/"));
  assert.ok(inside("C:\\x\\y", "C:\\", "\\"));
  assert.ok(!inside("C:\\xy", "C:\\x", "\\"));
});

test("isModel: model files, and a Core ML package only as a folder", async () => {
  const { isModel } = await pick("isModel");
  assert.ok(isModel({ name: "net.onnx", is_dir: false }));
  assert.ok(isModel({ name: "Lander.TFLITE", is_dir: false }));
  assert.ok(isModel({ name: "Thrust.mlpackage", is_dir: true }));
  assert.ok(!isModel({ name: "Thrust.mlpackage", is_dir: false }));
  assert.ok(!isModel({ name: "weights.pt", is_dir: false }));
  assert.ok(!isModel({ name: "onnx", is_dir: true }));
});

test("games files by their names", async () => {
  const { chessFormat } = await pick("chessFormat");
  assert.equal(chessFormat("Games.PGN.zst"), "pgn");
  assert.equal(chessFormat("mates.fen"), "epd");
  assert.equal(chessFormat("Shusaku.SGF"), "sgf");
  // A Polyglot book is known by its bytes, not its name.
  assert.equal(chessFormat("book.bin"), "");
});

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
  // An HDF5 file is a model only when its bytes say so: Preview asks.
  assert.ok(!isModel({ name: "survey.h5", is_dir: false }));
  assert.ok(!isModel({ name: "onnx", is_dir: true }));
});

test("games files by their names", async () => {
  const { chessFormat } = await pick("chessFormat");
  assert.equal(chessFormat("Games.PGN.zst"), "pgn");
  assert.equal(chessFormat("Club.si4"), "pgn");
  assert.equal(chessFormat("Club.si5"), "pgn");
  assert.equal(chessFormat("mates.fen"), "epd");
  assert.equal(chessFormat("Shusaku.SGF"), "sgf");
  assert.equal(chessFormat("club.PDN"), "pdn");
  assert.equal(chessFormat("meijin.kifu"), "shogi");
  assert.equal(chessFormat("ryuo.CSA"), "shogi");
  assert.equal(chessFormat("club.pbn"), "pbn");
  // A Polyglot book is known by its bytes, not its name.
  assert.equal(chessFormat("book.bin"), "");
});

test("without: one of two equal entries goes, the other stays", async () => {
  const { without } = await pick("without");
  assert.deepEqual(without(["/a", "/a", "/b"], 0), ["/a", "/b"]);
  assert.deepEqual(without(["*.log"], 0), []);
});

test("sameEntry: a new listing of an unchanged file is the same entry; any change is not", async () => {
  const { sameEntry } = await pick("sameEntry");
  const a = { name: "x.zip", path: "/d/x.zip", is_dir: false, size: 9, modified: 5, icon: { glyph: "z" } };
  assert.ok(sameEntry(a, { ...a, icon: { glyph: "z" } }) && sameEntry(null, null));
  assert.ok(!sameEntry(a, { ...a, icon: { glyph: "y" } }));
  assert.ok(!sameEntry(a, { ...a, size: 10 }) && !sameEntry(a, { ...a, modified: 6 }));
  assert.ok(!sameEntry(a, { ...a, online: true }) && !sameEntry(a, null) && !sameEntry(undefined, a));
});

test("looksLikeDhtml: a DhtmlXQ xiangqi game by its name and first bytes", async () => {
  const { looksLikeDhtml } = await pick("looksLikeDhtml");
  assert.ok(looksLikeDhtml("0001-对兵互进右马局", "﻿[DhtmlXQ]\n[DhtmlXQ_ver]x"));
  assert.ok(looksLikeDhtml("game.che", "  [DhtmlXQ_title]") && looksLikeDhtml("g.TXT", "[dhtmlxq]"));
  assert.ok(!looksLikeDhtml("main.rs", "[DhtmlXQ]") && !looksLikeDhtml("README", "# [DhtmlXQ]"));
  assert.ok(looksLikeDhtml("001-1.古谱的启示", "[DhtmlXQ]") && looksLikeDhtml("002.Opening game", "[DhtmlXQ]") && looksLikeDhtml(".hidden", "[DhtmlXQ]"));
});

test("viewEnd: the move shown keeps its variations and the other moves from its position in view", async () => {
  const { viewEnd } = await pick("viewEnd");
  // 2.O-O Nc6 (2…a5 3.bxa5) 3.Bb2 a6 {Controls b5} (3…g6) 4.Rfd1 1-0
  const m = { t: "move" };
  const L = [m, m, { t: "open" }, m, m, { t: "close" }, m, m, { t: "comment" }, { t: "open" }, m, { t: "close" }, m, { t: "result" }];
  assert.equal(viewEnd(L, 0), 5, "on 2.O-O: 2…Nc6 and its variation 2…a5, not 3.Bb2");
  assert.equal(viewEnd(L, 1), 6, "on 2…Nc6: its variation and 3.Bb2");
  assert.equal(viewEnd(L, 6), 11, "on 3.Bb2: 3…a6, its comment and its variation");
  assert.equal(viewEnd(L, 3), 5, "in a variation: up to its end");
  assert.equal(viewEnd(L, 12), 12, "the last move");
});

test("keyOwner: Esc in a game's list or board gives the keys back to the panels", async () => {
  const { keyOwner } = await pick("keyOwner");
  const inView = { closest: (s) => (s.includes(".chess-keys") ? {} : null) };
  const outside = { closest: () => null };
  // The game list in the preview pane kept every key but F1-F12: arrows and Esc did nothing
  // there with one game, and the panels never got them back.
  assert.equal(keyOwner(inView, "Down"), "view");
  assert.equal(keyOwner(inView, "Esc"), "panels");
  assert.equal(keyOwner(inView, "F3"), "theirs");
  assert.equal(keyOwner(outside, "Down"), "theirs");
  assert.equal(keyOwner(undefined, "Esc"), "theirs");
});

test("browseKey: an engine line stepped through, back to the game, played", async () => {
  const { browseKey } = await pick("browseKey");
  const b = { k: 1, steps: ["c5", "Nf3", "d6"], at: 1 };
  assert.deepEqual(browseKey(b, "ArrowRight"), { browse: { ...b, at: 2 }, used: true });
  assert.equal(browseKey({ ...b, at: 3 }, "ArrowRight").browse.at, 3, "→ stops at its last move");
  assert.equal(browseKey({ ...b, at: 3 }, "ArrowLeft").browse.at, 2);
  assert.deepEqual(browseKey(b, "ArrowLeft"), { browse: null, used: true }, "← on its first move: the game");
  assert.deepEqual(browseKey({ ...b, at: 2 }, "Escape"), { browse: null, used: true });
  assert.deepEqual(browseKey(b, "Enter"), { browse: b, used: "play" });
  // The board's own: kept for flip, notation and another line; ended for the rest.
  assert.deepEqual(browseKey(b, "f"), { browse: b, used: false });
  assert.deepEqual(browseKey(b, "3"), { browse: b, used: false });
  assert.deepEqual(browseKey(b, "Home"), { browse: null, used: false });
  assert.deepEqual(browseKey(b, "b"), { browse: null, used: false });
});

test("keysToBoard: a button clicked in a game's view gives the arrows back to the board", async () => {
  const { keysToBoard } = await pick("keysToBoard");
  const button = { closest: (s) => (s === "button" ? {} : null) };
  const square = { closest: () => null };
  // Engine, Analyse game, Flip, a move: clicked, the focus stayed on the button and ← → were dead.
  assert.equal(keysToBoard(button, 1), true);
  // Enter or Space on a focused button: it stays focused, Tab goes on from there.
  assert.equal(keysToBoard(button, 0), false);
  assert.equal(keysToBoard(square, 1), false);
  assert.equal(keysToBoard(undefined, 1), false);
});

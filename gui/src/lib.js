import { invoke, convertFileSrc } from "@tauri-apps/api/core";

import { t, num, i18n } from "./i18n.svelte.js";
import { isBomName, isProvenanceName } from "./sniff.js";

export { invoke, convertFileSrc };

const NAMED = {
  Enter: "Enter",
  Escape: "Esc",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  " ": "Space",
  ContextMenu: "Menu",
};

// With Alt/Ctrl held, `key` can be a composed character (macOS Option), so use the
// physical key for these.
const CODES = { Period: ".", Comma: ",", Minus: "-", Equal: "=", Slash: "/", Semicolon: ";", Space: "Space" };

/** A key pressed while an input method (Japanese, Korean, Chinese …) is still composing: it
 *  belongs to the composition, not to Coxswain. WebKit marks some of them only by keyCode 229. */
export const composing = (e) => e.isComposing || e.keyCode === 229;

/** The canonical key string, exactly as coxswain-core's `Key` displays it; "" while composing. */
export function keyString(e) {
  if (composing(e)) return "";
  let k = e.key;
  let shift = e.shiftKey;
  if (/^F\d{1,2}$/.test(k)) {
    // function key
  } else if (NAMED[k]) {
    k = NAMED[k];
  } else {
    if (e.altKey || e.ctrlKey) {
      if (/^Key[A-Z]$/.test(e.code)) k = e.code.slice(3).toLowerCase();
      else if (/^Digit\d$/.test(e.code)) k = e.code.slice(5);
      else if (CODES[e.code] && !shift) k = CODES[e.code];
    }
    if (k === "Space") {
      // Ctrl+Space
    } else if ([...k].length !== 1) {
      return null; // a lone modifier, dead key, ...
    } else if (/[a-z]/i.test(k)) {
      shift = shift || k !== k.toLowerCase();
      k = shift ? k.toUpperCase() : k.toLowerCase();
    } else {
      shift = false; // punctuation already says whether Shift was held
    }
  }
  return (e.ctrlKey ? "Ctrl+" : "") + (e.altKey ? "Alt+" : "") + (shift ? "Shift+" : "") + k;
}

/** Whether `path` is `folder` or below it; `folder` may end with `sep` (a root). */
export const inside = (path, folder, sep) => path === folder || path.startsWith(folder.endsWith(sep) ? folder : folder + sep);
/** `list` without its entry at `i`: one of two equal entries (config.toml may name one twice) goes, not both. */
export const without = (list, i) => list.filter((_, j) => j !== i);
export const basename = (p) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
export const parent = (p) => {
  const sep = p.includes("\\") && !p.includes("/") ? "\\" : "/";
  const i = p.replace(/[\\/]+$/, "").lastIndexOf(sep);
  if (i < 0) return null;
  return i === 0 ? sep : p.slice(0, i) + (/^[A-Za-z]:$/.test(p.slice(0, i)) ? sep : "");
};

/** Breadcrumb segments: [{ name, path }], root first. */
export function crumbs(p, home) {
  const out = [];
  let cur = p;
  while (cur) {
    out.unshift({ name: cur === home ? "~" : basename(cur) || cur, path: cur });
    if (cur === home) break;
    cur = parent(cur);
  }
  return out;
}

// Left-to-right isolate: keeps "338 B" and dates in order inside right-to-left (Hebrew) text.
const ltr = (s) => `\u2066${s}\u2069`;

/** Exact bytes below 10 KB, then one decimal. */
export function size(n) {
  const fmt = (v, digits, u) => ltr(`${sizeNum(v, digits)} ${t(`unit.${u}`)}`);
  if (n < 10_240) return fmt(n, 0, "B");
  let v = n;
  for (const u of ["KB", "MB", "GB", "TB", "PB"]) {
    v /= 1024;
    if (v < 1024) return fmt(v, v < 100 ? 1 : 0, u);
  }
  return fmt(v, 0, "EB");
}

// A size's number with no or one decimal: a row's size is drawn often, so the two formatters
// are made once per language.
let sz = { lang: "", f: [] };
function sizeNum(v, digits) {
  if (sz.lang !== i18n.lang) sz = { lang: i18n.lang, f: [0, 1].map((k) => new Intl.NumberFormat(i18n.lang, { minimumFractionDigits: k, maximumFractionDigits: k, useGrouping: false })) };
  return sz.f[digits].format(v);
}

// Two digits at least, in the language's own (Persian writes ۰۵); one formatter per language.
let two = { lang: "", f: null };
function pad2(n) {
  if (two.lang !== i18n.lang) two = { lang: i18n.lang, f: new Intl.NumberFormat(i18n.lang, { minimumIntegerDigits: 2, useGrouping: false }) };
  return two.f.format(n);
}

const ymd = (d) => `${pad2(d.getFullYear())}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;

export function date(secs) {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  return ltr(`${ymd(d)} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`);
}

/** The day alone, in the language's own digits. */
export function day(secs) {
  return secs ? ltr(ymd(new Date(secs * 1000))) : "";
}

const H = 3600;
const D = 24 * H;

/** Short relative age, as on the age chip: 5m, 3h, 2d, 4w, 8mo, 2y. */
export function age(secs) {
  if (!secs) return "";
  const s = Math.max(0, Date.now() / 1000 - secs);
  const a = (key, n) => t(`age.${key}`, { n: num(n) });
  if (s < H) return a("minutes", Math.max(1, Math.round(s / 60)));
  if (s < D) return a("hours", Math.round(s / H));
  if (s < 14 * D) return a("days", Math.round(s / D));
  if (s < 60 * D) return a("weeks", Math.round(s / (7 * D)));
  if (s < 365 * D) return a("months", Math.round(s / (30 * D)));
  return a("years", Math.round(s / (365 * D)));
}

/**
 * File age as heat: red within the hour, yellow within a day, green within a week, cyan
 * within a month, blue within a year, gray after. Hue moves on a log scale in between.
 */
export function ageColor(secs, look) {
  if (!secs) return "transparent";
  const s = Math.max(1, Date.now() / 1000 - secs);
  // Phosphor (the Cyber theme): no red. Cyan when new, fading through green to dim green.
  if (look === "crt") {
    const f = Math.min(1, Math.log(s) / Math.log(365 * D));
    return `hsl(${185 - 60 * f} 100% ${42 - 26 * f}%)`;
  }
  const stops = [
    [H, 0],
    [D, 50],
    [7 * D, 120],
    [30 * D, 185],
    [365 * D, 225],
  ];
  if (s >= 365 * D) return "hsl(220 8% 50%)";
  let [t0, h0] = [1, 0];
  for (const [t1, h1] of stops) {
    if (s <= t1) {
      const f = Math.log(s / t0) / Math.log(t1 / t0);
      return `hsl(${h0 + (h1 - h0) * Math.max(0, f)} 75% 50%)`;
    }
    [t0, h0] = [t1, h1];
  }
}

/** Shell-quote a word the way coxswain-core's `quote` does: in double quotes for Windows'
 *  `cmd /C` (`win`), in single quotes for a Unix shell. */
export const quote = (s, win = false) =>
  /^[A-Za-z0-9_\-./+,:@]+$/.test(s) ? s : win ? `"${s.replaceAll('"', '""')}"` : `'${s.replaceAll("'", "'\\''")}'`;

/** Theme slots become CSS variables: --panel-fg, --panel-bg, ... */
export function applyTheme(theme, gui, look = "modern", crt = false) {
  // Shapes and chrome per look, and the tube (scanlines, glow) on top: looks.css.
  document.documentElement.dataset.look = look;
  document.documentElement.toggleAttribute("data-crt", crt);
  const root = document.documentElement.style;
  for (const [slot, s] of Object.entries(theme)) {
    const name = slot.replaceAll("_", "-");
    s.fg ? root.setProperty(`--${name}-fg`, s.fg) : root.removeProperty(`--${name}-fg`);
    s.bg ? root.setProperty(`--${name}-bg`, s.bg) : root.removeProperty(`--${name}-bg`);
    root.setProperty(`--${name}-weight`, s.bold ? "600" : "normal");
  }
  // Light or dark, for native controls and scrollbars.
  const bg = theme.panel?.bg ?? "#000000";
  const lum = parseInt(bg.slice(1, 3), 16) * 0.3 + parseInt(bg.slice(3, 5), 16) * 0.59 + parseInt(bg.slice(5, 7), 16) * 0.11;
  root.setProperty("color-scheme", lum > 128 ? "light" : "dark");
  root.setProperty("--font", gui.font);
  root.setProperty("--icon-font", gui.icon_font);
  root.setProperty("--mono-font", gui.mono_font);
  root.setProperty("--font-size", `${gui.font_size}px`);
  root.setProperty("--row", `${Math.round(gui.font_size * gui.line_height)}px`);
}

/** Tag colour ids (stored; never translated). Show them with tagName(). */
export const TAGS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
export const tagName = (id) => t(`tag.${id}`);
export const TAG_COLORS = {
  red: "#ef5350",
  orange: "#ffa726",
  yellow: "#fdd835",
  green: "#66bb6a",
  blue: "#42a5f5",
  purple: "#ab47bc",
  gray: "#9e9e9e",
};

const IMAGE = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "avif"];
/** The old pictures the core reads and shows as a PNG it makes (`pictures::endings`): TIFF,
 *  TGA, Netpbm, DDS, HDR, OpenEXR, QOI, farbfeld, JPEG XL, IFF, PCX, XPM, XBM, Photoshop. Set
 *  once the config has come. */
const OLD = new Set();
export const setPictureEndings = (all) => (OLD.clear(), all.forEach((x) => OLD.add(x)));
export const isOldPicture = (name) => OLD.has(name.split(".").pop().toLowerCase());
/** What ffmpeg and ImageMagick show (`media::endings`): an ending's kind (`video_other`,
 *  `audio_other`, `heif`, `magick`, `raw`), and what each action takes. Set once the config
 *  has come. */
const MEDIA = new Map();
const MEDIA_TAKES = { media: new Set(), pictures: new Set() };
export function setMediaEndings(all) {
  MEDIA.clear();
  for (const kind of ["video_other", "audio_other", "heif", "magick", "raw"]) for (const x of all[kind] ?? []) MEDIA.set(x, kind);
  MEDIA_TAKES.media = new Set(["video", "video_other", "audio", "audio_other"].flatMap((k) => all[k] ?? []));
  MEDIA_TAKES.pictures = new Set(all.pictures ?? []);
}
const endingOfName = (name) => (name.includes(".") ? name.split(".").pop().toLowerCase() : "");
/** A video or a song *Convert video or audio* takes; a picture *Convert or edit pictures* takes. */
export const isMedia = (name) => MEDIA_TAKES.media.has(endingOfName(name));
export const isEditable = (name) => MEDIA_TAKES.pictures.has(endingOfName(name));
/** The media preview a kind gets its picture from (convert.rs `TOOLS`), for thumbnails. */
export const MEDIA_TOOL = { video: "ffmpeg", video_other: "ffmpeg", audio: "waveform", audio_other: "waveform", heif: "heif", magick: "imagemagick", raw: "raw", geo: "gdal", scad: "openscad", kicad: "kicad_thumb" };
/** Geodata (`geo::endings`): the vector endings, a map by GDAL (a GPX file's drawn without
 *  it); TIFF is geodata only when GDAL finds it is a GeoTIFF. Set once the config has come. */
const GEO = new Set();
export const setGeoEndings = (all) => (GEO.clear(), (all.vector ?? []).forEach((x) => GEO.add(x)));
/** A file *Convert geodata* may take: the backend leaves out a TIFF that is no GeoTIFF. */
export const isGeo = (name) => GEO.has(endingOfName(name)) || /\.tiff?$/i.test(name);
/** GeoJSON, KML and GPX are text: their preview shows the map or the source. */
export const isGeoText = (name) => /\.(geojson|kml|gpx)$/i.test(name);
/** An OpenSCAD model: *Export 3D model* takes it; its preview shows the model or the source. */
export const isScad = (name) => /\.scad$/i.test(name);
/** A KiCad schematic or board: *Export from KiCad* takes it. */
export const isKicad = (name) => /\.kicad_(sch|pcb)$/i.test(name);
/** A list of checksums, as `checksum::is_list` tells one. */
/** The first part of a split file, as `split::joins_into` tells one: `name.001`. */
export const isFirstPart = (name) => /.\.0*1$/.test(name) && /\.\d{3,}$/.test(name);
export const isSums = (name) => /^(SHA256SUMS|SHA512SUMS|B3SUMS)|\.(sha256|sha512|b3)$/i.test(name);
/** Tracker music: facts and sample names, no playback. */
const TRACKER = ["mod", "xm", "s3m", "it"];
const VIDEO = ["mp4", "webm", "mkv", "mov", "m4v", "ogv"];
const AUDIO = ["mp3", "flac", "wav", "ogg", "m4a", "opus", "aac"];
const FONT = ["ttf", "otf", "woff", "woff2"];

/** Kinds drawn in the browser (graphviz, asciidoc, parquet) or by an external tool (the rest). */
const CONVERTED = Object.fromEntries(
  Object.entries({
    graphviz: "dot gv",
    asciidoc: "adoc asciidoc",
    // OpenSCAD models, drawn from what OpenSCAD makes of them (convert.rs `openscad`).
    scad: "scad",
    // KiCad: a schematic's sheets or a board's layers, a page each (convert.rs `kicad`).
    kicad: "kicad_sch kicad_pcb",
    parquet: "parquet pq",
    latex: "tex ltx",
    typst: "typ",
    postscript: "ps eps",
    // What LibreOffice opens and the app cannot draw itself: Word and Works, Windows Write,
    // WordPerfect, Lotus 1-2-3 and WordPro, StarOffice, AbiWord, ClarisWorks, Apple's iWork,
    // flat OpenDocument, CorelDRAW, PageMaker, Publisher, Visio, and Windows clip art.
    office: "doc docm dot dotx odt ott rtf wri ppt pptx pps ppsx pot potx odp otp odg vsd vsdx pub wpd wps wk1 wks 123 lwp sxw sxc sxi sxd abw cwk pages key numbers fodt fods fodp fodg cdr pmd wmf emf",
    xps: "xps oxps",
    plantuml: "puml plantuml pu iuml wsd",
    // Drawn by the programs in Kroki's image (convert.rs `kroki`), or the four packaged on their own.
    d2: "d2",
    ditaa: "ditaa",
    svgbob: "bob svgbob",
    pikchr: "pikchr",
    nomnoml: "nomnoml noml",
    wavedrom: "wavedrom",
    vega: "vg vega",
    vegalite: "vl vegalite",
    dbml: "dbml",
    erd: "er erd",
    bytefield: "bytefield",
    rst: "rst rest",
    drawio: "drawio dio",
    duckdb: "duckdb ddb",
    // E-books calibre reads (`ebook`); a .pdb is one only by its header, so it is not here.
    ebook: "mobi azw azw3 fb2 lit",
  }).flatMap(([kind, exts]) => exts.split(" ").map((e) => [e, kind])),
);

/** Which external tool renders a kind (see convert.rs), and whether it is quick enough to run by itself.
 *  `verb` is a catalogue key for the button text: callers show it with t(conv.verb). */
export const CONVERTER = {
  // A build takes a while: by itself (the latex_auto setting) once the file has stayed selected.
  latex: { tool: "latex", auto: "latex_auto", wait: 800, verb: "convert.build_pdf" },
  // LibreOffice takes a few seconds: it starts once the file has been selected a moment.
  office: { tool: "libreoffice", auto: true, wait: 600, verb: "convert.render" },
  // Typst compiles in a blink, so it starts at once; Ghostscript too.
  typst: { tool: "typst", auto: true, verb: "convert.build_pdf" },
  postscript: { tool: "ghostscript", auto: true, verb: "convert.render" },
  xps: { tool: "gxps", auto: true, verb: "convert.render" },
  plantuml: { tool: "plantuml", auto: true, verb: "convert.render" },
  ...Object.fromEntries(["d2", "ditaa", "svgbob", "pikchr", "nomnoml", "wavedrom", "vega", "vegalite", "dbml", "erd", "bytefield"].map((k) => [k, { tool: k, auto: true, verb: "convert.render" }])),
  rst: { tool: "pandoc", auto: true, verb: "convert.render" },
  // ffmpeg and ImageMagick (`media`): a frame of a video no web view plays, a song's waveform,
  // HEIC, GIMP and RAW photos.
  video_other: { tool: "ffmpeg", auto: true, verb: "convert.render" },
  audio_other: { tool: "waveform", auto: true, verb: "convert.render" },
  heif: { tool: "heif", auto: true, verb: "convert.render" },
  magick: { tool: "imagemagick", auto: true, verb: "convert.render" },
  raw: { tool: "raw", auto: true, verb: "convert.render" },
  // GDAL's map of geodata's own shapes (`geo`).
  geo: { tool: "gdal", auto: true, verb: "convert.render" },
  // OpenSCAD's model, drawn from its STL (`scad`).
  scad: { tool: "openscad", auto: true, verb: "convert.render" },
  kicad: { tool: "kicad", auto: true, verb: "convert.render" },
  duckdb: { tool: "duckdb", auto: true, verb: "convert.read_tables" },
  // calibre makes the book an EPUB in a second or two, once it has stayed selected a moment.
  ebook: { tool: "calibre", auto: true, wait: 400, verb: "convert.render" },
};
const SHEET = ["csv", "tsv", "xlsx", "xlsm", "xlsb", "xls", "ods"];

/** Every archive ending, longest first, and those of archives that take a password: the
 *  core's (`archive::endings`), set once the config has come (`setArchiveEndings`). */
const archive = { all: [], locked: [] };
export const setArchiveEndings = ([all, locked]) => Object.assign(archive, { all, locked });
const endingOf = (list, name) => {
  const lower = name.toLowerCase();
  return list.find((e) => lower.endsWith(e));
};

export const isArchive = (name) => !!endingOf(archive.all, name);
/** The endings a disk image may have (`disk_image::endings`), set once the config has come.
 *  `.vhd` (VHDL too) and `.img` are disk images only when their content says so. */
const disk = { all: [] };
export const setDiskEndings = (all) => (disk.all = all);
export const diskName = (name) => !!endingOf(disk.all, name);
/** A disk image by its name alone. */
export const diskSure = (name) => diskName(name) && !/\.(vhd|img)$/i.test(name);
/** The `.vhd` and `.img` files the preview found to be disk images (`disk_image_facts`). */
export const diskSeen = new Set();
/** `name` with its archive ending (.tar.gz whole) swapped for `ending`, or `ending` added. */
export const withEnding = (name, ending) => name.slice(0, name.length - (endingOf(archive.all, name)?.length ?? 0)) + ending;
/** The pack format (of `formats`, from the core) whose ending `name` has, if any. */
export const packFormat = (formats, name) => formats.find((f) => f.endings.some((e) => name.toLowerCase().endsWith(e)));
/** Whether an archive by this name can have a password: a zip or a 7z (archive::takes_password). */
export const takesPassword = (name) => !!endingOf(archive.locked, name);
/** The path segment that leads into a file's or folder's git history (history::MARKER). */
export const HISTORY = "@history";
/** The segments that lead to a repository's branches and worktrees (history::BRANCHES, WORKTREES). */
export const BRANCHES = "@branches";
export const WORKTREES = "@worktrees";
/** After `@history`: the commits of every branch. */
export const ALL = "@all";
/** After `@history`: the commits not pushed and not merged. */
export const UPSTREAM = "@upstream";
export const MARKERS = [HISTORY, BRANCHES, WORKTREES, ALL, UPSTREAM];
/** The segment after a folder that lists its ZFS dataset's snapshots (zfs::MARKER), and the one
 *  after a file that lists its package's files (bsd::PACKAGE). */
export const SNAPSHOTS = "@snapshots";
export const PACKAGE = "@package";
export const isHistory = (p) => p.split(/[\\/]/).some((s) => MARKERS.includes(s));
/** What a history path is of: the part before the marker. */
export const historyOf = (p) => p.slice(0, p.search(/[\\/]@history([\\/]|$)/));
/** What the core says when an archive is locked and wants its password (archive::LOCKED). */
export const LOCKED = "locked: a password is needed";

export { looksLikeBom, looksLikeProvenance } from "./sniff.js";

/** How the preview pane should show a file. */
/** The last token of a game's `layout` to keep in view with the move at token `i`: the move,
 *  the variations that replace it, the next move and its variations, with their comments
 *  (`chess::pgn::view_end`, which the terminal app uses). */
export function viewEnd(layout, i) {
  let depth = 0;
  let moves = 0;
  let end = i;
  for (let j = i + 1; j < layout.length; j++) {
    const t = layout[j].t;
    if (t === "open") depth++;
    else if (t === "close" && depth === 0) return j;
    else if (t === "close") depth--;
    else if (t === "move" && depth === 0 && ++moves === 2) return end;
    else if (t === "result") return end;
    end = j;
  }
  return end;
}

/** What the panels do with a key pressed in a view that has keys of its own (a game's board or
 *  list, the BOM tree): "theirs" when it is not in one or is a function key, "panels" for Esc,
 *  which gives the keys back to the panels (blurring the view), "view" for the rest. */
export function keyOwner(el, k) {
  if (!el?.closest?.(".bom-keys, .chess-keys") || /^F\d+$/.test(k)) return "theirs";
  return k === "Esc" ? "panels" : "view";
}

/** A click in a game's view (chess, Go, draughts, shogi, xiangqi, bridge) that gives the keys
 *  back to its board: one with the mouse (`detail` > 0) on a button there (Engine, Analyse game,
 *  Flip, a move …). A button clicked kept the focus, and ← → ↑ ↓ no longer stepped the game.
 *  Enter or Space on a button (`detail` 0) leaves the focus on it, so Tab goes on from there. */
export function keysToBoard(target, detail) {
  return detail > 0 && !!target?.closest?.("button");
}

/** A key on the chess board while one of the engine's lines is stepped through (`b`: { k, steps,
 *  at }, `at` moves of `steps` shown): → ← step it, ← on its first move or Esc go back to the
 *  game (null), Enter plays it up to the move shown ("play"). f, n and 1–5 keep it and are
 *  the board's; any other key ends it and is the board's too. (`chess.rs`'s `game_key`.) */
export function browseKey(b, key) {
  if (key === "ArrowRight") return { browse: { ...b, at: Math.min(b.at + 1, b.steps.length) }, used: true };
  if (key === "ArrowLeft" && b.at > 1) return { browse: { ...b, at: b.at - 1 }, used: true };
  if (key === "ArrowLeft" || key === "Escape") return { browse: null, used: true };
  if (key === "Enter") return { browse: b, used: "play" };
  return { browse: ["f", "n", "1", "2", "3", "4", "5"].includes(key) ? b : null, used: false };
}

/** A games file by its name: "pgn" (also packed, a SCID database's .si4 or .si5 and a ChessBase database's .cbh), "epd" (EPD and FEN), "sgf" (Go), "pdn"
 *  (draughts), "shogi" (KIF, CSA), "pbn" (bridge), or ""
 *  (`chess::format`). A Polyglot book (.bin) is known by its bytes: `chess_is_book`. */
export function chessFormat(name) {
  const n = name.toLowerCase();
  if (/\.(pgn(\.gz|\.zst)?|si[45]|cbh|cbv)$/.test(n)) return "pgn";
  if (/\.(epd|fen)$/.test(n)) return "epd";
  if (n.endsWith(".sgf")) return "sgf";
  if (n.endsWith(".pdn")) return "pdn";
  if (/\.(kif|kifu|csa)$/.test(n)) return "shogi";
  if (n.endsWith(".pbn")) return "pbn";
  return "";
}

/** A DhtmlXQ xiangqi game (`games::is_dhtml`): a name such files may have (`games::dhtml_name`:
 *  no ending, `.che`, `.txt`, `.dhtmlxq`, or an "ending" that is no file type's but part of the
 *  name, not 1 to 8 ASCII letters and digits) and text that starts with `[DhtmlXQ`. */
export const looksLikeDhtml = (name, text) => (!/.\.[a-z0-9]{1,8}$/i.test(name) || /\.(che|txt|dhtmlxq)$/i.test(name)) && /^\uFEFF?\s*\[DhtmlXQ/i.test(text);

/** A draw.io diagram by its name (`export::is_drawio`): `.drawio`, `.dio`, or a picture with
 *  the diagram in it, `.drawio.svg` and `.drawio.png`. */
export const isDrawio = (name) => /\.(drawio|dio|drawio\.svg|drawio\.png)$/i.test(name);

/** A neural network model Netron draws (`neural::named`); a Core ML package is a folder. A `.h5`
 *  is one only when its bytes hold a Keras model (`is_model`, in Preview). */
export const isModel = (item) => (item.is_dir ? /\.mlpackage$/i : /\.(onnx|tflite|mlmodel|keras)$/i).test(item.name);

/** Whether two listed entries say the same of the same file: every field alike, its icon too (a
 *  new listing of an unchanged file). A changed size, date or anything else makes them differ. */
export function sameEntry(a, b) {
  return a === b || (!!a && !!b && JSON.stringify(a) === JSON.stringify(b));
}

export function previewKind(item) {
  if (!item) return "none";
  if (isModel(item)) return "netron";
  if (item.is_dir) return "folder";
  if (isDrawio(item.name)) return "drawio";
  if (chessFormat(item.name)) return chessFormat(item.name) === "sgf" ? "go" : "chess";
  if (isProvenanceName(item.name)) return "provenance";
  if (isBomName(item.name)) return "bom";
  // A comic is a zip of pictures: shown page by page (Enter still opens it like a folder).
  if (/\.cbz$/i.test(item.name)) return "comic";
  if (isArchive(item.name)) return "archive";
  // A .vhd or .img that turns out to be none is shown as text instead (Preview).
  if (diskName(item.name)) return "disk_image";
  // A local model: its facts (`local_models::facts`), not a hex dump. A pickle is named, never read.
  if (/\.(gguf|safetensors|pt|pth|ckpt)$|^pytorch_model.*\.bin$/i.test(item.name)) return "model";
  const ext = item.name.includes(".") ? item.name.split(".").pop().toLowerCase() : "";
  if (ext === "pdf") return "pdf";
  // Geodata: a map of its own shapes, a GPX file's drawn without GDAL.
  if (GEO.has(ext)) return ext === "gpx" ? "gpx" : "geo";
  if (FONT.includes(ext)) return "font";
  if (SHEET.includes(ext)) return "sheet";
  if (ext === "docx") return "docx";
  if (ext === "ipynb") return "notebook";
  if (ext === "mmd" || ext === "mermaid") return "mermaid";
  if (ext === "html" || ext === "htm" || ext === "xhtml") return "html";
  const special = { db: "sqlite", sqlite: "sqlite", sqlite3: "sqlite", db3: "sqlite", epub: "epub", pem: "cert", crt: "cert", cer: "cert", der: "cert", eml: "mail", msg: "mail", dbf: "sqlite", plist: "plist", ics: "calendar", vcf: "contacts", jsonl: "jsonl", ndjson: "jsonl", json: "data", geojson: "data", yaml: "data", yml: "data", toml: "data", log: "log" };
  if (special[ext]) return special[ext];
  if (CONVERTED[ext]) return CONVERTED[ext];
  if (MEDIA.has(ext)) return MEDIA.get(ext);
  if (IMAGE.includes(ext)) return "image";
  // A .hdr or .mod that is none (another program's header, Modula-2) is shown as text (Preview).
  if (OLD.has(ext)) return "picture";
  if (TRACKER.includes(ext)) return "tracker";
  if (VIDEO.includes(ext)) return "video";
  if (AUDIO.includes(ext)) return "audio";
  if (ext === "md" || ext === "markdown") return "markdown";
  return "text";
}

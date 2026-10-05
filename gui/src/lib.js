import { invoke, convertFileSrc } from "@tauri-apps/api/core";

import { t, num } from "./i18n.svelte.js";

export { invoke, convertFileSrc };

/** A folder's listing. An entry whose path is the folder's and its name comes as the name
 *  only (a third less JSON for a big folder); here it gets its path again. */
export async function listDir(args) {
  const r = await invoke("list_dir", args);
  for (const e of r.items) e.path ??= r.prefix + e.name;
  return r;
}

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
  const fmt = (v, digits, u) => ltr(`${num(v, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false })} ${t(`unit.${u}`)}`);
  if (n < 10_240) return fmt(n, 0, "B");
  let v = n;
  for (const u of ["KB", "MB", "GB", "TB", "PB"]) {
    v /= 1024;
    if (v < 1024) return fmt(v, v < 100 ? 1 : 0, u);
  }
  return fmt(v, 0, "EB");
}

export function date(secs) {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const p = (n) => String(n).padStart(2, "0");
  return ltr(`${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`);
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

/** `*` and `?` wildcards, whole name, case-insensitive. */
export function glob(pattern, name) {
  const re = pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  // `u`: `?` is one character, also outside the basic plane (an emoji).
  return new RegExp(`^${re}$`, "iu").test(name);
}

/** Shell-quote a word the way coxswain-core's `quote` does on Unix. */
export const quote = (s) => (/^[\w\-./+,:@]+$/.test(s) ? s : `'${s.replaceAll("'", "'\\''")}'`);

/** Theme slots become CSS variables: --panel-fg, --panel-bg, ... */
export function applyTheme(theme, gui, look = "modern") {
  // Shapes and chrome per look: looks.css.
  document.documentElement.dataset.look = look;
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
const VIDEO = ["mp4", "webm", "mkv", "mov", "m4v", "ogv"];
const AUDIO = ["mp3", "flac", "wav", "ogg", "m4a", "opus", "aac"];
const FONT = ["ttf", "otf", "woff", "woff2"];

/** Kinds drawn in the browser (graphviz, asciidoc, parquet) or by an external tool (the rest). */
const CONVERTED = Object.fromEntries(
  Object.entries({
    graphviz: "dot gv",
    asciidoc: "adoc asciidoc",
    parquet: "parquet pq",
    latex: "tex ltx",
    office: "doc docm dotx odt ott rtf ppt pptx pps ppsx pot potx odp otp odg vsd vsdx pub wpd wps",
    plantuml: "puml plantuml pu iuml wsd",
    rst: "rst rest",
    drawio: "drawio dio",
    duckdb: "duckdb ddb",
  }).flatMap(([kind, exts]) => exts.split(" ").map((e) => [e, kind])),
);

/** Which external tool renders a kind (see convert.rs), and whether it is quick enough to run by itself.
 *  `verb` is a catalogue key for the button text: callers show it with t(conv.verb). */
export const CONVERTER = {
  // A build takes a while: by itself (the latex_auto setting) once the file has stayed selected.
  latex: { tool: "latex", auto: "latex_auto", wait: 800, verb: "convert.build_pdf" },
  // LibreOffice takes a few seconds: it starts once the file has been selected a moment.
  office: { tool: "libreoffice", auto: true, wait: 600, verb: "convert.render" },
  plantuml: { tool: "plantuml", auto: true, verb: "convert.render" },
  rst: { tool: "pandoc", auto: true, verb: "convert.render" },
  duckdb: { tool: "duckdb", auto: true, verb: "convert.read_tables" },
};
const SHEET = ["csv", "tsv", "xlsx", "xlsm", "xls", "ods"];

const ARCHIVE = /\.(zip|jar|apk|nupkg|whl|vsix|tar|tgz|tar\.gz|tar\.bz2|tbz2?|tar\.xz|txz|tar\.zst|tzst|7z)$/i;

export const isArchive = (name) => ARCHIVE.test(name);
/** `name` with its archive ending (.tar.gz whole) swapped for `ending`, or `ending` added. */
export const withEnding = (name, ending) => name.replace(ARCHIVE, "") + ending;
/** The pack format (of `formats`, from the core) whose ending `name` has, if any. */
export const packFormat = (formats, name) => formats.find((f) => f.endings.some((e) => name.toLowerCase().endsWith(e)));
/** Whether an archive by this name can have a password: a zip or a 7z (archive::takes_password). */
export const takesPassword = (name) => /\.(zip|jar|apk|nupkg|whl|vsix|7z)$/i.test(name);
/** The path segment that leads into a file's or folder's git history (history::MARKER). */
export const HISTORY = "@history";
/** The segments that lead to a repository's branches and worktrees (history::BRANCHES, WORKTREES). */
export const BRANCHES = "@branches";
export const WORKTREES = "@worktrees";
export const MARKERS = [HISTORY, BRANCHES, WORKTREES];
export const isHistory = (p) => p.split(/[\\/]/).some((s) => MARKERS.includes(s));
/** What a history path is of: the part before the marker. */
export const historyOf = (p) => p.slice(0, p.search(/[\\/]@history([\\/]|$)/));
/** What the core says when an archive is locked and wants its password (archive::LOCKED). */
export const LOCKED = "locked: a password is needed";

/** Names that say CycloneDX BOM. Other JSON and XML files are recognised by their first bytes (looksLikeBom). */
const BOM_NAME = /(\.(cdx|cbom)\.(json|xml)|^bom\.(json|xml))$/i;

/** Whether the start of a JSON or XML file is a CycloneDX BOM's (as coxswain-core's bom::sniff_head). */
export function looksLikeBom(text) {
  const head = text.slice(0, 8192);
  return (head.includes('"bomFormat"') && head.includes('"CycloneDX"')) || head.includes("http://cyclonedx.org/schema/bom/");
}

/** How the preview pane should show a file. */
export function previewKind(item) {
  if (!item) return "none";
  if (item.is_dir) return "folder";
  if (BOM_NAME.test(item.name)) return "bom";
  if (isArchive(item.name)) return "archive";
  const ext = item.name.includes(".") ? item.name.split(".").pop().toLowerCase() : "";
  if (ext === "pdf") return "pdf";
  if (FONT.includes(ext)) return "font";
  if (SHEET.includes(ext)) return "sheet";
  if (ext === "docx") return "docx";
  if (ext === "ipynb") return "notebook";
  if (ext === "mmd" || ext === "mermaid") return "mermaid";
  if (ext === "html" || ext === "htm" || ext === "xhtml") return "html";
  const special = { db: "sqlite", sqlite: "sqlite", sqlite3: "sqlite", db3: "sqlite", epub: "epub", pem: "cert", crt: "cert", cer: "cert", der: "cert", eml: "mail", plist: "plist", ics: "calendar", vcf: "contacts", jsonl: "jsonl", ndjson: "jsonl", json: "data", geojson: "data", yaml: "data", yml: "data", toml: "data", log: "log" };
  if (special[ext]) return special[ext];
  if (CONVERTED[ext]) return CONVERTED[ext];
  if (IMAGE.includes(ext)) return "image";
  if (VIDEO.includes(ext)) return "video";
  if (AUDIO.includes(ext)) return "audio";
  if (ext === "md" || ext === "markdown") return "markdown";
  return "text";
}

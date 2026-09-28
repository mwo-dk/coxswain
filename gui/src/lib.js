import { invoke, convertFileSrc } from "@tauri-apps/api/core";

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
};

// With Alt/Ctrl held, `key` can be a composed character (macOS Option), so use the
// physical key for these.
const CODES = { Period: ".", Comma: ",", Minus: "-", Equal: "=", Slash: "/", Semicolon: ";", Space: "Space" };

/** The canonical key string, exactly as bosum-core's `Key` displays it. */
export function keyString(e) {
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

/** Exact bytes below 10 KB, then one decimal. */
export function size(n) {
  if (n < 10_240) return `${n} B`;
  let v = n;
  for (const u of ["KB", "MB", "GB", "TB", "PB"]) {
    v /= 1024;
    if (v < 1024) return `${v < 100 ? v.toFixed(1) : v.toFixed(0)} ${u}`;
  }
  return `${v.toFixed(0)} EB`;
}

export function date(secs) {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

const H = 3600;
const D = 24 * H;

/** Short relative age, as on the age chip: 5m, 3h, 2d, 4w, 8mo, 2y. */
export function age(secs) {
  if (!secs) return "";
  const s = Math.max(0, Date.now() / 1000 - secs);
  if (s < H) return `${Math.max(1, Math.round(s / 60))}m`;
  if (s < D) return `${Math.round(s / H)}h`;
  if (s < 14 * D) return `${Math.round(s / D)}d`;
  if (s < 60 * D) return `${Math.round(s / (7 * D))}w`;
  if (s < 365 * D) return `${Math.round(s / (30 * D))}mo`;
  return `${Math.round(s / (365 * D))}y`;
}

/**
 * File age as heat: red within the hour, yellow within a day, green within a week, cyan
 * within a month, blue within a year, gray after. Hue moves on a log scale in between.
 */
export function ageColor(secs) {
  if (!secs) return "transparent";
  const s = Math.max(1, Date.now() / 1000 - secs);
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
  return new RegExp(`^${re}$`, "i").test(name);
}

/** Shell-quote a word the way bosum-core's `quote` does on Unix. */
export const quote = (s) => (/^[\w\-./+,:@]+$/.test(s) ? s : `'${s.replaceAll("'", "'\\''")}'`);

/** Theme slots become CSS variables: --panel-fg, --panel-bg, ... */
export function applyTheme(theme, gui) {
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

export const TAGS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
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
const SHEET = ["csv", "tsv", "xlsx", "xlsm", "xls", "ods"];

const ARCHIVE = /\.(zip|jar|apk|nupkg|whl|vsix|tar|tgz|tar\.gz)$/i;

export const isArchive = (name) => ARCHIVE.test(name);

/** How the preview pane should show a file. */
export function previewKind(item) {
  if (!item) return "none";
  if (item.is_dir) return "folder";
  if (isArchive(item.name)) return "archive";
  const ext = item.name.includes(".") ? item.name.split(".").pop().toLowerCase() : "";
  if (ext === "pdf") return "pdf";
  if (FONT.includes(ext)) return "font";
  if (SHEET.includes(ext)) return "sheet";
  if (ext === "docx") return "docx";
  if (ext === "ipynb") return "notebook";
  if (ext === "mmd" || ext === "mermaid") return "mermaid";
  if (IMAGE.includes(ext)) return "image";
  if (VIDEO.includes(ext)) return "video";
  if (AUDIO.includes(ext)) return "audio";
  if (ext === "md" || ext === "markdown") return "markdown";
  return "text";
}

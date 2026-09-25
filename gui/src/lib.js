import { invoke } from "@tauri-apps/api/core";

export { invoke };

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
const CODES = { Period: ".", Comma: ",", Minus: "-", Equal: "=", Slash: "/", Semicolon: ";" };

/** The canonical key string, exactly as coxswain-core's `Key` displays it. */
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
    if ([...k].length !== 1) return null; // a lone modifier, dead key, ...
    if (/[a-z]/i.test(k)) {
      shift = shift || k !== k.toLowerCase();
      k = shift ? k.toUpperCase() : k.toLowerCase();
    } else {
      shift = false; // punctuation already says whether Shift was held
    }
  }
  return (e.ctrlKey ? "Ctrl+" : "") + (e.altKey ? "Alt+" : "") + (shift ? "Shift+" : "") + k;
}

export const sep = (p) => (p.includes("\\") && !p.includes("/") ? "\\" : "/");
export const basename = (p) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
export const parent = (p) => {
  const s = sep(p);
  const i = p.replace(/[\\/]+$/, "").lastIndexOf(s);
  if (i < 0) return null;
  return i === 0 ? s : p.slice(0, i) + (/^[A-Za-z]:$/.test(p.slice(0, i)) ? s : "");
};

export function size(n) {
  if (n < 100_000_000) return n.toLocaleString();
  let v = n;
  for (const u of ["K", "M", "G", "T", "P"]) {
    v /= 1024;
    if (v < 10_000) return `${v.toFixed(0)}${u}`;
  }
  return `${v.toFixed(0)}E`;
}

export function date(secs) {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** `*` and `?` wildcards, whole name, case-insensitive. */
export function glob(pattern, name) {
  const re = pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  return new RegExp(`^${re}$`, "i").test(name);
}

/** Theme slots from the config become CSS variables: --panel-fg, --panel-bg, ... */
export function applyTheme(theme, gui) {
  const root = document.documentElement.style;
  for (const [slot, s] of Object.entries(theme)) {
    const name = slot.replaceAll("_", "-");
    s.fg ? root.setProperty(`--${name}-fg`, s.fg) : root.removeProperty(`--${name}-fg`);
    s.bg ? root.setProperty(`--${name}-bg`, s.bg) : root.removeProperty(`--${name}-bg`);
    root.setProperty(`--${name}-weight`, s.bold ? "bold" : "normal");
  }
  root.setProperty("--font", gui.font);
  root.setProperty("--font-size", `${gui.font_size}px`);
  root.setProperty("--row", `${gui.font_size * gui.line_height}px`);
}

// The treemap's hit tests, for Space.svelte: the rectangles come from coxswain-core's
// `space::Tree::layout`, parents before what is in them.

const inside = (r, x, y) => x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h;

/** The deepest rectangle at (x, y), its index; with `files`, folders are passed over. -1 when none. */
export function at(tiles, x, y, files = false) {
  for (let i = tiles.length - 1; i >= 0; i--) {
    const r = tiles[i];
    if ((!files || r.kind !== "folder") && inside(r, x, y)) return i;
  }
  return -1;
}

/** The rectangle next to `i` that way (dx, dy: -1, 0 or 1), never a folder; `i` when there is none. */
export function step(tiles, i, dx, dy) {
  const r = tiles[i];
  if (!r) return tiles.findIndex((t) => t.kind !== "folder");
  const [cx, cy] = [r.x + r.w / 2, r.y + r.h / 2];
  // Past the frames and title strips of folders between the two.
  for (let d = 0.5; d < 64; d += 2) {
    const x = dx > 0 ? r.x + r.w + d : dx < 0 ? r.x - d : cx;
    const y = dy > 0 ? r.y + r.h + d : dy < 0 ? r.y - d : cy;
    const j = at(tiles, x, y, true);
    if (j >= 0) return j;
  }
  return i;
}

/** The folders around tile `i`, outermost first: the folder chain a hover lights. */
export function chain(tiles, i) {
  const r = tiles[i];
  if (!r) return [];
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  return tiles.flatMap((t, j) => (j !== i && t.kind === "folder" && t.depth < r.depth && inside(t, cx, cy) ? [j] : []));
}

/** `s` as it fits `max` (a width, measured by `width`): whole, or cut with … keeping at least
 *  `keep` characters; "" when even that does not fit, so no crumb like "06 Tr…" is drawn. */
export function fit(s, max, width, keep = 8) {
  if (width(s) <= max) return s;
  let lo = 0;
  let hi = s.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (width(s.slice(0, mid) + "…") <= max) lo = mid;
    else hi = mid - 1;
  }
  return lo >= Math.min(keep, s.length - 1) ? s.slice(0, lo) + "…" : "";
}

// ------------------------------------------------------------ colours

const hex = (c) => {
  const n = parseInt(c.slice(1), 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
};
const css = ([r, g, b]) => "#" + [r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
/** `a` mixed with `b`: `k` of `b` (0 to 1). Both `#rrggbb`. */
export const mix = (a, b, k) => css(hex(a).map((v, i) => v + (hex(b)[i] - v) * k));
export const luma = (c) => {
  const [r, g, b] = hex(c);
  return 0.299 * r + 0.587 * g + 0.114 * b;
};
const far = (a, b) => Math.hypot(...hex(a).map((v, i) => v - hex(b)[i])) >= 70;

/**
 * Each family's colour from the theme's own: `theme` holds its colours as `#rrggbb` (red,
 * orange, yellow, green, blue, magenta, dir, dim, fg, bg; any may be missing), `fallback` the
 * core's fixed ones. Kinds a theme gives the same colour (a phosphor's greens, Classic blue's
 * two yellows) are told apart by lightness, so no two families look alike.
 */
export function palette(theme, fallback) {
  const c = (name, f) => theme[name] ?? fallback[f];
  const between = (a, fa, b, fb) => mix(c(a, fa), c(b, fb), 0.5);
  const wanted = {
    image: c("magenta", "image"),
    video: c("red", "video"),
    audio: c("orange", "audio"),
    archive: c("yellow", "archive"),
    document: c("blue", "document"),
    code: c("green", "code"),
    data: c("dir", "data"),
    program: between("red", "video", "magenta", "image"),
    disk: between("orange", "audio", "dim", "other"),
    other: c("dim", "other"),
  };
  const dark = luma(theme.bg ?? "#000000") < 128;
  const out = {};
  for (const [family, colour] of Object.entries(wanted)) {
    let pick = colour;
    // Lighter, then darker, in steps, until it stands apart from those already chosen.
    for (let step = 1; step <= 6 && Object.values(out).some((o) => !far(o, pick)); step++) {
      const toward = (step % 2 === 1) === dark ? "#ffffff" : "#000000";
      pick = mix(colour, toward, 0.18 * Math.ceil(step / 2));
    }
    out[family] = pick;
  }
  return out;
}

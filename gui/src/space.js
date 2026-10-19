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

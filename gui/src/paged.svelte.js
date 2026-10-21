// A tab's listing as the page holds it: the backend (gui/src-tauri/src/tabs.rs) has the whole
// of it, the page only the rows around what is in view and the count. Views read it like an
// array (`length`, `at`, `slice`); a slice of rows not here yet asks for them, and shows empty
// rows (`placeholder`) until they come.

import { invoke } from "./lib.js";

/** Rows asked for beyond what is in view, on each side. */
export const SPAN = 300;

export class Paged {
  /** Rows in the listing. */
  total = $state(0);
  /** Index of `rows[0]`. */
  from = $state(0);
  rows = $state.raw([]);
  /** Counts the listings: a view brings its cursor into view when a new one comes. */
  listed = $state(0);

  /** `id`: the tab's id, the backend's key. `onrows(rows)`: told of every row that comes. */
  constructor(id, onrows = () => {}) {
    this.id = id;
    this.onrows = onrows;
    /** The number of the listing the backend has for the tab (`tab_list`, `tab_sort`). */
    this.generation = 0;
    /** The rows being asked for, [from, to). */
    this.asking = null;
  }

  get length() {
    return this.total;
  }

  at(i) {
    return this.rows[i - this.from];
  }

  /** A new listing (`tab_list`, `tab_sort`): its count and rows. */
  reset(r) {
    this.total = r.total;
    this.take(r.rows);
    this.listed++;
  }

  take({ from, prefix, items }) {
    for (const e of items) e.path ??= prefix + e.name;
    this.onrows(items);
    this.from = from;
    this.rows = items;
  }

  /** Rows `a` to `b` (not included), those not here yet as placeholders, which are asked for. */
  slice(a, b = this.total) {
    [a, b] = [Math.max(0, a), Math.min(this.total, b)];
    const out = [];
    let missing = false;
    for (let i = a; i < b; i++) {
      const e = this.rows[i - this.from];
      if (!e) missing = true;
      out.push(e ?? { name: "", path: `\0${i}`, icon: { glyph: "" }, placeholder: true });
    }
    if (missing) this.want(a, b);
    return out;
  }

  /** Make sure rows `a` to `b` come (with a margin), unless they are being asked for. */
  want(a, b) {
    if (this.asking && this.asking[0] <= a && b <= this.asking[1]) return;
    const range = [Math.max(0, a - SPAN), b + SPAN];
    this.asking = range;
    const generation = this.generation;
    invoke("tab_rows", { tab: this.id, generation, from: range[0], to: range[1] }).then(
      (r) => {
        if (this.asking === range) this.asking = null;
        if (r && generation === this.generation) this.take(r);
      },
      () => this.asking === range && (this.asking = null),
    );
  }

  /** The rows here read again (their marks changed). */
  refresh() {
    if (!this.rows.length) return;
    const generation = this.generation;
    invoke("tab_rows", { tab: this.id, generation, from: this.from, to: this.from + this.rows.length }).then((r) => r && generation === this.generation && this.take(r), () => {});
  }

  /** The same rows without the first `n` (the current folder's `..` in a Miller column). */
  skip(n) {
    if (!n) return this;
    const me = this;
    return {
      get length() {
        return Math.max(0, me.total - n);
      },
      get listed() {
        return me.listed;
      },
      at: (i) => me.at(i + n),
      slice: (a, b = me.total - n) => me.slice(a + n, b + n),
    };
  }
}

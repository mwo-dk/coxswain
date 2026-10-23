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
    /** The number of the latest listing asked for (`tab_list`, `tab_sort`), and of the one
     *  shown: rows and marks are asked of that one. */
    this.generation = 0;
    this.shown = 0;
    /** The generation of a `tab_list` under way, else 0. */
    this.loading = 0;
    /** The rows being asked for, [from, to), and the last asked for by a view. */
    this.asking = null;
    this.view = [0, 0];
  }

  get length() {
    return this.total;
  }

  at(i) {
    return this.rows[i - this.from];
  }

  /** A new listing (`tab_list`, `tab_sort`) number `generation`: its count and rows. */
  reset(r, generation) {
    this.shown = generation;
    this.asking = null;
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

  /** Rows that came again (their marks changed) put in place of the ones here. */
  patch({ from, prefix, items }) {
    const rows = [...this.rows];
    items.forEach((e, k) => {
      e.path ??= prefix + e.name;
      if (rows[from - this.from + k]) rows[from - this.from + k] = e;
    });
    this.rows = rows;
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
    this.view = [a, b];
    return out;
  }

  /** Make sure rows `a` to `b` come (with a margin), unless they are being asked for. */
  want(a, b) {
    if (this.asking && this.asking[0] <= a && b <= this.asking[1]) return;
    const range = [Math.max(0, a - SPAN), b + SPAN];
    this.asking = range;
    const generation = this.shown;
    invoke("tab_rows", { tab: this.id, generation, from: range[0], to: range[1] }).then(
      (r) => {
        if (this.asking === range) this.asking = null;
        if (r && generation === this.shown) this.take(r);
      },
      () => this.asking === range && (this.asking = null),
    );
  }

  /** The rows in view read again (their marks changed). */
  refresh() {
    const generation = this.shown;
    const [a, b] = this.view;
    invoke("tab_rows", { tab: this.id, generation, from: Math.max(a, this.from), to: Math.min(b, this.from + this.rows.length) }).then((r) => r && generation === this.shown && this.patch(r), () => {});
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

<script>
  import { untrack } from "svelte";
  // A scrolling list that keeps only the rows in view in the DOM, so a folder of 100,000
  // files costs what a screenful does: the rest is empty space of the right height. Every
  // row is exactly `rowH` pixels high. `cursor` is kept in view, as scrollIntoView would.
  /** @type {{ items: any[], rowH: number, cursor?: number, top?: number, row: import("svelte").Snippet<[any, number]>, [k: string]: any }} */
  // `top` (how far it is scrolled) can be bound, so a tab keeps its place while another shows.
  let { items, rowH, cursor = -1, top = $bindable(0), row, ...rest } = $props();
  let el = $state();
  let height = $state(0);
  /** Rows rendered beyond the edges, so a scroll never shows a gap before the next render. */
  const OVER = 8;
  /** The most pixels the scrolling box is made: WebKit lays out up to about 2^25. */
  const MAX = 2 ** 24;
  // Held to the list: when it gets shorter while scrolled far down, the rows at its end show
  // (and the browser, its content shorter, scrolls up and tells `top`).
  // A list taller than WebKit lays out (a million rows) is a box of `MAX` pixels, scrolled
  // through `scale` times faster: `at` is how far down the whole list the view is.
  const full = $derived(items.length * rowH);
  const box = $derived(Math.min(full, MAX));
  const scale = $derived(box > height ? Math.max(1, (full - height) / (box - height)) : 1);
  const at = $derived(top * scale);
  const first = $derived(Math.max(0, Math.min(items.length, Math.floor(at / rowH)) - OVER));
  const last = $derived(Math.min(items.length, Math.ceil((at + height) / rowH) + OVER));
  const pad = $derived(Math.max(0, top + first * rowH - at));

  $effect(() => {
    if (!el) return;
    height = el.clientHeight;
    const ro = new ResizeObserver(() => (height = el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  });

  let shown, shownItems;
  $effect(() => {
    if (!el || !height || cursor < 0) return;
    // A new list (another folder or tab) comes with where it was scrolled, and brings its
    // cursor into view too.
    // (A tab's paged listing stays one object: its `listed` counts the listings.)
    const now = items.listed === undefined ? items : `${items.listed}`;
    if (now !== shown || items !== shownItems) {
      shown = now;
      shownItems = items;
      const was = untrack(() => top);
      if (el.scrollTop !== was) el.scrollTop = was;
    }
    const y = cursor * rowH;
    const seen = el.scrollTop * scale;
    if (y < seen) el.scrollTop = y / scale;
    else if (y + rowH > seen + height) el.scrollTop = (y + rowH - height) / scale;
    top = el.scrollTop; // the scroll event comes later; the rows there are wanted now
  });
</script>

<div bind:this={el} {...rest} onscroll={(ev) => (top = ev.currentTarget.scrollTop)}>
  <div style:height="{pad}px"></div>
  {#each items.slice(first, last) as e, j (e.name === ".." ? "\0.." : e.path)}{@render row(e, first + j)}{/each}
  <div style:height="{Math.max(0, box - pad - (last - first) * rowH)}px"></div>
</div>

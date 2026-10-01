<script>
  // A scrolling list that keeps only the rows in view in the DOM, so a folder of 100,000
  // files costs what a screenful does: the rest is empty space of the right height. Every
  // row is exactly `rowH` pixels high. `cursor` is kept in view, as scrollIntoView would.
  /** @type {{ items: any[], rowH: number, cursor?: number, row: import("svelte").Snippet<[any, number]>, [k: string]: any }} */
  let { items, rowH, cursor = -1, row, ...rest } = $props();
  let el = $state();
  let top = $state(0);
  let height = $state(0);
  /** Rows rendered beyond the edges, so a scroll never shows a gap before the next render. */
  const OVER = 8;
  const first = $derived(Math.max(0, Math.floor(top / rowH) - OVER));
  const last = $derived(Math.min(items.length, Math.ceil((top + height) / rowH) + OVER));

  $effect(() => {
    if (!el) return;
    height = el.clientHeight;
    const ro = new ResizeObserver(() => (height = el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  });

  $effect(() => {
    if (!el || !height || cursor < 0) return;
    const y = cursor * rowH;
    if (y < el.scrollTop) el.scrollTop = y;
    else if (y + rowH > el.scrollTop + height) el.scrollTop = y + rowH - height;
    top = el.scrollTop; // the scroll event comes later; the rows there are wanted now
  });
</script>

<div bind:this={el} {...rest} onscroll={(ev) => (top = ev.currentTarget.scrollTop)}>
  <div style:height="{first * rowH}px"></div>
  {#each items.slice(first, last) as e, j (e.path)}{@render row(e, first + j)}{/each}
  <div style:height="{(items.length - last) * rowH}px"></div>
</div>

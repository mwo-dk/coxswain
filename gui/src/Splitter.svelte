<script>
  /** Drag handle between two areas. `onmove(dx)` gets the pointer delta in px. */
  let { onmove, vertical = true } = $props();
  let dragging = $state(false);
  let last = 0;

  function down(e) {
    dragging = true;
    last = vertical ? e.clientX : e.clientY;
    e.currentTarget.setPointerCapture(e.pointerId);
  }
  function move(e) {
    if (!dragging) return;
    const now = vertical ? e.clientX : e.clientY;
    onmove(now - last);
    last = now;
  }
</script>

<div
  class="splitter"
  class:vertical
  class:dragging
  role="separator"
  aria-orientation={vertical ? "vertical" : "horizontal"}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={() => (dragging = false)}
></div>

<style>
  .splitter {
    flex: 0 0 5px;
    cursor: row-resize;
    background: var(--border-fg);
    background-clip: content-box;
    padding: 2px 0;
    opacity: 0.6;
    transition: opacity 0.15s;
    touch-action: none;
  }
  .vertical {
    cursor: col-resize;
    padding: 0 2px;
  }
  .splitter:hover,
  .dragging {
    opacity: 1;
    background: var(--accent-bg);
    background-clip: content-box;
  }
</style>

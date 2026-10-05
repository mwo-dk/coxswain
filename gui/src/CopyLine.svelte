<script>
  // A line to copy and run yourself (an install command), with a Copy button. Coxswain never
  // runs it.
  import { invoke } from "./lib.js";
  import { t } from "./i18n.svelte.js";

  let { line } = $props();
  let said = $state("");
  async function copy() {
    try {
      await invoke("copy_text", { text: line });
      said = t("common.copied");
    } catch (e) {
      said = String(e);
    }
  }
</script>

<span class="copyline"><code class="mono">{line}</code> <button onclick={copy}>{t("common.copy")}</button>{#if said}<small class="hint"> {said}</small>{/if}</span>

<style>
  .copyline {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  code {
    user-select: all;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 1px 10px;
    cursor: pointer;
  }
</style>

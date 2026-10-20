<script>
  // A neural network model's graph (docs/previews/neural-networks.md), drawn by Netron
  // (gui/public/vendor/netron, made by gui/netron/build.mjs). Netron runs in a frame sandboxed
  // to scripts alone: no origin, so no storage, no app calls and no reach into this page, and
  // a policy that allows no connection. The model's bytes go to it by message; so do the files
  // it names next to the model (weights, external data), and only from the model's own folder.
  import { invoke, basename, parent } from "./lib.js";
  import { bytes } from "./renderers.js";
  import { t } from "./i18n.svelte.js";

  /** `dir`: a Core ML package, a folder: Netron opens its Manifest.json. */
  let { path, dir = false } = $props();

  /** The biggest model or file next to it handed to Netron. */
  const MAX = 512 * 1024 * 1024;
  let frame = $state();
  /** Netron draws light or dark as its frame's colour scheme says: the theme's. */
  const scheme = getComputedStyle(document.documentElement).colorScheme.includes("dark") ? "dark" : "light";
  let error = $state("");
  let shown = $state(false);

  const sep = $derived(path.includes("\\") && !path.includes("/") ? "\\" : "/");
  const folder = $derived(dir ? path : parent(path));
  const main = $derived(dir ? "Manifest.json" : basename(path));

  /** A file the model names, inside its folder: no "..", no absolute path, no drive. */
  function inside(name) {
    const parts = String(name).split("/");
    if (parts.some((p) => !p || p === "." || p === ".." || /[\\:\0]/.test(p))) return null;
    return folder.replace(/[\\/]+$/, "") + sep + parts.join(sep);
  }

  async function send(message, transfer) {
    frame?.contentWindow?.postMessage(message, "*", transfer);
  }

  async function open() {
    try {
      const b = await bytes(dir ? inside(main) : path, MAX);
      send({ open: main, bytes: b }, [b]);
    } catch (err) {
      error = String(err?.message ?? err);
    }
  }

  async function answer(id, name) {
    const at = inside(name);
    try {
      if (!at) throw new Error(t("netron.outside"));
      const b = await bytes(at, MAX);
      send({ id, bytes: b }, [b]);
    } catch (err) {
      send({ id, error: String(err?.message ?? err) });
    }
  }

  function onmessage(e) {
    if (!frame || e.source !== frame.contentWindow || !e.data?.netron) return;
    const m = e.data;
    if (m.ready) open();
    else if (m.shown) shown = true;
    else if (typeof m.error === "string") error = m.error;
    else if (typeof m.fetch === "string" && Number.isInteger(m.id)) answer(m.id, m.fetch);
    else if (typeof m.url === "string" && /^https?:\/\//i.test(m.url)) invoke("open_path", { path: m.url });
  }
</script>

<svelte:window {onmessage} />

{#if error}
  <p class="more">{t("netron.failed")} {error}</p>
{/if}
<div class="netron" class:hidden={!!error} data-shown={shown || undefined}>
  <iframe bind:this={frame} style:color-scheme={scheme} sandbox="allow-scripts" src="vendor/netron/index.html" title={t("netron.title", { name: basename(path) })}></iframe>
  {#if !shown && !error}<p class="more">{t("netron.reading")}</p>{/if}
</div>
<p class="credit">{t("netron.credit")}</p>

<style>
  .netron {
    position: relative;
    height: 48vh;
    min-height: 280px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-sm);
    overflow: hidden;
  }
  .netron.hidden {
    display: none;
  }
  iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
  .netron .more {
    position: absolute;
    left: 10px;
    top: 6px;
    margin: 0;
  }
  .credit {
    font-size: 0.85em;
    opacity: 0.75;
    margin: 4px 0 0;
  }
</style>

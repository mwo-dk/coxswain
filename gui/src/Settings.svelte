<script>
  // Settings: every choice is written to config.toml at once (comments and layout kept, see
  // save_settings in main.rs) and applied without a restart.
  import { ui, setTheme, tab } from "./app.svelte.js";
  import { invoke, size } from "./lib.js";
  import { t, setLanguage } from "./i18n.svelte.js";

  const flags = import.meta.glob("../node_modules/flag-icons/flags/4x3/{gb,au,ca,nz,dk,se,fi,ee,lv,lt,de,fr,it,nl,ar,es-ct,es-pv,il}.svg", {
    query: "?url",
    import: "default",
    eager: true,
  });
  const flag = (name) => flags[`../node_modules/flag-icons/flags/4x3/${name}.svg`];

  const s = $derived(ui.cfg.settings);
  let error = $state("");
  let saved = $state(false);

  /** Save one setting and use the new config everywhere. */
  async function set(name, value) {
    error = "";
    try {
      const cfg = await invoke("save_settings", { changes: { [name]: value } });
      ui.cfg = cfg;
      setLanguage(cfg);
      setTheme(cfg.settings.theme);
      saved = true;
      loadImages();
    } catch (e) {
      error = String(e);
    }
  }

  // Built-in themes first, in their order, then any `[themes.<name>]` from config.toml.
  const themes = $derived([...ui.cfg.builtin_themes, ...Object.keys(ui.cfg.themes).filter((id) => !ui.cfg.builtin_themes.includes(id))]);
  const themeName = (id) => (ui.cfg.builtin_themes.includes(id) ? t(`theme.${id}`) : id);
  const close = () => (ui.modal = null);

  // Container images: which the runtime has, with Pull (also updates) and Remove.
  let images = $state(null);
  const loadImages = () => invoke("images").then((v) => (images = v), (e) => (images = String(e)));
  loadImages();
  async function imageAction(cmd, im) {
    error = "";
    im.pulling = "";
    await invoke(cmd, { image: im.image }).catch((e) => (error = String(e)));
    loadImages();
  }
  // Refresh while a pull runs, so the progress line moves.
  $effect(() => {
    if (!Array.isArray(images) || !images.some((im) => im.pulling != null)) return;
    const id = setInterval(loadImages, 1000);
    return () => clearInterval(id);
  });
  // Search inside files: the helper's store, its folders, and what it is doing.
  let index = $state(null);
  const loadIndex = () => invoke("index_status").then((v) => (index = v), () => (index = null));
  loadIndex();
  $effect(() => {
    const id = setInterval(loadIndex, 2000);
    return () => clearInterval(id);
  });
  /** A search setting: saved, then a helper with the new settings takes over. */
  async function setSearch(name, value) {
    await set(name, value);
    if (!error) await invoke("index_action", { what: "restart" }).catch((e) => (error = String(e)));
    loadIndex();
  }
  const adding = $state({ text_roots: "", names_only: "" });
  /** What the store has of a folder read: its bytes and files, and whether its disk is away. */
  const rootInfo = (dir) => {
    const r = index?.roots?.find(([p]) => p === dir);
    if (!r) return "";
    const [, at, [bytes]] = r;
    return at ? t("settings.search_root_size", { size: size(bytes) }) : t("settings.search_root_away", { size: size(bytes) });
  };
  async function addFolder(name, input) {
    const dir = await invoke("resolve_path", { base: tab()?.dir ?? ui.cfg.home, input: input || tab()?.dir || "" });
    adding[name] = "";
    if (!s[name].includes(dir)) setSearch(name, [...s[name], dir]);
  }
  async function serviceSet(on) {
    error = "";
    await invoke("index_service", { on }).catch((e) => (error = String(e)));
    loadIndex();
  }
  // Search by meaning: the model, its download, and whether it is on.
  let meaning = $state(null);
  const loadMeaning = () => invoke("meaning_status").then((v) => (meaning = v), () => (meaning = null));
  loadMeaning();
  $effect(() => {
    const id = setInterval(loadMeaning, meaning?.downloading ? 500 : 3000);
    return () => clearInterval(id);
  });
  async function meaningAction(what) {
    error = "";
    await invoke("meaning_action", { what }).catch((e) => (error = String(e)));
    loadMeaning();
    loadIndex();
  }
  // A server makes the vectors instead: Ollama, or one that speaks the OpenAI API (Lemonade,
  // LM Studio, llama.cpp …). Its models are listed; a server elsewhere is said to be elsewhere.
  const server = $derived(s.meaning_engine === "builtin" ? null : s.meaning_engine);
  const wanted = $derived(s.meaning_model || "bge-m3");
  let models = $state({ ok: false, list: [], error: "" });
  $effect(() => {
    const [engine, url] = [server, s.meaning_url];
    if (!engine) return;
    invoke("meaning_models", { engine, url }).then((list) => (models = { ok: true, list, error: "" }), (e) => (models = { ok: false, list: [], error: String(e) }));
  });
  // A pull that finished brings its model into the list.
  let pulling = false;
  $effect(() => {
    const now = !!meaning?.downloading;
    if (pulling && !now && server) invoke("meaning_models", { engine: server, url: s.meaning_url }).then((list) => (models = { ok: true, list, error: "" }), () => {});
    pulling = now;
  });
  const remote = $derived.by(() => {
    if (!server) return "";
    try {
      const host = new URL(s.meaning_url || (server === "ollama" ? "http://localhost:11434" : "http://localhost")).hostname;
      return ["localhost", "127.0.0.1", "[::1]", "::1"].includes(host) ? "" : host;
    } catch {
      return "";
    }
  });
  async function pull(model) {
    error = "";
    await invoke("meaning_pull", { model, url: s.meaning_url }).catch((e) => (error = String(e)));
    loadMeaning();
  }
  // Deleting the index takes a second click.
  let forgetting = $state(false);
  async function indexAction(what) {
    if (what === "forget" && !forgetting) return (forgetting = true);
    forgetting = false;
    await invoke("index_action", { what }).catch((e) => (error = String(e)));
    loadIndex();
  }

  // Opened at a section: `--settings=search`.
  $effect(() => {
    if (ui.modal?.section) document.getElementById(`settings-${ui.modal.section}`)?.scrollIntoView();
  });

  // Previews made by tools, kept in the cache: how much room, and a way to start afresh.
  let previewCache = $state(null);
  const loadPreviewCache = () => invoke("preview_cache").then((v) => (previewCache = v), () => (previewCache = null));
  loadPreviewCache();
  async function clearPreviewCache() {
    await invoke("clear_preview_cache").catch((e) => (error = String(e)));
    loadPreviewCache();
  }

  const imageStatus = (im) => (im.pulling != null ? im.pulling || t("common.loading") : im.size != null ? t("settings.image_pulled", { size: size(im.size) }) : t("settings.image_not_pulled"));
</script>

<div class="settings" role="dialog" aria-modal="true" aria-label={t("settings.title")}>
  <header>
    <h2>{"\u{f013}"} {t("settings.title")}</h2>
    <button class="x" title={t("common.close")} onclick={close}>×</button>
  </header>

  <div class="body">
    <section>
      <h3>{t("settings.language")}</h3>
      <div class="langs" role="radiogroup" aria-label={t("settings.language")}>
        <button class="lang" class:on={s.language === "auto"} role="radio" aria-checked={s.language === "auto"} onclick={() => set("language", "auto")}>
          <span class="auto">{"\u{f0ac}"}</span>
          <span>{t("settings.language_auto")}<small>{ui.cfg.languages.find((l) => l[0] === ui.cfg.language)?.[1]}</small></span>
        </button>
        {#each ui.cfg.languages as [code, name, fl] (code)}
          <button class="lang" class:on={s.language === code} role="radio" aria-checked={s.language === code} lang={code} onclick={() => set("language", code)}>
            <img src={flag(fl)} alt="" />
            <span>{name}</span>
          </button>
        {/each}
      </div>
      <p class="hint">{t("settings.language_hint")}</p>
    </section>

    <section>
      <h3>{t("settings.appearance")}</h3>
      <p class="label">{t("settings.theme")}</p>
      <div class="themes" role="radiogroup" aria-label={t("settings.theme")}>
        {#each themes as id (id)}
          {@const th = ui.cfg.themes[id]}
          <button class="theme" class:on={s.theme === id} role="radio" aria-checked={s.theme === id} onclick={() => set("theme", id)}>
            <!-- A tiny window in the theme's colours: sidebar, a folder, the cursor row, a file. -->
            <span class="swatch" style:background={th.panel.bg} style:border-color={th.border.fg} aria-hidden="true">
              <span class="side" style:background={th.sidebar.bg}></span>
              <span class="rows">
                <i style:background={th.directory.fg}></i>
                <i class="cur" style:background={th.cursor.bg}><b style:background={th.cursor.fg}></b></i>
                <i style:background={th.panel.fg}></i>
              </span>
            </span>
            <span>{themeName(id)}</span>
          </button>
        {/each}
      </div>
      <div class="grid">
        <label for="glyphs">{t("settings.glyphs")}</label>
        <select id="glyphs" value={s.glyphs} onchange={(e) => set("glyphs", e.currentTarget.value)}>
          <option value="nerd">{t("settings.glyphs_nerd")}</option>
          <option value="ascii">{t("settings.glyphs_ascii")}</option>
        </select>
        <label for="size">{t("settings.font_size")}</label>
        <input id="size" type="number" min="9" max="28" value={s.font_size} onchange={(e) => set("font_size", Number(e.currentTarget.value))} />
        <label for="font">{t("settings.font")}</label>
        <input id="font" value={s.font} spellcheck="false" onchange={(e) => set("font", e.currentTarget.value)} />
        <label for="mono">{t("settings.mono_font")}</label>
        <input id="mono" value={s.mono_font} spellcheck="false" onchange={(e) => set("mono_font", e.currentTarget.value)} />
        {#if ui.cfg.looks[s.theme] !== "modern"}<span></span><span class="hint">{t("settings.font_theme_hint")}</span>{/if}
        <label for="icons">{t("settings.icon_font")}</label>
        <input id="icons" value={s.icon_font} spellcheck="false" onchange={(e) => set("icon_font", e.currentTarget.value)} />
      </div>
    </section>

    <section>
      <h3>{t("settings.behaviour")}</h3>
      <label class="check"><input type="checkbox" checked={s.show_hidden} onchange={(e) => set("show_hidden", e.currentTarget.checked)} /> {t("settings.show_hidden")}</label>
      <label class="check"><input type="checkbox" checked={s.confirm_delete} onchange={(e) => set("confirm_delete", e.currentTarget.checked)} /> {t("settings.confirm_delete")}</label>
      <label class="check"><input type="checkbox" checked={s.check_updates} onchange={(e) => set("check_updates", e.currentTarget.checked)} /> {t("settings.check_updates")}</label>
    </section>

    {#snippet folders(name, none)}
      <div class="folders">
        {#each s[name] as dir (dir)}
          <div class="folder">
            <span class="mono">{dir}{#if name === "text_roots"}<small class="hint">{rootInfo(dir)}</small>{/if}</span>
            <button onclick={() => setSearch(name, s[name].filter((d) => d !== dir))}>{t("common.remove")}</button>
          </div>
        {:else}
          <span class="hint">{none}{#if name === "text_roots" && index?.roots?.[0]} · {rootInfo(index.roots[0][0])}{/if}</span>
        {/each}
        <form class="folder" onsubmit={(e) => (e.preventDefault(), addFolder(name, adding[name]))}>
          <input bind:value={adding[name]} spellcheck="false" placeholder={tab()?.dir} aria-label={t("settings.search_add")} />
          <button type="submit">{t("settings.search_add")}</button>
        </form>
      </div>
    {/snippet}

    <section id="settings-search">
      <h3>{t("settings.search")}</h3>
      <label class="check"><input type="checkbox" checked={s.search_text} onchange={(e) => setSearch("search_text", e.currentTarget.checked)} /> {t("settings.search_text")}</label>
      {#if s.search_text}
        <p class="hint">
          {#if !index?.shared}
            {t("settings.search_no_helper")}
          {:else}
            {t("settings.search_status", { texts: index.texts, pending: index.pending, size: size(index.bytes) })}
            {#if index.paused}<br /><strong>{t("settings.search_paused")}</strong>{/if}
          {/if}
          {#if index?.path}<br /><span class="mono">{index.path}</span>{/if}
        </p>
        <label class="check"><input type="checkbox" checked={index?.service} disabled={!index} onchange={(e) => serviceSet(e.currentTarget.checked)} /> {t("settings.search_service")}</label>
        <div class="buttons">
          <button disabled={!index?.shared || !index.pending} onclick={() => indexAction("now")}>{t("settings.search_now")}</button>
          <button disabled={!index?.shared} class:danger={forgetting} onclick={() => indexAction("forget")} onblur={() => (forgetting = false)}>{forgetting ? t("settings.search_forget_confirm") : t("settings.search_forget")}</button>
        </div>
        <div class="grid">
          <span class="top">{t("settings.search_roots")}</span>
          {@render folders("text_roots", t("settings.search_roots_home"))}
          <span class="top">{t("settings.search_names_only")}</span>
          {@render folders("names_only", t("settings.search_names_only_none"))}
          {#if index?.tools?.length}
            <span class="top">{t("settings.search_tools")}</span>
            <ul class="tools">
              {#each index.tools as [name, there] (name)}
                <li class:missing={!there}>{there ? "✓" : "✗"} {t(`settings.search_tool_${name}`)}{#if !there}<small class="hint"> · {t("settings.search_tool_missing")}</small>{/if}</li>
              {/each}
            </ul>
          {/if}
        </div>
        <p class="hint">{t("settings.search_hint")}</p>
      {/if}
    </section>

    <section id="settings-meaning">
      <h3>{t("settings.meaning")}</h3>
      <p class="hint">{t("settings.meaning_hint")}</p>
      <div class="grid">
        <label for="mengine">{t("settings.meaning_engine")}</label>
        <select id="mengine" value={s.meaning_engine} onchange={(e) => setSearch("meaning_engine", e.currentTarget.value)}>
          <option value="builtin">{t("settings.meaning_builtin", { size: size(meaning?.size ?? 0) })}</option>
          <option value="ollama">Ollama</option>
          <option value="openai">{t("settings.meaning_openai")}</option>
        </select>
        {#if server}
          <label for="murl">{t("settings.meaning_url")}</label>
          <input id="murl" value={s.meaning_url} spellcheck="false" placeholder={server === "ollama" ? "http://localhost:11434" : "http://localhost:8000/api/v1"} onchange={(e) => setSearch("meaning_url", e.currentTarget.value.trim())} />
          <label for="mmodel">{t("settings.meaning_model")}</label>
          <div class="folder">
            <input id="mmodel" list="mmodels" value={s.meaning_model} spellcheck="false" placeholder={server === "ollama" ? "bge-m3" : ""} onchange={(e) => setSearch("meaning_model", e.currentTarget.value.trim())} />
            <datalist id="mmodels">{#each models.list as m (m)}<option value={m}></option>{/each}</datalist>
            {#if server === "ollama" && models.ok && !models.list.some((m) => m.split(":")[0] === wanted)}
              <button disabled={!!meaning?.downloading} onclick={() => pull(wanted)}>{t("settings.meaning_pull", { model: wanted })}</button>
            {/if}
          </div>
          {#if server === "openai"}
            <label for="mkey">{t("settings.meaning_key_env")}</label>
            <input id="mkey" value={s.meaning_key_env} spellcheck="false" placeholder="OPENAI_API_KEY" onchange={(e) => setSearch("meaning_key_env", e.currentTarget.value.trim())} />
          {/if}
          <span></span>
          <p class="hint">
            {models.ok ? t("settings.meaning_server_ok") : models.error}
            {#if remote}<br /><strong>{t("settings.meaning_remote", { host: remote })}</strong>{/if}
          </p>
        {/if}
      </div>
      {#if meaning?.downloading}
        <p class="hint">{t("settings.meaning_downloading", { done: size(meaning.downloading[0]), total: size(meaning.downloading[1]) })}</p>
        <progress max={meaning.downloading[1] || 1} value={meaning.downloading[0]}></progress>
        <div class="buttons"><button onclick={() => meaningAction("cancel")}>{t("common.cancel")}</button></div>
      {:else if s.search_meaning && (server || meaning?.installed)}
        <p class="hint">
          {t("settings.meaning_status", { done: index?.meaning_done ?? 0, pending: index?.meaning_pending ?? 0 })}
          {#if index?.meaning_engine}<br /><span class="mono">{index.meaning_engine}</span>{/if}
          {#if index?.meaning_error}<br /><span class="err">{index.meaning_error}</span>{/if}
          {#if index?.paused}<br /><strong>{t("settings.search_paused")}</strong>{/if}
        </p>
        <div class="buttons">
          <button onclick={() => meaningAction("off")}>{t("settings.meaning_off")}</button>
          {#if !server}<button onclick={() => meaningAction("remove")}>{t("settings.meaning_remove")}</button>{/if}
        </div>
      {:else}
        {#if meaning?.error}<p class="err">{meaning.error}</p>{/if}
        <div class="buttons">
          {#if server || meaning?.installed}
            <button class="primary" disabled={!s.search_text} onclick={() => setSearch("search_meaning", true)}>{t("settings.meaning_on")}</button>
            {#if !server}<button onclick={() => meaningAction("remove")}>{t("settings.meaning_remove")}</button>{/if}
          {:else}
            <button class="primary" disabled={!s.search_text} onclick={() => meaningAction("download")}>{t("settings.meaning_download", { size: size(meaning?.size ?? 0) })}</button>
          {/if}
        </div>
      {/if}
    </section>

    <section>
      <h3>{t("settings.previews")}</h3>
      <div class="grid">
        <label for="prefer">{t("settings.prefer")}</label>
        <select id="prefer" value={s.preview_prefer} onchange={(e) => set("preview_prefer", e.currentTarget.value)}>
          <option value="auto">{t("settings.prefer_auto")}</option>
          <option value="local">{t("settings.prefer_local")}</option>
          <option value="container">{t("settings.prefer_container")}</option>
        </select>
        <label for="runtime">{t("settings.container")}</label>
        <select id="runtime" value={s.preview_container} onchange={(e) => set("preview_container", e.currentTarget.value)}>
          <option value="auto">{t("settings.container_auto")}</option>
          <option value="podman">podman</option>
          <option value="docker">docker</option>
          <option value="off">{t("settings.container_off")}</option>
        </select>
        <label for="latex">{t("settings.latex_image")}</label>
        <input id="latex" value={s.latex_image} spellcheck="false" onchange={(e) => set("latex_image", e.currentTarget.value)} />
        <span></span>
        <label class="check"><input type="checkbox" checked={s.latex_auto} onchange={(e) => set("latex_auto", e.currentTarget.checked)} /> {t("settings.latex_auto")}</label>
        <label for="timeout">{t("settings.timeout")}</label>
        <input id="timeout" type="number" min="10" max="3600" value={s.preview_timeout} onchange={(e) => set("preview_timeout", Number(e.currentTarget.value))} />
        <span>{t("settings.preview_cache")}</span>
        <div class="folder">
          <span class="hint">{previewCache == null ? "" : t("settings.preview_cache_size", { size: size(previewCache) })}</span>
          <button disabled={!previewCache} onclick={clearPreviewCache}>{t("settings.preview_cache_clear")}</button>
        </div>
        <span class="top">{t("settings.images")}</span>
        <div class="images">
          {#if typeof images === "string"}
            <span class="hint">{images}</span>
          {:else}
            {#each images ?? [] as im (im.image)}
              <div class="image">
                <span class="mono" title={im.tool}>{im.image}</span>
                <small class="hint">{imageStatus(im)}</small>
                <button disabled={im.pulling != null} onclick={() => imageAction("pull_image", im)}>{t("settings.pull")}</button>
                <button disabled={im.pulling != null || im.size == null} onclick={() => imageAction("remove_image", im)}>{t("common.remove")}</button>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </section>
  </div>

  <footer>
    {#if error}<span class="err">{error}</span>{:else}<span class="hint">{saved ? t("settings.saved", { path: ui.cfg.config_path }) : t("settings.stored_in", { path: ui.cfg.config_path })}</span>{/if}
    <button class="primary" onclick={close}>{t("common.close")}</button>
  </footer>
</div>

<style>
  .settings {
    position: fixed;
    inset: 5vh 10vw;
    z-index: 11;
    display: flex;
    flex-direction: column;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
  }
  header {
    border-bottom: 1px solid var(--border-fg);
  }
  footer {
    border-top: 1px solid var(--border-fg);
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font);
  }
  h3 {
    margin: 0 0 8px;
    font-size: 0.95em;
    color: var(--hidden-fg);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 12px;
    cursor: pointer;
  }
  .x {
    border: 0;
    font-size: 1.3em;
    padding: 0 6px;
  }
  .primary {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .langs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 6px;
  }
  .lang {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: start;
    padding: 6px 10px;
  }
  .lang.on {
    border-color: var(--accent-bg);
    background: color-mix(in srgb, var(--accent-bg) 18%, transparent);
  }
  .lang img {
    width: 24px;
    height: 18px;
    border-radius: var(--r-sm);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--border-fg) 80%, transparent);
    flex: none;
  }
  .lang small {
    display: block;
    color: var(--hidden-fg);
    font-size: 0.8em;
  }
  .auto {
    width: 24px;
    text-align: center;
    font-family: var(--icon-font);
    font-size: 1.2em;
  }
  p.label {
    margin: 0 0 6px;
    color: var(--hidden-fg);
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 6px;
    margin-bottom: 12px;
  }
  .theme {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 5px;
    padding: 6px;
    text-align: start;
  }
  .theme.on {
    border-color: var(--accent-bg);
    background: color-mix(in srgb, var(--accent-bg) 18%, transparent);
  }
  .swatch {
    display: flex;
    height: 44px;
    border: 1px solid;
    border-radius: var(--r-sm);
    overflow: hidden;
  }
  .side {
    width: 28%;
  }
  .rows {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 0 6px;
  }
  .rows i {
    display: block;
    height: 4px;
    width: 60%;
    border-radius: var(--r-sm);
  }
  .rows i.cur {
    width: auto;
    height: 9px;
    padding: 2.5px 5px;
    border-radius: 0;
  }
  .rows i.cur b {
    display: block;
    height: 4px;
    width: 50%;
    border-radius: var(--r-sm);
  }
  .grid {
    display: grid;
    grid-template-columns: max-content minmax(0, 24em);
    gap: 8px 14px;
    align-items: center;
  }
  label,
  .grid > span {
    color: var(--hidden-fg);
  }
  .images {
    display: grid;
    gap: 6px;
  }
  .image {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 2px 8px;
    align-items: center;
  }
  /* The name on a line of its own, then its status with the buttons. */
  .image span {
    grid-column: 1 / -1;
    overflow-wrap: anywhere;
  }
  .image small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .top {
    align-self: start;
  }
  .folders {
    display: grid;
    gap: 6px;
  }
  .folder {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    margin: 0;
  }
  .folder span {
    overflow-wrap: anywhere;
  }
  .folder small {
    display: block;
    margin: 2px 0 0;
  }
  .tools {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  .tools .missing {
    color: var(--hidden-fg);
  }
  .danger {
    border-color: var(--git-deleted-fg);
    color: var(--git-deleted-fg);
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin: 8px 0 12px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--dialog-fg);
    margin-bottom: 6px;
  }
  input:not([type="checkbox"]),
  select {
    font: inherit;
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 8px;
  }
  .hint {
    color: var(--hidden-fg);
    font-size: 0.85em;
    margin: 6px 0 0;
    overflow-wrap: anywhere;
  }
  .err {
    color: var(--git-deleted-fg);
  }
</style>

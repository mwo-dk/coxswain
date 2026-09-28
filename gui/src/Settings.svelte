<script>
  // Settings: every choice is written to config.toml at once (comments and layout kept, see
  // save_settings in main.rs) and applied without a restart.
  import { ui, setTheme } from "./app.svelte.js";
  import { invoke } from "./lib.js";
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
      const themeChanged = cfg.settings.theme !== ui.cfg.settings.theme;
      ui.cfg = cfg;
      setLanguage(cfg);
      setTheme(themeChanged ? cfg.settings.theme : ui.theme);
      saved = true;
    } catch (e) {
      error = String(e);
    }
  }

  const THEMES = ["dark", "light", "nord", "midnight", "nc"];
  const close = () => (ui.modal = null);
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
      <div class="grid">
        <label for="theme">{t("settings.theme")}</label>
        <select id="theme" value={s.theme} onchange={(e) => set("theme", e.currentTarget.value)}>
          {#each THEMES.filter((id) => ui.cfg.themes[id]) as id (id)}<option value={id}>{t(`theme.${id}`)}</option>{/each}
          {#each Object.keys(ui.cfg.themes).filter((id) => !THEMES.includes(id)) as id (id)}<option value={id}>{id}</option>{/each}
        </select>
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
        <label for="timeout">{t("settings.timeout")}</label>
        <input id="timeout" type="number" min="10" max="3600" value={s.preview_timeout} onchange={(e) => set("preview_timeout", Number(e.currentTarget.value))} />
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
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.45);
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
    border-radius: 7px;
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
    border-radius: 2px;
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
  .grid {
    display: grid;
    grid-template-columns: max-content minmax(0, 24em);
    gap: 8px 14px;
    align-items: center;
  }
  label {
    color: var(--hidden-fg);
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
    border-radius: 7px;
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

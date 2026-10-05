<script>
  // The first-run guide (coxswain_core::guide): the two panels and their keys, how far Find
  // looks, looks, and what can leave the machine. Shown once on the first start, skippable on
  // every step, opened again from Help (F1) and Settings → Overview. Each choice is saved to
  // config.toml at once, as in Settings.
  import { onDestroy } from "svelte";
  import { ui, setTheme, themeName } from "./app.svelte.js";
  import { invoke } from "./lib.js";
  import { t, setLanguage } from "./i18n.svelte.js";
  import CopyLine from "./CopyLine.svelte";

  const m = $derived(ui.modal);
  const s = $derived(ui.cfg.settings);
  const STEPS = 4;
  let error = $state("");

  // Closed in any way (Done, Skip, Esc, the setup guide taking over): not shown by itself again.
  onDestroy(() => invoke("guide_seen").catch(() => {}));
  const close = () => (ui.modal = null);
  const go = (d) => {
    const step = m.step + d;
    if (step >= STEPS) return close();
    if (step >= 0) m.step = step;
  };
  // Keys from App, through Dialogs: Enter next, Esc skip (closes), Backspace back.
  ui.modal.key = (k) => {
    if (k === "Enter") go(1);
    else if (k === "Backspace") go(-1);
    else if (k === "Esc") close();
    else return false;
    return true;
  };

  async function save(changes) {
    error = "";
    try {
      ui.cfg = await invoke("save_settings", { changes });
      setLanguage(ui.cfg);
    } catch (e) {
      error = String(e);
    }
  }

  // Step 2: a level is turned on, or the search setup guide opens to choose a model, and comes
  // back here when it closes.
  async function level(id) {
    const changes = await invoke("search_level", { level: id }).catch((e) => ((error = String(e)), undefined));
    if (changes === undefined) return;
    if (!changes) {
      ui.resume = { kind: "guide", step: 1 };
      ui.modal = { kind: "setup" };
      return;
    }
    await save(changes);
  }

  // Step 3: whether a Nerd Font is there (asked once).
  let nerd = $state(undefined);
  $effect(() => {
    if (m.step === 2 && nerd === undefined) invoke("nerd_font").then((v) => (nerd = v), () => (nerd = null));
  });
  const swatch = (id) => ui.cfg.themes[id]?.panel ?? {};
  const keyOf = (name) => ui.cfg.actions[name]?.[1] ?? "";
</script>

<div class="guide" role="dialog" aria-modal="true" aria-label={t("guide.name")}>
  <header>
    <h2>{t("guide.title", { name: t(`guide.step${m.step + 1}`), n: m.step + 1, of: STEPS })}</h2>
    <button class="x" title={t("guide.skip")} onclick={close}>×</button>
  </header>
  <div class="body">
    {#if m.step === 0}
      <p class="lead">{t("guide.panels_intro")}</p>
      <div class="mini" aria-hidden="true">
        <div class="on"><b>projects</b><span>src/ · docs/ · README.md</span></div>
        <div><b>Documents</b><span>budget.txt · report.pdf</span></div>
      </div>
      <table class="keys">
        <tbody>
          {#each ui.cfg.guide_keys as [label, key] (label)}<tr><td><kbd>{key}</kbd></td><td>{label}</td></tr>{/each}
        </tbody>
      </table>
      <p class="hint">{t("guide.panels_more")}</p>
    {:else if m.step === 1}
      <p class="lead">{t("guide.find_intro", { key: keyOf("search") })}</p>
      <div class="levels" role="radiogroup">
        {#each ui.cfg.levels as [id] (id)}
          <label class:on={ui.cfg.level === id}>
            <input type="radio" name="level" checked={ui.cfg.level === id} onchange={() => level(id)} />
            <span><b>{t(`settings.level.${id}`)}</b><br /><small class="hint">{t(`settings.level.${id}.hint`)}</small></span>
          </label>
        {/each}
      </div>
      <p class="hint">{t("guide.find_setup")}</p>
    {:else if m.step === 2}
      <p class="lead">{t("guide.looks_intro")}</p>
      <div class="swatches" role="radiogroup" aria-label={t("setting.theme")}>
        {#each ui.cfg.guide_themes as id (id)}
          <button class="sw" class:on={ui.theme === id} role="radio" aria-checked={ui.theme === id} style:background={swatch(id).bg} style:color={swatch(id).fg} onclick={() => setTheme(id, true)}>
            Aa<small>{themeName(id)}</small>
          </button>
        {/each}
      </div>
      <p class="hint">{t("guide.looks_more")}</p>
      <label class="row">{t("setting.language")}
        <select value={s.language} onchange={(e) => save({ language: e.currentTarget.value })}>
          <option value="auto">{t("settings.language_auto")}</option>
          {#each ui.cfg.languages as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
        </select>
      </label>
      <div class="icons">
        <b>{t("setting.glyphs")}</b>
        {#if s.glyphs === "ascii"}
          <p>{t("guide.icons_plain_now")} <button onclick={() => save({ glyphs: "nerd" })}>{t("guide.icons_nerd")}</button></p>
        {:else if nerd === false}
          <p class="warn">{t("guide.icons_missing")}</p>
          <p>
            {#if ui.cfg.installs["nerd-font"]}{t("guide.icons_install")} <CopyLine line={ui.cfg.installs["nerd-font"]} />{:else}{t("guide.icons_get")}{/if}
          </p>
          <p><button onclick={() => save({ glyphs: "ascii" })}>{t("guide.icons_plain")}</button></p>
        {:else if nerd}
          <p><span class="glyph">{"\u{f07b} \u{f15b}"}</span> {t("guide.icons_found")}</p>
        {:else}
          <p><span class="glyph">{"\u{f07b} \u{f15b}"}</span> {t("guide.icons_unknown")} <button onclick={() => save({ glyphs: "ascii" })}>{t("guide.icons_plain")}</button></p>
        {/if}
      </div>
    {:else}
      <p class="lead">{t("guide.privacy_intro")}</p>
      <label class="check"><input type="checkbox" checked={s.check_updates} onchange={(e) => save({ check_updates: e.currentTarget.checked })} /> {t("guide.privacy_update")} <span class="mono hint">api.github.com</span></label>
      <p class="label">{t("guide.privacy_now")}</p>
      <ul>
        {#each ui.cfg.outbound as o, i (i)}<li>{o.what} → <span class="mono">{o.to}</span>{#if o.local} <small class="hint">({t("settings.privacy_local")})</small>{/if}</li>{:else}<li>{t("settings.privacy_nothing")}</li>{/each}
      </ul>
      <p class="hint">{t("guide.again")}</p>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
  <footer>
    <span class="hint">{t(m.step + 1 < STEPS ? "guide.keys" : "guide.keys_last")}</span>
    {#if m.step + 1 < STEPS}<button onclick={close}>{t("guide.skip")}</button>{/if}
    {#if m.step > 0}<button onclick={() => go(-1)}>{t("guide.back")}</button>{/if}
    <button class="primary" onclick={() => go(1)}>{t(m.step + 1 < STEPS ? "guide.next" : "guide.done")}</button>
  </footer>
</div>

<style>
  .guide {
    position: fixed;
    inset: 8vh max(4vw, calc(50vw - 330px));
    z-index: 12;
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
    justify-content: flex-end;
  }
  footer .hint {
    flex: 1;
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  p {
    margin: 0;
  }
  .lead {
    font-size: 1.05em;
  }
  .hint {
    opacity: 0.75;
  }
  .warn,
  .err {
    color: var(--git-conflict-fg, #e55);
  }
  button,
  select {
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 12px;
    cursor: pointer;
  }
  select {
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    margin-inline-start: 8px;
  }
  .primary {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .x {
    border: 0;
    font-size: 1.3em;
    padding: 0 6px;
  }
  .mini {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .mini div {
    display: flex;
    flex-direction: column;
    padding: 8px 10px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    opacity: 0.7;
  }
  .mini .on {
    opacity: 1;
    border-color: var(--accent-bg);
  }
  .mini span {
    font-size: 0.85em;
    opacity: 0.75;
  }
  .keys td {
    padding: 2px 10px 2px 0;
  }
  kbd {
    font-family: var(--mono-font, monospace);
  }
  .levels {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .levels label {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: var(--r);
    cursor: pointer;
  }
  .levels label.on {
    border-color: var(--accent-bg);
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .sw {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 8.5em;
    padding: 8px;
    font-size: 1.2em;
  }
  .sw small {
    font-size: 0.65em;
  }
  .sw.on {
    outline: 2px solid var(--accent-bg);
  }
  .icons {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .glyph {
    font-family: var(--icon-font);
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  ul {
    margin: 0;
    padding-inline-start: 1.2em;
  }
</style>

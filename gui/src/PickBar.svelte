<script>
  // Pick mode (`coxswain-gui --pick`): what is asked for, the name with `--save`, Show all
  // with `--filter`, and Choose (or Save) and Cancel. Esc cancels too, as closing the window does.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ui, tab, pickItem, pickSave } from "./app.svelte.js";
  import { t } from "./i18n.svelte.js";

  const p = ui.cfg.pick;
  const saving = p.save != null;
</script>

<div class="pickbar" role="toolbar" aria-label={ui.cfg.pick_ask}>
  <span class="ask">{ui.cfg.pick_ask}</span>
  {#if saving}
    <label class="name">{t("pick.save_title")}
      <input bind:value={ui.pickName} spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), pickSave())} />
    </label>
  {/if}
  {#if p.filter.length}
    <label class="all" title={ui.cfg.pick_key}><input type="checkbox" bind:checked={ui.pickAll} /> {t("pick.show_all")} <kbd>{ui.cfg.pick_key}</kbd></label>
  {/if}
  <button class="primary" title="Enter" onclick={() => (saving ? pickSave() : pickItem(tab()))}>{saving ? t("verb.save") : t("pick.choose")}</button>
  <button title="Esc" onclick={() => getCurrentWindow().close()}>{t("common.cancel")}</button>
</div>

<style>
  .pickbar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 6px 6px 0;
    padding: 4px 12px;
    border-radius: var(--r);
    background: var(--cmdline-bg);
    color: var(--cmdline-fg);
  }
  .ask {
    flex: 1;
    font-weight: bold;
  }
  .name input {
    width: 24em;
    margin-inline-start: 6px;
  }
  .all {
    display: flex;
    align-items: center;
    gap: 4px;
  }
</style>

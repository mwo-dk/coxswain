<script>
  // Under a disk image's facts: what more can be done with it, and how (disk_image::Next).
  import { ui } from "./app.svelte.js";
  import CopyLine from "./CopyLine.svelte";
  import { t } from "./i18n.svelte.js";

  let { next, onsettings = () => {} } = $props();
</script>

{#if next?.state === "turn_on"}
  <p class="more">{t("disk_image.next_off")} <button class="render" onclick={() => { onsettings(); ui.modal = { kind: "settings", section: "disk_images" }; }}>{t("disk_image.open_settings")}</button></p>
{:else if next?.state === "install"}
  <p class="more">{#if next.line}{t("install.with")} <CopyLine line={next.line} />{:else}{t("disk_image.next_install_get")}{/if}</p>
{:else if next?.state === "open"}
  <p class="more">{t("disk_image.next_open", { key: ui.cfg.actions.open?.[1] ?? "Enter" })}</p>
{/if}

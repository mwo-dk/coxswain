<script>
  // The guided setup of search by meaning and Ask: each step says what it does and how it
  // stands, and any can be skipped. Nothing is downloaded until a button says so; the logic is
  // coxswain_core::setup's, shared with the terminal app's `coxswain --setup-search`.
  import { ui } from "./app.svelte.js";
  import { invoke, size } from "./lib.js";
  import { t } from "./i18n.svelte.js";

  const s = $derived(ui.cfg.settings);
  // Back to where it was started from (the first-run guide), else closed.
  const close = () => {
    ui.modal = ui.resume;
    ui.resume = null;
  };
  let error = $state("");

  async function set(changes) {
    error = "";
    try {
      ui.cfg = await invoke("save_settings", { changes });
    } catch (e) {
      error = String(e);
    }
  }

  // What the helper and the built-in model are doing, for the statuses.
  let index = $state(null);
  let meaning = $state(null);
  let chatBuiltin = $state(null);
  const load = () => {
    invoke("index_status").then((v) => (index = v), () => {});
    invoke("meaning_status").then((v) => (meaning = v), () => {});
    invoke("chat_status").then((v) => (chatBuiltin = v), () => {});
  };
  load();
  $effect(() => {
    const id = setInterval(load, meaning?.downloading || chatBuiltin?.downloading ? 500 : 2000);
    return () => clearInterval(id);
  });

  // Step 2: the servers on this machine, the machine, and what suits it.
  let look = $state(null);
  const probe = () => {
    look = null;
    invoke("setup_probe").then((v) => (look = v), (e) => (error = String(e)));
  };
  probe();
  let remoteUrl = $state("");
  let remote = $state(null);
  async function probeRemote() {
    remote = (await invoke("setup_probe_url", { url: remoteUrl }).catch((e) => (error = String(e)))) ?? false;
  }
  const servers = $derived([...(look?.found ?? []), ...(remote ? [remote] : [])]);
  // The server chosen: the one set now, else the one recommended; null is the built-in model.
  let chosen = $state(undefined);
  const server = $derived(chosen !== undefined ? chosen : servers.find((f) => s.meaning_engine !== "builtin" && f.engine === s.meaning_engine && sameUrl(f.url, s.meaning_url)) ?? (look ? (look.best != null ? look.found[look.best] : null) : undefined));
  const sameUrl = (a, b) => (a || "").replace(/\/$/, "") === (b || "http://localhost:11434").replace(/\/$/, "");
  const name = (kind) => ({ ollama: "Ollama", lemonade: "Lemonade", lmstudio: "LM Studio", llamacpp: "llama.cpp", jan: "Jan", localai: "LocalAI" })[kind] ?? "OpenAI API";
  const machineLine = $derived.by(() => {
    const m = look?.machine;
    if (!m) return "";
    const gpu = m.gpu ? `${m.gpu[1]}${m.gpu[2] ? ` (${m.gpu[2]} GB)` : ""}` : t("setup.no_gpu");
    return t("setup.machine", { what: m.npu ? `${gpu} + NPU` : gpu, ram: m.ram });
  });
  const isRemote = (url) => !!url && !/^https?:\/\/(localhost|127\.|\[::1\])/.test(url);

  // Step 3: the model for the vectors.
  const embedModels = $derived(server ? server.models.filter((m) => m.embed).map((m) => m.name) : []);
  const suggestEmbed = $derived(server ? (embedModels.find((m) => /bge-m3|nomic/i.test(m)) ?? (server.kind === "ollama" ? "bge-m3" : look?.advice?.embed || "")) : "");
  let embed = $state("");
  $effect(() => {
    if (!embedModels.includes(embed)) embed = embedModels.find((m) => m.startsWith(suggestEmbed)) ?? embedModels[0] ?? "";
  });
  const meaningNow = $derived(s.search_meaning ? (s.meaning_engine === "builtin" ? t("setup.builtin_short") : `${s.meaning_model} · ${s.meaning_url || "http://localhost:11434"}`) : "");
  let vectorsChange = $state(null);
  async function useEmbed() {
    const changes = { meaning_engine: server.engine, meaning_url: server.url, meaning_model: embed, search_meaning: true, search_text: true };
    const why = s.search_meaning ? await invoke("meaning_change", { name: "meaning_model", value: embed, engine: server.engine, url: server.url }).catch(() => null) : null;
    if (why && !vectorsChange) return (vectorsChange = { why, go: () => set(changes) });
    vectorsChange = null;
    await set(changes);
  }
  async function useBuiltin() {
    const why = s.search_meaning && s.meaning_engine !== "builtin" ? await invoke("meaning_change", { name: "meaning_engine", value: "builtin" }).catch(() => null) : null;
    if (why && !vectorsChange) return (vectorsChange = { why, builtin: true });
    vectorsChange = null;
    // The download turns meaning on when it is done; the page hears `config-changed`.
    if (!meaning?.installed) return invoke("meaning_action", { what: "download" }).then(load, (e) => (error = String(e)));
    await set({ meaning_engine: "builtin", search_meaning: true, search_text: true });
  }
  const pull = (model) => invoke("meaning_pull", { model, url: server.url, kind: server.kind }).then(() => setTimeout(probe, 1500), (e) => (error = String(e)));
  // A finished download brings its model into the lists.
  let pulling = false;
  $effect(() => {
    const now = !!meaning?.downloading;
    if (pulling && !now) probe();
    pulling = now;
  });

  // Step 4: Ask's chat model: a built-in one, or one on the same server (Ollama here with the
  // built-in model for the vectors).
  let builtinChat = $state("");
  $effect(() => {
    const list = chatBuiltin?.models ?? [];
    if (!list.some((m) => m.key === builtinChat)) builtinChat = (list.find((m) => m.key === s.ask_model) ?? list.find((m) => m.suggested) ?? list[0])?.key ?? "";
  });
  const builtinChosen = $derived(chatBuiltin?.models.find((m) => m.key === builtinChat));
  function useBuiltinChat() {
    if (builtinChosen.installed) return useChat(builtinChosen.key);
    // The download makes it Ask's when it is done; then it is tried.
    invoke("chat_action", { what: "download", model: builtinChosen.key }).then(load, (e) => (error = String(e)));
  }
  let fetchingChat = false;
  $effect(() => {
    const now = !!chatBuiltin?.downloading;
    if (fetchingChat && !now && !chatBuiltin.error && s.ask_model.startsWith("builtin:")) useChat(s.ask_model);
    fetchingChat = now;
  });
  const askServer = $derived(server ?? servers.find((f) => f.kind === "ollama") ?? null);
  const chatModels = $derived(askServer ? askServer.models.filter((m) => m.chat).map((m) => m.name) : []);
  const suggestChat = $derived(askServer?.kind === "lemonade" || askServer?.kind === "ollama" || !askServer ? (look?.advice?.chat ?? "qwen3:8b") : "");
  let chat = $state("");
  $effect(() => {
    if (!chatModels.includes(chat)) chat = chatModels.find((m) => m === s.ask_model) ?? chatModels.find((m) => suggestChat && m.startsWith(suggestChat)) ?? chatModels[0] ?? "";
  });
  let trial = $state(null);
  async function useChat(model = chat) {
    await set({ ask_model: model });
    trial = { busy: true };
    try {
      trial = { ms: await invoke("setup_try") };
      if (model.startsWith("builtin:")) return;
      speed = await invoke("setup_speed", { server: askServer });
      speedChecked = true;
    } catch (e) {
      trial = { error: String(e) };
    }
  }

  // Step 5: the speed check, after the test question has loaded the model.
  let speed = $state(null);
  let speedChecked = $state(false);

  // Step 6: start with the session.
  const service = (on) => invoke("index_service", { on }).then(load, (e) => (error = String(e)));
</script>

<div class="setup-search" role="dialog" aria-modal="true" aria-label={t("setup.title")}>
  <header>
    <h2>{"\u{f002}"} {t("setup.title")}</h2>
    <button class="x" title={t("common.close")} onclick={close}>×</button>
  </header>
  <div class="body">
    <p class="hint">{t("setup.intro")}</p>
    {#if error}<p class="err">{error}</p>{/if}

    <section>
      <h3>{t("setup.step_text")} <span class="state" class:ok={s.search_text}>{s.search_text ? t("setup.on") : t("setup.off")}</span></h3>
      <p class="hint">{t("setup.text_hint")}</p>
      {#if !s.search_text}<button class="primary" onclick={() => set({ search_text: true })}>{t("setup.turn_on")}</button>{:else if index}<p class="hint">{t("setup.text_count", { files: index.texts ?? 0 })}</p>{/if}
    </section>

    <section>
      <h3>{t("setup.step_server")} {#if meaningNow}<span class="state ok">{meaningNow}</span>{/if}</h3>
      <p class="hint">{t("setup.server_hint")}</p>
      {#if !look}<p class="hint">{t("setup.looking")}</p>
      {:else}
        <p>{machineLine}</p>
        <p class="hint">{look.advice.why}</p>
        <div class="choices" role="radiogroup">
          {#each servers as f (f.url)}
            <label class="choice" class:on={server?.url === f.url}><input type="radio" name="server" checked={server?.url === f.url} onchange={() => (chosen = f)} />
              <span>{t("setup.found", { server: name(f.kind), url: f.url, embed: f.models.filter((m) => m.embed).length, chat: f.models.filter((m) => m.chat).length })}
                {#if look.best != null && look.found[look.best]?.url === f.url}<span class="badge">{t("setup.recommended")}</span>{/if}
                {#if isRemote(f.url)}<br /><strong>{t("settings.meaning_remote", { host: f.url })}</strong>{/if}</span></label>
          {/each}
          <label class="choice" class:on={server === null}><input type="radio" name="server" checked={server === null} onchange={() => (chosen = null)} />
            <span>{t("setup.builtin", { size: size(meaning?.size ?? 0), where: look.builtin_runs })}{#if look.advice.server == null || !look.found.length}<span class="badge">{t("setup.recommended")}</span>{/if}</span></label>
        </div>
        {#if !look.found.length}<p class="hint">{t("setup.none_found")}</p>{/if}
        <div class="row">
          <input placeholder="http://192.168.1.20:11434" bind:value={remoteUrl} spellcheck="false" aria-label={t("setup.other")} />
          <button disabled={!remoteUrl.trim()} onclick={probeRemote}>{t("setup.look")}</button>
          <button onclick={probe}>{t("setup.look_again")}</button>
        </div>
        {#if remote === false}<p class="err">{t("setup.nobody", { url: remoteUrl })}</p>{/if}
        <p class="hint">{t("setup.other_hint")}</p>
      {/if}
    </section>

    <section>
      <h3>{t("setup.step_embed")}</h3>
      {#if server === null}
        <p class="hint">{t("setup.builtin_hint")}</p>
        {#if meaning?.downloading}<p class="hint">{t("setup.downloading", { percent: Math.floor((meaning.downloading[0] * 100) / Math.max(1, meaning.downloading[1])) })}</p>
        {:else if s.search_meaning && s.meaning_engine === "builtin" && meaning?.installed}<p class="ok">{t("setup.builtin_on")}</p>
        {:else}<button class="primary" onclick={useBuiltin}>{meaning?.installed ? t("setup.use_builtin") : t("setup.download_builtin", { size: size(meaning?.size ?? 0) })}</button>{/if}
        {#if meaning?.error}<p class="err">{meaning.error}</p>{/if}
      {:else if server}
        <p class="hint">{t("setup.embed_hint", { model: suggestEmbed || "bge-m3" })}</p>
        <div class="row">
          <select bind:value={embed} aria-label={t("setup.step_embed")}>{#each embedModels as m (m)}<option value={m}>{m}</option>{/each}</select>
          <button class="primary" disabled={!embed} onclick={useEmbed}>{t("setup.use")}</button>
          {#if suggestEmbed && !embedModels.some((m) => m.startsWith(suggestEmbed))}<button disabled={!!meaning?.downloading} onclick={() => pull(suggestEmbed)}>{t("setup.pull", { model: suggestEmbed })}</button>{/if}
        </div>
        {#if meaning?.downloading}<p class="hint">{t("setup.downloading", { percent: Math.floor((meaning.downloading[0] * 100) / Math.max(1, meaning.downloading[1])) })}</p>{/if}
        {#if meaning?.error}<p class="err">{meaning.error}</p>{/if}
      {/if}
      {#if vectorsChange}
        <p><strong>{vectorsChange.why}</strong></p>
        <div class="row">
          <button class="primary" onclick={vectorsChange.builtin ? useBuiltin : useEmbed}>{t("settings.meaning_change_go")}</button>
          <button onclick={() => (vectorsChange = null)}>{t("settings.meaning_change_keep")}</button>
        </div>
      {/if}
    </section>

    <section>
      <h3>{t("setup.step_ask")} {#if s.ask_model}<span class="state ok">{ui.cfg.ask_name}</span>{/if}</h3>
      <p class="hint">{t("setup.ask_builtin_hint")}</p>
      {#if chatBuiltin}
        <div class="row">
          <select bind:value={builtinChat} aria-label={t("setup.step_ask")}>{#each chatBuiltin.models as m (m.key)}<option value={m.key}>{t("setup.ask_builtin", { model: m.name, size: size(m.size), where: chatBuiltin.runs })}</option>{/each}</select>
          {#if !askServer && builtinChosen?.suggested}<span class="badge">{t("setup.recommended")}</span>{/if}
          <button class:primary={!askServer && builtinChosen?.suggested} disabled={!builtinChosen || !!chatBuiltin.downloading || trial?.busy} onclick={useBuiltinChat}>{builtinChosen?.installed ? t("setup.use_try") : t("setup.download_chat", { model: builtinChosen?.name ?? "", size: size(builtinChosen?.size ?? 0) })}</button>
        </div>
        {#if builtinChosen?.estimate}<p class="hint">{builtinChosen.estimate}</p>{/if}
        {#if chatBuiltin.downloading}<p class="hint">{t("setup.downloading", { percent: Math.floor((chatBuiltin.downloading[1] * 100) / Math.max(1, chatBuiltin.downloading[2])) })}</p>{/if}
        {#if chatBuiltin.error}<p class="err">{chatBuiltin.error}</p>{/if}
      {/if}
      {#if askServer}
        <p class="hint">{t("setup.ask_hint", { model: suggestChat || chatModels[0] || "qwen3:8b" })}</p>
        <div class="row">
          <select bind:value={chat} aria-label={t("setup.step_ask")}>{#each chatModels as m (m)}<option value={m}>{m}</option>{/each}</select>
          <button class="primary" disabled={!chat || trial?.busy} onclick={() => useChat()}>{t("setup.use_try")}</button>
          {#if suggestChat && !chatModels.some((m) => m.startsWith(suggestChat))}<button disabled={!!meaning?.downloading} onclick={() => pull(suggestChat)}>{t("setup.pull", { model: suggestChat })}</button>{/if}
        </div>
      {/if}
      {#if trial?.busy}<p class="hint">{t("dialogs.ask_waiting", { model: ui.cfg.ask_name })}</p>{/if}
      {#if trial?.ms != null}<p class="ok">{t("setup.try_done", { seconds: (trial.ms / 1000).toFixed(1) })}</p>{/if}
      {#if trial?.error}<p class="err">{trial.error}</p>{/if}
    </section>

    <section>
      <h3>{t("setup.step_speed")}</h3>
      {#if s.ask_model.startsWith("builtin:")}<p class="hint">{t("setup.speed_builtin", { where: chatBuiltin?.runs ?? "" })}</p>
      {:else if !look?.machine?.gpu && !look?.machine?.npu}<p class="hint">{t("setup.speed_no_gpu")}</p>
      {:else if !speedChecked}<p class="hint">{t("setup.speed_hint")}</p>
      {:else if speed}<p><strong>{speed}</strong></p>
      {:else}<p class="ok">{t("setup.speed_ok")}</p>{/if}
    </section>

    <section>
      <h3>{t("setup.step_service")} <span class="state" class:ok={index?.service}>{index?.service ? t("setup.on") : t("setup.off")}</span></h3>
      <p class="hint">{t("setup.service_hint")}</p>
      <label class="check"><input type="checkbox" checked={index?.service} disabled={!index} onchange={(e) => service(e.currentTarget.checked)} /> {t("setup.service_on")}</label>
    </section>

    <section>
      <h3>{t("setup.step_done")}</h3>
      <ul>
        <li>{t("setup.sum_text", { state: s.search_text ? t("setup.on") : t("setup.off") })}</li>
        <li>{t("setup.sum_meaning", { state: meaningNow || t("setup.off") })}</li>
        <li>{t("setup.sum_ask", { state: s.ask_model ? ui.cfg.ask_name : t("setup.off") })}</li>
        <li>{t("setup.sum_service", { state: index?.service ? t("setup.on") : t("setup.off") })}</li>
      </ul>
      <button class="primary" onclick={close}>{t("setup.done")}</button>
    </section>
  </div>
</div>

<style>
  .setup-search {
    position: fixed;
    inset: 5vh 14vw;
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
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font);
  }
  h3 {
    margin: 0 0 6px;
    font-size: 0.95em;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  section {
    border-top: 1px solid var(--border-fg);
    padding-top: 10px;
  }
  p {
    margin: 4px 0;
  }
  button,
  input,
  select {
    font: inherit;
    color: inherit;
  }
  button {
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 12px;
    cursor: pointer;
  }
  input:not([type="checkbox"], [type="radio"]),
  select {
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 8px;
    min-width: 16em;
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
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-top: 6px;
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 6px 0;
  }
  .choice,
  .check {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  /* The chosen server stands out, whatever the theme does to radio buttons. */
  .choice {
    padding: 2px 6px;
    border: 1px solid transparent;
    border-radius: var(--r);
    cursor: pointer;
  }
  .choice.on {
    border-color: var(--accent-bg);
    font-weight: bold;
  }
  input[type="radio"],
  input[type="checkbox"] {
    accent-color: var(--accent-bg);
  }
  .badge,
  .state {
    font-size: 0.8em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 0 6px;
    margin-inline-start: 6px;
    font-weight: normal;
  }
  .badge,
  .state.ok,
  .ok {
    color: var(--git-added-fg);
  }
  .hint {
    color: var(--hidden-fg);
    font-size: 0.9em;
    overflow-wrap: anywhere;
  }
  .err {
    color: var(--git-deleted-fg);
  }
</style>

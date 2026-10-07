<script>
  // Settings, by task: a list of areas on the left, the area on the right. Every option is
  // described once in coxswain-core (coxswain_core::settings: its area, config key and costs;
  // its label and one-line explanation are `setting.<name>` texts), written to config.toml at
  // once (comments and layout kept) and applied without a restart.
  import { tick, untrack } from "svelte";
  import { ui, setTheme, themeIds, themeName, tab, cd } from "./app.svelte.js";
  import { invoke, size, parent } from "./lib.js";
  import { t, setLanguage } from "./i18n.svelte.js";
  import CopyLine from "./CopyLine.svelte";
  import derafsh from "../../docs/flags/derafsh.svg?url";

  const flags = import.meta.glob("../node_modules/flag-icons/flags/4x3/{gb,au,ca,nz,dk,se,fi,ee,lv,lt,de,at,ch,fr,it,nl,ar,es-ct,es-pv,il,pl,cz,ua,gr,jp,kr,am,ge}.svg", {
    query: "?url",
    import: "default",
    eager: true,
  });
  // Persian's banner is ours, kept with the docs' flags.
  const flag = (name) => (name === "derafsh" ? derafsh : flags[`../node_modules/flag-icons/flags/4x3/${name}.svg`]);

  const AREAS = ["overview", "search", "previews", "looks", "behaviour", "keys", "privacy"];
  const s = $derived(ui.cfg.settings);
  const optOf = (name) => ui.cfg.options.find((o) => o.name === name);
  /** An option's place in config.toml, as written there: `[search] text`. */
  const keyOf = (name) => {
    const p = optOf(name)?.path ?? [];
    return p.length > 1 ? `[${p.slice(0, -1).join(".")}] ${p.at(-1)}` : (p[0] ?? "");
  };
  let error = $state("");
  /** Settings → Behaviour → Show the hints again was pressed. */
  let hintsReset = $state(false);
  let saved = $state(false);
  const close = () => (ui.modal = null);

  /** Save options and use the new config everywhere (the helper restarts when it must). */
  async function save(changes) {
    error = "";
    try {
      const cfg = await invoke("save_settings", { changes });
      ui.cfg = cfg;
      setLanguage(cfg);
      setTheme(cfg.settings.theme);
      saved = true;
      loadIndex();
    } catch (e) {
      error = String(e);
    }
  }
  const set = (name, value) => save({ [name]: value });

  // ------------------------------------------------------------ areas, and where it opens

  let area = $state("overview");
  let query = $state("");
  let flash = $state("");
  /** Opens `section` (an area or an option, see settings::open_at): its
   *  area, scrolled to the option with the details around it open. */
  async function go(section) {
    const hit = ui.cfg.sections.find(([n]) => n === (section ?? ""));
    const [a, at] = hit ? [hit[1], hit[2]] : ["overview", null];
    area = a;
    query = "";
    if (!at) return;
    await tick();
    const el = document.getElementById(`opt-${at}`);
    if (!el) return;
    for (let d = el.closest("details"); d; d = d.parentElement?.closest("details")) d.open = true;
    el.scrollIntoView({ block: "center" });
    flash = at;
    setTimeout(() => flash === at && (flash = ""), 1600);
  }
  $effect(() => {
    const section = ui.modal?.section;
    untrack(() => go(section));
  });
  $effect(() => {
    if (area === "previews") untrack(() => (loadImages(), loadPreviewCache()));
  });

  /** "Find a setting": every option whose label, explanation or config key holds the words. */
  const found = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return ui.cfg.options.filter((o) => [t(`setting.${o.name}`), t(`setting.${o.name}.hint`), o.name, keyOf(o.name)].some((x) => x.toLowerCase().includes(q)));
  });

  // ------------------------------------------------------------ search: status and level

  let index = $state(null);
  let lines = $state([]);
  const loadIndex = () => {
    invoke("index_status").then((v) => (index = v), () => (index = null));
    invoke("search_status").then((v) => (lines = v), () => {});
  };
  loadIndex();
  $effect(() => {
    const id = setInterval(loadIndex, 2000);
    return () => clearInterval(id);
  });
  const openSetup = () => (ui.modal = { kind: "setup" });
  /** A level: what it needs is turned on, or the setup guide opens to choose a model. */
  async function chooseLevel(level) {
    const changes = await invoke("search_level", { level }).catch((e) => ((error = String(e)), undefined));
    if (changes === undefined) return;
    if (!changes) return openSetup();
    await save(changes);
  }
  // Ask's test question: how long the first word took.
  let trial = $state(null);
  async function tryAsk() {
    trial = { busy: true };
    try {
      trial = { ms: await invoke("setup_try") };
    } catch (e) {
      trial = { error: String(e) };
    }
  }
  async function step(line) {
    error = "";
    if (line.step === "turn_on") return chooseLevel("text");
    if (line.step === "set_up") return openSetup();
    if (line.step === "try_it") return tryAsk();
    await invoke("index_action", { what: line.step === "read_now" ? "now" : "restart" }).catch((e) => (error = String(e)));
    loadIndex();
  }

  // ------------------------------------------------------------ search: details

  /** Settings closes and the active panel shows `path`: a folder opened, a file with the cursor on it. */
  async function showInPanel(path, isFile = true) {
    close();
    const tb = tab();
    await cd(tb, isFile ? parent(path) : path);
    const i = tb.items.findIndex((e) => e.path === path);
    if (i >= 0) tb.cursor = i;
  }
  const adding = $state({});
  /** What the store has of a folder read: its bytes, and whether its disk is away. */
  const rootInfo = (dir) => {
    const r = index?.roots?.find(([p]) => p === dir);
    if (!r) return "";
    const [, at, [bytes]] = r;
    return at ? t("settings.search_root_size", { size: size(bytes) }) : t("settings.search_root_away", { size: size(bytes) });
  };
  async function addFolder(name, input) {
    const dir = await invoke("resolve_path", { base: tab()?.dir ?? ui.cfg.home, input: input || tab()?.dir || "" });
    adding[name] = "";
    if (!s[name].includes(dir)) set(name, [...s[name], dir]);
  }
  function addWord(name) {
    const word = (adding[name] ?? "").trim();
    adding[name] = "";
    if (word && !s[name].includes(word)) set(name, [...s[name], word]);
  }
  async function serviceSet(on) {
    error = "";
    await invoke("index_service", { on }).catch((e) => (error = String(e)));
    loadIndex();
  }
  // The built-in model: downloaded or not, its download, and whether meaning is on.
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
  // A model server makes the vectors instead: its models are listed.
  const server = $derived(s.meaning_engine === "builtin" ? null : s.meaning_engine);
  const wanted = $derived(s.meaning_model || "bge-m3");
  let models = $state({ ok: false, list: [], error: "" });
  $effect(() => {
    const [engine, url] = [server, s.meaning_url];
    if (!engine) return;
    invoke("meaning_models", { engine, url }).then((list) => (models = { ok: true, list, error: "" }), (e) => (models = { ok: false, list: [], error: String(e) }));
  });
  // Ask's chat models (only those that can chat): on the same server, or Ollama here.
  let chatModels = $state([]);
  $effect(() => {
    const [engine, url] = [server ?? "ollama", server ? s.meaning_url : ""];
    invoke("meaning_models", { engine, url, chat: true }).then((list) => (chatModels = list), () => (chatModels = []));
  });
  // Why the chat model cannot answer: asked when Settings opens, tried when it is saved.
  let askProblem = $state(null);
  $effect(() => {
    if (s.ask_model && s.search_meaning) invoke("ask_check", { tryIt: false }).then((p) => (askProblem = p), () => {});
    else askProblem = null;
  });
  async function setAskModel(model) {
    await set("ask_model", model);
    askProblem = model ? await invoke("ask_check", { tryIt: true }).catch((e) => String(e)) : null;
  }
  // Ask off: the field is never empty but offers a model, a server's first, else the
  // built-in one for this machine; Use sets it (downloading a built-in one first).
  const askPreselected = $derived(chatModels[0] ?? chat?.preselected ?? "");
  function useAskPreselected() {
    const m = chat?.models.find((x) => x.key === askPreselected);
    if (m && !m.installed) chatAction("download", m.key);
    else setAskModel(askPreselected);
  }
  // The built-in chat models: downloaded or not, and a download under way.
  let chat = $state(null);
  const loadChat = () => invoke("chat_status").then((v) => (chat = v), () => (chat = null));
  loadChat();
  $effect(() => {
    const id = setInterval(loadChat, chat?.downloading ? 500 : 3000);
    return () => clearInterval(id);
  });
  async function chatAction(what, model) {
    error = "";
    await invoke("chat_action", { what, model }).catch((e) => (error = String(e)));
    loadChat();
  }
  const builtinAsk = $derived(s.ask_model.startsWith("builtin:"));
  // A change of the vectors' model reads every file's meaning again: said, and confirmed, first.
  let vectorsChange = $state(null);
  async function setVectors(el, name, value) {
    const why = await invoke("meaning_change", { name, value }).catch(() => null);
    if (!why) return set(name, value);
    vectorsChange = { why, go: () => set(name, value), keep: () => (el.value = s[name]) };
  }
  // A pull that finished brings its model into the list.
  let pulling = false;
  $effect(() => {
    const now = !!meaning?.downloading;
    if (pulling && !now && server) invoke("meaning_models", { engine: server, url: s.meaning_url }).then((list) => (models = { ok: true, list, error: "" }), () => {});
    pulling = now;
  });
  const hostOf = (url) => url.replace(/^\w+:\/\//, "").split(/[/?#]/)[0];
  const isRemote = (url) => !/^(localhost|127\.|\[::1\])/.test(hostOf(url));
  const serverUrl = $derived(s.meaning_url || (server !== "openai" ? "http://localhost:11434" : ""));
  async function pull(model) {
    error = "";
    await invoke("meaning_pull", { model, url: s.meaning_url }).catch((e) => (error = String(e)));
    loadMeaning();
  }
  // Deleting what was read takes a second click.
  let forgetting = $state(false);
  async function forget() {
    if (!forgetting) return (forgetting = true);
    forgetting = false;
    await invoke("index_action", { what: "forget" }).catch((e) => (error = String(e)));
    loadIndex();
  }

  // ------------------------------------------------------------ previews

  // Container images, with Pull (also updates) and Remove: asked only while Previews is shown,
  // as it asks the container runtime.
  let images = $state(null);
  const loadImages = () => invoke("images").then((v) => (images = v), (e) => (images = String(e)));
  async function imageAction(cmd, im) {
    error = "";
    im.pulling = "";
    await invoke(cmd, { image: im.image }).catch((e) => (error = String(e)));
    loadImages();
  }
  $effect(() => {
    if (area !== "previews" || !Array.isArray(images) || !images.some((im) => im.pulling != null)) return;
    const id = setInterval(loadImages, 1000);
    return () => clearInterval(id);
  });
  let previewCache = $state(null);
  const loadPreviewCache = () => invoke("preview_cache").then((v) => (previewCache = v), () => (previewCache = null));
  async function clearPreviewCache() {
    await invoke("clear_preview_cache").catch((e) => (error = String(e)));
    loadPreviewCache();
  }
  const imageStatus = (im) => (im.pulling != null ? im.pulling || t("common.loading") : im.size != null ? t("settings.image_pulled", { size: size(im.size) }) : t("settings.image_not_pulled"));

  // ------------------------------------------------------------ looks

  const current = $derived(ui.cfg.languages.find((l) => l.code === ui.cfg.language));
  let langFilter = $state("");
  /** The languages under their regions, those the filter leaves: [group key, languages]. */
  const langGroups = $derived(
    ui.cfg.languages
      .filter((l) => !langFilter.trim() || `${l.name} ${l.code}`.toLowerCase().includes(langFilter.trim().toLowerCase()))
      .reduce((g, l) => {
        if (g.at(-1)?.[0] !== l.group) g.push([l.group, []]);
        g.at(-1)[1].push(l);
        return g;
      }, []),
  );
  const themes = $derived(themeIds());
  let themeFor = $state("theme");
  /** The theme brings its own font (every look but the modern one): the Font field does nothing. */
  const ownFont = $derived(ui.cfg.looks[s.theme] !== "modern");

  // ------------------------------------------------------------ keys

  let keyFilter = $state("");
  const keysOf = (action) => Object.entries(ui.cfg.keymap).filter(([, a]) => a === action).map(([k]) => k);
  const keyGroups = $derived(
    ui.cfg.groups
      .map(([label, acts]) => [label, acts.filter((a) => {
        const q = keyFilter.trim().toLowerCase();
        return !q || `${ui.cfg.actions[a]?.[0] ?? a} ${a} ${keysOf(a).join(" ")}`.toLowerCase().includes(q);
      })])
      .filter(([, acts]) => acts.length),
  );
  const openKeys = () => invoke("open_keys").catch((e) => (error = String(e)));
  const openScripts = () => invoke("scripts_folder").then((dir) => showInPanel(dir, false), (e) => (error = String(e)));
  const openConfig = () => invoke("open_path", { path: ui.cfg.config_path }).catch((e) => (error = String(e)));

  // ------------------------------------------------------------ what's new

  // Every version's changes. Those not read yet are marked, and count as read once in sight.
  let changes = $state([]);
  invoke("changes").then((v) => (changes = v), () => {});
  const fresh = new Set(ui.news.unread);
  const shownOpen = $derived(changes.filter((c, i) => fresh.has(c.version) || (!fresh.size && i === 0)));
  const earlier = $derived(changes.slice(shownOpen.length));
  const newsCount = $derived(ui.news.notices.length + ui.news.unread.length);
  function seen(el) {
    const o = new IntersectionObserver(([e]) => {
      if (!e.isIntersecting || !ui.news.unread.length) return;
      ui.news.unread = [];
      invoke("read_changes").catch(() => {});
    });
    o.observe(el);
    return { destroy: () => o.disconnect() };
  }
  let flashCopied = $state("");
  function actOnNotice(n) {
    invoke("dismiss_notice", { id: n.id }).catch(() => {});
    ui.news.notices = ui.news.notices.filter((x) => x.id !== n.id);
    if (n.settings) go(n.settings);
  }

  /** A number field: saved when it holds a number in its range (× `scale`); emptied or out of
   *  range, it goes back to what is saved. */
  function number(e, name, scale = 1) {
    const el = e.currentTarget;
    if (el.validity.valid && el.value !== "") set(name, Number.isInteger(el.valueAsNumber * scale) ? el.valueAsNumber * scale : Math.round(el.valueAsNumber * scale * 100) / 100);
    else el.value = s[name] / scale;
  }
  const levelName = (l) => (l ? t(`settings.level.${l}`) : t("settings.level.custom"));
</script>

<!-- An option: its label with its cost badges, the control, and one line on what it does. -->
{#snippet head(name)}
  <span class="lab">{t(`setting.${name}`)}</span>
  {#each optOf(name)?.costs ?? [] as c (c)}<span class="badge" class:out={c === "leaves"}>{t(`settings.cost.${c}`)}</span>{/each}
{/snippet}
{#snippet check(name, value = !!s[name], onchange = (v) => set(name, v), sub = false)}
  <div class="opt" class:sub id="opt-{name}" class:flash={flash === name} title={keyOf(name)}>
    <label class="check"><input type="checkbox" checked={value} onchange={(e) => onchange(e.currentTarget.checked)} /> {@render head(name)}</label>
    <p class="hint">{t(`setting.${name}.hint`)}</p>
  </div>
{/snippet}
{#snippet field(name, control)}
  <div class="opt" id="opt-{name}" class:flash={flash === name} title={keyOf(name)}>
    <label class="field" for="in-{name}">{@render head(name)}</label>
    {@render control()}
    <p class="hint">{t(`setting.${name}.hint`)}</p>
  </div>
{/snippet}
{#snippet text(name, placeholder = "", disabled = false)}
  {#snippet control()}<input id="in-{name}" value={s[name]} {placeholder} {disabled} spellcheck="false" onchange={(e) => set(name, e.currentTarget.value.trim())} />{/snippet}
  {@render field(name, control)}
{/snippet}
{#snippet num(name, min, max, stepBy = 1, scale = 1)}
  {#snippet control()}<input id="in-{name}" class="short" type="number" {min} {max} step={stepBy} value={s[name] / scale} onchange={(e) => number(e, name, scale)} />{/snippet}
  {@render field(name, control)}
{/snippet}
{#snippet choice(name, pairs)}
  {#snippet control()}
    <select id="in-{name}" value={s[name]} onchange={(e) => set(name, e.currentTarget.value)}>
      {#each pairs as [value, label] (value)}<option {value}>{label}</option>{/each}
    </select>
  {/snippet}
  {@render field(name, control)}
{/snippet}
<!-- A list of folders (or of names and patterns: `words`), each with Remove, and a field to add one. -->
{#snippet list(name, none, words = false)}
  {#snippet control()}
    <div class="folders">
      {#if words}
        <div class="chips">
          {#each s[name] as w (w)}
            <span class="chip mono">{w}<button title={t("common.remove")} aria-label={t("common.remove")} onclick={() => set(name, s[name].filter((x) => x !== w))}>×</button></span>
          {:else}
            <span class="hint">{none}</span>
          {/each}
        </div>
      {:else}
        {#each s[name] as dir (dir)}
          <div class="folder">
            <span class="mono">{dir}{#if name === "text_roots"}<small class="hint">{rootInfo(dir)}</small>{/if}</span>
            <button onclick={() => set(name, s[name].filter((d) => d !== dir))}>{t("common.remove")}</button>
          </div>
        {:else}
          <span class="hint">{none}{#if name === "text_roots" && index?.roots?.[0]}{" · "}{rootInfo(index.roots[0][0])}{/if}</span>
        {/each}
      {/if}
      <form class="folder" onsubmit={(e) => (e.preventDefault(), words ? addWord(name) : addFolder(name, adding[name]))}>
        <input id="in-{name}" bind:value={adding[name]} spellcheck="false" placeholder={words ? "*.log" : tab()?.dir} />
        <button type="submit">{t("settings.search_add")}</button>
      </form>
    </div>
  {/snippet}
  {@render field(name, control)}
{/snippet}

<div class="settings" role="dialog" aria-modal="true" aria-label={t("settings.title")}>
  <header>
    <h2>{"\u{f013}"} {t("settings.title")}</h2>
    <button class="x" title={t("common.close")} onclick={close}>×</button>
  </header>

  <div class="main">
    <nav aria-label={t("settings.title")}>
      <!-- Esc empties it first; a second Esc closes Settings. -->
      <input class="find" type="search" bind:value={query} placeholder={t("settings.find")} aria-label={t("settings.find")} onkeydown={(e) => e.key === "Escape" && query && ((query = ""), e.stopPropagation())} />
      {#each AREAS as a (a)}
        <button class="area" class:on={!query && area === a} aria-current={!query && area === a ? "page" : undefined} onclick={() => ((area = a), (query = ""))}>
          {t(`settings.area.${a}`)}{#if a === "overview" && newsCount}<span class="new">{newsCount}</span>{/if}
        </button>
      {/each}
    </nav>

    <div class="body">
      {#if query}
        <!-- ------------------------------------------------ Find a setting -->
        {#each found as o (o.name)}
          <button class="result" onclick={() => go(o.name)}>
            <span><strong>{t(`setting.${o.name}`)}</strong> <small class="hint">{t(`settings.area.${o.area}`)} · <span class="mono">{keyOf(o.name)}</span></small></span>
            <small class="hint">{t(`setting.${o.name}.hint`)}</small>
          </button>
        {:else}
          <p class="hint">{t("settings.find_none")}</p>
        {/each}
      {:else if area === "overview"}
        <!-- ------------------------------------------------ Overview -->
        <h3>{t("settings.area.overview")}</h3>
        <div class="status">
          <button class="link strong" onclick={() => (area = "search")}>{t("settings.area.search")}</button>
          <span>{levelName(ui.cfg.level)}{#each lines.filter((l) => l.part !== "names") as l (l.part)}<br /><small class="hint">{l.label}: {l.text}</small>{#if l.note}<small class="hint" class:err={l.bad}>{" · "}{l.note}</small>{/if}{/each}</span>
          <span></span>
          <button class="link strong" onclick={() => (area = "previews")}>{t("settings.area.previews")}</button>
          <span>{t({ auto: "settings.prefer_auto", local: "settings.prefer_local", container: "settings.prefer_container" }[s.preview_prefer] ?? "settings.prefer_auto")}</span>
          <span></span>
          <button class="link strong" onclick={() => (area = "looks")}>{t("settings.area.looks")}</button>
          <span>{themeName(s.theme)} · {current?.name}</span>
          <span></span>
          <button class="link strong" onclick={() => (area = "privacy")}>{t("settings.area.privacy")}</button>
          <span>
            {#each ui.cfg.outbound.filter((o) => !o.local) as o, i (i)}{#if i}<br />{/if}{o.what} → <span class="mono">{o.to}</span>{:else}{t("settings.privacy_nothing")}{/each}
          </span>
          <span></span>
        </div>
        <div class="buttons"><button class="primary" onclick={openSetup}>{t("setup.open")}</button> <span class="hint">{t("setup.open_hint")}</span></div>
        <div class="buttons"><button onclick={() => (ui.modal = { kind: "guide", step: 0 })}>{t("guide.show_again")}</button> <span class="hint">{t("guide.show_again_hint")}</span></div>

        <section id="opt-news" use:seen>
          <h3>{t("news.title")}</h3>
          {#if ui.news.notices.length}
            <p class="label">{t("news.for_you")}</p>
            {#each ui.news.notices as n (n.id)}
              <div class="tip">
                <span>{n.text}</span>
                {#if n.copy}<button title={n.copy} onclick={() => invoke("copy_text", { text: n.copy }).then(() => (flashCopied = n.id), (e) => (error = String(e)))}>{flashCopied === n.id ? t("common.copied") : t("common.copy")}</button>{/if}
                {#if n.settings}<button onclick={() => actOnNotice(n)}>{t("news.show_me")}</button>{/if}
                <button title={t("news.dismiss_hint")} onclick={() => actOnNotice({ ...n, settings: null })}>{t("news.dismiss")}</button>
              </div>
            {/each}
          {/if}
          {#snippet change(c)}
            <div class="change">
              <p class="label">{c.version} <small class="hint">{c.date}</small>{#if fresh.has(c.version)} <span class="new">{t("news.new")}</span>{/if}</p>
              <p class="what">{#each c.parts as [text, url], i (i)}{#if url}<button class="link" onclick={() => invoke("open_path", { path: url })}>{text}</button>{:else}{text}{/if}{/each}</p>
            </div>
          {/snippet}
          {#each shownOpen as c (c.version)}{@render change(c)}{/each}
          {#if earlier.length}
            <details>
              <summary>{t("news.earlier", { n: earlier.length })}</summary>
              {#each earlier as c (c.version)}{@render change(c)}{/each}
            </details>
          {/if}
        </section>
      {:else if area === "search"}
        <!-- ------------------------------------------------ Finding files -->
        <h3>{t("settings.area.search")}</h3>
        <div class="status">
          {#each lines as l (l.part)}
            <strong>{l.label}</strong>
            <span>{l.text}
              {#if l.note}<br /><small class="hint" class:err={l.bad}>{l.note}</small>{/if}
              {#if l.part === "meaning" && index?.meaning_runs_text && s.meaning_engine === "builtin"}<br /><small class="hint">{index.meaning_runs_text}</small>{/if}
              {#if l.part === "ask" && trial}<br /><small class="hint" class:err={trial.error}>{trial.busy ? t("common.loading") : trial.error ?? t("setup.try_done", { seconds: (trial.ms / 1000).toFixed(1) })}</small>{/if}
            </span>
            {#if l.step}<button disabled={trial?.busy && l.step === "try_it"} onclick={() => step(l)}>{t(`settings.step.${l.step}`)}</button>{:else}<span></span>{/if}
          {/each}
        </div>

        <fieldset class="levels">
          <legend>{t("settings.level")}</legend>
          {#each ui.cfg.levels as [l, costs] (l)}
            <label class="level" class:on={ui.cfg.level === l}>
              <input type="radio" name="level" checked={ui.cfg.level === l} onchange={() => chooseLevel(l)} />
              <span><span class="lab">{t(`settings.level.${l}`)}</span>{#each costs as c (c)}<span class="badge" class:out={c === "leaves"}>{t(`settings.cost.${c}`)}</span>{/each}
                <small class="hint">{t(`settings.level.${l}.hint`)}</small></span>
            </label>
          {/each}
          {#if !ui.cfg.level}<p class="hint err">{t("settings.level.custom")}</p>{/if}
        </fieldset>
        <div class="buttons"><button class="primary" onclick={openSetup}>{t("setup.open")}</button> <span class="hint">{t("setup.open_hint")}</span></div>
        <div class="buttons"><button onclick={() => (ui.modal = { kind: "guide", step: 0 })}>{t("guide.show_again")}</button> <span class="hint">{t("guide.show_again_hint")}</span></div>

        <details class="details">
          <summary>{t("settings.details")}</summary>

          <details class="group" open>
            <summary>{t("settings.group.reads")}</summary>
            {@render check("search_text")}
            {@render check("search_archives")}
            {#if s.search_archives}{@render check("search_archives_everywhere", undefined, undefined, true)}{/if}
            {@render check("search_history")}
            {@render check("search_cloud", s.search_cloud === "all", (v) => set("search_cloud", v ? "all" : "local-only"))}
            {#if s.search_cloud !== "all"}
              <!-- The clouds found, offered first; then the folders read anyway. -->
              {#each (index?.clouds ?? []).filter(([, dir]) => !s.cloud_read.includes(dir)) as [name, dir] (dir)}
                <div class="folder sub">
                  <span class="mono">{name}<small class="hint">{dir}</small></span>
                  <button onclick={() => set("cloud_read", [...s.cloud_read, dir])}>{t("settings.search_cloud_read_one")}</button>
                </div>
              {/each}
              {@render list("cloud_read", t("settings.search_names_only_none"))}
            {/if}
            {#if index?.tools?.length}
              <div class="opt">
                <span class="lab">{t("settings.search_tools")}</span>
                <ul class="tools">
                  {#each index.tools as [name, there] (name)}
                    <li class:missing={!there}>{there ? "✓" : "✗"} {t(`settings.search_tool_${name}`)}{#if !there}<small class="hint">{" · "}{t("settings.search_tool_missing")}</small>
                      <br /><small>{#if ui.cfg.installs[name]}{t("install.with")} <CopyLine line={ui.cfg.installs[name]} />{:else}{t("install.get", { program: name })}{/if}</small>{/if}</li>
                  {/each}
                </ul>
              </div>
            {/if}
            {@render num("text_max_size", 1, 4096, 1, 1048576)}
            {@render num("max_results", 10, 1000000)}
          </details>

          <details class="group">
            <summary>{t("settings.group.folders")}</summary>
            {@render list("text_roots", t("settings.search_roots_home"))}
            {@render list("names_only", t("settings.search_names_only_none"))}
            {@render list("text_exclude", t("settings.search_names_only_none"), true)}
            {@render list("name_roots", t("settings.search_names_only_none"))}
            {@render list("name_exclude", t("settings.search_names_only_none"), true)}
            {@render check("watch")}
          </details>

          <details class="group" id="opt-meaning">
            <summary>{t("settings.group.meaning")}</summary>
            {#snippet meaningSwitch()}
              {#if meaning?.downloading}
                <p class="hint">{t("settings.meaning_downloading", { done: size(meaning.downloading[0]), total: size(meaning.downloading[1]) })}</p>
                <progress max={meaning.downloading[1] || 1} value={meaning.downloading[0]}></progress>
                <div class="buttons"><button onclick={() => meaningAction("cancel")}>{t("common.cancel")}</button></div>
              {:else}
                {#if meaning?.error}<p class="err">{meaning.error}</p>{/if}
                <div class="buttons">
                  {#if s.search_meaning}
                    <button onclick={() => meaningAction("off")}>{t("settings.meaning_off")}</button>
                  {:else if server || meaning?.installed}
                    <button class="primary" onclick={() => save({ search_text: true, search_meaning: true })}>{t("settings.meaning_on")}</button>
                  {:else}
                    <button class="primary" onclick={() => (s.search_text ? meaningAction("download") : save({ search_text: true }).then(() => meaningAction("download")))}>{t("settings.meaning_download", { size: size(meaning?.size ?? 0) })}</button>
                  {/if}
                  {#if !server && meaning?.installed}<button onclick={() => meaningAction("remove")}>{t("settings.meaning_remove")}</button>{/if}
                </div>
                {#if !server && meaning?.folder && meaning?.installed}<p class="hint"><span class="mono">{meaning.folder}</span> <button class="link" onclick={() => showInPanel(meaning.folder, false)}>{t("settings.show_in_panel")}</button></p>{/if}
              {/if}
            {/snippet}
            {@render field("search_meaning", meaningSwitch)}
            {#snippet engine()}
              <select id="in-meaning_engine" value={s.meaning_engine} onchange={(e) => setVectors(e.currentTarget, "meaning_engine", e.currentTarget.value)}>
                <option value="builtin">{t("settings.meaning_builtin", { size: size(meaning?.size ?? 0) })}</option>
                <option value="ollama">Ollama</option>
                <option value="openai">{t("settings.meaning_openai")}</option>
              </select>
            {/snippet}
            {@render field("meaning_engine", engine)}
            {#if server}
              {#snippet url()}
                <input id="in-meaning_url" value={s.meaning_url} spellcheck="false" placeholder={server === "ollama" ? "http://localhost:11434" : "http://localhost:8000/api/v1"} onchange={(e) => set("meaning_url", e.currentTarget.value.trim())} />
                <p class="hint">{models.ok ? t("settings.meaning_server_ok") : models.error}{#if isRemote(serverUrl)}<br /><strong>{t("settings.meaning_remote", { host: hostOf(serverUrl) })}</strong>{/if}</p>
              {/snippet}
              {@render field("meaning_url", url)}
              {#snippet model()}
                <div class="folder">
                  <input id="in-meaning_model" list="mmodels" value={s.meaning_model} spellcheck="false" placeholder={server === "ollama" ? "bge-m3" : ""} onchange={(e) => setVectors(e.currentTarget, "meaning_model", e.currentTarget.value.trim())} />
                  <datalist id="mmodels">{#each models.list as m (m)}<option value={m}></option>{/each}</datalist>
                  {#if server === "ollama" && models.ok && !models.list.some((m) => m.split(":")[0] === wanted)}
                    <button disabled={!!meaning?.downloading} onclick={() => pull(wanted)}>{t("settings.meaning_pull", { model: wanted })}</button>
                  {/if}
                </div>
              {/snippet}
              {@render field("meaning_model", model)}
              {#if server === "openai"}{@render text("meaning_key_env", "OPENAI_API_KEY")}{/if}
            {/if}
            {#if vectorsChange}
              <p><strong>{vectorsChange.why}</strong></p>
              <div class="buttons">
                <button class="primary" onclick={() => { vectorsChange.go(); vectorsChange = null; }}>{t("settings.meaning_change_go")}</button>
                <button onclick={() => { vectorsChange.keep(); vectorsChange = null; }}>{t("settings.meaning_change_keep")}</button>
              </div>
            {/if}
            <!-- Only on a Mac, where the built-in model can run on the GPU. -->
            {#if !server && (index?.meaning_runs?.metal || index?.meaning_runs?.cpu_why)}
              {@render check("meaning_device", s.meaning_device === "cpu", (v) => set("meaning_device", v ? "cpu" : "auto"))}
            {/if}
          </details>

          <details class="group" id="opt-ask">
            <summary>{t("settings.group.ask")}</summary>
            {#snippet askModel()}
              <div class="folder">
                <input id="in-ask_model" list="askmodels" value={s.ask_model || askPreselected} spellcheck="false" placeholder="qwen3:8b" onchange={(e) => setAskModel(e.currentTarget.value.trim())} />
                <datalist id="askmodels">{#each chat?.models ?? [] as m (m.key)}<option value={m.key}>{m.name}</option>{/each}{#each chatModels as m (m)}<option value={m}></option>{/each}</datalist>
                {#if !s.ask_model && askPreselected}<button class="primary" disabled={!!chat?.downloading} onclick={useAskPreselected}>{t("settings.ask_use")}</button>{/if}
                <button disabled={!s.ask_model || !s.search_meaning || trial?.busy} onclick={tryAsk}>{t("settings.step.try_it")}</button>
              </div>
              {#if askProblem}<p class="err">{askProblem}</p>{/if}
              {#if trial && !trial.busy}<p class="hint" class:err={trial.error}>{trial.error ?? t("setup.try_done", { seconds: (trial.ms / 1000).toFixed(1) })}</p>{/if}
              {#if !builtinAsk && isRemote(serverUrl)}<p class="hint"><strong>{t("settings.ask_remote", { host: hostOf(serverUrl) })}</strong></p>{/if}
            {/snippet}
            {@render field("ask_model", askModel)}
            <div class="opt" id="opt-ask_builtin">
              <span class="field">{t("settings.ask_builtin")}</span>
              {#each chat?.models ?? [] as m (m.key)}
                <p>{t("settings.ask_builtin_model", { model: m.name, size: size(m.size), where: chat.runs })}{#if m.suggested}<span class="badge">{t("setup.recommended")}</span>{/if}</p>
                {#if m.estimate}<p class="hint">{m.estimate}</p>{/if}
                {#if chat.downloading?.[0] === m.key}
                  <p class="hint">{t("settings.meaning_downloading", { done: size(chat.downloading[1]), total: size(chat.downloading[2]) })}</p>
                  <progress max={chat.downloading[2] || 1} value={chat.downloading[1]}></progress>
                  <div class="buttons"><button onclick={() => chatAction("cancel", m.key)}>{t("common.cancel")}</button></div>
                {:else}
                  <div class="buttons">
                    {#if s.ask_model !== m.key}<button class:primary={m.suggested} disabled={!!chat.downloading} onclick={() => (m.installed ? setAskModel(m.key) : chatAction("download", m.key))}>{m.installed ? t("settings.ask_use") : t("settings.ask_download", { size: size(m.size) })}</button>{/if}
                    {#if m.installed}<button onclick={() => chatAction("remove", m.key)}>{t("settings.meaning_remove")}</button>{/if}
                  </div>
                {/if}
              {/each}
              {#if chat?.error}<p class="err">{chat.error}</p>{/if}
              <p class="hint">{t("settings.ask_builtin_hint")}</p>
            </div>
            {@render check("ask_think")}
            {@render num("ask_context", 2048, 131072)}
          </details>

          <details class="group">
            <summary>{t("settings.group.background")}</summary>
            <div class="opt">
              <label class="check"><input type="checkbox" checked={index?.service} disabled={!index} onchange={(e) => serviceSet(e.currentTarget.checked)} /> <span class="lab">{t("setup.service_on")}</span><span class="badge">{t("settings.cost.cpu")}</span></label>
              <p class="hint">{t("setup.service_hint")}</p>
            </div>
            <div class="buttons"><button disabled={!index?.shared || !index.pending} onclick={() => step({ step: "read_now" })}>{t("settings.step.read_now")}</button></div>
            {#if index?.path}<p class="hint"><span class="mono">{index.path}</span> <button class="link" onclick={() => showInPanel(index.path)}>{t("settings.show_in_panel")}</button></p>{/if}
            <div class="danger-row">
              <button disabled={!index?.shared} class="danger" onclick={forget} onblur={() => (forgetting = false)}>{forgetting ? t("settings.search_forget_confirm") : t("settings.forget", { size: size(index?.bytes ?? 0) })}</button>
              <p class="hint">{t("settings.forget_hint")}</p>
            </div>
          </details>
        </details>
      {:else if area === "previews"}
        <!-- ------------------------------------------------ Previews (asks the container runtime) -->
        <h3>{t("settings.area.previews")}</h3>
        {#if typeof images === "string"}<p class="hint err">{images}</p>{/if}
        {@render choice("preview_prefer", [["auto", t("settings.prefer_auto")], ["local", t("settings.prefer_local")], ["container", t("settings.prefer_container")]])}
        {@render choice("preview_container", [["auto", t("settings.container_auto")], ["podman", "podman"], ["docker", "docker"], ["off", t("settings.container_off")]])}
        {@render text("latex_image")}
        {@render check("latex_auto")}
        {@render num("preview_timeout", 10, 3600)}
        <div class="opt">
          <span class="lab">{t("settings.preview_cache")}</span>
          <div class="folder">
            <span class="hint">{previewCache == null ? "" : t("settings.preview_cache_size", { size: size(previewCache) })}</span>
            <button disabled={!previewCache} onclick={clearPreviewCache}>{t("settings.preview_cache_clear")}</button>
          </div>
        </div>
        {#if Array.isArray(images)}
          <div class="opt">
            <span class="lab">{t("settings.images")}</span>
            <div class="images">
              {#each images as im (im.image)}
                <div class="image">
                  <span class="mono" title={im.tool}>{im.image}</span>
                  <small class="hint">{imageStatus(im)}</small>
                  <button disabled={im.pulling != null} onclick={() => imageAction("pull_image", im)}>{t("settings.pull")}</button>
                  <button disabled={im.pulling != null || im.size == null} onclick={() => imageAction("remove_image", im)}>{t("common.remove")}</button>
                </div>
              {/each}
            </div>
          </div>
        {/if}
        <p class="hint">{t("settings.previews_more")}</p>
      {:else if area === "looks"}
        <!-- ------------------------------------------------ Looks -->
        <h3>{t("settings.area.looks")}</h3>
        <div class="opt" id="opt-language" class:flash={flash === "language"} title={keyOf("language")}>
          <span class="lab">{t("setting.language")}</span>
          <p class="lang current">
            {#if current}<img src={flag(current.flag)} alt="" />{/if}
            <span>{current?.name}{#if s.language === "auto"}<small>{t("settings.language_auto")}</small>{/if}</span>
          </p>
          <input class="short" type="search" bind:value={langFilter} placeholder={t("settings.language_filter")} aria-label={t("settings.language_filter")} />
          <div class="langs" role="radiogroup" aria-label={t("setting.language")}>
            <button class="lang" class:on={s.language === "auto"} role="radio" aria-checked={s.language === "auto"} onclick={() => set("language", "auto")}>
              <span class="auto">{"\u{f0ac}"}</span>
              <span>{t("settings.language_auto")}</span>
            </button>
            {#each langGroups as [group, langs] (group)}
              <p class="label region">{t(group)}</p>
              {#each langs as l (l.code)}
                <button class="lang" class:on={s.language === l.code} role="radio" aria-checked={s.language === l.code} lang={l.code} onclick={() => set("language", l.code)}>
                  <img src={flag(l.flag)} alt="" />
                  <span>{l.name}</span>
                  {#if l.new}<span class="new">{t("news.new")}</span>{/if}
                </button>
              {/each}
            {/each}
          </div>
          <p class="hint">{t("setting.language.hint")} <button class="link" onclick={() => invoke("open_path", { path: ui.cfg.improve_url })}>{t("settings.language_improve")}</button></p>
        </div>

        <div class="opt" id="opt-{themeFor}" class:flash={flash === "theme" || flash === "tui_theme"} title={keyOf(themeFor)}>
          <div class="for" role="radiogroup" aria-label={t("settings.theme_for")}>
            <span class="lab">{t("settings.theme_for")}</span>
            <label><input type="radio" name="themefor" checked={themeFor === "theme"} onchange={() => (themeFor = "theme")} /> {t("settings.theme_gui")}</label>
            <label><input type="radio" name="themefor" checked={themeFor === "tui_theme"} onchange={() => (themeFor = "tui_theme")} /> {t("settings.theme_tui")}</label>
          </div>
          <span class="lab">{t(`setting.${themeFor}`)}</span>
          <div class="themes" role="radiogroup" aria-label={t(`setting.${themeFor}`)}>
            {#each themes as id (id)}
              {@const th = ui.cfg.themes[id]}
              <button class="theme" class:on={s[themeFor] === id} role="radio" aria-checked={s[themeFor] === id} onclick={() => set(themeFor, id)}>
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
          <p class="hint">{t(`setting.${themeFor}.hint`)}</p>
        </div>
        {@render choice("glyphs", [["nerd", t("settings.glyphs_nerd")], ["ascii", t("settings.glyphs_ascii")]])}
        {@render text("font", "", ownFont)}
        {#if ownFont}<p class="hint note">{t("settings.font_own", { theme: themeName(s.theme) })}</p>{/if}
        {@render text("mono_font")}
        {@render text("icon_font")}
        {@render num("font_size", 9, 28)}
        {@render num("line_height", 1.2, 3, 0.1)}
      {:else if area === "behaviour"}
        <!-- ------------------------------------------------ Behaviour -->
        <h3>{t("settings.area.behaviour")}</h3>
        {@render check("show_hidden")}
        {@render check("confirm_delete")}
        {@render choice("right_click", [["mark", t("settings.right_click_mark")], ["menu", t("settings.right_click_menu")]])}
        {@render check("hints")}
        <div class="buttons sub"><button onclick={() => invoke("hints_reset").then(() => (hintsReset = true), (e) => (error = String(e)))}>{t("settings.hints_reset")}</button>{#if hintsReset} <span class="hint">{t("settings.hints_reset_done")}</span>{/if}</div>
        {@render check("folder_sizes")}
        {@render check("git_last_commit")}
        {@render text("editor", "$EDITOR")}
        {@render text("viewer", "$PAGER")}
        {@render check("bom_viewer")}
        {@render check("provenance_viewer")}
      {:else if area === "keys"}
        <!-- ------------------------------------------------ Keys -->
        <h3>{t("settings.area.keys")}</h3>
        <div class="folder">
          <input type="search" bind:value={keyFilter} placeholder={t("settings.keys_filter")} aria-label={t("settings.keys_filter")} />
          <button onclick={openKeys}>{t("settings.open_config")}</button>
        </div>
        <p class="hint">{t("settings.keys_hint")}</p>
        {#if ui.cfg.scripts_dir}
          <p class="hint">{t("settings.scripts_folder", { path: ui.cfg.scripts_dir })} <button class="link" onclick={openScripts}>{t("settings.open_folder")}</button></p>
        {/if}
        {#each keyGroups as [label, acts] (label)}
          <p class="label region">{label}</p>
          <div class="keys">
            {#each acts as a (a)}
              <span>{ui.cfg.actions[a]?.[0] ?? a}</span>
              <span class="mono">{keysOf(a).join(" · ") || "—"}</span>
            {/each}
          </div>
        {/each}
      {:else if area === "privacy"}
        <!-- ------------------------------------------------ Privacy and updates -->
        <h3>{t("settings.area.privacy")}</h3>
        {@render check("check_updates")}
        <div class="opt">
          <span class="lab">{t("settings.privacy_out")}</span>
          <div class="keys">
            {#each ui.cfg.outbound as o, i (i)}
              <span>{o.what}</span>
              <span class="mono">{o.local ? `${o.to} (${t("settings.privacy_local")})` : o.to}</span>
            {:else}
              <span class="hint">{t("settings.privacy_nothing")}</span>
            {/each}
          </div>
        </div>
        <div class="opt">
          <span class="lab">{t("settings.paths")}</span>
          <div class="keys">
            {#each ui.cfg.paths.filter(([, p]) => p) as [what, p] (what)}
              <span>{t(`settings.path.${what.replace(" ", "_")}`)}</span>
              <span><span class="mono">{p}</span> <button class="link" onclick={() => showInPanel(p, !["cache", "model", "previews", "archive looks"].includes(what))}>{t("settings.show_in_panel")}</button></span>
            {/each}
          </div>
        </div>
        <div class="buttons"><button onclick={openConfig}>{t("settings.open_config")}</button> <span class="hint">{t("settings.version", { version: ui.cfg.version })}</span></div>
      {/if}
    </div>
  </div>

  <footer>
    {#if error}<span class="err">{error}</span>{:else}<span class="hint">{saved ? t("settings.saved", { path: ui.cfg.config_path }) : t("settings.stored_in", { path: ui.cfg.config_path })}</span>{/if}
    <button class="primary" onclick={close}>{t("common.close")}</button>
  </footer>
</div>

<style>
  .settings {
    position: fixed;
    inset: 5vh 8vw;
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
    font-family: var(--icon-font), var(--font), var(--scripts);
  }
  h3 {
    margin: 0 0 4px;
    font-size: 0.95em;
    color: var(--hidden-fg);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .main {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(150px, 13em) minmax(0, 1fr);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 8px;
    border-inline-end: 1px solid var(--border-fg);
    overflow: auto;
  }
  nav .find {
    margin-bottom: 8px;
    min-width: 0;
  }
  .area {
    display: flex;
    align-items: center;
    border-color: transparent;
    text-align: start;
    padding: 6px 10px;
  }
  .area.on {
    border-color: var(--accent-bg);
    background: color-mix(in srgb, var(--accent-bg) 18%, transparent);
  }
  .area .new {
    margin-inline-start: auto;
  }
  .body {
    overflow: auto;
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
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
  button:disabled {
    opacity: 0.5;
    cursor: default;
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
  .opt {
    max-width: 48em;
    border-radius: var(--r);
    transition: background 0.4s;
  }
  .opt.sub {
    margin-inline-start: 24px;
  }
  .opt.flash {
    background: color-mix(in srgb, var(--accent-bg) 22%, transparent);
  }
  .opt > .hint,
  .opt .field + * + .hint {
    margin: 2px 0 0;
  }
  .lab {
    color: var(--dialog-fg);
  }
  .opt > .lab,
  .field {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }
  .badge {
    margin-inline: 3px;
    padding: 0 6px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-pill);
    font-size: 0.75em;
    color: var(--hidden-fg);
    white-space: nowrap;
  }
  .badge.out {
    border-color: var(--git-modified-fg, var(--accent-bg));
    color: var(--git-modified-fg, var(--accent-bg));
  }
  .status {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr) max-content;
    gap: 6px 14px;
    align-items: start;
    padding: 10px 12px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    max-width: 52em;
  }
  .status .link.strong {
    font-weight: bold;
    text-align: start;
  }
  .levels {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
    max-width: 48em;
  }
  legend {
    font-weight: bold;
    margin-bottom: 6px;
  }
  .level {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 6px 10px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    cursor: pointer;
  }
  .level.on {
    border-color: var(--accent-bg);
    background: color-mix(in srgb, var(--accent-bg) 18%, transparent);
  }
  .level .lab {
    margin-inline-end: 6px;
  }
  .level small {
    display: block;
  }
  details.details > summary {
    font-weight: bold;
  }
  details.group {
    margin: 8px 0 0 12px;
    display: grid;
    gap: 10px;
  }
  details.group[open] > summary {
    margin-bottom: 8px;
  }
  details.group > :global(*:not(summary)) {
    margin-bottom: 10px;
  }
  summary {
    cursor: pointer;
  }
  .result {
    display: grid;
    gap: 2px;
    text-align: start;
    border-color: transparent;
    max-width: 52em;
  }
  .result:hover {
    border-color: var(--border-fg);
  }
  .langs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 4px;
    margin-top: 6px;
  }
  .lang {
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: start;
    padding: 3px 8px;
  }
  .lang.current {
    margin: 0 0 6px;
    padding: 0;
  }
  .lang .new {
    margin-inline-start: auto;
  }
  .region {
    grid-column: 1 / -1;
    margin: 6px 0 0;
  }
  .lang.on {
    border-color: var(--accent-bg);
    background: color-mix(in srgb, var(--accent-bg) 18%, transparent);
  }
  .lang img {
    width: 20px;
    height: 15px;
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
    width: 20px;
    text-align: center;
    font-family: var(--icon-font);
  }
  p.label {
    margin: 0 0 6px;
    color: var(--hidden-fg);
  }
  .for {
    display: flex;
    gap: 14px;
    align-items: center;
    margin-bottom: 8px;
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 6px;
    margin-top: 4px;
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
    height: 40px;
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
  .keys {
    display: grid;
    grid-template-columns: minmax(10em, max-content) minmax(0, 1fr);
    gap: 4px 16px;
    max-width: 52em;
  }
  .keys > span {
    overflow-wrap: anywhere;
  }
  .images {
    display: grid;
    gap: 6px;
    max-width: 36em;
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
  .folders {
    display: grid;
    gap: 6px;
    max-width: 36em;
  }
  .folder {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;
    margin: 0;
    max-width: 36em;
  }
  .folder span {
    overflow-wrap: anywhere;
  }
  .folder.sub {
    margin-inline-start: 24px;
  }
  .folder small {
    display: block;
    margin: 2px 0 0;
  }
  .link {
    font: inherit;
    color: var(--accent-bg);
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0 0 0 6px;
    border: 1px solid var(--border-fg);
    border-radius: 4px;
  }
  .chip button {
    border: 0;
    background: none;
    padding: 0 6px;
    min-width: 0;
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
  .danger-row {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px dashed var(--border-fg);
  }
  .danger {
    border-color: var(--git-deleted-fg);
    color: var(--git-deleted-fg);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 4px 0;
  }
  .check {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    color: var(--dialog-fg);
  }
  input:not([type="checkbox"], [type="radio"]),
  select {
    font: inherit;
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 4px 8px;
    min-width: 0;
    width: 100%;
    max-width: 36em;
    box-sizing: border-box;
  }
  input.short {
    max-width: 12em;
  }
  input:disabled {
    opacity: 0.5;
  }
  .hint {
    color: var(--hidden-fg);
    font-size: 0.85em;
    margin: 4px 0 0;
    overflow-wrap: anywhere;
  }
  .note {
    margin-top: -6px;
  }
  .err {
    color: var(--git-deleted-fg);
  }
  .tip {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 6px;
  }
  .tip span {
    flex: 1;
  }
  .change {
    margin: 10px 0 0;
  }
  .change .label {
    margin: 0;
    color: inherit;
    font-weight: bold;
  }
  .what {
    margin: 2px 0 0;
    max-width: 100ch;
    line-height: 1.45;
  }
  .new {
    margin-inline-start: 6px;
    padding: 0 6px;
    border-radius: var(--r-pill);
    font-size: 0.8em;
    color: var(--accent-fg);
    background: var(--accent-bg);
  }
  .mono {
    font-family: var(--mono-font);
  }
</style>

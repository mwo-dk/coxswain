<script>
  // Settings, by task: a list of areas on the left, the area on the right. Every option is
  // described once in coxswain-core (coxswain_core::settings: its area, config key and costs;
  // its label and one-line explanation are `setting.<name>` texts), written to config.toml at
  // once (comments and layout kept) and applied without a restart.
  import { tick, untrack } from "svelte";
  import { ui, setPreviewTool, setTheme, themeIds, themeName, tab, showInTab, keysOf } from "./app.svelte.js";
  import { invoke, size, parent, without } from "./lib.js";
  import { t, setLanguage } from "./i18n.svelte.js";
  import CopyLine from "./CopyLine.svelte";
  import derafsh from "../../docs/flags/derafsh.svg?url";

  const flags = import.meta.glob("../node_modules/flag-icons/flags/4x3/{gb,au,ca,nz,dk,se,fi,ee,lv,lt,de,at,ch,be,fr,it,nl,ar,bo,cl,co,cr,cu,do,ec,gt,hn,mx,ni,pa,pe,pr,sv,uy,ve,es-ct,es-pv,il,pl,cz,ua,gr,tw,sg,jp,kr,am,ge,no}.svg", {
    query: "?url",
    import: "default",
    eager: true,
  });
  // Persian's banner is ours, kept with the docs' flags.
  const flag = (name) => (name === "derafsh" ? derafsh : flags[`../node_modules/flag-icons/flags/4x3/${name}.svg`]);

  const AREAS = ["overview", "search", "previews", "games", "looks", "behaviour", "keys", "privacy"];
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
  /** The git tools installed here, looked for when Behaviour opens. */
  let gitTools = $state([]);
  // Open a terminal here: what opens unset, the terminals installed, and whether a command of
  // one's own is being typed (Another command…).
  let terms = $state([null, []]);
  let termOther = $state(false);
  const termListed = $derived(!termOther && (!s.terminal || terms[1].some(([n]) => n === s.terminal)));
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
      // A refused value does not stay in its control: the stored one is shown again, in a
      // field, a switch or a choice alike. A list shows the stored items already (its field
      // was emptied when the item was added).
      for (const name of Object.keys(changes)) {
        const el = document.getElementById(`in-${name}`) ?? document.querySelector(`#opt-${name} input[type=checkbox]`);
        if (!el || Array.isArray(s[name])) continue;
        if (el.type === "checkbox") el.checked = !!s[name];
        else if (el.tagName === "SELECT") el.value = s[name] ?? "";
        else el.value = el.type === "number" ? s[name] / (Number(el.dataset.scale) || 1) : (s[name] ?? "");
      }
    }
  }
  // What is kept of long files: what the change reads again, adds or frees, said first.
  let longChange = $state(null);
  // Which ask is the last: an answer to an older one, or to one cancelled, is dropped.
  let asks = 0;
  async function set(name, value) {
    const changes = { [name]: value };
    if (!name.startsWith("long_files") && !commitChanges(name)) return save(changes);
    // A newer ask on another option: the one waiting goes back to its stored value.
    if (longChange && longChange.name !== name) keepLong();
    const ask = ++asks;
    // Counting the changes of commits takes seconds with many large repositories: said meanwhile.
    if (commitChanges(name)) longChange = { why: t("changes.costing"), name, busy: true };
    const why = await invoke("long_files_change", { changes }).catch(() => null);
    if (ask !== asks) return;
    if (!why) return void ((longChange = null), save(changes));
    longChange = { why, name, go: () => save(changes) };
  }
  // Git history on or off keeps or drops the changes of commits too.
  const commitChanges = (name) => name === "search_history" || name.startsWith("search_commit_changes");
  function keepLong() {
    asks++;
    const name = longChange?.name;
    longChange = null;
    const el = document.getElementById("in-long_files");
    if (el) el.value = s.long_files;
    const box = document.querySelector(`#opt-${name} input[type=checkbox]`);
    if (box) box.checked = !!s[name];
  }

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
    // One block shows both themes: the one asked for.
    if (at === "theme" || at === "tui_theme") themeFor = at;
    await tick();
    // Disk use's rows come when they are read: the user's own files are shown once they are.
    // Both reads: the second (with the container images) adds rows above Your own files.
    if (at === "disk_yours") await Promise.all([diskRead, diskFull]);
    await tick();
    // With none of the user's own files kept, Disk use itself.
    const el = document.getElementById(`opt-${at}`) ?? (at === "disk_yours" ? document.getElementById("opt-disk") : null);
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
    if (area === "previews") untrack(() => (loadTools(), loadPreviewCache()));
    if (area === "games") untrack(loadEngines);
    if (area === "privacy") untrack(() => (loadDisk(), loadServers()));
    if (area === "behaviour") invoke("git_tools").then((v) => (gitTools = v), () => {});
    if (area === "behaviour") invoke("terminals").then((v) => (terms = v), () => {});
  });

  /** "Find a setting": every option whose label, explanation or config key holds the words,
   *  case and accents aside, with its area and group (coxswain_core::settings::search, as in
   *  the terminal app). Enter in the field goes to the first. */
  let found = $state([]);
  $effect(() => {
    const q = query;
    if (!q.trim()) return void (found = []);
    invoke("settings_find", { query: q }).then((v) => query === q && (found = v), () => {});
  });
  function findKey(e) {
    if (e.key === "Escape" && query) (query = ""), e.stopPropagation();
    else if (e.key === "Enter" && found[0]) e.preventDefault(), go(found[0].name);
    else if (e.key === "ArrowDown" && found[0]) e.preventDefault(), document.querySelector(".settings .result")?.focus();
  }

  // ------------------------------------------------------------ search: status and level

  let index = $state(null);
  let lines = $state([]);
  const loadIndex = () => {
    invoke("index_status").then((v) => (index = v), () => (index = null));
    invoke("search_status").then((v) => (lines = v), () => {});
  };
  loadIndex();
  // Polled only while an area that shows them is open (a search lists every area's options),
  // and read at once on arriving there.
  const shows = (...areas) => !!query || areas.includes(area);
  $effect(() => {
    if (!shows("search", "overview")) return;
    untrack(loadIndex);
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
  // The speed test of search by meaning: its report, or why not.
  let bench = $state(null);
  async function runBench() {
    bench = { busy: true };
    try {
      bench = { text: await invoke("meaning_bench") };
    } catch (e) {
      bench = { error: String(e) };
    }
  }
  async function step(line) {
    error = "";
    if (line.step === "turn_on") return chooseLevel("text");
    if (line.step === "set_up") return openSetup();
    if (line.step === "try_it") return tryAsk();
    if (line.step === "bench") return runBench();
    const what = { read_now: "now", pause: "pause", resume: "resume", stop_transcribing: "stop_transcribing" }[line.step] ?? "restart";
    await invoke("index_action", { what }).catch((e) => (error = String(e)));
    loadIndex();
  }

  // ------------------------------------------------------------ search: details

  /** Settings closes and the active panel shows `path`: a folder opened, a file with the cursor on it. */
  async function showInPanel(path, isFile = true) {
    close();
    const tb = tab();
    await showInTab(tb, path, !isFile);
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
    const dir = await invoke("folder_to_add", { panel: tab()?.dir ?? ui.cfg.home, input: input ?? "" });
    adding[name] = "";
    if (dir && !s[name].includes(dir)) set(name, [...s[name], dir]);
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
    if (!shows("search")) return;
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
  // Ask off: a model is offered, a server's on the graphics card first, else the built-in one
  // for this machine; Use sets it (downloading a built-in one first).
  const askPreselected = $derived(chat?.preselected ?? "");
  // The list of chat models: a pick sets Ask's model in one step; "Another model…" types one.
  const OTHER = "\u0001other";
  let askOther = $state(false);
  const askListed = $derived(!askOther && (!s.ask_model || (chat?.groups ?? []).some((g) => g.models.some((m) => m.value === s.ask_model))));
  const askName = (value) => (chat?.models ?? []).find((m) => m.key === value)?.name ?? value;
  function pickAsk(value) {
    askOther = value === OTHER;
    if (askOther) return;
    const m = chat?.models.find((x) => x.key === value);
    if (m && !m.installed) chatAction("download", m.key);
    else setAskModel(value);
  }
  // A model chosen that is slow here, with the better one: said on its line until it changes.
  const askPoor = $derived(lines.find((l) => l.part === "ask")?.poor ?? null);
  const meaningPoor = $derived(lines.find((l) => l.part === "meaning")?.poor ?? null);
  async function usePoor(p) {
    if (p.ask) return "ask_model" in p.changes ? setAskModel(p.changes.ask_model) : save(p.changes);
    const c = p.changes;
    const why = await invoke("meaning_change", { name: "meaning_model", value: c.meaning_model, engine: c.meaning_engine, url: c.meaning_url }).catch(() => null);
    if (!why) return save(c);
    vectorsChange = { why, go: () => save(c), keep: () => {} };
  }
  // The built-in models on the disk: loaded when their group opens, and after each action.
  let models_ = $state(null);
  let modelsSaid = $state("");
  let deleting = $state("");
  const loadModels = () => invoke("models_list").then((v) => (models_ = v), (e) => (modelsSaid = String(e)));
  const unusedBytes = $derived((models_ ?? []).filter((e) => !e.in_use).reduce((n, e) => n + e.bytes, 0));
  async function modelAction(what, e) {
    // Deleting the one in use takes a second click.
    if (what === "delete" && e.in_use && deleting !== e.id) return (deleting = e.id);
    deleting = "";
    modelsSaid = await invoke("models_action", { what, id: e.id }).catch((err) => String(err));
    loadModels();
    loadChat();
    loadIndex();
  }
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
    if (!shows("search")) return;
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

  // Preview tools: per kind of file what is installed, its images with Pull (also updates) and
  // Remove, and the choice. Asked only while Previews is shown, as it asks every program and the
  // container runtime.
  let tools = $state(null);
  // The Whisper models to choose from, marked by what suits this machine: asked once.
  let whisperModels = $state(null);
  $effect(() => {
    if (area === "search" && s.search_whisper && !whisperModels) untrack(() => invoke("whisper_models").then((v) => (whisperModels = v), () => {}));
  });
  const loadTools = () => invoke("preview_tools").then((v) => (tools = v), (e) => (tools = String(e)));
  async function imageAction(cmd, image) {
    error = "";
    tools.pulling[image] = "";
    await invoke(cmd, { image }).catch((e) => (error = String(e)));
    loadTools();
  }
  $effect(() => {
    if (area !== "previews" || !tools?.pulling || !Object.keys(tools.pulling).length) return;
    const id = setInterval(loadTools, 1000);
    return () => clearInterval(id);
  });
  async function setTool(r, choice) {
    error = "";
    await setPreviewTool(r, choice).catch((e) => (error = String(e)));
    loadTools();
  }
  /** Try: a sample of the kind, shown in the panel with the preview open. */
  async function tryTool(r) {
    const path = await invoke("preview_sample", { kind: r.kind }).catch((e) => void (error = String(e)));
    if (!path) return;
    ui.showPreview = true;
    showInPanel(path);
  }
  const limits = $derived([["time", t("settings.chess_limit_time")], ["depth", t("settings.chess_limit_depth")], ["nodes", t("settings.chess_limit_nodes")]]);
  const preferLabel = (v) => t({ auto: "settings.prefer_auto", local: "settings.prefer_local", container: "settings.prefer_container" }[v] ?? "settings.prefer_auto");
  const installed = (r) => r.programs.filter((p) => p.path);
  const imageState = (im) => (tools.pulling[im.image] != null ? tools.pulling[im.image] || t("common.loading") : im.pulled != null ? t("settings.image_pulled", { size: size(im.pulled) }) : im.download ? `${t("settings.image_not_pulled")} (${im.download})` : t("settings.image_not_pulled"));
  // ------------------------------------------------------------ the chess engine
  /** The known engines (only looked for, never run), and what Test engine read: the chosen
   *  engine's name and options, "testing" while it runs, or why it could not. */
  let engines = $state([]);
  let engine = $state(null);
  const loadEngines = () => invoke("chess_engines").then((v) => (engines = v), () => (engines = []));
  async function chooseEngine(program) {
    engine = null;
    await set("chess_engine", program === "stockfish" ? "" : program);
    loadEngines();
  }
  async function testEngine() {
    engine = "testing";
    engine = await invoke("chess_engine_test").catch((e) => ({ error: String(e) }));
  }
  /** An option of the tested engine set (null: back to its default), or all of them reset. */
  async function setEngineOption(name, value, reset = false, el = null) {
    error = "";
    try {
      engine = await invoke("chess_engine_set", { name, value, reset });
      saved = true;
    } catch (e) {
      error = String(e);
      // A refused value does not stay in its field: the stored one (or the default) is shown again.
      const o = engine?.options?.find((o) => o.name === name);
      if (el && o) el.type === "checkbox" ? (el.checked = optionValue(o)) : (el.value = optionValue(o));
    }
  }
  async function engineConfig() {
    await invoke("chess_engine_config", { key: engine?.key ?? "Stockfish" }).catch(() => {});
    openConfig();
  }
  const optionValue = (o) => (o.value ?? (o.kind === "spin" ? Number(o.default) : o.kind === "check" ? o.default === "true" : o.default));
  const optionAbout = (o) =>
    `${o.kind === "spin" ? t("chess.option_spin", { min: o.min ?? 0, max: o.max ?? 0 }) : o.kind === "check" ? t("chess.option_check") : o.kind === "combo" ? t("chess.option_combo", { values: o.vars.join(", ") }) : t("chess.option_string")} · ${t("chess.engine_default", { value: o.default === "" ? "—" : o.default })}`;

  let previewCache = $state(null);
  const loadPreviewCache = () => invoke("preview_cache").then((v) => (previewCache = v), () => (previewCache = null));
  async function clearPreviewCache() {
    await invoke("clear_preview_cache").catch((e) => (error = String(e)));
    loadPreviewCache();
  }

  // ------------------------------------------------------------ privacy: history servers

  /** The Subversion and CVS servers said yes to, whose history may be read. */
  let servers = $state([]);
  const loadServers = () => invoke("get_state").then((st) => (servers = st.history_servers ?? []), () => {});
  const forgetServer = (server) => invoke("history_forget", { server }).then(loadServers, (e) => (ui.status = String(e)));

  // ------------------------------------------------------------ privacy: disk use

  // Every place Coxswain keeps things: at once, then again with the container images, which
  // asks podman or docker.
  let disk = $state(null);
  let diskSaid = $state("");
  let clearing = $state("");
  let also = $state({ models: false, outside: false });
  let diskRead = Promise.resolve();
  let diskFull = Promise.resolve();
  const loadDisk = () => {
    diskRead = invoke("disk_list", { images: false }).then((v) => (disk ??= v), (e) => (diskSaid = String(e)));
    diskFull = invoke("disk_list", { images: true }).then((v) => (disk = v), () => {});
  };
  const diskOf = (kind) => (disk ?? []).filter((d) => (kind === "own" ? d.kind === "own" || d.kind === "model" : d.kind === kind));
  /** What *Clear everything that can be built again* clears, with what is ticked. */
  const going = $derived((disk ?? []).filter((d) => (d.kind === "own" ? d.rebuildable : d.kind === "model" ? also.models && !d.in_use : d.kind === "outside" && also.outside) && d.bytes));
  const bytesOf = (list) => list.reduce((n, d) => n + d.bytes, 0);
  const unusedModels = $derived(bytesOf((disk ?? []).filter((d) => d.kind === "model" && !d.in_use)));
  const outsideBytes = $derived(bytesOf(diskOf("outside")));
  /** Clear a row, or with none everything that goes: each takes a second click. */
  async function clearDisk(d) {
    const id = d?.id ?? "";
    if (clearing !== (id || "*")) return (clearing = id || "*");
    clearing = "";
    diskSaid = await (d?.kind === "model" ? invoke("models_action", { what: "delete", id }) : invoke("disk_clear", { id, models: also.models, outside: also.outside })).catch((e) => String(e));
    disk = null;
    loadDisk();
    loadIndex();
  }

  // ------------------------------------------------------------ looks

  const current = $derived(ui.cfg.languages.find((l) => l.code === ui.cfg.language));
  let langFilter = $state("");
  /** Items in runs of the same key, in order: [key, items]. */
  const groupRuns = (items, keyOf) =>
    items.reduce((g, x) => {
      const key = keyOf(x);
      if (g.at(-1)?.[0] !== key) g.push([key, []]);
      g.at(-1)[1].push(x);
      return g;
    }, []);
  /** The languages under their regions, those the filter leaves: [group key, languages]. */
  const langGroups = $derived(
    groupRuns(
      ui.cfg.languages.filter((l) => !langFilter.trim() || `${l.name} ${l.code}`.toLowerCase().includes(langFilter.trim().toLowerCase())),
      (l) => l.group,
    ),
  );
  /** The themes under their groups, your own last: [group key, theme ids]. */
  const themeGroup = $derived(Object.fromEntries(ui.cfg.builtin_themes));
  const themeGroups = $derived(groupRuns(themeIds(), (id) => themeGroup[id] ?? "theme.group.own"));
  let themeFor = $state("theme");
  /** The theme brings its own font (every look but the modern one): the Font field does nothing. */
  const ownFont = $derived(ui.cfg.looks[s.theme] !== "modern");

  // ------------------------------------------------------------ keys

  let keyFilter = $state("");
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
  {#snippet control()}<input id="in-{name}" class="short" type="number" {min} {max} step={stepBy} data-scale={scale} value={s[name] / scale} onchange={(e) => number(e, name, scale)} />{/snippet}
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
          <!-- Unkeyed: config.toml may name an entry twice, and a repeated key stops the area showing. -->
          {#each s[name] as w, i}
            <span class="chip mono">{w}<button title={t("common.remove")} aria-label={t("common.remove")} onclick={() => set(name, without(s[name], i))}>×</button></span>
          {:else}
            <span class="hint">{none}</span>
          {/each}
        </div>
      {:else}
        {#each s[name] as dir, i}
          <div class="folder">
            <span class="mono">{dir}{#if name === "text_roots"}<small class="hint">{rootInfo(dir)}</small>{/if}</span>
            <button onclick={() => set(name, without(s[name], i))}>{t("common.remove")}</button>
          </div>
        {:else}
          <span class="hint">{none}{#if name === "text_roots" && index?.roots?.[0]}{" · "}{rootInfo(index.roots[0][0])}{/if}</span>
        {/each}
      {/if}
      <form class="folder" onsubmit={(e) => (e.preventDefault(), words ? addWord(name) : addFolder(name, adding[name]))}>
        <input id="in-{name}" bind:value={adding[name]} spellcheck="false" placeholder={words ? "*.log" : "~/Documents"} />
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
      <input class="find" type="search" bind:value={query} placeholder={t("settings.find")} aria-label={t("settings.find")} onkeydown={findKey} />
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
            <span><strong>{t(`setting.${o.name}`)}</strong> <small class="hint">{t(`settings.area.${o.area}`)}{#if o.group} › {o.group}{/if} · <span class="mono">{keyOf(o.name)}</span></small></span>
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
                {#if n.config_line}<button onclick={() => invoke("open_config_at", { line: n.config_line }).catch((e) => (error = String(e)))}>{t("config.open_at", { line: n.config_line })}</button>{/if}
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
              {#if l.bar}<br /><progress max={l.bar[1] || 1} value={l.bar[0]}></progress> <small class="hint">{l.bar[1] ? Math.min(99, Math.floor((l.bar[0] * 100) / l.bar[1])) : 100} %</small>{/if}
              {#if l.note}<br /><small class="hint" class:err={l.bad}>{l.note}</small>{/if}
              {#if l.part === "meaning" && index?.meaning_runs_text && s.meaning_engine === "builtin"}<br /><small class="hint">{index.meaning_runs_text}</small>{/if}
              {#if l.poor}<br /><small class={l.poor.tip ? "hint" : "err"}>{l.poor.text}</small>{#if l.poor.button}{" "}<button class="link" onclick={() => usePoor(l.poor)}>{l.poor.button}</button>{/if}{/if}
              {#if l.part === "ask" && trial}<br /><small class="hint" class:err={trial.error}>{trial.busy ? t("common.loading") : trial.error ?? t("setup.try_done", { seconds: (trial.ms / 1000).toFixed(1) })}</small>{/if}
              {#if l.part === "meaning" && bench}<br /><small class="hint" class:err={bench.error}>{bench.busy ? t("common.loading") : bench.error ?? bench.text}</small>{/if}
            </span>
            {#if l.step}<span class="steps"
                ><button disabled={(trial?.busy && l.step === "try_it") || (bench?.busy && l.step === "bench")} onclick={() => step(l)}>{t(`settings.step.${l.step}`)}</button
                >{#if l.also}<button onclick={() => step({ step: l.also })}>{t(`settings.step.${l.also}`)}</button>{/if}</span
              >{:else}<span></span>{/if}
          {/each}
        </div>

        <fieldset class="levels">
          <legend>{t("settings.level")}</legend>
          <!-- The grid inside, not on the fieldset: an older WebKitGTK (FreeBSD 15.1's) sizes a grid fieldset's legend to its narrowest, a word a line. -->
          <div class="level-list">
            {#each ui.cfg.levels as [l, costs] (l)}
              <label class="level" class:on={ui.cfg.level === l}>
                <input type="radio" name="level" checked={ui.cfg.level === l} onchange={() => chooseLevel(l)} />
                <span><span class="lab">{t(`settings.level.${l}`)}</span>{#each costs as c (c)}<span class="badge" class:out={c === "leaves"}>{t(`settings.cost.${c}`)}</span>{/each}
                  <small class="hint">{t(`settings.level.${l}.hint`)}</small></span>
              </label>
            {/each}
            {#if !ui.cfg.level}<p class="hint err">{t("settings.level.custom")}</p>{/if}
          </div>
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
            {#if s.search_history}
              {@render check("search_commit_changes", undefined, undefined, true)}
              {#if !s.search_commit_changes}{@render list("search_commit_changes_in", t("settings.search_names_only_none"))}{/if}
              {#if longChange && commitChanges(longChange.name)}
                <!-- The room and time it takes, or what turning it off frees, said before it is saved. -->
                <p><strong>{longChange.why}</strong></p>
                <div class="buttons">
                  {#if !longChange.busy}<button class="primary" onclick={() => { longChange.go(); longChange = null; }}>{t("common.apply")}</button>{/if}
                  <button onclick={keepLong}>{t("common.cancel")}</button>
                </div>
              {/if}
            {/if}
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
            {@render check("search_tika")}
            {@render check("search_whisper")}
            {#if s.search_whisper && whisperModels}{@render choice("whisper_model", whisperModels)}{/if}
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
                <p class="hint">{models.ok ? t("settings.meaning_server_ok") : models.error}{#if !ui.cfg.server_local}<br /><strong>{t("settings.meaning_remote", { host: ui.cfg.server_host })}</strong>{/if}</p>
              {/snippet}
              {@render field("meaning_url", url)}
              {#snippet model()}
                <div class="folder">
                  <input id="in-meaning_model" list="mmodels" value={s.meaning_model} spellcheck="false" placeholder={server === "ollama" ? "bge-m3" : ""} onchange={(e) => setVectors(e.currentTarget, "meaning_model", e.currentTarget.value.trim())} />
                  <datalist id="mmodels">{#each models.list as m}<option value={m}></option>{/each}</datalist>
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
            {#if meaningPoor}
              <p class={meaningPoor.tip ? "hint" : "err"}>{meaningPoor.text}</p>
              <div class="buttons"><button class="primary" onclick={() => usePoor(meaningPoor)}>{meaningPoor.button}</button></div>
            {/if}
            <!-- Only on a Mac, where the built-in model can run on the GPU. -->
            {#if !server && (index?.meaning_runs?.metal || index?.meaning_runs?.cpu_why)}
              {@render check("meaning_device", s.meaning_device === "cpu", (v) => set("meaning_device", v ? "cpu" : "auto"))}
            {/if}
            <!-- Sample or Everything, for every folder and for some; a change says its cost first. -->
            {@render choice("long_files", [["sample", t("settings.long_files.sample")], ["everything", t("settings.long_files.everything")]])}
            {@render list("long_files_everything", t("settings.search_names_only_none"))}
            {@render list("long_files_sample", t("settings.search_names_only_none"))}
            {#if longChange && !commitChanges(longChange.name)}
              <p><strong>{longChange.why}</strong></p>
              <div class="buttons">
                <button class="primary" onclick={() => { longChange.go(); longChange = null; }}>{t("common.apply")}</button>
                <button onclick={keepLong}>{t("common.cancel")}</button>
              </div>
            {/if}
          </details>

          <details class="group" id="opt-ask">
            <summary>{t("settings.group.ask")}</summary>
            {#snippet askModel()}
              <!-- One list: the server's chat models and the built-in ones, under where each runs. -->
              <div class="folder">
                <select id="in-ask_model" value={askListed ? s.ask_model : OTHER} onchange={(e) => pickAsk(e.currentTarget.value)}>
                  {#each chat?.groups ?? [] as g (g.label)}
                    <optgroup label={g.label}>
                      {#if g.missing}<option disabled>{g.missing}</option>{/if}
                      {#each g.models as m (m.value)}<option value={m.value}>{m.label}{m.recommended ? ` · ${t("setup.recommended")}` : ""}{m.slow ? ` · ${t("settings.ask_slow_here")}` : ""}</option>{/each}
                    </optgroup>
                  {/each}
                  <option value={OTHER}>{t("settings.ask_other")}</option>
                  <option value="">{t("settings.ask_off")}</option>
                </select>
                <button disabled={!s.ask_model || !s.search_meaning || trial?.busy} onclick={tryAsk}>{t("settings.step.try_it")}</button>
              </div>
              {#if askOther || !askListed}<input id="in-ask_other" value={s.ask_model} spellcheck="false" placeholder="qwen3:8b" onchange={(e) => setAskModel(e.currentTarget.value.trim())} />{/if}
              {#if !s.ask_model && askPreselected}<div class="buttons"><button class="primary" disabled={!!chat?.downloading} onclick={useAskPreselected}>{t("settings.use_model", { model: askName(askPreselected) })}</button></div>{/if}
              {#if chat?.groups?.some((g) => g.server && g.missing && !g.loading)}<div class="buttons"><button onclick={openSetup}>{t("settings.ask_set_up_server")}</button></div>{/if}
              {#if askPoor}
                <p class={askPoor.tip ? "hint" : "err"}>{askPoor.text}</p>
                <div class="buttons">{#if askPoor.button}<button class="primary" onclick={() => usePoor(askPoor)}>{askPoor.button}</button>{:else}<button onclick={openSetup}>{t("setup.open")}</button>{/if}</div>
              {/if}
              {#if chat?.downloading}
                <p class="hint">{t("settings.meaning_downloading", { done: size(chat.downloading[1]), total: size(chat.downloading[2]) })}</p>
                <progress max={chat.downloading[2] || 1} value={chat.downloading[1]}></progress>
                <div class="buttons"><button onclick={() => chatAction("cancel", chat.downloading[0])}>{t("common.cancel")}</button></div>
              {/if}
              {#if chat?.error}<p class="err">{chat.error}</p>{/if}
              {#if askProblem}<p class="err">{askProblem}</p>{/if}
              {#if trial && !trial.busy}<p class="hint" class:err={trial.error}>{trial.error ?? t("setup.try_done", { seconds: (trial.ms / 1000).toFixed(1) })}</p>{/if}
              {#if !builtinAsk && !ui.cfg.server_local}<p class="hint"><strong>{t("settings.ask_remote", { host: ui.cfg.server_host })}</strong></p>{/if}
              <p class="hint">{t("settings.ask_builtin_hint")}</p>
            {/snippet}
            {#snippet codeModel()}
              <!-- Same as Ask, or a model of the same list for questions about code. -->
              <select id="in-ask_code_model" value={s.ask_code_model} onchange={(e) => set("ask_code_model", e.currentTarget.value)}>
                <option value="">{t("settings.code_same")}</option>
                {#each chat?.code_groups ?? [] as g (g.label)}
                  {#if g.models.length}
                    <optgroup label={g.label}>
                      {#each g.models as m (m.value)}<option value={m.value}>{m.label}{m.recommended ? ` · ${t("setup.recommended")}` : ""}{m.slow ? ` · ${t("settings.ask_slow_here")}` : ""}</option>{/each}
                    </optgroup>
                  {/if}
                {/each}
                {#if s.ask_code_model && !(chat?.code_groups ?? []).some((g) => g.models.some((m) => m.value === s.ask_code_model))}<option value={s.ask_code_model}>{s.ask_code_model}</option>{/if}
              </select>
              {#if chat?.code_why}<p class="hint">{chat.code_why}</p>{/if}
            {/snippet}
            {@render field("ask_model", askModel)}
            {@render field("ask_code_model", codeModel)}
            {@render check("ask_think")}
            {@render num("ask_context", 2048, 131072)}
            {#if chat?.context.why}
              <p class="hint">{chat.context.why} {#if chat.context.suggest}<button onclick={() => set("ask_context", chat.context.suggest)}>{t("setup.context_use", { n: chat.context.suggest })}</button>{/if}</p>
            {/if}
          </details>

          <details class="group" id="opt-models" ontoggle={(e) => e.currentTarget.open && loadModels()}>
            <summary>{t("models.title")}</summary>
            <p class="hint">{t("models.hint")}</p>
            {#each models_ ?? [] as e (e.id)}
              <div class="opt">
                <p><strong>{e.name}</strong>{#if e.in_use}<span class="badge">{t("models.in_use")}</span>{/if}</p>
                <p class="hint">{e.summary}</p>
                <p class="hint"><span class="mono">{e.folder}</span> <button class="link" onclick={() => showInPanel(e.folder, false)}>{t("settings.show_in_panel")}</button></p>
                <div class="buttons">
                  {#if e.loaded && e.what === "ask"}<button onclick={() => modelAction("unload", e)}>{t("models.unload")}</button>{/if}
                  {#if e.loaded && e.what === "meaning"}<span class="hint">{t("models.stays_loaded")}</span>{/if}
                  <button class="danger" onclick={() => modelAction("delete", e)} onblur={() => (deleting = "")}>{deleting === e.id ? t("models.confirm_again", { model: e.name }) : t("common.delete")}</button>
                </div>
              </div>
            {:else}
              <p class="hint">{t("models.none")}</p>
            {/each}
            {#if modelsSaid}<p class="hint">{modelsSaid}</p>{/if}
            {#if unusedBytes}<div class="buttons"><button class="danger" onclick={() => modelAction("unused", { id: "" })}>{t("models.delete_unused", { size: size(unusedBytes) })}</button></div>{/if}
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
        <details class="group" id="opt-previews_general" open>
          <summary>{t("settings.group.general")}</summary>
          {@render choice("preview_prefer", [["auto", t("settings.prefer_auto")], ["local", t("settings.prefer_local")], ["container", t("settings.prefer_container")]])}
          {@render num("preview_timeout", 10, 3600)}
          <div class="opt">
            <span class="lab">{t("settings.preview_cache")}</span>
            <div class="folder">
              <span class="hint">{previewCache == null ? "" : t("settings.preview_cache_size", { size: size(previewCache) })}</span>
              <button disabled={!previewCache} onclick={clearPreviewCache}>{t("settings.preview_cache_clear")}</button>
            </div>
          </div>
        </details>
        <details class="group" id="opt-documents" open>
          <summary>{t("settings.group.documents")}</summary>
          {@render check("latex_auto")}
          {@render check("ebooks")}
          {@render check("drawio")}
        </details>
        <details class="group" id="opt-images" open>
          <summary>{t("settings.group.images")}</summary>
          {@render choice("preview_container", [["auto", t("settings.container_auto")], ["podman", "podman"], ["docker", "docker"], ["off", t("settings.container_off")]])}
          {@render text("latex_image")}
          {@render choice("image_updates", [["off", t("settings.image_updates_off")], ["weekly", t("settings.image_updates_weekly")], ["monthly", t("settings.image_updates_monthly")]])}
          {@render check("image_prune")}
          <p class="hint">{t("settings.previews_more")}</p>
        </details>
        <details class="group" id="opt-safety" open>
          <summary>{t("settings.group.safety")}</summary>
          {@render check("virus_scan")}
        </details>
        <details class="group" open>
          <summary>{t("ptools.title")}</summary>
          <div class="opt" id="opt-preview_tools" class:flash={flash === "preview_tools"}>
            <div class="ptools">
              <p class="hint">{t("ptools.hint")}</p>
              {#if typeof tools === "string"}<p class="hint err">{tools}</p>{:else if !tools}<p class="hint">{t("common.loading")}</p>{/if}
              {#if tools?.no_runtime}<p class="hint">{tools.no_runtime}</p>{/if}
              {#each tools?.rows ?? [] as r (r.kind)}
                <div class="ptool" id="ptool-{r.kind}" class:flash={flash === `ptool-${r.kind}`}>
                  <div class="ptool-head">
                    <b>{r.title}</b>
                    <small class="hint mono exts" title={r.exts}>{r.exts}</small>
                  </div>
                  <small>{installed(r).length ? t("ptools.installed", { programs: installed(r).map((p) => (p.version ? `${p.name} ${p.version}` : p.name)).join(", ") }) : t("ptools.installed_none")}</small>
                  {#if !installed(r).length && r.install}<small>{t("install.with")} <CopyLine line={r.install} /></small>{/if}
                  {#each r.images as im (im.key)}
                    <div class="ptool-image" title={im.about ?? ""}>
                      <small>{im.image ? t("ptools.container", { image: im.image.split("@")[0], state: imageState(im) }) : t("ptools.container_none", { key: im.key })}</small>
                      {#if !im.image}<button onclick={openConfig}>{t("settings.open_config")}</button>{/if}
                      {#if im.image && !tools.no_runtime}
                        <button disabled={tools.pulling[im.image] != null} onclick={() => imageAction("pull_image", im.image)}>{t("settings.pull")}</button>
                        <button disabled={tools.pulling[im.image] != null || im.pulled == null} onclick={() => imageAction("remove_image", im.image)}>{t("common.remove")}</button>
                      {/if}
                    </div>
                  {/each}
                  <div class="ptool-use">
                    <small class:err={r.missing}>{t("ptools.uses", { engine: r.uses })}</small>
                    {#if r.can_choose}<select aria-label={t("ptools.choice")} value={r.choice} onchange={(e) => setTool(r, e.currentTarget.value)}>
                      <option value="">{t("ptools.choice.default", { value: preferLabel(s.preview_prefer) })}</option>
                      <option value="local">{t("ptools.choice.local")}</option>
                      <option value="container" disabled={!r.container}>{t(r.container ? "ptools.choice.container" : "ptools.choice.container_no_image")}</option>
                      <option value="off">{t("ptools.choice.off")}</option>
                    </select>{/if}
                    {#if r.sample}<button title={t("ptools.try_hint")} onclick={() => tryTool(r)}>{t("ptools.try")}</button>{/if}
                  </div>
                </div>
              {/each}
            </div>
          </div>
        </details>
      {:else if area === "games"}
        <!-- ------------------------------------------------ Games: chess, and the boards of Go, draughts, shogi and xiangqi -->
        <h3>{t("settings.area.games")}</h3>
        <details class="group" id="opt-board" open>
          <summary>{t("settings.group.board")}</summary>
          {@render choice("chess_board", [["theme", t("settings.chess_board_theme")], ["green", t("settings.chess_board_green")], ["brown", t("settings.chess_board_brown")]])}
          {@render choice("chess_pieces", [["auto", t("settings.chess_pieces_auto")], ["pictures", t("settings.chess_pieces_pictures")], ["octants", t("settings.chess_pieces_octants")], ["sextants", t("settings.chess_pieces_sextants")], ["quadrants", t("settings.chess_pieces_quadrants")], ["halves", t("settings.chess_pieces_halves")], ["figurines", t("settings.chess_pieces_figurines")]])}
          {@render choice("chess_notation", [["figurines", t("settings.chess_figurines")], ["letters", t("settings.chess_letters")]])}
        </details>
        <details class="group" open>
          <summary>{t("chess.engines_title")}</summary>
          {@render check("chess_analysis")}
          {@render text("chess_engine", t("settings.chess_engine_auto"))}
          <div class="opt" id="opt-chess_engines" class:flash={flash === "chess_engines"}>
            <span class="lab">{t("chess.engines_title")}</span>
            <div class="ptools">
              <p class="hint">{t("chess.engines_hint")}</p>
              <div class="engines" role="radiogroup" aria-label={t("chess.engines_title")}>
                {#each engines as e (e.program)}
                  <div class="ptool">
                    <label class="ptool-head"><input type="radio" name="chess-engine" checked={e.chosen} onchange={() => chooseEngine(e.program)} /> <b>{e.name}</b> <small class="hint mono">{e.program}</small></label>
                    {#if e.path}<small class="mono">{e.path}</small>{:else}<small>{t("chess.engine_not_found")}{#if e.install} · {t("install.with")} <CopyLine line={e.install} />{/if}</small>{/if}
                    {#if e.note}<small class="hint">{e.note}</small>{/if}
                    {#if !e.path && e.site}<small><button class="link" onclick={() => invoke("open_path", { path: e.site })}>{e.site}</button></small>{/if}
                  </div>
                {/each}
              </div>
            </div>
          </div>
          <div class="opt" id="opt-chess_engine_options">
            <span class="lab">{t("chess.engine_options")}</span>
            <div class="buttons">
              <button disabled={engine === "testing"} title={t("chess.engine_test_hint")} onclick={testEngine}>{engine === "testing" ? t("chess.engine_testing") : t("chess.engine_test")}</button>
              {#if engine?.options}<button title={t("chess.engine_reset_hint")} onclick={() => setEngineOption("", null, true)}>{t("chess.engine_reset")}</button>{/if}
              <button onclick={engineConfig}>{t("settings.open_config")}</button>
            </div>
            {#if engine?.error}<p class="hint err">{engine.error}</p>{/if}
            {#if engine?.options}
              <p class="hint">{t("chess.engine_tested", { name: engine.name })} · {t("chess.engine_options_hint")}</p>
              {#if !engine.options.length}<p class="hint">{t("chess.engine_options_none")}</p>{/if}
              <div class="engine-opts">
                {#each engine.options as o (o.name)}
                  <label class="field" for="eopt-{o.name}" title={optionAbout(o)}>{o.name}</label>
                  <div class="eopt">
                    {#if o.kind === "check"}
                      <input id="eopt-{o.name}" type="checkbox" checked={optionValue(o)} onchange={(ev) => setEngineOption(o.name, ev.currentTarget.checked, false, ev.currentTarget)} />
                    {:else if o.kind === "combo"}
                      <select id="eopt-{o.name}" value={optionValue(o)} onchange={(ev) => setEngineOption(o.name, ev.currentTarget.value, false, ev.currentTarget)}>
                        {#each o.vars as v (v)}<option value={v}>{v}</option>{/each}
                      </select>
                    {:else if o.kind === "spin"}
                      <input id="eopt-{o.name}" class="short" type="number" min={o.min} max={o.max} value={optionValue(o)} onchange={(ev) => setEngineOption(o.name, Number(ev.currentTarget.value), false, ev.currentTarget)} />
                    {:else}
                      <input id="eopt-{o.name}" value={optionValue(o)} maxlength="1024" spellcheck="false" onchange={(ev) => setEngineOption(o.name, ev.currentTarget.value, false, ev.currentTarget)} />
                    {/if}
                    {#if o.value != null}<button class="link" title={t("chess.engine_default", { value: o.default || "—" })} onclick={() => setEngineOption(o.name, null)}>{t("chess.option_default_button")}</button>{/if}
                    <small class="hint">{optionAbout(o)}</small>
                  </div>
                {/each}
              </div>
              {#each engine.ignored as [n, why] (n)}<p class="hint err">✗ {n}: {why}</p>{/each}
            {/if}
            <p class="hint">{t("chess.engine_options_more")}</p>
          </div>
          {@render choice("chess_engine_limit", limits)}
          {@render num("chess_engine_time", 100, 60000, 100)}
          {@render num("chess_engine_depth", 1, 99)}
          {@render num("chess_engine_nodes", 1000, 10000000000, 1000)}
          {@render num("chess_engine_lines", 1, 5)}
        </details>
        <details class="group" id="opt-review" open>
          <summary>{t("settings.group.review")}</summary>
          {@render choice("chess_review_limit", limits)}
          {@render num("chess_review_time", 100, 10000, 100)}
          {@render num("chess_review_depth", 1, 99)}
          {@render num("chess_review_nodes", 1000, 10000000000, 1000)}
        </details>
        <details class="group" id="opt-book" open>
          <summary>{t("settings.group.book")}</summary>
          {@render text("chess_book", t("settings.chess_book_auto"))}
        </details>
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
          <div class="buttons"><button onclick={() => invoke("suggest_translation", { language: ui.cfg.language }).catch((err) => (error = String(err)))} title={keysOf("suggest_translation").join(" · ")}>{t("action.suggest_translation")}</button> <span class="hint">{t("settings.suggest_translation_hint_gui")}</span></div>
        </div>

        <!-- A line between: the themes are not part of the language list above. -->
        <div class="opt apart" id="opt-{themeFor}" class:flash={flash === "theme" || flash === "tui_theme"} title={keyOf(themeFor)}>
          <div class="for" role="radiogroup" aria-label={t("settings.theme_for")}>
            <span class="lab">{t("settings.theme_for")}</span>
            <label><input type="radio" name="themefor" checked={themeFor === "theme"} onchange={() => (themeFor = "theme")} /> {t("settings.theme_gui")}</label>
            <label><input type="radio" name="themefor" checked={themeFor === "tui_theme"} onchange={() => (themeFor = "tui_theme")} /> {t("settings.theme_tui")}</label>
          </div>
          <span class="lab">{t(`setting.${themeFor}`)}</span>
          <div class="themes" role="radiogroup" aria-label={t(`setting.${themeFor}`)}>
            {#each themeGroups as [group, ids] (group)}
              <p class="label region">{t(group)}</p>
              {#each ids as id (id)}
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
        <details class="group" id="opt-panels" open>
          <summary>{t("settings.group.panels")}</summary>
          {@render check("show_hidden")}
          {@render check("confirm_delete")}
          {@render choice("right_click", [["mark", t("settings.right_click_mark")], ["menu", t("settings.right_click_menu")]])}
          {@render choice("typing", [["find", t("settings.typing_find")], ["command", t("settings.typing_command")]])}
          {@render check("hints")}
          <div class="buttons sub"><button onclick={() => invoke("hints_reset").then(() => (hintsReset = true), (e) => (error = String(e)))}>{t("settings.hints_reset")}</button>{#if hintsReset} <span class="hint">{t("settings.hints_reset_done")}</span>{/if}</div>
          {@render check("folder_sizes")}
          {@render check("disk_images")}
          {@render choice("repos_order", [["name", t("settings.repos_order_name")], ["used", t("settings.repos_order_used")]])}
        </details>
        <details class="group" id="opt-git" open>
          <summary>{t("settings.group.git")}</summary>
          {@render check("git_last_commit")}
          <!-- A tool's name from the list of those installed, or a command of one's own. -->
          {#snippet gitTool()}
            <input id="in-git_tool" list="git-tools" value={s.git_tool} placeholder={t("settings.git_tool_auto", { name: gitTools[0] ?? t("settings.git_tool_terminal") })} spellcheck="false" onchange={(e) => set("git_tool", e.currentTarget.value.trim())} />
            <datalist id="git-tools">{#each gitTools as n (n)}<option value={n}></option>{/each}</datalist>
          {/snippet}
          {@render field("git_tool", gitTool)}
          {@render check("git_prefer_git")}
        </details>
        <details class="group" id="opt-programs" open>
          <summary>{t("settings.group.programs")}</summary>
          {@render text("editor", "$EDITOR")}
          {@render text("viewer", t("setting.viewer.empty"))}
          <!-- A terminal from those installed, or a command of one's own ({dir} the folder). -->
          {#snippet terminal()}
            <select id="in-terminal" value={termListed ? s.terminal : OTHER} onchange={(e) => ((termOther = e.currentTarget.value === OTHER), termOther || set("terminal", e.currentTarget.value))}>
              <option value="">{t("settings.git_tool_auto", { name: terms[0] ?? t("settings.terminal_none") })}</option>
              {#each terms[1] as [name, label] (name)}<option value={name}>{label}</option>{/each}
              <option value={OTHER}>{t("settings.git_tool_other")}</option>
            </select>
            {#if !termListed}<input id="in-terminal_other" value={s.terminal} spellcheck="false" placeholder="footclient -D {"{dir}"}" onchange={(e) => set("terminal", e.currentTarget.value.trim())} />{/if}
          {/snippet}
          {@render field("terminal", terminal)}
          {@render check("bom_viewer")}
          {@render check("provenance_viewer")}
        </details>
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
        <div class="opt" id="opt-history_servers">
          <span class="lab">{t("settings.history_servers")}</span>
          <div class="keys">
            {#each servers as server (server)}
              <span class="mono">{server}</span>
              <span><button class="link" onclick={() => forgetServer(server)}>{t("settings.history_server_forget")}</button></span>
            {:else}
              <span class="hint">{t("settings.history_servers_none")}</span>
            {/each}
          </div>
          <small class="hint">{t("settings.history_servers_hint")}</small>
        </div>
        <details class="group" id="opt-disk" class:flash={flash === "disk"}>
          <summary>{t("disk.title")} <span class="hint">{disk ? size(bytesOf(disk)) : ""}</span></summary>
          <p class="hint">{t("disk.hint")}</p>
          {#snippet diskRow(d)}
            <div class="disk-row">
              <strong class="mono" title={d.paths[0] ?? d.name}>{d.name}</strong>
              <span class="num">{size(d.bytes)}</span>
              <span>{d.cost}{#if d.in_use}<span class="badge">{t("models.in_use")}</span>{/if}</span>
              <span class="disk-buttons">
                {#if d.paths.length}<button class="link" title={d.paths[0]} onclick={() => showInPanel(d.paths[0], !(d.name.endsWith("/") || d.kind !== "own"))}>{t("settings.show_in_panel")}</button>{/if}
                {#if d.clear && d.kind === "yours"}
                  <button class="danger" disabled={!d.bytes} onclick={() => clearDisk(d)} onblur={() => (clearing = "")}>{clearing === d.id ? t("disk.confirm_remove", { what: d.paths[0] ?? d.name }) : t("disk.remove")}</button>
                {:else if d.clear}
                  <button class="danger" disabled={!d.bytes} onclick={() => clearDisk(d)} onblur={() => (clearing = "")}>{clearing === d.id ? t("disk.confirm_one", { what: d.paths[0] ?? d.name }) : t("disk.clear")}</button>
                {/if}
              </span>
              <small class="hint">{d.what}</small>
            </div>
          {/snippet}
          {#each diskOf("own") as d (d.id)}{@render diskRow(d)}{/each}
          {#if diskOf("outside").length}
            <p class="lab">{t("disk.head.outside")}</p>
            <p class="hint">{t("disk.outside_hint")}</p>
            {#each diskOf("outside") as d (d.id)}{@render diskRow(d)}{/each}
          {/if}
          {#if diskOf("yours").length}
            <p class="lab" id="opt-disk_yours" class:flash={flash === "disk_yours"}>{t("disk.head.yours")}</p>
            <p class="hint">{t("disk.yours_hint")}</p>
            {#each diskOf("yours") as d (d.id)}{@render diskRow(d)}{/each}
          {/if}
          <div class="danger-row">
            {#if unusedModels}<label class="check"><input type="checkbox" bind:checked={also.models} /> {t("disk.also_models", { size: size(unusedModels) })}</label>{/if}
            {#if outsideBytes}<label class="check"><input type="checkbox" bind:checked={also.outside} /> {t("disk.also_outside", { size: size(outsideBytes) })}</label>{/if}
            {#if clearing === "*"}<p class="hint err">{t("disk.confirm")} {going.map((d) => d.paths[0] ?? d.name).join(", ")}</p>{/if}
            <button class="danger" disabled={!going.length} onclick={() => clearDisk(null)} onblur={() => (clearing = "")}>{clearing === "*" ? t("disk.confirm_all") : t("disk.clear_all", { size: size(bytesOf(going)) })}</button>
            <p class="hint">{t("disk.clear_all_hint")}</p>
          </div>
          {#if diskSaid}<p class="hint">{diskSaid}</p>{/if}
        </details>
        <div class="opt">
          <span class="lab">{t("settings.paths")}</span>
          <div class="keys">
            {#each ui.cfg.paths.filter(([, p]) => p) as [what, p] (what)}
              <span>{t(`settings.path.${what.replace(" ", "_")}`)}</span>
              <span><span class="mono">{p}</span> <button class="link" onclick={() => showInPanel(p, !["cache", "models", "previews", "archive looks"].includes(what))}>{t("settings.show_in_panel")}</button></span>
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
    /* One long word ("Previsualitzacions", "Forhåndsvisninger") breaks rather than spill out
       of a narrow window's list. */
    overflow-wrap: anywhere;
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
  .opt.apart {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border-fg);
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
  .status .steps {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .status .link.strong {
    font-weight: bold;
    text-align: start;
  }
  .levels {
    border: 0;
    margin: 0;
    padding: 0;
    max-width: 48em;
  }
  .level-list {
    display: grid;
    gap: 6px;
  }
  legend {
    font-weight: bold;
    margin-bottom: 6px;
    padding: 0;
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
  .engine-opts {
    display: grid;
    grid-template-columns: minmax(8em, max-content) 1fr;
    gap: 0.3em 0.8em;
    align-items: start;
    margin: 0.4em 0;
  }
  .eopt {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2em 0.6em;
    align-items: center;
  }
  .ptools {
    display: grid;
    gap: 10px;
    max-width: 40em;
  }
  .ptools > .hint {
    margin: 0;
  }
  .ptool {
    display: grid;
    gap: 2px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-fg);
  }
  .ptool-head {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }
  .ptool-head b {
    white-space: nowrap;
  }
  .ptool-use select {
    width: auto;
    max-width: 100%;
  }
  .ptool .exts {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ptool-image,
  .ptool-use {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 8px;
    align-items: center;
  }
  /* The image's words wrap; Pull and Remove stay together at the end, never on a line alone. */
  .ptool-image {
    flex-wrap: nowrap;
  }
  .ptool-image small {
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .ptool-image button {
    flex: none;
  }
  .ptool-use small {
    flex: 1 1 14em;
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
  .disk-row {
    display: grid;
    grid-template-columns: 14em 6em 1fr auto;
    gap: 2px 12px;
    align-items: baseline;
    padding: 6px 0;
    border-bottom: 1px solid var(--border-fg);
  }
  .disk-row .num {
    text-align: right;
  }
  .disk-row small {
    grid-column: 1 / -1;
  }
  .disk-buttons {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
  /* Its second-click question names the whole path: it wraps, the row's text keeps its width. */
  .disk-buttons .danger {
    max-width: 24em;
    overflow-wrap: anywhere;
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
    white-space: pre-line;
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

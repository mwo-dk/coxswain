<script>
  // Every modal: prompts, confirmations, find file, menus, batch rename, tags, help.
  // App forwards keys through `handleKey`; returning true means "handled".
  import { tick } from "svelte";
  import { ui, tab, cd, load } from "./app.svelte.js";
  import { Channel } from "@tauri-apps/api/core";
  import { invoke, takesPassword, basename, parent, size, date, TAGS, TAG_COLORS, tagName, isHistory, historyOf } from "./lib.js";
  import { t, tn, num } from "./i18n.svelte.js";

  let input = $state();
  let listEl = $state();

  // Focus the first field whenever a modal opens.
  $effect(() => {
    if (ui.modal) tick().then(() => {
      input?.focus();
      if (["input", "pack"].includes(ui.modal?.kind)) input?.select?.();
    });
  });

  // Keep the highlighted row in view.
  $effect(() => {
    if (ui.modal && "cursor" in ui.modal) {
      ui.modal.cursor;
      tick().then(() => listEl?.querySelector(".cursor")?.scrollIntoView({ block: "nearest" }));
    }
  });

  const close = () => (ui.modal = null);

  /** Close `m` and run its action, once, even if Enter and a button click both fire. */
  function confirm(m, ...args) {
    if (ui.modal !== m) return;
    close();
    m.run(...args);
  }

  // ------------------------------------------------------------ find file

  // One search at a time: keys typed while one runs collapse into a single follow-up, so
  // fast typing never queues a scan per keystroke.
  let busy = false;
  let again = false;
  /** Find file's depths: names everywhere, names in this folder, the text of files, Ask. */
  function setMode(m, mode) {
    m.mode = mode;
    m.res = null;
    m.cursor = 0;
    runSearch();
  }
  /** The depth each action opens Find file at; Tab and Shift+Tab go to the next and the previous. */
  const depths = { search: 0, search_text: 2, ask: 3 };
  /** A depth button's tooltip: its own key, if it has one, and Tab. */
  const depthKey = (i) => {
    const act = Object.keys(depths).find((a) => depths[a] === i);
    return [act && ui.cfg.actions[act]?.[1], "Tab / Shift+Tab"].filter(Boolean).join(" · ");
  };
  /** A snippet with the words it found marked; everything else is text, never markup. */
  const marked = (s) =>
    s
      .replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c])
      .replaceAll("\u0001", "<mark>")
      .replaceAll("\u0002", "</mark>");

  export async function runSearch() {
    const m = ui.modal;
    if (m?.kind !== "search" || m.mode === 3) return;
    if (busy) return void (again = true);
    busy = true;
    try {
      const res = await invoke("search", { query: m.query, scope: m.mode === 1 ? tab().dir : null, text: m.mode === 2 });
      if (ui.modal === m) {
        m.res = res;
        m.cursor = 0;
      }
    } finally {
      busy = false;
    }
    if (again) {
      again = false;
      runSearch();
    }
  }

  // Refresh results while the first index is still being built. A stale index (being
  // refreshed in the background) already answers correctly, so it is not polled.
  $effect(() => {
    if (ui.modal?.kind !== "search") return;
    const timer = setInterval(() => ui.modal?.res?.state === "building" && runSearch(), 700);
    return () => clearInterval(timer);
  });

  // Ask: a question answered by the user's chat model from the passages closest to it, with
  // the sources numbered. Follow-ups carry the turns before; closing Find file forgets them.
  const askReady = () => ui.cfg.settings.search_meaning && !!ui.cfg.settings.ask_model;
  async function askNow(m) {
    const question = m.query.trim();
    if (!question || m.asking || !askReady()) return;
    const earlier = (m.chat ?? []).filter((c) => c.a && !c.error).map((c) => [c.q, c.a]);
    m.chat = [...(m.chat ?? []), { q: question, a: "", sources: [], error: "" }];
    const turn = m.chat[m.chat.length - 1];
    Object.assign(m, { query: "", asking: true, cursor: 0 });
    const events = new Channel();
    events.onmessage = (e) => {
      if (ui.modal !== m) return void invoke("ask_stop");
      if (e.k === "sources") turn.sources = e.paths;
      else turn.a += e.text;
      // Follows the answer down, unless you scrolled up to read something.
      const end = !listEl || listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 40;
      if (end) tick().then(() => listEl?.lastElementChild?.scrollIntoView({ block: "end" }));
    };
    try {
      await invoke("ask", { question, earlier, onEvent: events });
    } catch (e) {
      turn.error = String(e);
    } finally {
      m.asking = false;
    }
  }
  /** An answer in pieces: text, and the [n] that point at its sources. */
  const cited = (text) => text.split(/(\[\d+\])/).map((part) => ({ part, n: /^\[\d+\]$/.test(part) ? +part.slice(1, -1) : 0 }));
  const lastSources = (m) => m.chat?.at(-1)?.sources ?? [];

  async function goToHit(h) {
    close();
    const tb = tab();
    // A commit: its folder as it was then.
    if (isHistory(h.path)) return cd(tb, h.path);
    await cd(tb, parent(h.path));
    const i = tb.items.findIndex((e) => e.path === h.path);
    if (i >= 0) tb.cursor = i;
  }

  // ------------------------------------------------------------ batch rename

  let planSeq = 0;
  export async function replan() {
    const m = ui.modal;
    if (m?.kind !== "rename") return;
    const n = ++planSeq;
    try {
      const plan = await invoke("rename_plan", {
        dir: m.dir,
        selected: m.names,
        pattern: m.pattern || "^$",
        replacement: m.replacement,
        flags: { case_insensitive: m.ci, global: m.global, whole_name: m.whole },
      });
      if (n === planSeq) Object.assign(m, { plan, error: "" });
    } catch (e) {
      if (n === planSeq) Object.assign(m, { plan: [], error: String(e) });
    }
  }

  async function applyRename() {
    const m = ui.modal;
    const changed = m.plan.filter((p) => p.from !== p.to);
    if (!changed.length || m.plan.some((p) => p.conflict)) return;
    try {
      await invoke("rename_apply", { dir: m.dir, plan: $state.snapshot(m.plan) });
      ui.status = tn("dialogs.renamed", changed.length);
      close();
      load(tab());
    } catch (e) {
      m.error = String(e);
    }
  }

  // ------------------------------------------------------------ tags

  async function setTag(color) {
    const m = ui.modal;
    close();
    await invoke("set_tags", { paths: m.paths, color });
    load(tab());
  }

  // ------------------------------------------------------------ properties

  async function savePerms() {
    const m = ui.modal;
    const mode = m.props.mode === null ? null : parseInt(m.mode, 8);
    if (mode !== null && !validMode(m.mode)) return void (m.error = t("dialogs.perm_invalid"));
    try {
      await invoke("set_permissions", { path: m.props.path, mode, readonly: m.readonly });
      close();
      load(tab());
    } catch (e) {
      m.error = String(e);
    }
  }

  const validMode = (s) => /^[0-7]{3,4}$/.test(s);

  /** rwxr-xr-x for 0o755. */
  const rwx = (mode) => [6, 3, 0].map((s) => ["r", "w", "x"].map((c, i) => (mode >> (s + 2 - i)) & 1 ? c : "-").join("")).join("");

  // Splits a text at its {placeholders}: odd entries are the placeholder names, so the
  // markup around them (code, bold) stays out of the translated text.
  const parts = (s) => s.split(/\{(\w+)\}/);

  // Screen-reader name for modals that have no title of their own.
  const KIND_LABEL = { help: "help.title", search: "search.title", rename: "action.batch_rename", props: "action.properties", tag: "action.tag" };

  // ------------------------------------------------------------ pack

  /** Pack, with the password if the target takes one and both fields say the same. */
  function packNow(m) {
    const locks = takesPassword(m.value);
    if (locks && m.password !== m.again) return;
    confirm(m, m.value, locks && m.password ? m.password : null, /\.7z$/i.test(m.value) && m.hide);
  }

  // ------------------------------------------------------------ menus

  const menuItems = (m) => m.items.filter((it) => it.label.toLowerCase().includes(m.filter.toLowerCase()));

  function run(it) {
    close();
    it.run();
  }

  /** Keys while a modal is open. */
  export function handleKey(e, k) {
    const m = ui.modal;
    const act = ui.cfg.keymap[k];
    if (k === "Esc") return close(), true;
    switch (m.kind) {
      case "input":
        if (k !== "Enter") return false;
        confirm(m, m.value);
        return true;
      case "pack":
        if (k !== "Enter") return false;
        packNow(m);
        return true;
      case "confirm":
        if (k === "Enter" || k === "y" || k === "Shift+Y") confirm(m);
        else if (k === "n" || k === "Shift+N") close();
        return true;
      case "search": {
        if (k === "Tab" || k === "Shift+Tab" || act in depths) {
          setMode(m, depths[act] ?? (m.mode + (k === "Tab" ? 1 : 3)) % 4);
          return true;
        }
        if (m.mode === 3) {
          const src = lastSources(m);
          if (k === "Enter" && m.query.trim()) askNow(m);
          else if (k === "Enter" && src[m.cursor]) goToHit({ path: src[m.cursor] });
          else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
          else if (k === "Down") m.cursor = Math.min(src.length - 1, m.cursor + 1);
          else if (act === "edit" && src[m.cursor]) invoke("edit_path", { path: src[m.cursor] });
          else return false;
          return true;
        }
        const hits = m.res?.hits ?? [];
        const h = hits[m.cursor];
        if (k === "Enter" && h) goToHit(h);
        else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(hits.length - 1, m.cursor + 1);
        else if (k === "PageUp") m.cursor = Math.max(0, m.cursor - 15);
        else if (k === "PageDown") m.cursor = Math.min(hits.length - 1, m.cursor + 15);
        else if (act === "edit" && h && !h.is_dir) invoke("edit_path", { path: h.path });
        else return false;
        return true;
      }
      case "menu": {
        const items = menuItems(m);
        // An entry's key is one character, upper case for Shift (`Shift+A` is `A`).
        const ch = k.replace(/^Shift\+/, "");
        if (k === "Enter" && items[m.cursor]) run(items[m.cursor]);
        else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(items.length - 1, m.cursor + 1);
        else if (m.direct && [...ch].length === 1 && m.items.some((it) => it.key === ch)) run(m.items.find((it) => it.key === ch));
        else return false;
        return true;
      }
      case "props":
        if (k === "Enter") savePerms();
        else return false;
        return true;
      case "rename":
        if (k === "Enter") applyRename();
        else return false;
        return true;
      case "tag": {
        const n = Number(k);
        if (k === "0") setTag("");
        else if (n >= 1 && n <= TAGS.length) setTag(TAGS[n - 1]);
        return true;
      }
      case "bom":
      case "settings":
      case "dupes":
        // windows with fields and buttons of their own: Esc (above) closes them, Enter is theirs
        return false;
      default:
        if (k === "Enter" || act === "help") close();
        return m.kind === "message";
    }
  }
</script>

{#if ui.modal && !["dupes", "settings", "bom"].includes(ui.modal.kind)}
  {@const m = ui.modal}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="dialog {m.kind}" role="dialog" aria-modal="true" aria-label={m.title ?? (KIND_LABEL[m.kind] ? t(KIND_LABEL[m.kind]) : m.kind)}>
      {#if m.kind === "input"}
        <h2>{m.title}</h2>
        <label>{m.label}{#if m.secret}<input bind:this={input} bind:value={m.value} type="password" autocomplete="off" />{:else}<input bind:this={input} bind:value={m.value} spellcheck="false" />{/if}</label>
        <div class="buttons">
          <button class="primary" onclick={() => confirm(m, m.value)}>{t("common.ok")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
      {:else if m.kind === "pack"}
        {@const locks = takesPassword(m.value)}
        <h2>{m.title}</h2>
        <label>{m.label}<input bind:this={input} bind:value={m.value} spellcheck="false" /></label>
        {#if locks}
          <div class="grid2">
            <label>{t("archive.pack_password")}<input bind:value={m.password} type="password" autocomplete="new-password" /></label>
            <label>{t("archive.pack_confirm")}<input bind:value={m.again} type="password" autocomplete="new-password" /></label>
          </div>
          {#if /\.7z$/i.test(m.value)}<label class="check"><input type="checkbox" bind:checked={m.hide} disabled={!m.password} /> {t("archive.pack_hide_names")}</label>{/if}
          {#if m.password !== m.again && m.again}<p class="err">{t("archive.pack_mismatch")}</p>{/if}
        {:else}
          <p class="meta">{t("archive.pack_no_password")}</p>
        {/if}
        <div class="buttons">
          <button class="primary" disabled={locks && m.password !== m.again} onclick={() => packNow(m)}>{t("common.ok")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
      {:else if m.kind === "confirm"}
        <h2>{m.title}</h2>
        <p>{m.text}</p>
        <div class="buttons">
          <button class="primary danger" bind:this={input} onclick={() => confirm(m)}>{m.ok ?? t("common.delete")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
      {:else if m.kind === "message"}
        <h2>{m.title}</h2>
        <pre>{m.text}</pre>
        <div class="buttons"><button class="primary" bind:this={input} onclick={close}>{t("common.ok")}</button></div>
      {:else if m.kind === "help"}
        <h2>{t("dialogs.help_title", { version: ui.cfg.version })}</h2>
        <div class="help" bind:this={input} tabindex="-1">
          <table>
            <tbody>
              <!-- Under group headings, the most used first; the first configured key first, and a
                   letter key written as on the keyboard: Ctrl+G, not Ctrl+g. -->
              {#each ui.cfg.groups as [group, names] (group)}
                <tr class="head"><th colspan="2">{group}</th></tr>
                {#each names as name (name)}
                  {@const [label, first] = ui.cfg.actions[name]}
                  {@const keys = Object.entries(ui.cfg.keymap)
                    .filter(([, a]) => a === name)
                    .map(([k]) => k)
                    .sort((a, b) => (b === first) - (a === first))
                    .map((k) => k.replace(/\+([a-z])$/, (_, c) => "+" + c.toUpperCase()))}
                  <tr><td>{label}</td><td>{#each keys as k, i (i)}<kbd>{k}</kbd>{/each}</td></tr>
                {/each}
              {/each}
            </tbody>
          </table>
          <p>{t("dialogs.help_mouse")}</p>
          <p>{#each parts(t("dialogs.help_syntax")) as s, i (i)}{#if i % 2}<b>{t("search.title")}</b>{:else}{s}{/if}{/each} <code>foo bar</code> · <code>foo|bar</code> · <code>!foo</code> · <code>*.rs</code> · <code>ext:rs;toml</code> · <code>file:</code> <code>folder:</code> · <code>src/ foo</code> · <code>case:</code></p>
          <p>
            {#each parts(t("dialogs.help_config")) as s, i (i)}{#if i % 2}<code>{s === "path" ? ui.cfg.config_path : "coxswain --dump-config"}</code>{:else}{s}{/if}{/each}
          </p>
        </div>
      {:else if m.kind === "menu"}
        <h2>{m.title}</h2>
        {#if !m.direct}
          <input bind:this={input} bind:value={m.filter} oninput={() => (m.cursor = 0)} placeholder={t("dialogs.filter")} spellcheck="false" />
        {/if}
        <ul class="list" bind:this={listEl}>
          {#each menuItems(m) as it, i (it.label + i)}
            <!-- Unfiltered, the list is m.items: a heading where the group changes. -->
            {#if !m.filter && it.group && it.group !== m.items[i - 1]?.group}<li class="head">{it.group}</li>{/if}
            <li>
              <button class:cursor={i === m.cursor} onclick={() => run(it)} onmouseenter={() => (m.cursor = i)}>
                {#if it.icon}<span class="glyph">{it.icon}</span>{/if}
                <span class="grow">{it.label}</span>
                {#if it.key}<kbd>{it.key}</kbd>{/if}
              </button>
            </li>
          {:else}
            <li class="none">{t("dialogs.no_matches")}</li>
          {/each}
        </ul>
      {:else if m.kind === "search"}
        <div class="search-bar">
          <span class="glyph">{"\u{f002}"}</span>
          <input bind:this={input} bind:value={m.query} oninput={runSearch} placeholder={t(m.mode === 3 ? "dialogs.ask_placeholder" : m.mode === 2 ? "dialogs.text_placeholder" : "dialogs.search_placeholder")} spellcheck="false" />
          <!-- The four ways to look, all in sight; Tab goes to the next. -->
          <div class="scopes" role="radiogroup" aria-label={t("search.title")}>
            {#each [t("dialogs.scope_everywhere"), t("dialogs.scope_in", { folder: basename(tab().dir) }), t("dialogs.scope_text"), t("dialogs.scope_ask")] as label, i (i)}
              <button class="scope" class:on={m.mode === i} role="radio" aria-checked={m.mode === i} title={depthKey(i)} onclick={() => { setMode(m, i); input.focus(); }}>{label}</button>
            {/each}
          </div>
        </div>
        {#if m.mode === 3}
          {#if !askReady()}
            <p class="meta tip">
              {t(ui.cfg.settings.search_meaning ? "dialogs.ask_setup" : "dialogs.ask_setup_meaning")}
              <button class="link" onclick={() => (ui.modal = { kind: "settings", section: ui.cfg.settings.search_meaning ? "ask" : "meaning" })}>{t("dialogs.ask_setup_open")}</button>
            </p>
          {:else if !m.chat?.length}
            <p class="meta">{t("dialogs.ask_hint", { model: ui.cfg.settings.ask_model })}</p>
          {/if}
          <div class="list chat" bind:this={listEl}>
            {#each m.chat ?? [] as c, ci (ci)}
              <p class="question" dir="auto">{c.q}</p>
              <p class="answer" dir="auto">
                {#each cited(c.a) as { part, n }, pi (pi)}{#if n && c.sources[n - 1]}<button class="cite" title={c.sources[n - 1]} onclick={() => goToHit({ path: c.sources[n - 1] })}>{part}</button>{:else}{part}{/if}{/each}{#if m.asking && ci === m.chat.length - 1}<span class="typing">▍</span>{/if}
              </p>
              {#if m.asking && ci === m.chat.length - 1 && !c.a && c.sources.length}<p class="meta">{t("dialogs.ask_waiting", { model: ui.cfg.settings.ask_model })}</p>{/if}
              {#if c.error}<p class="err">{c.error}</p>{/if}
              {#if c.sources.length}
                <ol class="sources">
                  {#each c.sources as src, i (i)}
                    <li><button class:cursor={ci === m.chat.length - 1 && i === m.cursor} onclick={() => goToHit({ path: src })}><b>{basename(src)}</b> <span class="where"><bdi>{parent(src)}</bdi></span></button></li>
                  {/each}
                </ol>
              {/if}
            {/each}
          </div>
          <p class="meta">{t("dialogs.ask_footer")}</p>
        {:else}
          <p class="meta">
            {#if m.res}
              {m.query ? `${tn("search.matches", m.res.total, { ms: num(m.res.micros / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })} · ` : ""}{#if m.mode === 2}{tn("search.texts", m.res.texts)}{m.res.pending ? t("search.reading", { n: num(m.res.pending) }) : ""}{:else}{tn("search.indexed", m.res.indexed)}{m.res.state === "building" ? t("search.building") : m.res.state === "stale" ? t("search.refreshing") : ""}{/if}{m.res.total > m.res.hits.length ? ` · ${tn("dialogs.showing_first", m.res.hits.length)}` : ""}
            {:else}{m.mode === 2 ? t("dialogs.text_hint") : `${t("dialogs.search_hint")} ${t("dialogs.search_tab_hint")}`}{/if}
          </p>
          {#if m.mode === 2 && m.res && !m.res.meaning}
            <p class="meta tip">
              {t("dialogs.meaning_tip")}
              <button class="link" onclick={() => (ui.modal = { kind: "settings", section: "meaning" })}>{t("dialogs.meaning_tip_open")}</button>
            </p>
          {/if}
          <ul class="list hits" bind:this={listEl}>
            {#each m.res?.hits ?? [] as h, i (h.path)}
              <li>
                <button class:cursor={i === m.cursor} onclick={() => (m.cursor = i)} ondblclick={() => goToHit(h)}>
                  {#if isHistory(h.path)}
                    <!-- A commit: the repository it is in; the snippet says which commit. -->
                    <span class="glyph">{"\u{f417}"}</span>
                    <b>{basename(historyOf(h.path))}</b>
                    <span class="where"><bdi>{t("history.commit_in", { repo: historyOf(h.path) })}</bdi></span>
                  {:else}
                    <span class="glyph" class:dir={h.is_dir}>{h.is_dir ? "\u{f07b}" : "\u{f15b}"}</span>
                    <b>{basename(h.path)}</b>
                    <span class="where"><bdi>{parent(h.path)}</bdi></span>
                  {/if}
                  {#if h.similar != null}<span class="snippet" dir="auto"><em>{t("search.similar_to")}</em> {h.snippet}</span>
                  {:else if h.snippet}<span class="snippet" dir="auto">{@html marked(h.snippet)}</span>{/if}
                </button>
              </li>
            {/each}
          </ul>
          <p class="meta">{t("dialogs.search_footer")}</p>
        {/if}
      {:else if m.kind === "rename"}
        <h2>{tn("dialogs.rename_title", m.names.length)}</h2>
        <div class="grid2">
          <label>{t("dialogs.rename_find")}<input bind:this={input} bind:value={m.pattern} oninput={replan} spellcheck="false" placeholder={t("dialogs.rename_find_hint")} /></label>
          <label>{t("dialogs.rename_replace")}<input bind:value={m.replacement} oninput={replan} spellcheck="false" placeholder={"$1 · {n} · {n:3}"} /></label>
        </div>
        <div class="flags">
          <label><input type="checkbox" bind:checked={m.ci} onchange={replan} /> {t("dialogs.rename_ignore_case")}</label>
          <label><input type="checkbox" bind:checked={m.global} onchange={replan} /> {t("dialogs.rename_all")}</label>
          <label><input type="checkbox" bind:checked={m.whole} onchange={replan} /> {t("dialogs.rename_extension")}</label>
        </div>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        <ul class="list plan">
          {#each m.plan as p (p.from)}
            <li class:changed={p.from !== p.to} class:bad={p.conflict}>
              <span class="from">{p.from}</span><span class="arrow">→</span><span class="to">{p.to}</span>
              {#if p.conflict}<span class="why">{p.conflict}</span>{/if}
            </li>
          {/each}
        </ul>
        <div class="buttons">
          <button class="primary" disabled={!m.plan.some((p) => p.from !== p.to) || m.plan.some((p) => p.conflict)} onclick={applyRename}>{t("common.rename")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
      {:else if m.kind === "props"}
        {@const p = m.props}
        <h2>{basename(p.path)}</h2>
        <dl class="props">
          <dt>{t("dialogs.location")}</dt><dd>{parent(p.path)}</dd>
          <dt>{t("dialogs.type")}</dt><dd>{t(`dialogs.kind.${p.kind}`)}{#if p.link_target} → {p.link_target}{/if}</dd>
          <dt>{t("dialogs.size")}</dt><dd>{size(p.size)}{#if p.size >= 10_240} {tn("dialogs.bytes", p.size)}{/if}{#if p.kind === "folder"} {tn("dialogs.in_files", p.files)}{/if}</dd>
          {#if p.created}<dt>{t("dialogs.created")}</dt><dd>{date(p.created)}</dd>{/if}
          {#if p.modified}<dt>{t("dialogs.modified")}</dt><dd>{date(p.modified)}</dd>{/if}
          {#if p.accessed}<dt>{t("dialogs.accessed")}</dt><dd>{date(p.accessed)}</dd>{/if}
          {#if p.mode !== null}
            <dt>{t("dialogs.owner")}</dt><dd>{t("dialogs.owner_ids", { uid: p.uid, gid: p.gid })}</dd>
            <dt>{t("dialogs.permissions")}</dt>
            <dd class="perm"><input bind:this={input} bind:value={m.mode} size="5" spellcheck="false" /> <code>{validMode(m.mode) ? rwx(parseInt(m.mode, 8)) : ""}</code></dd>
          {:else}
            <dt>{t("dialogs.attributes")}</dt><dd><label class="check"><input type="checkbox" bind:checked={m.readonly} /> {t("dialogs.readonly")}</label></dd>
          {/if}
        </dl>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        <div class="buttons">
          <button class="primary" onclick={savePerms}>{t("common.apply")}</button>
          <button onclick={close}>{t("common.close")}</button>
        </div>
      {:else if m.kind === "tag"}
        <h2>{t("action.tag")}</h2>
        <div class="tags">
          {#each TAGS as c, i (c)}
            <button onclick={() => setTag(c)} title={tagName(c)}><span class="dot" style:background={TAG_COLORS[c]}></span>{i + 1} {tagName(c)}</button>
          {/each}
          <button onclick={() => setTag("")}><span class="dot none"></span>0 {t("dialogs.tag_none")}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: start center;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.45);
    z-index: 10;
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: min(460px, 92vw);
    max-width: 92vw;
    max-height: 76vh;
    padding: 16px 18px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    box-sizing: border-box;
  }
  .dialog.search,
  .dialog.rename,
  .dialog.help {
    width: min(900px, 92vw);
  }
  .dialog.search {
    height: 70vh;
    padding: 10px 12px;
  }
  .dialog.menu {
    width: min(520px, 92vw);
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--hidden-fg);
    font-size: 0.9em;
  }
  input:not([type="checkbox"]) {
    font: inherit;
    font-size: 1rem;
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 7px 10px;
    outline: none;
  }
  input:focus {
    border-color: var(--accent-bg);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .buttons button {
    padding: 6px 16px;
    border-radius: var(--r);
    border: 1px solid var(--border-fg);
  }
  .buttons .primary {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .buttons .danger {
    background: var(--git-deleted-fg);
  }
  .buttons button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    font-family: var(--mono-font);
    overflow: auto;
  }
  .glyph {
    font-family: var(--icon-font);
    color: var(--hidden-fg);
  }
  .glyph.dir {
    color: var(--directory-fg);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  .list li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border-radius: var(--r);
    text-align: start;
    white-space: nowrap;
  }
  .list button.cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .grow {
    flex: 1;
  }
  kbd {
    font-family: var(--mono-font);
    font-size: 0.8em;
    padding: 1px 6px;
    margin-inline-start: 4px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border-fg);
    color: var(--hidden-fg);
  }
  .none {
    padding: 8px 10px;
    color: var(--hidden-fg);
  }
  .search-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
  }
  .search-bar input {
    flex: 1;
    font-size: 1.1rem;
    border: 0;
    background: transparent;
    padding: 6px 0;
  }
  .scopes {
    display: flex;
    gap: 4px;
  }
  .scope.on {
    color: var(--accent-fg);
    background: var(--accent-bg);
    border-color: transparent;
  }
  .tip .link {
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent-bg);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
  .scope {
    padding: 4px 10px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-pill);
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .meta {
    font-size: 0.85em;
    color: var(--hidden-fg);
    padding: 0 8px;
  }
  .chat {
    overflow-y: auto;
    padding: 0 0.25em;
  }
  .chat .question {
    font-weight: bold;
    margin: 0.8em 0 0.3em;
  }
  .chat .answer {
    white-space: pre-wrap;
    margin: 0 0 0.4em;
    line-height: 1.45;
  }
  .chat .cite {
    all: unset;
    cursor: pointer;
    color: var(--accent, currentColor);
    text-decoration: underline;
  }
  .chat .sources {
    margin: 0 0 0.6em;
    padding-inline-start: 2em;
    font-size: 0.9em;
  }
  .chat .sources button {
    all: unset;
    cursor: pointer;
    display: block;
    width: 100%;
  }
  .chat .sources button.cursor {
    outline: 1px solid currentColor;
  }
  .typing {
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    to {
      opacity: 0;
    }
  }
  .hits {
    flex: 1;
    border-top: 1px solid var(--border-fg);
    padding-top: 6px;
  }
  .hits b {
    font-weight: 600;
    color: var(--search-hit-fg);
  }
  .list button.cursor b {
    color: inherit;
  }
  .snippet {
    flex-basis: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-inline-start: 1.9em;
    color: var(--hidden-fg);
    font-size: 0.9em;
    text-align: start;
  }
  .snippet :global(mark) {
    color: var(--search-hit-fg);
    background: none;
    font-weight: 600;
  }
  .list.hits button:has(.snippet) {
    flex-wrap: wrap;
    row-gap: 0;
  }
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    color: var(--hidden-fg);
    font-size: 0.9em;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .flags {
    display: flex;
    gap: 18px;
  }
  .flags label {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--dialog-fg);
  }
  /* One grid for all rows, so a conflict's reason does not push its row out of line. */
  .plan {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr auto 1fr auto;
    align-content: start;
    column-gap: 10px;
    font-family: var(--mono-font);
    font-size: 0.9em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
  }
  .plan li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    padding: 2px 0;
    color: var(--hidden-fg);
    white-space: nowrap;
  }
  .plan span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .plan .changed .to {
    color: var(--git-added-fg);
  }
  .plan .bad .to,
  .why,
  .err {
    color: var(--git-deleted-fg);
  }
  .why {
    font-family: var(--font);
    font-size: 0.85em;
  }
  .tags {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }
  .tags button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--r);
    border: 1px solid var(--border-fg);
  }
  .tags button:hover {
    background: var(--cursor-bg);
  }
  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  .dot.none {
    border: 1px dashed var(--hidden-fg);
  }
  .props {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 0;
  }
  .props dt {
    color: var(--hidden-fg);
  }
  .props dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .perm {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .perm input {
    width: 5em;
    font-family: var(--mono-font);
    padding: 3px 8px;
  }
  .check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--dialog-fg);
  }
  .help {
    overflow: auto;
    outline: none;
  }
  .help table {
    width: 100%;
    border-collapse: collapse;
  }
  .help td {
    padding: 3px 6px;
    border-bottom: 1px solid color-mix(in srgb, var(--border-fg) 50%, transparent);
  }
  .help th,
  .list li.head {
    padding: 12px 6px 4px;
    text-align: start;
    font-weight: 600;
    color: var(--directory-fg);
  }
  .list li.head {
    padding-inline: 10px;
  }
  .help p {
    margin-top: 10px;
    color: var(--hidden-fg);
  }
  code {
    font-family: var(--mono-font);
  }
</style>

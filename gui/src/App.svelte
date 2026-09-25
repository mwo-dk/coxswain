<script>
  import { SvelteSet } from "svelte/reactivity";
  import { tick } from "svelte";
  import Panel from "./Panel.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke, keyString, basename, parent, glob, applyTheme } from "./lib.js";

  let cfg = $state(null);
  let panels = $state([]);
  let active = $state(0);
  let showHidden = $state(true);
  let cmd = $state("");
  let status = $state("");
  let quick = $state(null);
  let lastOutput = $state("");
  /** One modal at a time: { kind: "input" | "confirm" | "search" | "menu" | "view" | "help" | "message", ... } */
  let modal = $state(null);
  let cmdInput = $state();
  let modalInput = $state();

  const cur = () => panels[active];
  const other = () => panels[active ^ 1];
  const entry = (p = cur()) => p.entries[p.cursor];

  function newPanel(dir) {
    return { dir, entries: [], cursor: 0, marked: new SvelteSet(), sort: "name", reverse: false, git: null, error: null };
  }

  async function init() {
    cfg = await invoke("get_config");
    applyTheme(cfg.theme, cfg.gui);
    showHidden = cfg.show_hidden;
    panels = [newPanel(cfg.start[0]), newPanel(cfg.start[1])];
    await Promise.all([load(0), load(1)]);
  }
  init().catch((e) => (status = String(e)));

  /** Re-read a panel, keeping the cursor on `focus` (default: the current name). */
  async function load(i, dir = panels[i].dir, focus) {
    const p = panels[i];
    const keep = focus ?? entry(p)?.name;
    try {
      const r = await invoke("list_dir", { dir, showHidden, sort: p.sort, reverse: p.reverse });
      if (r.dir !== p.dir) {
        p.marked.clear();
        p.git = null;
      }
      const alive = new Set(r.entries.map((e) => e.path));
      for (const m of [...p.marked]) if (!alive.has(m)) p.marked.delete(m);
      p.dir = r.dir;
      p.entries = r.entries;
      p.error = null;
      const at = keep ? r.entries.findIndex((e) => e.name === keep) : -1;
      p.cursor = at >= 0 ? at : Math.max(0, Math.min(p.cursor, r.entries.length - 1));
    } catch (e) {
      p.error = String(e);
      return;
    }
    invoke("git_status", { dir: p.dir }).then((g) => {
      if (panels[i].dir === dir) panels[i].git = g;
    });
  }

  async function cd(i, dir) {
    const from = panels[i].dir;
    panels[i].cursor = 0;
    await load(i, dir, parent(from) === dir ? basename(from) : undefined);
  }

  const reload = () => Promise.all([load(0), load(1)]);

  function targets(p = cur()) {
    if (p.marked.size) return p.entries.filter((e) => p.marked.has(e.path)).map((e) => e.path);
    const e = entry(p);
    return e && e.name !== ".." ? [e.path] : [];
  }

  const describe = (paths) => (paths.length === 1 ? `"${basename(paths[0])}"` : `${paths.length} items`);

  function move(delta) {
    const p = cur();
    p.cursor = Math.max(0, Math.min(p.entries.length - 1, p.cursor + delta));
  }

  function toggleMark(p, i) {
    const e = p.entries[i];
    if (!e || e.name === "..") return;
    p.marked.has(e.path) ? p.marked.delete(e.path) : p.marked.add(e.path);
  }

  async function openModal(m) {
    modal = m;
    await tick();
    modalInput?.focus();
    modalInput?.select?.();
  }

  function prompt(title, label, value, run) {
    openModal({ kind: "input", title, label, value, run });
  }

  async function op(promise, ok) {
    try {
      await promise;
      status = ok;
    } catch (e) {
      openModal({ kind: "message", title: "Error", text: String(e) });
    }
    panels.forEach((p) => p.marked.clear());
    await reload();
  }

  async function view(path) {
    try {
      const [text, truncated] = await invoke("read_text", { path, max: 4 << 20 });
      openModal({ kind: "view", title: path, text: truncated ? text + "\n… (first 4 MB)" : text });
    } catch (e) {
      status = String(e);
    }
  }

  async function runShell(command, dir) {
    status = `Running: ${command}`;
    try {
      lastOutput = await invoke("run_command", { cmd: command, dir });
    } catch (e) {
      lastOutput = String(e);
    }
    status = "";
    openModal({ kind: "view", title: `${dir}> ${command}`, text: lastOutput || "(no output)" });
    reload();
  }

  async function runCmdline() {
    const c = cmd.trim();
    cmd = "";
    if (!c) return;
    if (c === "cd" || c.startsWith("cd ")) {
      const arg = c.slice(2).trim().replace(/^["']|["']$/g, "") || "~";
      const dir = await invoke("resolve_path", { base: cur().dir, input: arg });
      await cd(active, dir);
      if (cur().error) status = `cd: ${cur().error}`;
      return;
    }
    runShell(c, cur().dir);
  }

  function open(i = cur().cursor) {
    const e = cur().entries[i];
    if (!e) return;
    if (e.is_dir) return cd(active, e.path);
    invoke("open_path", { path: e.path }).then(
      () => (status = `Opened ${e.name}`),
      (err) => (status = String(err)),
    );
  }

  const actions = {
    quit: () => getCurrentWindow().close(),
    up: () => move(-1),
    down: () => move(1),
    page_up: () => move(-pageSize()),
    page_down: () => move(pageSize()),
    home: () => (cur().cursor = 0),
    end: () => (cur().cursor = cur().entries.length - 1),
    open: () => open(),
    parent: () => {
      const up = cur().entries.find((e) => e.name === "..");
      if (up) cd(active, up.path);
    },
    switch_panel: () => (active ^= 1),
    mark: () => {
      toggleMark(cur(), cur().cursor);
      move(1);
    },
    select_group: () => selectGroup(true),
    unselect_group: () => selectGroup(false),
    invert_selection: () => {
      const p = cur();
      for (const e of p.entries) if (!e.is_dir) p.marked.has(e.path) ? p.marked.delete(e.path) : p.marked.add(e.path);
    },
    search: () => openModal({ kind: "search", query: "", scoped: false, res: null, cursor: 0 }),
    refresh: async () => {
      await reload();
      status = "Reread";
    },
    swap_panels: () => {
      panels = [panels[1], panels[0]];
      active ^= 1;
    },
    toggle_panels: () => openModal({ kind: "view", title: "Last output", text: lastOutput || "(nothing run yet)" }),
    toggle_hidden: () => {
      showHidden = !showHidden;
      reload();
    },
    goto_left: () => gotoPrompt(0),
    goto_right: () => gotoPrompt(1),
    same_dir: () => cd(active ^ 1, cur().dir),
    sort_name: () => sortBy("name"),
    sort_ext: () => sortBy("ext"),
    sort_time: () => sortBy("time"),
    sort_size: () => sortBy("size"),
    copy_path: () => {
      const e = entry();
      if (!e || e.name === "..") return;
      const q = /^[\w\-./+,:@]+$/.test(e.name) ? e.name : `'${e.name.replaceAll("'", "'\\''")}'`;
      cmd = (cmd && !cmd.endsWith(" ") ? cmd + " " : cmd) + q + " ";
      cmdInput?.focus();
    },
    view: () => {
      const e = entry();
      if (e && !e.is_dir) view(e.path);
    },
    edit: () => {
      const e = entry();
      if (e && !e.is_dir) invoke("edit_path", { path: e.path }).catch((err) => (status = String(err)));
    },
    copy: () => transfer(false),
    move: () => transfer(true),
    mkdir: () =>
      prompt("Make directory", "Create the directory:", "", async (name) => {
        if (!name.trim()) return;
        await op(invoke("mkdir", { base: cur().dir, name }), `Created ${name}`);
        await load(active, cur().dir, name.split(/[\\/]/)[0]);
      }),
    delete: () => {
      const paths = targets();
      if (!paths.length) return;
      const run = () => op(invoke("delete", { paths }), `Deleted ${describe(paths)}`);
      if (cfg.confirm_delete) openModal({ kind: "confirm", title: "Delete", text: `Do you wish to delete ${describe(paths)}?`, run });
      else run();
    },
    user_menu: () =>
      openModal({
        kind: "menu",
        title: "User menu",
        direct: true,
        filter: "",
        cursor: 0,
        items: cfg.user_menu.map((u, i) => ({ key: u.key, label: u.label, run: () => runUser(i) })),
      }),
    menu: () =>
      openModal({
        kind: "menu",
        title: "Commands",
        direct: false,
        filter: "",
        cursor: 0,
        items: Object.entries(cfg.actions)
          .filter(([name]) => !["menu", "up", "down"].includes(name))
          .map(([name, [label, key]]) => ({ key, label, run: () => actions[name]() })),
      }),
    help: () => openModal({ kind: "help" }),
  };

  function pageSize() {
    const rows = document.querySelector(".panel.active .rows");
    const row = rows?.querySelector(".row");
    return row ? Math.max(1, Math.floor(rows.clientHeight / row.offsetHeight) - 1) : 20;
  }

  function sortBy(k) {
    const p = cur();
    p.reverse = p.sort === k && !p.reverse;
    p.sort = k;
    load(active);
  }

  function gotoPrompt(side) {
    prompt(side ? "Right panel" : "Left panel", "Go to directory:", panels[side].dir, async (v) => {
      const dir = await invoke("resolve_path", { base: cur().dir, input: v });
      await cd(side, dir);
    });
  }

  function selectGroup(sel) {
    prompt(sel ? "Select" : "Unselect", "Files matching:", "*", (v) => {
      const pats = v.split(/[\s;,]+/).filter(Boolean);
      const p = cur();
      for (const e of p.entries)
        if (!e.is_dir && pats.some((g) => glob(g, e.name))) sel ? p.marked.add(e.path) : p.marked.delete(e.path);
    });
  }

  function transfer(isMove, dest = other().dir) {
    const paths = targets();
    if (!paths.length) return;
    const verb = isMove ? "Move" : "Copy";
    prompt(isMove ? "Rename/Move" : "Copy", `${verb} ${describe(paths)} to:`, dest, (d) => {
      if (!d.trim()) return;
      op(invoke(isMove ? "rename" : "copy", { paths, base: cur().dir, dest: d }), `${isMove ? "Moved" : "Copied"} ${describe(paths)}`);
    });
  }

  function runUser(i) {
    const p = cur();
    const e = entry(p);
    const file = e && e.name !== ".." ? e.path : null;
    const selected = p.entries.filter((x) => p.marked.has(x.path)).map((x) => x.path);
    const u = cfg.user_menu[i];
    status = `Running: ${u.label}`;
    invoke("run_user", { index: i, dir: p.dir, file, selected }).then(
      (out) => {
        lastOutput = out;
        status = "";
        openModal({ kind: "view", title: u.label, text: out || "(no output)" });
        reload();
      },
      (err) => (status = String(err)),
    );
  }

  // ------------------------------------------------------------ search

  let searchSeq = 0;
  async function runSearch() {
    const m = modal;
    if (m?.kind !== "search") return;
    const seq = ++searchSeq;
    const res = await invoke("search", { query: m.query, scope: m.scoped ? cur().dir : null });
    if (seq === searchSeq && modal === m) {
      m.res = res;
      m.cursor = 0;
    }
  }

  // While the index builds, keep results fresh.
  $effect(() => {
    if (modal?.kind !== "search") return;
    const t = setInterval(() => modal?.res?.state !== "ready" && runSearch(), 700);
    return () => clearInterval(t);
  });

  async function goToHit(h) {
    modal = null;
    const dir = parent(h.path);
    if (dir) await cd(active, dir);
    const i = cur().entries.findIndex((e) => e.path === h.path || e.name === basename(h.path));
    if (i >= 0) cur().cursor = i;
  }

  // Keep the highlighted search hit / menu item in view.
  $effect(() => {
    if (modal && "cursor" in modal) {
      modal.cursor;
      tick().then(() => document.querySelector(".dialog li.cursor")?.scrollIntoView({ block: "nearest" }));
    }
  });

  // ------------------------------------------------------------ keys

  function menuItems(m) {
    const f = m.filter.toLowerCase();
    return m.items.filter((it) => it.label.toLowerCase().includes(f));
  }

  function modalKey(e, k) {
    const m = modal;
    const act = cfg.keymap[k];
    if (k === "Esc" || (act === "quit" && m.kind !== "input")) {
      modal = null;
      return true;
    }
    switch (m.kind) {
      case "input":
        if (k === "Enter") {
          modal = null;
          m.run(m.value);
          return true;
        }
        return false;
      case "confirm":
        if (k === "Enter" || k === "y" || k === "Shift+Y") {
          modal = null;
          m.run();
        } else if (k === "n" || k === "Shift+N") modal = null;
        return true;
      case "search": {
        const hits = m.res?.hits ?? [];
        const h = hits[m.cursor];
        if (k === "Enter" && h) goToHit(h);
        else if (k === "Tab") {
          m.scoped = !m.scoped;
          runSearch();
        } else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(hits.length - 1, m.cursor + 1);
        else if (k === "PageUp") m.cursor = Math.max(0, m.cursor - 15);
        else if (k === "PageDown") m.cursor = Math.min(hits.length - 1, m.cursor + 15);
        else if (act === "view" && h && !h.is_dir) view(h.path);
        else if (act === "edit" && h && !h.is_dir) invoke("edit_path", { path: h.path });
        else return false;
        return true;
      }
      case "menu": {
        const items = menuItems(m);
        if (k === "Enter" && items[m.cursor]) {
          modal = null;
          items[m.cursor].run();
        } else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(items.length - 1, m.cursor + 1);
        else if (m.direct && [...k].length === 1 && m.items.some((it) => it.key === k)) {
          modal = null;
          m.items.find((it) => it.key === k).run();
        } else return false;
        return true;
      }
      default:
        // view, help, message: F-keys close too, arrows scroll natively.
        if (k === "Enter" || act === "help" || act === "view") modal = null;
        return m.kind === "message";
    }
  }

  function onkeydown(e) {
    if (!cfg) return;
    const k = keyString(e);
    if (!k) return;
    if (modal) {
      if (modalKey(e, k)) e.preventDefault();
      return;
    }
    status = "";
    const plain = [...e.key].length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey;

    // Quick search: Alt+letter starts it, letters extend it.
    if (quick !== null) {
      if (plain || (e.altKey && /^Key[A-Z]$|^Digit/.test(e.code))) {
        quick += plain ? e.key : k.split("+").pop().toLowerCase();
        return quickJump(e);
      }
      if (k === "Backspace") {
        quick = quick.slice(0, -1);
        return e.preventDefault();
      }
      const esc = k === "Esc";
      quick = null;
      if (esc) return e.preventDefault();
    }
    const act = cfg.keymap[k];
    if (!act && e.altKey && !e.ctrlKey && /^Key[A-Z]$|^Digit/.test(e.code)) {
      quick = k.split("+").pop().toLowerCase();
      return quickJump(e);
    }
    // While the command line has text, editing keys belong to it.
    if (cmd && (plain || ["Enter", "Backspace", "Delete", "Left", "Right", "Home", "End", "Esc"].includes(k))) {
      if (k === "Enter") runCmdline();
      else if (k === "Esc") cmd = "";
      else {
        cmdInput.focus();
        return;
      }
      return e.preventDefault();
    }
    if (act) {
      e.preventDefault();
      actions[act]?.();
    } else if (plain) {
      cmdInput.focus(); // the character lands in the command line
    }
  }

  function quickJump(e) {
    e.preventDefault();
    const p = cur();
    const i = p.entries.findIndex((x) => x.name.toLowerCase().startsWith(quick.toLowerCase()));
    if (i >= 0) p.cursor = i;
  }

  const fkeys = $derived.by(() => {
    if (!cfg) return [];
    return Array.from({ length: 10 }, (_, i) => {
      const act = cfg.keymap[`F${i + 1}`];
      return { n: i + 1, act, label: act ? cfg.actions[act][0] : "" };
    });
  });
</script>

<svelte:window {onkeydown} />

{#if cfg && panels.length}
  <main>
    <div class="panels">
      {#each panels as p, i (i)}
        <Panel
          panel={p}
          active={i === active && !modal}
          glyphs={cfg.glyphs}
          onpick={(row) => {
            active = i;
            p.cursor = row;
          }}
          onopen={(row) => {
            active = i;
            open(row);
          }}
          onmark={(row, range) => {
            if (!range) return toggleMark(p, row);
            const from = Math.min(p.cursor, row);
            for (let r = from; r <= Math.max(p.cursor, row); r++) if (p.entries[r].name !== "..") p.marked.add(p.entries[r].path);
          }}
          onsort={(k) => {
            active = i;
            sortBy(k);
          }}
          ongoto={() => gotoPrompt(i)}
          ondropfiles={(isMove) => {
            if (i === active) return;
            transfer(isMove, p.dir);
          }}
        />
      {/each}
    </div>
    <label class="cmdline">
      <span class="prompt">{quick !== null ? `Quick search: ${quick}` : status || `${cur().dir}>`}</span>
      <input bind:this={cmdInput} bind:value={cmd} spellcheck="false" autocomplete="off" aria-label="Command line" />
    </label>
    <nav class="keybar">
      {#each fkeys as f}
        <button disabled={!f.act} onclick={() => f.act && actions[f.act]()}><b>{f.n}</b><span>{f.label}</span></button>
      {/each}
    </nav>
  </main>
{:else}
  <main class="loading">{status || "Loading…"}</main>
{/if}

{#if modal}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (modal = null)}>
    <div class="dialog {modal.kind}" role="dialog" aria-modal="true" aria-label={modal.title ?? modal.kind}>
      {#if modal.kind === "input"}
        <h2>{modal.title}</h2>
        <p>{modal.label}</p>
        <input bind:this={modalInput} bind:value={modal.value} spellcheck="false" />
        <p class="hint">Enter = OK · Esc = Cancel</p>
      {:else if modal.kind === "confirm"}
        <h2>{modal.title}</h2>
        <p class="center">{modal.text}</p>
        <div class="buttons">
          <button onclick={() => { const r = modal.run; modal = null; r(); }}>Yes (Enter)</button>
          <button onclick={() => (modal = null)}>No (Esc)</button>
        </div>
      {:else if modal.kind === "message"}
        <h2>{modal.title}</h2>
        <pre>{modal.text}</pre>
      {:else if modal.kind === "view"}
        <h2 title={modal.title}>{modal.title}</h2>
        <pre class="viewer" tabindex="-1" bind:this={modalInput}>{modal.text}</pre>
      {:else if modal.kind === "help"}
        <h2>Help</h2>
        <div class="help" tabindex="-1" bind:this={modalInput}>
          <p><b>Coxswain</b> — the ship's officer who gets the work done.</p>
          <table>
            <tbody>
              {#each Object.entries(cfg.actions) as [name, [label]]}
                <tr><td>{label}</td><td>{Object.entries(cfg.keymap).filter(([, a]) => a === name).map(([k]) => k).join(", ")}</td></tr>
              {/each}
            </tbody>
          </table>
          <p>Mouse: click selects, double-click opens, right/Ctrl-click marks, Shift-click marks a range, drag rows to the other panel to copy (Shift = move), click a column title to sort, click the path to go elsewhere.</p>
          <p>Alt+letter quick search · typing goes to the command line · Enter runs it in the panel's directory.</p>
          <p><b>Find file</b> (Everything syntax): <code>foo bar</code> both · <code>foo|bar</code> either · <code>!foo</code> not · <code>*.rs</code> wildcards · <code>ext:rs;toml</code> · <code>file:</code> <code>folder:</code> · <code>src/ foo</code> path contains · <code>case:</code> · <code>"a b"</code> phrase.</p>
          <p>Config: <code>{cfg.config_path}</code> — run <code>coxswain --dump-config</code> for every option.</p>
        </div>
      {:else if modal.kind === "menu"}
        <h2>{modal.title}</h2>
        {#if !modal.direct}
          <input bind:this={modalInput} bind:value={modal.filter} oninput={() => (modal.cursor = 0)} placeholder="Type to filter" spellcheck="false" />
        {/if}
        <ul class="menu">
          {#each menuItems(modal) as it, i}
            <li class:cursor={i === modal.cursor}>
              <button onclick={() => { modal = null; it.run(); }} onmouseenter={() => (modal.cursor = i)}>
                <span>{it.label}</span><kbd>{it.key}</kbd>
              </button>
            </li>
          {/each}
        </ul>
      {:else if modal.kind === "search"}
        <h2>Find file</h2>
        <div class="search-bar">
          <button class="scope" title="Tab toggles" onclick={() => { modal.scoped = !modal.scoped; runSearch(); modalInput.focus(); }}>
            {modal.scoped ? `in ${basename(cur().dir)}` : "everywhere"}
          </button>
          <input bind:this={modalInput} bind:value={modal.query} oninput={runSearch} placeholder="name, *.rs, ext:md, src/ foo, !test…" spellcheck="false" />
        </div>
        <p class="hint">
          {#if modal.res}
            {modal.query ? `${modal.res.total.toLocaleString()} matches in ${(modal.res.micros / 1000).toFixed(2)} ms · ` : ""}{modal.res.indexed.toLocaleString()} files indexed{modal.res.state === "building" ? " · building index…" : modal.res.state === "stale" ? " · refreshing index" : ""}
          {:else}
            Type to search every file name on this machine.
          {/if}
        </p>
        <ul class="hits">
          {#each modal.res?.hits ?? [] as h, i (h.path)}
            {@const name = basename(h.path)}
            <li class:cursor={i === modal.cursor}>
              <button onclick={() => (modal.cursor = i)} ondblclick={() => goToHit(h)}>
                <b>{name}{h.is_dir ? "/" : ""}</b> <span><bdi>{parent(h.path)}</bdi></span>
              </button>
            </li>
          {/each}
        </ul>
        <p class="hint">Enter go to · Tab everywhere/here · F3 view · F4 edit · Esc close</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--panel-bg, #0000aa);
    font-family: var(--font, monospace);
    font-size: var(--font-size, 14px);
    overflow: hidden;
  }
  :global(#app) {
    height: 100%;
  }
  main {
    display: grid;
    grid-template-rows: 1fr auto auto;
    height: 100%;
  }
  .loading {
    place-items: center;
    color: var(--panel-fg, #55ffff);
  }
  .panels {
    display: grid;
    grid-template-columns: 1fr 1fr;
    min-height: 0;
  }
  .cmdline {
    display: flex;
    gap: 1ch;
    background: var(--cmdline-bg);
    color: var(--cmdline-fg);
    padding: 0 0.5ch;
    line-height: var(--row);
  }
  .prompt {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 50%;
  }
  .cmdline input {
    flex: 1;
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
    outline: none;
  }
  .keybar {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    background: var(--keybar-num-bg);
  }
  .keybar button {
    display: flex;
    font: inherit;
    border: 0;
    padding: 0;
    background: none;
    cursor: pointer;
    text-align: left;
    line-height: var(--row);
  }
  .keybar b {
    color: var(--keybar-num-fg);
    padding: 0 0.3ch 0 0.6ch;
    font-weight: normal;
  }
  .keybar span {
    flex: 1;
    background: var(--keybar-label-bg);
    color: var(--keybar-label-fg);
    padding-left: 0.3ch;
    overflow: hidden;
    white-space: nowrap;
  }
  .keybar button:hover span {
    filter: brightness(1.2);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.35);
  }
  .dialog {
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 4px double var(--dialog-border-fg);
    box-shadow: 1ch 1ch 0 rgb(0 0 0 / 0.5);
    padding: 0.5em 1.5ch 1em;
    min-width: min(60ch, 90vw);
    max-width: 90vw;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    gap: 0.4em;
  }
  .dialog.search,
  .dialog.view,
  .dialog.help {
    width: 90vw;
    height: 85vh;
  }
  h2 {
    font-size: inherit;
    text-align: center;
    margin: 0 0 0.3em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--dialog-border-fg);
  }
  p {
    margin: 0;
  }
  .center {
    text-align: center;
  }
  .hint {
    opacity: 0.8;
    text-align: center;
  }
  .dialog input {
    font: inherit;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 0;
    padding: 0.2em 0.5ch;
    outline: none;
  }
  .buttons {
    display: flex;
    justify-content: center;
    gap: 2ch;
  }
  .buttons button {
    font: inherit;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 0;
    padding: 0.2em 1.5ch;
    cursor: pointer;
  }
  pre {
    margin: 0;
    overflow: auto;
    flex: 1;
    font: inherit;
    white-space: pre-wrap;
    outline: none;
  }
  .viewer {
    white-space: pre;
  }
  .help {
    overflow: auto;
    outline: none;
  }
  .help td {
    padding: 0 2ch 0 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    outline: none;
  }
  .hits {
    flex: 1;
  }
  li button {
    display: flex;
    gap: 2ch;
    width: 100%;
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0 0.5ch;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
  }
  .menu li button span {
    flex: 1;
  }
  .hits b {
    color: var(--search-hit-fg);
  }
  .hits span {
    opacity: 0.75;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  li.cursor button {
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
  }
  .search-bar {
    display: flex;
    gap: 1ch;
  }
  .search-bar input {
    flex: 1;
  }
  .scope {
    font: inherit;
    border: 1px solid var(--dialog-border-fg);
    background: none;
    color: inherit;
    cursor: pointer;
  }
  kbd {
    font: inherit;
    opacity: 0.8;
  }
</style>

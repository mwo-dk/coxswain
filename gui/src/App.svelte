<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { listen } from "@tauri-apps/api/event";
  import { ui, init, tab, pane, otherTab, item, load, cd, newTab, goBack, goForward, openItem, toggleMark, targets, reloadAll, snapshot, setTheme, nextView, measureFolders, columnMenu } from "./app.svelte.js";
  import { invoke, keyString, basename, parent, glob, quote, isArchive } from "./lib.js";
  import Sidebar from "./Sidebar.svelte";
  import Pane from "./Pane.svelte";
  import Preview from "./Preview.svelte";
  import Splitter from "./Splitter.svelte";
  import Dialogs from "./Dialogs.svelte";

  let dialogs = $state();
  let panes = $state([]);
  let cmdInput = $state();
  let main = $state();
  /** Command/script output shown in the preview pane. */
  let output = $state(null);
  let notesFocus = $state(0);
  let ready = $state(false);

  let update = $state(null);
  init().then(
    () => {
      ready = true;
      invoke("check_update").then((v) => (update = v), () => {});
    },
    (e) => (ui.status = String(e)),
  );

  // Save the session a moment after anything in it changes.
  $effect(() => {
    if (!ready) return;
    const snap = JSON.stringify(snapshot());
    const timer = setTimeout(() => invoke("save_session", { session: JSON.parse(snap) }), 600);
    return () => clearTimeout(timer);
  });

  // Folders open in any tab reread themselves when something changes in them.
  const allTabs = () => ui.panes.flatMap((p) => p.tabs);
  $effect(() => {
    if (!ready) return;
    invoke("watch_dirs", { dirs: [...new Set(allTabs().map((t) => t.dir))] });
  });
  listen("dir-changed", (ev) => {
    for (const t of allTabs()) if (ev.payload.includes(t.dir)) load(t);
  });

  // Files dragged in from other applications, or from a pane (the drag is native, see dragOut).
  const paneAt = ({ x, y }) => {
    const el = document.elementFromPoint(x / devicePixelRatio, y / devicePixelRatio)?.closest("[data-pane]");
    return el ? Number(el.dataset.pane) : null;
  };
  getCurrentWebview().onDragDropEvent(({ payload: p }) => {
    if (p.type === "leave") return void (ui.dropPane = null);
    const at = p.position ? paneAt(p.position) : null;
    if (p.type !== "drop") return void (ui.dropPane = at);
    ui.dropPane = null;
    if (at === null || !p.paths.length) return;
    const dest = tab(at).dir;
    if (p.paths.every((x) => parent(x) === dest)) return; // dropped where it came from
    const n = describe(p.paths);
    const go = (isMove) => op(invoke(isMove ? "rename" : "copy", { paths: p.paths, base: dest, dest }), `${isMove ? "Moved" : "Copied"} ${n}`);
    ui.modal = {
      kind: "menu",
      title: `Drop ${n} in ${basename(dest) || dest}`,
      direct: true,
      filter: "",
      cursor: 0,
      items: [
        { key: "c", label: "Copy here", icon: "\u{f0c5}", run: () => go(false) },
        { key: "m", label: "Move here", icon: "\u{f0b2}", run: () => go(true) },
      ],
    };
  });

  const describe = (paths) => (paths.length === 1 ? `"${basename(paths[0])}"` : `${paths.length} items`);
  const move = (d) => {
    const t = tab();
    t.cursor = Math.max(0, Math.min(t.items.length - 1, t.cursor + d));
  };
  const pageSize = () => {
    const rows = document.querySelector(".pane.active .rows, .pane.active .col.current");
    const row = rows?.querySelector('[role="option"]');
    return row ? Math.max(1, Math.floor(rows.clientHeight / row.offsetHeight) - 1) : 20;
  };

  function prompt(title, label, value, run) {
    ui.modal = { kind: "input", title, label, value, run };
  }

  async function op(promise, ok) {
    try {
      await promise;
      ui.status = ok;
    } catch (e) {
      ui.modal = { kind: "message", title: "Something went wrong", text: String(e) };
    }
    for (const p of ui.panes) for (const t of p.tabs) t.marked.clear();
    await reloadAll();
  }

  function transfer(isMove, dest = otherTab().dir) {
    const paths = targets();
    if (!paths.length) return;
    prompt(isMove ? "Move or rename" : "Copy", `${isMove ? "Move" : "Copy"} ${describe(paths)} to:`, dest, (d) => {
      if (d.trim()) op(invoke(isMove ? "rename" : "copy", { paths, base: tab().dir, dest: d }), `${isMove ? "Moved" : "Copied"} ${describe(paths)}`);
    });
  }

  function remove(forever) {
    const paths = targets();
    if (!paths.length) return;
    const run = () => op(invoke("delete", { paths, forever }), `${forever ? "Deleted" : "Moved to the trash:"} ${describe(paths)}`);
    const text = forever ? `Permanently delete ${describe(paths)}? This cannot be undone.` : `Move ${describe(paths)} to the trash?`;
    if (ui.cfg.confirm_delete) ui.modal = { kind: "confirm", title: "Delete", text, ok: forever ? "Delete" : "Move to trash", run };
    else run();
  }

  async function clip(cut) {
    const paths = targets();
    if (!paths.length) return;
    try {
      await invoke("clip_set", { paths, cut });
      ui.status = `${cut ? "Cut" : "Copied"} ${describe(paths)} to the clipboard`;
    } catch (e) {
      ui.status = String(e);
    }
  }

  function showOutput(title, text) {
    ui.lastOutput = text;
    output = { title, text };
    ui.showPreview = true;
  }

  async function runShell(command, dir) {
    ui.status = `Running ${command}…`;
    const text = await invoke("run_command", { cmd: command, dir }).catch(String);
    ui.status = "";
    showOutput(command, text);
    reloadAll();
  }

  async function runCmdline() {
    const c = ui.cmd.trim();
    ui.cmd = "";
    if (!c) return;
    if (c === "cd" || c.startsWith("cd ")) {
      const arg = c.slice(2).trim().replace(/^["']|["']$/g, "") || "~";
      return cd(tab(), await invoke("resolve_path", { base: tab().dir, input: arg }));
    }
    runShell(c, tab().dir);
  }

  function selectGroup(sel) {
    prompt(sel ? "Select files" : "Unselect files", "Matching:", "*", (v) => {
      const pats = v.split(/[\s;,]+/).filter(Boolean);
      const t = tab();
      for (const e of t.items) if (!e.is_dir && pats.some((g) => glob(g, e.name))) sel ? t.marked.add(e.path) : t.marked.delete(e.path);
    });
  }

  function addTab() {
    const p = pane();
    p.tabs.push(newTab(tab().dir, tab().view));
    p.active = p.tabs.length - 1;
    load(tab());
  }

  function cycleTab(d) {
    const p = pane();
    p.active = (p.active + d + p.tabs.length) % p.tabs.length;
  }

  const THEMES = [
    ["dark", "Dark"],
    ["light", "Light"],
    ["nord", "Nord"],
    ["midnight", "Tokyo Night"],
    ["nc", "Classic blue (NC)"],
  ];

  function sortBy(k) {
    const t = tab();
    t.reverse = t.sort === k && !t.reverse;
    t.sort = k;
    load(t);
  }

  function runScript(s) {
    const t = tab();
    const e = item(t);
    const file = e && e.name !== ".." ? e.path : null;
    const selected = t.items.filter((x) => t.marked.has(x.path)).map((x) => x.path);
    ui.status = `Running ${s.label}…`;
    invoke("run_script", { user: s.user, path: s.path, dir: t.dir, file, selected }).then(
      (text) => {
        ui.status = "";
        showOutput(s.label, text);
        reloadAll();
      },
      (err) => (ui.status = String(err)),
    );
  }

  const actions = {
    quit: () => getCurrentWindow().close(),
    up: () => move(-1),
    down: () => move(1),
    page_up: () => move(-pageSize()),
    page_down: () => move(pageSize()),
    home: () => (tab().cursor = 0),
    end: () => (tab().cursor = tab().items.length - 1),
    open: () => openItem(tab()),
    parent: () => parent(tab().dir) && cd(tab(), parent(tab().dir)),
    switch_panel: () => ui.dual && (ui.activePane ^= 1),
    mark: () => {
      toggleMark(tab(), tab().cursor);
      move(1);
    },
    select_group: () => selectGroup(true),
    unselect_group: () => selectGroup(false),
    invert_selection: () => {
      const t = tab();
      for (const e of t.items) if (!e.is_dir) t.marked.has(e.path) ? t.marked.delete(e.path) : t.marked.add(e.path);
    },
    search: () => (ui.modal = { kind: "search", query: "", scoped: false, res: null, cursor: 0 }),
    refresh: async () => {
      await reloadAll();
      ui.status = "Reread";
    },
    swap_panels: () => {
      ui.panes = [ui.panes[1], ui.panes[0]];
      ui.activePane ^= 1;
    },
    // Ctrl+O hid NC's panels; here it switches between one and two panes.
    toggle_panels: () => (ui.dual = !ui.dual),
    toggle_hidden: () => {
      ui.showHidden = !ui.showHidden;
      reloadAll();
    },
    goto_left: () => panes[0]?.editPath(),
    goto_right: () => (ui.dual ? panes[1] : panes[0])?.editPath(),
    same_dir: () => ui.dual && cd(tab(ui.activePane ^ 1), tab().dir),
    sort_name: () => sortBy("name"),
    sort_ext: () => sortBy("ext"),
    sort_time: () => sortBy("time"),
    sort_size: () => sortBy("size"),
    copy_path: () => {
      const e = item();
      if (!e || e.name === "..") return;
      ui.cmd = (ui.cmd && !ui.cmd.endsWith(" ") ? ui.cmd + " " : ui.cmd) + quote(e.name) + " ";
      cmdInput?.focus();
    },
    // F3 toggles the preview, as NC's F3 toggled its viewer; Esc closes it too.
    view: () => {
      ui.showPreview = !(ui.showPreview && !output);
      output = null;
    },
    edit: () => {
      const e = item();
      if (e && !e.is_dir) invoke("edit_path", { path: e.path }).catch((err) => (ui.status = String(err)));
    },
    copy: () => transfer(false),
    move: () => transfer(true),
    mkdir: () =>
      prompt("New folder", "Name:", "", async (name) => {
        if (!name.trim()) return;
        await op(invoke("mkdir", { base: tab().dir, name }), `Created ${name}`);
        await load(tab(), tab().dir, name.split(/[\\/]/)[0]);
      }),
    delete: () => remove(false),
    delete_forever: () => remove(true),
    clip_copy: () => clip(false),
    clip_cut: () => clip(true),
    paste: async () => {
      const dir = tab().dir;
      try {
        const [n, moved] = await invoke("paste", { dir });
        ui.status = `${moved ? "Moved" : "Pasted"} ${n} item${n > 1 ? "s" : ""}`;
      } catch (e) {
        ui.modal = { kind: "message", title: "Paste", text: String(e) };
      }
      reloadAll();
    },
    properties: async () => {
      const e = item();
      if (!e || e.name === "..") return;
      ui.status = "Reading properties…";
      try {
        const p = await invoke("properties", { path: e.path });
        ui.modal = { kind: "props", props: p, mode: p.mode?.toString(8).padStart(3, "0") ?? "", readonly: p.readonly };
      } catch (err) {
        ui.modal = { kind: "message", title: "Properties", text: String(err) };
      }
      ui.status = "";
    },
    extract: () => {
      const paths = targets().filter((p) => isArchive(basename(p)));
      if (!paths.length) return void (ui.status = "Not a zip or tar archive");
      prompt("Extract", `Extract ${describe(paths)} into a new folder in:`, otherTab().dir, (d) => {
        if (d.trim()) op(invoke("extract", { paths, base: tab().dir, dest: d }), `Extracted ${describe(paths)}`);
      });
    },
    user_menu: async () => {
      const list = await invoke("scripts");
      ui.modal = {
        kind: "menu",
        title: "Scripts",
        direct: true,
        filter: "",
        cursor: 0,
        items: list.map((s) => ({ key: s.key, label: s.label, icon: s.path ? "\u{f489}" : "\u{f120}", run: () => runScript(s) })),
      };
    },
    menu: () =>
      (ui.modal = {
        kind: "menu",
        title: "Commands",
        direct: false,
        filter: "",
        cursor: 0,
        items: [
          ...Object.entries(ui.cfg.actions)
            .filter(([n]) => !["menu", "up", "down"].includes(n))
            .map(([n, [label, key]]) => ({ key, label, run: () => actions[n]?.() })),
          ...THEMES.map(([id, label]) => ({ key: id === ui.theme ? "current" : "", label: `Theme: ${label}`, icon: "\u{f53f}", run: () => setTheme(id) })),
        ],
      }),
    help: () => (ui.modal = { kind: "help" }),
    new_tab: addTab,
    close_tab: () => {
      const p = pane();
      if (p.tabs.length > 1) {
        p.tabs.splice(p.active, 1);
        p.active = Math.min(p.active, p.tabs.length - 1);
      }
    },
    next_tab: () => cycleTab(1),
    prev_tab: () => cycleTab(-1),
    toggle_preview: () => {
      output = null;
      ui.showPreview = !ui.showPreview;
    },
    toggle_view: () => (tab().view = nextView(tab().view)),
    toggle_sidebar: () => (ui.showSidebar = !ui.showSidebar),
    edit_path: () => panes[ui.dual ? ui.activePane : 0]?.editPath(),
    dir_sizes: async () => {
      const t = tab();
      const dirs = t.marked.size ? targets(t) : t.items.filter((e) => e.is_dir && e.name !== "..").map((e) => e.path);
      if (!dirs.length) return;
      ui.status = `Measuring ${dirs.length} folder${dirs.length > 1 ? "s" : ""}…`;
      await measureFolders(t, dirs);
      ui.status = "";
    },
    batch_rename: () => {
      const t = tab();
      const names = (t.marked.size ? t.items.filter((e) => t.marked.has(e.path)) : t.items.filter((e) => e.name !== "..")).map((e) => e.name);
      if (!names.length) return;
      ui.modal = { kind: "rename", dir: t.dir, names, pattern: "", replacement: "", ci: false, global: false, whole: false, plan: [], error: "" };
      dialogs.replan();
    },
    tag: () => {
      const paths = targets();
      if (paths.length) ui.modal = { kind: "tag", paths };
    },
    notes: () => {
      output = null;
      ui.showPreview = true;
      notesFocus++;
    },
    columns: () => columnMenu(),
    back: () => goBack(),
    forward: () => goForward(),
  };

  // ------------------------------------------------------------ keys

  function onkeydown(e) {
    if (!ready) return;
    const k = keyString(e);
    if (!k) return;
    if (ui.modal) {
      if (dialogs.handleKey(e, k)) e.preventDefault();
      return;
    }
    // Typing in notes, the path bar or another field: leave it alone, except that Esc
    // leaves the field and function keys keep working (otherwise F3/F8 seem dead).
    const field = e.target.closest?.("textarea, input:not(.cmd)");
    // Copy/cut/paste of text in the command line stays native.
    if (["clip_copy", "clip_cut", "paste"].includes(ui.cfg.keymap[k]) && e.target.closest?.(".cmd") && ui.cmd) return;
    if (field && k === "Esc") return field.blur();
    if (field && !/^F\d+$/.test(k)) return;
    ui.status = "";
    const plain = [...e.key].length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey;

    // Quick search: Alt+letter starts it, letters extend it.
    if (ui.quick !== null) {
      if (plain || (e.altKey && /^Key[A-Z]$|^Digit/.test(e.code))) {
        ui.quick += plain ? e.key : k.split("+").pop().toLowerCase();
        return quickJump(e);
      }
      if (k === "Backspace") {
        ui.quick = ui.quick.slice(0, -1);
        return e.preventDefault();
      }
      const esc = k === "Esc";
      ui.quick = null;
      if (esc) return e.preventDefault();
    }
    const act = ui.cfg.keymap[k];
    if (k === "Esc" && !ui.cmd && ui.showPreview) {
      e.preventDefault();
      output = null;
      ui.showPreview = false;
      return;
    }
    if (!act && e.altKey && !e.ctrlKey && /^Key[A-Z]$|^Digit/.test(e.code)) {
      ui.quick = k.split("+").pop().toLowerCase();
      return quickJump(e);
    }
    // While the command line has text, editing keys belong to it.
    if (ui.cmd && (plain || ["Enter", "Backspace", "Delete", "Left", "Right", "Home", "End", "Esc", "Space"].includes(k))) {
      if (k === "Enter") runCmdline();
      else if (k === "Esc") ui.cmd = "";
      else return cmdInput.focus();
      return e.preventDefault();
    }
    // Thumbnails: arrows move in two dimensions.
    if (tab().view === "grid" && ["Left", "Right", "Up", "Down"].includes(k)) {
      e.preventDefault();
      const cols = Number(document.querySelector(`[data-pane="${ui.activePane}"] .grid`)?.dataset.cols) || 1;
      return move({ Left: -1, Right: 1, Up: -cols, Down: cols }[k]);
    }
    // Miller columns: Left/Right walk the tree.
    if (tab().view === "columns" && (k === "Left" || k === "Right")) {
      e.preventDefault();
      if (k === "Left") return actions.parent();
      const it = item();
      return it?.is_dir && it.name !== ".." && openItem(tab());
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
    const t = tab();
    const i = t.items.findIndex((x) => x.name.toLowerCase().startsWith(ui.quick.toLowerCase()));
    if (i >= 0) t.cursor = i;
  }

  const fkeys = $derived.by(() => {
    if (!ui.cfg) return [];
    return Array.from({ length: 10 }, (_, i) => {
      const act = ui.cfg.keymap[`F${i + 1}`];
      return { n: i + 1, act, label: act ? ui.cfg.actions[act][0] : "" };
    });
  });

  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
</script>

<svelte:window {onkeydown} />

{#if ready}
  <div class="app" bind:this={main}>
    <div class="work">
      {#if ui.showSidebar}
        <div class="side" style:width="min({ui.sidebarW}px, 24vw)"><Sidebar /></div>
        <Splitter onmove={(dx) => (ui.sidebarW = clamp(ui.sidebarW + dx, 150, 480))} />
      {/if}
      <div class="panes">
        {#if ui.dual}
          <div class="pane-slot" style:flex-basis="{ui.split}%"><Pane bind:this={panes[0]} index={0} /></div>
          <Splitter onmove={(dx) => (ui.split = clamp(ui.split + (dx / main.clientWidth) * 100, 20, 80))} />
          <div class="pane-slot" style:flex-basis="{100 - ui.split}%"><Pane bind:this={panes[1]} index={1} /></div>
        {:else}
          <div class="pane-slot" style:flex-basis="100%"><Pane bind:this={panes[0]} index={ui.activePane} /></div>
        {/if}
      </div>
      {#if ui.showPreview}
        <Splitter onmove={(dx) => (ui.previewW = clamp(ui.previewW - dx, 240, 900))} />
        <div class="side" style:width="min({ui.previewW}px, 60vw)">
          <Preview {output} {notesFocus} onclearoutput={() => (output = null)} />
        </div>
      {/if}
    </div>

    <label class="cmdline">
      <span class="prompt">{ui.quick !== null ? `Quick search: ${ui.quick}` : `${tab().dir} ❯`}</span>
      <input class="cmd" bind:this={cmdInput} bind:value={ui.cmd} spellcheck="false" autocomplete="off" placeholder="Type a command…" aria-label="Command line" />
      {#if ui.status}<span class="status">{ui.status}</span>{/if}
      {#if update}
        <!-- With a package manager, the command upgrades; the page still has the release notes. -->
        <button class="update" title={update[1] ? `Upgrade with: ${update[1]}. Click for the release notes.` : "Open the releases page"}
          onclick={() => invoke("open_path", { path: "https://github.com/mwo-dk/coxswain/releases/latest" })}>
          Coxswain {update[0]} is available{update[1] ? `: ${update[1]}` : ""}
        </button>
      {/if}
    </label>
    <nav class="keybar" aria-label="Function keys">
      {#each fkeys as f (f.n)}
        <button disabled={!f.act} onclick={() => f.act && actions[f.act]()}><kbd>F{f.n}</kbd><span>{f.label}</span></button>
      {/each}
    </nav>
  </div>
{:else}
  <div class="loading">{ui.status || "Loading…"}</div>
{/if}

<Dialogs bind:this={dialogs} />

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: var(--sidebar-bg, #18191b);
    color: var(--panel-fg, #ddd);
    font-family: var(--font, system-ui, sans-serif);
    font-size: var(--font-size, 13px);
    -webkit-font-smoothing: antialiased;
  }
  :global(#app) {
    height: 100%;
  }
  :global(*::-webkit-scrollbar) {
    width: 10px;
    height: 10px;
  }
  :global(*::-webkit-scrollbar-thumb) {
    background: color-mix(in srgb, var(--hidden-fg) 40%, transparent);
    border-radius: 5px;
    border: 2px solid transparent;
    background-clip: content-box;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .work {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
    padding: 6px 6px 0;
  }
  .side {
    flex: none;
    min-height: 0;
    border-radius: 8px;
    overflow: hidden;
  }
  .panes {
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .pane-slot {
    display: flex;
    min-width: 0;
    flex-grow: 0;
    flex-shrink: 1;
  }
  .cmdline {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 6px 6px 0;
    padding: 0 12px;
    height: 30px;
    border-radius: 8px;
    background: var(--cmdline-bg);
    color: var(--cmdline-fg);
    font-family: var(--mono-font);
    font-size: 0.95em;
  }
  .prompt {
    color: var(--hidden-fg);
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cmd {
    flex: 1;
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
    outline: none;
  }
  .cmd::placeholder {
    color: var(--hidden-fg);
    opacity: 0.6;
  }
  .status {
    font-family: var(--font);
    color: var(--marked-fg);
    white-space: nowrap;
  }
  .keybar {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    gap: 4px;
    padding: 6px;
  }
  .keybar button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    font: inherit;
    font-size: 0.9em;
    color: var(--keybar-label-fg);
    background: var(--keybar-label-bg);
    border: 1px solid var(--border-fg);
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
  }
  .keybar button:hover:not(:disabled) {
    border-color: var(--accent-bg);
  }
  .keybar button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .keybar kbd {
    font-family: var(--mono-font);
    font-size: 0.85em;
    color: var(--keybar-num-fg);
  }
  .update {
    font: inherit;
    font-family: var(--font);
    color: var(--accent-fg);
    background: var(--accent-bg);
    border: 0;
    border-radius: 999px;
    padding: 1px 10px;
    cursor: pointer;
    white-space: nowrap;
  }
  .loading {
    display: grid;
    place-items: center;
    height: 100%;
    color: var(--hidden-fg, #888);
  }
</style>

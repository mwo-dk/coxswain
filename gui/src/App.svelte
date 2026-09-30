<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { listen } from "@tauri-apps/api/event";
  import { ui, init, tab, pane, otherTab, item, load, cd, newTab, goBack, goForward, openItem, toggleMark, targets, reloadAll, snapshot, setTheme, nextView, measureFolders, columnMenu } from "./app.svelte.js";
  import { invoke, keyString, basename, parent, glob, quote, isArchive, LOCKED } from "./lib.js";
  import { t, tn } from "./i18n.svelte.js";
  import Sidebar from "./Sidebar.svelte";
  import Pane from "./Pane.svelte";
  import Preview from "./Preview.svelte";
  import Splitter from "./Splitter.svelte";
  import Dialogs from "./Dialogs.svelte";
  import Duplicates from "./Duplicates.svelte";
  import Settings from "./Settings.svelte";

  let dialogs = $state();
  let panes = $state([]);
  let cmdInput = $state();
  let main = $state();
  /** Command/script output shown in the preview pane. */
  let output = $state(null);
  let notesFocus = $state(0);
  let ready = $state(false);

  let update = $state(null);
  let notice = $state(null);
  function actOn(n) {
    if (n.settings) ui.modal = { kind: "settings", section: n.settings };
    else if (n.url) invoke("open_path", { path: n.url });
    dismissNotice(n);
  }
  const dismissNotice = (n) => invoke("dismiss_notice", { id: n.id }).then(() => (notice = null), () => {});
  init().then(
    () => {
      ready = true;
      // Once a day (check_update itself throttles), so a window left open for weeks still hears.
      const check = () => invoke("check_update").then((v) => (update = v), () => {});
      check();
      setInterval(check, 3600e3);
      // What can be turned on, and the title with the version and the kinds of search on.
      const tell = () =>
        invoke("notices").then((n) => {
          notice = n.notice;
          invoke("set_title", { title: n.title }).catch(() => {});
        }, () => {});
      tell();
      setInterval(tell, 10e3);
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
    const go = (isMove) => op(invoke(isMove ? "rename" : "copy", { paths: p.paths, base: dest, dest }), t(isMove ? "status.moved" : "status.copied", { what: n }));
    ui.modal = {
      kind: "menu",
      title: t("app.drop_title", { what: n, folder: basename(dest) || dest }),
      direct: true,
      filter: "",
      cursor: 0,
      items: [
        { key: "c", label: t("app.copy_here"), icon: "\u{f0c5}", run: () => go(false) },
        { key: "m", label: t("app.move_here"), icon: "\u{f0b2}", run: () => go(true) },
      ],
    };
  });

  const describe = (paths) => (paths.length === 1 ? `"${basename(paths[0])}"` : tn("items", paths.length));
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


  /** Run `start(password)`; when it meets a locked archive, ask for the password and run it
   *  again with that. The password lives only in this call. */
  async function op(start, ok, password = null) {
    try {
      await (typeof start === "function" ? start(password) : start);
      ui.status = ok;
    } catch (e) {
      if (typeof start === "function" && String(e).includes(LOCKED)) {
        ui.modal = {
          kind: "input",
          secret: true,
          title: t("archive.locked_title"),
          label: t(password === null ? "archive.locked_label" : "archive.locked_again"),
          value: "",
          run: (pw) => op(start, ok, pw),
        };
        return;
      }
      ui.modal = { kind: "message", title: t("dialog.error"), text: String(e) };
    }
    for (const p of ui.panes) for (const t of p.tabs) t.marked.clear();
    await reloadAll();
  }

  function transfer(isMove, dest = otherTab().dir) {
    const paths = targets();
    if (!paths.length) return;
    const what = describe(paths);
    prompt(t(isMove ? "dialog.move" : "dialog.copy"), t(isMove ? "dialog.move_to" : "dialog.copy_to", { what }), dest, (d) => {
      if (d.trim()) op((password) => invoke(isMove ? "rename" : "copy", { paths, base: tab().dir, dest: d, password }), t(isMove ? "status.moved" : "status.copied", { what }));
    });
  }

  function remove(forever) {
    const paths = targets();
    if (!paths.length) return;
    const what = describe(paths);
    const run = () => op(invoke("delete", { paths, forever }), t(forever ? "status.deleted" : "status.trashed", { what }));
    // Inside an archive there is no trash: it is taken out of the archive, which is written anew.
    const text = tab().archive ? t("confirm.archive_remove", { what, archive: basename(tab().archive) }) : t(forever ? "confirm.delete_forever" : "confirm.trash", { what });
    if (ui.cfg.confirm_delete) ui.modal = { kind: "confirm", title: t("dialog.delete"), text, ok: t(forever ? "common.delete" : "app.move_to_trash"), run };
    else run();
  }

  async function clip(cut) {
    const paths = targets();
    if (!paths.length) return;
    try {
      await invoke("clip_set", { paths, cut });
      ui.status = t(cut ? "app.clip_cut" : "app.clip_copied", { what: describe(paths) });
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
    ui.status = t("app.running", { what: command });
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
    prompt(t(sel ? "app.select_files" : "app.unselect_files"), t("app.matching"), "*", (v) => {
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

  function sortBy(k) {
    const t = tab();
    t.reverse = t.sort === k && !t.reverse;
    t.sort = k;
    load(t);
  }

  function runScript(s) {
    const tb = tab();
    const e = item(tb);
    const file = e && e.name !== ".." ? e.path : null;
    const selected = tb.items.filter((x) => tb.marked.has(x.path)).map((x) => x.path);
    ui.status = t("app.running", { what: s.label });
    invoke("run_script", { user: s.user, path: s.path, dir: tb.dir, file, selected }).then(
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
    // mode: 0 names everywhere, 1 names in this folder, 2 the text of files.
    search: () => (ui.modal = { kind: "search", query: "", mode: 0, res: null, cursor: 0 }),
    refresh: async () => {
      await reloadAll();
      ui.status = t("status.reread");
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
      prompt(t("dialog.new_folder"), t("app.name_label"), "", async (name) => {
        if (!name.trim()) return;
        await op(invoke("mkdir", { base: tab().dir, name }), t("status.created", { what: name }));
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
        ui.status = tn(moved ? "app.moved_items" : "app.pasted_items", n);
      } catch (e) {
        ui.modal = { kind: "message", title: t("action.paste"), text: String(e) };
      }
      reloadAll();
    },
    properties: async () => {
      const e = item();
      if (!e || e.name === "..") return;
      ui.status = t("app.reading_properties");
      try {
        const p = await invoke("properties", { path: e.path });
        ui.modal = { kind: "props", props: p, mode: p.mode?.toString(8).padStart(3, "0") ?? "", readonly: p.readonly };
      } catch (err) {
        ui.modal = { kind: "message", title: t("action.properties"), text: String(err) };
      }
      ui.status = "";
    },
    extract: () => {
      const paths = targets().filter((p) => isArchive(basename(p)));
      if (!paths.length) return void (ui.status = t("app.not_archive"));
      const what = describe(paths);
      prompt(t("app.extract"), t("app.extract_into", { what }), otherTab().dir, (d) => {
        if (d.trim()) op((password) => invoke("extract", { paths, base: tab().dir, dest: d, password }), t("app.extracted", { what }));
      });
    },
    pack: () => {
      const paths = targets();
      if (!paths.length) return;
      const what = describe(paths);
      const name = paths.length === 1 ? basename(paths[0]).replace(/\.[^.]*$/, "") || basename(paths[0]) : basename(tab().dir) || "archive";
      prompt(t("archive.pack"), t("archive.pack_into", { what }), `${otherTab().dir}${ui.cfg.sep}${name}.zip`, (d) => {
        if (d.trim()) op(invoke("pack", { paths, base: tab().dir, dest: d }), t("archive.packed", { what }));
      });
    },
    user_menu: async () => {
      const list = await invoke("scripts");
      ui.modal = {
        kind: "menu",
        title: t("app.scripts"),
        direct: true,
        filter: "",
        cursor: 0,
        items: list.map((s) => ({ key: s.key, label: s.label, icon: s.path ? "\u{f489}" : "\u{f120}", run: () => runScript(s) })),
      };
    },
    menu: () =>
      (ui.modal = {
        kind: "menu",
        title: t("menu.commands"),
        direct: false,
        filter: "",
        cursor: 0,
        items: [
          ...Object.entries(ui.cfg.actions)
            .filter(([n]) => !["menu", "up", "down"].includes(n))
            .map(([n, [label, key]]) => ({ key, label, run: () => actions[n]?.() })),
          ...ui.cfg.builtin_themes.map((id) => ({ key: id === ui.theme ? t("app.current") : "", label: t("app.theme", { name: t(`theme.${id}`) }), icon: "\u{f53f}", run: () => setTheme(id, true) })),
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
      const tb = tab();
      const dirs = tb.marked.size ? targets(tb) : tb.items.filter((e) => e.is_dir && e.name !== "..").map((e) => e.path);
      if (!dirs.length) return;
      ui.status = tn("app.measuring", dirs.length);
      await measureFolders(tb, dirs);
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
    duplicates: () => (ui.modal = { kind: "dupes" }),
    settings: () => (ui.modal = { kind: "settings" }),
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
      <span class="prompt">{#if ui.quick !== null}{t("quick_search", { query: ui.quick })}{:else}<bdi dir="ltr">{tab().dir}</bdi> ❯{/if}</span>
      <input class="cmd" dir="auto" bind:this={cmdInput} bind:value={ui.cmd} spellcheck="false" autocomplete="off" placeholder={t("app.cmd_placeholder")} aria-label={t("app.cmd_line")} />
      {#if ui.status}<span class="status">{ui.status}</span>{/if}
      <button class="gear" title={`${t("settings.title")} (${ui.cfg.actions.settings?.[1] ?? ""})`} onclick={() => (ui.modal = { kind: "settings" })}>{"\u{f013}"} {t("settings.title")}</button>
      {#if notice && !update}
        <span class="notice">
          <button class="update" onclick={() => actOn(notice)}>{notice.text}</button>
          <button class="dismiss" title={t("common.close")} aria-label={t("common.close")} onclick={() => dismissNotice(notice)}>×</button>
        </span>
      {/if}
      {#if update}
        <!-- With a package manager, the command upgrades; the page still has the release notes. -->
        <button class="update" title={update[1] ? t("app.update_how", { how: update[1] }) : t("app.update_releases")}
          onclick={() => invoke("open_path", { path: "https://github.com/mwo-dk/coxswain/releases/latest" })}>
          {update[1] ? t("status.update", { version: update[0], how: update[1] }) : t("app.update_available", { version: update[0] })}
        </button>
      {/if}
    </label>
    <nav class="keybar" aria-label={t("app.fkeys")}>
      {#each fkeys as f (f.n)}
        <button disabled={!f.act} onclick={() => f.act && actions[f.act]()}><kbd>F{f.n}</kbd><span>{f.label}</span></button>
      {/each}
    </nav>
  </div>
{:else}
  <div class="loading">{ui.status || t("common.loading")}</div>
{/if}

<Dialogs bind:this={dialogs} />
{#if ui.modal?.kind === "dupes"}<Duplicates />{/if}
{#if ui.modal?.kind === "settings"}<Settings />{/if}

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
    border-radius: var(--r-sm);
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
    border-radius: var(--r);
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
    border-radius: var(--r);
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
    border-radius: var(--r);
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
  .gear {
    font: inherit;
    font-family: var(--icon-font), var(--font);
    color: var(--hidden-fg);
    background: none;
    border: 0;
    padding: 0 2px;
    cursor: pointer;
    white-space: nowrap;
  }
  .gear:hover {
    color: var(--cmdline-fg);
  }
  .notice {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .dismiss {
    font: inherit;
    color: var(--hidden-fg);
    background: none;
    border: 0;
    cursor: pointer;
    padding: 0 4px;
  }
  .update {
    font: inherit;
    font-family: var(--font);
    color: var(--accent-fg);
    background: var(--accent-bg);
    border: 0;
    border-radius: var(--r-pill);
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

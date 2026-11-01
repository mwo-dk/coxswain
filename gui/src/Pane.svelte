<script>
  import { tick } from "svelte";
  import { ui, addTab, closeTab, cd, up, goBack, goForward, focusPane, openFind, nextView, openGitView, openSnapshots, openHistory, gitFacts, gitMenu, binAct, actionLabel } from "./app.svelte.js";
  import { invoke, basename, crumbs, size, MARKERS, SNAPSHOTS, PACKAGE, date, composing } from "./lib.js";
  import { t as tr, tn } from "./i18n.svelte.js"; // `t` is the tab here
  import DetailsView from "./DetailsView.svelte";
  import ColumnsView from "./ColumnsView.svelte";
  import GridView from "./GridView.svelte";

  /** @type {{ index: number }} */
  let { index } = $props();

  /** A tab's name: its folder's, but a history's list and a commit's top (and the branches',
   *  the worktrees') are named after what they are of, not "@history" or a commit id. */
  const tabName = (tb) => {
    if (tb.bin || basename(tb.dir) === "@bin") return tr("sidebar.bin");
    const name = basename(tb.dir) || tb.dir;
    const top = tb.history && MARKERS.some((m) => name === m || tb.dir.endsWith(`${m}/${name}`) || tb.dir.endsWith(`${m}\\${name}`));
    if (name === SNAPSHOTS || name === PACKAGE) return basename(tb.dir.slice(0, -name.length - 1)) || name;
    return top ? basename(tb.history.target) : name;
  };
  /** The badge of a list of commits, branches or worktrees. */
  const badgeOf = (h) => tr(`${h.view}.badge_of`, { name: basename(h.target) });

  const p = $derived(ui.panes[index]);
  const t = $derived(p.tabs[p.active]);
  const active = $derived(ui.activePane === index && !ui.modal);
  let editing = $state(false);
  let pathInput = $state();
  let pathValue = $state("");

  export async function editPath() {
    pathValue = t.dir;
    editing = true;
    await tick();
    pathInput?.select();
  }

  async function submitPath() {
    editing = false;
    const dir = await invoke("resolve_path", { base: t.dir, input: pathValue });
    await cd(t, dir);
  }


  // Folders as measured; inside an archive (or a list of snapshots) as listed.
  const listedSizes = $derived((t.archive && !t.history) || (t.snapshot && !t.snapshot.snapshot));
  const selBytes = $derived(listedSizes ? t.marks.bytes + t.marks.dir_bytes : t.marks.dirs.reduce((a, d) => a + (t.sizes[d] ?? 0), t.marks.bytes));
  const count = $derived(t.items.length - (t.up ? 1 : 0));
  /** The ZFS footer: dataset, compression ratio, space (used of the quota when it has one). */
  const zfsLine = $derived(
    t.zfs &&
      (t.zfs.quota
        ? tr("zfs.footer_quota", { dataset: t.zfs.dataset, ratio: t.zfs.compressratio, used: size(t.zfs.used), quota: size(t.zfs.quota) })
        : tr("zfs.footer", { dataset: t.zfs.dataset, ratio: t.zfs.compressratio, used: size(t.zfs.used), free: size(t.zfs.available) })),
  );
</script>

<section
  class="pane"
  class:active
  class:in-archive={!!t.archive || !!t.history || !!t.snapshot || !!t.package}
  class:drag-over={ui.dropPane === index}
  data-pane={index}
  aria-label={tr("pane.label", { n: index + 1 })}
  onpointerdown={() => focusPane(index)}
>
  <div class="tabs" role="tablist">
    {#each p.tabs as tb, i (tb.id)}
      <div class="tab" class:current={i === p.active} role="tab" tabindex="-1" aria-selected={i === p.active}
        onclick={() => focusPane(index, i)} onauxclick={(e) => e.button === 1 && closeTab(p, i)} onkeydown={() => {}} title={`${tb.dir}\n${tr("pane.tab_tip", { key: ui.cfg.actions.close_tab?.[1] ?? "" })}`}>
        <span class="ticon">{tb.git ? "\u{e702}" : "\u{f07b}"}</span>
        <span class="tname">{tabName(tb)}</span>
        {#if p.tabs.length > 1}
          <button class="x" title={tr("pane.close_tab")} onclick={(e) => { e.stopPropagation(); closeTab(p, i); }}>×</button>
        {/if}
      </div>
    {/each}
    <button class="newtab" title={tr("pane.new_tab")} onclick={() => addTab(p)}>+</button>
  </div>

  <div class="bar">
    <button class="nav flip" title={tr("pane.back")} disabled={!t.back.length} onclick={() => goBack(t)}>{"\u{f060}"}</button>
    <button class="nav flip" title={tr("pane.forward")} disabled={!t.fwd.length} onclick={() => goForward(t)}>{"\u{f061}"}</button>
    <button class="nav" title={tr("pane.up")} disabled={!up(t)} onclick={() => cd(t, up(t))}>{"\u{f062}"}</button>
    {#if editing}
      <input
        class="path-edit"
        bind:this={pathInput}
        bind:value={pathValue}
        spellcheck="false"
        onkeydown={(e) => {
          if (composing(e)) return;
          if (e.key === "Enter") submitPath();
          if (e.key === "Escape") editing = false;
          e.stopPropagation();
        }}
        onblur={() => (editing = false)}
      />
    {:else}
      <!-- Ctrl+L is the keyboard way in; a click on empty space is a shortcut. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <div class="crumbs" role="navigation" onclick={(e) => e.target === e.currentTarget && editPath()} title={tr("pane.path_tip")}>
        {#each crumbs(t.dir, ui.cfg.home) as c, i (c.path)}
          {#if i > 0}<span class="sep flip">{"\u{f054}"}</span>{/if}
          <button class="crumb" class:archive-crumb={c.path === t.archive || (t.history && MARKERS.includes(c.name)) || (t.snapshot && (c.name === SNAPSHOTS || c.name === t.snapshot.snapshot)) || (t.package && c.name === PACKAGE)} onclick={() => cd(t, c.path === t.history?.target ? t.history.base : c.path)}>{c.name}</button>
        {/each}
        {#if t.archive}
          <!-- Inside an archive: said, so a copy out is not taken for a copy between folders. -->
          <span class="archive-badge" class:has-image={!!t.image} title={(t.image ? `${t.image}\n` : "") + (t.diskImage ? tr("disk_image.copy_out_hint", { archive: basename(t.archive) }) : tr("archive.inside_tip"))}>{"\u{f410}"}{#if t.image}<span class="badge-image">{t.image}</span>{/if}<span class="badge-kind">{t.image ? "· " : " "}{tr(t.diskImage || t.nested ? "dialogs.readonly" : t.locked ? "archive.badge_locked" : "archive.badge")}</span></span>
        {/if}
        {#if t.history}
          <!-- In a history: which file or folder, and which commit, so an old file is not taken for today's. -->
          {@const c = t.history.commit}
          <span class="archive-badge" title={c ? `${c.hash}\n${c.author} · ${date(c.time)}\n${c.subject}` : tr(`${t.history.view}.inside_tip`)}>
            {t.history.view === "history" ? "\u{f1da}" : "\u{e725}"} {c ? tr("history.badge_at", { commit: c.hash.slice(0, 7) }) : badgeOf(t.history)}
          </span>
          {#if t.history.view === "history" && !c && t.history.scoped}
            <!-- Which commits the list and its graph show: this branch's, or every branch's. -->
            {@const keyOf = (s) => ui.cfg.actions[s === "upstream" ? "history_upstream" : "history_all"]?.[1] ?? ""}
            <span class="scope" role="group">
              {#each ["branch", "all", "upstream"] as s (s)}
                <button class:on={t.history.scope === s} aria-pressed={t.history.scope === s} title={tr(`history.scope_${s}_tip`, { key: keyOf(s) })} onclick={() => t.history.scope !== s && openHistory(t, s === "branch" ? t.history.scope : s)}>{tr(`history.scope_${s}`)}</button>
              {/each}
            </span>
          {/if}
        {/if}
        {#if t.snapshot}
          <!-- Among a dataset's snapshots: which, so an old file is not taken for today's. -->
          <span class="archive-badge" title={tr(t.snapshot.snapshot ? "zfs.inside_tip" : "zfs.list_tip")}>
            {"\u{f030}"} {t.snapshot.snapshot ? tr("zfs.badge_at", { name: t.snapshot.snapshot, dataset: t.snapshot.dataset }) : tr("zfs.badge_list", { dataset: t.snapshot.dataset })}
          </span>
        {/if}
        {#if t.package}
          <span class="archive-badge" title={tr("pkg.list_tip")}>{"\u{f487}"} {tr("pkg.badge", { name: t.package })}</span>
        {/if}
        {#if t.bin}
          <!-- The bin: what it holds, and its actions as buttons (the action menu has them too). -->
          <span class="archive-badge" title={tr("bin.inside_tip")}>{"\u{f1f8}"} {ui.bin?.text ?? tr("sidebar.bin")}</span>
          <span class="scope" role="group">
            <!-- As icons, the action's name and key in its tooltip: the path bar leaves little room. -->
            {#each ui.bin && !ui.bin.restores ? ["bin_delete", "bin_empty"] : ["bin_restore", "bin_delete", "bin_empty"] as a (a)}
              {@const key = ui.cfg.actions[a]?.[1]}
              <button class="bin-act" title={key ? `${actionLabel(a)} (${key})` : actionLabel(a)} aria-label={actionLabel(a)} onclick={() => binAct(a.slice(4))}>{{ bin_restore: "\u{f0e2}", bin_delete: "\u{f1f8}", bin_empty: "\u{f12d}" }[a]}</button>
            {/each}
          </span>
        {/if}
      </div>
    {/if}
    <!-- Find: names, words in files, meaning and Ask, from here (Ctrl+F). -->
    <button class="nav" title={`${tr("search.title")} · ${ui.cfg.actions.search?.[1] ?? ""}`} aria-label={tr("search.title")} onclick={() => { focusPane(index, p.active); openFind("all"); }}>{"\u{f002}"}</button>
    <button class="nav" title={ui.dual ? tr("pane.one_pane") : tr("pane.two_panes")} onclick={() => (ui.dual = !ui.dual)}>
      {ui.dual ? "\u{f2d0}" : "\u{eb56}"}
    </button>
    <button class="nav" title={tr("pane.view")} onclick={() => (t.view = nextView(t.view))}>
      {({ details: "\u{f03a}", columns: "\u{f0db}", grid: "\u{f00a}" })[t.view]}
    </button>
  </div>

  {#if t.error}<div class="error">{t.error}</div>{/if}

  {#if t.view === "columns"}
    <ColumnsView {t} {active} onfocus={() => focusPane(index)} />
  {:else if t.view === "grid"}
    <GridView {t} {active} onfocus={() => focusPane(index)} />
  {:else}
    <DetailsView {t} {active} onfocus={() => focusPane(index)} />
  {/if}

  <footer class="foot">
    <!-- Against the upstream: which, how far ahead and behind, and the last fetch. -->
    <span>{tn("items", count)}{#if t.history?.upstream}&nbsp;· {t.history.upstream}{/if}{#if t.marks.count}&nbsp;· <b>{tn("pane.selected", t.marks.count, { size: size(selBytes) })}</b>{/if}</span>
    {#if t.hasNotes}<span class="note" title={tr("pane.has_notes")}>{"\u{f249}"}</span>{/if}
    <!-- On ZFS: the dataset and its space; a click lists the snapshots. -->
    {#if zfsLine}<button class="zfs-line" title={`${t.zfs.mountpoint}\n${tr("zfs.footer_tip", { key: ui.cfg.actions.snapshots?.[1] ?? "" })}`} onclick={() => openSnapshots(t)}>{zfsLine}</button>{/if}
    <!-- The git line: a click shows the branches; hovered, the upstream, remote and last fetch. -->
    {#if t.git}<button class="git-prompt" title={[t.git.root, ...(ui.facts[t.git.root]?.slice(1) ?? []), tr("branches.line_tip"), tr("gitconfig.line_tip", { key: ui.cfg.actions.git_config?.[1] || "F9" }), tr("gittool.line_tip", { key: ui.cfg.actions.git_tool?.[1] || "F9" }), tr("remote.line_tip", { fetch: ui.cfg.actions.fetch?.[1] || "F9", pull: ui.cfg.actions.pull?.[1] || "F9" })].join("\n")} onmouseenter={() => gitFacts(t.git.root)} onclick={() => openGitView(false, t)} oncontextmenu={(ev) => (ev.preventDefault(), gitMenu(t))}>{t.git.prompt}</button>{/if}
  </footer>
</section>

<style>
  .bin-act {
    font-family: var(--icon-font);
  }
  /* Right to left, "back" points right and the path chevrons point left. */
  :global([dir="rtl"]) .flip {
    transform: scaleX(-1);
  }
  .pane {
    display: flex;
    flex-direction: column;
    min-width: 180px;
    min-height: 0;
    flex: 1;
    background: var(--panel-bg);
    color: var(--panel-fg);
    border-radius: var(--r);
    border: 1px solid var(--border-fg);
    overflow: hidden;
  }
  .pane.active {
    border-color: color-mix(in srgb, var(--accent-bg) 70%, var(--border-fg));
  }
  .pane.drag-over {
    outline: 2px dashed var(--accent-bg);
    outline-offset: -3px;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
    padding: 0;
  }
  .tabs {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    padding: 4px 6px 0;
    background: var(--tab-bg);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 200px;
    padding: 5px 10px;
    border-radius: var(--r) var(--r) 0 0;
    color: var(--tab-fg);
    cursor: default;
    white-space: nowrap;
  }
  .tab:hover {
    background: color-mix(in srgb, var(--tab-active-bg) 50%, transparent);
  }
  .tab.current {
    background: var(--tab-active-bg);
    color: var(--tab-active-fg);
  }
  .ticon {
    font-family: var(--icon-font);
    color: var(--directory-fg);
  }
  .tname {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    opacity: 0;
    width: 1.3em;
    border-radius: var(--r-sm);
    line-height: 1.2;
  }
  .tab:hover .x,
  .tab.current .x {
    opacity: 0.7;
  }
  .x:hover {
    opacity: 1;
    background: var(--cursor-bg);
  }
  .newtab {
    padding: 4px 10px;
    color: var(--tab-fg);
    border-radius: var(--r);
    margin-bottom: 2px;
  }
  .newtab:hover {
    background: var(--tab-active-bg);
  }
  .bar {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 2px;
    padding: 6px 6px;
    border-bottom: 1px solid var(--border-fg);
  }
  .nav {
    font-family: var(--icon-font);
    width: 28px;
    height: 26px;
    border-radius: var(--r);
    color: var(--hidden-fg);
  }
  .nav:hover:not(:disabled) {
    background: var(--cursor-bg);
    color: var(--panel-fg);
  }
  .nav:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .crumbs,
  .path-edit {
    flex: 1;
    min-width: 0;
    height: 26px;
    margin: 0 4px;
    border-radius: var(--r);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    box-sizing: border-box;
  }
  /* A path too long for the bar loses its start, not its end: the folder you are in and the
     badge stay in sight. While it fits, the auto margin (the badge's, or the spacer's) keeps it
     at the start. */
  .crumbs {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    overflow: hidden;
    padding: 0 4px;
    cursor: text;
    white-space: nowrap;
  }
  /* Inside an archive the pane is tinted, and the archive's name in the path is marked. */
  .pane.in-archive {
    background: color-mix(in srgb, var(--accent-bg) 7%, var(--panel-bg, transparent));
  }
  .archive-crumb {
    color: var(--accent-fg);
    background: color-mix(in srgb, var(--accent-bg) 60%, transparent);
  }
  .crumbs:not(:has(.archive-badge))::after {
    content: "";
    margin-inline-start: auto;
  }
  .archive-badge {
    margin-inline-start: auto;
    padding: 0 8px;
    border-radius: var(--r-pill);
    font-size: 0.8em;
    color: var(--accent-fg);
    background: var(--accent-bg);
    flex: none;
  }
  /* A container image's name may be longer than the bar: it is cut with an ellipsis, at most
     part of the bar's width, and the kind (Read-only) always shows. */
  .archive-badge.has-image {
    display: inline-flex;
    flex: 0 1 auto;
    max-width: 70%;
    gap: 0.35em;
    white-space: pre;
  }
  .badge-image {
    min-width: 6ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badge-kind {
    flex: none;
  }
  /* This branch | All branches, next to a history's badge. */
  .scope {
    display: flex;
    flex: none;
    margin-inline-start: 6px;
    font-size: 0.8em;
    border: 1px solid var(--accent-bg);
    border-radius: var(--r-pill);
    overflow: hidden;
  }
  .scope button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0 8px;
    cursor: pointer;
  }
  .scope button.on {
    color: var(--accent-fg);
    background: var(--accent-bg);
  }
  .crumb {
    padding: 1px 6px;
    border-radius: var(--r-sm);
    cursor: pointer;
    flex: none;
  }
  .crumb:last-child {
    font-weight: 600;
  }
  .crumb:hover {
    background: var(--cursor-bg);
  }
  .sep {
    font-family: var(--icon-font);
    font-size: 0.65em;
    color: var(--hidden-fg);
    flex: none;
  }
  .path-edit {
    font: inherit;
    color: var(--dialog-input-fg);
    padding: 0 8px;
    outline: 1px solid var(--accent-bg);
  }
  .error {
    padding: 6px 12px;
    color: var(--git-deleted-fg);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    font-size: 0.85em;
    color: var(--hidden-fg);
    border-top: 1px solid var(--border-fg);
    white-space: nowrap;
  }
  .foot b {
    color: var(--marked-fg);
    font-weight: 600;
  }
  .note {
    font-family: var(--icon-font);
    color: var(--marked-fg);
  }
  .zfs-line {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .zfs-line:hover {
    color: var(--panel-fg);
  }
  .git-prompt {
    margin-inline-start: auto;
    background: none;
    border: 0;
    padding: 0;
    font-size: inherit;
    cursor: pointer;
    white-space: nowrap;
    font-family: var(--icon-font), var(--font), var(--scripts);
    color: var(--git-branch-fg);
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>

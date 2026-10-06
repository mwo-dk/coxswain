<script>
  // Build provenance as inputs → build → outputs, with the signer and the checks against the
  // disk (docs/design/provenance-viewer.md). In the preview pane, and full-window (`full`) from
  // its ⤢ button. The Rust side (provenance.rs) reads and checks; nothing is verified. Every
  // string from the file is shown as text, never HTML.
  import { onDestroy, untrack } from "svelte";
  import { ui, tab, otherTab, item, cd, focusPane } from "./app.svelte.js";
  import { invoke, basename, parent, previewKind, size, date } from "./lib.js";
  import { t, tn } from "./i18n.svelte.js";

  let { path, full = false } = $props();

  let view = $state(null);
  let error = $state("");
  /** The statement shown, in a file of several. */
  let at = $state(0);
  /** Per subject: a check result, or "checking". */
  let subjects = $state([]);
  /** Per dependency: where its commit is (git sources only). */
  let sources = $state([]);
  /** The row under the cursor: a column ("in", "build", "out") and a row in it. */
  let sel = $state({ col: "build", i: 0 });
  let moreInputs = $state(false);
  /** A compare with an earlier build: { old, diff } or { old, error }. */
  let compare = $state(null);
  let areas = $state(new Set());
  let flowEl = $state();

  const entry = $derived(view?.entries[at] ?? null);
  const st = $derived(entry?.statement);
  const prov = $derived(st?.predicate.kind === "provenance" ? st.predicate : null);
  /** The other pane's folder, where subjects are looked for too. */
  const otherDir = $derived(ui.dual ? (otherTab()?.dir ?? null) : null);
  const here = $derived(parent(path));

  $effect(() => {
    const p = path;
    error = "";
    const timer = setTimeout(async () => {
      try {
        const v = await invoke("provenance_info", { path: p });
        if (p !== path) return;
        if (compare && compare.path !== p) stopCompare();
        view = v;
        if (at >= v.entries.length) at = 0;
      } catch (err) {
        if (p === path) (error = String(err)), (view = null);
      }
    }, 60);
    return () => clearTimeout(timer);
  });

  // The checks: every subject against the files here, every git source against a checkout.
  $effect(() => {
    const p = path;
    const k = at;
    const other = otherDir !== here ? otherDir : null;
    const e = entry;
    // Only the reads above make this run again: the checks write the results they read.
    untrack(() => {
      subjects = [];
      sources = [];
      if (!e) return;
      e.statement.subjects.forEach((_, i) => check(p, k, i, other, false));
      if (e.git.some(Boolean))
        invoke("provenance_sources", { path: p, entry: k, other })
          .then((s) => p === path && k === at && (sources = s))
          .catch(() => {});
    });
  });

  function check(p, k, i, other, all) {
    subjects[i] = "checking";
    invoke("provenance_subject", { path: p, entry: k, index: i, other, all })
      .then((r) => p === path && k === at && (subjects[i] = r))
      .catch((err) => p === path && k === at && (subjects[i] = { state: "unreadable", message: String(err) }));
  }

  onDestroy(() => invoke("provenance_cancel").catch(() => {}));

  // ------------------------------------------------------------ rows

  /** Inputs: git sources first, then the rest, in the file's order within each. */
  const inputs = $derived.by(() => {
    if (!prov) return [];
    const all = prov.dependencies.map((d, i) => ({ d, i, git: entry.git[i] }));
    return [...all.filter((x) => x.git), ...all.filter((x) => !x.git)];
  });
  const INPUTS_SHOWN = 8;
  const shownInputs = $derived(moreInputs ? inputs : inputs.slice(0, INPUTS_SHOWN));
  const outputs = $derived(st ? [...st.subjects.map((s, i) => ({ s, i })), ...(prov?.byproducts ?? []).map((s, i) => ({ s, i, by: true }))] : []);
  const columns = $derived([prov ? "in" : null, prov ? "build" : null, "out"].filter(Boolean));
  const rowsIn = (col) => (col === "in" ? shownInputs.length + (inputs.length > shownInputs.length ? 1 : 0) : col === "build" ? 1 : outputs.length);

  // A new statement or file starts at the build (or the outputs, when there is no build).
  $effect(() => {
    void at;
    void path;
    const start = prov ? "build" : "out";
    untrack(() => {
      sel = { col: start, i: 0 };
      moreInputs = false;
    });
  });

  const ARROW_MARK = { matches: "✓", differs: "✗", missing: "?", cannotCheck: "–", large: "↵", unreadable: "!" };
  const COLOR = { matches: "green", differs: "red", missing: "grey", cannotCheck: "muted", large: "grey", unreadable: "red" };

  function subjectWord(r, s) {
    if (!r) return "";
    if (r === "checking") return t("provenance.subject.checking");
    switch (r.state) {
      case "matches":
        return t("provenance.subject.matches");
      case "differs":
        return t("provenance.subject.differs");
      case "missing":
        return t("provenance.subject.missing");
      case "large":
        return t("provenance.subject.large", { size: size(r.size) });
      case "unreadable":
        return t("provenance.subject.unreadable", { message: r.message });
      case "cannotCheck": {
        const why = r.why;
        if (why === "image") return t("provenance.subject.image");
        if (why === "noDigest") return t("provenance.subject.no_digest");
        return t("provenance.subject.algorithms", { algorithms: (why.algorithms ?? Object.keys(s.digest)).join(", ") });
      }
    }
    return "";
  }

  const SOURCE_MARK = { onBranch: "●", ahead: "●", elsewhere: "◐", missing: "○", noCheckout: "○" };
  function sourceWord(r) {
    if (!r) return "";
    switch (r.state) {
      case "onBranch":
        return r.behind ? tn("provenance.source.behind", r.behind) : t("provenance.source.on_head");
      case "ahead":
        return tn("provenance.source.ahead", r.ahead);
      case "elsewhere":
        return t("provenance.source.elsewhere");
      case "missing":
        return t("provenance.source.missing");
      case "noCheckout":
        return t("provenance.source.no_checkout");
    }
    return "";
  }

  const label = (r) => r.name ?? r.uri ?? "";
  const digests = (r) => Object.entries(r.digest ?? {});
  const when = (s) => {
    const ms = Date.parse(s);
    return Number.isNaN(ms) ? s : date(ms / 1000);
  };
  const fact = (k) => entry?.facts.find((f) => f.key === k);

  const kindLabel = $derived.by(() => {
    if (!st) return "";
    const p = st.predicate;
    if (p.kind === "provenance") return t("provenance.kind.slsa", { version: p.version });
    if (p.kind === "vsa") return t("provenance.kind.vsa");
    if (p.kind === "bom") return t("provenance.kind.bom");
    return t("provenance.kind.other", { type: st.predicateType });
  });

  // ------------------------------------------------------------ actions

  /** Shows a file or folder in a pane: the other pane from the preview (so this stays), else this one. */
  async function reveal(target, isDir = false) {
    if (!target) return;
    let tb = tab();
    if (full) ui.modal = null;
    else if (ui.dual) {
      tb = otherTab();
      focusPane(ui.activePane ^ 1);
    }
    await cd(tb, isDir ? target : parent(target));
    if (!isDir) {
      const i = tb.items.findIndex((e) => e.path === target);
      if (i >= 0) tb.cursor = i;
    }
    ui.status = t("status.showing", { name: basename(target) });
  }

  function activate() {
    if (sel.col === "out") {
      const o = outputs[sel.i];
      if (!o || o.by) return;
      const r = subjects[o.i];
      if (r?.state === "large") check(path, at, o.i, otherDir !== here ? otherDir : null, true);
      else if (r?.path) reveal(r.path);
    } else if (sel.col === "in") {
      if (sel.i >= shownInputs.length) return void (moreInputs = true);
      const r = sources[shownInputs[sel.i].i];
      if (r?.checkout) reveal(r.checkout, true);
    }
  }

  async function openBom() {
    try {
      const p = await invoke("provenance_bom", { path, entry: at });
      ui.modal = { kind: "bom", path: p };
    } catch (err) {
      ui.status = String(err);
    }
  }

  const openLink = (u) => /^https?:\/\//i.test(u) && invoke("open_path", { path: u });

  // ------------------------------------------------------------ compare

  /** The file under the cursor in the other pane, when it could be an earlier build's provenance. */
  const other = $derived.by(() => {
    const e = ui.dual ? item(otherTab()) : null;
    if (!e || e.is_dir || e.path === path) return null;
    return previewKind(e) === "provenance" || /\.(json|jsonl|ndjson)$/i.test(e.name) ? e : null;
  });

  async function runCompare(old = other?.path) {
    if (!old) return;
    try {
      compare = { old, path, diff: await invoke("provenance_diff", { old, path }) };
    } catch (err) {
      compare = { old, path, error: String(err) };
    }
  }

  function stopCompare() {
    compare = null;
    areas = new Set();
  }

  const pair = $derived(compare?.diff?.pairs.find((p) => p.after === at) ?? null);
  const changes = $derived((pair?.changes ?? []).filter((c) => !areas.size || areas.has(c.area)));
  const AREAS = [
    ["builder", "builder"],
    ["parameter", "parameters"],
    ["dependency", "dependencies"],
    ["subject", "subjects"],
    ["signer", "signer"],
  ];
  /** Rows the compare marks: dependencies and subjects by key, as the diff names them. */
  const changed = $derived(new Set((pair?.changes ?? []).filter((c) => c.area === "dependency" || c.area === "subject").map((c) => c.key)));
  const depKey = (d) => {
    const u = d.uri;
    if (!u) return label(d);
    if (u.startsWith("git+")) {
      const at = u.lastIndexOf("@");
      return at > u.indexOf("://") ? u.slice(0, at) : u;
    }
    return u.split("?")[0];
  };

  // ------------------------------------------------------------ keys

  function onkeydown(e) {
    const ci = columns.indexOf(sel.col);
    const n = rowsIn(sel.col);
    const move = (col, i) => (sel = { col, i: Math.max(0, Math.min(rowsIn(col) - 1, i)) });
    switch (e.key) {
      case "ArrowRight":
        if (ci < columns.length - 1) move(columns[ci + 1], sel.i);
        break;
      case "ArrowLeft":
        if (ci > 0) move(columns[ci - 1], sel.i);
        break;
      case "ArrowDown":
        move(sel.col, sel.i + 1);
        break;
      case "ArrowUp":
        move(sel.col, sel.i - 1);
        break;
      case "Home":
        move(sel.col, 0);
        break;
      case "End":
        move(sel.col, n - 1);
        break;
      case "Enter":
        activate();
        break;
      case "[":
        if (at > 0) at -= 1;
        break;
      case "]":
        if (view && at < view.entries.length - 1) at += 1;
        break;
      case "Escape":
        if (full) ui.modal = null;
        else flowEl?.blur();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  // Keep the row under the cursor in sight.
  $effect(() => {
    const { col, i } = sel;
    flowEl?.querySelector(`[data-row="${col}-${i}"]`)?.scrollIntoView({ block: "nearest" });
  });

  const pick = (col, i) => ((sel = { col, i }), flowEl?.focus());
  const isSel = (col, i) => sel.col === col && sel.i === i;
</script>

{#snippet digestList(r)}
  {#each digests(r) as [a, d] (a)}<div class="kv"><span class="k">{a}</span><code class="hex">{d}</code></div>{/each}
{/snippet}

{#snippet link(u)}
  {#if /^https?:\/\//i.test(u)}<button class="link" onclick={() => openLink(u)}>{u}</button>{:else}<code>{u}</code>{/if}
{/snippet}

<div class="prov" class:full role={full ? "dialog" : undefined} aria-modal={full ? "true" : undefined} aria-label={full ? basename(path) : undefined}>
  {#if full}
    <header class="top">
      <h2>{"\u{f0c9}"} {basename(path)}</h2>
      <button class="x" title={t("provenance.close")} onclick={() => (ui.modal = null)}>×</button>
    </header>
  {/if}

  {#if error}
    <p class="note">{error}</p>
  {:else if !view || !entry}
    <p class="note">{t("provenance.reading")}</p>
  {:else}
    <div class="head">
      <div class="row">
        <b>{kindLabel}</b>
        {#if view.entries.length > 1}
          <span class="pager">
            <button class="plain" disabled={at === 0} title={t("provenance.prev")} onclick={() => (at -= 1)}>‹</button>
            {t("provenance.statement_of", { at: at + 1, total: view.entries.length })}
            <button class="plain" disabled={at === view.entries.length - 1} title={t("provenance.next")} onclick={() => (at += 1)}>›</button>
          </span>
        {/if}
        <span class="grow"></span>
        {#if st.predicate.kind === "bom"}<button class="plain" onclick={openBom}>{t("provenance.open_bom")}</button>{/if}
        {#if prov && !compare}
          <button class="plain" disabled={!other} title={other ? t("provenance.compare_title", { name: other.name }) : t("provenance.compare_none")} onclick={() => runCompare()}>⇄ {t("provenance.compare")}</button>
        {/if}
        {#if !full}<button class="icon-btn" title={t("provenance.window")} onclick={() => (ui.modal = { kind: "provenance", path })}>{"\u{f065}"}</button>{/if}
      </div>
      <div class="signer">
        {#if entry.signer}
          {t("provenance.signed_by", { identity: entry.signer.identity })}
          {#if entry.signer.claims.find(([c]) => c === "issuer")}<small>· {t("provenance.via", { issuer: entry.signer.claims.find(([c]) => c === "issuer")[1] })}</small>{/if}
        {:else if entry.signatures.some((s) => s.keyid)}
          {t("provenance.signed_key", { key: entry.signatures.find((s) => s.keyid).keyid })}
        {:else if entry.signatures.length || entry.publicKey}
          {t("provenance.signed_nokey")}
        {:else}
          {t("provenance.unsigned")}
        {/if}
        {#each entry.log.slice(0, 1) as l (l.index)}
          <small>· {t("provenance.logged", { index: l.index ?? "?", when: l.integratedTime ? date(l.integratedTime) : "?" })}</small>
        {/each}
      </div>
      <div class="unverified" title={t("provenance.not_verified_tip")}>⚠ {t("provenance.not_verified")}</div>
      {#if view.issues.length}
        <details class="issues">
          <summary>{tn("provenance.issues", view.issues.length)}</summary>
          <ul>{#each view.issues.slice(0, 200) as is, k (k)}<li>{is.line ? `${is.line}: ` : ""}{is.message}</li>{/each}</ul>
        </details>
      {/if}
    </div>

    {#if compare}
      <div class="chips compare">
        <span class="label">{t("provenance.compared", { name: basename(compare.old) })}</span>
        {#if compare.error}
          <span class="note">{compare.error}</span>
        {:else if !pair}
          <span class="note">{tn("provenance.change.unpaired", 1)}</span>
        {:else if !pair.changes.length}
          <span class="note">{t("provenance.change.none")}</span>
        {:else}
          {#each AREAS as [a, k] (a)}
            {@const n = compare.diff.counts[k]}
            {#if n}<button class="chip" class:on={areas.has(a)} onclick={() => (areas.has(a) ? areas.delete(a) : areas.add(a), (areas = new Set(areas)))}>{tn(`provenance.change.${k}`, n)}</button>{/if}
          {/each}
        {/if}
        <button class="x small" title={t("provenance.stop_compare")} onclick={stopCompare}>×</button>
      </div>
      {#if changes.length}
        <ul class="changes mono">
          {#each changes.slice(0, 300) as c, k (k)}
            <li>
              <small class="area">{t(`provenance.area.${c.area}`)}</small>
              <span class="key">{c.key}</span>
              {#if c.kind === "added"}<span class="add">＋ {c.after}</span>
              {:else if c.kind === "removed"}<span class="del">− {c.before}</span>
              {:else}<span class="del">{c.before}</span> → <span class="add">{c.after}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="flow" class:solo={!prov} bind:this={flowEl} tabindex="0" role="grid" aria-label={basename(path)} {onkeydown}>
      {#if prov}
        <section class="col" aria-label={t("provenance.inputs")}>
          <h3>{t("provenance.inputs")}</h3>
          {#each shownInputs as x, k (x.i)}
            {@const r = sources[x.i]}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
            <div class="item" class:sel={isSel("in", k)} data-row="in-{k}" role="row" onclick={() => pick("in", k)} ondblclick={activate}>
              <span class="mark {r?.state === 'onBranch' || r?.state === 'ahead' ? 'green' : 'grey'}">{x.git ? (SOURCE_MARK[r?.state] ?? "·") : "·"}</span>
              <span class="text">
                <span class="name" title={x.d.uri ?? label(x.d)}>{entry.inputLabels[x.i]}</span>
                {#if changed.has(depKey(x.d))}<span class="cmark" title={t("provenance.area.dependency")}>Δ</span>{/if}
                {#if x.d.configSource}<small class="tag">{t("provenance.config_source")}</small>{/if}
                {#if x.git}<small class="state">{sourceWord(r)}</small>{/if}
              </span>
            </div>
          {/each}
          {#if inputs.length > shownInputs.length}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
            <div class="item more" class:sel={isSel("in", shownInputs.length)} data-row="in-{shownInputs.length}" role="row" onclick={() => (moreInputs = true)}>
              {tn("provenance.more", inputs.length - shownInputs.length)}
            </div>
          {/if}
          {#if !inputs.length}<p class="none">{t("provenance.no_inputs")}</p>{/if}
        </section>
        <div class="arrow" aria-hidden="true">▶</div>
        <section class="col" aria-label={t("provenance.build")}>
          <h3>{t("provenance.build")}</h3>
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
          <div class="item build" class:sel={isSel("build", 0)} data-row="build-0" role="row" onclick={() => pick("build", 0)}>
            <b title={prov.builder.id}>{entry.builderLabel}</b>
            {#each ["workflow", "trigger", "runner"] as k (k)}
              {@const f = fact(k)}
              {#if f}<small>{t(`provenance.fact.${k}`)}: {k === "workflow" ? basename(f.value.split("@")[0]) : f.value}</small>{/if}
            {/each}
            {#if prov.started}<small>{when(prov.started)}{prov.finished ? ` → ${when(prov.finished)}` : ""}</small>{/if}
            {#if entry.facts.some((f) => f.conflict)}<small class="red">⚠ {t("provenance.conflicts")}</small>{/if}
          </div>
        </section>
        <div class="arrow" aria-hidden="true">▶</div>
      {/if}
      <section class="col" aria-label={t("provenance.outputs")}>
        <h3>{t("provenance.outputs")}</h3>
        {#each outputs as o, k (`${o.by ? "b" : "s"}${o.i}`)}
          {@const r = o.by ? null : subjects[o.i]}
          {@const state = r === "checking" ? null : r?.state}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
          <div class="item" class:sel={isSel("out", k)} class:by={o.by} data-row="out-{k}" role="row" onclick={() => pick("out", k)} ondblclick={activate}>
            <span class="mark {COLOR[state] ?? 'muted'}">{o.by ? "·" : r === "checking" ? "…" : (ARROW_MARK[state] ?? "·")}</span>
            <span class="text">
              <span class="name">{label(o.s)}</span>
              {#if !o.by && changed.has(label(o.s))}<span class="cmark" title={t("provenance.area.subject")}>Δ</span>{/if}
              {#if o.by}<small class="tag">{t("provenance.byproduct")}</small>{:else}<small class="state {COLOR[state] ?? ''}">{subjectWord(r, o.s)}</small>{/if}
            </span>
          </div>
        {/each}
        {#if !outputs.length}<p class="none">{t("provenance.no_outputs")}</p>{/if}
      </section>
    </div>

    <div class="details">
      {#if st.predicate.kind === "vsa"}
        {@const v = st.predicate}
        <p class="verdict {v.result === 'PASSED' ? 'green' : v.result === 'FAILED' ? 'red' : 'grey'}">
          {v.result === "PASSED" ? "✓" : v.result === "FAILED" ? "✗" : "?"} {v.result === "PASSED" ? t("provenance.vsa.passed") : v.result === "FAILED" ? t("provenance.vsa.failed") : (v.result ?? "?")}
          {#each v.levels as l (l)}<small class="tag">{l}</small>{/each}
        </p>
        <div class="kv"><span class="k">{t("provenance.vsa.verifier")}</span>{@render link(v.verifier)}</div>
        {#if v.resource}<div class="kv"><span class="k">{t("provenance.vsa.resource")}</span>{@render link(v.resource)}</div>{/if}
        {#if v.policy}<div class="kv"><span class="k">{t("provenance.vsa.policy")}</span>{@render link(v.policy)}</div>{/if}
        {#if v.time}<div class="kv"><span class="k">{t("provenance.vsa.time")}</span>{when(v.time)}</div>{/if}
        <p class="note">{t("provenance.vsa.note")}</p>
      {:else if st.predicate.kind === "other" && sel.col !== "out"}
        <p class="note">{t("provenance.other_note")}</p>
      {/if}

      {#if sel.col === "build" && prov}
        {#each entry.facts as f (f.key)}
          <div class="kv">
            <span class="k">{t(`provenance.fact.${f.key}`)}</span>
            <span>{#if f.key === "invocation" || f.key === "repository"}{@render link(f.value)}{:else if f.key === "started" || f.key === "finished"}{when(f.value)}{:else}<code>{f.value}</code>{/if}
              <!-- Where a fact came from matters only when a certificate could have said it. -->
              {#if entry.signer}<small class="from">{t(`provenance.from.${f.from}`)}</small>{/if}
              {#if f.conflict}<small class="red">⚠ {t("provenance.conflict", { value: f.conflict })}</small>{/if}</span>
          </div>
        {/each}
        <div class="kv"><span class="k">{t("provenance.builder_id")}</span><code>{prov.builder.id}</code></div>
        <div class="kv"><span class="k">{t("provenance.build_type")}</span><code>{prov.buildType}</code></div>
        {#each Object.entries(prov.builder.version) as [k, v] (k)}<div class="kv"><span class="k">{t("provenance.builder_version")}</span><code>{k} {v}</code></div>{/each}
        {#if prov.builder.dependencies.length}
          <details><summary>{tn("provenance.builder_deps", prov.builder.dependencies.length)}</summary>
            <ul>{#each prov.builder.dependencies as d, k (k)}<li><code>{label(d)}</code></li>{/each}</ul></details>
        {/if}
        {#if prov.completeness || prov.reproducible != null}
          <div class="kv"><span class="k">v0.2</span><span>
            {#each ["parameters", "environment", "materials"] as k (k)}{#if prov.completeness?.[k]}<small class="tag">{t(`provenance.complete.${k}`)}</small>{/if}{/each}
            {#if prov.reproducible}<small class="tag">{t("provenance.reproducible")}</small>{/if}</span></div>
        {/if}
        {#each [["external", prov.external], ["internal", prov.internal]] as [k, v] (k)}
          {#if v != null && (typeof v !== "object" || Object.keys(v).length)}
            <details><summary>{t(`provenance.${k}`)}</summary><pre class="mono json">{JSON.stringify(v, null, 2).slice(0, 20000)}</pre></details>
          {/if}
        {/each}
      {:else if sel.col === "in" && shownInputs[sel.i]}
        {@const x = shownInputs[sel.i]}
        {@const r = sources[x.i]}
        <div class="kv"><span class="k">{t("provenance.uri")}</span><code>{x.d.uri ?? label(x.d)}</code></div>
        {@render digestList(x.d)}
        {#if x.d.entryPoint}<div class="kv"><span class="k">{t("provenance.fact.workflow")}</span><code>{x.d.entryPoint}</code></div>{/if}
        {#if x.git}<div class="kv"><span class="k">{t("provenance.checkout")}</span><span>{sourceWord(r)}{#if r?.checkout} · <button class="link" onclick={() => reveal(r.checkout, true)}>{r.checkout}</button>{/if}</span></div>{/if}
      {:else if sel.col === "out" && outputs[sel.i]}
        {@const o = outputs[sel.i]}
        {@const r = o.by ? null : subjects[o.i]}
        {#if o.s.uri}<div class="kv"><span class="k">{t("provenance.uri")}</span>{@render link(o.s.uri)}</div>{/if}
        {#if r?.state === "differs"}
          <div class="kv"><span class="k">{t("provenance.named")}</span><code class="hex">{r.algorithm}:{r.expected}</code></div>
          <div class="kv"><span class="k">{t("provenance.on_disk")}</span><code class="hex red">{r.algorithm}:{r.actual}</code></div>
          <p class="red">{t("provenance.differs_note")}</p>
        {:else}
          {@render digestList(o.s)}
          {#if r?.state === "matches"}<p class="note">{t("provenance.matches_note")}</p>{/if}
          {#if r?.state === "large"}<button class="plain" onclick={activate}>{t("provenance.subject.check")}</button>{/if}
        {/if}
        {#if r?.path}<div class="kv"><span class="k">{t("provenance.file")}</span><button class="link" onclick={() => reveal(r.path)}>{r.path}</button></div>{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Marks keep their meaning in every theme: fixed colours that read on dark and light alike,
     as the BOM view's ratings do, and a glyph beside each so colour is never the only signal. */
  .prov {
    --st-green: #43a047;
    --st-red: #e53935;
    --st-grey: #9e9e9e;
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 6px;
    padding: 8px 12px;
    container-type: inline-size;
  }
  .prov.full {
    position: fixed;
    inset: 4vh 4vw;
    z-index: 11;
    padding: 0 0 8px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
  }
  .full > :not(.top) {
    margin-inline: 16px;
  }
  .top {
    display: flex;
    align-items: center;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font), var(--cjk);
  }
  h3 {
    margin: 0 0 4px;
    font-size: 0.78em;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--hidden-fg);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .plain {
    padding: 1px 8px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-sm, 4px);
    font-size: 0.85em;
  }
  .link {
    color: var(--accent, inherit);
    text-decoration: underline;
    padding: 0;
    text-align: start;
    word-break: break-all;
  }
  .icon-btn {
    font-family: var(--icon-font), var(--font);
  }
  .x {
    font-size: 1.3em;
    padding: 0 6px;
  }
  .x.small {
    font-size: 1.1em;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .pager {
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .signer {
    font-size: 0.88em;
    word-break: break-all;
  }
  .signer small,
  .from,
  .state,
  .none,
  .note {
    color: var(--hidden-fg);
  }
  .unverified {
    font-size: 0.85em;
    color: var(--st-grey);
  }
  .issues summary {
    cursor: pointer;
    font-size: 0.85em;
    color: var(--st-red);
  }
  .issues ul,
  .details ul {
    margin: 2px 0;
    padding-inline-start: 1.4em;
    font-size: 0.85em;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
  }
  .chip {
    font-size: 0.82em;
    padding: 1px 8px;
    border: 1px solid var(--border-fg);
    border-radius: 999px;
  }
  .chip.on {
    background: var(--sel-bg);
    color: var(--sel-fg);
  }
  .compare .label {
    font-size: 0.85em;
  }
  .changes {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 30%;
    overflow: auto;
    font-size: 0.8em;
  }
  .changes li {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 1px 0;
    word-break: break-all;
  }
  .area {
    color: var(--hidden-fg);
    min-width: 6em;
  }
  .add {
    color: var(--st-green);
  }
  .del {
    color: var(--st-red);
  }
  .flow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) auto minmax(0, 1fr);
    gap: 6px;
    align-items: start;
    outline: none;
    overflow: auto;
    flex: 0 1 auto;
    max-height: 55%;
    padding: 4px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-md, 6px);
  }
  .flow:focus-visible {
    border-color: var(--sel-bg);
  }
  .flow.solo {
    grid-template-columns: minmax(0, 1fr);
  }
  /* A narrow pane stacks the columns, inputs above the build above the outputs. */
  @container (max-width: 520px) {
    .flow {
      grid-template-columns: minmax(0, 1fr);
    }
    .arrow {
      transform: rotate(90deg);
      justify-self: center;
    }
  }
  .arrow {
    align-self: center;
    color: var(--hidden-fg);
    font-size: 0.8em;
  }
  .col {
    min-width: 0;
  }
  .item {
    display: flex;
    align-items: baseline;
    gap: 5px;
    padding: 2px 5px;
    border-radius: var(--r-sm, 4px);
    font-size: 0.86em;
    cursor: default;
  }
  .item.sel {
    background: var(--sel-bg);
    color: var(--sel-fg);
  }
  .item.sel .state,
  .item.sel .tag {
    color: inherit;
  }
  .item.by {
    opacity: 0.65;
  }
  .item.more {
    color: var(--hidden-fg);
    font-style: italic;
  }
  .build {
    flex-direction: column;
    align-items: stretch;
    border: 1px solid var(--border-fg);
    padding: 5px 7px;
  }
  /* The mark in a column of its own; the name, and under it what the check found. */
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 5px;
  }
  .text .state {
    flex-basis: 100%;
  }
  .name {
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .mark {
    flex: none;
    font-weight: 700;
    width: 1em;
    text-align: center;
  }
  .cmark {
    font-size: 0.85em;
    color: var(--st-red);
  }
  .tag {
    font-size: 0.8em;
    padding: 0 5px;
    border: 1px solid var(--border-fg);
    border-radius: 999px;
    color: var(--hidden-fg);
  }
  .green {
    color: var(--st-green);
  }
  .red {
    color: var(--st-red);
  }
  .grey,
  .muted {
    color: var(--st-grey);
  }
  .item.sel .mark {
    color: inherit;
  }
  .details {
    overflow: auto;
    min-height: 0;
    flex: 1;
    font-size: 0.86em;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .kv {
    display: grid;
    grid-template-columns: 9em minmax(0, 1fr);
    gap: 6px;
    align-items: baseline;
  }
  .kv .k {
    color: var(--hidden-fg);
  }
  code,
  .hex {
    font-family: var(--mono-font, monospace);
    font-size: 0.92em;
    overflow-wrap: anywhere;
  }
  .verdict {
    font-weight: 600;
    margin: 0;
  }
  .json {
    max-height: 18em;
    overflow: auto;
    margin: 2px 0;
    font-size: 0.85em;
  }
  details summary {
    cursor: pointer;
  }
</style>

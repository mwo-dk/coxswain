// Rich previews. Each heavy library is imported the first time a file needs it, so none of
// them cost anything at startup. Everything here renders untrusted file content, so every
// HTML result goes through DOMPurify before it reaches the DOM.

import { marked } from "marked";
import DOMPurify from "dompurify";
import { highlight } from "./hl.js";
import { convertFileSrc, invoke } from "./lib.js";
import { t } from "./i18n.svelte.js";

/** Sanitized HTML. No `<style>`: a file's style sheet would restyle the whole window. `styles`
 *  keeps them for a diagram's own SVG (Mermaid scopes its rules to the diagram's id) and for HTML
 *  whose every piece is sanitized already. */
export const clean = (html, { styles = false } = {}) =>
  DOMPurify.sanitize(html, { USE_PROFILES: { html: true, svg: true, svgFilters: true, mathMl: true }, FORBID_TAGS: styles ? [] : ["style"] });
/** Text as base64 of its UTF-8, a piece at a time (a large diagram would overflow the call). */
const base64 = (s) => {
  const u = new TextEncoder().encode(s);
  let bin = "";
  for (let i = 0; i < u.length; i += 0x8000) bin += String.fromCharCode(...u.subarray(i, i + 0x8000));
  return btoa(bin);
};
const escape = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** File bytes through the asset protocol; refuses files too big to preview. */
export async function bytes(path, max = 25 * 1024 * 1024) {
  // The size first, so a file too big is never read.
  const head = await fetch(convertFileSrc(path), { method: "HEAD" });
  if (Number(head.headers.get("content-length")) > max) throw new Error(t("render.too_large"));
  const r = await fetch(convertFileSrc(path));
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  if (Number(r.headers.get("content-length")) > max) throw new Error(t("render.too_large"));
  return r.arrayBuffer();
}

// ------------------------------------------------------------ markdown, math, mermaid

let mathReady;
function withMath() {
  mathReady ??= Promise.all([import("marked-katex-extension"), import("katex/dist/katex.min.css")]).then(([k]) =>
    marked.use(k.default({ throwOnError: false, nonStandard: true })),
  );
  return mathReady;
}

let mermaidReady;
let mermaidSeq = 0;
async function mermaid() {
  mermaidReady ??= import("mermaid").then(({ default: m }) => m);
  const m = await mermaidReady;
  // Plain SVG text labels (no foreignObject HTML), so the sanitizer keeps them intact.
  // The diagram takes the theme's own colours, so it is green in Cyber and grey in Windows 95.
  const css = getComputedStyle(document.documentElement);
  const v = (name, fallback) => css.getPropertyValue(name).trim() || fallback;
  const dark = css.colorScheme === "dark";
  const [bg, fg, box, line] = [v("--panel-bg", dark ? "#1e1f22" : "#ffffff"), v("--panel-fg", dark ? "#dcdde1" : "#1f2328"), v("--status-bg", dark ? "#2b2d31" : "#f3f4f6"), v("--border-fg", "#808080")];
  m.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "base",
    themeVariables: {
      darkMode: dark,
      background: bg,
      primaryColor: box,
      primaryTextColor: fg,
      primaryBorderColor: fg,
      secondaryColor: box,
      tertiaryColor: bg,
      lineColor: fg,
      textColor: fg,
      mainBkg: box,
      nodeBorder: fg,
      clusterBkg: bg,
      clusterBorder: line,
      edgeLabelBackground: bg,
      actorBkg: box,
      actorBorder: fg,
      actorTextColor: fg,
      signalColor: fg,
      signalTextColor: fg,
      labelBoxBkgColor: box,
      labelTextColor: fg,
      noteBkgColor: box,
      noteTextColor: fg,
      noteBorderColor: line,
      fontFamily: v("--font", "sans-serif"),
    },
    htmlLabels: false,
    flowchart: { htmlLabels: false },
  });
  return m;
}

/** One diagram as sanitized SVG, or the error as text. */
export async function renderMermaid(src) {
  try {
    const { svg } = await (await mermaid()).render(`coxswain-mermaid-${++mermaidSeq}`, src);
    return `<div class="diagram">${clean(svg, { styles: true })}</div>`;
  } catch (e) {
    return `<pre class="diagram-error">${escape(String(e?.message ?? e))}</pre>`;
  }
}

/** Markdown with $math$ (KaTeX) and ```mermaid blocks drawn as diagrams. */
export async function renderMarkdown(src) {
  if (/\$|\\\(|\\\[/.test(src)) await withMath();
  const html = clean(await marked.parse(src));
  // Most documents have no diagram: no second parse and sanitizing for them.
  if (!html.includes('class="language-mermaid"')) return html;
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const code of doc.querySelectorAll("pre > code.language-mermaid")) {
    // renderMermaid's result is already sanitized; parse it in an inert document too.
    const svg = new DOMParser().parseFromString(await renderMermaid(code.textContent), "text/html");
    code.parentElement.replaceWith(...doc.adoptNode(svg.body).childNodes);
  }
  return clean(doc.body.innerHTML, { styles: true });
}

export { highlight };

/** Highlighting on a worker, for a file's text or diff: a big one takes the page's thread for
 *  hundreds of milliseconds. A newer call drops the one still running (it resolves to null),
 *  so moving on from a big file costs nothing. Null too when the worker fails. */
let hlWorker = null;
let hlPending = null;
export function highlightOff(src, lang) {
  if (hlPending) {
    hlWorker.terminate();
    hlWorker = null;
    hlPending(null);
  }
  hlWorker ??= new Worker(new URL("./hl.worker.js", import.meta.url), { type: "module" });
  return new Promise((resolve) => {
    const done = (v) => {
      hlPending = null;
      resolve(v);
    };
    hlPending = resolve;
    hlWorker.onmessage = (ev) => done(ev.data);
    hlWorker.onerror = () => done(null);
    hlWorker.postMessage([src, lang]);
  });
}

// ------------------------------------------------------------ documents

/** Word .docx as HTML (text, headings, lists, tables, embedded images). */
export async function renderDocx(path) {
  const { default: mammoth } = await import("mammoth");
  const r = await mammoth.convertToHtml({ arrayBuffer: await bytes(path) });
  return clean(r.value);
}

/** A workbook (xlsx, xls, ods, csv, tsv): sheet names, and rows per sheet on demand. */
export async function readSheet(path) {
  const XLSX = await import("xlsx");
  const wb = XLSX.read(await bytes(path), { type: "array", sheetRows: 201, dense: true });
  return {
    names: wb.SheetNames,
    rows: (name) => XLSX.utils.sheet_to_json(wb.Sheets[name], { header: 1, raw: false, defval: "" }),
  };
}

/** A Jupyter notebook: markdown cells, highlighted code, and text, image and HTML outputs. */
export async function renderNotebook(path) {
  const nb = JSON.parse(new TextDecoder().decode(await bytes(path)));
  const lang = nb.metadata?.kernelspec?.language ?? nb.metadata?.language_info?.name ?? "python";
  const join = (v) => (Array.isArray(v) ? v.join("") : (v ?? ""));
  const parts = [];
  for (const cell of nb.cells ?? []) {
    const src = join(cell.source);
    if (cell.cell_type === "markdown") {
      parts.push(`<div class="cell md">${await renderMarkdown(src)}</div>`);
      continue;
    }
    if (cell.cell_type !== "code") continue;
    const n = cell.execution_count ?? " ";
    parts.push(`<div class="cell code"><span class="prompt">In [${escape(String(n))}]</span><pre class="hljs"><code>${highlight(src, lang)}</code></pre></div>`);
    for (const out of cell.outputs ?? []) {
      const data = out.data ?? {};
      if (out.output_type === "stream") parts.push(`<pre class="out">${escape(join(out.text))}</pre>`);
      else if (out.output_type === "error") parts.push(`<pre class="out err">${escape(`${out.ename}: ${out.evalue}`)}</pre>`);
      else if (data["image/png"]) parts.push(`<img class="out" src="data:image/png;base64,${escape(join(data["image/png"]).trim())}">`);
      else if (data["image/svg+xml"]) parts.push(`<div class="out">${clean(join(data["image/svg+xml"]))}</div>`);
      else if (data["text/html"]) parts.push(`<div class="out">${clean(join(data["text/html"]))}</div>`);
      else if (data["text/plain"]) parts.push(`<pre class="out">${escape(join(data["text/plain"]))}</pre>`);
    }
  }
  // Every part is sanitized or escaped already; a markdown cell's diagram keeps its styles.
  return clean(parts.join(""), { styles: true });
}

// ------------------------------------------------------------ fonts

let fontSeq = 0;
/** Load a font file under a fresh family name: that name, and the face, which the preview
 *  that loaded it removes from `document.fonts` when it shows something else. */
export async function loadFont(path) {
  const family = `coxswain-preview-${++fontSeq}`;
  const face = new FontFace(family, `url("${convertFileSrc(path)}")`);
  document.fonts.add(await face.load());
  return { family, face };
}

// ------------------------------------------------------------ structured data

/** JSON, YAML or TOML text as a plain value, for the tree view. */
export async function parseData(src, ext) {
  if (ext === "json" || ext === "geojson") return JSON.parse(src);
  if (ext === "yaml" || ext === "yml") return (await import("yaml")).parse(src);
  if (ext === "toml") return (await import("smol-toml")).parse(src);
  throw new Error(t("render.no_tree", { ext }));
}

// ------------------------------------------------------------ calendar and contacts

/** RFC 5545 / 6350 content lines, unfolded: [{ name, params, value }]. */
function contentLines(src) {
  return src
    .replace(/\r?\n[ \t]/g, "")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((l) => {
      const i = l.indexOf(":");
      const [name, ...params] = l.slice(0, i).split(";");
      const value = l.slice(i + 1).replace(/\\n/gi, "\n").replace(/\\([,;\\])/g, "$1");
      return { name: name.toUpperCase(), params: params.join(";"), value };
    });
}

/** 20260928T100000Z -> "2026-09-28 10:00 UTC"; all-day 20260928 -> "2026-09-28". */
function icalDate(v) {
  const m = /^(\d{4})(\d{2})(\d{2})(?:T(\d{2})(\d{2}))?(Z)?/.exec(v ?? "");
  if (!m) return v ?? "";
  return `${m[1]}-${m[2]}-${m[3]}${m[4] ? ` ${m[4]}:${m[5]}` : ""}${m[6] ? " UTC" : ""}`;
}

/** Events of an .ics file, in file order. */
export function calendar(src) {
  const events = [];
  let cur = null;
  for (const { name, value } of contentLines(src)) {
    if (name === "BEGIN" && value === "VEVENT") cur = {};
    else if (name === "END" && value === "VEVENT" && cur) events.push(cur), (cur = null);
    else if (cur) cur[name] ??= value;
  }
  return events.map((e) => ({ title: e.SUMMARY ?? t("render.no_title"), start: icalDate(e.DTSTART), end: icalDate(e.DTEND), where: e.LOCATION ?? "", note: e.DESCRIPTION ?? "" }));
}

/** Cards of a .vcf file. */
export function contacts(src) {
  const cards = [];
  let cur = null;
  for (const { name, value } of contentLines(src)) {
    if (name === "BEGIN" && value.toUpperCase() === "VCARD") cur = { email: [], tel: [] };
    else if (name === "END" && cur) cards.push(cur), (cur = null);
    else if (cur && name === "FN") cur.name = value;
    else if (cur && name === "ORG") cur.org = value.replaceAll(";", ", ");
    else if (cur && name === "TITLE") cur.title = value;
    else if (cur && name === "EMAIL") cur.email.push(value);
    else if (cur && name === "TEL") cur.tel.push(value);
  }
  return cards;
}

// ------------------------------------------------------------ Graphviz, AsciiDoc, Parquet

let vizReady;
/** A Graphviz graph (.dot, .gv) as sanitized SVG, drawn by Graphviz compiled to WebAssembly. */
export async function renderGraphviz(src) {
  vizReady ??= import("@viz-js/viz").then((m) => m.instance());
  try {
    return `<div class="diagram">${clean((await vizReady).renderString(src, { format: "svg" }))}</div>`;
  } catch (e) {
    return `<pre class="diagram-error">${escape(String(e?.message ?? e))}</pre>`;
  }
}

/** A PowerPoint deck's slides as HTML, drawn in the browser at once (layout, text, pictures,
 *  tables; not every font, effect or chart). LibreOffice, where there is one, follows with an
 *  exact rendering. */
export async function renderPptx(path) {
  const [{ pptxToHtml }, { default: JSZip }] = await Promise.all([import("@jvmr/pptx-to-html"), import("jszip")]);
  const slides = await pptxToHtml(await mendPptx(JSZip, await bytes(path)), { width: 960, height: 540, scaleToFit: true, letterbox: false });
  return slides.map(clean);
}

/** Files saved by PowerPoint for the web start their XML parts with a byte order mark, which the
 *  browser's XML parser refuses, and name parts by absolute paths (`/ppt/slides/slide1.xml`)
 *  where the library expects relative ones. Both are mended before the library sees them. */
async function mendPptx(JSZip, buffer) {
  const zip = await JSZip.loadAsync(buffer);
  let changed = false;
  for (const name of Object.keys(zip.files).filter((n) => /\.(xml|rels)$/.test(n))) {
    const before = await zip.file(name).async("string");
    let xml = before.replace(/^\uFEFF/, "");
    if (name.endsWith(".rels") && xml.includes('Target="/')) {
      // "ppt/slides/_rels/slide1.xml.rels" describes "ppt/slides/slide1.xml": targets are relative to "ppt/slides/".
      const base = name.replace(/_rels\/[^/]*$/, "").split("/").filter(Boolean);
      xml = xml.replace(/Target="\/([^"]*)"/g, (_, abs) => {
        const to = abs.split("/");
        let i = 0;
        while (i < base.length && i < to.length - 1 && base[i] === to[i]) i++;
        return `Target="${[...Array(base.length - i).fill(".."), ...to.slice(i)].join("/")}"`;
      });
    }
    if (xml !== before) {
      zip.file(name, xml);
      changed = true;
    }
  }
  return changed ? zip.generateAsync({ type: "arraybuffer" }) : buffer;
}

/** An HTML file as a page to show in a sandboxed frame: no script runs (the frame allows none)
 *  and nothing comes from the web (the page's content policy allows only the folder it is in),
 *  so a downloaded page cannot run code or report that it was opened. Its own pictures,
 *  styles and fonts, next to it, come through the app's file protocol. */
export function renderHtml(src, path) {
  return pageHead(convertFileSrc(path), path) + src;
}

/** What goes before an HTML file's own text in the frame: its content security policy and
 *  its base. `fileUrl` is the file through the app's file protocol, `path` the file on disk.
 *  The policy names the page's folder (and what is below it), not the whole file protocol,
 *  so `../private/photo.jpg` or `/home/me/keys.png` is not shown. */
export function pageHead(fileUrl, path) {
  const u = new URL(fileUrl);
  const origin = `${u.protocol}//${u.host}`;
  // The folder, one segment at a time, so the page's relative links resolve beside it. A leading
  // `/` is kept, encoded: the protocol takes the path after the host's `/`, and without it a
  // stylesheet's path would be relative.
  const lead = path.startsWith("/") ? "%2F" : "";
  const dir = lead + path.split(/[\\/]/).slice(0, -1).filter(Boolean).map(encodeURIComponent).join("/");
  const base = `${origin}/${dir}/`;
  const csp = `default-src 'none'; img-src ${base} data:; style-src ${base} 'unsafe-inline'; font-src ${base} data:; media-src ${base}`;
  // Before the page's own head: the parser puts them in the head it makes, and the page cannot undo them.
  return `<meta http-equiv="Content-Security-Policy" content="${csp}"><base href="${base}">`;
}

let drawioReady;
/** A draw.io diagram drawn into `el` by draw.io's own viewer (gui/public/vendor/drawio), loaded on
 *  first use. It works offline: shapes from draw.io's extra libraries (AWS, Azure, …) that it
 *  would fetch from diagrams.net show as plain boxes. The pages are tabs of the preview, which
 *  picks one with the viewer this returns (`selectPage`). */
export async function renderDrawio(el, xml) {
  drawioReady ??= new Promise((ok, fail) => {
    const local = "vendor/drawio/none";
    // Every path the viewer would otherwise take from diagrams.net: nothing is fetched from there.
    Object.assign(window, { STENCIL_PATH: local, SHAPES_PATH: local, STYLE_PATH: local, GRAPH_IMAGE_PATH: local, PROXY_URL: local, DRAW_MATH_URL: local, DRAWIO_LIGHTBOX_URL: local, mxImageBasePath: local, mxBasePath: local, mxLoadResources: false, mxLoadStylesheets: false });
    const s = document.createElement("script");
    s.src = "vendor/drawio/viewer-static.min.js";
    s.onload = () => {
      // No MathJax here: the viewer would load DRAW_MATH_URL's script, get the app's index.html
      // back (a SyntaxError) and keep a diagram with maths hidden for good. This is its own
      // "maths did not load" path: the labels show their TeX as text.
      window.Editor.MathJaxRender = () => window.Editor.onMathJaxDone();
      ok();
    };
    s.onerror = () => fail(new Error("draw.io viewer"));
    document.head.append(s);
  });
  await drawioReady;
  el.replaceChildren();
  const doc = window.mxUtils.parseXml(xml);
  return new window.GraphViewer(el, doc.documentElement, { nav: true, lightbox: false, toolbar: "zoom layers", "toolbar-nohide": true, "toolbar-position": "top", resize: true, center: true });
}

/** The names of a draw.io diagram's pages, in order; "" for one without a name. */
export function drawioPages(xml) {
  const doc = new DOMParser().parseFromString(xml, "text/xml");
  return [...doc.getElementsByTagName("diagram")].map((d) => d.getAttribute("name") ?? "");
}

/** AsciiDoc as HTML, in Asciidoctor's secure mode (no file includes), sanitized. Its diagram
 *  blocks (`[plantuml]`, `[d2]`, `[mermaid]` … over `----`, found by the core) are drawn:
 *  Mermaid and Graphviz here, the rest by their programs, installed or in an image already
 *  pulled, as pictures; one that cannot be drawn shows its source and why. */
export async function renderAsciidoc(src) {
  const [{ convert }, { source, blocks }] = await Promise.all([import("@asciidoctor/core"), invoke("adoc_blocks", { text: src })]);
  const html = clean(await convert(source, { safe: "secure", attributes: { showtitle: true } }));
  if (!blocks.length) return html;
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const el of doc.querySelectorAll("div.cx-diagram[data-n]")) {
    const b = blocks[Number(el.dataset.n)];
    if (!b) continue;
    let drawn;
    if (b.kind === "mermaid") drawn = await renderMermaid(b.text);
    else if (b.kind === "graphviz" || b.kind === "dot") drawn = await renderGraphviz(b.text);
    else
      drawn = await invoke("adoc_diagram", { kind: b.kind, text: b.text }).then(
        // A picture, never inline SVG: its styles and scripts stay inside it.
        (svg) => `<div class="diagram"><img src="data:image/svg+xml;base64,${base64(svg)}" alt="${escape(b.kind)}"></div>`,
        (err) => `<pre class="diagram-source">${escape(b.text)}</pre><p class="diagram-error">${escape(String(err?.message ?? err))}</p>`,
      );
    const part = new DOMParser().parseFromString(drawn, "text/html");
    el.replaceWith(...doc.adoptNode(part.body).childNodes);
  }
  return clean(doc.body.innerHTML, { styles: true });
}

/** A Parquet file: its column schema, row count and the first 200 rows as a table. */
export async function readParquet(path) {
  const [{ parquetMetadata, parquetReadObjects }, { compressors }] = await Promise.all([import("hyparquet"), import("hyparquet-compressors")]);
  const file = await bytes(path, 500 * 1024 * 1024);
  const meta = parquetMetadata(file);
  const objs = await parquetReadObjects({ file, rowEnd: 200, compressors });
  const cols = objs.length ? Object.keys(objs[0]) : meta.schema.slice(1).map((s) => s.name);
  const cell = (v) => (v === null || v === undefined ? "" : typeof v === "object" ? JSON.stringify(v, (_, x) => (typeof x === "bigint" ? x.toString() : x)) : String(v));
  const schema = meta.schema.slice(1).map((s) => [s.name, s.type ?? "group", s.repetition_type ?? ""]);
  return { rows: Number(meta.num_rows), schema, table: [cols, ...objs.map((o) => cols.map((c) => cell(o[c])))] };
}

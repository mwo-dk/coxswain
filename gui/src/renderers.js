// Rich previews. Each heavy library is imported the first time a file needs it, so none of
// them cost anything at startup. Everything here renders untrusted file content, so every
// HTML result goes through DOMPurify before it reaches the DOM.

import { marked } from "marked";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import { convertFileSrc } from "./lib.js";
import { t } from "./i18n.svelte.js";

export const clean = (html) => DOMPurify.sanitize(html, { USE_PROFILES: { html: true, svg: true, svgFilters: true, mathMl: true } });
const escape = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/** File bytes through the asset protocol; refuses files too big to preview. */
export async function bytes(path, max = 25 * 1024 * 1024) {
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
    return `<div class="diagram">${clean(svg)}</div>`;
  } catch (e) {
    return `<pre class="diagram-error">${escape(String(e?.message ?? e))}</pre>`;
  }
}

/** Markdown with $math$ (KaTeX) and ```mermaid blocks drawn as diagrams. */
export async function renderMarkdown(src) {
  if (/\$|\\\(|\\\[/.test(src)) await withMath();
  const doc = new DOMParser().parseFromString(clean(await marked.parse(src)), "text/html");
  for (const code of doc.querySelectorAll("pre > code.language-mermaid")) {
    // renderMermaid's result is already sanitized; parse it in an inert document too.
    const svg = new DOMParser().parseFromString(await renderMermaid(code.textContent), "text/html");
    code.parentElement.replaceWith(...doc.adoptNode(svg.body).childNodes);
  }
  return clean(doc.body.innerHTML);
}

export const highlight = (src, lang) =>
  hljs.getLanguage(lang ?? "") ? hljs.highlight(src, { language: lang }).value : hljs.highlightAuto(src).value;

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
    parts.push(`<div class="cell code"><span class="prompt">In [${n}]</span><pre class="hljs"><code>${highlight(src, lang)}</code></pre></div>`);
    for (const out of cell.outputs ?? []) {
      const data = out.data ?? {};
      if (out.output_type === "stream") parts.push(`<pre class="out">${escape(join(out.text))}</pre>`);
      else if (out.output_type === "error") parts.push(`<pre class="out err">${escape(`${out.ename}: ${out.evalue}`)}</pre>`);
      else if (data["image/png"]) parts.push(`<img class="out" src="data:image/png;base64,${join(data["image/png"]).trim()}">`);
      else if (data["image/svg+xml"]) parts.push(`<div class="out">${clean(join(data["image/svg+xml"]))}</div>`);
      else if (data["text/html"]) parts.push(`<div class="out">${clean(join(data["text/html"]))}</div>`);
      else if (data["text/plain"]) parts.push(`<pre class="out">${escape(join(data["text/plain"]))}</pre>`);
    }
  }
  return clean(parts.join(""));
}

// ------------------------------------------------------------ fonts

let fontSeq = 0;
/** Load a font file under a fresh family name and return that name. */
export async function loadFont(path) {
  const family = `coxswain-preview-${++fontSeq}`;
  const face = new FontFace(family, `url("${convertFileSrc(path)}")`);
  document.fonts.add(await face.load());
  return family;
}

// ------------------------------------------------------------ structured data

/** JSON, YAML or TOML text as a plain value, for the tree view. */
export async function parseData(src, ext) {
  if (ext === "json" || ext === "geojson") return JSON.parse(src);
  if (ext === "yaml" || ext === "yml") return (await import("yaml")).parse(src);
  if (ext === "toml") return (await import("smol-toml")).parse(src);
  throw new Error(t("render.no_tree", { ext }));
}

/** JSON Lines: up to 200 objects as rows, with the union of their keys as columns. */
export function jsonLines(src) {
  const objs = src
    .split("\n")
    .filter((l) => l.trim())
    .slice(0, 200)
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch {
        return { [t("render.not_json")]: l };
      }
    });
  const cols = [...new Set(objs.flatMap((o) => (o && typeof o === "object" && !Array.isArray(o) ? Object.keys(o) : [t("render.value")])))];
  const cell = (v) => (v === undefined ? "" : typeof v === "object" ? JSON.stringify(v) : String(v));
  const rows = objs.map((o) => cols.map((c) => cell(o && typeof o === "object" && !Array.isArray(o) ? o[c] : o)));
  return [cols, ...rows];
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

// ------------------------------------------------------------ logs

const LEVELS = [
  [/\b(FATAL|CRIT(ICAL)?|ERROR|ERR|PANIC)\b/, "lv-error"],
  [/\b(WARN(ING)?)\b/, "lv-warn"],
  [/\b(INFO|NOTICE)\b/, "lv-info"],
  [/\b(DEBUG|TRACE|VERBOSE)\b/, "lv-debug"],
];

/** A log file with each line coloured by its level. */
export function logLines(src) {
  return src
    .split("\n")
    .map((l) => {
      const cls = LEVELS.find(([re]) => re.test(l))?.[1];
      return cls ? `<span class="${cls}">${escape(l)}</span>` : escape(l);
    })
    .join("\n");
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
  const u = new URL(convertFileSrc(path));
  const origin = `${u.protocol}//${u.host}`;
  // The folder, one segment at a time, so the page's relative links resolve beside it.
  const dir = path.split(/[\\/]/).slice(0, -1).filter(Boolean).map(encodeURIComponent).join("/");
  const base = `${origin}/${dir}/`;
  const csp = `default-src 'none'; img-src ${origin} data:; style-src ${origin} 'unsafe-inline'; font-src ${origin} data:; media-src ${origin}`;
  // Before the page's own head: the parser puts them in the head it makes, and the page cannot undo them.
  return `<meta http-equiv="Content-Security-Policy" content="${csp}"><base href="${base}">${src}`;
}

let drawioReady;
/** A draw.io diagram drawn into `el` by draw.io's own viewer (gui/public/vendor/drawio), loaded on
 *  first use. It works offline: shapes from draw.io's extra libraries (AWS, Azure, …) that it
 *  would fetch from diagrams.net show as plain boxes. Several pages get a page switcher. */
export async function renderDrawio(el, xml) {
  drawioReady ??= new Promise((ok, fail) => {
    const local = "vendor/drawio/none";
    // Every path the viewer would otherwise take from diagrams.net: nothing is fetched from there.
    Object.assign(window, { STENCIL_PATH: local, SHAPES_PATH: local, STYLE_PATH: local, GRAPH_IMAGE_PATH: local, PROXY_URL: local, DRAW_MATH_URL: local, mxImageBasePath: local, mxBasePath: local, mxLoadResources: false, mxLoadStylesheets: false });
    const s = document.createElement("script");
    s.src = "vendor/drawio/viewer-static.min.js";
    s.onload = ok;
    s.onerror = () => fail(new Error("draw.io viewer"));
    document.head.append(s);
  });
  await drawioReady;
  el.replaceChildren();
  const doc = window.mxUtils.parseXml(xml);
  new window.GraphViewer(el, doc.documentElement, { nav: true, lightbox: false, toolbar: "pages zoom layers", "toolbar-nohide": true, "toolbar-position": "top", resize: true, center: true });
}

/** AsciiDoc as HTML, in Asciidoctor's secure mode (no file includes), sanitized. */
export async function renderAsciidoc(src) {
  const { convert } = await import("@asciidoctor/core");
  return clean(await convert(src, { safe: "secure", attributes: { showtitle: true } }));
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

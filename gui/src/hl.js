// Syntax highlighting, shared by the page and its highlighting worker (hl.worker.js).
import hljs from "highlight.js/lib/common";

export const highlight = (src, lang) =>
  hljs.getLanguage(lang ?? "") ? hljs.highlight(src, { language: lang }).value : hljs.highlightAuto(src).value;

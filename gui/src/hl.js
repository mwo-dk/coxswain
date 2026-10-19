// Syntax highlighting, shared by the page and its highlighting worker (hl.worker.js). The
// common languages are in the bundle; the worker loads any other first (grammars.js).
import hljs from "highlight.js/lib/common";

export { hljs };

/** HTML of `src` highlighted as `lang` when its grammar is loaded, else in the language
 *  guessed among the common ones. */
export const highlight = (src, lang) =>
  hljs.getLanguage(lang ?? "") ? hljs.highlight(src, { language: lang }).value : hljs.highlightAuto(src, COMMON).value;

/** The languages a file's language is guessed among: the bundle's (highlight.js's common set),
 *  so a guess costs the same however many grammars earlier files loaded. */
const COMMON = hljs.listLanguages();

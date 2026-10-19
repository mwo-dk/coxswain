// Every highlight.js language beyond the common ones, and the five grammars added for what it
// lacks, each a chunk of its own that the highlighting worker loads the first time a file needs
// it. The core (`code::language`) says which grammar a file takes.
import { hljs } from "./hl.js";

/** highlight.js's other languages, by name: each a lazy chunk. */
const BUILT_IN = Object.fromEntries(
  // The common ones are in the bundle already (hl.js): no chunk of their own.
  Object.entries(import.meta.glob([
    "../node_modules/highlight.js/es/languages/*.js",
    "!**/*.js.js",
    "!**/languages/xml.js", "!**/languages/bash.js", "!**/languages/c.js", "!**/languages/cpp.js", "!**/languages/csharp.js", "!**/languages/css.js", "!**/languages/markdown.js", "!**/languages/diff.js", "!**/languages/ruby.js", "!**/languages/go.js", "!**/languages/graphql.js", "!**/languages/ini.js", "!**/languages/java.js", "!**/languages/javascript.js", "!**/languages/json.js", "!**/languages/kotlin.js", "!**/languages/less.js", "!**/languages/lua.js", "!**/languages/makefile.js", "!**/languages/perl.js", "!**/languages/objectivec.js", "!**/languages/php.js", "!**/languages/php-template.js", "!**/languages/plaintext.js", "!**/languages/python.js", "!**/languages/python-repl.js", "!**/languages/r.js", "!**/languages/rust.js", "!**/languages/scss.js", "!**/languages/shell.js", "!**/languages/sql.js", "!**/languages/swift.js", "!**/languages/yaml.js", "!**/languages/typescript.js", "!**/languages/vbnet.js", "!**/languages/wasm.js",
  ])).map(([path, load]) => [
    path.slice(path.lastIndexOf("/") + 1, -3),
    async () => (await load()).default,
  ]),
);

/** Grammars for languages highlight.js lacks (npm packages: gui/package.json). */
const ADDED = {
  cobol: async () => (await import("highlightjs-cobol/dist/cobol.es.min.js")).default,
  zig: async () => (await import("highlightjs-zig/src/languages/zig.js")).default,
  solidity: async () => (await import("highlightjs-solidity/dist/solidity.es.min.js")).default,
  terraform: async () => (await import("@taga3s/highlightjs-terraform")).definer,
  // Registers itself under its name.
  svelte: async () => {
    (await import("highlightjs-svelte")).default({ registerLanguage: (_, grammar) => (svelteGrammar = grammar) });
    return svelteGrammar;
  },
};
let svelteGrammar;

/** Load the grammar `lang` (a highlight.js name or alias) if it is not yet. Never throws: a
 *  grammar that fails to load leaves the text to be guessed. */
export async function load(lang) {
  if (!lang || hljs.getLanguage(lang)) return;
  const get = ADDED[lang] ?? BUILT_IN[lang];
  if (!get) return;
  try {
    hljs.registerLanguage(lang, await get());
  } catch {
    // Left to the guess.
  }
}

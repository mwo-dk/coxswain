// Highlights text off the page's thread: [source, language] in, HTML out. The language's
// grammar is loaded first when it is not in the bundle.
import { highlight } from "./hl.js";
import { load } from "./grammars.js";

onmessage = async ({ data: [src, lang] }) => {
  await load(lang);
  postMessage(highlight(src, lang));
};

// Highlights text off the page's thread: [source, language] in, HTML out.
import { highlight } from "./hl.js";

onmessage = ({ data: [src, lang] }) => postMessage(highlight(src, lang));

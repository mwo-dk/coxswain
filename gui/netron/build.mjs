// Builds gui/public/vendor/netron from a pinned Netron release (docs/previews/neural-networks.md):
//   node gui/netron/build.mjs            (from the repository, after `npm ci` in gui)
// It downloads the release's source archive, checks its SHA-256, and bundles host.js with the
// Netron modules of ONNX, TensorFlow Lite, Core ML and Keras into one classic script
// (netron.js), the four formats' operator descriptions inside it. Any other Netron module
// (Python, pickles, PyTorch, …) is replaced by stub.js, and the build fails when one of the
// left-out modules or a web address of Netron's telemetry ends up in the bundle anyway.
// Netron's telemetry class (base.js) is cut out as it is bundled.
// index.html is Netron's, with its script line and its Content-Security-Policy changed.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, copyFileSync, mkdirSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "rolldown";

const VERSION = "9.3.2";
const SHA256 = "6e3f0f334f7d033de58293cebe6d39e2d495cba13fcc9842447c18635de9855b";
// The app's own address on each system: tauri://localhost (Linux, macOS, the BSDs), http://tauri.localhost (Windows).
const SELF = "tauri://localhost http://tauri.localhost";
const URL = `https://github.com/lutzroeder/netron/archive/refs/tags/v${VERSION}.tar.gz`;

// The Netron modules that are bundled; any other one is stub.js.
const KEEP = new Set([
  "base", "view", "grapher", "dagre", "text", "json", "xml", "protobuf", "flatbuffers", "flexbuffers", "zip", "tar", "hdf5",
  "onnx", "onnx-proto", "onnx-schema", "tflite", "tflite-schema", "coreml", "coreml-proto", "keras", "keras-proto",
]);

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(here, "../public/vendor/netron");
const work = mkdtempSync(join(tmpdir(), "coxswain-netron-"));
try {
  const archive = join(work, "netron.tar.gz");
  execFileSync("curl", ["-sSfL", "-o", archive, URL]);
  const sum = createHash("sha256").update(readFileSync(archive)).digest("hex");
  if (sum !== SHA256) throw new Error(`netron v${VERSION}: SHA-256 ${sum}, expected ${SHA256}`);
  execFileSync("tar", ["-xzf", archive, "-C", work]);
  const src = join(work, `netron-${VERSION}`, "source");
  const stub = join(here, "stub.js");

  await build({
    input: join(here, "host.js"),
    platform: "browser",
    transform: { define: { NETRON_VERSION: JSON.stringify(VERSION) } },
    plugins: [
      {
        name: "netron",
        resolveId(id, importer) {
          if (id.startsWith("netron/")) return join(src, id.slice(7));
          if (!importer || !importer.startsWith(src)) return null;
          if (!id.startsWith("./")) return stub;
          const name = basename(id).replace(/\.js$/, "");
          return KEEP.has(name) && existsSync(join(src, `${name}.js`)) ? join(src, `${name}.js`) : stub;
        },
        // The operator descriptions, as one string each, without white space.
        load(id) {
          // Netron's Google Analytics client, which only its browser host starts: removed.
          if (id === join(src, "base.js")) {
            const code = readFileSync(id, "utf8");
            const cut = code.replace(/^base\.Telemetry = class \{[\s\S]*?^\};$/m, 'base.Telemetry = class { constructor() { throw new Error("No telemetry in Coxswain."); } };');
            if (cut === code) throw new Error("base.js no longer has base.Telemetry");
            return cut;
          }
          if (id.endsWith(".json")) return { code: `export default ${JSON.stringify(JSON.stringify(JSON.parse(readFileSync(id, "utf8"))))};`, moduleType: "js" };
          return null;
        },
      },
    ],
    output: { file: join(out, "netron.js"), format: "iife", minify: true, codeSplitting: false, comments: { legal: false } },
  });

  const js = readFileSync(join(out, "netron.js"), "utf8");
  for (const bad of ["google-analytics.com", "ipinfo.io", "sendBeacon", "XMLHttpRequest", "pytorch.ModelFactory", "pickle.Unpickler = "]) {
    if (js.includes(bad)) throw new Error(`netron.js holds ${bad}`);
  }

  let html = readFileSync(join(src, "index.html"), "utf8");
  const csp = `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src ${SELF}; style-src ${SELF} 'unsafe-inline'; img-src data: blob:; font-src 'none'; connect-src 'none'; frame-src 'none'; worker-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'">`;
  const swap = (from, to) => {
    if (!html.includes(from)) throw new Error(`index.html no longer has ${from}`);
    html = html.replace(from, to);
  };
  swap(`<meta http-equiv="Content-Security-Policy" content="script-src 'self'; frame-src 'none'">`, csp);
  // Its style sheet to a file of its own: the app's policy hashes inline ones, and a frame with
  // no origin is not 'self', so both policies name the app's own address instead.
  const style = html.match(/<style>\n([\s\S]*?)<\/style>\n/);
  if (!style) throw new Error("index.html no longer has its <style>");
  html = html.replace(style[0], `<link rel="stylesheet" type="text/css" href="index.css">\n`);
  // Netron's menu (open, export) is not used: the app opens files, and nothing is saved from the frame.
  writeFileSync(join(out, "index.css"), `${style[1]}.menu-button { display: none !important; }\n`);
  swap(`<script type="text/javascript" src="index.js"></script>`, `<script defer src="netron.js"></script>`);
  swap(`<meta name="version" content="0.0.0">`, `<meta name="version" content="${VERSION}">`);
  // Its icons are not shipped: a frame has no tab to show them in.
  html = html.replace(/^<link rel="[^"]*icon[^"]*"[^>]*>\n/gm, "");
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, "index.html"), html);
  copyFileSync(join(src, "grapher.css"), join(out, "grapher.css"));
  copyFileSync(join(work, `netron-${VERSION}`, "LICENSE"), join(out, "LICENSE"));
  writeFileSync(
    join(out, "README"),
    `Netron ${VERSION} (https://github.com/lutzroeder/netron, tag v${VERSION}, source archive SHA-256 ${SHA256}),\n` +
      `MIT licence, see LICENSE. Made by gui/netron/build.mjs: netron.js bundles Coxswain's host (gui/netron/host.js)\n` +
      `with Netron's unmodified modules for ONNX, TensorFlow Lite, Core ML and Keras; index.html and grapher.css are\n` +
      `Netron's, index.html with its script line, version and Content-Security-Policy changed.\n`,
  );
  console.log(`netron ${VERSION}: ${(js.length / 1048576).toFixed(2)} MB`);
} finally {
  rmSync(work, { recursive: true, force: true });
}

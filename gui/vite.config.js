import { defineConfig, searchForWorkspaceRoot } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { readFileSync, rmSync } from "node:fs";
import { resolve } from "node:path";

// Modules left out of the build (docs/customise/modules.md): `COXSWAIN_MODULES_OUT=diagrams,data
// npm run build` leaves their libraries out of the bundle (each `__MODULE_<ID>__` is false, so the
// import behind it is dropped) and their viewers out of `dist/vendor`. The page treats them as
// off (`lib.js`, `__MODULES_OUT__`). The names are coxswain-core's features, read from its
// Cargo.toml, so a misspelt one fails the build.
const core = readFileSync(new URL("../crates/coxswain-core/Cargo.toml", import.meta.url), "utf8");
const MODULES = JSON.parse(core.match(/^modules = (\[.*\])$/m)[1]);
const OUT = (process.env.COXSWAIN_MODULES_OUT ?? "").split(",").map((s) => s.trim()).filter(Boolean);
for (const m of OUT) if (!MODULES.includes(m)) throw new Error(`COXSWAIN_MODULES_OUT: no module "${m}" (${MODULES.join(", ")})`);
let outDir;
const VENDOR = { diagrams: ["drawio"], neural_networks: ["netron"], games: ["chess-pieces", "game-pieces"] };

// Tauri expects a fixed port and does not want the screen cleared.
export default defineConfig({
  plugins: [
    svelte(),
    {
      name: "modules-out",
      apply: "build",
      configResolved(c) {
        outDir = resolve(c.root, c.build.outDir);
      },
      closeBundle() {
        for (const m of OUT) for (const v of VENDOR[m] ?? []) rmSync(resolve(outDir, "vendor", v), { recursive: true, force: true });
      },
    },
  ],
  define: {
    __MODULES_OUT__: JSON.stringify(OUT),
    ...Object.fromEntries(MODULES.map((m) => [`__MODULE_${m.toUpperCase()}__`, JSON.stringify(!OUT.includes(m))])),
  },
  clearScreen: false,
  // Persian's flag is drawn by us and kept in docs/flags, outside gui/.
  server: { port: 1420, strictPort: true, fs: { allow: [searchForWorkspaceRoot(process.cwd()), "../docs/flags"] } },
  // The big chunks (Mermaid and its optional ELK layout, SheetJS, KaTeX) load only when a
  // preview needs them.
  build: { target: "es2022", chunkSizeWarningLimit: 1500 },
  // The highlighting worker loads a grammar when a file needs it: a module, so it can.
  worker: { format: "es" },
});

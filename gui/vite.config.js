import { defineConfig, searchForWorkspaceRoot } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and does not want the screen cleared.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // Persian's flag is drawn by us and kept in docs/flags, outside gui/.
  server: { port: 1420, strictPort: true, fs: { allow: [searchForWorkspaceRoot(process.cwd()), "../docs/flags"] } },
  // The big chunks (Mermaid and its optional ELK layout, SheetJS, KaTeX) load only when a
  // preview needs them.
  build: { target: "es2022", chunkSizeWarningLimit: 1500 },
});

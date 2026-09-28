import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and does not want the screen cleared.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // The big chunks (Mermaid and its optional ELK layout, SheetJS, KaTeX) load only when a
  // preview needs them.
  build: { target: "es2022", chunkSizeWarningLimit: 1500 },
});

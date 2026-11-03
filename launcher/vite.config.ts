import { fileURLToPath, URL } from "node:url";
import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

/** Tessera Launcher Nightly (TESSERA_NIGHTLY set, like the Rust build): its own name and look. */
const nightly = !!process.env.TESSERA_NIGHTLY;

// Tauri expects a fixed port and doesn't want the terminal cleared.
export default defineConfig({
  plugins: [
    vue(),
    {
      name: "tessera-channel",
      // before the first paint, like the design boot script in index.html
      transformIndexHtml: (html) =>
        nightly ? html.replace('<html lang="en"', '<html lang="en" data-channel="nightly"').replace("<title>Tessera Launcher</title>", "<title>Tessera Launcher Nightly</title>") : html,
    },
  ],
  define: { __NIGHTLY__: JSON.stringify(nightly) },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: { target: "safari15", outDir: "dist" },
});

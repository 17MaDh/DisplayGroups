import { defineConfig } from "vite";

// The UI lives in ./ui; strings come from the shared ../../locales folder. Two pages
// (paths relative to the root): the settings window, and the "Identify" label shown on
// each display (Linux).
export default defineConfig({
  root: "ui",
  base: "./",
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    fs: { allow: ["..", "../.."] },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: { main: "index.html", identify: "identify.html" },
    },
  },
});

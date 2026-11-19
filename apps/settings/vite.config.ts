import { defineConfig } from "vite";

// The UI lives in ./ui; strings come from the shared ../../locales folder.
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
  },
});

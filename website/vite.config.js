import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import { docsPlugin } from "./build/docs.js";

// Build both pages with relative asset links for GitHub Pages and custom domains.
export default defineConfig({
  base: "./",
  plugins: [docsPlugin(fileURLToPath(new URL("./content", import.meta.url)))],
  build: {
    rollupOptions: {
      input: {
        home: fileURLToPath(new URL("./index.html", import.meta.url)),
        docs: fileURLToPath(new URL("./docs.html", import.meta.url)),
      },
    },
  },
});

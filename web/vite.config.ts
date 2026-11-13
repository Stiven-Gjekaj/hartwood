import { defineConfig } from "vite";

// The site lives under /hartwood/ on GitHub Pages, so every path is
// relative to the page.
export default defineConfig({
  base: "./",
  worker: { format: "es" },
  build: { target: "es2022" },
});

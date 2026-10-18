// @ts-check
import { defineConfig } from "astro/config";
import { unified } from "@astrojs/markdown-remark";
import { site } from "./src/site.config.mjs";
import remarkGuideLinks from "./src/lib/remark-guide-links.mjs";

export default defineConfig({
  site: site.url,
  trailingSlash: "always",
  build: { format: "directory" },
  markdown: {
    // The guide's links are rewritten so the same Markdown works on GitHub and here.
    processor: unified({ remarkPlugins: [remarkGuideLinks] }),
    shikiConfig: { theme: "github-dark-dimmed" },
  },
  vite: {
    // The guide's pages and images live in ../docs/guide, outside this folder.
    server: { fs: { allow: [".."] } },
  },
});

import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

// The user guide is published straight from docs/guide, its one source of truth. The pages
// have no front matter; titles come from each page's first heading.
const guide = defineCollection({
  loader: glob({
    pattern: "**/*.md",
    base: "../docs/guide",
    generateId: ({ entry }) => {
      const id = entry.replace(/\\/g, "/").replace(/\.md$/, "");
      return id === "README" ? "index" : id.replace(/\/README$/, "");
    },
  }),
});

// Drift's end-user licence agreement, from LICENSE.md at the repository root (id "license").
const legal = defineCollection({
  loader: glob({ pattern: "LICENSE.md", base: ".." }),
});

export const collections = { guide, legal };

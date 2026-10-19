// Rewrites links in the user guide so the same Markdown works on GitHub and on the site:
//
//   getting-started.md          -> /guide/getting-started/
//   ../reference/units.md#mm    -> /guide/reference/units/#mm
//   README.md                   -> /guide/
//   ../../README.md (outside)   -> the file on GitHub
//
// Images are left alone: Astro resolves relative image paths itself.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { site } from "../site.config.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const guideRoot = path.join(repoRoot, "docs", "guide");

function walk(node, fn) {
  fn(node);
  if (node.children) for (const child of node.children) walk(child, fn);
}

export function rewrite(url, fromFile) {
  if (!url || /^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith("#") || url.startsWith("/")) {
    return url;
  }
  const [target, hash = ""] = url.split("#");
  const abs = path.resolve(path.dirname(fromFile), target);
  const rel = path.relative(guideRoot, abs).split(path.sep).join("/");
  const anchor = hash ? `#${hash}` : "";
  if (rel.startsWith("..")) {
    const repoRel = path.relative(repoRoot, abs).split(path.sep).join("/");
    const kind = /\.[a-z0-9]+$/i.test(repoRel) ? "blob" : "tree";
    return `${site.sourceUrl}/${kind}/main/${repoRel}${anchor}`;
  }
  if (!rel.endsWith(".md")) return url;
  let slug = rel.replace(/\.md$/, "").replace(/(^|\/)README$/, "");
  slug = slug ? `${slug}/` : "";
  return `/guide/${slug}${anchor}`;
}

export default function remarkGuideLinks() {
  return (tree, file) => {
    const from = file.path || file.history?.[0];
    if (!from || !path.resolve(from).startsWith(guideRoot)) return;
    walk(tree, (node) => {
      if (node.type === "link" || node.type === "definition") {
        node.url = rewrite(node.url, path.resolve(from));
      }
    });
  };
}

import { getCollection, type CollectionEntry } from "astro:content";

export type GuideEntry = CollectionEntry<"guide">;

export interface NavItem {
  id: string;
  href: string;
  title: string;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

export function guideHref(id: string): string {
  return id === "index" ? "/guide/" : `/guide/${id}/`;
}

/** A page's title: its first heading, with the "Tutorial 3:"-style prefix kept. */
export function guideTitle(entry: GuideEntry): string {
  const h1 = entry.rendered?.metadata?.headings?.find((h) => h.depth === 1);
  if (h1) return h1.text;
  const m = entry.body?.match(/^#\s+(.+)$/m);
  return m ? m[1].trim() : entry.id;
}

const groupLabels: Record<string, string> = {
  "": "Start here",
  tutorials: "Tutorials",
  reference: "Reference",
  ai: "Drawing with AI",
};

/**
 * The sidebar, in the order the guide's README links its pages, so the guide's contents
 * page stays the one place that decides the order. Pages the README doesn't link come last.
 */
export async function guideNav(): Promise<NavGroup[]> {
  const entries = await getCollection("guide");
  const byId = new Map(entries.map((e) => [e.id, e]));
  const readme = byId.get("index");
  const order: string[] = [];
  for (const m of readme?.body?.matchAll(/\]\(([^)#\s]+\.md)(?:#[^)]*)?\)/g) ?? []) {
    const id = m[1].replace(/^\.\//, "").replace(/\.md$/, "");
    if (byId.has(id) && !order.includes(id)) order.push(id);
  }
  for (const e of entries) {
    if (e.id !== "index" && !order.includes(e.id)) order.push(e.id);
  }
  const groups = new Map<string, NavGroup>();
  groups.set("", { label: groupLabels[""], items: [{ id: "index", href: "/guide/", title: "Introduction" }] });
  for (const id of order) {
    const dir = id.includes("/") ? id.split("/")[0] : "";
    if (!groups.has(dir)) groups.set(dir, { label: groupLabels[dir] ?? dir, items: [] });
    const entry = byId.get(id)!;
    groups.get(dir)!.items.push({ id, href: guideHref(id), title: shortTitle(guideTitle(entry)) });
  }
  return [...groups.values()];
}

/** Sidebar titles drop the "Tutorial 3: " prefix; the numbering is in the list. */
function shortTitle(title: string): string {
  return title.replace(/^Tutorial\s+\d+\s*[:.–-]\s*/i, "");
}

// Screenshots for the site come from the user guide's images, so they're updated in one place.
import type { ImageMetadata } from "astro";

const files = import.meta.glob<{ default: ImageMetadata }>("../../../docs/guide/images/*.png", {
  eager: true,
});

export function screen(name: string): ImageMetadata {
  const hit = Object.entries(files).find(([path]) => path.endsWith(`/${name}.png`));
  if (!hit) throw new Error(`No screenshot docs/guide/images/${name}.png`);
  return hit[1].default;
}

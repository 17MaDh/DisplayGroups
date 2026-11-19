// Group icons: built-in ones are Segoe Fluent glyphs on a coloured tile (the same as
// the OSD); custom `file:` icons are the imported .ico, loaded once as a data: URL.

import { api } from "./api";
import { h, icon } from "./dom";
import { iconGlyph } from "./state";

const images = new Map<string, Promise<string>>();

export function iconTile(name: string): HTMLElement {
  if (name.startsWith("file:")) {
    const img = h("img", { class: "tile image", alt: "" });
    let url = images.get(name);
    if (!url) {
      url = api.iconImage(name);
      images.set(name, url);
    }
    url.then((src) => (img.src = src)).catch(() => images.delete(name));
    return img;
  }
  const { glyph, color } = iconGlyph(name);
  return h("span", { class: "tile", style: `background:${color}` }, icon(glyph));
}

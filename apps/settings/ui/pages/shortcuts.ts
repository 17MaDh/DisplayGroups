// Shortcuts: one click to create a Desktop or Start Menu shortcut for a group.

import { api } from "../api";
import { h, icon } from "../dom";
import { t } from "../i18n";
import { toast } from "../main";
import { iconGlyph, state } from "../state";

async function create(id: string, desktop: boolean, startMenu: boolean): Promise<void> {
  const saved = state.saved.includes(id);
  if (!saved) {
    toast(t("common.saveFirst"), "error");
    return;
  }
  const r = await api.cli({ kind: "shortcut", id, desktop, startMenu });
  toast(r.ok ? `${t("shortcuts.created")}\n${r.output}` : r.output || t("common.failed"), r.ok ? "info" : "error");
}

export function renderShortcuts(): HTMLElement {
  return h(
    "div",
    { class: "page" },
    h("h2", {}, t("nav.shortcuts")),
    h("p", { class: "muted" }, t("shortcuts.intro")),
    ...state.config.groups.map((g) => {
      const { glyph, color } = iconGlyph(g.icon);
      return h(
        "div",
        { class: "setting" },
        h("span", { class: "tile", style: `background:${color};display:grid;place-items:center;inline-size:32px;block-size:32px;border-radius:7px;color:#fff` }, icon(glyph)),
        h("span", { class: "text" }, g.name, h("small", {}, t("shortcuts.target", { name: g.name }))),
        h("button", { class: "btn", onclick: () => void create(g.id, true, false) }, icon(""), t("shortcuts.desktop")),
        h("button", { class: "btn", onclick: () => void create(g.id, false, true) }, icon(""), t("shortcuts.startMenu")),
      );
    }),
    state.config.groups.length ? h("p", { class: "muted" }, t("shortcuts.pinHint")) : h("p", { class: "muted" }, t("groups.empty")),
  );
}

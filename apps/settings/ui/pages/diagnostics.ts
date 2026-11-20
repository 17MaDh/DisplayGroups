// Diagnostics: versions, paths, logs, import/export, what the system sees, config problems.

import { api } from "../api";
import { h, icon, ltr } from "../dom";
import { t } from "../i18n";
import { toast } from "../main";
import { changed, dirty, refreshStatus, reload, state } from "../state";

async function importFile(file: File): Promise<void> {
  if (dirty()) {
    toast(t("common.saveFirst"), "error");
    return;
  }
  const result = await api.importConfig(await file.text());
  if (result.saved) {
    await reload();
    toast(t("diagnostics.imported"));
  } else {
    state.issues = result.issues;
    toast(t("diagnostics.importRejected"), "error");
  }
  changed();
}

export function renderDiagnostics(): HTMLElement {
  const status = state.status;
  const picker = h("input", {
    type: "file",
    accept: ".json,application/json",
    style: "display:none",
    onchange: (e: Event) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) void importFile(file);
    },
  });
  return h(
    "div",
    { class: "page" },
    h("h2", {}, t("nav.diagnostics")),
    h(
      "div",
      { class: "card" },
      h(
        "div",
        {},
        t("diagnostics.version", { version: state.platform.version, build: state.platform.build, system: state.platform.system }) +
          (state.portable ? ` · ${t("diagnostics.portable")}` : ""),
      ),
      h("div", { class: "muted", style: "user-select:text" }, state.configPath),
      h("div", { class: "muted", style: "user-select:text" }, state.logsDir),
      h(
        "div",
        { class: "row", style: "margin-block-start:12px" },
        h("button", { class: "btn", onclick: () => void api.openLogs() }, icon(""), t("diagnostics.openLogs")),
        h(
          "button",
          {
            class: "btn",
            onclick: async () => {
              const path = await api.exportConfig();
              toast(`${t("diagnostics.exported")}\n${path}`);
            },
          },
          icon(""),
          t("diagnostics.export"),
        ),
        h("button", { class: "btn", onclick: () => picker.click() }, icon(""), t("diagnostics.import")),
        picker,
      ),
    ),
    h(
      "div",
      { class: "row" },
      h("h3", {}, t("diagnostics.displays")),
      h("span", { class: "spacer" }),
      h(
        "button",
        {
          class: "btn",
          onclick: async () => {
            await refreshStatus();
            changed();
          },
        },
        icon(""),
        t("diagnostics.refresh"),
      ),
    ),
    status
      ? h(
          "table",
          { class: "list" },
          h("thead", {}, h("tr", {}, h("th", {}, t("diagnostics.monitor")), h("th", {}, t("diagnostics.state")), h("th", {}, t("diagnostics.mode")), h("th", {}, t("diagnostics.key")))),
          h(
            "tbody",
            {},
            ...status.connected.map((c) => {
              const active = status.displays.find((d) => d.monitorKey === c.monitorKey && c.monitorKey !== null) ?? status.displays.find((d) => d.name === c.name);
              return h(
                "tr",
                {},
                h("td", {}, c.name),
                h("td", {}, h("span", { class: `badge ${c.active ? "ok" : ""}` }, c.active ? (active?.primary ? t("editor.primary") : t("common.on")) : t("common.off"))),
                h(
                  "td",
                  {},
                  c.active && active
                    ? ltr(`${active.width}×${active.height}${active.refreshMilliHz ? ` · ${Math.round(active.refreshMilliHz / 1000)} Hz` : ""} · (${active.x}, ${active.y})`)
                    : "—",
                ),
                h("td", { class: "muted", style: "user-select:text" }, c.monitorKey ?? t("diagnostics.notInConfig")),
              );
            }),
          ),
        )
      : h("p", { class: "muted" }, t("diagnostics.noStatus")),
    h("h3", {}, t("diagnostics.problems")),
    state.issues.length
      ? h("div", { class: "issues" }, ...state.issues.map((i) => h("div", { class: `issue ${i.severity}` }, h("strong", {}, i.path), i.message)))
      : h("p", { class: "muted" }, t("diagnostics.noProblems")),
  );
}

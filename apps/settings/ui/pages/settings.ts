// Settings: theme, language, start-up, OSD, links, disconnect fallback, lock screen.

import { api } from "../api";
import { h, numberInput, select, toggle } from "../dom";
import { setLanguage, t } from "../i18n";
import { themeSwitch, toast } from "../main";
import { changed, state } from "../state";

function row(title: string, description: string | null, control: HTMLElement): HTMLElement {
  return h("div", { class: "setting" }, h("span", { class: "text" }, title, description ? h("small", {}, description) : null), control);
}

async function integrationToggle(kind: "startup" | "protocol", on: boolean): Promise<void> {
  const r = await api.cli({ kind, on });
  toast(r.output || (r.ok ? t("common.done") : t("common.failed")), r.ok ? "info" : "error");
  state.integration = await api.integration();
  changed();
}

export function renderSettings(): HTMLElement {
  const s = state.config.settings;
  const fb = s.disconnectFallback;
  const { languages: available, systemLanguage } = state.platform;
  const systemName = available.find((l) => l.code === systemLanguage)?.name ?? "English";
  const languages: [string, string][] = [
    ["system", t("settings.languageSystem", { language: systemName })],
    ...available.map((l) => [l.code, l.name] as [string, string]),
  ];
  return h(
    "div",
    { class: "page" },
    h("h2", {}, t("nav.settings")),
    h("h3", {}, t("settings.appearance")),
    row(t("settings.theme"), null, themeSwitch()),
    row(
      t("settings.language"),
      t("settings.languageHint"),
      select(languages, s.language, async (v) => {
        s.language = v;
        await setLanguage(v, systemLanguage);
        changed();
      }),
    ),
    h("h3", {}, t("settings.startup")),
    row(t("settings.startWithWindows"), t("settings.startWithWindowsHint"), toggle(state.integration.startup, (v) => void integrationToggle("startup", v))),
    row(t("settings.startMinimized"), t("settings.startMinimizedHint"), toggle(s.startMinimized, (v) => ((s.startMinimized = v), changed()))),
    h("h3", {}, t("settings.switching")),
    row(t("settings.osd"), null, toggle(s.osd.enabled, (v) => ((s.osd.enabled = v), changed()))),
    row(
      t("settings.osdDuration"),
      null,
      h(
        "span",
        { class: "row" },
        h("input", {
          type: "range",
          min: 800,
          max: 5000,
          step: 100,
          value: String(s.osd.durationMs),
          disabled: !s.osd.enabled,
          onchange: (e: Event) => ((s.osd.durationMs = Number((e.target as HTMLInputElement).value)), changed()),
        }),
        t("common.seconds", { n: (s.osd.durationMs / 1000).toFixed(1) }),
      ),
    ),
    row(t("settings.protocol"), t("settings.protocolHint"), toggle(state.integration.protocol, (v) => void integrationToggle("protocol", v))),
    row(
      t("settings.fallback"),
      t("settings.fallbackHint"),
      h(
        "span",
        { class: "row" },
        toggle(fb.enabled, (v) => {
          fb.enabled = v;
          if (v && !fb.group) fb.group = state.config.groups[0]?.id ?? null;
          changed();
        }),
        fb.enabled
          ? select(
              state.config.groups.map((g) => [g.id, g.name] as [string, string]),
              fb.group ?? "",
              (v) => ((fb.group = v), changed()),
            )
          : null,
        fb.enabled ? numberInput(fb.debounceMs, (v) => ((fb.debounceMs = Math.max(500, v)), changed()), { min: 500, step: 500, title: t("settings.debounce") }) : null,
      ),
    ),
    h("h3", {}, t("settings.advanced")),
    row(t("settings.lockScreen"), t("settings.lockScreenHint"), toggle(s.lockScreenSupport, (v) => ((s.lockScreenSupport = v), changed()))),
    row(
      t("settings.logLevel"),
      null,
      select(
        [
          ["error", "error"],
          ["warn", "warn"],
          ["info", "info"],
          ["debug", "debug"],
          ["trace", "trace"],
        ],
        s.logLevel,
        (v) => ((s.logLevel = v), changed()),
      ),
    ),
  );
}

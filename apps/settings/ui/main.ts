// Shell: title bar, navigation, theme, toasts, and a flash-free start.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Theme } from "./api";
import { h, icon } from "./dom";
import { setLanguage, t } from "./i18n";
import { renderDiagnostics } from "./pages/diagnostics";
import { renderGroups } from "./pages/groups";
import { renderHotkeys } from "./pages/hotkeys";
import { renderSettings } from "./pages/settings";
import { renderShortcuts } from "./pages/shortcuts";
import { renderTriggers } from "./pages/triggers";
import { changed, dirty, discard, reload, save, state, subscribe, type Page } from "./state";

const NAV: [Page, string, string][] = [
  ["groups", "", "nav.groups"],
  ["hotkeys", "", "nav.hotkeys"],
  ["triggers", "\uE945", "nav.triggers"],
  ["shortcuts", "", "nav.shortcuts"],
  ["settings", "", "nav.settings"],
  ["diagnostics", "", "nav.diagnostics"],
];

const PAGES: Record<Page, () => HTMLElement> = {
  groups: renderGroups,
  hotkeys: renderHotkeys,
  triggers: renderTriggers,
  shortcuts: renderShortcuts,
  settings: renderSettings,
  diagnostics: renderDiagnostics,
};

const root = document.getElementById("app") as HTMLDivElement;
const media = matchMedia("(prefers-color-scheme: dark)");

/** Applies the theme to the page, the native title bar and the Mica backdrop. */
export function applyTheme(theme: Theme): void {
  const dark = theme === "dark" || (theme === "system" && media.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  getCurrentWindow()
    .setTheme(theme === "system" ? null : theme)
    .catch(() => undefined);
}

media.addEventListener("change", () => applyTheme(state.config.settings.theme));

let toastTimer = 0;

export function toast(message: string, kind: "info" | "error" = "info", action?: { label: string; run: () => void }): void {
  document.querySelector(".toast")?.remove();
  window.clearTimeout(toastTimer);
  const el = h(
    "div",
    { class: `toast ${kind}`, role: kind === "error" ? "alert" : "status" },
    h("span", {}, message),
    action
      ? h(
          "button",
          {
            class: "btn",
            onclick: () => {
              el.remove();
              action.run();
            },
          },
          action.label,
        )
      : null,
  );
  document.body.append(el);
  toastTimer = window.setTimeout(() => el.remove(), kind === "error" ? 9000 : 5000);
}

export function themeSwitch(): HTMLElement {
  const current = state.config.settings.theme;
  const options: [Theme, string][] = [
    ["system", t("theme.system")],
    ["dark", t("theme.dark")],
    ["light", t("theme.light")],
  ];
  return h(
    "div",
    { class: "segmented", role: "group", "aria-label": t("settings.theme") },
    ...options.map(([value, label]) =>
      h(
        "button",
        {
          "aria-pressed": String(value === current),
          onclick: () => {
            state.config.settings.theme = value;
            applyTheme(value);
            changed();
          },
        },
        label,
      ),
    ),
  );
}

export async function saveNow(): Promise<void> {
  const ok = await save();
  if (ok) {
    toast(t("common.saved"));
  } else {
    toast(t("common.notSaved"), "error");
  }
}

function titlebar(): HTMLElement {
  const isDirty = dirty();
  const errors = state.issues.filter((i) => i.severity === "error").length;
  return h(
    "header",
    { class: "titlebar" },
    h("h1", {}, t("settings.title")),
    errors ? h("span", { class: "badge bad" }, t("common.problems", { count: errors })) : null,
    isDirty ? h("span", { class: "dirty-dot", title: t("common.unsaved") }) : null,
    themeSwitch(),
    h("button", { class: "btn", disabled: !isDirty, onclick: () => discard() }, t("common.discard")),
    h("button", { class: "btn accent", disabled: !isDirty, onclick: () => void saveNow() }, t("common.save")),
  );
}

function nav(): HTMLElement {
  return h(
    "nav",
    { class: "nav", "aria-label": t("settings.title") },
    ...NAV.map(([page, glyph, key]) =>
      h(
        "button",
        {
          "aria-current": state.page === page ? "page" : undefined,
          onclick: () => {
            state.page = page;
            changed();
          },
        },
        icon(glyph),
        t(key),
      ),
    ),
  );
}

let lastPage: Page | null = null;
let appliedTheme: Theme | null = null;
let appliedLanguage: string | null = null;

function render(): void {
  // Keep theme and language in step with the config (e.g. after "Discard" or an import).
  const { theme, language } = state.config.settings;
  if (theme !== appliedTheme) {
    appliedTheme = theme;
    applyTheme(theme);
  }
  if (language !== appliedLanguage) {
    appliedLanguage = language;
    void setLanguage(language, state.platform.systemLanguage).then(() => {
      if (appliedLanguage === language) render();
    });
  }
  const previous = root.querySelector(".content");
  const scroll = previous && lastPage === state.page ? previous.scrollTop : 0;
  const page = PAGES[state.page]();
  if (lastPage === state.page) page.style.animation = "none";
  const content = h("main", { class: "content" }, page);
  root.replaceChildren(titlebar(), nav(), content);
  content.scrollTop = scroll;
  lastPage = state.page;
}

window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    if (dirty()) void saveNow();
  }
});

async function main(): Promise<void> {
  state.platform = await api.platform();
  if (state.platform.mica) document.documentElement.dataset.mica = "1";
  state.icons = await api.builtinIcons();
  await reload();
  state.integration = await api.integration();
  appliedTheme = state.config.settings.theme;
  applyTheme(appliedTheme);
  appliedLanguage = state.config.settings.language;
  await setLanguage(appliedLanguage, state.platform.systemLanguage);
  subscribe(render);
  render();

  const win = getCurrentWindow();
  await win.onCloseRequested((event) => {
    if (dirty() && !window.confirm(t("common.unsavedClose"))) event.preventDefault();
  });
  // The window was created hidden: show it only now that theme and content are in
  // place, then fade the page in. No white flash.
  await win.show();
  await win.setFocus();
  requestAnimationFrame(() => document.body.classList.add("ready"));
}

main().catch(async (e) => {
  document.body.classList.add("ready");
  document.body.textContent = String(e);
  await getCurrentWindow().show();
});

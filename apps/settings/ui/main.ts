// Shell: title bar, navigation, theme, toasts, and a flash-free start.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Config, type Group, type Theme } from "./api";
import { h, icon } from "./dom";
import { setLanguage, t } from "./i18n";
import { renderDiagnostics } from "./pages/diagnostics";
import { forgetModes, renderGroups } from "./pages/groups";
import { renderHotkeys } from "./pages/hotkeys";
import { renderSettings } from "./pages/settings";
import { renderShortcuts } from "./pages/shortcuts";
import { renderTriggers } from "./pages/triggers";
import { changed, dirty, discard, refreshStatus, reload, save, state, subscribe, type Page } from "./state";

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
  toastTimer = window.setTimeout(() => el.remove(), kind === "error" || action ? 9000 : 5000);
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

/** Switches to `g` for real and says how it ended (the countdown dialog decides). */
export async function applyAndReport(g: Group): Promise<void> {
  toast(t("groups.testing", { name: g.name }));
  let code: number;
  try {
    code = await api.applyGroup(g.id);
  } catch (e) {
    toast(String(e), "error");
    return;
  }
  if (await refreshStatus()) changed();
  if (code === 0) toast(t("groups.applied", { name: g.name }));
  else if (code === 3) toast(t("groups.reverted", { name: g.name }));
  else toast(t("groups.applyFailed", { name: g.name }), "error");
}

/**
 * Saving never changes the screen. When the group on screen now was edited, offer to
 * apply it so Windows follows.
 */
export async function saveNow(): Promise<void> {
  const before = JSON.parse(state.saved) as Config;
  const ok = await save();
  if (!ok) {
    toast(t("common.notSaved"), "error");
    return;
  }
  const onScreen = state.config.groups.find((g) => g.id === state.currentGroup);
  const old = before.groups.find((g) => g.id === onScreen?.id);
  if (onScreen && old && JSON.stringify(old.displays) !== JSON.stringify(onScreen.displays)) {
    toast(t("groups.savedOnScreen", { name: onScreen.name }), "info", { label: t("groups.applyNow"), run: () => void applyAndReport(onScreen) });
  } else {
    toast(t("common.saved"));
  }
}

/** JSON with sorted keys and without empty values, to compare configs by content. */
function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(
          Object.entries(v as Record<string, unknown>)
            .filter(([, x]) => x !== null && x !== undefined)
            .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
        )
      : v,
  );
}

/** Windows' displays changed: show the new state (re-rendered only if it differs). */
async function displaysChanged(): Promise<void> {
  if (await refreshStatus()) {
    forgetModes();
    changed();
  }
}

/**
 * The config file changed on disk (a capture, a change from the tray or another
 * window). Unsaved edits here are never replaced without asking.
 */
async function configChangedOnDisk(): Promise<void> {
  const disk = await api.loadConfig();
  if (canonical(disk.config) === canonical(JSON.parse(state.saved))) return;
  if (dirty()) {
    toast(t("common.changedOnDisk"), "info", { label: t("common.loadChanges"), run: () => void reload().then(changed) });
    return;
  }
  await reload();
  changed();
}

let configTimer = 0;

async function followChanges(): Promise<void> {
  await listen("displays-changed", () => void displaysChanged());
  await listen("config-changed", () => {
    window.clearTimeout(configTimer);
    configTimer = window.setTimeout(() => void configChangedOnDisk(), 300);
  });
  // Changes made while the window was in the background are shown on return.
  window.addEventListener("focus", () => void displaysChanged());
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
  await followChanges();

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

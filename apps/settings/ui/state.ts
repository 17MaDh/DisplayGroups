// Application state, change notification and the save/discard cycle.

import { api, type Config, type Issue, type Platform, type Status } from "./api";

export type Page = "groups" | "hotkeys" | "triggers" | "shortcuts" | "settings" | "diagnostics";

export const state = {
  config: null as unknown as Config,
  saved: "",
  issues: [] as Issue[],
  configPath: "",
  logsDir: "",
  portable: false,
  platform: null as unknown as Platform,
  integration: { startup: false, protocol: false },
  status: null as Status | null,
  currentGroup: null as string | null,
  icons: {} as Record<string, string>,
  page: "groups" as Page,
  selectedGroup: null as string | null,
  selectedDisplay: 0,
};

const listeners: (() => void)[] = [];

export function subscribe(fn: () => void): void {
  listeners.push(fn);
}

/** Re-renders after a change. */
export function changed(): void {
  for (const fn of listeners) fn();
}

export function dirty(): boolean {
  return JSON.stringify(state.config) !== state.saved;
}

export async function reload(): Promise<void> {
  const s = await api.loadConfig();
  state.config = s.config;
  state.saved = JSON.stringify(s.config);
  state.issues = s.issues;
  state.configPath = s.configPath;
  state.logsDir = s.logsDir;
  state.portable = s.portable;
  if (!state.config.groups.some((g) => g.id === state.selectedGroup)) {
    state.selectedGroup = state.config.groups[0]?.id ?? null;
    state.selectedDisplay = 0;
  }
  await refreshStatus();
}

export async function refreshStatus(): Promise<void> {
  try {
    const [status, current] = await api.displayStatus(state.config);
    state.status = status;
    state.currentGroup = current;
  } catch {
    state.status = null;
  }
}

/** Saves the config; on validation errors nothing is written and the issues are kept. */
export async function save(): Promise<boolean> {
  const result = await api.saveConfig(state.config);
  state.issues = result.issues;
  if (result.saved) state.saved = JSON.stringify(state.config);
  changed();
  return result.saved;
}

export function discard(): void {
  state.config = JSON.parse(state.saved) as Config;
  changed();
}

export function monitorName(key: string): string {
  return state.config.monitors[key]?.displayName ?? key;
}

/**
 * The number "Identify" shows on this monitor: its position among the config's
 * monitors ordered by key (as `Config::monitor_number` in dg-model), from 1.
 */
export function monitorNumber(key: string): number {
  const keys = Object.keys(state.config.monitors).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const i = keys.indexOf(key);
  return i < 0 ? keys.length + 1 : i + 1;
}

export function iconGlyph(icon: string): { glyph: string; color: string } {
  const name = icon.startsWith("builtin:") ? icon.slice(8) : "monitor";
  const [glyph, color] = (state.icons[name] ?? state.icons["monitor"] ?? "|#2563eb").split("|");
  return { glyph, color };
}

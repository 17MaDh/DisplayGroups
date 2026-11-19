// UI strings from the shared locales/*.json. English is bundled; other languages are
// read from `locales\<lang>.json` next to the executable and fall back key by key.

import en from "../../../locales/en.json";
import { api } from "./api";

const RTL = new Set(["ar", "he", "fa", "ur"]);
let strings: Record<string, string> = { ...en };

/**
 * Loads the strings for a language setting. "system" means the Windows display
 * language (`system`, from the platform info). The page switches to right-to-left only
 * when that language's file was actually loaded.
 */
export async function setLanguage(setting: string, system: string): Promise<void> {
  const wanted = !setting || setting === "system" ? system || "en" : setting;
  strings = { ...en };
  let loaded = "en";
  if (wanted !== "en") {
    try {
      Object.assign(strings, await api.locale(wanted));
      loaded = wanted;
    } catch {
      // Missing file: stay in English.
    }
  }
  const root = document.documentElement;
  root.lang = loaded;
  root.dir = RTL.has(loaded) ? "rtl" : "ltr";
}

/** `t("groups.deleted", { name })` replaces `{name}`. Unknown keys return the key. */
export function t(key: string, vars: Record<string, string | number> = {}): string {
  let s = strings[key] ?? key;
  for (const [k, v] of Object.entries(vars)) {
    s = s.split(`{${k}}`).join(String(v));
  }
  return s;
}

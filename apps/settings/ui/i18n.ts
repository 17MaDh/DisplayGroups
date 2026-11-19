// UI strings from the shared locales/*.json. English is bundled; other languages are
// read from `locales\<lang>.json` next to the executable and fall back key by key.

import en from "../../../locales/en.json";
import { api } from "./api";

const RTL = new Set(["ar", "he", "fa", "ur"]);
let strings: Record<string, string> = { ...en };

export async function setLanguage(language: string): Promise<void> {
  strings = { ...en };
  if (language && language !== "en") {
    try {
      Object.assign(strings, await api.locale(language));
    } catch {
      // Missing file: stay in English.
    }
  }
  const root = document.documentElement;
  root.lang = language || "en";
  root.dir = RTL.has(language) ? "rtl" : "ltr";
}

/** `t("groups.deleted", { name })` replaces `{name}`. Unknown keys return the key. */
export function t(key: string, vars: Record<string, string | number> = {}): string {
  let s = strings[key] ?? key;
  for (const [k, v] of Object.entries(vars)) {
    s = s.split(`{${k}}`).join(String(v));
  }
  return s;
}

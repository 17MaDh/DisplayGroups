// UI strings from the shared locales/*.json. English is bundled; other languages are
// read from `locales/<lang>.json` next to the executable and fall back key by key.
//
// A key can have a version for one system, `<key>@linux`, used there instead of the
// plain key (for example "Displays GNOME sees" instead of "Displays Windows sees").

import en from "../../../locales/en.json";
import { api } from "./api";

const RTL = new Set(["ar", "he", "fa", "ur"]);
const english: Record<string, string> = en;
let local: Record<string, string> = {};
let variant = "";

/** The system the strings are for ("windows" or "linux"). */
export function setVariant(os: string): void {
  variant = os === "windows" ? "" : os;
}

/**
 * Loads the strings for a language setting. "system" means the system's display
 * language (`system`, from the platform info). The page switches to right-to-left only
 * when that language's file was actually loaded.
 */
export async function setLanguage(setting: string, system: string): Promise<void> {
  const wanted = !setting || setting === "system" ? system || "en" : setting;
  local = {};
  let loaded = "en";
  if (wanted !== "en") {
    try {
      local = await api.locale(wanted);
      loaded = wanted;
    } catch {
      // Missing file: stay in English.
    }
  }
  const root = document.documentElement;
  root.lang = loaded;
  root.dir = RTL.has(loaded) ? "rtl" : "ltr";
}

function lookup(key: string): string | undefined {
  const special = variant ? `${key}@${variant}` : null;
  // The chosen language first, even its plain wording, then English.
  return (special && local[special]) ?? local[key] ?? (special && english[special]) ?? english[key];
}

/** `t("groups.deleted", { name })` replaces `{name}`. Unknown keys return the key. */
export function t(key: string, vars: Record<string, string | number> = {}): string {
  let s = lookup(key) ?? key;
  for (const [k, v] of Object.entries(vars)) {
    s = s.split(`{${k}}`).join(String(v));
  }
  return s;
}

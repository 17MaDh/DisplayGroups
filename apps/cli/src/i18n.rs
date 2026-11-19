//! UI strings for the native parts (tray, countdown dialog, OSD, identify). The same
//! `locales/<lang>.json` files are used by the settings app.
//!
//! English is compiled in; another language is read from `locales\<lang>.json` next
//! to the executable and falls back to English key by key. The setting `"system"`
//! follows the Windows display language. The agent calls [`init`] again when the
//! setting changes.

// On Linux the strings are only used by parts that come in later phases (§16).
#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

const EN: &str = include_str!("../../../locales/en.json");

/// Languages written right to left.
const RTL: [&str; 4] = ["ar", "he", "fa", "ur"];

struct Strings {
    map: HashMap<String, String>,
    /// The language actually loaded ("en" when the wanted file is missing).
    language: String,
}

static STRINGS: RwLock<Option<Strings>> = RwLock::new(None);

/// The language code a setting stands for: `"system"` (or empty) means the Windows
/// display language.
pub fn resolve(setting: &str) -> String {
    let s = setting.trim().to_ascii_lowercase();
    if s.is_empty() || s == "system" {
        crate::sys::ui_language()
    } else {
        s
    }
}

/// `locales` next to the executable. Debug builds fall back to the repository's
/// folder, so translations can be tried without copying files.
pub fn locales_dir() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("locales");
    if beside.is_dir() || !cfg!(debug_assertions) {
        Some(beside)
    } else {
        Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../locales"
        )))
    }
}

fn english() -> Strings {
    Strings {
        map: serde_json::from_str(EN).unwrap_or_default(),
        language: "en".into(),
    }
}

fn load(setting: &str) -> Strings {
    let mut strings = english();
    let wanted = resolve(setting);
    let valid = wanted
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if wanted == "en" || !valid {
        return strings;
    }
    let text =
        locales_dir().and_then(|d| std::fs::read_to_string(d.join(format!("{wanted}.json"))).ok());
    if let Some(extra) = text.and_then(|t| serde_json::from_str::<HashMap<String, String>>(&t).ok())
    {
        strings.map.extend(extra);
        strings.language = wanted;
    }
    strings
}

/// Loads, or reloads, the strings for a language setting.
pub fn init(setting: &str) {
    let strings = load(setting);
    *STRINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(strings);
}

fn with<R>(f: impl FnOnce(&Strings) -> R) -> R {
    {
        let guard = STRINGS.read().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = guard.as_ref() {
            return f(s);
        }
    }
    f(&english())
}

/// The string for `key`, or the key itself when missing.
pub fn t(key: &str) -> String {
    with(|s| s.map.get(key).cloned().unwrap_or_else(|| key.to_string()))
}

/// Whether the loaded language is written right to left.
pub fn is_rtl() -> bool {
    with(|s| RTL.contains(&s.language.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_strings_are_embedded() {
        let map: HashMap<String, String> = serde_json::from_str(EN).unwrap();
        assert!(map.len() > 40);
        assert_eq!(english().map["confirm.revert"], "Revert");
        assert_eq!(t("no.such.key"), "no.such.key");
    }

    #[test]
    fn placeholders_are_present() {
        assert!(t("confirm.body").contains("{group}") && t("confirm.body").contains("{seconds}"));
    }

    #[test]
    fn system_resolves_to_a_language_code() {
        assert_eq!(resolve("ar"), "ar");
        assert_eq!(resolve(" EN "), "en");
        let system = resolve("system");
        assert!((2..=3).contains(&system.len()), "{system}");
    }

    #[test]
    fn every_translation_matches_english() {
        // Each locales/<lang>.json is valid, has only known keys and keeps the
        // placeholders of the English string.
        let en: HashMap<String, String> = serde_json::from_str(EN).unwrap();
        let dir = locales_dir().unwrap();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let map: HashMap<String, String> =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for (key, value) in &map {
                let english = en
                    .get(key)
                    .unwrap_or_else(|| panic!("{}: unknown key {key}", path.display()));
                for part in english.split('{').skip(1) {
                    let name = part.split('}').next().unwrap_or_default();
                    assert!(
                        value.contains(&format!("{{{name}}}")),
                        "{}: {key} lacks {{{name}}}",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn arabic_loads_right_to_left() {
        let s = load("ar");
        assert_eq!(s.language, "ar");
        assert!(RTL.contains(&s.language.as_str()));
        assert_ne!(s.map["confirm.revert"], "Revert");
    }

    #[test]
    fn arabic_is_complete() {
        let en: HashMap<String, String> = serde_json::from_str(EN).unwrap();
        let path = locales_dir().unwrap().join("ar.json");
        let ar: HashMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut missing: Vec<&String> = en.keys().filter(|k| !ar.contains_key(*k)).collect();
        missing.sort();
        assert!(missing.is_empty(), "ar.json lacks {missing:?}");
    }
}

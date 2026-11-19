//! UI strings for the native parts (tray, countdown dialog, OSD). The same
//! `locales/<lang>.json` files are used by the settings app.
//!
//! English is compiled in; another language is read from `locales\<lang>.json` next
//! to the executable and falls back to English key by key.

use std::collections::HashMap;
use std::sync::OnceLock;

const EN: &str = include_str!("../../../locales/en.json");

static STRINGS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Loads the strings once; later calls are ignored.
pub fn init(language: &str) {
    let mut map: HashMap<String, String> = serde_json::from_str(EN).unwrap_or_default();
    if language != "en"
        && language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        let file = std::env::current_exe().ok().and_then(|p| {
            p.parent()
                .map(|d| d.join("locales").join(format!("{language}.json")))
        });
        if let Some(text) = file.and_then(|f| std::fs::read_to_string(f).ok()) {
            if let Ok(extra) = serde_json::from_str::<HashMap<String, String>>(&text) {
                map.extend(extra);
            }
        }
    }
    let _ = STRINGS.set(map);
}

/// The string for `key`, or the key itself when missing.
pub fn t(key: &str) -> String {
    STRINGS
        .get_or_init(|| serde_json::from_str(EN).unwrap_or_default())
        .get(key)
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_strings_are_embedded() {
        let map: HashMap<String, String> = serde_json::from_str(EN).unwrap();
        assert!(map.len() > 40);
        assert_eq!(t("confirm.revert"), "Revert");
        assert_eq!(t("no.such.key"), "no.such.key");
    }

    #[test]
    fn placeholders_are_present() {
        assert!(t("confirm.body").contains("{group}") && t("confirm.body").contains("{seconds}"));
    }
}

//! Hotkey chords such as `Ctrl+Alt+F12` (ARCHITECTURE.md §7).
//!
//! Chords are stored by virtual-key code, so they work with any keyboard layout. The
//! live conflict probe (does another process hold the chord?) is Win32 and lives in
//! `dg-win`; this crate holds parsing and the curated table of well-known defaults.

use std::fmt;

/// `RegisterHotKey` modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u32);

impl Modifiers {
    pub const ALT: Modifiers = Modifiers(0x1);
    pub const CTRL: Modifiers = Modifiers(0x2);
    pub const SHIFT: Modifiers = Modifiers(0x4);
    pub const WIN: Modifiers = Modifiers(0x8);

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn with(self, other: Modifiers) -> Modifiers {
        Modifiers(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub modifiers: Modifiers,
    /// Windows virtual-key code.
    pub vk: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordError {
    #[error("the shortcut is empty")]
    Empty,
    #[error("\"{0}\" is not a key name")]
    UnknownKey(String),
    #[error("the shortcut has no key, only modifiers")]
    NoKey,
    #[error("the shortcut has more than one key (\"{0}\" and \"{1}\")")]
    TwoKeys(String, String),
    #[error("\"{0}\" appears twice")]
    Repeated(String),
    #[error("add Ctrl, Alt, Shift or Win; a single key would fire while typing")]
    NeedsModifier,
}

/// Named keys: (canonical name, aliases, virtual-key code).
const NAMED_KEYS: &[(&str, &[&str], u32)] = &[
    ("Backspace", &["Back", "BkSp"], 0x08),
    ("Tab", &[], 0x09),
    ("Enter", &["Return"], 0x0D),
    ("Pause", &["Break"], 0x13),
    ("Esc", &["Escape"], 0x1B),
    ("Space", &["Spacebar"], 0x20),
    ("PageUp", &["PgUp", "Prior"], 0x21),
    ("PageDown", &["PgDn", "Next"], 0x22),
    ("End", &[], 0x23),
    ("Home", &[], 0x24),
    ("Left", &["ArrowLeft"], 0x25),
    ("Up", &["ArrowUp"], 0x26),
    ("Right", &["ArrowRight"], 0x27),
    ("Down", &["ArrowDown"], 0x28),
    ("PrintScreen", &["PrtSc", "PrtScn", "Snapshot"], 0x2C),
    ("Insert", &["Ins"], 0x2D),
    ("Delete", &["Del"], 0x2E),
    ("ScrollLock", &["Scroll"], 0x91),
    ("Semicolon", &[";"], 0xBA),
    ("Equals", &["=", "Plus"], 0xBB),
    ("Comma", &[","], 0xBC),
    ("Minus", &["-"], 0xBD),
    ("Period", &["."], 0xBE),
    ("Slash", &["/"], 0xBF),
    ("Backquote", &["`", "Grave", "Tilde"], 0xC0),
    ("LeftBracket", &["["], 0xDB),
    ("Backslash", &["\\"], 0xDC),
    ("RightBracket", &["]"], 0xDD),
    ("Quote", &["'", "Apostrophe"], 0xDE),
];

const MODIFIER_NAMES: &[(&str, Modifiers)] = &[
    ("ctrl", Modifiers::CTRL),
    ("control", Modifiers::CTRL),
    ("alt", Modifiers::ALT),
    ("shift", Modifiers::SHIFT),
    ("win", Modifiers::WIN),
    ("windows", Modifiers::WIN),
    ("super", Modifiers::WIN),
    ("meta", Modifiers::WIN),
];

fn key_from_name(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(u32::from(bytes[0])); // VK codes for A-Z / 0-9 equal their ASCII codes
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    if let Some(n) = upper
        .strip_prefix("NUMPAD")
        .or_else(|| upper.strip_prefix("NUM"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        if n <= 9 {
            return Some(0x60 + n);
        }
    }
    NAMED_KEYS
        .iter()
        .find(|(canonical, aliases, _)| {
            canonical.eq_ignore_ascii_case(name)
                || aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
        .map(|(_, _, vk)| *vk)
}

/// Canonical display name of a virtual key, e.g. `F12`, `A`, `Numpad5`.
pub fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from).unwrap_or_default(),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        _ => NAMED_KEYS
            .iter()
            .find(|(_, _, code)| *code == vk)
            .map(|(name, _, _)| (*name).to_string())
            .unwrap_or_else(|| format!("VK{vk:#04X}")),
    }
}

impl Chord {
    /// Parses `Ctrl+Alt+F12`, `win + shift + 1`, ... (case-insensitive).
    pub fn parse(text: &str) -> Result<Chord, ChordError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ChordError::Empty);
        }
        let mut modifiers = Modifiers::default();
        let mut key: Option<(String, u32)> = None;
        for raw in text.split('+') {
            let part = raw.trim();
            if part.is_empty() {
                return Err(ChordError::UnknownKey("+".into()));
            }
            if let Some((_, m)) = MODIFIER_NAMES
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(part))
            {
                if modifiers.contains(*m) {
                    return Err(ChordError::Repeated(part.to_string()));
                }
                modifiers = modifiers.with(*m);
                continue;
            }
            let vk = key_from_name(part).ok_or_else(|| ChordError::UnknownKey(part.to_string()))?;
            if let Some((first, _)) = &key {
                return Err(ChordError::TwoKeys(first.clone(), part.to_string()));
            }
            key = Some((part.to_string(), vk));
        }
        let (_, vk) = key.ok_or(ChordError::NoKey)?;
        // F13–F24 exist only on macro keyboards and are safe on their own.
        if modifiers.is_empty() && !(0x7C..=0x87).contains(&vk) {
            return Err(ChordError::NeedsModifier);
        }
        Ok(Chord { modifiers, vk })
    }

    /// Shortcuts Windows itself owns; registering them fails or breaks the system.
    pub fn reserved_by_windows(&self) -> Option<&'static str> {
        RESERVED
            .iter()
            .find(|(chord, _)| Chord::parse(chord).ok() == Some(*self))
            .map(|(_, what)| *what)
    }

    /// Applications whose *default* shortcut is this chord. Users may have changed
    /// them; the live probe in `dg-win` is the authority for global hotkeys.
    pub fn known_conflicts(&self) -> Vec<&'static str> {
        KNOWN_DEFAULTS
            .iter()
            .filter(|(chord, _)| Chord::parse(chord).ok() == Some(*self))
            .map(|(_, owner)| *owner)
            .collect()
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (m, name) in [
            (Modifiers::CTRL, "Ctrl"),
            (Modifiers::ALT, "Alt"),
            (Modifiers::SHIFT, "Shift"),
            (Modifiers::WIN, "Win"),
        ] {
            if self.modifiers.contains(m) {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&key_name(self.vk))
    }
}

const RESERVED: &[(&str, &str)] = &[
    ("Ctrl+Alt+Delete", "Windows security screen"),
    ("Ctrl+Shift+Esc", "Task Manager"),
    ("Alt+Tab", "Windows task switcher"),
    ("Alt+F4", "Close window"),
    ("Win+L", "Lock Windows"),
    ("Win+D", "Show desktop"),
    ("Win+P", "Windows Project menu"),
    ("Win+Tab", "Task view"),
    ("Win+G", "Xbox Game Bar"),
];

/// Default shortcuts of common overlays and launchers. On the test machine
/// `Alt+Z` and `Alt+F10` were verified as held by the NVIDIA overlay.
const KNOWN_DEFAULTS: &[(&str, &str)] = &[
    ("Alt+Z", "NVIDIA overlay"),
    ("Alt+F1", "NVIDIA overlay (screenshot)"),
    ("Alt+F9", "NVIDIA overlay (record)"),
    ("Alt+F10", "NVIDIA overlay (instant replay)"),
    ("Alt+R", "NVIDIA / AMD performance overlay"),
    ("Win+Alt+R", "Xbox Game Bar (record)"),
    ("Win+Alt+G", "Xbox Game Bar (record that)"),
    ("Win+Alt+PrintScreen", "Xbox Game Bar (screenshot)"),
    ("Shift+Tab", "Steam overlay"),
    ("Ctrl+Shift+M", "Discord (mute)"),
    ("Ctrl+Shift+D", "Discord (deafen)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_canonically() {
        let c = Chord::parse(" alt + ctrl+f12 ").unwrap();
        assert_eq!(c.vk, 0x7B);
        assert!(c.modifiers.contains(Modifiers::CTRL) && c.modifiers.contains(Modifiers::ALT));
        assert_eq!(c.to_string(), "Ctrl+Alt+F12");
        assert_eq!(
            Chord::parse("Win+Shift+1").unwrap().to_string(),
            "Shift+Win+1"
        );
        assert_eq!(
            Chord::parse("ctrl+alt+pgdn").unwrap().to_string(),
            "Ctrl+Alt+PageDown"
        );
        assert_eq!(
            Chord::parse("Ctrl+Num5").unwrap().to_string(),
            "Ctrl+Numpad5"
        );
        assert_eq!(Chord::parse("Ctrl+=").unwrap().to_string(), "Ctrl+Equals");
    }

    #[test]
    fn modifier_bits_match_register_hotkey() {
        let c = Chord::parse("Ctrl+Alt+Shift+Win+A").unwrap();
        assert_eq!(c.modifiers.bits(), 0x1 | 0x2 | 0x4 | 0x8);
        assert_eq!(c.vk, 0x41);
    }

    #[test]
    fn rejects_bad_chords() {
        assert_eq!(Chord::parse(""), Err(ChordError::Empty));
        assert_eq!(Chord::parse("Ctrl+Alt"), Err(ChordError::NoKey));
        assert_eq!(Chord::parse("F12"), Err(ChordError::NeedsModifier));
        assert_eq!(
            Chord::parse("Ctrl+Ctrl+A"),
            Err(ChordError::Repeated("Ctrl".into()))
        );
        assert!(matches!(
            Chord::parse("Ctrl+A+B"),
            Err(ChordError::TwoKeys(..))
        ));
        assert!(matches!(
            Chord::parse("Ctrl+Hyper"),
            Err(ChordError::UnknownKey(_))
        ));
        assert!(matches!(
            Chord::parse("Ctrl++"),
            Err(ChordError::UnknownKey(_))
        ));
        assert!(Chord::parse("F13").is_ok(), "macro keys may stand alone");
    }

    #[test]
    fn reserved_and_known_conflicts() {
        assert_eq!(
            Chord::parse("win+l").unwrap().reserved_by_windows(),
            Some("Lock Windows")
        );
        assert_eq!(
            Chord::parse("Alt+Z").unwrap().known_conflicts(),
            ["NVIDIA overlay"]
        );
        assert!(Chord::parse("Ctrl+Alt+F12")
            .unwrap()
            .known_conflicts()
            .is_empty());
        assert!(Chord::parse("Ctrl+Alt+F12")
            .unwrap()
            .reserved_by_windows()
            .is_none());
    }
}

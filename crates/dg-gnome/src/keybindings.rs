//! Hotkeys as GNOME custom shortcuts (ARCHITECTURE.md §16.3, L2).
//!
//! On Wayland an application cannot grab global keys. GNOME's own "Custom Shortcuts"
//! (Settings → Keyboard) run a command for a key combination, so each DisplayGroups
//! hotkey becomes one of them, running `displaygroups apply …`. They live in GSettings
//! under paths of our own (`…/custom-keybindings/displaygroups-N/`), so the user's other
//! custom shortcuts are never touched, and they keep working when the agent is not
//! running. `gsettings` writes them, as GNOME Settings does through the same schema.

use std::path::Path;
use std::process::Command;

use dg_hotkey::Chord;
use dg_model::{Config, HotkeyAction};
use uuid::Uuid;

pub const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
pub const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
pub const BASE_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
const OUR_PREFIX: &str = "displaygroups-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Shown in GNOME Settings, e.g. "DisplayGroups: Desktop".
    pub name: String,
    /// A command line as GNOME parses it (shell quoting).
    pub command: String,
    /// A GTK accelerator such as `<Control><Alt>F12`.
    pub binding: String,
}

/// Parses GSettings' text for an array of strings: `['/a/', '/b/']` or `@as []`.
pub fn parse_list(text: &str) -> Vec<String> {
    let text = text.trim().trim_start_matches("@as").trim();
    let inner = text.trim_start_matches('[').trim_end_matches(']');
    let mut items = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' && c != '"' {
            continue;
        }
        let quote = c;
        let mut item = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        item.push(next);
                    }
                }
                c if c == quote => break,
                c => item.push(c),
            }
        }
        items.push(item);
    }
    items
}

pub fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".into();
    }
    let quoted: Vec<String> = items.iter().map(|i| gvariant_string(i)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A GVariant text string literal: `'it\'s'`.
pub fn gvariant_string(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// One argument for a command GNOME runs (it splits commands like a shell).
pub fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:,=+@%".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

pub fn is_ours(path: &str) -> bool {
    path.strip_prefix(BASE_PATH)
        .is_some_and(|rest| rest.starts_with(OUR_PREFIX))
}

pub fn our_path(index: usize) -> String {
    format!("{BASE_PATH}{OUR_PREFIX}{index}/")
}

/// The new list: the user's own custom shortcuts first, unchanged, then ours.
pub fn merged_list(current: &[String], ours: usize) -> Vec<String> {
    current
        .iter()
        .filter(|p| !is_ours(p))
        .cloned()
        .chain((0..ours).map(our_path))
        .collect()
}

fn gsettings(args: &[&str]) -> Result<String, String> {
    let output = Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|e| format!("gsettings is not available ({e})"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "gsettings {}: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Makes GNOME's custom shortcuts match `shortcuts`: ours are rewritten, the user's
/// others are kept.
pub fn sync(shortcuts: &[Shortcut]) -> Result<(), String> {
    let current = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    for path in current.iter().filter(|p| is_ours(p)) {
        let at = format!("{ENTRY_SCHEMA}:{path}");
        for key in ["name", "command", "binding"] {
            gsettings(&["reset", &at, key])?;
        }
    }
    for (i, s) in shortcuts.iter().enumerate() {
        let at = format!("{ENTRY_SCHEMA}:{}", our_path(i));
        gsettings(&["set", &at, "name", &gvariant_string(&s.name)])?;
        gsettings(&["set", &at, "command", &gvariant_string(&s.command)])?;
        gsettings(&["set", &at, "binding", &gvariant_string(&s.binding)])?;
    }
    let list = merged_list(&current, shortcuts.len());
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&list)])?;
    Ok(())
}

/// The GNOME custom shortcuts for the config's hotkeys, running `exe` (the CLI), and the
/// hotkeys that cannot be one (an invalid chord, or a key GNOME has no name for).
/// `recover_label` names the "recover" action in GNOME Settings.
pub fn for_config(cfg: &Config, exe: &Path, recover_label: &str) -> (Vec<Shortcut>, Vec<String>) {
    let exe = shell_quote(&exe.to_string_lossy());
    let name_of = |id: &Uuid| {
        cfg.group_by_id(*id)
            .map_or_else(|| id.to_string(), |g| g.name.clone())
    };
    let mut shortcuts = Vec::new();
    let mut problems = Vec::new();
    for hotkey in &cfg.hotkeys {
        let chord = match Chord::parse(&hotkey.chord) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("\"{}\": {e}", hotkey.chord));
                continue;
            }
        };
        let Some(binding) = chord.gtk_accelerator() else {
            problems.push(format!("{chord}: GNOME has no name for this key"));
            continue;
        };
        let (label, args) = match &hotkey.action {
            HotkeyAction::Apply { group } => (name_of(group), format!("apply {group}")),
            HotkeyAction::Toggle { groups } | HotkeyAction::Cycle { groups } => (
                groups.iter().map(name_of).collect::<Vec<_>>().join(" / "),
                format!(
                    "toggle {}",
                    groups
                        .iter()
                        .map(Uuid::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            ),
            HotkeyAction::Recover => (recover_label.to_string(), "recover".into()),
        };
        shortcuts.push(Shortcut {
            name: format!("DisplayGroups: {label}"),
            command: format!("{exe} {args}"),
            binding,
        });
    }
    (shortcuts, problems)
}

const CTRL: u8 = 1;
const ALT: u8 = 2;
const SHIFT: u8 = 4;
const SUPER: u8 = 8;

/// A GTK accelerator reduced to what decides a clash: its modifiers and its key, with
/// GNOME's spellings unified (`<Primary>` = `<Control>` = `<Ctrl>`, `<Mod1>` = `<Alt>`,
/// `<Mod4>` = `<Super>`) and the key in lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accel {
    mods: u8,
    key: String,
}

pub fn parse_accelerator(text: &str) -> Option<Accel> {
    let mut rest = text.trim();
    let mut mods = 0;
    while let Some(after) = rest.strip_prefix('<') {
        let (name, tail) = after.split_once('>')?;
        mods |= match name.to_ascii_lowercase().as_str() {
            "primary" | "control" | "ctrl" | "ctl" => CTRL,
            "alt" | "mod1" => ALT,
            "shift" | "shft" => SHIFT,
            "super" | "mod4" | "meta" | "hyper" => SUPER,
            _ => 0,
        };
        rest = tail;
    }
    (!rest.is_empty()).then(|| Accel {
        mods,
        key: rest.to_ascii_lowercase(),
    })
}

/// GNOME's built-in shortcuts: window manager, shell, Mutter (including Wayland's
/// Ctrl+Alt+F1…F12, which switch virtual terminals) and media keys.
const GNOME_SCHEMAS: [&str; 5] = [
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.mutter.wayland.keybindings",
    SCHEMA,
];

/// The keys and accelerators in `gsettings list-recursively` output (`schema key value`
/// lines); values that are not strings are left out.
pub fn bindings_in(listing: &str) -> Vec<(String, Vec<String>)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ' ');
            let _schema = parts.next()?;
            let key = parts.next()?;
            let value = parts.next()?.trim();
            if !(value.starts_with('[') || value.starts_with("@as") || value.starts_with('\'')) {
                return None;
            }
            let list = parse_list(value);
            (!list.is_empty()).then(|| (key.to_string(), list))
        })
        .collect()
}

fn uses(accelerators: &[String], wanted: &Accel) -> bool {
    accelerators
        .iter()
        .filter_map(|a| parse_accelerator(a))
        .any(|a| &a == wanted)
}

/// What else in GNOME already uses `binding`: its own shortcuts (by settings key, such
/// as `switch-to-session-12`) and the user's other custom shortcuts (by name).
/// DisplayGroups' own shortcuts are not counted.
pub fn conflicts(binding: &str) -> Result<Vec<String>, String> {
    let wanted = parse_accelerator(binding).ok_or_else(|| format!("not a key: {binding}"))?;
    let mut found = Vec::new();
    for schema in GNOME_SCHEMAS {
        // A schema can be missing (Mutter's Wayland keys on older systems).
        let Ok(listing) = gsettings(&["list-recursively", schema]) else {
            continue;
        };
        for (key, accelerators) in bindings_in(&listing) {
            if key != "custom-keybindings" && uses(&accelerators, &wanted) {
                found.push(key);
            }
        }
    }
    let current = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    for path in current.iter().filter(|p| !is_ours(p)) {
        let at = format!("{ENTRY_SCHEMA}:{path}");
        let value = |key: &str| {
            gsettings(&["get", &at, key])
                .map(|v| parse_list(&format!("[{v}]")).pop().unwrap_or_default())
                .unwrap_or_default()
        };
        if uses(&[value("binding")], &wanted) {
            let name = value("name");
            found.push(if name.is_empty() { path.clone() } else { name });
        }
    }
    found.dedup();
    Ok(found)
}

/// Whether one of DisplayGroups' own shortcuts in GNOME uses `binding`.
pub fn is_ours_now(binding: &str) -> bool {
    let Some(wanted) = parse_accelerator(binding) else {
        return false;
    };
    read().is_ok_and(|ours| {
        ours.iter()
            .any(|s| uses(std::slice::from_ref(&s.binding), &wanted))
    })
}

/// Our shortcuts as GNOME has them now (for tests and diagnostics).
pub fn read() -> Result<Vec<Shortcut>, String> {
    let current = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    current
        .iter()
        .filter(|p| is_ours(p))
        .map(|path| {
            let at = format!("{ENTRY_SCHEMA}:{path}");
            let get = |key: &str| -> Result<String, String> {
                Ok(parse_list(&format!("[{}]", gsettings(&["get", &at, key])?))
                    .pop()
                    .unwrap_or_default())
            };
            Ok(Shortcut {
                name: get("name")?,
                command: get("command")?,
                binding: get("binding")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_round_trip() {
        assert!(parse_list("@as []").is_empty());
        let list = parse_list("['/a/b/', '/it\\'s/']");
        assert_eq!(list, vec!["/a/b/".to_string(), "/it's/".to_string()]);
        assert_eq!(parse_list(&format_list(&list)), list);
        assert_eq!(format_list(&[]), "@as []");
    }

    #[test]
    fn the_users_own_shortcuts_are_kept() {
        let current = vec![
            format!("{BASE_PATH}custom0/"),
            our_path(0),
            our_path(1),
            format!("{BASE_PATH}custom1/"),
        ];
        let merged = merged_list(&current, 1);
        assert_eq!(
            merged,
            vec![
                format!("{BASE_PATH}custom0/"),
                format!("{BASE_PATH}custom1/"),
                our_path(0)
            ]
        );
        assert!(!is_ours("/elsewhere/displaygroups-0/"));
    }

    #[test]
    fn hotkeys_become_gnome_shortcuts() {
        let desk = Uuid::new_v4();
        let couch = Uuid::new_v4();
        let cfg: Config = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "groups": [
                { "id": desk, "name": "Desktop", "displays": [] },
                { "id": couch, "name": "Couch", "displays": [] }
            ],
            "hotkeys": [
                { "chord": "Ctrl+Alt+F12", "action": { "type": "toggle", "groups": [desk, couch] } },
                { "chord": "Win+Shift+R", "action": { "type": "recover" } },
                { "chord": "Ctrl+Nope", "action": { "type": "apply", "group": desk } }
            ]
        }))
        .unwrap();
        let exe = Path::new("/opt/Display Groups/displaygroups");
        let (shortcuts, problems) = for_config(&cfg, exe, "Recover");
        assert_eq!(shortcuts.len(), 2);
        assert_eq!(shortcuts[0].binding, "<Control><Alt>F12");
        assert_eq!(shortcuts[0].name, "DisplayGroups: Desktop / Couch");
        assert_eq!(
            shortcuts[0].command,
            format!("'/opt/Display Groups/displaygroups' toggle {desk} {couch}")
        );
        assert_eq!(shortcuts[1].binding, "<Shift><Super>r");
        assert_eq!(shortcuts[1].name, "DisplayGroups: Recover");
        assert!(shortcuts[1].command.ends_with(" recover"));
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn accelerators_compare_across_gnome_spellings() {
        let ours = parse_accelerator("<Control><Alt>F12").unwrap();
        assert_eq!(parse_accelerator("<Primary><Alt>F12"), Some(ours.clone()));
        assert_eq!(parse_accelerator("<Alt><Ctrl>f12"), Some(ours.clone()));
        assert_ne!(parse_accelerator("<Primary><Alt>F11"), Some(ours));
        assert_eq!(parse_accelerator("<Mod4>L"), parse_accelerator("<Super>l"));
        assert_eq!(parse_accelerator("<Super>"), None);
        assert_eq!(parse_accelerator(""), None);
    }

    #[test]
    fn gnome_bindings_are_read_from_gsettings_listings() {
        let listing = "\
org.gnome.mutter.wayland.keybindings switch-to-session-12 ['<Primary><Alt>F12']
org.gnome.mutter.wayland.keybindings restore-shortcuts @as []
org.gnome.desktop.wm.keybindings switch-applications ['<Super>Tab', '<Alt>Tab']
org.gnome.settings-daemon.plugins.media-keys volume-step 6
org.gnome.settings-daemon.plugins.media-keys screensaver-static 'XF86ScreenSaver'";
        let bindings = bindings_in(listing);
        assert_eq!(
            bindings,
            vec![
                (
                    "switch-to-session-12".to_string(),
                    vec!["<Primary><Alt>F12".to_string()]
                ),
                (
                    "switch-applications".to_string(),
                    vec!["<Super>Tab".to_string(), "<Alt>Tab".to_string()]
                ),
                (
                    "screensaver-static".to_string(),
                    vec!["XF86ScreenSaver".to_string()]
                ),
            ]
        );
        let wanted = parse_accelerator("<Control><Alt>F12").unwrap();
        assert!(uses(&bindings[0].1, &wanted));
        assert!(!uses(&bindings[1].1, &wanted));
    }

    #[test]
    fn commands_are_quoted_for_gnome() {
        assert_eq!(
            shell_quote("/usr/bin/displaygroups"),
            "/usr/bin/displaygroups"
        );
        assert_eq!(shell_quote("/opt/My Apps/dg"), "'/opt/My Apps/dg'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(
            gvariant_string("DisplayGroups: Sam's TV"),
            "'DisplayGroups: Sam\\'s TV'"
        );
    }
}

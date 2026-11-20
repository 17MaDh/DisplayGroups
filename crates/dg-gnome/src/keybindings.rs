//! Hotkeys as GNOME custom shortcuts (ARCHITECTURE.md §16.3, L2).
//!
//! On Wayland an application cannot grab global keys. GNOME's own "Custom Shortcuts"
//! (Settings → Keyboard) run a command for a key combination, so each DisplayGroups
//! hotkey becomes one of them, running `displaygroups apply …`. They live in GSettings
//! under paths of our own (`…/custom-keybindings/displaygroups-N/`), so the user's other
//! custom shortcuts are never touched, and they keep working when the agent is not
//! running. `gsettings` writes them, as GNOME Settings does through the same schema.

use std::process::Command;

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
    fn commands_are_quoted_for_gnome() {
        assert_eq!(
            shell_quote("/usr/bin/displaygroups"),
            "/usr/bin/displaygroups"
        );
        assert_eq!(shell_quote("/opt/My Apps/dg"), "'/opt/My Apps/dg'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(
            gvariant_string("DisplayGroups: Ahmad's TV"),
            "'DisplayGroups: Ahmad\\'s TV'"
        );
    }
}

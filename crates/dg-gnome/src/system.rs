//! Small facts about the Linux session that the CLI, the agent and the settings app
//! share: the display language, whether the agent runs, the system's name, and opening
//! folders.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// The display language as a lowercase code such as `"ar"` or `"en"`, for the
/// `"system"` language setting: the first usable of `LANGUAGE`, `LC_ALL`,
/// `LC_MESSAGES` and `LANG`, as gettext reads them (`ar_SA.UTF-8` → `ar`).
pub fn ui_language() -> String {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .flat_map(|v| v.split(':').map(str::to_string).collect::<Vec<_>>())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX" && !v.starts_with("C."))
        .map_or_else(|| "en".to_string(), |v| language_code(&v))
}

pub fn language_code(locale: &str) -> String {
    let code = locale
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if (2..=3).contains(&code.len()) && code.chars().all(|c| c.is_ascii_lowercase()) {
        code
    } else {
        "en".to_string()
    }
}

/// The agent's single-instance lock, in the state folder.
pub fn agent_lock_path(state_dir: &Path) -> PathBuf {
    state_dir.join("agent.lock")
}

/// Opens (creating it if needed) and takes the agent lock. `Ok(None)` when another
/// process holds it. The lock lasts as long as the returned file is open.
pub fn take_agent_lock(state_dir: &Path) -> Result<Option<File>, String> {
    std::fs::create_dir_all(state_dir).map_err(|e| format!("{}: {e}", state_dir.display()))?;
    let path = agent_lock_path(state_dir);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

/// Whether an agent holds its lock right now (checked by taking and releasing it).
pub fn agent_running(state_dir: &Path) -> bool {
    if !agent_lock_path(state_dir).exists() {
        return false;
    }
    matches!(take_agent_lock(state_dir), Ok(None))
}

/// "Ubuntu 24.04.1 LTS · GNOME" from `/etc/os-release` and the session's desktop.
pub fn description() -> String {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_release_name(&text))
        .unwrap_or_else(|| "Linux".to_string());
    match std::env::var("XDG_CURRENT_DESKTOP") {
        Ok(desktop) if !desktop.is_empty() => {
            let desktop = desktop
                .split(':')
                .next_back()
                .unwrap_or(&desktop)
                .to_string();
            format!("{os} · {desktop}")
        }
        _ => os,
    }
}

/// `PRETTY_NAME` (else `NAME`) from the text of `/etc/os-release`.
pub fn os_release_name(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?.strip_prefix('=')?;
            let v = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            (!v.is_empty()).then_some(v)
        })
    };
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

/// The AppImage file this process came from (`$APPIMAGE`), if any.
///
/// An AppImage runs from a temporary mount that goes away when the process it started
/// ends. Programs that must outlive their parent (the agent, the settings window
/// opened from the tray) and commands saved for later (autostart, GNOME shortcuts,
/// launchers, links) therefore go through this file, which mounts it again; its entry
/// point, the settings app, hands command-line arguments on to `displaygroups`.
pub fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Variables an AppImage's start-up hooks point into its own mount, so that the GTK and
/// WebKit bundled with it find their modules, schemas and themes.
const APPIMAGE_GTK_VARS: [&str; 10] = [
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GIO_MODULE_DIR",
    "GI_TYPELIB_PATH",
    "GSETTINGS_SCHEMA_DIR",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GTK_PATH",
    "GTK_THEME",
];

/// Search-path variables an AppImage may have put its own folders in front of.
const SEARCH_PATHS: [&str; 3] = ["XDG_DATA_DIRS", "PATH", "LD_LIBRARY_PATH"];

/// How to change the environment `vars` of a process running from an AppImage mounted at
/// `appdir`, for a program of the system: `(name, None)` removes a variable and
/// `(name, Some(value))` sets it. Without this, a GTK program started as a before/after
/// action would load the AppImage's GTK modules. Empty when `appdir` is not a folder.
pub fn host_env_changes(vars: &[(String, String)], appdir: &str) -> Vec<(String, Option<String>)> {
    if !appdir.starts_with('/') || appdir.len() < 2 {
        return Vec::new();
    }
    let mut changes = Vec::new();
    for (name, value) in vars {
        if APPIMAGE_GTK_VARS.contains(&name.as_str()) {
            // GTK_THEME is set from APPIMAGE_GTK_THEME, usually empty.
            if value.is_empty() || value.contains(appdir) {
                changes.push((name.clone(), None));
            }
        } else if SEARCH_PATHS.contains(&name.as_str()) {
            let kept: Vec<&str> = value
                .split(':')
                .filter(|p| !p.is_empty() && !p.starts_with(appdir))
                .collect();
            if kept.len() != value.split(':').count() {
                let joined = kept.join(":");
                changes.push((name.clone(), (!joined.is_empty()).then_some(joined)));
            }
        } else if matches!(name.as_str(), "APPDIR" | "ARGV0" | "OWD") {
            changes.push((name.clone(), None));
        }
    }
    changes
}

/// Prepares `command`, a program of the system (`gsettings`, `xdg-open`, an action), to
/// run with the session's own environment rather than the AppImage's, when this process
/// runs from an AppImage. DisplayGroups' own programs keep the environment as it is.
pub fn for_host(command: &mut std::process::Command) -> &mut std::process::Command {
    if let Ok(appdir) = std::env::var("APPDIR") {
        let vars: Vec<(String, String)> = std::env::vars().collect();
        for (name, value) in host_env_changes(&vars, &appdir) {
            match value {
                Some(v) => command.env(name, v),
                None => command.env_remove(name),
            };
        }
    }
    command
}

/// Opens a folder, file or URL with the desktop's default application.
pub fn open(target: &Path) -> Result<(), String> {
    for_host(&mut std::process::Command::new("xdg-open"))
        .arg(target)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot open {}: {e}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_become_language_codes() {
        assert_eq!(language_code("ar_SA.UTF-8"), "ar");
        assert_eq!(language_code("en_US"), "en");
        assert_eq!(language_code("sr@latin"), "sr");
        assert_eq!(language_code("C"), "en");
    }

    #[test]
    fn the_system_is_named_from_os_release() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(os_release_name("NAME=Fedora\n").as_deref(), Some("Fedora"));
        assert_eq!(os_release_name("ID=x\n"), None);
    }

    #[test]
    fn programs_of_the_system_get_the_session_environment_back() {
        let vars: Vec<(String, String)> = [
            ("GTK_PATH", "/tmp/.mount_x/usr/lib/gtk-3.0"),
            ("GTK_THEME", ""),
            (
                "GSETTINGS_SCHEMA_DIR",
                "/tmp/.mount_x//usr/share/glib-2.0/schemas",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_x/usr/share:/usr/share:/usr/local/share",
            ),
            ("PATH", "/usr/local/bin:/usr/bin"),
            ("APPDIR", "/tmp/.mount_x"),
            ("APPIMAGE", "/home/sam/DisplayGroups.AppImage"),
            ("HOME", "/home/sam"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let changes = host_env_changes(&vars, "/tmp/.mount_x");
        let change = |name: &str| {
            changes
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(change("GTK_PATH"), Some(None));
        assert_eq!(change("GTK_THEME"), Some(None));
        assert_eq!(change("GSETTINGS_SCHEMA_DIR"), Some(None));
        assert_eq!(
            change("XDG_DATA_DIRS"),
            Some(Some("/usr/share:/usr/local/share".to_string()))
        );
        assert_eq!(change("APPDIR"), Some(None));
        // Untouched: a PATH without the mount, APPIMAGE, everything else.
        assert_eq!(change("PATH"), None);
        assert_eq!(change("APPIMAGE"), None);
        assert_eq!(change("HOME"), None);
        // Not from an AppImage: nothing changes.
        assert!(host_env_changes(&vars, "").is_empty());
    }

    #[test]
    fn the_agent_lock_is_seen_while_held() {
        let dir = std::env::temp_dir().join(format!("dg-lock-{}", std::process::id()));
        assert!(!agent_running(&dir));
        let held = take_agent_lock(&dir).unwrap().expect("free at first");
        assert!(agent_running(&dir));
        assert!(take_agent_lock(&dir).unwrap().is_none());
        drop(held);
        assert!(!agent_running(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

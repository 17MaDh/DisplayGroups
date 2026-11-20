//! Desktop entry files: starting the agent at login, the `displaygroups://` link
//! handler, and launchers that switch to a group (ARCHITECTURE.md §16.3, L2). All of
//! them live in the user's own folders, like the HKCU entries on Windows.

use std::path::{Path, PathBuf};

pub const AUTOSTART_FILE: &str = "displaygroups.desktop";
pub const URL_HANDLER_FILE: &str = "displaygroups-url.desktop";
pub const SCHEME_MIME: &str = "x-scheme-handler/displaygroups";

/// The user's XDG folders that desktop entries go into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub autostart: PathBuf,
    /// Shown in GNOME's app grid ("Start menu").
    pub applications: PathBuf,
    pub desktop: PathBuf,
    pub mimeapps: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let config = absolute_env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let data = absolute_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        let desktop = std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| user_dir(&text, "XDG_DESKTOP_DIR", &home))
            .unwrap_or_else(|| home.join("Desktop"));
        Some(Self {
            autostart: config.join("autostart"),
            applications: data.join("applications"),
            desktop,
            mimeapps: config.join("mimeapps.list"),
        })
    }
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// A folder from `user-dirs.dirs` (`XDG_DESKTOP_DIR="$HOME/Schreibtisch"`).
pub fn user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(key) && l[key.len()..].trim_start().starts_with('='))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    path.is_absolute().then_some(path)
}

/// One `Exec` argument, quoted as the Desktop Entry spec requires: inside double quotes
/// `"`, `` ` ``, `$` and `\` are escaped, `%` is doubled, and the file format then escapes
/// every backslash once more.
pub fn exec_arg(arg: &str) -> String {
    let arg = arg.replace('%', "%%");
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:,=+@%".contains(c));
    if plain {
        return arg;
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

fn exec_line(exe: &Path, args: &[&str]) -> String {
    std::iter::once(exe.to_string_lossy().into_owned())
        .chain(args.iter().map(|a| (*a).to_string()))
        .map(|a| exec_arg(&a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A string value on one line (desktop files have no multi-line values).
fn value(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .replace('\\', "\\\\")
}

fn entry(lines: &[(&str, String)]) -> String {
    let mut out = String::from("[Desktop Entry]\nType=Application\n");
    for (key, val) in lines {
        out.push_str(key);
        out.push('=');
        out.push_str(val);
        out.push('\n');
    }
    out
}

pub fn autostart_entry(exe: &Path) -> String {
    entry(&[
        ("Name", "DisplayGroups".into()),
        (
            "Comment",
            "Hotkeys, tray icon and automatic switching for display groups".into(),
        ),
        // The URL handler and the agent both register `exe`; `%u` is only for links.
        ("Exec", exec_line(exe, &["agent"])),
        ("Icon", "video-display".into()),
        ("Terminal", "false".into()),
        ("NoDisplay", "true".into()),
        ("X-GNOME-Autostart-enabled", "true".into()),
    ])
}

pub fn url_handler_entry(exe: &Path) -> String {
    let mut exec = exec_line(exe, &["uri"]);
    exec.push_str(" %u");
    entry(&[
        ("Name", "DisplayGroups link handler".into()),
        ("Exec", exec),
        ("Icon", "video-display".into()),
        ("Terminal", "false".into()),
        ("NoDisplay", "true".into()),
        ("MimeType", format!("{SCHEME_MIME};")),
    ])
}

/// A launcher that switches to a group.
pub fn launcher_entry(exe: &Path, group_id: &str, group_name: &str, icon: &str) -> String {
    entry(&[
        ("Name", value(group_name)),
        (
            "Comment",
            value(&format!(
                "Switch displays to \"{group_name}\" (DisplayGroups)"
            )),
        ),
        ("Exec", exec_line(exe, &["apply", group_id])),
        ("Icon", value(icon)),
        ("Terminal", "false".into()),
        ("Categories", "Utility;".into()),
    ])
}

pub fn launcher_file_name(group_id: &str) -> String {
    format!("displaygroups-{group_id}.desktop")
}

/// The freedesktop icon for a built-in group icon (`builtin:gaming` → `input-gaming`).
pub fn theme_icon(builtin: &str) -> &'static str {
    match builtin {
        "gaming" => "input-gaming",
        "work" => "computer",
        _ => "video-display",
    }
}

/// `mimeapps.list` with `mime` set to `desktop` (or removed) under
/// `[Default Applications]`; everything else is kept as it was.
pub fn set_default_app(text: &str, mime: &str, desktop: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut seen_section = false;
    let mut written = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults && !written {
                if let Some(d) = desktop {
                    out.push(format!("{mime}={d}"));
                }
                written = true;
            }
            in_defaults = trimmed == "[Default Applications]";
            seen_section |= in_defaults;
        } else if in_defaults
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == mime)
        {
            if let (Some(d), false) = (desktop, written) {
                out.push(format!("{mime}={d}"));
                written = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if in_defaults && !written {
        if let Some(d) = desktop {
            out.push(format!("{mime}={d}"));
        }
    } else if !seen_section {
        if let Some(d) = desktop {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push("[Default Applications]".into());
            out.push(format!("{mime}={d}"));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// The desktop file registered for `mime` in `mimeapps.list`, if any.
pub fn default_app(text: &str, mime: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
        } else if in_defaults {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == mime {
                    return v.split(';').next().map(|s| s.trim().to_string());
                }
            }
        }
    }
    None
}

// ----- file operations -----

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Starts the agent at login (`Some`) or not (`None`).
pub fn set_autostart(dirs: &Dirs, exe: Option<&Path>) -> Result<(), String> {
    let path = dirs.autostart.join(AUTOSTART_FILE);
    match exe {
        Some(exe) => write(&path, &autostart_entry(exe)),
        None => remove(&path),
    }
}

pub fn autostart_enabled(dirs: &Dirs) -> bool {
    std::fs::read_to_string(dirs.autostart.join(AUTOSTART_FILE)).is_ok_and(|text| {
        !text
            .lines()
            .any(|l| matches!(l.trim(), "Hidden=true" | "X-GNOME-Autostart-enabled=false"))
    })
}

/// Makes `displaygroups://` links open DisplayGroups, for this user only.
pub fn register_scheme(dirs: &Dirs, exe: &Path) -> Result<(), String> {
    write(
        &dirs.applications.join(URL_HANDLER_FILE),
        &url_handler_entry(exe),
    )?;
    let text = std::fs::read_to_string(&dirs.mimeapps).unwrap_or_default();
    write(
        &dirs.mimeapps,
        &set_default_app(&text, SCHEME_MIME, Some(URL_HANDLER_FILE)),
    )
}

pub fn unregister_scheme(dirs: &Dirs) -> Result<(), String> {
    remove(&dirs.applications.join(URL_HANDLER_FILE))?;
    if let Ok(text) = std::fs::read_to_string(&dirs.mimeapps) {
        if default_app(&text, SCHEME_MIME).as_deref() == Some(URL_HANDLER_FILE) {
            write(&dirs.mimeapps, &set_default_app(&text, SCHEME_MIME, None))?;
        }
    }
    Ok(())
}

/// The command that handles `displaygroups://` links, when it is DisplayGroups'.
pub fn scheme_command(dirs: &Dirs) -> Option<String> {
    let text = std::fs::read_to_string(&dirs.mimeapps).ok()?;
    if default_app(&text, SCHEME_MIME).as_deref() != Some(URL_HANDLER_FILE) {
        return None;
    }
    let entry = std::fs::read_to_string(dirs.applications.join(URL_HANDLER_FILE)).ok()?;
    entry
        .lines()
        .find_map(|l| l.strip_prefix("Exec=").map(str::to_string))
}

/// Writes a launcher into `dir` and marks it executable and trusted, so GNOME's desktop
/// runs it instead of opening it as text.
pub fn write_launcher(dir: &Path, file_name: &str, text: &str) -> Result<PathBuf, String> {
    let path = dir.join(file_name);
    write(&path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // Best effort: the GNOME desktop icons extension asks for this flag.
        let _ = std::process::Command::new("gio")
            .args(["set", &path.to_string_lossy(), "metadata::trusted", "true"])
            .output();
    }
    Ok(path)
}

/// A local path from a `file://` URI (as the file chooser portal returns them), with
/// percent-escapes decoded. `None` for other schemes or a remote host.
pub fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let hex = |i: usize| {
        bytes
            .get(i)
            .and_then(|b| char::from(*b).to_digit(16))
            .map(|d| d as u8)
    };
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(hi), Some(lo)) = (hex(i + 1), hex(i + 2)) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

/// Picture types a custom group icon may be.
pub const ICON_EXTENSIONS: [&str; 3] = ["png", "svg", "ico"];

/// The name a custom icon is stored under in the icons folder (`custom-<file name>`),
/// or an error when `source` is not a PNG, SVG or ICO file.
pub fn icon_file_name(source: &Path) -> Result<String, String> {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} is not a file", source.display()))?;
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !ICON_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("{name}: use a PNG, SVG or ICO picture"));
    }
    Ok(format!("custom-{name}"))
}

/// Copies a picture into the icons folder for `group.icon` and returns that value
/// (`file:custom-<name>`).
pub fn import_icon(icons_dir: &Path, source: &Path) -> Result<String, String> {
    let file = icon_file_name(source)?;
    let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    dg_model::store::write_atomic(&icons_dir.join(&file), &bytes).map_err(|e| e.to_string())?;
    Ok(format!("file:{file}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            path_from_uri("file:///home/u/My%20Pictures/tv%E2%9C%93.png"),
            Some(PathBuf::from("/home/u/My Pictures/tv✓.png"))
        );
        assert_eq!(
            path_from_uri("file://localhost/tmp/a%zz.png"),
            Some(PathBuf::from("/tmp/a%zz.png"))
        );
        assert_eq!(path_from_uri("https://example.org/a.png"), None);
        assert_eq!(path_from_uri("file://server/share/a.png"), None);
    }

    #[test]
    fn custom_icons_are_pictures() {
        assert_eq!(
            icon_file_name(Path::new("/home/u/Pictures/tv.PNG")).unwrap(),
            "custom-tv.PNG"
        );
        assert!(icon_file_name(Path::new("/home/u/notes.txt")).is_err());
        assert!(icon_file_name(Path::new("/home/u/noext")).is_err());
    }

    #[test]
    fn exec_arguments_are_quoted_per_the_spec() {
        assert_eq!(exec_arg("/usr/bin/displaygroups"), "/usr/bin/displaygroups");
        assert_eq!(exec_arg("my dir/dg"), "\"my dir/dg\"");
        assert_eq!(exec_arg("50%"), "50%%");
        assert_eq!(exec_arg("a\"b"), "\"a\\\\\"b\"");
        assert_eq!(exec_arg("C:\\x"), "\"C:\\\\\\\\x\"");
        assert_eq!(exec_arg(""), "\"\"");
    }

    #[test]
    fn entries_point_at_the_executable() {
        let exe = Path::new("/opt/Display Groups/displaygroups");
        let auto = autostart_entry(exe);
        assert!(auto.contains("Exec=\"/opt/Display Groups/displaygroups\" agent\n"));
        assert!(auto.contains("X-GNOME-Autostart-enabled=true"));
        let url = url_handler_entry(exe);
        assert!(url.contains(" uri %u\n"));
        assert!(url.contains("MimeType=x-scheme-handler/displaygroups;\n"));
        let launcher = launcher_entry(exe, "b5a5", "Gaming\non Couch", "input-gaming");
        assert!(launcher.contains("Name=Gaming on Couch\n"));
        assert!(launcher.contains(" apply b5a5\n"));
        assert!(launcher.contains("Icon=input-gaming\n"));
    }

    #[test]
    fn mimeapps_keeps_everything_else() {
        let text = "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\nx-scheme-handler/http=firefox.desktop\n";
        let set = set_default_app(text, SCHEME_MIME, Some(URL_HANDLER_FILE));
        assert!(set.contains("text/plain=gedit.desktop;"));
        assert!(set.contains("x-scheme-handler/http=firefox.desktop"));
        assert_eq!(
            default_app(&set, SCHEME_MIME).as_deref(),
            Some(URL_HANDLER_FILE)
        );
        // Setting again replaces instead of adding a second line.
        let again = set_default_app(&set, SCHEME_MIME, Some(URL_HANDLER_FILE));
        assert_eq!(again.matches(SCHEME_MIME).count(), 1);
        let removed = set_default_app(&again, SCHEME_MIME, None);
        assert_eq!(default_app(&removed, SCHEME_MIME), None);
        assert!(removed.contains("x-scheme-handler/http=firefox.desktop"));
        // An empty file gets the section.
        let fresh = set_default_app("", SCHEME_MIME, Some(URL_HANDLER_FILE));
        assert_eq!(
            fresh,
            "[Default Applications]\nx-scheme-handler/displaygroups=displaygroups-url.desktop\n"
        );
    }

    // Unix paths: `/home/a` is not absolute on Windows.
    #[cfg(unix)]
    #[test]
    fn user_dirs_are_read() {
        let home = Path::new("/home/a");
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/سطح المكتب\"\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n";
        assert_eq!(
            user_dir(text, "XDG_DESKTOP_DIR", home),
            Some(home.join("سطح المكتب"))
        );
        assert_eq!(user_dir(text, "XDG_MUSIC_DIR", home), None);
    }

    #[test]
    fn files_are_written_and_removed() {
        let root = std::env::temp_dir().join(format!("dg-desktop-{}", std::process::id()));
        let dirs = Dirs {
            autostart: root.join("autostart"),
            applications: root.join("applications"),
            desktop: root.join("Desktop"),
            mimeapps: root.join("mimeapps.list"),
        };
        let exe = Path::new("/usr/bin/displaygroups");
        assert!(!autostart_enabled(&dirs));
        set_autostart(&dirs, Some(exe)).unwrap();
        assert!(autostart_enabled(&dirs));
        set_autostart(&dirs, None).unwrap();
        assert!(!autostart_enabled(&dirs));
        register_scheme(&dirs, exe).unwrap();
        assert!(scheme_command(&dirs).unwrap().ends_with(" uri %u"));
        unregister_scheme(&dirs).unwrap();
        assert_eq!(scheme_command(&dirs), None);
        let _ = std::fs::remove_dir_all(root);
    }
}

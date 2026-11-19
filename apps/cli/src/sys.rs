//! The few system facts and actions the shared CLI code needs, per platform.

/// The display language as a lowercase code such as `"ar"` or `"en"`, for the
/// `"system"` language setting.
#[cfg(windows)]
pub fn ui_language() -> String {
    dg_win::system::ui_language()
}

/// Linux: the first usable of `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG`, as
/// gettext reads them (`ar_SA.UTF-8` → `ar`).
#[cfg(not(windows))]
pub fn ui_language() -> String {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .flat_map(|v| v.split(':').map(str::to_string).collect::<Vec<_>>())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX" && !v.starts_with("C."))
        .map_or_else(|| "en".to_string(), |v| language_code(&v))
}

#[cfg(not(windows))]
fn language_code(locale: &str) -> String {
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

/// Whether the system theme is dark, for native dialogs.
#[cfg(windows)]
pub fn prefers_dark() -> bool {
    dg_win::system::system_prefers_dark()
}

#[cfg(not(windows))]
pub fn prefers_dark() -> bool {
    false
}

/// An error from the windowless executable, which has no console to print to.
#[cfg(windows)]
pub fn show_error(title: &str, text: &str) {
    dg_win::ui::show_error(title, text);
}

#[cfg(not(windows))]
pub fn show_error(_title: &str, text: &str) {
    eprintln!("error: {text}");
}

#[cfg(windows)]
pub fn show_info(title: &str, text: &str) {
    dg_win::ui::show_info(title, text);
}

#[cfg(not(windows))]
pub fn show_info(_title: &str, text: &str) {
    println!("{text}");
}

/// Starts a program for a before/after action, without waiting for it.
#[cfg(windows)]
pub fn spawn(path: &str, args: &[String], working_dir: Option<&str>) -> Result<(), String> {
    dg_win::shell::spawn(path, args, working_dir)
}

#[cfg(not(windows))]
pub fn spawn(path: &str, args: &[String], working_dir: Option<&str>) -> Result<(), String> {
    let mut command = std::process::Command::new(path);
    command.args(args);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    command
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start {path}: {e}"))
}

/// Opens a file, folder or URL with its default application.
#[cfg(windows)]
pub fn open(target: &str) -> Result<(), String> {
    dg_win::shell::open(target)
}

#[cfg(not(windows))]
pub fn open(target: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot open {target}: {e}"))
}

#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn locales_become_language_codes() {
        assert_eq!(super::language_code("ar_SA.UTF-8"), "ar");
        assert_eq!(super::language_code("en_US"), "en");
        assert_eq!(super::language_code("sr@latin"), "sr");
        assert_eq!(super::language_code("C"), "en");
    }
}

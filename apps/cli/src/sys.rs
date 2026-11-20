//! The few system facts and actions the shared CLI code needs, per platform.

/// The display language as a lowercase code such as `"ar"` or `"en"`, for the
/// `"system"` language setting.
#[cfg(windows)]
pub fn ui_language() -> String {
    dg_win::system::ui_language()
}

/// Linux: from the locale variables, as gettext reads them (`ar_SA.UTF-8` → `ar`).
#[cfg(not(windows))]
pub fn ui_language() -> String {
    dg_gnome::system::ui_language()
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
    crate::output::stderr_line(&format!("error: {text}"));
}

#[cfg(windows)]
pub fn show_info(title: &str, text: &str) {
    dg_win::ui::show_info(title, text);
}

#[cfg(not(windows))]
pub fn show_info(_title: &str, text: &str) {
    crate::output::stdout_line(text);
}

/// Starts a program for a before/after action, without waiting for it.
#[cfg(windows)]
pub fn spawn(path: &str, args: &[String], working_dir: Option<&str>) -> Result<(), String> {
    dg_win::shell::spawn(path, args, working_dir)
}

#[cfg(not(windows))]
pub fn spawn(path: &str, args: &[String], working_dir: Option<&str>) -> Result<(), String> {
    let mut command = std::process::Command::new(path);
    dg_gnome::system::for_host(&mut command).args(args);
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
    dg_gnome::system::for_host(&mut std::process::Command::new("xdg-open"))
        .arg(target)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot open {target}: {e}"))
}

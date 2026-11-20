//! The settings app on Linux (GNOME, ARCHITECTURE.md §16.6): Mutter's DisplayConfig for
//! the status and modes, its `MonitorsChanged` signal for changes, GNOME's shortcuts
//! for hotkey checks, the file chooser portal, and "Identify" with windows of our own.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use dg_engine::Status;
use dg_gnome::dbus::Mutter;
use dg_gnome::desktop::{self, Dirs};
use dg_gnome::{keybindings, system, DisplayState};
use dg_hotkey::Chord;
use dg_identity::match_monitor;
use dg_model::paths::AppPaths;
use dg_model::{store, Config};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use crate::{App, Integration, ModeOption, Probe};

pub const OS: &str = "linux";
pub const CLI: &str = "displaygroups";
/// GNOME asks "Keep these display settings?" itself, so the same CLI switches.
pub const CLI_SWITCH: &str = "displaygroups";

pub fn no_console(_command: &mut Command) {}

pub fn build() -> u32 {
    0
}

pub fn mica() -> bool {
    false
}

pub fn description() -> String {
    system::description()
}

pub fn ui_language() -> String {
    system::ui_language()
}

/// Errors before the window exists: a notification, since the app is usually started
/// from the app grid without a terminal.
pub fn show_error(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    let _ = dg_gnome::notify::notify(title, text, "dialog-error", 10_000);
}

/// Opens the folder that holds `path` (GNOME Files has no portable "select" argument).
pub fn reveal(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = system::open(dir);
    }
}

pub fn open_folder(path: &Path) -> Result<(), String> {
    system::open(path)
}

pub fn decorate(_window: &WebviewWindow) {}

/// Whether GNOME can bind the chord, and what else in GNOME already uses it (its own
/// shortcuts win over custom ones: Ctrl+Alt+F1…F12 switch virtual terminals, for
/// example).
pub fn probe(chord: &Chord) -> Probe {
    let Some(binding) = chord.gtk_accelerator() else {
        return Probe {
            error: Some("GNOME has no name for this key".into()),
            ..Probe::default()
        };
    };
    let ours = keybindings::is_ours_now(&binding);
    // Without GNOME's settings nothing can be checked; say it is free.
    let known = keybindings::conflicts(&binding).unwrap_or_default();
    Probe {
        free: known.is_empty(),
        reserved: None,
        known,
        ours: Some(ours),
        error: None,
    }
}

pub fn agent_running(paths: &AppPaths) -> bool {
    system::agent_running(&paths.state_dir)
}

pub fn integration() -> Integration {
    let dirs = Dirs::from_env();
    Integration {
        startup: dirs.as_ref().is_some_and(desktop::autostart_enabled),
        protocol: dirs.as_ref().and_then(desktop::scheme_command).is_some(),
    }
}

fn display_state() -> Result<DisplayState, String> {
    Mutter::connect()
        .and_then(|m| m.state())
        .map_err(|e| e.to_string())
}

pub fn display_status(config: &Config) -> Result<(Status, Option<Uuid>), String> {
    let state = display_state()?;
    let status = dg_gnome::status(config, &state);
    let ids: Vec<Uuid> = config.groups.iter().map(|g| g.id).collect();
    let current = dg_gnome::current_group(config, &state, &ids);
    Ok((status, current))
}

/// GNOME lists the modes of every connected monitor, also one that is off.
pub fn monitor_modes(config: &Config, monitor: &str) -> Result<Vec<ModeOption>, String> {
    let entry = config.monitors.get(monitor).ok_or("unknown monitor")?;
    let state = display_state()?;
    let observed: Vec<_> = state
        .monitors
        .iter()
        .map(dg_gnome::observe::observe)
        .collect();
    let m = match_monitor(&entry.identity, &observed).map_err(|_| "off".to_string())?;
    Ok(state.monitors[m.index]
        .modes
        .iter()
        .map(|mode| ModeOption {
            width: mode.width.max(mode.height),
            height: mode.width.min(mode.height),
            refresh_hz: mode.refresh.round() as u32,
        })
        .collect())
}

/// How long the displays must be quiet before the page is told (GNOME sends a burst of
/// changes per switch).
const SETTLE: Duration = Duration::from_millis(700);
const CONFIG_POLL: Duration = Duration::from_secs(1);

/// Tells the page when GNOME's displays change (`MonitorsChanged`) and when the config
/// file changes on disk.
pub fn watch_changes(app: AppHandle, config_file: PathBuf) {
    let (tx, rx) = mpsc::channel::<()>();
    if let Err(e) = dg_gnome::dbus::watch_changes(move || {
        let _ = tx.send(());
    }) {
        eprintln!("display changes are not followed: {e}");
    }
    let displays = app.clone();
    let _ = std::thread::Builder::new()
        .name("display-watch".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(SETTLE).is_ok() {}
                let _ = displays.emit("displays-changed", ());
            }
        });
    let _ = std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let stamp = || {
                std::fs::metadata(&config_file)
                    .and_then(|m| m.modified())
                    .ok()
            };
            let mut last: Option<SystemTime> = stamp();
            loop {
                std::thread::sleep(CONFIG_POLL);
                let now = stamp();
                if now != last {
                    last = now;
                    let _ = app.emit("config-changed", ());
                }
            }
        });
}

/// This user's programs, without the session's own services.
pub fn running_programs() -> Vec<String> {
    dg_gnome::detect::user_programs()
}

/// "Choose file…" for a group icon: the desktop's file chooser (portal), then the
/// picture is copied into the icons folder. `None` if cancelled.
#[tauri::command]
pub async fn pick_icon(
    app: State<'_, App>,
    title: String,
    filter: String,
) -> Result<Option<String>, String> {
    let icons_dir = app.paths.icons_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let types = ["image/png", "image/svg+xml", "image/vnd.microsoft.icon"];
        let Some(path) = dg_gnome::portal::open_file(&title, &filter, &types)? else {
            return Ok(None);
        };
        desktop::import_icon(&icons_dir, &path).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The agent holds the tray, triggers and the fallback, and keeps GNOME's shortcuts in
/// step with the config. Opening the settings app starts it if it is not running.
/// Debug builds never start it, and neither does `DISPLAYGROUPS_NO_AGENT=1`.
pub fn start_agent(exe_dir: &Path, paths: &AppPaths) {
    if cfg!(debug_assertions) || std::env::var_os("DISPLAYGROUPS_NO_AGENT").is_some() {
        return;
    }
    let agent = exe_dir.join(CLI);
    if system::agent_running(&paths.state_dir) || !agent.is_file() {
        return;
    }
    let _ = Command::new(agent)
        .arg("agent")
        .current_dir(exe_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// What one display shows during "Identify" (read by `identify.html`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Label {
    number: u32,
    name: String,
    mode: String,
    #[serde(skip)]
    x: i32,
    #[serde(skip)]
    y: i32,
}

fn hz(milli: u32) -> String {
    if milli.is_multiple_of(1000) {
        (milli / 1000).to_string()
    } else {
        format!("{:.2}", f64::from(milli) / 1000.0)
    }
}

/// One label per display that is on. Monitors in the config keep their number from
/// `Config::monitor_number`, as in the layout editor; others are numbered after them.
fn labels(config: &Config, state: &DisplayState) -> Vec<Label> {
    let status = dg_gnome::status(config, state);
    let mut next_unknown = u32::try_from(config.monitors.len()).unwrap_or(u32::MAX);
    status
        .displays
        .iter()
        .map(|d| {
            let key = d.monitor_key.as_deref();
            let number = key
                .and_then(|k| config.monitor_number(k))
                .unwrap_or_else(|| {
                    next_unknown += 1;
                    next_unknown
                });
            let name = key
                .and_then(|k| config.monitors.get(k))
                .map_or_else(|| d.name.clone(), |m| m.display_name.clone());
            let rate = d
                .refresh_milli_hz
                .map(|m| format!(" · {} Hz", hz(m)))
                .unwrap_or_default();
            Label {
                number,
                name,
                mode: format!("{} × {}{rate}", d.width, d.height),
                x: d.x,
                y: d.y,
            }
        })
        .collect()
}

/// For each of `wanted` (Mutter's layout positions), the index of the toolkit monitor in
/// the same place. Both describe the same layout, in logical (Wayland) or scaled (X11)
/// units, so the order of their positions matches even when the numbers do not.
fn pair_by_position(wanted: &[(i32, i32)], monitors: &[(i32, i32)]) -> Vec<Option<usize>> {
    if wanted.len() == monitors.len() {
        let order = |list: &[(i32, i32)]| {
            let mut idx: Vec<usize> = (0..list.len()).collect();
            idx.sort_by_key(|&i| list[i]);
            idx
        };
        let (w, m) = (order(wanted), order(monitors));
        let mut out = vec![None; wanted.len()];
        for (wi, mi) in w.into_iter().zip(m) {
            out[wi] = Some(mi);
        }
        return out;
    }
    wanted
        .iter()
        .map(|p| monitors.iter().position(|m| m == p))
        .collect()
}

/// Puts each label window full screen on its display and shows it. It is also moved and
/// sized to the display first, for X11 window managers that ignore the full-screen
/// request (Wayland ignores the move). GTK calls belong on the main thread.
fn place(windows: &[WebviewWindow], positions: &[(i32, i32)]) {
    use gtk::prelude::*;
    let monitors: Vec<gtk::gdk::Rectangle> = gtk::gdk::Display::default()
        .map(|display| {
            (0..display.n_monitors())
                .filter_map(|i| display.monitor(i))
                .map(|m| m.geometry())
                .collect()
        })
        .unwrap_or_default();
    let origins: Vec<(i32, i32)> = monitors.iter().map(|g| (g.x(), g.y())).collect();
    let targets = pair_by_position(positions, &origins);
    for ((window, target), at) in windows.iter().zip(targets).zip(positions) {
        match (window.gtk_window(), target) {
            (Ok(gtk_window), Some(index)) => {
                let g = &monitors[index];
                eprintln!(
                    "identify: the display at {at:?} is toolkit monitor {index} ({}×{} at {}, {})",
                    g.width(),
                    g.height(),
                    g.x(),
                    g.y()
                );
                gtk_window.move_(g.x(), g.y());
                gtk_window.resize(g.width(), g.height());
                if let Some(screen) = GtkWindowExt::screen(&gtk_window) {
                    gtk_window.fullscreen_on_monitor(&screen, index as i32);
                }
            }
            _ => {
                eprintln!("identify: no toolkit monitor matches the display at {at:?}");
                let _ = window.set_fullscreen(true);
            }
        }
        let _ = window.show();
    }
}

/// "Identify": a borderless, see-through window on each display that is on, with its
/// number, name and mode, for `seconds`. GNOME Shell keeps its own monitor labels for
/// GNOME Settings, and Wayland apps cannot place windows, but they can ask to be full
/// screen on a given display.
pub fn identify(app: &AppHandle, seconds: u32) -> Result<(), String> {
    let config_file = app.state::<App>().paths.config_file.clone();
    let config = store::load(&config_file).map_err(|e| e.to_string())?.config;
    let labels = labels(&config, &display_state()?);
    if labels.is_empty() {
        return Err("no display is on".into());
    }
    let mut windows = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let data = serde_json::to_string(label).map_err(|e| e.to_string())?;
        let window = WebviewWindowBuilder::new(
            app,
            format!("identify-{i}"),
            WebviewUrl::App("identify.html".into()),
        )
        .title("DisplayGroups")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .initialization_script(format!("window.__DG_IDENTIFY = {data};"))
        .build()
        .map_err(|e| e.to_string())?;
        windows.push(window);
    }
    let positions: Vec<(i32, i32)> = labels.iter().map(|l| (l.x, l.y)).collect();
    let placed = windows.clone();
    app.run_on_main_thread(move || place(&placed, &positions))
        .map_err(|e| e.to_string())?;
    let _ = std::thread::Builder::new()
        .name("identify".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(u64::from(seconds)));
            for window in windows {
                let _ = window.destroy();
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_windows_follow_the_layout_order() {
        // Mutter's logical layout and GDK's monitors in another unit and order.
        let wanted = [(1920, 0), (0, 0)];
        let gdk = [(0, 0), (960, 0)];
        assert_eq!(pair_by_position(&wanted, &gdk), vec![Some(1), Some(0)]);
        // A different count: exact positions only.
        assert_eq!(
            pair_by_position(&[(0, 0), (1920, 0)], &[(0, 0)]),
            vec![Some(0), None]
        );
    }

    #[test]
    fn refresh_rates_are_short() {
        assert_eq!(hz(60_000), "60");
        assert_eq!(hz(119_880), "119.88");
    }
}

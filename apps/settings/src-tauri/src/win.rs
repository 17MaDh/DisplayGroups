//! The settings app on Windows: the CCD display backend, Mica, a hidden window for
//! display changes, `RegisterHotKey` probes and the Windows dialogs (ARCHITECTURE.md §9).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use dg_engine::ccd::Scope;
use dg_engine::{DisplayBackend, Engine, Status};
use dg_hotkey::Chord;
use dg_identity::match_monitor;
use dg_model::paths::AppPaths;
use dg_model::{store, Config};
use dg_win::window::{AgentEvent, AgentWindow};
use dg_win::WinBackend;
use tauri::{Emitter, State};
use uuid::Uuid;

use crate::{App, Integration, ModeOption, Probe};

pub const OS: &str = "windows";
/// The console CLI, for commands whose output the page shows.
pub const CLI: &str = "displaygroups.exe";
/// The windowless CLI for switching: it shows the countdown and its own error dialogs.
pub const CLI_SWITCH: &str = "displaygroupsw.exe";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn no_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn build() -> u32 {
    dg_win::system::windows_build()
}

/// Windows 11 draws Mica behind the window.
pub fn mica() -> bool {
    build() >= 22000
}

pub fn description() -> String {
    format!("Windows build {}", build())
}

pub fn ui_language() -> String {
    dg_win::system::ui_language()
}

pub fn show_error(title: &str, text: &str) {
    dg_win::ui::show_error(title, text);
}

/// Shows a file selected in Explorer.
pub fn reveal(path: &Path) {
    let _ = Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

pub fn open_folder(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

pub fn decorate(window: &tauri::WebviewWindow) {
    if mica() {
        let effects = tauri::window::EffectsBuilder::new()
            .effect(tauri::window::Effect::Mica)
            .build();
        let _ = window.set_effects(effects);
    }
}

/// Whether Windows or another program holds the chord, by trying `RegisterHotKey`.
pub fn probe(chord: &Chord) -> Probe {
    let reserved = chord.reserved_by_windows().map(str::to_string);
    Probe {
        free: reserved.is_none()
            && dg_win::system::hotkey_is_free(chord.modifiers.bits(), chord.vk),
        reserved,
        known: chord
            .known_conflicts()
            .into_iter()
            .map(str::to_string)
            .collect(),
        ours: None,
        error: None,
    }
}

pub fn agent_running(_paths: &AppPaths) -> bool {
    dg_win::agent::is_running()
}

pub fn integration() -> Integration {
    Integration {
        startup: dg_win::integration::startup_enabled(),
        protocol: dg_win::integration::protocol_command().is_some(),
    }
}

pub fn display_status(config: &Config) -> Result<(Status, Option<Uuid>), String> {
    let backend = WinBackend::new();
    let engine = Engine::new(&backend);
    let status = engine.status(config).map_err(|e| e.to_string())?;
    let ids: Vec<Uuid> = config.groups.iter().map(|g| g.id).collect();
    let current = engine.current_group(config, &ids).ok().flatten();
    Ok((status, current))
}

/// Modes of a monitor that is on (Windows only lists modes of active displays).
pub fn monitor_modes(config: &Config, monitor: &str) -> Result<Vec<ModeOption>, String> {
    let entry = config.monitors.get(monitor).ok_or("unknown monitor")?;
    let backend = WinBackend::new();
    let active = backend.query(Scope::Active).map_err(|e| e.to_string())?;
    let observed: Vec<_> = active
        .targets
        .iter()
        .map(dg_engine::observe::observe)
        .collect();
    let m = match_monitor(&entry.identity, &observed).map_err(|_| "off".to_string())?;
    let target = active.targets[m.index].target;
    Ok(backend
        .list_modes(target)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|d| d.refresh_hz > 1 && d.width >= 640)
        .map(|d| ModeOption {
            width: d.width.max(d.height),
            height: d.width.min(d.height),
            refresh_hz: d.refresh_hz,
        })
        .collect())
}

/// Window class of the settings app's hidden event window (not the agent's class, so
/// `dg_win::agent::is_running` is not fooled).
const WATCH_WINDOW_CLASS: &str = "DisplayGroups.Settings.Watch";
const TIMER_DISPLAYS: usize = 1;

/// Tells the page when Windows' displays change (a display turned on or off,
/// connected, removed, rearranged or set to another mode) and when the config file
/// changes on disk, so the window shows what Windows has now.
pub fn watch_changes(app: tauri::AppHandle, config_file: PathBuf) {
    if let Some(dir) = config_file.parent() {
        let handle = app.clone();
        let _ = dg_win::system::watch_dir(dir, move || {
            let _ = handle.emit("config-changed", ());
        });
    }
    let _ = std::thread::Builder::new()
        .name("display-watch".into())
        .spawn(move || {
            let Ok(window) = AgentWindow::create_with(WATCH_WINDOW_CLASS, "DisplayGroups Settings")
            else {
                return;
            };
            window.run(|event| {
                match event {
                    // Windows sends several messages per change; report once it settles.
                    AgentEvent::DisplayChanged | AgentEvent::DeviceChanged => {
                        window.set_timer(TIMER_DISPLAYS, 700)
                    }
                    AgentEvent::Timer(TIMER_DISPLAYS) => {
                        window.kill_timer(TIMER_DISPLAYS);
                        let _ = app.emit("displays-changed", ());
                    }
                    _ => {}
                }
                true
            });
        });
}

/// Programs with a window open right now (lower-case executable names, without
/// Windows shell processes).
pub fn running_programs() -> Vec<String> {
    dg_win::detect::programs_with_windows()
}

/// "Identify" through the CLI, which draws OSD-style labels on each display.
pub fn identify(app: &tauri::AppHandle, seconds: u32) -> Result<(), String> {
    use tauri::Manager;
    let state = app.state::<App>();
    let exe = state.exe_dir.join(CLI);
    let mut command = Command::new(&exe);
    command
        .args(["identify", "--seconds", &seconds.to_string(), "--config"])
        .arg(&state.paths.config_file);
    no_console(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// "Choose file…" for a group icon: the Windows Open dialog, then the file is stored in
/// the icons folder as `.ico`. Returns the value for `group.icon`; `None` if cancelled.
#[tauri::command]
pub fn pick_icon(
    app: State<App>,
    window: tauri::WebviewWindow,
    title: String,
    filter: String,
) -> Result<Option<String>, String> {
    let owner = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    let Some(source) = dg_win::ui::pick_file(owner, &title, &filter, "*.ico;*.png")? else {
        return Ok(None);
    };
    let (file, ico) = dg_win::icons::import_file(&source)?;
    store::write_atomic(&app.paths.icons_dir.join(&file), &ico).map_err(|e| e.to_string())?;
    Ok(Some(format!("file:{file}")))
}

/// Hotkeys, the tray and the OSD live in the agent. Opening the settings app (for
/// example from the installer's last page) starts it if it is not running, so the
/// hotkeys being edited are live. Debug builds never start it, and neither does
/// `DISPLAYGROUPS_NO_AGENT=1`, so working on the UI does not arm real hotkeys.
pub fn start_agent(exe_dir: &Path, _paths: &AppPaths) {
    if cfg!(debug_assertions) || std::env::var_os("DISPLAYGROUPS_NO_AGENT").is_some() {
        return;
    }
    let agent = exe_dir.join(CLI_SWITCH);
    if dg_win::agent::is_running() || !agent.is_file() {
        return;
    }
    let _ = Command::new(agent)
        .arg("agent")
        .current_dir(exe_dir)
        .spawn();
}

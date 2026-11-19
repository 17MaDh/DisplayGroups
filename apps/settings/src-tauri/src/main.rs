//! DisplayGroups settings app (ARCHITECTURE.md §9). Runs only while its window is open.
//!
//! Config editing, validation, monitor status/modes and the hotkey probe run
//! in-process. Actions with side effects that the CLI already implements (capture,
//! validate, shortcuts, startup, protocol, test-apply) run `displaygroups.exe` /
//! `displaygroupsw.exe` next to this executable, so there is a single implementation.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{BTreeMap, HashMap};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use dg_engine::ccd::Scope;
use dg_engine::{DisplayBackend, Engine, Status};
use dg_hotkey::Chord;
use dg_identity::match_monitor;
use dg_model::paths::AppPaths;
use dg_model::validate::{self, Issue};
use dg_model::{store, Config};
use dg_win::WinBackend;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct App {
    paths: AppPaths,
    exe_dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigState {
    config: Config,
    issues: Vec<Issue>,
    config_path: String,
    logs_dir: String,
    portable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    saved: bool,
    issues: Vec<Issue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CliResult {
    ok: bool,
    code: i32,
    output: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HotkeyProbe {
    valid: bool,
    canonical: Option<String>,
    error: Option<String>,
    /// `None` when the chord is invalid.
    free: Option<bool>,
    reserved: Option<String>,
    known: Vec<String>,
    /// Whether the DisplayGroups agent is running (it holds the saved chords).
    agent_running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Platform {
    mica: bool,
    build: u32,
    version: String,
    languages: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Integration {
    startup: bool,
    protocol: bool,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
struct ModeOption {
    width: u32,
    height: u32,
    refresh_hz: u32,
}

/// CLI operations the UI may request. Arguments are built here, never taken raw from
/// the page.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum CliCommand {
    Capture {
        name: String,
    },
    Validate {
        id: Uuid,
    },
    Shortcut {
        id: Uuid,
        desktop: bool,
        #[serde(rename = "startMenu")]
        start_menu: bool,
    },
    Startup {
        on: bool,
    },
    Protocol {
        on: bool,
    },
}

fn issues_have_errors(issues: &[Issue]) -> bool {
    validate::has_errors(issues)
}

#[tauri::command]
fn platform(app: State<App>) -> Platform {
    let build = dg_win::system::windows_build();
    let mut languages = vec!["en".to_string()];
    if let Ok(dir) = std::fs::read_dir(app.exe_dir.join("locales")) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) {
                    if stem != "en" {
                        languages.push(stem);
                    }
                }
            }
        }
    }
    Platform {
        mica: build >= 22000,
        build,
        version: env!("CARGO_PKG_VERSION").to_string(),
        languages,
    }
}

/// Strings for a language other than English (English is bundled with the UI).
#[tauri::command]
fn locale(app: State<App>, language: String) -> Result<HashMap<String, String>, String> {
    if !language
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid language code".into());
    }
    let path = app.exe_dir.join("locales").join(format!("{language}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_config(app: State<App>) -> Result<ConfigState, String> {
    let loaded = store::load(&app.paths.config_file).map_err(|e| e.to_string())?;
    let issues = validate::validate(&loaded.config);
    Ok(ConfigState {
        config: loaded.config,
        issues,
        config_path: app.paths.config_file.display().to_string(),
        logs_dir: app.paths.logs_dir.display().to_string(),
        portable: app.paths.portable,
    })
}

/// Validates and saves; refuses to save while there are errors. The agent picks up
/// the new file by itself.
#[tauri::command]
fn save_config(app: State<App>, config: Config) -> Result<SaveResult, String> {
    let issues = validate::validate(&config);
    if issues_have_errors(&issues) {
        return Ok(SaveResult {
            saved: false,
            issues,
        });
    }
    store::save(&app.paths.config_file, &config).map_err(|e| e.to_string())?;
    Ok(SaveResult {
        saved: true,
        issues,
    })
}

#[tauri::command]
fn import_config(app: State<App>, text: String) -> Result<SaveResult, String> {
    let config = store::parse(&text).map_err(|e| e.to_string())?;
    save_config(app, config)
}

/// Writes a copy next to the config and shows it in Explorer.
#[tauri::command]
fn export_config(app: State<App>) -> Result<String, String> {
    let loaded = store::load(&app.paths.config_file).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let dir = app
        .paths
        .config_file
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let path = dir.join(format!("config.export-{stamp}.json"));
    store::write_json(&path, &loaded.config).map_err(|e| e.to_string())?;
    let _ = Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    Ok(path.display().to_string())
}

#[tauri::command]
fn open_logs(app: State<App>) -> Result<(), String> {
    std::fs::create_dir_all(&app.paths.logs_dir).map_err(|e| e.to_string())?;
    Command::new("explorer.exe")
        .arg(&app.paths.logs_dir)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn probe_hotkey(chord: String) -> HotkeyProbe {
    match Chord::parse(&chord) {
        Ok(c) => {
            let reserved = c.reserved_by_windows().map(str::to_string);
            HotkeyProbe {
                valid: true,
                canonical: Some(c.to_string()),
                error: None,
                free: Some(
                    reserved.is_none() && dg_win::system::hotkey_is_free(c.modifiers.bits(), c.vk),
                ),
                reserved,
                known: c
                    .known_conflicts()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                agent_running: dg_win::agent::is_running(),
            }
        }
        Err(e) => HotkeyProbe {
            valid: false,
            canonical: None,
            error: Some(e.to_string()),
            free: None,
            reserved: None,
            known: Vec::new(),
            agent_running: dg_win::agent::is_running(),
        },
    }
}

#[tauri::command]
fn integration_state() -> Integration {
    Integration {
        startup: dg_win::integration::startup_enabled(),
        protocol: dg_win::integration::protocol_command().is_some(),
    }
}

/// Connected and active displays, mapped to config monitor keys, plus the group on
/// screen now.
#[tauri::command]
async fn display_status(config: Config) -> Result<(Status, Option<Uuid>), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let backend = WinBackend::new();
        let engine = Engine::new(&backend);
        let status = engine.status(&config).map_err(|e| e.to_string())?;
        let ids: Vec<Uuid> = config.groups.iter().map(|g| g.id).collect();
        let current = engine.current_group(&config, &ids).ok().flatten();
        Ok((status, current))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Modes offered by a monitor that is currently on (Windows only lists modes of
/// active displays).
#[tauri::command]
async fn monitor_modes(config: Config, monitor: String) -> Result<Vec<ModeOption>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let entry = config
            .monitors
            .get(&monitor)
            .ok_or("unknown monitor")?
            .clone();
        let backend = WinBackend::new();
        let active = backend.query(Scope::Active).map_err(|e| e.to_string())?;
        let observed: Vec<_> = active
            .targets
            .iter()
            .map(dg_engine::observe::observe)
            .collect();
        let m = match_monitor(&entry.identity, &observed)
            .map_err(|_| "turn this display on to list its modes".to_string())?;
        let target = active.targets[m.index].target;
        let mut modes: Vec<ModeOption> = backend
            .list_modes(target)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|d| d.refresh_hz > 1 && d.width >= 640)
            .map(|d| ModeOption {
                width: d.width.max(d.height),
                height: d.width.min(d.height),
                refresh_hz: d.refresh_hz,
            })
            .collect();
        modes.sort_by(|a, b| b.cmp(a));
        modes.dedup();
        Ok(modes)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_cli(app: State<'_, App>, command: CliCommand) -> Result<CliResult, String> {
    let mut args: Vec<String> = Vec::new();
    match command {
        CliCommand::Capture { name } => {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > 128 {
                return Err("the group name must be 1–128 characters".into());
            }
            args.extend(["capture".into(), name]);
        }
        CliCommand::Validate { id } => {
            args.extend(["validate".into(), id.to_string(), "--json".into()]);
        }
        CliCommand::Shortcut {
            id,
            desktop,
            start_menu,
        } => {
            args.extend(["shortcut".into(), id.to_string()]);
            if desktop {
                args.push("--desktop".into());
            }
            if start_menu {
                args.push("--start-menu".into());
            }
        }
        CliCommand::Startup { on } => args.extend(["startup".into(), toggle(on)]),
        CliCommand::Protocol { on } => args.extend(["protocol".into(), toggle(on)]),
    }
    let exe = app.exe_dir.join("displaygroups.exe");
    let config = app.paths.config_file.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let output = Command::new(&exe)
            .args(&args)
            .arg("--config")
            .arg(&config)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(err.trim());
        }
        Ok(CliResult {
            ok: output.status.success(),
            code: output.status.code().unwrap_or(-1),
            output: text.trim().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn toggle(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

/// "Test": runs the real switch with its countdown through `displaygroupsw.exe`.
#[tauri::command]
fn apply_group(app: State<App>, id: Uuid) -> Result<(), String> {
    let exe = app.exe_dir.join("displaygroupsw.exe");
    Command::new(&exe)
        .args(["apply", &id.to_string()])
        .spawn()
        .map(drop)
        .map_err(|e| format!("{}: {e}", exe.display()))
}

/// Built-in icon names and their glyphs, so the UI shows the same symbols as the OSD.
#[tauri::command]
fn builtin_icons() -> BTreeMap<String, String> {
    dg_win::icons::BUILTIN
        .iter()
        .map(|name| {
            let style = dg_win::icons::builtin_style(name);
            (
                (*name).to_string(),
                format!(
                    "{}|#{:02x}{:02x}{:02x}",
                    style.glyph, style.color.0, style.color.1, style.color.2
                ),
            )
        })
        .collect()
}

/// Hotkeys, the tray and the OSD live in the agent. Opening the settings app (for
/// example from the installer's last page) starts it if it is not running, so the
/// hotkeys being edited are live. Debug builds never start it, and neither does
/// `DISPLAYGROUPS_NO_AGENT=1`, so working on the UI does not arm real hotkeys.
fn start_agent(exe_dir: &std::path::Path) {
    if cfg!(debug_assertions) || std::env::var_os("DISPLAYGROUPS_NO_AGENT").is_some() {
        return;
    }
    let agent = exe_dir.join("displaygroupsw.exe");
    if dg_win::agent::is_running() || !agent.is_file() {
        return;
    }
    let _ = Command::new(agent)
        .arg("agent")
        .current_dir(exe_dir)
        .spawn();
}

fn main() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    let paths = match AppPaths::resolve(Some(&exe_dir)) {
        Ok(p) => p,
        Err(e) => {
            dg_win::ui::show_error("DisplayGroups", &e.to_string());
            return;
        }
    };
    start_agent(&exe_dir);
    tauri::Builder::default()
        .manage(App { paths, exe_dir })
        .setup(|app| {
            if dg_win::system::windows_build() >= 22000 {
                if let Some(window) = app.get_webview_window("main") {
                    let effects = tauri::window::EffectsBuilder::new()
                        .effect(tauri::window::Effect::Mica)
                        .build();
                    let _ = window.set_effects(effects);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            platform,
            locale,
            load_config,
            save_config,
            import_config,
            export_config,
            open_logs,
            probe_hotkey,
            integration_state,
            display_status,
            monitor_modes,
            run_cli,
            apply_group,
            builtin_icons,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| dg_win::ui::show_error("DisplayGroups", &e.to_string()));
}

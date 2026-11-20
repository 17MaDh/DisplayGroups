//! DisplayGroups settings app (ARCHITECTURE.md §9, §16.6). Runs only while its window is
//! open.
//!
//! Config editing and validation run in-process and are the same on every system. The
//! display status, modes, hotkey checks, change watching, dialogs and "Identify" are per
//! system: `win.rs` (Windows) and `linux.rs` (GNOME). Actions with side effects that the
//! CLI already implements (capture, validate, shortcuts, startup, protocol, test-apply)
//! run the `displaygroups` executable next to this one, so there is a single
//! implementation.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command;

use dg_engine::Status;
use dg_hotkey::Chord;
use dg_model::paths::AppPaths;
use dg_model::validate::{self, Issue};
use dg_model::{store, Config};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

#[cfg(windows)]
#[path = "win.rs"]
mod sys;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod sys;

pub(crate) struct App {
    pub paths: AppPaths,
    pub exe_dir: PathBuf,
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

/// What the system says about a valid chord (see `probe` in `win.rs` / `linux.rs`).
#[derive(Default)]
pub(crate) struct Probe {
    /// No other program or shortcut uses it.
    pub free: bool,
    /// Windows keeps it for itself.
    pub reserved: Option<String>,
    /// Who else uses it, or usually does.
    pub known: Vec<String>,
    /// DisplayGroups itself holds it now (`None`: the page works it out).
    pub ours: Option<bool>,
    /// The chord cannot be used on this system.
    pub error: Option<String>,
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
    ours: Option<bool>,
    /// Whether the DisplayGroups agent is running (it holds or syncs the saved chords).
    agent_running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Platform {
    /// `"windows"` or `"linux"`; the page hides what the system does not support.
    os: &'static str,
    mica: bool,
    /// Windows build number (0 elsewhere).
    build: u32,
    /// The system's name for Diagnostics, e.g. "Ubuntu 24.04.1 LTS · GNOME".
    system: String,
    version: String,
    /// English first, then every `locales/<code>.json`, with its own name for itself.
    languages: Vec<Language>,
    /// What the `"system"` setting resolves to (the system's display language).
    system_language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Language {
    code: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Integration {
    pub startup: bool,
    pub protocol: bool,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModeOption {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// The result of "Test" / "Apply now".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyResult {
    /// The CLI's exit code: 0 kept, 3 reverted, others failed.
    code: i32,
    /// What it printed (Windows' windowless CLI shows its own dialogs instead).
    output: String,
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

/// `locales` next to the executable. Debug builds fall back to the repository's
/// folder, so translations can be tried without copying files.
fn locales_dir(app: &App) -> PathBuf {
    let beside = app.exe_dir.join("locales");
    if beside.is_dir() || !cfg!(debug_assertions) {
        beside
    } else {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../locales"))
    }
}

fn valid_language_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 16
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn read_locale(app: &App, code: &str) -> Result<HashMap<String, String>, String> {
    if !valid_language_code(code) {
        return Err("invalid language code".into());
    }
    let path = locales_dir(app).join(format!("{code}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
fn platform(app: State<App>) -> Platform {
    let mut languages = vec![Language {
        code: "en".into(),
        name: "English".into(),
    }];
    if let Ok(dir) = std::fs::read_dir(locales_dir(&app)) {
        let mut found: Vec<Language> = dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .filter_map(|path| {
                let code = path.file_stem()?.to_string_lossy().into_owned();
                if code == "en" || !valid_language_code(&code) {
                    return None;
                }
                let name = read_locale(&app, &code)
                    .ok()
                    .and_then(|m| m.get("language.name").cloned())
                    .unwrap_or_else(|| code.clone());
                Some(Language { code, name })
            })
            .collect();
        found.sort_by(|a, b| a.code.cmp(&b.code));
        languages.extend(found);
    }
    Platform {
        os: sys::OS,
        mica: sys::mica(),
        build: sys::build(),
        system: sys::description(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        languages,
        system_language: sys::ui_language(),
    }
}

/// Strings for a language other than English (English is bundled with the UI).
#[tauri::command]
fn locale(app: State<App>, language: String) -> Result<HashMap<String, String>, String> {
    read_locale(&app, &language)
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
    if validate::has_errors(&issues) {
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

/// Writes a copy next to the config and shows it in the file manager.
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
    sys::reveal(&path);
    Ok(path.display().to_string())
}

#[tauri::command]
fn open_logs(app: State<App>) -> Result<(), String> {
    std::fs::create_dir_all(&app.paths.logs_dir).map_err(|e| e.to_string())?;
    sys::open_folder(&app.paths.logs_dir)
}

#[tauri::command]
async fn probe_hotkey(app: State<'_, App>, chord: String) -> Result<HotkeyProbe, String> {
    let paths = app.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let agent_running = sys::agent_running(&paths);
        let chord = match Chord::parse(&chord) {
            Ok(c) => c,
            Err(e) => {
                return HotkeyProbe {
                    valid: false,
                    canonical: None,
                    error: Some(e.to_string()),
                    free: None,
                    reserved: None,
                    known: Vec::new(),
                    ours: None,
                    agent_running,
                }
            }
        };
        let probe = sys::probe(&chord);
        HotkeyProbe {
            valid: probe.error.is_none(),
            canonical: Some(chord.to_string()),
            free: probe.error.is_none().then_some(probe.free),
            error: probe.error,
            reserved: probe.reserved,
            known: probe.known,
            ours: probe.ours,
            agent_running,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn integration_state() -> Integration {
    sys::integration()
}

/// Connected and active displays, mapped to config monitor keys, plus the group on
/// screen now.
#[tauri::command]
async fn display_status(config: Config) -> Result<(Status, Option<Uuid>), String> {
    tauri::async_runtime::spawn_blocking(move || sys::display_status(&config))
        .await
        .map_err(|e| e.to_string())?
}

/// Modes a monitor offers. Windows lists them only for displays that are on; GNOME
/// for every connected one. `"off"` (translated by the page) when it cannot be listed.
#[tauri::command]
async fn monitor_modes(config: Config, monitor: String) -> Result<Vec<ModeOption>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut modes = sys::monitor_modes(&config, &monitor)?;
        modes.retain(|m| m.refresh_hz > 1 && m.width >= 640);
        modes.sort_by(|a, b| b.cmp(a));
        modes.dedup();
        Ok(modes)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn cli_output(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(err.trim());
    }
    text.trim().to_string()
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
    let exe = app.exe_dir.join(sys::CLI);
    let config = app.paths.config_file.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = Command::new(&exe);
        command.args(&args).arg("--config").arg(&config);
        sys::no_console(&mut command);
        let output = command
            .output()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        Ok(CliResult {
            ok: output.status.success(),
            code: output.status.code().unwrap_or(-1),
            output: cli_output(&output),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn toggle(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

/// "Test" / "Apply now": runs the real switch through the CLI and returns once it is
/// kept or reverted (Windows' countdown) or applied (GNOME asks by itself afterwards),
/// so the page can say what happened.
#[tauri::command]
async fn apply_group(app: State<'_, App>, id: Uuid) -> Result<ApplyResult, String> {
    let exe = app.exe_dir.join(sys::CLI_SWITCH);
    tauri::async_runtime::spawn_blocking(move || {
        let output = Command::new(&exe)
            .args(["apply", &id.to_string()])
            .output()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        Ok(ApplyResult {
            code: output.status.code().unwrap_or(-1),
            output: cli_output(&output),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Identify": each display shows its number and name for a few seconds.
#[tauri::command]
async fn identify(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || sys::identify(&app, 3))
        .await
        .map_err(|e| e.to_string())?
}

/// Programs to pick from for a trigger (lower-case names, like `steam.exe`).
#[tauri::command]
async fn running_programs() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(sys::running_programs)
        .await
        .map_err(|e| e.to_string())
}

/// The media type of a custom icon file, from its extension.
fn image_type(file: &str) -> &'static str {
    let ext = file.rsplit('.').next().unwrap_or_default();
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "image/x-icon",
    }
}

/// A custom group icon (`file:…`) as a `data:` URL; built-in icons are glyphs.
#[tauri::command]
fn icon_image(app: State<App>, icon: String) -> Result<String, String> {
    let name = icon.strip_prefix("file:").ok_or("not a custom icon")?;
    let file = std::path::Path::new(name)
        .file_name()
        .ok_or("invalid icon name")?;
    let path = app.paths.icons_dir.join(file);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!(
        "data:{};base64,{}",
        image_type(&file.to_string_lossy()),
        base64(&bytes)
    ))
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Built-in icon names and their glyphs (`glyph|#rrggbb`), so the UI shows the same
/// symbols as the OSD.
#[tauri::command]
fn builtin_icons() -> BTreeMap<String, String> {
    dg_model::icons::builtin_table().into_iter().collect()
}

/// `--identify [--seconds N]`: only show the display labels, then quit (what
/// `displaygroups identify` runs on Linux).
fn identify_only(args: &[String]) -> Option<u32> {
    args.iter().any(|a| a == "--identify").then(|| {
        args.iter()
            .position(|a| a == "--seconds")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(3)
            .clamp(1, 30)
    })
}

fn main() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    let paths = match AppPaths::resolve(Some(&exe_dir)) {
        Ok(p) => p,
        Err(e) => {
            sys::show_error("DisplayGroups", &e.to_string());
            return;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let labels_only = identify_only(&args);
    if labels_only.is_none() {
        sys::start_agent(&exe_dir, &paths);
    }
    tauri::Builder::default()
        .manage(App { paths, exe_dir })
        .setup(move |app| {
            if let Some(seconds) = labels_only {
                sys::identify(app.handle(), seconds)?;
                return Ok(());
            }
            // The main window is described in tauri.conf.json but created here, so
            // `--identify` can run without it.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("no main window in the configuration")?;
            let window =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
            sys::decorate(&window);
            let config_file = app.state::<App>().paths.config_file.clone();
            sys::watch_changes(app.handle().clone(), config_file);
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
            identify,
            builtin_icons,
            sys::pick_icon,
            icon_image,
            running_programs,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| sys::show_error("DisplayGroups", &e.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
        assert_eq!(base64(&[0xFF, 0xEF]), "/+8=");
    }

    #[test]
    fn language_codes_are_checked() {
        assert!(valid_language_code("ar"));
        assert!(valid_language_code("pt-BR"));
        assert!(!valid_language_code(""));
        assert!(!valid_language_code("..\\x"));
        assert!(!valid_language_code("ar/../../secret"));
    }

    #[test]
    fn custom_icons_get_their_media_type() {
        assert_eq!(image_type("custom-tv.PNG"), "image/png");
        assert_eq!(image_type("custom-tv.svg"), "image/svg+xml");
        assert_eq!(image_type("couch-1a2b.ico"), "image/x-icon");
    }

    #[test]
    fn identify_mode_reads_its_seconds() {
        let args = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(identify_only(&args(&[])), None);
        assert_eq!(identify_only(&args(&["--identify"])), Some(3));
        assert_eq!(
            identify_only(&args(&["--identify", "--seconds", "7"])),
            Some(7)
        );
        assert_eq!(
            identify_only(&args(&["--identify", "--seconds", "99"])),
            Some(30)
        );
    }
}

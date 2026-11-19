//! The resident agent, `displaygroupsw.exe agent` (ARCHITECTURE.md §7, §10, §11.3):
//! global hotkeys, the OSD, live config reload and the disconnect fallback.
//!
//! The UI thread only handles window messages. Every switch runs on a worker thread
//! that reuses the CLI's `apply`/`toggle`/`recover`, so the OSD keeps animating while
//! Windows reconfigures the displays and the "Keep these settings?" box is open.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dg_engine::ccd::{Scope, TargetRef};
use dg_engine::{next_in_cycle, DisplayBackend, Engine};
use dg_hotkey::Chord;
use dg_model::{store, Config, Group, HotkeyAction, Theme};
use dg_win::integration;
use dg_win::osd::{Osd, OsdContent};
use dg_win::system::{
    system_prefers_dark, trim_working_set, watch_dir, working_set_bytes, SingleInstance,
};
use dg_win::tray::{classify, show_menu, MenuItem, Tray, TrayClick};
use dg_win::window::{AgentEvent, AgentWindow};
use dg_win::WinBackend;
use uuid::Uuid;

use crate::commands::{self, hz, LastSwitch, Trigger};
use crate::i18n::t;
use crate::output::Output;
use crate::{CliError, Ctx, EXIT_OK};

const APP_CONFIG_CHANGED: u32 = 1;
const APP_JOB_DONE: u32 = 2;
const TIMER_RELOAD: usize = 1;
const TIMER_FALLBACK: usize = 2;
const TIMER_MEMORY: usize = 3;
/// Display events right after our own switch are its echo, not a disconnect.
const OWN_SWITCH_GRACE: Duration = Duration::from_secs(10);
const FALLBACK_COOLDOWN: Duration = Duration::from_secs(60);
const DRY_RUN_DETAIL: &str = "Dry run: displays were not changed";

#[derive(Debug, Clone)]
enum Job {
    Hotkey(HotkeyAction),
    /// A group picked from the tray menu.
    Group(Uuid),
    Fallback(Uuid),
}

// Tray menu command ids.
const CMD_RECOVER: u32 = 900;
const CMD_SETTINGS: u32 = 901;
const CMD_STARTUP: u32 = 902;
const CMD_MINIMIZED: u32 = 903;
const CMD_EXIT: u32 = 904;
const CMD_GROUP_BASE: u32 = 1000;

#[derive(Debug)]
enum JobOutcome {
    Switched,
    Preview {
        title: String,
        detail: String,
        icon: String,
    },
    Failed,
}

struct Agent {
    ctx: Ctx,
    cfg: Config,
    dry_run: bool,
    out: Output,
    bindings: Vec<(i32, HotkeyAction)>,
    busy: bool,
    locked: bool,
    last_switch: Option<Instant>,
    last_fallback: Option<Instant>,
    known_active: Vec<TargetRef>,
    done: Arc<Mutex<Option<JobOutcome>>>,
    tray: Option<Tray>,
}

pub(crate) fn run(ctx: &Ctx, cfg: Config, dry_run: bool, out: Output) -> Result<i32, CliError> {
    let Some(_instance) = SingleInstance::acquire(dg_win::agent::AGENT_MUTEX) else {
        out.line("The DisplayGroups agent is already running.");
        return Ok(EXIT_OK);
    };
    let window = AgentWindow::create().map_err(CliError::Config)?;
    let osd = Osd::create().map_err(CliError::Config)?;
    if let Some(dir) = ctx.config_path.parent() {
        let poster = window.poster();
        let watching = watch_dir(dir, move || {
            poster.post(APP_CONFIG_CHANGED, 0);
        });
        if let Err(e) = watching {
            tracing::warn!("config changes will not be picked up: {e}");
        }
    }

    let mut agent = Agent {
        ctx: ctx.clone(),
        cfg,
        dry_run,
        out,
        bindings: Vec::new(),
        busy: false,
        locked: false,
        last_switch: None,
        last_fallback: None,
        known_active: active_targets(),
        done: Arc::default(),
        tray: None,
    };
    agent.tray = match Tray::add(&window, &t("tray.tooltip")) {
        Ok(tray) => Some(tray),
        Err(e) => {
            tracing::warn!("no tray icon: {e}");
            None
        }
    };
    agent.register_hotkeys(&window);
    sync_lock_screen(&agent.cfg);
    if !agent.cfg.settings.start_minimized && !dry_run {
        open_settings();
    }
    window.set_timer(TIMER_MEMORY, 5000);
    tracing::info!(dry_run, "agent started");
    out.line(if dry_run {
        "Agent running (dry run: hotkeys only show the OSD). Close this window or press Ctrl+C to stop."
    } else {
        "Agent running. Close this window or press Ctrl+C to stop."
    });

    window.run(|event| agent.handle(event, &window, &osd));
    tracing::info!("agent stopped");
    Ok(EXIT_OK)
}

impl Agent {
    /// Returns false to stop the agent.
    fn handle(&mut self, event: AgentEvent, window: &AgentWindow, osd: &Osd) -> bool {
        match event {
            AgentEvent::Tray(code) => {
                tracing::debug!(code, "tray event");
                if matches!(classify(code), TrayClick::Menu | TrayClick::Primary) {
                    return self.tray_menu(window);
                }
            }
            AgentEvent::TaskbarCreated => {
                if let Some(tray) = &self.tray {
                    if let Err(e) = tray.register() {
                        tracing::warn!("tray icon not restored: {e}");
                    }
                }
            }
            AgentEvent::Hotkey(id) => {
                let Some(action) = self
                    .bindings
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, a)| a.clone())
                else {
                    return true;
                };
                if self.busy {
                    tracing::info!("hotkey ignored: a switch is already running");
                } else if self.locked {
                    tracing::info!("hotkey ignored: the session is locked");
                } else {
                    self.start(Job::Hotkey(action), window);
                }
            }
            AgentEvent::App {
                code: APP_CONFIG_CHANGED,
                ..
            } => window.set_timer(TIMER_RELOAD, 300),
            AgentEvent::Timer(TIMER_RELOAD) => {
                window.kill_timer(TIMER_RELOAD);
                self.reload(window);
            }
            AgentEvent::App {
                code: APP_JOB_DONE, ..
            } => self.finish(window, osd),
            AgentEvent::Switched => {
                self.last_switch = Some(Instant::now());
                self.show_last_switch(osd);
            }
            AgentEvent::DisplayChanged | AgentEvent::DeviceChanged => {
                let fallback = self.cfg.settings.disconnect_fallback;
                if fallback.enabled {
                    window.set_timer(TIMER_FALLBACK, fallback.debounce_ms.max(500));
                }
            }
            AgentEvent::Timer(TIMER_FALLBACK) => {
                window.kill_timer(TIMER_FALLBACK);
                self.check_fallback(window);
            }
            AgentEvent::Timer(TIMER_MEMORY) => {
                window.kill_timer(TIMER_MEMORY);
                let before = working_set_bytes().unwrap_or_default();
                trim_working_set();
                let after = working_set_bytes().unwrap_or_default();
                tracing::info!(
                    before_kb = before / 1024,
                    after_kb = after / 1024,
                    "agent idle: working set trimmed"
                );
            }
            AgentEvent::SessionLocked => self.locked = true,
            AgentEvent::SessionUnlocked => {
                self.locked = false;
                // Lock-screen switches are reported by the service (pending the §8.5 spike).
            }
            _ => {}
        }
        true
    }

    /// Shows the tray menu and runs the chosen command. Returns false for "Exit".
    fn tray_menu(&mut self, window: &AgentWindow) -> bool {
        let ids: Vec<Uuid> = self.cfg.groups.iter().map(|g| g.id).collect();
        let current = if ids.is_empty() {
            None
        } else {
            Engine::new(&WinBackend::new())
                .current_group(&self.cfg, &ids)
                .ok()
                .flatten()
        };
        let mut items = Vec::new();
        if self.cfg.groups.is_empty() {
            items.push(MenuItem::disabled(t("tray.noGroups")));
        }
        for (i, g) in self.cfg.groups.iter().enumerate() {
            let id = CMD_GROUP_BASE + u32::try_from(i).unwrap_or(0);
            items.push(MenuItem::checkbox(
                id,
                g.name.clone(),
                Some(g.id) == current,
            ));
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::command(CMD_RECOVER, t("tray.recover")));
        items.push(MenuItem::Separator);
        items.push(MenuItem::command(CMD_SETTINGS, t("tray.settings")));
        items.push(MenuItem::checkbox(
            CMD_STARTUP,
            t("tray.startWithWindows"),
            integration::startup_enabled(),
        ));
        items.push(MenuItem::checkbox(
            CMD_MINIMIZED,
            t("tray.startMinimized"),
            self.cfg.settings.start_minimized,
        ));
        items.push(MenuItem::Separator);
        items.push(MenuItem::command(CMD_EXIT, t("tray.exit")));

        let chosen = show_menu(window, &items, crate::i18n::is_rtl());
        tracing::debug!(?chosen, "tray menu closed");
        match chosen {
            Some(CMD_EXIT) => return false,
            Some(CMD_RECOVER) if !self.busy => {
                self.start(Job::Hotkey(HotkeyAction::Recover), window)
            }
            Some(CMD_SETTINGS) => open_settings(),
            Some(CMD_STARTUP) => {
                let result = if integration::startup_enabled() {
                    integration::set_startup(None)
                } else {
                    crate::integrate::windowless_exe()
                        .map_err(|e| e.to_string())
                        .and_then(|exe| integration::set_startup(Some(&exe)))
                };
                if let Err(e) = result {
                    self.out.error(e);
                }
            }
            Some(CMD_MINIMIZED) => {
                self.cfg.settings.start_minimized = !self.cfg.settings.start_minimized;
                if let Err(e) = store::save(&self.ctx.config_path, &self.cfg) {
                    self.out.error(e.to_string());
                }
            }
            Some(id) if id >= CMD_GROUP_BASE && !self.busy => {
                let index = (id - CMD_GROUP_BASE) as usize;
                if let Some(g) = self.cfg.groups.get(index) {
                    let gid = g.id;
                    self.start(Job::Group(gid), window);
                }
            }
            _ => {}
        }
        true
    }

    fn register_hotkeys(&mut self, window: &AgentWindow) {
        for (id, _) in self.bindings.drain(..) {
            window.unregister_hotkey(id);
        }
        let mut problems = Vec::new();
        for (index, hotkey) in self.cfg.hotkeys.iter().enumerate() {
            let chord = match Chord::parse(&hotkey.chord) {
                Ok(c) => c,
                Err(e) => {
                    problems.push(format!("\"{}\": {e}", hotkey.chord));
                    continue;
                }
            };
            if let Some(owner) = chord.reserved_by_windows() {
                problems.push(format!("{chord} is reserved by Windows ({owner})"));
                continue;
            }
            let id = i32::try_from(index + 1).unwrap_or(i32::MAX);
            if window.register_hotkey(id, chord.modifiers.bits(), chord.vk) {
                tracing::info!(%chord, "hotkey registered");
                self.bindings.push((id, hotkey.action.clone()));
            } else {
                let owners = chord.known_conflicts();
                let hint = if owners.is_empty() {
                    String::new()
                } else {
                    format!(" (probably {})", owners.join(" or "))
                };
                problems.push(format!("{chord} is already used by another program{hint}"));
            }
        }
        if !problems.is_empty() {
            let text = format!(
                "{}\n\n{}\n\n{}",
                t("hotkeys.notActive"),
                problems.join("\n"),
                t("hotkeys.chooseOther")
            );
            tracing::warn!("{text}");
            if self.out.console {
                eprintln!("warning: {text}");
            } else {
                std::thread::spawn(move || dg_win::ui::show_error("DisplayGroups", &text));
            }
        }
    }

    fn reload(&mut self, window: &AgentWindow) {
        match store::load(&self.ctx.config_path) {
            Ok(loaded) => {
                if loaded.config.settings.language != self.cfg.settings.language {
                    crate::i18n::init(&loaded.config.settings.language);
                    self.out.rtl = crate::i18n::is_rtl();
                }
                self.cfg = loaded.config;
                self.register_hotkeys(window);
                sync_lock_screen(&self.cfg);
                tracing::info!("config reloaded");
            }
            Err(e) => tracing::warn!("config not reloaded: {e}"),
        }
    }

    fn start(&mut self, job: Job, window: &AgentWindow) {
        self.busy = true;
        let ctx = self.ctx.clone();
        let cfg = self.cfg.clone();
        let (out, dry_run) = (self.out, self.dry_run);
        let done = Arc::clone(&self.done);
        let poster = window.poster();
        std::thread::spawn(move || {
            let outcome = run_job(&ctx, &cfg, &job, dry_run, out);
            *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
            poster.post(APP_JOB_DONE, 0);
        });
    }

    fn finish(&mut self, window: &AgentWindow, osd: &Osd) {
        self.busy = false;
        window.drop_pending_hotkeys();
        let outcome = self.done.lock().unwrap_or_else(|e| e.into_inner()).take();
        match outcome {
            Some(JobOutcome::Preview {
                title,
                detail,
                icon,
            }) => self.show_osd(osd, &title, &detail, &icon),
            Some(JobOutcome::Switched) => self.last_switch = Some(Instant::now()),
            Some(JobOutcome::Failed) | None => {}
        }
        self.known_active = active_targets();
        // Trim once the OSD has faded out and the agent is idle again.
        window.set_timer(TIMER_MEMORY, self.cfg.settings.osd.duration_ms + 3000);
    }

    fn check_fallback(&mut self, window: &AgentWindow) {
        let now_active = active_targets();
        let previous = std::mem::replace(&mut self.known_active, now_active.clone());
        let settings = self.cfg.settings.disconnect_fallback;
        if !settings.enabled || self.busy || self.locked {
            return;
        }
        let recent = |t: Option<Instant>, window: Duration| t.is_some_and(|t| t.elapsed() < window);
        if recent(self.last_switch, OWN_SWITCH_GRACE)
            || recent(self.last_fallback, FALLBACK_COOLDOWN)
        {
            return;
        }
        if !fallback_needed(&previous, &now_active) {
            return;
        }
        let Some(group) = settings.group.and_then(|id| self.cfg.group_by_id(id)) else {
            return;
        };
        // Never fall back to a group whose monitors are not all connected.
        let display = WinBackend::new();
        let ready = display
            .query(Scope::All)
            .is_ok_and(|all| dg_engine::resolve::resolve_group(&self.cfg, group, &all).is_ok());
        if !ready {
            tracing::info!(group = %group.name, "displays disconnected, but the fallback group is not available");
            return;
        }
        tracing::info!(group = %group.name, "displays disconnected; switching to the fallback group");
        self.last_fallback = Some(Instant::now());
        let id = group.id;
        self.start(Job::Fallback(id), window);
    }

    fn show_last_switch(&self, osd: &Osd) {
        let Ok(Some(last)) = store::read_json::<LastSwitch>(&self.ctx.paths.last_switch_file())
        else {
            return;
        };
        let detail = last
            .primary
            .map(|p| {
                let rate = p
                    .refresh_milli_hz
                    .map(|m| format!(" · {} Hz", hz(m)))
                    .unwrap_or_default();
                format!("{} × {}{rate}", p.width, p.height)
            })
            .unwrap_or_default();
        self.show_osd(osd, &last.group_name, &detail, &last.icon);
    }

    fn show_osd(&self, osd: &Osd, title: &str, detail: &str, icon: &str) {
        let settings = &self.cfg.settings;
        if !settings.osd.enabled {
            return;
        }
        let dark = match settings.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => system_prefers_dark(),
        };
        osd.show(
            OsdContent {
                glyph: glyph_for(icon),
                title: title.to_string(),
                detail: detail.to_string(),
                dark,
                rtl: crate::i18n::is_rtl(),
            },
            settings.osd.duration_ms,
        );
    }
}

fn run_job(ctx: &Ctx, cfg: &Config, job: &Job, dry_run: bool, out: Output) -> JobOutcome {
    let result = if dry_run {
        preview(cfg, job)
    } else {
        execute(ctx, cfg, job, out)
    };
    result.unwrap_or_else(|e| {
        out.error(e.to_string());
        JobOutcome::Failed
    })
}

fn execute(ctx: &Ctx, cfg: &Config, job: &Job, out: Output) -> Result<JobOutcome, CliError> {
    match job {
        Job::Hotkey(HotkeyAction::Apply { group }) => {
            commands::apply(
                ctx,
                cfg,
                &group_by_id(cfg, *group)?,
                false,
                Trigger::Hotkey,
                out,
            )?;
        }
        Job::Hotkey(HotkeyAction::Toggle { groups } | HotkeyAction::Cycle { groups }) => {
            let keys: Vec<String> = groups.iter().map(Uuid::to_string).collect();
            commands::toggle(ctx, cfg, &keys, false, Trigger::Hotkey, out)?;
        }
        Job::Hotkey(HotkeyAction::Recover) => {
            commands::recover(ctx, false, out)?;
        }
        Job::Group(id) => {
            commands::apply(ctx, cfg, &group_by_id(cfg, *id)?, false, Trigger::Tray, out)?;
        }
        Job::Fallback(id) => {
            commands::apply(
                ctx,
                cfg,
                &group_by_id(cfg, *id)?,
                false,
                Trigger::Fallback,
                out,
            )?;
        }
    }
    Ok(JobOutcome::Switched)
}

/// Starts the settings app if it is installed next to the agent.
fn open_settings() {
    let Some(exe) = std::env::current_exe()
        .ok()
        .map(|p| p.with_file_name("displaygroups-settings.exe"))
    else {
        return;
    };
    if !exe.is_file() {
        std::thread::spawn(|| dg_win::ui::show_info("DisplayGroups", &t("tray.settingsMissing")));
        return;
    }
    if let Err(e) = std::process::Command::new(&exe).spawn() {
        tracing::warn!("cannot start {}: {e}", exe.display());
    }
}

/// What a hotkey *would* do, for `agent --dry-run`.
fn preview(cfg: &Config, job: &Job) -> Result<JobOutcome, CliError> {
    let target = match job {
        Job::Hotkey(HotkeyAction::Apply { group }) | Job::Group(group) | Job::Fallback(group) => {
            Some(*group)
        }
        Job::Hotkey(HotkeyAction::Toggle { groups } | HotkeyAction::Cycle { groups }) => {
            let display = WinBackend::new();
            let current = Engine::new(&display).current_group(cfg, groups)?;
            next_in_cycle(groups, current)
        }
        Job::Hotkey(HotkeyAction::Recover) => None,
    };
    Ok(match target.and_then(|id| cfg.group_by_id(id)) {
        Some(g) => JobOutcome::Preview {
            title: g.name.clone(),
            detail: DRY_RUN_DETAIL.into(),
            icon: g.icon.clone(),
        },
        None => JobOutcome::Preview {
            title: "Recover".into(),
            detail: DRY_RUN_DETAIL.into(),
            icon: "builtin:monitor".into(),
        },
    })
}

fn group_by_id(cfg: &Config, id: Uuid) -> Result<Group, CliError> {
    cfg.group_by_id(id).cloned().ok_or_else(|| {
        CliError::Config(format!(
            "the hotkey points at a group that no longer exists ({id})"
        ))
    })
}

/// Sends the lock-screen hotkeys and their groups' geometry to the service
/// (ARCHITECTURE.md §8.2), on a background thread. Without the service this only logs.
fn sync_lock_screen(cfg: &Config) {
    use dg_ipc::pipe::PipeConnection;
    use dg_ipc::{read_message, write_message, Request, Response, PROTOCOL_VERSION, SERVICE_PIPE};

    if !cfg.settings.lock_screen_support {
        return;
    }
    let (profiles, hotkeys) = dg_ipc::lock_payload(cfg);
    if let Err(e) = dg_ipc::validate_sync(&profiles, &hotkeys) {
        tracing::warn!("lock-screen hotkeys not synced: {e}");
        return;
    }
    std::thread::spawn(move || {
        let mut pipe = match PipeConnection::connect(SERVICE_PIPE) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!("lock-screen service is not available ({e})");
                return;
            }
        };
        let exchange = || -> Result<Response, dg_ipc::IpcError> {
            write_message(
                &mut pipe,
                &Request::Hello {
                    version: PROTOCOL_VERSION,
                },
            )?;
            match read_message(&mut pipe)? {
                Response::Hello { version } if version == PROTOCOL_VERSION => {}
                Response::Hello { version } => {
                    return Err(dg_ipc::IpcError::Version {
                        theirs: version,
                        ours: PROTOCOL_VERSION,
                    })
                }
                other => return Ok(other),
            }
            let count = hotkeys.len();
            write_message(&mut pipe, &Request::SyncLockProfiles { profiles, hotkeys })?;
            let reply = read_message(&mut pipe)?;
            tracing::info!(hotkeys = count, ?reply, "lock-screen hotkeys synced");
            Ok(reply)
        };
        if let Err(e) = exchange() {
            tracing::warn!("lock-screen sync failed: {e}");
        }
    });
}

fn active_targets() -> Vec<TargetRef> {
    WinBackend::new()
        .query(Scope::Active)
        .map(|t| t.active_targets())
        .unwrap_or_default()
}

/// True when every display that was on is gone (ARCHITECTURE.md §11.3).
fn fallback_needed(previous: &[TargetRef], now: &[TargetRef]) -> bool {
    !previous.is_empty() && previous.iter().all(|t| !now.contains(t))
}

/// Built-in icons map to Segoe Fluent Icons / MDL2 Assets glyphs (present on
/// Windows 10 and 11), so the OSD needs no image files.
fn glyph_for(icon: &str) -> Option<char> {
    match icon.strip_prefix("builtin:") {
        Some("couch" | "gaming") => Some('\u{E7FC}'), // Game
        Some("work") => Some('\u{E821}'),             // Work
        _ => Some('\u{E7F4}'),                        // TVMonitor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dg_engine::ccd::Luid;

    fn t(id: u32) -> TargetRef {
        TargetRef {
            adapter: Luid { low: 1, high: 0 },
            id,
        }
    }

    #[test]
    fn fallback_only_when_every_previous_display_is_gone() {
        // TV unplugged; Windows brought the desk monitors back on its own.
        assert!(fallback_needed(&[t(1)], &[t(2), t(3)]));
        assert!(fallback_needed(&[t(1)], &[]));
        // One of two desk monitors unplugged: the other is still on, leave it alone.
        assert!(!fallback_needed(&[t(2), t(3)], &[t(2)]));
        // Nothing was known before (agent just started).
        assert!(!fallback_needed(&[], &[t(2)]));
    }

    #[test]
    fn icons_map_to_glyphs() {
        assert_eq!(glyph_for("builtin:couch"), Some('\u{E7FC}'));
        assert_eq!(glyph_for("builtin:monitor"), Some('\u{E7F4}'));
        assert_eq!(glyph_for("file:custom.ico"), Some('\u{E7F4}'));
    }
}

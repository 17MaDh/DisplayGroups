//! The resident agent on Linux, `displaygroups agent` (ARCHITECTURE.md §16.3, L2): the
//! tray icon, hotkeys as GNOME custom shortcuts, live config reload, the disconnect
//! fallback and automatic switching.
//!
//! One loop handles events from a one-second ticker, Mutter's `MonitorsChanged` signal,
//! the tray menu and finished switches. Switches run on a worker thread through the
//! CLI's own `apply`/`toggle`/`recover`, so the tray stays responsive; GNOME Shell asks
//! "Keep these display settings?" itself.
//!
//! Unlike Windows, hotkeys do not go through the agent: GNOME runs `displaygroups …` for
//! them. The agent only keeps GNOME's custom shortcuts in step with the config.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant, SystemTime};

use dg_engine::triggers::{Decision, Needs, Observation, Triggers};
use dg_gnome::dbus::Mutter;
use dg_gnome::desktop::{self, Dirs};
use dg_gnome::detect;
use dg_gnome::keybindings::{self, Shortcut};
use dg_gnome::tray::{TrayCommand, TrayHandle, TrayLabels, TrayModel};
use dg_model::{store, Config, Group};
use uuid::Uuid;

use crate::commands::{self, Trigger};
use crate::i18n::t;
use crate::output::Output;
use crate::{CliError, Ctx, EXIT_OK};

const TICK: Duration = Duration::from_secs(1);
/// Programs, controllers and the config file are looked at every other tick.
const POLL_TICKS: u64 = 2;
/// Display events right after our own switch are its echo, not a disconnect.
const OWN_SWITCH_GRACE: Duration = Duration::from_secs(10);
const FALLBACK_COOLDOWN: Duration = Duration::from_secs(60);

enum Event {
    Tick,
    DisplaysChanged,
    Tray(TrayCommand),
    JobDone,
}

#[derive(Debug, Clone)]
enum Job {
    Apply {
        group: Uuid,
        trigger: Trigger,
        confirm: bool,
    },
    Recover,
}

struct Agent {
    ctx: Ctx,
    cfg: Config,
    dry_run: bool,
    out: Output,
    tx: Sender<Event>,
    tray: Option<TrayHandle>,
    busy: bool,
    ticks: u64,
    config_stamp: Option<SystemTime>,
    /// The GNOME shortcuts last written, so an unchanged config writes nothing.
    synced: Option<Vec<Shortcut>>,
    started: Instant,
    triggers: Triggers,
    needs: Needs,
    controller: bool,
    /// In a dry run, the group a trigger would have switched to.
    dry_current: Option<Uuid>,
    known_active: Vec<String>,
    fallback_due: Option<Instant>,
    last_switch: Option<Instant>,
    last_fallback: Option<Instant>,
    warned_titles: bool,
}

pub(crate) fn run(ctx: &Ctx, cfg: Config, dry_run: bool, out: Output) -> Result<i32, CliError> {
    // One agent per user: a lock on `state/agent.lock`, held until the process ends.
    let Some(_instance) =
        dg_gnome::system::take_agent_lock(&ctx.paths.state_dir).map_err(CliError::Config)?
    else {
        out.line("The DisplayGroups agent is already running.");
        return Ok(EXIT_OK);
    };
    let (tx, rx) = mpsc::channel();
    {
        let tx = tx.clone();
        std::thread::Builder::new()
            .name("ticker".into())
            .spawn(move || {
                while tx.send(Event::Tick).is_ok() {
                    std::thread::sleep(TICK);
                }
            })
            .map_err(|e| CliError::Config(e.to_string()))?;
    }
    {
        let tx = tx.clone();
        if let Err(e) = dg_gnome::dbus::watch_changes(move || {
            let _ = tx.send(Event::DisplaysChanged);
        }) {
            tracing::warn!("display changes will not be noticed: {e}");
        }
    }

    let mut agent = Agent {
        ctx: ctx.clone(),
        config_stamp: modified(&ctx.config_path),
        cfg,
        dry_run,
        out,
        tx: tx.clone(),
        tray: None,
        busy: false,
        ticks: 0,
        synced: None,
        started: Instant::now(),
        triggers: Triggers::new(),
        needs: Needs::default(),
        controller: false,
        dry_current: None,
        known_active: active_connectors(),
        fallback_due: None,
        last_switch: None,
        last_fallback: None,
        warned_titles: false,
    };
    agent.tray = match dg_gnome::tray::spawn(agent.tray_model(), move |command| {
        let _ = tx.send(Event::Tray(command));
    }) {
        Ok(tray) => Some(tray),
        Err(e) => {
            tracing::warn!("no tray icon: {e}");
            None
        }
    };
    agent.sync_hotkeys();
    agent.arm_triggers();
    tracing::info!(dry_run, "agent started");
    out.line(if dry_run {
        "Agent running (dry run: the tray and triggers only show a notification). Press Ctrl+C to stop."
    } else {
        "Agent running. Press Ctrl+C to stop."
    });

    for event in rx {
        if !agent.handle(event) {
            break;
        }
    }
    tracing::info!("agent stopped");
    Ok(EXIT_OK)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn active_connectors() -> Vec<String> {
    Mutter::connect()
        .and_then(|m| m.state())
        .map(|s| {
            s.logical_monitors
                .into_iter()
                .flat_map(|l| l.connectors)
                .collect()
        })
        .unwrap_or_default()
}

/// True when every display that was on is gone (ARCHITECTURE.md §11.3).
fn fallback_needed(previous: &[String], now: &[String]) -> bool {
    !previous.is_empty() && previous.iter().all(|c| !now.contains(c))
}

/// Errors from the agent go to the log and, since nobody sees its output, to a
/// notification.
fn report(text: &str) {
    tracing::error!("{text}");
    let _ = dg_gnome::notify::notify("DisplayGroups", text, "dialog-warning", 0);
}

impl Agent {
    /// Returns false to stop the agent.
    fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Tick => {
                self.ticks += 1;
                if self.ticks.is_multiple_of(POLL_TICKS) {
                    self.check_config();
                    if self.needs.controller {
                        self.controller = detect::controller_connected();
                    }
                    self.evaluate_triggers();
                }
                if self.fallback_due.is_some_and(|due| Instant::now() >= due) {
                    self.fallback_due = None;
                    self.check_fallback();
                }
            }
            Event::DisplaysChanged => {
                self.refresh_tray();
                let fallback = self.cfg.settings.disconnect_fallback;
                if fallback.enabled {
                    let debounce = Duration::from_millis(fallback.debounce_ms.max(500).into());
                    self.fallback_due = Some(Instant::now() + debounce);
                } else {
                    self.known_active = active_connectors();
                }
            }
            Event::Tray(command) => match command {
                TrayCommand::Exit => return false,
                TrayCommand::Apply(group) if !self.busy => self.start(Job::Apply {
                    group,
                    trigger: Trigger::Tray,
                    confirm: true,
                }),
                TrayCommand::Recover if !self.busy => self.start(Job::Recover),
                TrayCommand::Settings => open_settings(),
                TrayCommand::ToggleStartup => self.toggle_startup(),
                _ => {}
            },
            Event::JobDone => {
                self.busy = false;
                self.last_switch = Some(Instant::now());
                self.known_active = active_connectors();
                self.refresh_tray();
            }
        }
        true
    }

    fn tray_model(&self) -> TrayModel {
        let ids: Vec<Uuid> = self.cfg.groups.iter().map(|g| g.id).collect();
        let current = self.dry_current.or_else(|| {
            Mutter::connect()
                .and_then(|m| m.state())
                .ok()
                .and_then(|s| dg_gnome::current_group(&self.cfg, &s, &ids))
        });
        TrayModel {
            groups: self
                .cfg
                .groups
                .iter()
                .map(|g| (g.id, g.name.clone()))
                .collect(),
            current,
            startup: Dirs::from_env().is_some_and(|d| desktop::autostart_enabled(&d)),
            labels: TrayLabels {
                tooltip: t("tray.tooltip"),
                no_groups: t("tray.noGroups"),
                recover: t("tray.recover"),
                settings: t("tray.settings"),
                startup: t("tray.startAtLogin"),
                exit: t("tray.exit"),
            },
        }
    }

    fn refresh_tray(&self) {
        if let Some(tray) = &self.tray {
            tray.set(self.tray_model());
        }
    }

    fn toggle_startup(&mut self) {
        let Some(dirs) = Dirs::from_env() else {
            return;
        };
        let result = if desktop::autostart_enabled(&dirs) {
            desktop::set_autostart(&dirs, None)
        } else {
            crate::commands::self_exe()
                .map_err(|e| e.to_string())
                .and_then(|exe| desktop::set_autostart(&dirs, Some(&exe)))
        };
        if let Err(e) = result {
            report(&e);
        }
        self.refresh_tray();
    }

    fn check_config(&mut self) {
        let stamp = modified(&self.ctx.config_path);
        if stamp == self.config_stamp {
            return;
        }
        self.config_stamp = stamp;
        match store::load(&self.ctx.config_path) {
            Ok(loaded) => {
                if loaded.config.settings.language != self.cfg.settings.language {
                    crate::i18n::init(&loaded.config.settings.language);
                }
                self.cfg = loaded.config;
                self.sync_hotkeys();
                self.arm_triggers();
                self.refresh_tray();
                tracing::info!("config reloaded");
            }
            Err(e) => tracing::warn!("config not reloaded: {e}"),
        }
    }

    /// Writes the config's hotkeys into GNOME's custom shortcuts when they changed.
    fn sync_hotkeys(&mut self) {
        if self.dry_run {
            tracing::info!("dry run: GNOME shortcuts are left as they are");
            return;
        }
        let (shortcuts, mut problems) = shortcuts_for(&self.cfg);
        // GNOME's own shortcuts win over custom ones: on Wayland, Ctrl+Alt+F1…F12
        // switch virtual terminals, for example.
        for s in &shortcuts {
            if let Ok(used) = keybindings::conflicts(&s.binding) {
                if !used.is_empty() {
                    problems.push(format!(
                        "{} ({}): {}",
                        s.name,
                        s.binding,
                        t("hotkeys.usedByGnome").replace("{what}", &used.join(", "))
                    ));
                }
            }
        }
        if self.synced.as_ref() != Some(&shortcuts) {
            match keybindings::sync(&shortcuts) {
                Ok(()) => {
                    tracing::info!(count = shortcuts.len(), "GNOME shortcuts updated");
                    self.synced = Some(shortcuts);
                }
                Err(e) => report(&format!("{}\n{e}", t("hotkeys.notActive"))),
            }
        }
        if !problems.is_empty() {
            report(&format!(
                "{}\n{}\n{}",
                t("hotkeys.notActive"),
                problems.join("\n"),
                t("hotkeys.chooseOther")
            ));
        }
    }

    /// Starts or stops the trigger checks for the current config, and takes a first
    /// look, which only primes the triggers (ARCHITECTURE.md §15.1).
    fn arm_triggers(&mut self) {
        self.needs = Triggers::needs(&self.cfg);
        if self.needs.controller {
            self.controller = detect::controller_connected();
        }
        if self.needs.windows && !self.warned_titles {
            self.warned_titles = true;
            tracing::warn!(
                "window-title filters are not available on GNOME; those triggers never start"
            );
        }
        tracing::info!(
            triggers = self.cfg.triggers.len(),
            programs = self.needs.programs,
            controller = self.needs.controller,
            "triggers armed"
        );
        self.evaluate_triggers();
    }

    /// Samples what the enabled triggers watch and starts at most one switch.
    fn evaluate_triggers(&mut self) {
        let needs = self.needs;
        if needs == Needs::default() || self.busy {
            return;
        }
        let obs = Observation {
            programs: if needs.programs {
                detect::running_program_names()
            } else {
                HashSet::new()
            },
            // GNOME on Wayland does not tell other programs' window titles.
            windows: Vec::new(),
            controller: self.controller,
            now_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        };
        let cfg = &self.cfg;
        let dry = if self.dry_run { self.dry_current } else { None };
        let mut current = || {
            dry.or_else(|| {
                let ids: Vec<Uuid> = cfg.groups.iter().map(|g| g.id).collect();
                Mutter::connect()
                    .and_then(|m| m.state())
                    .ok()
                    .and_then(|s| dg_gnome::current_group(cfg, &s, &ids))
            })
        };
        let Some(decision) = self.triggers.evaluate(cfg, &obs, &mut current) else {
            return;
        };
        let confirm = cfg
            .triggers
            .iter()
            .find(|t| t.id == decision.trigger())
            .is_some_and(|t| t.confirm);
        let group = decision.group();
        match decision {
            Decision::SwitchTo { .. } => tracing::info!(%group, "trigger started: switching"),
            Decision::SwitchBack { .. } => tracing::info!(%group, "trigger ended: switching back"),
        }
        if self.dry_run {
            self.dry_current = Some(group);
        }
        self.start(Job::Apply {
            group,
            trigger: Trigger::Auto,
            confirm,
        });
    }

    fn check_fallback(&mut self) {
        let now_active = active_connectors();
        let previous = std::mem::replace(&mut self.known_active, now_active.clone());
        let settings = self.cfg.settings.disconnect_fallback;
        if !settings.enabled || self.busy {
            return;
        }
        let recent =
            |at: Option<Instant>, window: Duration| at.is_some_and(|t| t.elapsed() < window);
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
        let ready = Mutter::connect()
            .and_then(|m| m.state())
            .is_ok_and(|s| dg_gnome::plan(&self.cfg, group, &s).is_ok());
        if !ready {
            tracing::info!(group = %group.name, "displays disconnected, but the fallback group is not available");
            return;
        }
        tracing::info!(group = %group.name, "displays disconnected; switching to the fallback group");
        self.last_fallback = Some(Instant::now());
        let id = group.id;
        self.start(Job::Apply {
            group: id,
            trigger: Trigger::Fallback,
            confirm: true,
        });
    }

    fn start(&mut self, job: Job) {
        self.busy = true;
        let ctx = self.ctx.clone();
        let cfg = self.cfg.clone();
        let (out, dry_run) = (self.out, self.dry_run);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = if dry_run {
                preview(&ctx, &cfg, &job);
                Ok(())
            } else {
                execute(&ctx, &cfg, &job, out)
            };
            if let Err(e) = result {
                report(&e.to_string());
            }
            let _ = tx.send(Event::JobDone);
        });
    }
}

fn execute(ctx: &Ctx, cfg: &Config, job: &Job, out: Output) -> Result<(), CliError> {
    match job {
        Job::Apply {
            group,
            trigger,
            confirm,
        } => {
            commands::apply(
                ctx,
                cfg,
                &group_by_id(cfg, *group)?,
                !confirm,
                *trigger,
                out,
            )?;
        }
        Job::Recover => {
            commands::recover(ctx, false, out)?;
        }
    }
    Ok(())
}

/// What a switch *would* do, for `agent --dry-run`: a notification only.
fn preview(ctx: &Ctx, cfg: &Config, job: &Job) {
    let (title, icon) = match job {
        Job::Apply { group, .. } => match cfg.group_by_id(*group) {
            Some(g) => (g.name.clone(), g.icon.clone()),
            None => return,
        },
        Job::Recover => (t("tray.recover"), "builtin:monitor".into()),
    };
    commands::announce(ctx, cfg, &title, &t("osd.dryRun"), &icon);
}

fn group_by_id(cfg: &Config, id: Uuid) -> Result<Group, CliError> {
    cfg.group_by_id(id).cloned().ok_or_else(|| {
        CliError::Config(format!(
            "a tray item or trigger points at a group that no longer exists ({id})"
        ))
    })
}

/// Starts the settings app installed next to the agent. From an AppImage it is started
/// through the AppImage file, so the window does not depend on the agent's mount.
fn open_settings() {
    let exe = dg_gnome::system::appimage().or_else(|| {
        std::env::current_exe()
            .ok()
            .map(|p| p.with_file_name("displaygroups-settings"))
    });
    match exe {
        Some(exe) if exe.is_file() => {
            if let Err(e) = std::process::Command::new(&exe).spawn() {
                tracing::warn!("cannot start {}: {e}", exe.display());
            }
        }
        _ => {
            let _ = dg_gnome::notify::notify(
                "DisplayGroups",
                &t("tray.settingsMissing"),
                "video-display",
                5000,
            );
        }
    }
}

/// The GNOME custom shortcuts for the config's hotkeys, running this program, and the
/// hotkeys that cannot be one.
fn shortcuts_for(cfg: &Config) -> (Vec<Shortcut>, Vec<String>) {
    match commands::self_exe() {
        Ok(exe) => keybindings::for_config(cfg, &exe, &t("tray.recover")),
        Err(e) => (Vec::new(), vec![e.to_string()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_only_when_every_display_is_gone() {
        let tv = vec!["HDMI-1".to_string()];
        let desk = vec!["DP-1".to_string(), "DP-2".to_string()];
        assert!(fallback_needed(&tv, &desk));
        assert!(!fallback_needed(&desk, &["DP-1".to_string()]));
        assert!(!fallback_needed(&[], &desk));
    }
}

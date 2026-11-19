//! The switching state machine (ARCHITECTURE.md §6):
//! Resolve → Snapshot → Plan → Try (S1..S4) → Settle → Reconcile → UserFixups →
//! Verify → Confirm → Commit | Revert.

use std::collections::HashSet;

use dg_identity::{match_monitor, monitor_key};
use dg_model::{
    AudioSettings, Config, DisplaySetting, Group, Mode, MonitorEntry, OthersPolicy, Position,
    RefreshPolicy,
};
use serde::Serialize;
use uuid::Uuid;

use crate::backend::{AudioBackend, DisplayBackend, ModeChange};
use crate::ccd::{
    rotation_from_ccd, sdc, win32, PathInfo, Scope, TargetDetails, TargetRef, Topology,
};
use crate::dpi::{self, DpiPlan};
use crate::observe::{display_name_of, identity_of, observe};
use crate::plan::{self, Placement, Strategy};
use crate::refresh::pick_refresh_hz;
use crate::report::{ApplyReport, Attempt, PrimaryInfo, ValidateReport};
use crate::resolve::resolve_group;
use crate::snapshot::{relink, Snapshot};
use crate::EngineError;

#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// False in the lock-screen broker (SYSTEM): per-user DPI and audio are skipped.
    pub user_context: bool,
    pub settle_timeout_ms: u32,
    pub poll_interval_ms: u32,
    pub audio_wait_ms: u32,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            user_context: true,
            settle_timeout_ms: 5000,
            poll_interval_ms: 250,
            audio_wait_ms: 5000,
        }
    }
}

/// Asks the user whether to keep a new layout. Returning false reverts it.
pub trait Confirmer {
    fn confirm(&self, seconds: u32, group_name: &str) -> bool;
}

#[derive(Default)]
pub struct ApplyOptions<'a> {
    /// 0 skips the confirmation (and therefore the auto-revert).
    pub confirm_seconds: u32,
    pub confirmer: Option<&'a dyn Confirmer>,
}

/// A layout captured from the screen, ready to be stored as a group.
#[derive(Debug, Clone)]
pub struct Captured {
    /// Monitors not yet in the config.
    pub new_monitors: Vec<(String, MonitorEntry)>,
    pub displays: Vec<DisplaySetting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoverMethod {
    LastKnownGood,
    LastKnownGoodTopology,
    Extend,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDisplay {
    pub name: String,
    pub monitor_key: Option<String>,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub refresh_milli_hz: Option<u32>,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedMonitor {
    pub name: String,
    pub monitor_key: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub displays: Vec<ActiveDisplay>,
    pub connected: Vec<ConnectedMonitor>,
}

struct Rollback {
    active: Topology,
    dpi: Vec<(TargetRef, i32)>,
}

pub struct Engine<'a> {
    display: &'a dyn DisplayBackend,
    audio: Option<&'a dyn AudioBackend>,
    options: EngineOptions,
}

impl<'a> Engine<'a> {
    pub fn new(display: &'a dyn DisplayBackend) -> Self {
        Self {
            display,
            audio: None,
            options: EngineOptions::default(),
        }
    }

    pub fn with_audio(mut self, audio: &'a dyn AudioBackend) -> Self {
        self.audio = Some(audio);
        self
    }

    pub fn with_options(mut self, options: EngineOptions) -> Self {
        self.options = options;
        self
    }

    /// Switches to `group`. On success the layout is live; `report.kept` tells whether
    /// the user kept it or it was reverted.
    pub fn apply(
        &self,
        cfg: &Config,
        group: &Group,
        opts: &ApplyOptions,
    ) -> Result<ApplyReport, EngineError> {
        let all = self.display.query(Scope::All)?;
        let active = self.display.query(Scope::Active)?;
        let resolved = resolve_group(cfg, group, &all)?;
        let placements = plan::placements(cfg, group, &resolved, &all, &active)?;
        let rollback = self.rollback_point(active);

        let mut attempts = Vec::new();
        let strategy = self.run_ladder(&placements, &mut attempts)?;
        tracing::info!(group = %group.name, %strategy, "topology applied");

        let mut notes = Vec::new();
        self.settle(&placements);
        self.reconcile(&placements, &mut notes)?;
        if self.options.user_context {
            self.user_fixups(group, &placements, &mut notes);
        }

        let mut final_state = self.display.query(Scope::Active)?;
        for p in placements.iter().filter(|p| p.display_index.is_some()) {
            if final_state.active_path_for(p.target).is_none() {
                notes.push(format!("{} is not on after the switch", p.name));
            }
        }
        let primary = primary_info(&final_state);

        let mut kept = true;
        if let (true, Some(confirmer)) = (opts.confirm_seconds > 0, opts.confirmer) {
            if !confirmer.confirm(opts.confirm_seconds, &group.name) {
                self.revert(&rollback, &mut notes);
                kept = false;
                final_state = self.display.query(Scope::Active)?;
            }
        }

        Ok(ApplyReport {
            group_id: group.id,
            group_name: group.name.clone(),
            attempts,
            strategy: Some(strategy),
            notes,
            kept,
            primary: if kept {
                primary
            } else {
                primary_info(&final_state)
            },
            final_state,
        })
    }

    /// Dry run: asks Windows to validate every strategy without changing anything.
    pub fn validate(&self, cfg: &Config, group: &Group) -> Result<ValidateReport, EngineError> {
        let all = self.display.query(Scope::All)?;
        let active = self.display.query(Scope::Active)?;
        let resolved = resolve_group(cfg, group, &all)?;
        let placements = plan::placements(cfg, group, &resolved, &all, &active)?;
        let mut attempts = Vec::new();
        for strategy in Strategy::LADDER {
            let Some(p) = plan::build(strategy, &placements) else {
                continue;
            };
            if let Err(e) = plan::check_invariants(&p) {
                tracing::warn!(%strategy, "skipping: {e}");
                continue;
            }
            let code = self
                .display
                .set_config(&p.paths, &p.modes, p.flags | sdc::VALIDATE);
            if code == win32::ACCESS_DENIED {
                return Err(EngineError::Locked);
            }
            attempts.push(Attempt {
                strategy,
                validate: code,
                apply: None,
            });
        }
        Ok(ValidateReport {
            group_id: group.id,
            group_name: group.name.clone(),
            would_use: attempts
                .iter()
                .find(|a| a.validate == 0)
                .map(|a| a.strategy),
            attempts,
        })
    }

    /// Restores the last-known-good layout, or Windows' "Extend" topology.
    pub fn recover(
        &self,
        snapshot: Option<&Snapshot>,
        extend: bool,
    ) -> Result<RecoverMethod, EngineError> {
        if let (false, Some(snapshot)) = (extend, snapshot) {
            let all = self.display.query(Scope::All)?;
            match relink(&snapshot.topology, &all) {
                Ok(topo) => {
                    let paths: Vec<PathInfo> = topo.active_paths().copied().collect();
                    let flags = sdc::USE_SUPPLIED_DISPLAY_CONFIG
                        | sdc::SAVE_TO_DATABASE
                        | sdc::ALLOW_CHANGES
                        | sdc::APPLY;
                    match self.display.set_config(&paths, &topo.modes, flags) {
                        0 => return Ok(RecoverMethod::LastKnownGood),
                        win32::ACCESS_DENIED => return Err(EngineError::Locked),
                        code => tracing::warn!(code, "exact last-known-good layout rejected"),
                    }
                    let bare: Vec<PathInfo> = paths.iter().map(plan::bare_path).collect();
                    let code = self.display.set_config(&bare, &[], flags);
                    if code == 0 {
                        return Ok(RecoverMethod::LastKnownGoodTopology);
                    }
                    tracing::warn!(code, "last-known-good topology rejected");
                }
                Err(e) => tracing::warn!("cannot re-link the last-known-good layout: {e}"),
            }
        }
        match self
            .display
            .set_config(&[], &[], sdc::TOPOLOGY_EXTEND | sdc::APPLY)
        {
            0 => Ok(RecoverMethod::Extend),
            win32::ACCESS_DENIED => Err(EngineError::Locked),
            code => Err(EngineError::Win32 {
                op: "SetDisplayConfig(SDC_TOPOLOGY_EXTEND)",
                code,
            }),
        }
    }

    /// Captures the current layout. Monitors already in `cfg` keep their keys.
    pub fn capture(&self, cfg: &Config) -> Result<Captured, EngineError> {
        let active = self.display.query(Scope::Active)?;
        let mut used: HashSet<String> = cfg.monitors.keys().cloned().collect();
        let mut new_monitors = Vec::new();
        let mut displays = Vec::new();

        for path in active.active_paths() {
            let target = path.target_ref();
            let Some(details) = active.target(target) else {
                continue;
            };
            let Some(src) = active.source_mode(path) else {
                continue;
            };
            let key = match known_monitor(cfg, details) {
                Some(k) => k,
                None => {
                    let identity = identity_of(details);
                    let base = monitor_key(
                        &identity.pnp_id,
                        &identity.product_code,
                        identity.serial.as_deref(),
                        identity.connector_hint.as_deref(),
                    );
                    let key = unique_key(&base, &used);
                    new_monitors.push((
                        key.clone(),
                        MonitorEntry {
                            display_name: display_name_of(details),
                            identity,
                            extra: Default::default(),
                        },
                    ));
                    key
                }
            };
            used.insert(key.clone());

            let rotation = rotation_from_ccd(path.target.rotation);
            let (width, height) = rotation.apply(src.width, src.height);
            let scale_percent = if self.options.user_context {
                self.display
                    .get_dpi(target)
                    .ok()
                    .and_then(|i| i.current_percent())
            } else {
                None
            };
            displays.push(DisplaySetting {
                monitor: key,
                enabled: true,
                mode: Mode {
                    width,
                    height,
                    refresh_milli_hz: active.refresh_milli_hz(path).unwrap_or(60_000),
                    refresh_policy: RefreshPolicy::BestAtOrBelow,
                },
                position: Position { x: src.x, y: src.y },
                rotation,
                scale_percent,
                hdr: None,
                captured: active.target_mode(path).map(Into::into),
                extra: Default::default(),
            });
        }
        if displays.is_empty() {
            return Err(EngineError::Unsupported(
                "no active displays to capture".into(),
            ));
        }
        Ok(Captured {
            new_monitors,
            displays,
        })
    }

    /// Which of `candidates` matches what is on screen now (for toggle/cycle).
    pub fn current_group(
        &self,
        cfg: &Config,
        candidates: &[Uuid],
    ) -> Result<Option<Uuid>, EngineError> {
        let all = self.display.query(Scope::All)?;
        let active = self.display.query(Scope::Active)?;
        let on_screen: HashSet<TargetRef> = active.active_targets().into_iter().collect();
        let primary = active.primary_target();
        for id in candidates {
            let Some(group) = cfg.group_by_id(*id) else {
                continue;
            };
            let Ok(resolved) = resolve_group(cfg, group, &all) else {
                continue;
            };
            let enabled: Vec<_> = resolved
                .iter()
                .filter(|r| group.displays[r.display_index].enabled)
                .collect();
            let wanted: HashSet<TargetRef> = enabled.iter().map(|r| r.target).collect();
            let group_primary = enabled
                .iter()
                .find(|r| {
                    let p = group.displays[r.display_index].position;
                    p.x == 0 && p.y == 0
                })
                .map(|r| r.target);
            let same_set = match group.others_policy {
                OthersPolicy::Disable => wanted == on_screen,
                OthersPolicy::Leave => wanted.is_subset(&on_screen),
            };
            if same_set && group_primary == primary {
                return Ok(Some(*id));
            }
        }
        Ok(None)
    }

    /// What is connected and on screen, mapped to config monitor keys.
    pub fn status(&self, cfg: &Config) -> Result<Status, EngineError> {
        let all = self.display.query(Scope::All)?;
        let active = self.display.query(Scope::Active)?;
        let on_screen: HashSet<TargetRef> = active.active_targets().into_iter().collect();
        let displays = active
            .active_paths()
            .filter_map(|p| {
                let d = active.target(p.target_ref())?;
                let s = active.source_mode(p)?;
                Some(ActiveDisplay {
                    name: display_name_of(d),
                    monitor_key: known_monitor(cfg, d),
                    width: s.width,
                    height: s.height,
                    x: s.x,
                    y: s.y,
                    refresh_milli_hz: active.refresh_milli_hz(p),
                    primary: s.x == 0 && s.y == 0,
                })
            })
            .collect();
        let connected = all
            .targets
            .iter()
            .filter(|d| d.available)
            .map(|d| ConnectedMonitor {
                name: display_name_of(d),
                monitor_key: known_monitor(cfg, d),
                active: on_screen.contains(&d.target),
            })
            .collect();
        Ok(Status {
            displays,
            connected,
        })
    }

    // ----- ladder -----

    fn run_ladder(
        &self,
        placements: &[Placement],
        attempts: &mut Vec<Attempt>,
    ) -> Result<Strategy, EngineError> {
        for strategy in Strategy::LADDER {
            let Some(p) = plan::build(strategy, placements) else {
                continue;
            };
            if let Err(e) = plan::check_invariants(&p) {
                tracing::warn!(%strategy, "skipping: {e}");
                continue;
            }
            let validate = self
                .display
                .set_config(&p.paths, &p.modes, p.flags | sdc::VALIDATE);
            if validate == win32::ACCESS_DENIED {
                return Err(EngineError::Locked);
            }
            if validate == win32::INVALID_PARAMETER {
                tracing::warn!(%strategy, plan = ?p, "Windows rejected the arrays with error 87");
            }
            if validate != 0 {
                tracing::info!(%strategy, validate, "strategy rejected, trying the next one");
                attempts.push(Attempt {
                    strategy,
                    validate,
                    apply: None,
                });
                continue;
            }
            let applied = self
                .display
                .set_config(&p.paths, &p.modes, p.flags | sdc::APPLY);
            attempts.push(Attempt {
                strategy,
                validate,
                apply: Some(applied),
            });
            match applied {
                0 => return Ok(strategy),
                win32::ACCESS_DENIED => return Err(EngineError::Locked),
                code => tracing::warn!(%strategy, code, "validated but apply failed"),
            }
        }
        Err(EngineError::AllStrategiesFailed {
            attempts: attempts.clone(),
        })
    }

    fn rollback_point(&self, active: Topology) -> Rollback {
        let dpi = if self.options.user_context {
            active
                .active_targets()
                .into_iter()
                .filter_map(|t| self.display.get_dpi(t).ok().map(|i| (t, i.cur_rel)))
                .collect()
        } else {
            Vec::new()
        };
        Rollback { active, dpi }
    }

    fn settle(&self, placements: &[Placement]) {
        let wanted: Vec<TargetRef> = placements
            .iter()
            .filter(|p| p.display_index.is_some())
            .map(|p| p.target)
            .collect();
        let mut waited = 0;
        loop {
            if let Ok(active) = self.display.query(Scope::Active) {
                if wanted.iter().all(|t| active.active_path_for(*t).is_some()) {
                    return;
                }
            }
            if waited >= self.options.settle_timeout_ms {
                tracing::warn!("displays did not all come up within {waited} ms");
                return;
            }
            self.display.sleep_ms(self.options.poll_interval_ms);
            waited += self.options.poll_interval_ms;
        }
    }

    /// Corrects resolution, refresh rate, position and orientation that Windows chose
    /// differently (S2–S4 let it pick the signal timing).
    fn reconcile(
        &self,
        placements: &[Placement],
        notes: &mut Vec<String>,
    ) -> Result<(), EngineError> {
        let mut changes: Vec<(String, ModeChange)> = Vec::new();
        for p in placements.iter().filter(|p| p.display_index.is_some()) {
            let current = match self.display.current_mode(p.target) {
                Ok(m) => m,
                Err(e) => {
                    notes.push(format!("{}: cannot read the current mode ({e})", p.name));
                    continue;
                }
            };
            let modes = self.display.list_modes(p.target).unwrap_or_default();
            let (width, height, refresh_hz) = match pick_refresh_hz(
                &modes,
                p.native_width,
                p.native_height,
                p.refresh_milli_hz,
                p.refresh_policy,
            ) {
                Some(hz) => (p.desktop_width, p.desktop_height, hz),
                None => {
                    notes.push(format!(
                        "{}: {}×{} at {} Hz is not offered; kept {}×{} at {} Hz",
                        p.name,
                        p.native_width,
                        p.native_height,
                        p.refresh_milli_hz as f64 / 1000.0,
                        current.width,
                        current.height,
                        current.refresh_hz
                    ));
                    (current.width, current.height, current.refresh_hz)
                }
            };
            let orientation = p.rotation.saturating_sub(1);
            let differs = (current.width, current.height, current.refresh_hz)
                != (width, height, refresh_hz)
                || (current.x, current.y) != (p.x, p.y)
                || current.orientation != orientation;
            if differs {
                changes.push((
                    p.name.clone(),
                    ModeChange {
                        target: p.target,
                        width,
                        height,
                        refresh_hz,
                        x: p.x,
                        y: p.y,
                        orientation: (current.orientation != orientation).then_some(orientation),
                        primary: p.x == 0 && p.y == 0,
                    },
                ));
            }
        }
        if changes.is_empty() {
            return Ok(());
        }
        let only: Vec<ModeChange> = changes.iter().map(|(_, c)| *c).collect();
        let codes = self.display.apply_modes(&only)?;
        for ((name, change), code) in changes.iter().zip(codes) {
            if code == 0 {
                notes.push(format!(
                    "{name}: set {}×{} at {} Hz",
                    change.width, change.height, change.refresh_hz
                ));
            } else {
                notes.push(format!(
                    "{name}: Windows refused the mode change (code {code})"
                ));
            }
        }
        Ok(())
    }

    fn user_fixups(&self, group: &Group, placements: &[Placement], notes: &mut Vec<String>) {
        for p in placements {
            let Some(index) = p.display_index else {
                continue;
            };
            let d = &group.displays[index];
            if let Some(want) = d.scale_percent {
                self.apply_scale(p, want, notes);
            }
            if let Some(want) = d.hdr {
                self.apply_hdr(p, want, notes);
            }
        }
        if group.audio.enabled {
            self.apply_audio(&group.audio, notes);
        }
    }

    fn apply_scale(&self, p: &Placement, want: u32, notes: &mut Vec<String>) {
        let info = match self.display.get_dpi(p.target) {
            Ok(i) => i,
            Err(e) => {
                notes.push(format!("{}: scale control is unavailable ({e})", p.name));
                return;
            }
        };
        match dpi::plan(info, want) {
            Ok(DpiPlan::Unchanged { .. }) => {}
            Ok(DpiPlan::Change {
                from_percent,
                to_percent,
                relative,
            }) => {
                if let Err(e) = self.display.set_dpi(p.target, relative) {
                    notes.push(format!(
                        "{}: could not set scale to {to_percent}% ({e})",
                        p.name
                    ));
                    return;
                }
                match self.display.get_dpi(p.target) {
                    Ok(after) if after.cur_rel == relative => notes.push(format!(
                        "{}: scale {}% → {to_percent}%",
                        p.name,
                        from_percent.map_or("?".into(), |f| f.to_string())
                    )),
                    Ok(after) => notes.push(format!(
                        "{}: Windows kept the scale at {}%",
                        p.name,
                        after
                            .current_percent()
                            .map_or("?".into(), |f| f.to_string())
                    )),
                    Err(e) => notes.push(format!("{}: could not re-read the scale ({e})", p.name)),
                }
            }
            Err(e) => notes.push(format!("{}: {e}", p.name)),
        }
    }

    fn apply_hdr(&self, p: &Placement, want: bool, notes: &mut Vec<String>) {
        match self.display.get_hdr(p.target) {
            Ok(s) if !s.supported => {
                if want {
                    notes.push(format!("{}: HDR is not supported", p.name));
                }
            }
            Ok(s) if s.enabled == want => {}
            Ok(_) => match self.display.set_hdr(p.target, want) {
                Ok(()) => notes.push(format!(
                    "{}: HDR {}",
                    p.name,
                    if want { "on" } else { "off" }
                )),
                Err(e) => notes.push(format!("{}: could not change HDR ({e})", p.name)),
            },
            Err(e) => notes.push(format!("{}: cannot read HDR state ({e})", p.name)),
        }
    }

    fn apply_audio(&self, audio: &AudioSettings, notes: &mut Vec<String>) {
        let Some(backend) = self.audio else {
            notes.push("audio switching is not available".into());
            return;
        };
        let endpoint_id = match &audio.endpoint {
            Some(m) => {
                let needle = m.name_contains.to_lowercase();
                let mut waited = 0;
                let found = loop {
                    match backend.render_endpoints() {
                        Ok(list) => {
                            if let Some(e) = list
                                .into_iter()
                                .find(|e| e.name.to_lowercase().contains(&needle))
                            {
                                break Some(e);
                            }
                        }
                        Err(e) => {
                            notes.push(format!("cannot list audio outputs ({e})"));
                            break None;
                        }
                    }
                    if waited >= self.options.audio_wait_ms {
                        notes.push(format!(
                            "no audio output matching \"{}\" appeared",
                            m.name_contains
                        ));
                        break None;
                    }
                    self.display.sleep_ms(self.options.poll_interval_ms);
                    waited += self.options.poll_interval_ms;
                };
                found.and_then(|e| match backend.set_default_endpoint(&e.id) {
                    Ok(()) => {
                        notes.push(format!("audio output → {}", e.name));
                        Some(e.id)
                    }
                    Err(err) => {
                        notes.push(format!("could not switch audio to {} ({err})", e.name));
                        None
                    }
                })
            }
            None => backend.default_endpoint().ok().flatten(),
        };
        if let (Some(volume), Some(id)) = (audio.volume_percent, endpoint_id) {
            match backend.set_volume(&id, volume.min(100)) {
                Ok(()) => notes.push(format!("volume → {}%", volume.min(100))),
                Err(e) => notes.push(format!("could not set the volume ({e})")),
            }
        }
    }

    fn revert(&self, rollback: &Rollback, notes: &mut Vec<String>) {
        let paths: Vec<PathInfo> = rollback.active.active_paths().copied().collect();
        let flags = sdc::USE_SUPPLIED_DISPLAY_CONFIG
            | sdc::SAVE_TO_DATABASE
            | sdc::ALLOW_CHANGES
            | sdc::APPLY;
        let mut code = self
            .display
            .set_config(&paths, &rollback.active.modes, flags);
        if code != 0 {
            tracing::warn!(
                code,
                "exact revert rejected; using Windows' remembered layout"
            );
            let bare: Vec<PathInfo> = paths.iter().map(plan::bare_path).collect();
            code = self.display.set_config(
                &bare,
                &[],
                sdc::TOPOLOGY_SUPPLIED | sdc::ALLOW_PATH_ORDER_CHANGES | sdc::APPLY,
            );
        }
        if code != 0 {
            tracing::warn!(code, "topology revert rejected; extending all displays");
            code = self
                .display
                .set_config(&[], &[], sdc::TOPOLOGY_EXTEND | sdc::APPLY);
        }
        if self.options.user_context {
            for (target, rel) in &rollback.dpi {
                let _ = self.display.set_dpi(*target, *rel);
            }
        }
        if code == 0 {
            notes.push("reverted to the previous layout".into());
        } else {
            notes.push(format!(
                "could not revert (code {code}); press the recover hotkey"
            ));
        }
    }
}

/// The group after `current` in `list` (wrapping), or the first one if none is active.
pub fn next_in_cycle(list: &[Uuid], current: Option<Uuid>) -> Option<Uuid> {
    let first = *list.first()?;
    let Some(current) = current else {
        return Some(first);
    };
    match list.iter().position(|id| *id == current) {
        Some(i) => Some(list[(i + 1) % list.len()]),
        None => Some(first),
    }
}

fn primary_info(active: &Topology) -> Option<PrimaryInfo> {
    let path = active
        .active_paths()
        .find(|p| active.source_mode(p).is_some_and(|m| m.x == 0 && m.y == 0))?;
    let src = active.source_mode(path)?;
    Some(PrimaryInfo {
        width: src.width,
        height: src.height,
        refresh_milli_hz: active.refresh_milli_hz(path),
    })
}

fn known_monitor(cfg: &Config, details: &TargetDetails) -> Option<String> {
    let observed = observe(details);
    cfg.monitors
        .iter()
        .find(|(_, m)| {
            matches!(match_monitor(&m.identity, std::slice::from_ref(&observed)), Ok(x) if x.tier.is_certain())
        })
        .map(|(k, _)| k.clone())
}

fn unique_key(base: &str, used: &HashSet<String>) -> String {
    if !used.contains(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|k| !used.contains(k))
        .expect("an unused key always exists")
}

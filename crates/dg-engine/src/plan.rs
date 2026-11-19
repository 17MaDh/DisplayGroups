//! Building `SetDisplayConfig` arrays for each rung of the strategy ladder
//! (ARCHITECTURE.md §6) and checking them against the rules learned on real hardware.

use std::collections::HashSet;
use std::fmt;

use dg_model::{Config, Group, OthersPolicy, RefreshPolicy};
use serde::Serialize;

use crate::ccd::{
    rotation, rotation_to_ccd, scaling, sdc, Luid, ModeInfo, PathInfo, PathSource, PathTarget,
    Rational, TargetRef, Topology, VideoSignal, MODE_IDX_INVALID, PATH_ACTIVE, PIXELFORMAT_32BPP,
    SCANLINE_UNSPECIFIED,
};
use crate::refresh::TOLERANCE_MILLI_HZ;
use crate::resolve::Resolved;
use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Strategy {
    /// S1: paths + source modes + captured target timings.
    #[serde(rename = "S1")]
    FullSupplied,
    /// S2: paths + source modes; Windows picks the signal timing.
    #[serde(rename = "S2")]
    SourceModes,
    /// S3: paths only; Windows picks modes. Fixes the "forgotten set" error 31.
    #[serde(rename = "S3")]
    PathsOnly,
    /// S4: paths only, Windows restores what it remembers for this set.
    #[serde(rename = "S4")]
    WindowsDatabase,
}

impl Strategy {
    pub const LADDER: [Strategy; 4] = [
        Strategy::FullSupplied,
        Strategy::SourceModes,
        Strategy::PathsOnly,
        Strategy::WindowsDatabase,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Strategy::FullSupplied => "S1",
            Strategy::SourceModes => "S2",
            Strategy::PathsOnly => "S3",
            Strategy::WindowsDatabase => "S4",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Strategy::FullSupplied => "captured timings",
            Strategy::SourceModes => "resolution and position",
            Strategy::PathsOnly => "displays only",
            Strategy::WindowsDatabase => "Windows' remembered layout",
        }
    }

    /// Flags without `VALIDATE`/`APPLY`. Never combines `ALLOW_PATH_ORDER_CHANGES`
    /// with `USE_SUPPLIED_DISPLAY_CONFIG` (error 87) and always pairs
    /// `TOPOLOGY_SUPPLIED` with it (error 31 otherwise).
    pub fn flags(self) -> u32 {
        match self {
            Strategy::FullSupplied => sdc::USE_SUPPLIED_DISPLAY_CONFIG | sdc::SAVE_TO_DATABASE,
            Strategy::SourceModes | Strategy::PathsOnly => {
                sdc::USE_SUPPLIED_DISPLAY_CONFIG | sdc::ALLOW_CHANGES | sdc::SAVE_TO_DATABASE
            }
            Strategy::WindowsDatabase => sdc::TOPOLOGY_SUPPLIED | sdc::ALLOW_PATH_ORDER_CHANGES,
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.code(), self.description())
    }
}

/// Where one display goes in the new layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub target: TargetRef,
    pub source_adapter: Luid,
    pub source_id: u32,
    pub output_technology: u32,
    /// `DISPLAYCONFIG_ROTATION` (1..=4).
    pub rotation: u32,
    pub native_width: u32,
    pub native_height: u32,
    pub desktop_width: u32,
    pub desktop_height: u32,
    pub x: i32,
    pub y: i32,
    pub refresh_milli_hz: u32,
    pub refresh_policy: RefreshPolicy,
    pub captured: Option<VideoSignal>,
    /// Index into `group.displays`; `None` for a display kept as-is (`othersPolicy: leave`).
    pub display_index: Option<usize>,
    /// Current source and target modes of a kept display.
    pub keep_modes: Option<(ModeInfo, Option<ModeInfo>)>,
    pub name: String,
}

impl Placement {
    /// Timing usable for S1: the kept display's current timing, or a capture that still
    /// matches the configured resolution and refresh rate.
    pub fn exact_timing(&self) -> Option<VideoSignal> {
        match &self.keep_modes {
            Some((_, Some(ModeInfo::Target { signal, .. }))) => Some(*signal),
            Some(_) => None,
            None => self.captured.filter(|s| {
                s.active_width == self.native_width
                    && s.active_height == self.native_height
                    && s.v_sync
                        .milli_hz()
                        .is_some_and(|m| m.abs_diff(self.refresh_milli_hz) <= TOLERANCE_MILLI_HZ)
            }),
        }
    }
}

/// Lays out the group: picks a source for every enabled display and keeps other active
/// displays when the group says `othersPolicy: leave`.
pub fn placements(
    cfg: &Config,
    group: &Group,
    resolved: &[Resolved],
    all: &Topology,
    active: &Topology,
) -> Result<Vec<Placement>, EngineError> {
    let mut reserved: HashSet<(Luid, u32)> = HashSet::new();
    let mut out = Vec::new();

    if group.others_policy == OthersPolicy::Leave {
        for path in active.active_paths() {
            let target = path.target_ref();
            if resolved.iter().any(|r| r.target == target) {
                continue;
            }
            let Some(src) = active.source_mode(path) else {
                continue;
            };
            reserved.insert((path.source.adapter, path.source.id));
            let source_mode = active.modes[path.source.mode_idx as usize].clone();
            let target_mode = active
                .modes
                .get(path.target.mode_idx as usize)
                .filter(|m| matches!(m, ModeInfo::Target { .. }))
                .cloned();
            let rot = valid_rotation(path.target.rotation);
            let portrait = rot == rotation::ROTATE90 || rot == rotation::ROTATE270;
            let (native_width, native_height) = if portrait {
                (src.height, src.width)
            } else {
                (src.width, src.height)
            };
            out.push(Placement {
                target,
                source_adapter: path.source.adapter,
                source_id: path.source.id,
                output_technology: path.target.output_technology,
                rotation: rot,
                native_width,
                native_height,
                desktop_width: src.width,
                desktop_height: src.height,
                x: src.x,
                y: src.y,
                refresh_milli_hz: active.refresh_milli_hz(path).unwrap_or(0),
                refresh_policy: RefreshPolicy::BestAtOrBelow,
                captured: None,
                display_index: None,
                keep_modes: Some((source_mode, target_mode)),
                name: active
                    .target(target)
                    .map(|d| d.friendly_name.clone())
                    .unwrap_or_default(),
            });
        }
    }

    let enabled: Vec<&Resolved> = resolved
        .iter()
        .filter(|r| group.displays[r.display_index].enabled)
        .collect();

    // Pass 1: a target that is already on keeps its source (less for Windows to change).
    let mut chosen: Vec<Option<(Luid, u32)>> = vec![None; enabled.len()];
    for (slot, r) in chosen.iter_mut().zip(&enabled) {
        if let Some(p) = active.active_path_for(r.target) {
            let key = (p.source.adapter, p.source.id);
            if candidate_sources(all, r.target).contains(&key) && reserved.insert(key) {
                *slot = Some(key);
            }
        }
    }
    // Pass 2: everyone else takes the lowest free source on its adapter.
    for (slot, r) in chosen.iter_mut().zip(&enabled) {
        if slot.is_some() {
            continue;
        }
        let key = candidate_sources(all, r.target)
            .into_iter()
            .find(|k| !reserved.contains(k))
            .ok_or_else(|| EngineError::NoSourceAvailable {
                name: monitor_name(cfg, group, r),
            })?;
        reserved.insert(key);
        *slot = Some(key);
    }

    for (r, key) in enabled.iter().zip(chosen) {
        let (source_adapter, source_id) = key.expect("every slot is assigned above");
        let d = &group.displays[r.display_index];
        let (desktop_width, desktop_height) = d.desktop_size();
        out.push(Placement {
            target: r.target,
            source_adapter,
            source_id,
            output_technology: all
                .paths
                .iter()
                .find(|p| p.target_ref() == r.target)
                .map(|p| p.target.output_technology)
                .unwrap_or_default(),
            rotation: rotation_to_ccd(d.rotation),
            native_width: d.mode.width,
            native_height: d.mode.height,
            desktop_width,
            desktop_height,
            x: d.position.x,
            y: d.position.y,
            refresh_milli_hz: d.mode.refresh_milli_hz,
            refresh_policy: d.mode.refresh_policy,
            captured: d.captured.map(VideoSignal::from),
            display_index: Some(r.display_index),
            keep_modes: None,
            name: monitor_name(cfg, group, r),
        });
    }
    Ok(out)
}

fn monitor_name(cfg: &Config, group: &Group, r: &Resolved) -> String {
    let key = &group.displays[r.display_index].monitor;
    cfg.monitors
        .get(key)
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| key.clone())
}

fn candidate_sources(all: &Topology, target: TargetRef) -> Vec<(Luid, u32)> {
    let mut v: Vec<(Luid, u32)> = all
        .paths
        .iter()
        .filter(|p| p.target_ref() == target)
        .map(|p| (p.source.adapter, p.source.id))
        .collect();
    v.sort_by_key(|(a, id)| (a.high, a.low, *id));
    v.dedup();
    v
}

fn valid_rotation(value: u32) -> u32 {
    if (rotation::IDENTITY..=rotation::ROTATE270).contains(&value) {
        value
    } else {
        rotation::IDENTITY
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub strategy: Strategy,
    pub paths: Vec<PathInfo>,
    pub modes: Vec<ModeInfo>,
    /// Without `VALIDATE`/`APPLY`.
    pub flags: u32,
}

/// Builds the arrays for `strategy`, or `None` when it does not apply (S1 without timings).
pub fn build(strategy: Strategy, placements: &[Placement]) -> Option<Plan> {
    let with_source_modes = matches!(strategy, Strategy::FullSupplied | Strategy::SourceModes);
    let with_target_modes = strategy == Strategy::FullSupplied;
    if placements.is_empty()
        || (with_target_modes && !placements.iter().all(|p| p.exact_timing().is_some()))
    {
        return None;
    }

    let mut paths = Vec::with_capacity(placements.len());
    let mut modes = Vec::new();
    for p in placements {
        let mut source_idx = MODE_IDX_INVALID;
        let mut target_idx = MODE_IDX_INVALID;
        let mut refresh = Rational::ZERO;
        let mut scanline = SCANLINE_UNSPECIFIED;
        if with_source_modes {
            let mode = match &p.keep_modes {
                Some((source, _)) => source.clone(),
                None => ModeInfo::Source {
                    adapter: p.source_adapter,
                    id: p.source_id,
                    width: p.desktop_width,
                    height: p.desktop_height,
                    pixel_format: PIXELFORMAT_32BPP,
                    x: p.x,
                    y: p.y,
                },
            };
            source_idx = push(&mut modes, mode);
        }
        if with_target_modes {
            let signal = p.exact_timing().expect("checked above");
            target_idx = push(
                &mut modes,
                ModeInfo::Target {
                    adapter: p.target.adapter,
                    id: p.target.id,
                    signal,
                },
            );
            refresh = signal.v_sync;
            scanline = signal.scanline_ordering;
        }
        paths.push(PathInfo {
            source: PathSource {
                adapter: p.source_adapter,
                id: p.source_id,
                mode_idx: source_idx,
                status_flags: 0,
            },
            target: PathTarget {
                adapter: p.target.adapter,
                id: p.target.id,
                mode_idx: target_idx,
                output_technology: p.output_technology,
                rotation: valid_rotation(p.rotation),
                scaling: scaling::IDENTITY,
                refresh,
                scanline_ordering: scanline,
                available: true,
                status_flags: 0,
            },
            flags: PATH_ACTIVE,
        });
    }
    Some(Plan {
        strategy,
        paths,
        modes,
        flags: strategy.flags(),
    })
}

fn push(modes: &mut Vec<ModeInfo>, mode: ModeInfo) -> u32 {
    modes.push(mode);
    u32::try_from(modes.len() - 1).expect("a handful of modes")
}

/// Rejects arrays that Windows is known to refuse. Every rule here comes from a real
/// failure (ARCHITECTURE.md §6).
pub fn check_invariants(plan: &Plan) -> Result<(), String> {
    let f = plan.flags;
    if f & sdc::USE_SUPPLIED_DISPLAY_CONFIG != 0 && f & sdc::ALLOW_PATH_ORDER_CHANGES != 0 {
        return Err(
            "ALLOW_PATH_ORDER_CHANGES with USE_SUPPLIED_DISPLAY_CONFIG returns error 87".into(),
        );
    }
    if f & sdc::TOPOLOGY_SUPPLIED != 0 && !plan.modes.is_empty() {
        return Err("TOPOLOGY_SUPPLIED takes paths only".into());
    }
    if f & sdc::TOPOLOGY_SUPPLIED != 0 && f & sdc::ALLOW_PATH_ORDER_CHANGES == 0 {
        return Err("TOPOLOGY_SUPPLIED without ALLOW_PATH_ORDER_CHANGES returns error 31".into());
    }
    let mut sources = HashSet::new();
    let mut targets = HashSet::new();
    for (i, p) in plan.paths.iter().enumerate() {
        if !(rotation::IDENTITY..=rotation::ROTATE270).contains(&p.target.rotation) {
            return Err(format!(
                "path {i}: rotation {} is invalid (error 87)",
                p.target.rotation
            ));
        }
        if !scaling::VALID.contains(&p.target.scaling) {
            return Err(format!(
                "path {i}: scaling {} is invalid (error 87)",
                p.target.scaling
            ));
        }
        if !p.is_active() {
            return Err(format!("path {i}: not marked active"));
        }
        if !sources.insert((p.source.adapter, p.source.id)) {
            return Err(format!("path {i}: source {} is used twice", p.source.id));
        }
        if !targets.insert(p.target_ref()) {
            return Err(format!("path {i}: target {} is used twice", p.target.id));
        }
        check_index(plan, p.source.mode_idx, true, p.source.id, i)?;
        check_index(plan, p.target.mode_idx, false, p.target.id, i)?;
    }
    let source_modes: Vec<_> = plan
        .modes
        .iter()
        .filter_map(|m| match m {
            ModeInfo::Source { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect();
    if !source_modes.is_empty() {
        let at_origin = source_modes
            .iter()
            .filter(|&&(x, y)| x == 0 && y == 0)
            .count();
        if at_origin != 1 {
            return Err(format!(
                "{at_origin} displays at (0,0); exactly one primary is required"
            ));
        }
    }
    Ok(())
}

fn check_index(plan: &Plan, index: u32, source: bool, id: u32, path: usize) -> Result<(), String> {
    if index == MODE_IDX_INVALID {
        return Ok(());
    }
    let ok = match plan.modes.get(index as usize) {
        Some(ModeInfo::Source { id: mid, .. }) => source && *mid == id,
        Some(ModeInfo::Target { id: mid, .. }) => !source && *mid == id,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "path {path}: mode index {index} does not point at its own mode"
        ))
    }
}

/// A path stripped down for "Windows picks the modes" calls (S3/S4, revert fallbacks).
pub fn bare_path(p: &PathInfo) -> PathInfo {
    let mut q = *p;
    q.source.mode_idx = MODE_IDX_INVALID;
    q.target.mode_idx = MODE_IDX_INVALID;
    q.target.rotation = valid_rotation(q.target.rotation);
    if !scaling::VALID.contains(&q.target.scaling) {
        q.target.scaling = scaling::IDENTITY;
    }
    q.target.refresh = Rational::ZERO;
    q.target.scanline_ordering = SCANLINE_UNSPECIFIED;
    q.source.status_flags = 0;
    q.target.status_flags = 0;
    q.flags = PATH_ACTIVE;
    q
}

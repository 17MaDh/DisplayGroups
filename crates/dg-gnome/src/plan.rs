//! A group turned into the arguments of Mutter's `ApplyMonitorsConfig`.
//!
//! Mutter replaces the whole layout in one call and turns off every monitor that is not
//! listed, so a plan lists exactly the displays that should be on. Mutter checks the
//! layout itself (adjacent, no overlaps, supported scales); `validate` asks it with the
//! "verify" method, which changes nothing.

use dg_identity::{match_monitor, MatchError, Observed};
use dg_model::{Config, DisplaySetting, Group, OthersPolicy, RefreshPolicy};
use serde::{Deserialize, Serialize};

use crate::layout::{relayout, Rect};
use crate::observe::observe;
use crate::ops::{layout_size, transform_of};
use crate::state::{DisplayState, LayoutMode, Monitor, MonitorMode};
use crate::GnomeError;

/// Rates within half a hertz count as the same rate (119.88 vs 120), as on Windows.
const TOLERANCE_MILLI_HZ: u32 = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyConfig {
    /// The state serial the plan was made from; Mutter rejects a stale one.
    pub serial: u32,
    pub logical_monitors: Vec<LogicalConfig>,
    /// Adjustments worth telling the user about (a mode that is not offered, …).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalConfig {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u32,
    pub primary: bool,
    /// `(connector, mode id)`.
    pub monitors: Vec<(String, String)>,
}

struct Placed<'a> {
    display: &'a DisplaySetting,
    monitor: &'a Monitor,
    mode: &'a MonitorMode,
    scale: f64,
}

pub fn plan(cfg: &Config, group: &Group, state: &DisplayState) -> Result<ApplyConfig, GnomeError> {
    let observed: Vec<Observed> = state.monitors.iter().map(observe).collect();
    let mut notes = Vec::new();
    let mut placed: Vec<Placed> = Vec::new();

    for d in group.enabled_displays() {
        let entry = cfg
            .monitors
            .get(&d.monitor)
            .ok_or_else(|| GnomeError::UnknownMonitor(d.monitor.clone()))?;
        let name = entry.display_name.clone();
        let index = match match_monitor(&entry.identity, &observed) {
            Ok(m) => m.index,
            Err(MatchError::NotFound) => return Err(GnomeError::MonitorMissing { name }),
            Err(MatchError::Ambiguous { .. }) => return Err(GnomeError::AmbiguousMonitor { name }),
        };
        let monitor = &state.monitors[index];
        if placed
            .iter()
            .any(|p| p.monitor.connector == monitor.connector)
        {
            return Err(GnomeError::AmbiguousMonitor { name });
        }
        let mode = match pick_mode(monitor, d) {
            Some(m) => m,
            None => {
                let fallback = monitor
                    .preferred_mode()
                    .ok_or_else(|| GnomeError::MonitorMissing { name: name.clone() })?;
                notes.push(format!(
                    "{name} does not offer {}×{} at about {} Hz here; using {}×{} at {:.0} Hz",
                    d.mode.width,
                    d.mode.height,
                    d.mode.refresh_milli_hz / 1000,
                    fallback.width,
                    fallback.height,
                    fallback.refresh
                ));
                fallback
            }
        };
        let wanted = d
            .scale_percent
            .map(|p| f64::from(p) / 100.0)
            .or_else(|| state.logical_for(&monitor.connector).map(|l| l.scale))
            .unwrap_or(mode.preferred_scale);
        placed.push(Placed {
            display: d,
            monitor,
            mode,
            scale: snap_scale(mode, wanted),
        });
    }
    if placed.is_empty() {
        return Err(GnomeError::NothingToShow);
    }

    let primary = placed
        .iter()
        .position(|p| p.display.position.x == 0 && p.display.position.y == 0)
        .unwrap_or(0);
    // X11: one scale for everything, the primary's.
    if state.global_scale_required {
        let scale = placed[primary].scale;
        for p in &mut placed {
            p.scale = snap_scale(p.mode, scale);
        }
    }

    let src: Vec<Rect> = placed
        .iter()
        .map(|p| {
            let (w, h) = p
                .display
                .rotation
                .apply(p.display.mode.width, p.display.mode.height);
            Rect {
                x: p.display.position.x,
                y: p.display.position.y,
                w: w as i32,
                h: h as i32,
            }
        })
        .collect();
    let sizes: Vec<(i32, i32)> = placed
        .iter()
        .map(|p| {
            let (w, h) = p.display.rotation.apply(p.mode.width, p.mode.height);
            layout_size(state.layout_mode, w, h, p.scale)
        })
        .collect();
    let units: Vec<f64> = placed
        .iter()
        .map(|p| match state.layout_mode {
            LayoutMode::Logical => 1.0 / p.scale,
            LayoutMode::Physical => 1.0,
        })
        .collect();
    let rects = relayout(&src, &sizes, &units, primary);

    let mut logical_monitors: Vec<LogicalConfig> = placed
        .iter()
        .zip(&rects)
        .enumerate()
        .map(|(i, (p, r))| LogicalConfig {
            x: r.x,
            y: r.y,
            scale: p.scale,
            transform: transform_of(p.display.rotation),
            primary: i == primary,
            monitors: vec![(p.monitor.connector.clone(), p.mode.id.clone())],
        })
        .collect();

    // "Leave as they are": keep the other displays that are on, to the right.
    if group.others_policy == OthersPolicy::Leave {
        let mut right = rects.iter().map(|r| r.x + r.w).max().unwrap_or(0);
        for l in &state.logical_monitors {
            if l.connectors
                .iter()
                .any(|c| placed.iter().any(|p| &p.monitor.connector == c))
            {
                continue;
            }
            let monitors: Vec<(String, String)> = l
                .connectors
                .iter()
                .filter_map(|c| {
                    let m = state.monitor(c)?;
                    Some((c.clone(), m.current_mode()?.id.clone()))
                })
                .collect();
            let Some(first) = l.connectors.first().and_then(|c| state.monitor(c)) else {
                continue;
            };
            let Some(mode) = first.current_mode() else {
                continue;
            };
            let (w, _) = crate::ops::rotated(mode.width, mode.height, l.transform);
            let (lw, _) = layout_size(state.layout_mode, w, 0, l.scale);
            logical_monitors.push(LogicalConfig {
                x: right,
                y: 0,
                scale: l.scale,
                transform: l.transform,
                primary: false,
                monitors,
            });
            right += lw;
        }
    }

    Ok(ApplyConfig {
        serial: state.serial,
        logical_monitors,
        notes,
    })
}

/// Every connected monitor on at its preferred mode, left to right, keeping the current
/// primary: the way out when nothing else works (`recover --extend`).
pub fn extend_all(state: &DisplayState) -> ApplyConfig {
    let primary = state
        .logical_monitors
        .iter()
        .find(|l| l.primary)
        .and_then(|l| l.connectors.first().cloned());
    let mut x = 0;
    let mut logical_monitors: Vec<LogicalConfig> = Vec::new();
    for m in &state.monitors {
        let Some(mode) = m.preferred_mode() else {
            continue;
        };
        let wanted = if state.global_scale_required {
            1.0
        } else {
            mode.preferred_scale
        };
        let scale = snap_scale(mode, wanted);
        let (w, _) = layout_size(state.layout_mode, mode.width, mode.height, scale);
        logical_monitors.push(LogicalConfig {
            x,
            y: 0,
            scale,
            transform: 0,
            primary: primary.as_deref() == Some(m.connector.as_str()),
            monitors: vec![(m.connector.clone(), mode.id.clone())],
        });
        x += w;
    }
    if !logical_monitors.iter().any(|l| l.primary) {
        if let Some(first) = logical_monitors.first_mut() {
            first.primary = true;
        }
    }
    ApplyConfig {
        serial: state.serial,
        logical_monitors,
        notes: Vec::new(),
    }
}

/// The mode to use for `d` on `m`: the same size, with the refresh rate chosen by the
/// display's policy.
fn pick_mode<'a>(m: &'a Monitor, d: &DisplaySetting) -> Option<&'a MonitorMode> {
    let want = d.mode.refresh_milli_hz;
    let same_size = m.modes.iter().filter(|md| {
        (md.width, md.height) == (d.mode.width, d.mode.height)
            || (md.width, md.height) == (d.mode.height, d.mode.width)
    });
    match d.mode.refresh_policy {
        RefreshPolicy::Exact => same_size
            .filter(|md| md.refresh_milli_hz().abs_diff(want) <= TOLERANCE_MILLI_HZ)
            .min_by_key(|md| md.refresh_milli_hz().abs_diff(want)),
        RefreshPolicy::BestAtOrBelow => same_size
            .filter(|md| md.refresh_milli_hz() <= want.saturating_add(TOLERANCE_MILLI_HZ))
            .max_by_key(|md| md.refresh_milli_hz()),
    }
}

/// The supported scale closest to `wanted` (Mutter accepts only listed scales).
fn snap_scale(mode: &MonitorMode, wanted: f64) -> f64 {
    mode.supported_scales
        .iter()
        .copied()
        .min_by(|a, b| (a - wanted).abs().total_cmp(&(b - wanted).abs()))
        .unwrap_or(mode.preferred_scale.max(1.0))
}

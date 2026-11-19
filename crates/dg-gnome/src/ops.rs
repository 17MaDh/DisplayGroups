//! Status, current group and capture: Mutter's state mapped onto the shared config
//! model, so the CLI and the settings app show the same things as on Windows.

use std::collections::HashSet;

use dg_engine::{ActiveDisplay, Captured, ConnectedMonitor, NewMonitor, PreferredMode, Status};
use dg_identity::{match_monitor, monitor_key, Observed};
use dg_model::{
    Config, DisplaySetting, Mode, MonitorEntry, OthersPolicy, Position, RefreshPolicy, Rotation,
    SCALE_STEPS,
};
use uuid::Uuid;

use crate::layout::{relayout, Rect};
use crate::observe::{display_name_of, identity_of, observe};
use crate::state::{is_portrait, DisplayState, LayoutMode, Monitor};
use crate::GnomeError;

/// The config key of each connected monitor (by index into `state.monitors`), where the
/// config knows it for sure. Every config entry is matched against all connected
/// monitors, strongest match first, so identical twins are told apart.
pub fn known_monitors(cfg: &Config, state: &DisplayState) -> Vec<Option<String>> {
    let observed: Vec<Observed> = state.monitors.iter().map(observe).collect();
    let mut hits: Vec<_> = cfg
        .monitors
        .iter()
        .filter_map(|(key, e)| match match_monitor(&e.identity, &observed) {
            Ok(m) if m.tier.is_certain() => Some((m.tier, m.index, key)),
            _ => None,
        })
        .collect();
    hits.sort_by_key(|(tier, ..)| *tier);
    let mut keys = vec![None; state.monitors.len()];
    for (_, index, key) in hits {
        if keys[index].is_none() {
            keys[index] = Some(key.clone());
        }
    }
    keys
}

fn index_of(state: &DisplayState, connector: &str) -> Option<usize> {
    state.monitors.iter().position(|m| m.connector == connector)
}

pub fn status(cfg: &Config, state: &DisplayState) -> Status {
    let known = known_monitors(cfg, state);
    let mut displays = Vec::new();
    for l in &state.logical_monitors {
        for c in &l.connectors {
            let Some(i) = index_of(state, c) else {
                continue;
            };
            let m = &state.monitors[i];
            let mode = m.current_mode();
            let (w, h) = mode.map_or((0, 0), |md| rotated(md.width, md.height, l.transform));
            displays.push(ActiveDisplay {
                name: display_name_of(m),
                monitor_key: known[i].clone(),
                width: w,
                height: h,
                x: l.x,
                y: l.y,
                refresh_milli_hz: mode.map(|md| md.refresh_milli_hz()),
                primary: l.primary,
            });
        }
    }
    let mut used: HashSet<String> = cfg.monitors.keys().cloned().collect();
    let connected = state
        .monitors
        .iter()
        .zip(known)
        .map(|(m, key)| {
            let new_monitor = key.is_none().then(|| {
                let fresh = new_monitor(m, &used);
                used.insert(fresh.key.clone());
                fresh
            });
            ConnectedMonitor {
                name: display_name_of(m),
                monitor_key: key,
                active: state.logical_for(&m.connector).is_some(),
                new_monitor,
                preferred: m.preferred_mode().map(|p| PreferredMode {
                    width: p.width,
                    height: p.height,
                    refresh_milli_hz: p.refresh_milli_hz(),
                }),
            }
        })
        .collect();
    Status {
        displays,
        connected,
    }
}

/// Which of `candidates` matches what is on screen: the same displays on (all others off
/// unless the group leaves them alone) and the same primary.
pub fn current_group(cfg: &Config, state: &DisplayState, candidates: &[Uuid]) -> Option<Uuid> {
    let observed: Vec<Observed> = state.monitors.iter().map(observe).collect();
    let on: HashSet<&str> = state
        .logical_monitors
        .iter()
        .flat_map(|l| l.connectors.iter().map(String::as_str))
        .collect();
    let primary: Option<&str> = state
        .logical_monitors
        .iter()
        .find(|l| l.primary)
        .and_then(|l| l.connectors.first().map(String::as_str));
    candidates.iter().copied().find(|id| {
        let Some(group) = cfg.group_by_id(*id) else {
            return false;
        };
        let mut wanted = HashSet::new();
        let mut group_primary = None;
        for d in group.enabled_displays() {
            let Some(entry) = cfg.monitors.get(&d.monitor) else {
                return false;
            };
            let Ok(m) = match_monitor(&entry.identity, &observed) else {
                return false;
            };
            let connector = state.monitors[m.index].connector.as_str();
            wanted.insert(connector);
            if d.position.x == 0 && d.position.y == 0 {
                group_primary = Some(connector);
            }
        }
        let same_set = match group.others_policy {
            OthersPolicy::Disable => wanted == on,
            OthersPolicy::Leave => wanted.is_subset(&on),
        };
        same_set && !wanted.is_empty() && group_primary == primary
    })
}

/// The layout on screen, ready to be saved as a group. Positions are converted to the
/// config's physical-pixel layout (as on Windows), the primary at (0,0).
pub fn capture(cfg: &Config, state: &DisplayState) -> Result<Captured, GnomeError> {
    let mut used: HashSet<String> = cfg.monitors.keys().cloned().collect();
    let mut new_monitors = Vec::new();
    let mut displays = Vec::new();
    let mut src = Vec::new();
    let mut sizes = Vec::new();
    let mut units = Vec::new();
    let mut primary = 0;
    let known = known_monitors(cfg, state);

    for l in &state.logical_monitors {
        for c in &l.connectors {
            let Some(i) = index_of(state, c) else {
                continue;
            };
            let m = &state.monitors[i];
            let Some(mode) = m.current_mode() else {
                continue;
            };
            let key = match known[i].clone() {
                Some(k) => k,
                None => {
                    let fresh = new_monitor(m, &used);
                    new_monitors.push((fresh.key.clone(), fresh.entry));
                    fresh.key
                }
            };
            used.insert(key.clone());
            let (pw, ph) = rotated(mode.width, mode.height, l.transform);
            let (lw, lh) = layout_size(state.layout_mode, pw, ph, l.scale);
            if l.primary {
                primary = displays.len();
            }
            src.push(Rect {
                x: l.x,
                y: l.y,
                w: lw,
                h: lh,
            });
            sizes.push((pw as i32, ph as i32));
            units.push(match state.layout_mode {
                LayoutMode::Logical => l.scale,
                LayoutMode::Physical => 1.0,
            });
            displays.push(DisplaySetting {
                monitor: key,
                enabled: true,
                mode: Mode {
                    width: mode.width,
                    height: mode.height,
                    refresh_milli_hz: mode.refresh_milli_hz(),
                    refresh_policy: RefreshPolicy::BestAtOrBelow,
                },
                position: Position { x: 0, y: 0 },
                rotation: rotation_of(l.transform),
                scale_percent: Some(scale_step(l.scale)),
                hdr: None,
                captured: None,
                extra: Default::default(),
            });
        }
    }
    if displays.is_empty() {
        return Err(GnomeError::NothingToShow);
    }
    for (d, r) in displays
        .iter_mut()
        .zip(relayout(&src, &sizes, &units, primary))
    {
        d.position = Position { x: r.x, y: r.y };
    }
    Ok(Captured {
        new_monitors,
        displays,
    })
}

/// The config entry for a monitor seen for the first time, under a key not in `used`.
pub fn new_monitor(m: &Monitor, used: &HashSet<String>) -> NewMonitor {
    let identity = identity_of(m);
    let base = monitor_key(
        &identity.pnp_id,
        &identity.product_code,
        identity.serial.as_deref(),
        identity.connector_hint.as_deref(),
    );
    let key = if used.contains(&base) {
        (2..)
            .map(|i| format!("{base}-{i}"))
            .find(|k| !used.contains(k))
            .expect("an unused key always exists")
    } else {
        base
    };
    NewMonitor {
        key,
        entry: MonitorEntry {
            display_name: display_name_of(m),
            identity,
            extra: Default::default(),
        },
    }
}

/// Mode size as shown on screen (portrait swaps width and height).
pub fn rotated(width: u32, height: u32, transform: u32) -> (u32, u32) {
    if is_portrait(transform) {
        (height, width)
    } else {
        (width, height)
    }
}

/// A display's size in Mutter's layout.
pub fn layout_size(mode: LayoutMode, w: u32, h: u32, scale: f64) -> (i32, i32) {
    match mode {
        LayoutMode::Logical => (
            (f64::from(w) / scale).round() as i32,
            (f64::from(h) / scale).round() as i32,
        ),
        LayoutMode::Physical => (w as i32, h as i32),
    }
}

pub fn rotation_of(transform: u32) -> Rotation {
    match transform % 4 {
        1 => Rotation::Portrait,
        2 => Rotation::LandscapeFlipped,
        3 => Rotation::PortraitFlipped,
        _ => Rotation::Landscape,
    }
}

pub fn transform_of(rotation: Rotation) -> u32 {
    match rotation {
        Rotation::Landscape => 0,
        Rotation::Portrait => 1,
        Rotation::LandscapeFlipped => 2,
        Rotation::PortraitFlipped => 3,
    }
}

/// The config's scale steps are Windows' (100, 125, 150 …); GNOME's fractional scales
/// are rounded to the nearest one.
fn scale_step(scale: f64) -> u32 {
    let percent = scale * 100.0;
    SCALE_STEPS
        .iter()
        .copied()
        .min_by(|a, b| {
            (f64::from(*a) - percent)
                .abs()
                .total_cmp(&(f64::from(*b) - percent).abs())
        })
        .unwrap_or(100)
}

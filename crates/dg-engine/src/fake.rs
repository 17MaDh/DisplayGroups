//! In-memory backends that behave like Windows did on the test machine
//! (ARCHITECTURE.md §6, items marked [VERIFIED]):
//!
//! * rotation or scaling 0, or `ALLOW_PATH_ORDER_CHANGES` with
//!   `USE_SUPPLIED_DISPLAY_CONFIG`, returns 87;
//! * `TOPOLOGY_SUPPLIED` without `ALLOW_PATH_ORDER_CHANGES` returns 31;
//! * a display that turns on gets its scale reset to "Recommended";
//! * without supplied modes Windows picks a default mode (60 Hz on the TV).
//!
//! Tests script other answers (e.g. "Windows forgot the set") with [`FakeBackend::respond`].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use dg_model::{
    AudioSettings, Config, DisplaySetting, Group, Mode, MonitorEntry, MonitorIdentity, Position,
    RefreshPolicy, Rotation,
};
use uuid::Uuid;

use crate::backend::{
    AudioBackend, AudioEndpoint, DeviceMode, DisplayBackend, HdrState, ModeChange,
};
use crate::ccd::{
    output_technology, rotation, scaling, sdc, win32, Luid, ModeInfo, PathInfo, PathSource,
    PathTarget, Rational, Scope, TargetDetails, TargetRef, Topology, VideoSignal, MODE_IDX_INVALID,
    PATH_ACTIVE,
};
use crate::dpi::DpiInfo;
use crate::EngineError;

/// A recorded `SetDisplayConfig` call.
#[derive(Debug, Clone)]
pub struct Call {
    pub flags: u32,
    pub paths: Vec<PathInfo>,
    pub modes: Vec<ModeInfo>,
}

impl Call {
    pub fn uses(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
    pub fn is_validate(&self) -> bool {
        self.uses(sdc::VALIDATE)
    }
    pub fn is_apply(&self) -> bool {
        self.uses(sdc::APPLY)
    }
}

pub type Responder = Box<dyn FnMut(&Call) -> Option<i32>>;

#[derive(Debug, Clone)]
pub struct FakeTarget {
    pub details: TargetDetails,
    /// What Windows picks when no mode is supplied: (width, height, hz).
    pub default_mode: (u32, u32, u32),
    /// Offered modes: (width, height, hz).
    pub modes: Vec<(u32, u32, u32)>,
    pub dpi: Option<DpiInfo>,
    pub hdr: HdrState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeActive {
    pub target: TargetRef,
    pub source: (Luid, u32),
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub hz: u32,
    pub rotation: u32,
}

#[derive(Debug, Default)]
pub struct FakeState {
    pub sources_per_adapter: u32,
    pub targets: Vec<FakeTarget>,
    pub active: Vec<FakeActive>,
    pub calls: Vec<Call>,
    pub mode_changes: Vec<ModeChange>,
    pub dpi_sets: Vec<(TargetRef, i32)>,
    pub hdr_sets: Vec<(TargetRef, bool)>,
}

pub struct FakeBackend {
    pub state: RefCell<FakeState>,
    responder: RefCell<Option<Responder>>,
}

impl FakeBackend {
    pub fn new(state: FakeState) -> Self {
        Self {
            state: RefCell::new(state),
            responder: RefCell::new(None),
        }
    }

    /// Overrides the answer to `SetDisplayConfig` calls: return `Some(code)` to answer,
    /// `None` for the default behaviour.
    pub fn respond(&self, f: impl FnMut(&Call) -> Option<i32> + 'static) {
        *self.responder.borrow_mut() = Some(Box::new(f));
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state.borrow_mut().calls.clear();
    }

    /// Friendly names of the active displays, sorted.
    pub fn active_names(&self) -> Vec<String> {
        let s = self.state.borrow();
        let mut names: Vec<String> = s
            .active
            .iter()
            .filter_map(|a| s.targets.iter().find(|t| t.details.target == a.target))
            .map(|t| t.details.friendly_name.clone())
            .collect();
        names.sort();
        names
    }

    pub fn active_entry(&self, target: TargetRef) -> Option<FakeActive> {
        self.state
            .borrow()
            .active
            .iter()
            .find(|a| a.target == target)
            .cloned()
    }

    pub fn dpi_of(&self, target: TargetRef) -> Option<DpiInfo> {
        self.target(target).and_then(|t| t.dpi)
    }

    pub fn set_available(&self, target: TargetRef, available: bool) {
        let mut s = self.state.borrow_mut();
        if let Some(t) = s.targets.iter_mut().find(|t| t.details.target == target) {
            t.details.available = available;
        }
    }

    /// Simulates a reboot: the adapter gets a new LUID.
    pub fn change_adapter_luid(&self, new: Luid) {
        let mut s = self.state.borrow_mut();
        for t in &mut s.targets {
            t.details.target.adapter = new;
        }
        for a in &mut s.active {
            a.target.adapter = new;
            a.source.0 = new;
        }
    }

    fn target(&self, target: TargetRef) -> Option<FakeTarget> {
        self.state
            .borrow()
            .targets
            .iter()
            .find(|t| t.details.target == target)
            .cloned()
    }

    fn topology(&self, scope: Scope) -> Topology {
        let s = self.state.borrow();
        let mut paths = Vec::new();
        let mut modes = Vec::new();
        for a in &s.active {
            let tech = s
                .targets
                .iter()
                .find(|t| t.details.target == a.target)
                .map(|t| t.details.output_technology)
                .unwrap_or_default();
            let source_idx = index(modes.len());
            modes.push(ModeInfo::Source {
                adapter: a.source.0,
                id: a.source.1,
                width: a.width,
                height: a.height,
                pixel_format: 4,
                x: a.x,
                y: a.y,
            });
            let target_idx = index(modes.len());
            modes.push(ModeInfo::Target {
                adapter: a.target.adapter,
                id: a.target.id,
                signal: signal_for(a),
            });
            paths.push(PathInfo {
                source: PathSource {
                    adapter: a.source.0,
                    id: a.source.1,
                    mode_idx: source_idx,
                    status_flags: 1,
                },
                target: PathTarget {
                    adapter: a.target.adapter,
                    id: a.target.id,
                    mode_idx: target_idx,
                    output_technology: tech,
                    rotation: a.rotation,
                    scaling: scaling::IDENTITY,
                    refresh: Rational {
                        numerator: a.hz * 1000,
                        denominator: 1000,
                    },
                    scanline_ordering: 1,
                    available: true,
                    status_flags: 1,
                },
                flags: PATH_ACTIVE,
            });
        }
        if scope == Scope::All {
            for t in &s.targets {
                let tr = t.details.target;
                for src in 0..s.sources_per_adapter {
                    if s.active
                        .iter()
                        .any(|a| a.target == tr && a.source == (tr.adapter, src))
                    {
                        continue;
                    }
                    // Inactive paths come back with zeroed enums, like real QDC_ALL_PATHS.
                    paths.push(PathInfo {
                        source: PathSource {
                            adapter: tr.adapter,
                            id: src,
                            mode_idx: MODE_IDX_INVALID,
                            status_flags: 0,
                        },
                        target: PathTarget {
                            adapter: tr.adapter,
                            id: tr.id,
                            mode_idx: MODE_IDX_INVALID,
                            output_technology: t.details.output_technology,
                            rotation: 0,
                            scaling: 0,
                            refresh: Rational::ZERO,
                            scanline_ordering: 0,
                            available: t.details.available,
                            status_flags: 0,
                        },
                        flags: 0,
                    });
                }
            }
        }
        let targets = s
            .targets
            .iter()
            .filter(|t| {
                scope == Scope::All || s.active.iter().any(|a| a.target == t.details.target)
            })
            .map(|t| t.details.clone())
            .collect();
        Topology {
            paths,
            modes,
            targets,
        }
    }

    /// Answers like Windows does to arrays it cannot accept.
    fn strict_check(&self, call: &Call) -> Option<i32> {
        let s = self.state.borrow();
        if call.uses(sdc::USE_SUPPLIED_DISPLAY_CONFIG) && call.uses(sdc::ALLOW_PATH_ORDER_CHANGES) {
            return Some(win32::INVALID_PARAMETER);
        }
        if call.uses(sdc::USE_SUPPLIED_DISPLAY_CONFIG) && call.uses(sdc::TOPOLOGY_SUPPLIED) {
            return Some(win32::INVALID_PARAMETER);
        }
        if call.uses(sdc::TOPOLOGY_SUPPLIED) && !call.uses(sdc::ALLOW_PATH_ORDER_CHANGES) {
            return Some(win32::GEN_FAILURE);
        }
        let builtin = sdc::TOPOLOGY_EXTEND
            | sdc::TOPOLOGY_CLONE
            | sdc::TOPOLOGY_INTERNAL
            | sdc::TOPOLOGY_EXTERNAL;
        if call.paths.is_empty() && call.flags & builtin == 0 {
            return Some(win32::INVALID_PARAMETER);
        }
        let mut sources = std::collections::HashSet::new();
        for p in &call.paths {
            if !(rotation::IDENTITY..=rotation::ROTATE270).contains(&p.target.rotation)
                || !scaling::VALID.contains(&p.target.scaling)
                || p.source.id >= s.sources_per_adapter
                || !sources.insert((p.source.adapter, p.source.id))
            {
                return Some(win32::INVALID_PARAMETER);
            }
            for idx in [p.source.mode_idx, p.target.mode_idx] {
                if idx != MODE_IDX_INVALID && idx as usize >= call.modes.len() {
                    return Some(win32::INVALID_PARAMETER);
                }
            }
            match s
                .targets
                .iter()
                .find(|t| t.details.target == p.target_ref())
            {
                Some(t) if t.details.available => {}
                _ => return Some(win32::GEN_FAILURE),
            }
        }
        None
    }

    fn apply_call(&self, call: &Call) {
        let mut guard = self.state.borrow_mut();
        let FakeState {
            targets, active, ..
        } = &mut *guard;
        let previous = active.clone();

        let mut next = Vec::new();
        if call.paths.is_empty() {
            // SDC_TOPOLOGY_EXTEND: every available display, side by side.
            let mut x = 0;
            for (i, t) in targets.iter().filter(|t| t.details.available).enumerate() {
                let (w, h, hz) = t.default_mode;
                next.push(FakeActive {
                    target: t.details.target,
                    source: (t.details.target.adapter, index(i)),
                    width: w,
                    height: h,
                    x,
                    y: 0,
                    hz,
                    rotation: rotation::IDENTITY,
                });
                x += i32::try_from(w).unwrap_or(0);
            }
        } else {
            let mut cursor = 0i32;
            for p in &call.paths {
                let t = targets
                    .iter()
                    .find(|t| t.details.target == p.target_ref())
                    .expect("checked by strict_check");
                let (mut w, mut h, mut hz) = t.default_mode;
                if matches!(p.target.rotation, rotation::ROTATE90 | rotation::ROTATE270) {
                    std::mem::swap(&mut w, &mut h);
                }
                let (mut x, mut y) = (cursor, 0);
                match call.modes.get(p.source.mode_idx as usize) {
                    Some(ModeInfo::Source {
                        width,
                        height,
                        x: sx,
                        y: sy,
                        ..
                    }) => {
                        (w, h, x, y) = (*width, *height, *sx, *sy);
                    }
                    _ => cursor += i32::try_from(w).unwrap_or(0),
                }
                if let Some(ModeInfo::Target { signal, .. }) =
                    call.modes.get(p.target.mode_idx as usize)
                {
                    hz = signal.v_sync.milli_hz().map_or(hz, |m| (m + 500) / 1000);
                }
                next.push(FakeActive {
                    target: p.target_ref(),
                    source: (p.source.adapter, p.source.id),
                    width: w,
                    height: h,
                    x,
                    y,
                    hz,
                    rotation: p.target.rotation,
                });
            }
            // Windows keeps the primary at (0,0).
            if !next.iter().any(|a| a.x == 0 && a.y == 0) {
                let (dx, dy) = (next[0].x, next[0].y);
                for a in &mut next {
                    a.x -= dx;
                    a.y -= dy;
                }
            }
        }

        // A display that turns on comes back at the recommended scale.
        for a in &next {
            if !previous.iter().any(|p| p.target == a.target) {
                if let Some(d) = targets
                    .iter_mut()
                    .find(|t| t.details.target == a.target)
                    .and_then(|t| t.dpi.as_mut())
                {
                    d.cur_rel = 0;
                }
            }
        }
        *active = next;
    }

    fn not_active(op: &'static str) -> EngineError {
        EngineError::Win32 { op, code: 1 }
    }
}

impl DisplayBackend for FakeBackend {
    fn query(&self, scope: Scope) -> Result<Topology, EngineError> {
        Ok(self.topology(scope))
    }

    fn set_config(&self, paths: &[PathInfo], modes: &[ModeInfo], flags: u32) -> i32 {
        let call = Call {
            flags,
            paths: paths.to_vec(),
            modes: modes.to_vec(),
        };
        self.state.borrow_mut().calls.push(call.clone());
        if let Some(code) = self.strict_check(&call) {
            return code;
        }
        if let Some(responder) = self.responder.borrow_mut().as_mut() {
            if let Some(code) = responder(&call) {
                if code != 0 {
                    return code;
                }
            }
        }
        if call.is_apply() {
            self.apply_call(&call);
        }
        0
    }

    fn list_modes(&self, target: TargetRef) -> Result<Vec<DeviceMode>, EngineError> {
        if self.active_entry(target).is_none() {
            return Err(Self::not_active("EnumDisplaySettingsExW"));
        }
        let t = self
            .target(target)
            .ok_or_else(|| Self::not_active("EnumDisplaySettingsExW"))?;
        Ok(t.modes
            .iter()
            .map(|&(width, height, refresh_hz)| DeviceMode {
                width,
                height,
                refresh_hz,
                x: 0,
                y: 0,
                orientation: 0,
            })
            .collect())
    }

    fn current_mode(&self, target: TargetRef) -> Result<DeviceMode, EngineError> {
        let a = self
            .active_entry(target)
            .ok_or_else(|| Self::not_active("EnumDisplaySettingsExW"))?;
        Ok(DeviceMode {
            width: a.width,
            height: a.height,
            refresh_hz: a.hz,
            x: a.x,
            y: a.y,
            orientation: a.rotation.saturating_sub(1),
        })
    }

    fn apply_modes(&self, changes: &[ModeChange]) -> Result<Vec<i32>, EngineError> {
        let mut s = self.state.borrow_mut();
        let mut codes = Vec::new();
        for c in changes {
            s.mode_changes.push(*c);
            let offered = s
                .targets
                .iter()
                .find(|t| t.details.target == c.target)
                .is_some_and(|t| {
                    t.modes.iter().any(|&(w, h, hz)| {
                        hz == c.refresh_hz
                            && ((w, h) == (c.width, c.height) || (h, w) == (c.width, c.height))
                    })
                });
            match s.active.iter_mut().find(|a| a.target == c.target) {
                Some(a) if offered => {
                    (a.width, a.height, a.hz, a.x, a.y) =
                        (c.width, c.height, c.refresh_hz, c.x, c.y);
                    if let Some(o) = c.orientation {
                        a.rotation = o + 1;
                    }
                    codes.push(0);
                }
                _ => codes.push(-2), // DISP_CHANGE_BADMODE
            }
        }
        Ok(codes)
    }

    fn get_dpi(&self, target: TargetRef) -> Result<DpiInfo, EngineError> {
        if self.active_entry(target).is_none() {
            return Err(Self::not_active("DisplayConfigGetDeviceInfo(DPI)"));
        }
        self.dpi_of(target)
            .ok_or_else(|| EngineError::Unsupported("no DPI data".into()))
    }

    fn set_dpi(&self, target: TargetRef, relative: i32) -> Result<(), EngineError> {
        let mut s = self.state.borrow_mut();
        s.dpi_sets.push((target, relative));
        let d = s
            .targets
            .iter_mut()
            .find(|t| t.details.target == target)
            .and_then(|t| t.dpi.as_mut())
            .ok_or_else(|| EngineError::Unsupported("no DPI data".into()))?;
        if relative < d.min_rel || relative > d.max_rel {
            return Err(EngineError::Win32 {
                op: "DisplayConfigSetDeviceInfo(DPI)",
                code: win32::INVALID_PARAMETER,
            });
        }
        d.cur_rel = relative;
        Ok(())
    }

    fn get_hdr(&self, target: TargetRef) -> Result<HdrState, EngineError> {
        self.target(target)
            .map(|t| t.hdr)
            .ok_or_else(|| Self::not_active("DisplayConfigGetDeviceInfo(HDR)"))
    }

    fn set_hdr(&self, target: TargetRef, enabled: bool) -> Result<(), EngineError> {
        let mut s = self.state.borrow_mut();
        s.hdr_sets.push((target, enabled));
        if let Some(t) = s.targets.iter_mut().find(|t| t.details.target == target) {
            t.hdr.enabled = enabled;
        }
        Ok(())
    }

    fn sleep_ms(&self, _ms: u32) {}
}

fn index(i: usize) -> u32 {
    u32::try_from(i).expect("small test topologies")
}

fn signal_for(a: &FakeActive) -> VideoSignal {
    let (w, h) = if matches!(a.rotation, rotation::ROTATE90 | rotation::ROTATE270) {
        (a.height, a.width)
    } else {
        (a.width, a.height)
    };
    let (tw, th) = (w + 160, h + 45);
    VideoSignal {
        pixel_rate: u64::from(tw) * u64::from(th) * u64::from(a.hz),
        h_sync: Rational {
            numerator: th * a.hz,
            denominator: 1,
        },
        v_sync: Rational {
            numerator: a.hz * 1000,
            denominator: 1000,
        },
        active_width: w,
        active_height: h,
        total_width: tw,
        total_height: th,
        video_standard: 255,
        scanline_ordering: 1,
    }
}

/// Audio endpoints; `pending` ones appear after the given number of polls (like the TV's
/// HDMI audio, which only shows up once the TV is on).
#[derive(Default)]
pub struct FakeAudio {
    pub endpoints: RefCell<Vec<AudioEndpoint>>,
    pub pending: RefCell<Vec<(AudioEndpoint, u32)>>,
    pub default: RefCell<Option<String>>,
    pub volumes: RefCell<HashMap<String, u8>>,
}

impl AudioBackend for FakeAudio {
    fn render_endpoints(&self) -> Result<Vec<AudioEndpoint>, EngineError> {
        let mut pending = self.pending.borrow_mut();
        let mut endpoints = self.endpoints.borrow_mut();
        pending.retain_mut(|(e, polls)| {
            if *polls == 0 {
                endpoints.push(e.clone());
                false
            } else {
                *polls -= 1;
                true
            }
        });
        Ok(endpoints.clone())
    }

    fn default_endpoint(&self) -> Result<Option<String>, EngineError> {
        Ok(self.default.borrow().clone())
    }

    fn set_default_endpoint(&self, id: &str) -> Result<(), EngineError> {
        *self.default.borrow_mut() = Some(id.to_string());
        Ok(())
    }

    fn set_volume(&self, id: &str, percent: u8) -> Result<(), EngineError> {
        self.volumes.borrow_mut().insert(id.to_string(), percent);
        Ok(())
    }
}

// ----- the test machine -----

pub const ADAPTER: Luid = Luid {
    low: 85173,
    high: 0,
};
pub const TV: TargetRef = TargetRef {
    adapter: ADAPTER,
    id: 4352,
};
pub const DFI: TargetRef = TargetRef {
    adapter: ADAPTER,
    id: 4353,
};
pub const ASUS: TargetRef = TargetRef {
    adapter: ADAPTER,
    id: 4355,
};
pub const DESKTOP_GROUP: Uuid = Uuid::from_u128(0x5d3f_0e2a_7c41_4b8e_9a10_0000_0000_d35c);
pub const COUCH_GROUP: Uuid = Uuid::from_u128(0x5d3f_0e2a_7c41_4b8e_9a10_0000_0000_c0c4);
pub const TV_KEY: &str = "mon-sam-721b-uid4352";
pub const ASUS_KEY: &str = "mon-aus-aa06-uid4355";
pub const DFI_KEY: &str = "mon-hxm-40f3-uid4353";

fn details(
    target: TargetRef,
    name: &str,
    mfg: u16,
    product: u16,
    hw: &str,
    tech: u32,
) -> TargetDetails {
    TargetDetails {
        target,
        available: true,
        friendly_name: name.into(),
        edid_manufacture_id: Some(mfg),
        edid_product_code: Some(product),
        connector_instance: 1,
        device_path: format!(
            r"\\?\DISPLAY#{hw}#5&2031f481&0&UID{}#{{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}}",
            target.id
        ),
        output_technology: tech,
        edid: None,
    }
}

/// ASUS XG32UCWMG (primary, 4K 240 Hz) + DFI27280 on its left; Samsung TV connected but off.
/// Ids, LUID and positions are the values read from the real machine.
pub fn rig() -> FakeBackend {
    let tv_modes = [24, 30, 50, 60, 100, 119, 120]
        .iter()
        .map(|&hz| (3840, 2160, hz))
        .chain([(1920, 1080, 60), (1920, 1080, 120)])
        .collect();
    FakeBackend::new(FakeState {
        sources_per_adapter: 4,
        targets: vec![
            FakeTarget {
                details: details(
                    TV,
                    "SAMSUNG",
                    11596,
                    29211,
                    "SAM721B",
                    output_technology::HDMI,
                ),
                default_mode: (3840, 2160, 60),
                modes: tv_modes,
                dpi: Some(DpiInfo {
                    min_rel: -7,
                    cur_rel: -4,
                    max_rel: 2,
                }),
                hdr: HdrState {
                    supported: true,
                    enabled: false,
                },
            },
            FakeTarget {
                details: details(
                    DFI,
                    "DFI27280",
                    3363,
                    16627,
                    "HXM40F3",
                    output_technology::DISPLAYPORT_EXTERNAL,
                ),
                default_mode: (1920, 1080, 60),
                modes: vec![(1920, 1080, 60), (1920, 1080, 75)],
                dpi: Some(DpiInfo {
                    min_rel: 0,
                    cur_rel: 0,
                    max_rel: 1,
                }),
                hdr: HdrState::default(),
            },
            FakeTarget {
                details: details(
                    ASUS,
                    "XG32UCWMG",
                    45830,
                    43526,
                    "AUSAA06",
                    output_technology::DISPLAYPORT_EXTERNAL,
                ),
                default_mode: (3840, 2160, 60),
                modes: vec![
                    (3840, 2160, 60),
                    (3840, 2160, 120),
                    (3840, 2160, 144),
                    (3840, 2160, 240),
                    (2560, 1440, 240),
                ],
                dpi: Some(DpiInfo {
                    min_rel: -2,
                    cur_rel: 0,
                    max_rel: 5,
                }),
                hdr: HdrState {
                    supported: true,
                    enabled: true,
                },
            },
        ],
        active: vec![
            FakeActive {
                target: DFI,
                source: (ADAPTER, 0),
                width: 1920,
                height: 1080,
                x: -1920,
                y: 457,
                hz: 60,
                rotation: 1,
            },
            FakeActive {
                target: ASUS,
                source: (ADAPTER, 1),
                width: 3840,
                height: 2160,
                x: 0,
                y: 0,
                hz: 240,
                rotation: 1,
            },
        ],
        ..Default::default()
    })
}

fn monitor(
    name: &str,
    pnp: &str,
    product: &str,
    friendly: &str,
    conn: &str,
    tech: &str,
) -> MonitorEntry {
    MonitorEntry {
        display_name: name.into(),
        identity: MonitorIdentity {
            pnp_id: pnp.into(),
            product_code: product.into(),
            serial: None,
            friendly_name: friendly.into(),
            connector_hint: Some(conn.into()),
            adapter_hint: None,
            output_technology: Some(tech.into()),
            extra: Default::default(),
        },
        extra: Default::default(),
    }
}

fn display(key: &str, w: u32, h: u32, hz: u32, x: i32, y: i32, scale: u32) -> DisplaySetting {
    DisplaySetting {
        monitor: key.into(),
        enabled: true,
        mode: Mode {
            width: w,
            height: h,
            refresh_milli_hz: hz * 1000,
            refresh_policy: RefreshPolicy::BestAtOrBelow,
        },
        position: Position { x, y },
        rotation: Rotation::Landscape,
        scale_percent: Some(scale),
        hdr: None,
        captured: None,
        extra: Default::default(),
    }
}

/// "Desktop" and "Gaming on Couch" for [`rig`], as the user set them up.
pub fn rig_config() -> Config {
    let mut monitors = BTreeMap::new();
    monitors.insert(
        TV_KEY.into(),
        monitor(
            "Living-room TV",
            "SAM",
            "721B",
            "SAMSUNG",
            "UID4352",
            "hdmi",
        ),
    );
    monitors.insert(
        ASUS_KEY.into(),
        monitor(
            "ASUS XG32UCWMG",
            "AUS",
            "AA06",
            "XG32UCWMG",
            "UID4355",
            "displayport",
        ),
    );
    monitors.insert(
        DFI_KEY.into(),
        monitor(
            "Side monitor",
            "HXM",
            "40F3",
            "DFI27280",
            "UID4353",
            "displayport",
        ),
    );

    let mut desktop = Group::new("Desktop");
    desktop.id = DESKTOP_GROUP;
    desktop.displays = vec![
        display(ASUS_KEY, 3840, 2160, 240, 0, 0, 150),
        display(DFI_KEY, 1920, 1080, 60, -1920, 457, 100),
    ];
    let mut couch = Group::new("Gaming on Couch");
    couch.id = COUCH_GROUP;
    couch.icon = "builtin:couch".into();
    couch.displays = vec![display(TV_KEY, 3840, 2160, 120, 0, 0, 175)];
    couch.audio = AudioSettings::default();

    Config {
        monitors,
        groups: vec![desktop, couch],
        ..Config::default()
    }
}

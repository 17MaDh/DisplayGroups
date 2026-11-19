//! The seam between the engine and the operating system.

use serde::{Deserialize, Serialize};

use crate::ccd::{ModeInfo, PathInfo, Scope, TargetRef, Topology};
use crate::dpi::DpiInfo;
use crate::EngineError;

/// A mode as GDI reports it (`EnumDisplaySettingsExW`): desktop-oriented size,
/// whole-hertz refresh, position and `DMDO_*` orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub x: i32,
    pub y: i32,
    pub orientation: u32,
}

/// One per-display correction applied after the topology switch (`ChangeDisplaySettingsExW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeChange {
    pub target: TargetRef,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub x: i32,
    pub y: i32,
    /// `DMDO_*`; `None` leaves the orientation alone.
    pub orientation: Option<u32>,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HdrState {
    pub supported: bool,
    pub enabled: bool,
}

pub trait DisplayBackend {
    fn query(&self, scope: Scope) -> Result<Topology, EngineError>;

    /// Calls `SetDisplayConfig` and returns its raw Win32 result code.
    fn set_config(&self, paths: &[PathInfo], modes: &[ModeInfo], flags: u32) -> i32;

    /// Modes offered by an active target.
    fn list_modes(&self, target: TargetRef) -> Result<Vec<DeviceMode>, EngineError>;

    fn current_mode(&self, target: TargetRef) -> Result<DeviceMode, EngineError>;

    /// Applies all changes together; returns one Win32/`DISP_CHANGE` code per change.
    fn apply_modes(&self, changes: &[ModeChange]) -> Result<Vec<i32>, EngineError>;

    fn get_dpi(&self, target: TargetRef) -> Result<DpiInfo, EngineError>;
    fn set_dpi(&self, target: TargetRef, relative: i32) -> Result<(), EngineError>;

    fn get_hdr(&self, target: TargetRef) -> Result<HdrState, EngineError>;
    fn set_hdr(&self, target: TargetRef, enabled: bool) -> Result<(), EngineError>;

    /// Overridable so tests don't actually wait.
    fn sleep_ms(&self, ms: u32);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioEndpoint {
    pub id: String,
    pub name: String,
}

pub trait AudioBackend {
    /// Active playback endpoints.
    fn render_endpoints(&self) -> Result<Vec<AudioEndpoint>, EngineError>;
    fn default_endpoint(&self) -> Result<Option<String>, EngineError>;
    fn set_default_endpoint(&self, id: &str) -> Result<(), EngineError>;
    fn set_volume(&self, id: &str, percent: u8) -> Result<(), EngineError>;
}

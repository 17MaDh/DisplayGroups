//! Mutter's display state (`GetCurrentState`) as plain data.

use serde::{Deserialize, Serialize};

/// How Mutter positions logical monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutMode {
    /// Wayland's default: a monitor's size in the layout is its mode size divided by its
    /// scale, so a 3840×2160 monitor at 150% takes 2560×1440.
    Logical,
    /// X11: sizes and positions are physical pixels (as on Windows).
    Physical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayState {
    /// Passed back to `ApplyMonitorsConfig`, which refuses a stale serial.
    pub serial: u32,
    pub monitors: Vec<Monitor>,
    pub logical_monitors: Vec<LogicalMonitor>,
    pub layout_mode: LayoutMode,
    /// X11: every logical monitor must use the same scale.
    #[serde(default)]
    pub global_scale_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    /// e.g. `DP-1`, `HDMI-1`, `eDP-1`.
    pub connector: String,
    /// PnP id from the EDID, e.g. `SAM`.
    pub vendor: String,
    /// Product name from the EDID, or the product code when there is none.
    pub product: String,
    pub serial: String,
    /// What GNOME Settings calls it, e.g. "Samsung Electric Company 65\"".
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub builtin: bool,
    pub modes: Vec<MonitorMode>,
    /// Raw EDID from `/sys/class/drm`, when readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edid: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorMode {
    /// e.g. `3840x2160@119.880`; `ApplyMonitorsConfig` takes this id.
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub preferred_scale: f64,
    pub supported_scales: Vec<f64>,
    #[serde(default)]
    pub current: bool,
    #[serde(default)]
    pub preferred: bool,
}

impl MonitorMode {
    pub fn refresh_milli_hz(&self) -> u32 {
        (self.refresh * 1000.0).round() as u32
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalMonitor {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    /// 0 normal, 1–3 rotated by 90° steps, 4–7 the same flipped.
    pub transform: u32,
    pub primary: bool,
    /// Connectors shown in this logical monitor (more than one when mirrored).
    pub connectors: Vec<String>,
}

impl Monitor {
    pub fn current_mode(&self) -> Option<&MonitorMode> {
        self.modes.iter().find(|m| m.current)
    }

    pub fn preferred_mode(&self) -> Option<&MonitorMode> {
        self.modes
            .iter()
            .find(|m| m.preferred)
            .or_else(|| self.modes.first())
    }
}

impl DisplayState {
    /// The logical monitor that shows `connector`, if it is on.
    pub fn logical_for(&self, connector: &str) -> Option<&LogicalMonitor> {
        self.logical_monitors
            .iter()
            .find(|l| l.connectors.iter().any(|c| c == connector))
    }

    pub fn monitor(&self, connector: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.connector == connector)
    }
}

/// Whether a transform turns the monitor on its side.
pub fn is_portrait(transform: u32) -> bool {
    transform % 2 == 1
}

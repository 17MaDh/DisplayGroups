//! Engine-side mirror of the Windows CCD (Connecting and Configuring Displays) data
//! model. `dg-win` converts these to and from the real `DISPLAYCONFIG_*` structs.
//!
//! Everything is serializable so a layout can be stored as last-known-good.

use dg_model::{CapturedTiming, RationalValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Luid {
    pub low: u32,
    pub high: i32,
}

/// A display target (monitor connector) on an adapter. Volatile across reboots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetRef {
    pub adapter: Luid,
    pub id: u32,
}

pub const MODE_IDX_INVALID: u32 = u32::MAX;
pub const PATH_ACTIVE: u32 = 0x1;
pub const PIXELFORMAT_32BPP: u32 = 4;
pub const SCANLINE_UNSPECIFIED: u32 = 0;

/// `DISPLAYCONFIG_ROTATION`. Zero is not a valid value (it causes error 87).
pub mod rotation {
    pub const IDENTITY: u32 = 1;
    pub const ROTATE90: u32 = 2;
    pub const ROTATE180: u32 = 3;
    pub const ROTATE270: u32 = 4;
}

/// `DISPLAYCONFIG_SCALING`. Zero is not a valid value (it causes error 87).
pub mod scaling {
    pub const IDENTITY: u32 = 1;
    pub const CENTERED: u32 = 2;
    pub const STRETCHED: u32 = 3;
    pub const ASPECT_RATIO_CENTERED_MAX: u32 = 4;
    pub const CUSTOM: u32 = 5;
    pub const PREFERRED: u32 = 128;
    pub const VALID: [u32; 6] = [
        IDENTITY,
        CENTERED,
        STRETCHED,
        ASPECT_RATIO_CENTERED_MAX,
        CUSTOM,
        PREFERRED,
    ];
}

/// `SetDisplayConfig` flags.
pub mod sdc {
    pub const TOPOLOGY_INTERNAL: u32 = 0x0001;
    pub const TOPOLOGY_CLONE: u32 = 0x0002;
    pub const TOPOLOGY_EXTEND: u32 = 0x0004;
    pub const TOPOLOGY_EXTERNAL: u32 = 0x0008;
    pub const TOPOLOGY_SUPPLIED: u32 = 0x0010;
    pub const USE_SUPPLIED_DISPLAY_CONFIG: u32 = 0x0020;
    pub const VALIDATE: u32 = 0x0040;
    pub const APPLY: u32 = 0x0080;
    pub const NO_OPTIMIZATION: u32 = 0x0100;
    pub const SAVE_TO_DATABASE: u32 = 0x0200;
    pub const ALLOW_CHANGES: u32 = 0x0400;
    pub const PATH_PERSIST_IF_REQUIRED: u32 = 0x0800;
    pub const FORCE_MODE_ENUMERATION: u32 = 0x1000;
    pub const ALLOW_PATH_ORDER_CHANGES: u32 = 0x2000;
    pub const VIRTUAL_MODE_AWARE: u32 = 0x8000;
}

/// Win32 error codes that matter to the engine.
pub mod win32 {
    pub const SUCCESS: i32 = 0;
    pub const ACCESS_DENIED: i32 = 5;
    pub const GEN_FAILURE: i32 = 31;
    pub const NOT_SUPPORTED: i32 = 50;
    pub const INVALID_PARAMETER: i32 = 87;
    pub const DEVICE_NOT_CONNECTED: i32 = 1167;
    pub const BAD_CONFIGURATION: i32 = 1610;
}

/// `DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY` values we name in configs and logs.
pub mod output_technology {
    pub const HD15: u32 = 0;
    pub const DVI: u32 = 4;
    pub const HDMI: u32 = 5;
    pub const DISPLAYPORT_EXTERNAL: u32 = 10;
    pub const DISPLAYPORT_EMBEDDED: u32 = 11;
    pub const MIRACAST: u32 = 15;
    pub const INDIRECT_WIRED: u32 = 16;
    pub const INDIRECT_VIRTUAL: u32 = 17;
    pub const DISPLAYPORT_USB_TUNNEL: u32 = 18;
    pub const INTERNAL: u32 = 0x8000_0000;

    pub fn name(value: u32) -> &'static str {
        match value {
            HD15 => "vga",
            DVI => "dvi",
            HDMI => "hdmi",
            DISPLAYPORT_EXTERNAL => "displayport",
            DISPLAYPORT_EMBEDDED => "displayportEmbedded",
            MIRACAST => "miracast",
            INDIRECT_WIRED | INDIRECT_VIRTUAL => "indirect",
            DISPLAYPORT_USB_TUNNEL => "usbc",
            INTERNAL => "internal",
            _ => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
}

impl Rational {
    pub const ZERO: Rational = Rational {
        numerator: 0,
        denominator: 0,
    };

    /// The rate in thousandths of a hertz, rounded; `None` for 0/0 ("let Windows pick").
    pub fn milli_hz(self) -> Option<u32> {
        if self.numerator == 0 || self.denominator == 0 {
            return None;
        }
        let den = u64::from(self.denominator);
        let value = (u64::from(self.numerator) * 1000 + den / 2) / den;
        u32::try_from(value).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSource {
    pub adapter: Luid,
    pub id: u32,
    pub mode_idx: u32,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTarget {
    pub adapter: Luid,
    pub id: u32,
    pub mode_idx: u32,
    pub output_technology: u32,
    pub rotation: u32,
    pub scaling: u32,
    pub refresh: Rational,
    pub scanline_ordering: u32,
    pub available: bool,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathInfo {
    pub source: PathSource,
    pub target: PathTarget,
    pub flags: u32,
}

impl PathInfo {
    pub fn is_active(&self) -> bool {
        self.flags & PATH_ACTIVE != 0
    }

    pub fn target_ref(&self) -> TargetRef {
        TargetRef {
            adapter: self.target.adapter,
            id: self.target.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSignal {
    pub pixel_rate: u64,
    pub h_sync: Rational,
    pub v_sync: Rational,
    pub active_width: u32,
    pub active_height: u32,
    pub total_width: u32,
    pub total_height: u32,
    pub video_standard: u32,
    pub scanline_ordering: u32,
}

impl From<CapturedTiming> for VideoSignal {
    fn from(c: CapturedTiming) -> Self {
        let r = |v: RationalValue| Rational {
            numerator: v.numerator,
            denominator: v.denominator,
        };
        Self {
            pixel_rate: c.pixel_rate,
            h_sync: r(c.h_sync),
            v_sync: r(c.v_sync),
            active_width: c.active_width,
            active_height: c.active_height,
            total_width: c.total_width,
            total_height: c.total_height,
            video_standard: c.video_standard,
            scanline_ordering: c.scanline_ordering,
        }
    }
}

impl From<VideoSignal> for CapturedTiming {
    fn from(s: VideoSignal) -> Self {
        let r = |v: Rational| RationalValue {
            numerator: v.numerator,
            denominator: v.denominator,
        };
        Self {
            pixel_rate: s.pixel_rate,
            h_sync: r(s.h_sync),
            v_sync: r(s.v_sync),
            active_width: s.active_width,
            active_height: s.active_height,
            total_width: s.total_width,
            total_height: s.total_height,
            video_standard: s.video_standard,
            scanline_ordering: s.scanline_ordering,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModeInfo {
    Source {
        adapter: Luid,
        id: u32,
        width: u32,
        height: u32,
        pixel_format: u32,
        x: i32,
        y: i32,
    },
    Target {
        adapter: Luid,
        id: u32,
        signal: VideoSignal,
    },
    /// Desktop-image modes and anything newer; passed through untouched.
    Other {
        info_type: u32,
        adapter: Luid,
        id: u32,
        raw: Vec<u8>,
    },
}

impl ModeInfo {
    pub fn adapter(&self) -> Luid {
        match self {
            ModeInfo::Source { adapter, .. }
            | ModeInfo::Target { adapter, .. }
            | ModeInfo::Other { adapter, .. } => *adapter,
        }
    }

    pub fn id(&self) -> u32 {
        match self {
            ModeInfo::Source { id, .. }
            | ModeInfo::Target { id, .. }
            | ModeInfo::Other { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMode {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

/// Identification data for one target (`DISPLAYCONFIG_TARGET_DEVICE_NAME` + EDID).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDetails {
    pub target: TargetRef,
    pub available: bool,
    pub friendly_name: String,
    pub edid_manufacture_id: Option<u16>,
    pub edid_product_code: Option<u16>,
    pub connector_instance: u32,
    pub device_path: String,
    pub output_technology: u32,
    #[serde(default)]
    pub edid: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `QDC_ALL_PATHS`: every possible source/target combination.
    All,
    /// `QDC_ONLY_ACTIVE_PATHS`: what is on screen now.
    Active,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Topology {
    pub paths: Vec<PathInfo>,
    pub modes: Vec<ModeInfo>,
    pub targets: Vec<TargetDetails>,
}

impl Topology {
    pub fn target(&self, t: TargetRef) -> Option<&TargetDetails> {
        self.targets.iter().find(|d| d.target == t)
    }

    pub fn active_paths(&self) -> impl Iterator<Item = &PathInfo> {
        self.paths.iter().filter(|p| p.is_active())
    }

    pub fn active_path_for(&self, t: TargetRef) -> Option<&PathInfo> {
        self.active_paths().find(|p| p.target_ref() == t)
    }

    pub fn active_targets(&self) -> Vec<TargetRef> {
        self.active_paths().map(PathInfo::target_ref).collect()
    }

    pub fn source_mode(&self, path: &PathInfo) -> Option<SourceMode> {
        match self.modes.get(path.source.mode_idx as usize)? {
            ModeInfo::Source {
                width,
                height,
                x,
                y,
                ..
            } => Some(SourceMode {
                width: *width,
                height: *height,
                x: *x,
                y: *y,
            }),
            _ => None,
        }
    }

    pub fn target_mode(&self, path: &PathInfo) -> Option<VideoSignal> {
        match self.modes.get(path.target.mode_idx as usize)? {
            ModeInfo::Target { signal, .. } => Some(*signal),
            _ => None,
        }
    }

    /// The target whose source sits at desktop (0,0): Windows' primary display.
    pub fn primary_target(&self) -> Option<TargetRef> {
        self.active_paths()
            .find(|p| self.source_mode(p).is_some_and(|m| m.x == 0 && m.y == 0))
            .map(PathInfo::target_ref)
    }

    /// Refresh rate of an active path, from the path or its target mode.
    pub fn refresh_milli_hz(&self, path: &PathInfo) -> Option<u32> {
        path.target
            .refresh
            .milli_hz()
            .or_else(|| self.target_mode(path).and_then(|s| s.v_sync.milli_hz()))
    }
}

pub fn rotation_to_ccd(r: dg_model::Rotation) -> u32 {
    match r {
        dg_model::Rotation::Landscape => rotation::IDENTITY,
        dg_model::Rotation::Portrait => rotation::ROTATE90,
        dg_model::Rotation::LandscapeFlipped => rotation::ROTATE180,
        dg_model::Rotation::PortraitFlipped => rotation::ROTATE270,
    }
}

pub fn rotation_from_ccd(value: u32) -> dg_model::Rotation {
    match value {
        rotation::ROTATE90 => dg_model::Rotation::Portrait,
        rotation::ROTATE180 => dg_model::Rotation::LandscapeFlipped,
        rotation::ROTATE270 => dg_model::Rotation::PortraitFlipped,
        _ => dg_model::Rotation::Landscape,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_to_milli_hz() {
        let r = |n, d| Rational {
            numerator: n,
            denominator: d,
        };
        assert_eq!(r(240_016, 1000).milli_hz(), Some(240_016));
        assert_eq!(r(120_000_000, 1_001_000).milli_hz(), Some(119_880));
        assert_eq!(r(60, 1).milli_hz(), Some(60_000));
        assert_eq!(Rational::ZERO.milli_hz(), None);
    }
}

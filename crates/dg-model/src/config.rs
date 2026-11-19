//! The `config.json` schema (version 1). Field names are camelCase on disk.
//!
//! Every object keeps unknown fields in `extra`, so a file written by a newer
//! version survives a load/save round trip through an older one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::geometry::Rect;
use crate::ModelError;

/// Schema version written by this build.
pub const SCHEMA_VERSION: u32 = 1;

/// The scale percentages Windows offers, in order. DPI is stored by Windows as an
/// index relative to the recommended entry of this list.
pub const SCALE_STEPS: [u32; 12] = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

/// Upper bound for the "Keep these display settings?" countdown.
pub const MAX_CONFIRM_SECONDS: u32 = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub monitors: BTreeMap<String, MonitorEntry>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub hotkeys: Vec<Hotkey>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            monitors: BTreeMap::new(),
            groups: Vec::new(),
            hotkeys: Vec::new(),
            settings: Settings::default(),
            extra: Map::new(),
        }
    }
}

impl Config {
    /// Finds a group by UUID or by name (case-insensitive, surrounding spaces ignored).
    pub fn find_group(&self, key: &str) -> Option<&Group> {
        let key = key.trim();
        if let Ok(id) = Uuid::parse_str(key) {
            if let Some(g) = self.groups.iter().find(|g| g.id == id) {
                return Some(g);
            }
        }
        self.groups.iter().find(|g| names_equal(&g.name, key))
    }

    pub fn require_group(&self, key: &str) -> Result<&Group, ModelError> {
        self.find_group(key)
            .ok_or_else(|| ModelError::GroupNotFound(key.to_string()))
    }

    pub fn group_by_id(&self, id: Uuid) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_by_id_mut(&mut self, id: Uuid) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The number a monitor shows in "Identify" and in the layout editor: its position
    /// among the config's monitors (ordered by key), from 1. The same in every group.
    pub fn monitor_number(&self, key: &str) -> Option<u32> {
        self.monitors
            .keys()
            .position(|k| k == key)
            .map(|i| u32::try_from(i + 1).unwrap_or(u32::MAX))
    }

    /// Returns `base` if no group uses that name yet, otherwise `base 2`, `base 3`, ...
    pub fn unique_group_name(&self, base: &str) -> String {
        let taken = |n: &str| self.groups.iter().any(|g| names_equal(&g.name, n));
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .expect("an unused name always exists")
    }

    /// Deep-copies a group under a new id, named "<name> (copy)", inserted right after
    /// the original. Hotkeys and shortcuts that point at the original are not copied.
    pub fn duplicate_group(&mut self, id: Uuid) -> Result<Uuid, ModelError> {
        let index = self
            .groups
            .iter()
            .position(|g| g.id == id)
            .ok_or_else(|| ModelError::GroupNotFound(id.to_string()))?;
        let mut copy = self.groups[index].clone();
        copy.id = Uuid::new_v4();
        copy.name = self.unique_group_name(&format!("{} (copy)", copy.name));
        let new_id = copy.id;
        self.groups.insert(index + 1, copy);
        Ok(new_id)
    }
}

/// Group names compare case-insensitively (Unicode-aware) with surrounding spaces ignored.
pub fn names_equal(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorEntry {
    pub display_name: String,
    pub identity: MonitorIdentity,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What we remember about a physical monitor so it can be found again after reboots,
/// driver updates and cable moves. See ARCHITECTURE.md §4.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorIdentity {
    /// Three-letter PnP manufacturer id from the EDID, e.g. `SAM`.
    pub pnp_id: String,
    /// EDID product code as four upper-case hex digits, e.g. `721B`.
    pub product_code: String,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub friendly_name: String,
    /// `UIDnnnn` from the monitor device path; tells identical monitors apart.
    #[serde(default)]
    pub connector_hint: Option<String>,
    #[serde(default)]
    pub adapter_hint: Option<String>,
    #[serde(default)]
    pub output_technology: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default)]
    pub others_policy: OthersPolicy,
    #[serde(default)]
    pub displays: Vec<DisplaySetting>,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub actions: Actions,
    #[serde(default)]
    pub safety: Safety,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_icon() -> String {
    "builtin:monitor".to_string()
}

impl Group {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            icon: default_icon(),
            others_policy: OthersPolicy::default(),
            displays: Vec::new(),
            audio: AudioSettings::default(),
            actions: Actions::default(),
            safety: Safety::default(),
            extra: Map::new(),
        }
    }

    pub fn enabled_displays(&self) -> impl Iterator<Item = &DisplaySetting> {
        self.displays.iter().filter(|d| d.enabled)
    }

    /// The enabled display at desktop position (0,0), which Windows makes primary.
    pub fn primary(&self) -> Option<&DisplaySetting> {
        self.enabled_displays()
            .find(|d| d.position.x == 0 && d.position.y == 0)
    }
}

/// What happens to connected monitors that a group does not list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OthersPolicy {
    #[default]
    Disable,
    Leave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySetting {
    /// Key into `Config::monitors`.
    pub monitor: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub mode: Mode,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub rotation: Rotation,
    #[serde(default)]
    pub scale_percent: Option<u32>,
    #[serde(default)]
    pub hdr: Option<bool>,
    #[serde(default)]
    pub captured: Option<CapturedTiming>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn yes() -> bool {
    true
}

impl DisplaySetting {
    /// Size on the desktop after rotation.
    pub fn desktop_size(&self) -> (u32, u32) {
        self.rotation.apply(self.mode.width, self.mode.height)
    }

    pub fn rect(&self) -> Rect {
        let (w, h) = self.desktop_size();
        Rect::new(self.position.x, self.position.y, w, h)
    }
}

/// A display mode in the monitor's native (landscape) orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh_milli_hz: u32,
    #[serde(default)]
    pub refresh_policy: RefreshPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshPolicy {
    Exact,
    /// Highest available rate not above the target (e.g. 119 Hz when 120 Hz is missing).
    #[default]
    BestAtOrBelow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Rotation {
    #[default]
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

impl Rotation {
    pub fn is_portrait(self) -> bool {
        matches!(self, Rotation::Portrait | Rotation::PortraitFlipped)
    }

    /// Maps a native (landscape) size to the size it occupies on the desktop.
    pub fn apply(self, width: u32, height: u32) -> (u32, u32) {
        if self.is_portrait() {
            (height, width)
        } else {
            (width, height)
        }
    }
}

/// Exact signal timing captured from a working setup (CCD `DISPLAYCONFIG_VIDEO_SIGNAL_INFO`).
/// Lets the engine set the whole configuration in one call (strategy S1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedTiming {
    pub pixel_rate: u64,
    pub h_sync: RationalValue,
    pub v_sync: RationalValue,
    pub active_width: u32,
    pub active_height: u32,
    pub total_width: u32,
    pub total_height: u32,
    pub video_standard: u32,
    pub scanline_ordering: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RationalValue {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub enabled: bool,
    pub endpoint: Option<EndpointMatch>,
    /// 0..=100; `None` leaves the volume alone.
    pub volume_percent: Option<u8>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointMatch {
    /// Case-insensitive substring of the endpoint's friendly name.
    pub name_contains: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Actions {
    pub enabled: bool,
    /// Switches triggered through `displaygroups://` skip actions unless this is true.
    pub allow_from_uri: bool,
    pub before: Vec<Action>,
    pub after: Vec<Action>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A user-configured action. Runs only in user context, never in the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// Start a program with an explicit argument list (no shell involved).
    #[serde(rename_all = "camelCase")]
    Program {
        path: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    /// Open a URL or registered protocol, e.g. `steam://open/bigpicture`.
    #[serde(rename_all = "camelCase")]
    Open { target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Safety {
    /// 0 disables the "Keep these display settings?" countdown for this group.
    pub confirm_seconds: u32,
}

impl Default for Safety {
    fn default() -> Self {
        Self {
            confirm_seconds: 15,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    /// e.g. `Ctrl+Alt+F12`. Parsed by the agent (Phase 3).
    pub chord: String,
    pub action: HotkeyAction,
    #[serde(default)]
    pub on_lock_screen: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HotkeyAction {
    Apply { group: Uuid },
    Toggle { groups: Vec<Uuid> },
    Cycle { groups: Vec<Uuid> },
    Recover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub language: String,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    pub osd: Osd,
    pub disconnect_fallback: DisconnectFallback,
    pub lock_screen_support: bool,
    pub log_level: LogLevel,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            // Follows the Windows display language when a translation exists.
            language: "system".to_string(),
            start_with_windows: false,
            start_minimized: true,
            osd: Osd::default(),
            disconnect_fallback: DisconnectFallback::default(),
            lock_screen_support: false,
            log_level: LogLevel::Info,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Osd {
    pub enabled: bool,
    pub duration_ms: u32,
}

impl Default for Osd {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 1800,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DisconnectFallback {
    pub enabled: bool,
    pub group: Option<Uuid>,
    pub debounce_ms: u32,
}

impl Default for DisconnectFallback {
    fn default() -> Self {
        Self {
            enabled: false,
            group: None,
            debounce_ms: 3000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

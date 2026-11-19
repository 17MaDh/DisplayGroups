//! The D-Bus client for `org.gnome.Mutter.DisplayConfig`, and EDIDs from sysfs.

use std::collections::HashMap;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::plan::ApplyConfig;
use crate::state::{DisplayState, LayoutMode, LogicalMonitor, Monitor, MonitorMode};
use crate::GnomeError;

const DEST: &str = "org.gnome.Mutter.DisplayConfig";
const PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const IFACE: &str = "org.gnome.Mutter.DisplayConfig";

type Props = HashMap<String, OwnedValue>;
type Spec = (String, String, String, String);
type ModeTuple = (String, i32, i32, f64, f64, Vec<f64>, Props);
type MonitorTuple = (Spec, Vec<ModeTuple>, Props);
type LogicalTuple = (i32, i32, f64, u32, bool, Vec<Spec>, Props);
type StateTuple = (u32, Vec<MonitorTuple>, Vec<LogicalTuple>, Props);
type MonitorArg<'a> = (String, String, HashMap<String, Value<'a>>);
type LogicalArg<'a> = (i32, i32, f64, u32, bool, Vec<MonitorArg<'a>>);

/// How `ApplyMonitorsConfig` applies a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Checks the layout and changes nothing.
    Verify = 0,
    /// Applies it until the next change; GNOME does not remember it.
    Temporary = 1,
    /// Applies and remembers it; GNOME Shell asks "Keep these display settings?" and
    /// reverts after 20 seconds without an answer.
    Persistent = 2,
}

pub struct Mutter {
    conn: Connection,
}

impl Mutter {
    pub fn connect() -> Result<Self, GnomeError> {
        let conn = Connection::session().map_err(|e| GnomeError::Unavailable(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn state(&self) -> Result<DisplayState, GnomeError> {
        let reply = self
            .conn
            .call_method(Some(DEST), PATH, Some(IFACE), "GetCurrentState", &())
            .map_err(|e| GnomeError::Unavailable(e.to_string()))?;
        let (serial, monitors, logical, props): StateTuple = reply
            .body()
            .deserialize()
            .map_err(|e| GnomeError::Unavailable(format!("unexpected reply: {e}")))?;
        let edids = sysfs_edids();
        Ok(DisplayState {
            serial,
            monitors: monitors
                .into_iter()
                .map(
                    |((connector, vendor, product, serial), modes, props)| Monitor {
                        edid: edids.get(&connector).cloned(),
                        display_name: prop_string(&props, "display-name"),
                        builtin: prop_bool(&props, "is-builtin"),
                        modes: modes
                            .into_iter()
                            .map(|(id, w, h, refresh, preferred_scale, scales, props)| {
                                MonitorMode {
                                    id,
                                    width: w.max(0) as u32,
                                    height: h.max(0) as u32,
                                    refresh,
                                    preferred_scale,
                                    supported_scales: scales,
                                    current: prop_bool(&props, "is-current"),
                                    preferred: prop_bool(&props, "is-preferred"),
                                }
                            })
                            .collect(),
                        connector,
                        vendor,
                        product,
                        serial,
                    },
                )
                .collect(),
            logical_monitors: logical
                .into_iter()
                .map(
                    |(x, y, scale, transform, primary, specs, _)| LogicalMonitor {
                        x,
                        y,
                        scale,
                        transform,
                        primary,
                        connectors: specs.into_iter().map(|s| s.0).collect(),
                    },
                )
                .collect(),
            layout_mode: if prop_u32(&props, "layout-mode") == Some(2) {
                LayoutMode::Physical
            } else {
                LayoutMode::Logical
            },
            global_scale_required: prop_bool(&props, "global-scale-required"),
        })
    }

    pub fn apply(&self, plan: &ApplyConfig, method: Method) -> Result<(), GnomeError> {
        let logical: Vec<LogicalArg> = plan
            .logical_monitors
            .iter()
            .map(|l| {
                (
                    l.x,
                    l.y,
                    l.scale,
                    l.transform,
                    l.primary,
                    l.monitors
                        .iter()
                        .map(|(c, m)| (c.clone(), m.clone(), HashMap::new()))
                        .collect(),
                )
            })
            .collect();
        let props: HashMap<String, Value> = HashMap::new();
        self.conn
            .call_method(
                Some(DEST),
                PATH,
                Some(IFACE),
                "ApplyMonitorsConfig",
                &(plan.serial, method as u32, logical, props),
            )
            .map(drop)
            .map_err(|e| GnomeError::Rejected(dbus_message(&e)))
    }

    /// Waits up to `timeout` for the layout to change (e.g. after a switch or when the
    /// user answers GNOME's "Keep these display settings?").
    pub fn wait_changed(&self, timeout: Duration) -> Result<bool, GnomeError> {
        let before = self.state()?.serial;
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            if self.state()?.serial != before {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn dbus_message(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(desc), _) => desc.clone(),
        other => other.to_string(),
    }
}

fn prop_bool(p: &Props, key: &str) -> bool {
    p.get(key)
        .and_then(|v| bool::try_from(&**v).ok())
        .unwrap_or(false)
}

fn prop_u32(p: &Props, key: &str) -> Option<u32> {
    p.get(key).and_then(|v| u32::try_from(&**v).ok())
}

fn prop_string(p: &Props, key: &str) -> String {
    p.get(key)
        .and_then(|v| <&str>::try_from(&**v).ok().map(str::to_string))
        .unwrap_or_default()
}

/// EDIDs by Mutter connector name. The kernel calls connectors `card1-HDMI-A-1`, Mutter
/// calls the same one `HDMI-1`.
fn sysfs_edids() -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else {
        return out;
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((_, connector)) = name.split_once('-') else {
            continue;
        };
        let Ok(edid) = std::fs::read(entry.path().join("edid")) else {
            continue;
        };
        if edid.len() >= 128 {
            out.insert(mutter_connector_name(connector), edid);
        }
    }
    out
}

pub(crate) fn mutter_connector_name(kernel: &str) -> String {
    kernel.replacen("HDMI-A-", "HDMI-", 1)
}

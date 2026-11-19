//! Messages between the agent (user) and the lock-screen service (LocalSystem).

use std::collections::{BTreeMap, BTreeSet};

use dg_hotkey::Chord;
use dg_model::{
    CapturedTiming, Config, DisplaySetting, Group, HotkeyAction, Mode, MonitorEntry,
    MonitorIdentity, OthersPolicy, Position, Rotation,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
pub const SERVICE_PIPE: &str = r"\\.\pipe\DisplayGroups.Service.v1";
/// Protected DACL: SYSTEM and Administrators full control, interactive users
/// read/write, nobody else (ARCHITECTURE.md §8.3).
pub const SERVICE_PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

pub const MAX_PROFILES: usize = 32;
pub const MAX_DISPLAYS_PER_PROFILE: usize = 16;
pub const MAX_HOTKEYS: usize = 32;
pub const MAX_NAME_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    Hello {
        version: u32,
    },
    /// Replaces the caller's lock-screen data (stored per user SID by the service).
    SyncLockProfiles {
        profiles: Vec<LockProfile>,
        hotkeys: Vec<LockHotkey>,
    },
    /// Asked by the agent after unlock, to finish per-user fix-ups (DPI, audio).
    GetAppliedWhileLocked,
    Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    Hello {
        version: u32,
    },
    Ok,
    AppliedWhileLocked {
        group: Option<Uuid>,
        at_unix: Option<u64>,
    },
    Status {
        lock_screen_ready: bool,
        hotkeys: u32,
    },
    Error {
        message: String,
    },
}

/// Display geometry of one group: everything the broker needs, nothing more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockProfile {
    pub id: Uuid,
    pub name: String,
    pub others_policy: OthersPolicy,
    pub monitors: BTreeMap<String, MonitorIdentity>,
    pub displays: Vec<LockDisplay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockDisplay {
    pub monitor: String,
    pub enabled: bool,
    pub mode: Mode,
    pub position: Position,
    pub rotation: Rotation,
    pub captured: Option<CapturedTiming>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockHotkey {
    pub chord: String,
    pub action: HotkeyAction,
}

/// Builds the lock-screen payload from the user's config: hotkeys marked
/// `onLockScreen` and the groups they use, stripped of audio, actions, icons, scale
/// and HDR (per-user or user-context features).
pub fn lock_payload(cfg: &Config) -> (Vec<LockProfile>, Vec<LockHotkey>) {
    let hotkeys: Vec<LockHotkey> = cfg
        .hotkeys
        .iter()
        .filter(|h| h.on_lock_screen)
        .map(|h| LockHotkey {
            chord: h.chord.clone(),
            action: h.action.clone(),
        })
        .collect();
    let used: BTreeSet<Uuid> = hotkeys.iter().flat_map(|h| groups_of(&h.action)).collect();
    let profiles = cfg
        .groups
        .iter()
        .filter(|g| used.contains(&g.id))
        .map(|g| LockProfile {
            id: g.id,
            name: g.name.clone(),
            others_policy: g.others_policy,
            monitors: g
                .displays
                .iter()
                .filter_map(|d| {
                    cfg.monitors
                        .get(&d.monitor)
                        .map(|m| (d.monitor.clone(), m.identity.clone()))
                })
                .collect(),
            displays: g
                .displays
                .iter()
                .map(|d| LockDisplay {
                    monitor: d.monitor.clone(),
                    enabled: d.enabled,
                    mode: d.mode,
                    position: d.position,
                    rotation: d.rotation,
                    captured: d.captured,
                })
                .collect(),
        })
        .collect();
    (profiles, hotkeys)
}

fn groups_of(action: &HotkeyAction) -> Vec<Uuid> {
    match action {
        HotkeyAction::Apply { group } => vec![*group],
        HotkeyAction::Toggle { groups } | HotkeyAction::Cycle { groups } => groups.clone(),
        HotkeyAction::Recover => Vec::new(),
    }
}

/// Rebuilds a minimal config the engine can run in the broker (no per-user settings).
pub fn to_config(profiles: &[LockProfile]) -> Config {
    let mut cfg = Config::default();
    for p in profiles {
        for (key, identity) in &p.monitors {
            cfg.monitors.insert(
                key.clone(),
                MonitorEntry {
                    display_name: identity.friendly_name.clone(),
                    identity: identity.clone(),
                    extra: Default::default(),
                },
            );
        }
        let mut g = Group::new(p.name.clone());
        g.id = p.id;
        g.others_policy = p.others_policy;
        g.safety.confirm_seconds = 0; // no UI on the secure desktop
        g.displays = p
            .displays
            .iter()
            .map(|d| DisplaySetting {
                monitor: d.monitor.clone(),
                enabled: d.enabled,
                mode: d.mode,
                position: d.position,
                rotation: d.rotation,
                scale_percent: None,
                hdr: None,
                captured: d.captured,
                extra: Default::default(),
            })
            .collect();
        cfg.groups.push(g);
    }
    cfg
}

/// Checks a `SyncLockProfiles` payload before the service stores it. The service
/// runs as SYSTEM, so everything is bounded and every reference must resolve.
pub fn validate_sync(profiles: &[LockProfile], hotkeys: &[LockHotkey]) -> Result<(), String> {
    if profiles.len() > MAX_PROFILES {
        return Err(format!("more than {MAX_PROFILES} groups"));
    }
    if hotkeys.len() > MAX_HOTKEYS {
        return Err(format!("more than {MAX_HOTKEYS} hotkeys"));
    }
    let ids: BTreeSet<Uuid> = profiles.iter().map(|p| p.id).collect();
    if ids.len() != profiles.len() {
        return Err("duplicate group id".into());
    }
    for p in profiles {
        if p.name.chars().count() > MAX_NAME_CHARS {
            return Err(format!(
                "group name longer than {MAX_NAME_CHARS} characters"
            ));
        }
        if p.displays.is_empty() || p.displays.len() > MAX_DISPLAYS_PER_PROFILE {
            return Err(format!(
                "\"{}\" must have 1..={MAX_DISPLAYS_PER_PROFILE} displays",
                p.name
            ));
        }
        if p.monitors.len() > MAX_DISPLAYS_PER_PROFILE {
            return Err(format!("\"{}\" lists too many monitors", p.name));
        }
        if let Some(d) = p
            .displays
            .iter()
            .find(|d| !p.monitors.contains_key(&d.monitor))
        {
            return Err(format!(
                "\"{}\" uses unknown monitor \"{}\"",
                p.name, d.monitor
            ));
        }
    }
    for h in hotkeys {
        Chord::parse(&h.chord).map_err(|e| format!("hotkey \"{}\": {e}", h.chord))?;
        if let Some(missing) = groups_of(&h.action)
            .into_iter()
            .find(|id| !ids.contains(id))
        {
            return Err(format!(
                "hotkey \"{}\" uses unknown group {missing}",
                h.chord
            ));
        }
    }
    let issues = dg_model::validate::validate(&to_config(profiles));
    if let Some(e) = issues
        .iter()
        .find(|i| i.severity == dg_model::validate::Severity::Error)
    {
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dg_model::{AudioSettings, EndpointMatch, Hotkey, RefreshPolicy};

    fn sample() -> Config {
        let mut cfg = Config::default();
        cfg.monitors.insert(
            "tv".into(),
            MonitorEntry {
                display_name: "TV".into(),
                identity: MonitorIdentity {
                    pnp_id: "SAM".into(),
                    product_code: "721B".into(),
                    friendly_name: "SAMSUNG".into(),
                    ..Default::default()
                },
                extra: Default::default(),
            },
        );
        let mut couch = Group::new("Couch");
        couch.displays.push(DisplaySetting {
            monitor: "tv".into(),
            enabled: true,
            mode: Mode {
                width: 3840,
                height: 2160,
                refresh_milli_hz: 120_000,
                refresh_policy: RefreshPolicy::BestAtOrBelow,
            },
            position: Position::default(),
            rotation: Rotation::Landscape,
            scale_percent: Some(175),
            hdr: Some(true),
            captured: None,
            extra: Default::default(),
        });
        couch.audio = AudioSettings {
            enabled: true,
            endpoint: Some(EndpointMatch {
                name_contains: "SAMSUNG".into(),
            }),
            volume_percent: Some(40),
            extra: Default::default(),
        };
        couch.actions.enabled = true;
        let unused = Group::new("Not on the lock screen");
        cfg.hotkeys.push(Hotkey {
            chord: "Ctrl+Alt+F11".into(),
            action: HotkeyAction::Apply { group: couch.id },
            on_lock_screen: true,
            extra: Default::default(),
        });
        cfg.hotkeys.push(Hotkey {
            chord: "Ctrl+Alt+F10".into(),
            action: HotkeyAction::Apply { group: unused.id },
            on_lock_screen: false,
            extra: Default::default(),
        });
        cfg.groups = vec![couch, unused];
        cfg
    }

    #[test]
    fn payload_has_only_lock_screen_hotkeys_and_their_geometry() {
        let (profiles, hotkeys) = lock_payload(&sample());
        assert_eq!(hotkeys.len(), 1);
        assert_eq!(profiles.len(), 1);
        let json = serde_json::to_string(&profiles).unwrap();
        // Per-user and user-context data never reaches the service.
        for forbidden in ["volume", "audio", "actions", "scale", "hdr", "icon"] {
            assert!(
                !json.to_lowercase().contains(forbidden),
                "{forbidden} leaked: {json}"
            );
        }
        validate_sync(&profiles, &hotkeys).unwrap();
    }

    #[test]
    fn broker_config_never_asks_for_confirmation() {
        let (profiles, _) = lock_payload(&sample());
        let cfg = to_config(&profiles);
        assert_eq!(cfg.groups[0].safety.confirm_seconds, 0);
        assert_eq!(cfg.groups[0].displays[0].scale_percent, None);
    }

    #[test]
    fn hostile_payloads_are_rejected() {
        let (profiles, hotkeys) = lock_payload(&sample());

        let mut bad = profiles.clone();
        bad[0].displays[0].monitor = "nope".into();
        assert!(validate_sync(&bad, &hotkeys)
            .unwrap_err()
            .contains("unknown monitor"));

        let mut bad_keys = hotkeys.clone();
        bad_keys[0].chord = "Ctrl+Nonsense".into();
        assert!(validate_sync(&profiles, &bad_keys).is_err());

        let mut dangling = hotkeys.clone();
        dangling[0].action = HotkeyAction::Apply {
            group: Uuid::new_v4(),
        };
        assert!(validate_sync(&profiles, &dangling)
            .unwrap_err()
            .contains("unknown group"));

        let many: Vec<LockProfile> = (0..=MAX_PROFILES)
            .map(|_| {
                let mut p = profiles[0].clone();
                p.id = Uuid::new_v4();
                p
            })
            .collect();
        assert!(validate_sync(&many, &[]).is_err());

        let mut long = profiles.clone();
        long[0].name = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(validate_sync(&long, &hotkeys).is_err());
    }

    #[test]
    fn messages_use_tagged_json() {
        let json = serde_json::to_string(&Request::Hello {
            version: PROTOCOL_VERSION,
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"hello","version":1}"#);
        let back: Response = serde_json::from_str(r#"{"type":"error","message":"no"}"#).unwrap();
        assert_eq!(
            back,
            Response::Error {
                message: "no".into()
            }
        );
    }
}

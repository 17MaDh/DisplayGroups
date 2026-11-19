//! Tests against states shaped like the real three-display rig: a 4K 240 Hz monitor at
//! 150% (DP-1), a 1080p monitor (DP-2) and a 4K 120 Hz TV (HDMI-1).

use std::collections::BTreeMap;

use dg_model::{
    Config, DisplaySetting, Group, Mode, MonitorEntry, OthersPolicy, Position, RefreshPolicy,
    Rotation,
};
use uuid::Uuid;

use crate::observe::observe;
use crate::state::{DisplayState, LayoutMode, LogicalMonitor, Monitor, MonitorMode};
use crate::{capture, current_group, plan, status, GnomeError};

const ASUS: &str = "mon-aus-aa06-w5lmqs046072";
const DFI: &str = "mon-hxm-40f3-01";
const TV: &str = "mon-sam-721b-376965120";

/// A minimal EDID base block: ids, an optional serial string and a name.
fn edid(pnp: &str, product: u16, serial: u32, serial_text: Option<&str>, name: &str) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
    let c: Vec<u16> = pnp.bytes().map(|x| u16::from(x - b'A' + 1)).collect();
    b[8..10].copy_from_slice(&((c[0] << 10) | (c[1] << 5) | c[2]).to_be_bytes());
    b[10..12].copy_from_slice(&product.to_le_bytes());
    b[12..16].copy_from_slice(&serial.to_le_bytes());
    let mut text = |off: usize, tag: u8, s: &str| {
        b[off + 3] = tag;
        let mut field = [b' '; 13];
        field[..s.len()].copy_from_slice(s.as_bytes());
        if s.len() < 13 {
            field[s.len()] = 0x0A;
        }
        b[off + 5..off + 18].copy_from_slice(&field);
    };
    text(72, 0xFC, name);
    if let Some(s) = serial_text {
        text(90, 0xFF, s);
    }
    b
}

fn mode(w: u32, h: u32, refresh: f64, scales: &[f64], preferred_scale: f64) -> MonitorMode {
    MonitorMode {
        id: format!("{w}x{h}@{refresh:.3}"),
        width: w,
        height: h,
        refresh,
        preferred_scale,
        supported_scales: scales.to_vec(),
        current: false,
        preferred: false,
    }
}

fn monitors(current: &[(&str, &str)]) -> Vec<Monitor> {
    let hi_dpi = [1.0, 1.25, 1.5, 1.75, 2.0];
    let mut list = vec![
        Monitor {
            connector: "DP-1".into(),
            vendor: "AUS".into(),
            product: "XG32UCWMG".into(),
            serial: "W5LMQS046072".into(),
            display_name: "ASUSTek 32\"".into(),
            builtin: false,
            modes: vec![
                mode(3840, 2160, 239.990, &hi_dpi, 1.5),
                mode(3840, 2160, 143.999, &hi_dpi, 1.5),
                mode(3840, 2160, 59.997, &hi_dpi, 1.5),
                mode(1920, 1080, 60.0, &[1.0, 2.0], 1.0),
            ],
            edid: Some(edid("AUS", 0xAA06, 0, Some("W5LMQS046072"), "XG32UCWMG")),
        },
        Monitor {
            connector: "DP-2".into(),
            vendor: "HXM".into(),
            product: "DFI27280".into(),
            serial: "01".into(),
            display_name: "DFI 27\"".into(),
            builtin: false,
            modes: vec![mode(1920, 1080, 60.0, &[1.0, 2.0], 1.0)],
            edid: Some(edid("HXM", 0x40F3, 0, Some("01"), "DFI27280")),
        },
        Monitor {
            connector: "HDMI-1".into(),
            vendor: "SAM".into(),
            product: "SAMSUNG".into(),
            serial: "0x16780800".into(),
            display_name: "Samsung 65\"".into(),
            builtin: false,
            modes: vec![
                mode(3840, 2160, 119.880, &hi_dpi, 2.0),
                mode(3840, 2160, 60.0, &hi_dpi, 2.0),
                mode(1920, 1080, 120.0, &[1.0, 2.0], 1.0),
            ],
            edid: Some(edid("SAM", 0x721B, 376_965_120, None, "SAMSUNG")),
        },
    ];
    for m in &mut list {
        if let Some((_, id)) = current.iter().find(|(c, _)| *c == m.connector) {
            for md in &mut m.modes {
                md.current = md.id == *id;
            }
        }
        m.modes[0].preferred = true;
    }
    list
}

fn desk_state() -> DisplayState {
    DisplayState {
        serial: 7,
        monitors: monitors(&[("DP-1", "3840x2160@239.990"), ("DP-2", "1920x1080@60.000")]),
        logical_monitors: vec![
            LogicalMonitor {
                x: 0,
                y: 0,
                scale: 1.5,
                transform: 0,
                primary: true,
                connectors: vec!["DP-1".into()],
            },
            LogicalMonitor {
                x: -1920,
                y: 316,
                scale: 1.0,
                transform: 0,
                primary: false,
                connectors: vec!["DP-2".into()],
            },
        ],
        layout_mode: LayoutMode::Logical,
        global_scale_required: false,
    }
}

fn tv_state() -> DisplayState {
    DisplayState {
        serial: 9,
        monitors: monitors(&[("HDMI-1", "3840x2160@119.880")]),
        logical_monitors: vec![LogicalMonitor {
            x: 0,
            y: 0,
            scale: 2.0,
            transform: 0,
            primary: true,
            connectors: vec!["HDMI-1".into()],
        }],
        layout_mode: LayoutMode::Logical,
        global_scale_required: false,
    }
}

fn entry(name: &str, pnp: &str, product: &str, serial: &str) -> MonitorEntry {
    serde_json::from_value(serde_json::json!({
        "displayName": name,
        "identity": { "pnpId": pnp, "productCode": product, "serial": serial, "friendlyName": name }
    }))
    .unwrap()
}

fn display(
    monitor: &str,
    w: u32,
    h: u32,
    milli_hz: u32,
    x: i32,
    y: i32,
    scale: Option<u32>,
) -> DisplaySetting {
    DisplaySetting {
        monitor: monitor.into(),
        enabled: true,
        mode: Mode {
            width: w,
            height: h,
            refresh_milli_hz: milli_hz,
            refresh_policy: RefreshPolicy::BestAtOrBelow,
        },
        position: Position { x, y },
        rotation: Rotation::Landscape,
        scale_percent: scale,
        hdr: None,
        captured: None,
        extra: Default::default(),
    }
}

/// The config as captured on Windows: "Desktop" (ASUS + DFI) and "Couch" (TV only).
fn rig_config() -> (Config, Uuid, Uuid) {
    let mut monitors = BTreeMap::new();
    monitors.insert(
        ASUS.to_string(),
        entry("XG32UCWMG", "AUS", "AA06", "W5LMQS046072"),
    );
    monitors.insert(DFI.to_string(), entry("DFI27280", "HXM", "40F3", "01"));
    monitors.insert(TV.to_string(), entry("SAMSUNG", "SAM", "721B", "376965120"));
    let mut desk = Group::new("Desktop");
    desk.displays = vec![
        display(ASUS, 3840, 2160, 240_016, 0, 0, Some(150)),
        display(DFI, 1920, 1080, 60_000, -1920, 474, Some(100)),
    ];
    let mut couch = Group::new("Couch");
    couch.displays = vec![display(TV, 3840, 2160, 120_000, 0, 0, None)];
    let (d, c) = (desk.id, couch.id);
    let mut cfg: Config =
        serde_json::from_value(serde_json::json!({ "schemaVersion": 1 })).unwrap();
    cfg.monitors = monitors;
    cfg.groups = vec![desk, couch];
    (cfg, d, c)
}

#[test]
fn windows_identities_are_recognised_from_the_edid() {
    let (cfg, ..) = rig_config();
    let s = status(&cfg, &desk_state());
    let keys: Vec<Option<&str>> = s
        .connected
        .iter()
        .map(|c| c.monitor_key.as_deref())
        .collect();
    assert_eq!(keys, vec![Some(ASUS), Some(DFI), Some(TV)]);
    assert!(s.connected.iter().all(|c| c.new_monitor.is_none()));
    assert_eq!(s.displays.len(), 2);
    assert!(s.displays[0].primary);
    assert_eq!((s.displays[0].width, s.displays[0].height), (3840, 2160));
    assert!(!s.connected[2].active);
    assert_eq!(
        s.connected[2].preferred.map(|p| p.refresh_milli_hz),
        Some(119_880)
    );
}

#[test]
fn identical_twins_are_told_apart() {
    // Two monitors of one model without EDIDs, as Mutter's virtual monitors: one has a
    // serial, the other is only known by its connector.
    let twin = |connector: &str, serial: &str| Monitor {
        connector: connector.into(),
        vendor: "MetaVendor".into(),
        product: "MetaVirtualMonitor".into(),
        serial: serial.into(),
        display_name: String::new(),
        builtin: false,
        modes: vec![MonitorMode {
            current: true,
            ..mode(1920, 1080, 60.0, &[1.0], 1.0)
        }],
        edid: None,
    };
    let mut state = desk_state();
    state.monitors = vec![twin("Meta-0", "0x0"), twin("Meta-1", "0x1")];
    state.logical_monitors[0].connectors = vec!["Meta-0".into()];
    state.logical_monitors[1].connectors = vec!["Meta-1".into()];
    let (mut cfg, ..) = rig_config();
    let captured = capture(&cfg, &state).unwrap();
    for (key, entry) in captured.new_monitors {
        cfg.monitors.insert(key, entry);
    }
    let keys: Vec<Option<String>> = status(&cfg, &state)
        .connected
        .into_iter()
        .map(|c| c.monitor_key)
        .collect();
    assert!(keys.iter().all(Option::is_some), "{keys:?}");
    assert_ne!(keys[0], keys[1]);
}

#[test]
fn without_an_edid_mutter_ids_are_decoded_like_windows() {
    let mut tv = monitors(&[]).remove(2);
    tv.edid = None;
    tv.product = "0x721b".into();
    let o = observe(&tv);
    assert_eq!(
        (o.pnp_id.as_str(), o.product_code.as_str()),
        ("SAM", "721B")
    );
    assert_eq!(o.serial.as_deref(), Some("376965120"));
    tv.serial = "0x00000000".into();
    assert_eq!(observe(&tv).serial, None);
}

#[test]
fn the_current_group_follows_the_screen() {
    let (cfg, desk, couch) = rig_config();
    let ids = [desk, couch];
    assert_eq!(current_group(&cfg, &desk_state(), &ids), Some(desk));
    assert_eq!(current_group(&cfg, &tv_state(), &ids), Some(couch));
    let mut both = desk_state();
    both.logical_monitors[1].primary = true;
    both.logical_monitors[0].primary = false;
    assert_eq!(current_group(&cfg, &both, &ids), None, "another primary");
}

#[test]
fn capture_converts_the_logical_layout_to_pixels() {
    let (cfg, ..) = rig_config();
    let c = capture(&cfg, &desk_state()).unwrap();
    assert!(c.new_monitors.is_empty());
    let asus = &c.displays[0];
    assert_eq!(
        (asus.monitor.as_str(), asus.position),
        (ASUS, Position { x: 0, y: 0 })
    );
    assert_eq!(
        (
            asus.mode.width,
            asus.mode.refresh_milli_hz,
            asus.scale_percent
        ),
        (3840, 239_990, Some(150))
    );
    let dfi = &c.displays[1];
    assert_eq!(dfi.position, Position { x: -1920, y: 474 });
    assert_eq!(dfi.scale_percent, Some(100));
}

#[test]
fn capture_names_a_new_monitor_like_windows_does() {
    let (mut cfg, ..) = rig_config();
    cfg.monitors.remove(TV);
    let c = capture(&cfg, &tv_state()).unwrap();
    assert_eq!(c.new_monitors.len(), 1);
    let (key, entry) = &c.new_monitors[0];
    assert_eq!(key, TV);
    assert_eq!(entry.display_name, "SAMSUNG");
    assert_eq!(c.displays[0].scale_percent, Some(200));
}

#[test]
fn desktop_from_the_tv_turns_the_tv_off() {
    let (cfg, desk, _) = rig_config();
    let group = cfg.group_by_id(desk).unwrap();
    let p = plan(&cfg, group, &tv_state()).unwrap();
    assert_eq!(p.serial, 9);
    assert!(p.notes.is_empty(), "{:?}", p.notes);
    let monitors: Vec<_> = p
        .logical_monitors
        .iter()
        .map(|l| l.monitors[0].clone())
        .collect();
    assert_eq!(
        monitors,
        vec![
            ("DP-1".to_string(), "3840x2160@239.990".to_string()),
            ("DP-2".to_string(), "1920x1080@60.000".to_string()),
        ]
    );
    let asus = &p.logical_monitors[0];
    assert!(asus.primary && asus.scale == 1.5 && (asus.x, asus.y) == (0, 0));
    let dfi = &p.logical_monitors[1];
    assert!(!dfi.primary && dfi.scale == 1.0);
    assert_eq!((dfi.x, dfi.y), (-1920, 316));
}

#[test]
fn the_tv_group_picks_119_88_hz_and_keeps_its_scale() {
    let (cfg, _, couch) = rig_config();
    let p = plan(&cfg, cfg.group_by_id(couch).unwrap(), &desk_state()).unwrap();
    assert_eq!(p.logical_monitors.len(), 1);
    let tv = &p.logical_monitors[0];
    assert_eq!(
        tv.monitors[0],
        ("HDMI-1".to_string(), "3840x2160@119.880".to_string())
    );
    assert_eq!(tv.scale, 2.0, "off now: its preferred scale");
    assert!(tv.primary);
}

#[test]
fn a_mode_that_is_not_offered_falls_back_with_a_note() {
    let (mut cfg, _, couch) = rig_config();
    cfg.groups[1].displays[0].mode.width = 5120;
    cfg.groups[1].displays[0].mode.height = 2880;
    let p = plan(&cfg, cfg.group_by_id(couch).unwrap(), &desk_state()).unwrap();
    assert_eq!(p.logical_monitors[0].monitors[0].1, "3840x2160@119.880");
    assert_eq!(p.notes.len(), 1);
}

#[test]
fn a_missing_monitor_is_reported() {
    let (cfg, _, couch) = rig_config();
    let mut state = desk_state();
    state.monitors.retain(|m| m.connector != "HDMI-1");
    let err = plan(&cfg, cfg.group_by_id(couch).unwrap(), &state).unwrap_err();
    assert!(matches!(err, GnomeError::MonitorMissing { ref name } if name == "SAMSUNG"));
}

#[test]
fn x11_uses_pixels_and_one_scale() {
    let (cfg, desk, _) = rig_config();
    let mut state = tv_state();
    state.layout_mode = LayoutMode::Physical;
    state.global_scale_required = true;
    let p = plan(&cfg, cfg.group_by_id(desk).unwrap(), &state).unwrap();
    let dfi = &p.logical_monitors[1];
    assert_eq!((dfi.x, dfi.y), (-1920, 474));
    // 1.5 is not offered by the 1080p monitor: the nearest scale it supports.
    assert_eq!(dfi.scale, 1.0);
}

#[test]
fn leave_keeps_the_other_displays_on() {
    let (mut cfg, desk, _) = rig_config();
    {
        let g = cfg.group_by_id_mut(desk).unwrap();
        g.displays.truncate(1);
        g.others_policy = OthersPolicy::Leave;
    }
    let p = plan(&cfg, cfg.group_by_id(desk).unwrap(), &desk_state()).unwrap();
    assert_eq!(p.logical_monitors.len(), 2);
    let kept = &p.logical_monitors[1];
    assert_eq!(kept.monitors[0].0, "DP-2");
    assert_eq!((kept.x, kept.y, kept.primary), (2560, 0, false));
}

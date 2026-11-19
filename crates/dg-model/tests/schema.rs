//! Round-trip tests against the example from ARCHITECTURE.md §5.

use dg_model::{store, validate, Action, Config, HotkeyAction, OthersPolicy, RefreshPolicy, Theme};

const EXAMPLE: &str = r#"{
  "schemaVersion": 1,
  "monitors": {
    "mon-sam-721b-376965120": {
      "displayName": "Living-room TV",
      "identity": { "pnpId": "SAM", "productCode": "721B", "serial": "376965120",
                    "friendlyName": "SAMSUNG", "connectorHint": "UID4352",
                    "adapterHint": null, "outputTechnology": "hdmi" }
    }
  },
  "groups": [
    {
      "id": "6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60",
      "name": "Gaming on Couch",
      "icon": "builtin:couch",
      "othersPolicy": "disable",
      "displays": [
        { "monitor": "mon-sam-721b-376965120", "enabled": true,
          "mode": { "width": 3840, "height": 2160, "refreshMilliHz": 120000, "refreshPolicy": "bestAtOrBelow" },
          "position": { "x": 0, "y": 0 }, "rotation": "landscape",
          "scalePercent": 175, "hdr": null, "captured": null,
          "futureDisplayField": 7 }
      ],
      "audio": { "enabled": true, "endpoint": { "nameContains": "SAMSUNG" }, "volumePercent": 40 },
      "actions": { "enabled": false, "allowFromUri": false, "before": [],
                   "after": [ { "kind": "open", "target": "steam://open/bigpicture" },
                              { "kind": "program", "path": "C:\\Program Files\\Kodi\\kodi.exe", "args": ["-fs"], "workingDir": null } ] },
      "safety": { "confirmSeconds": 15 }
    }
  ],
  "hotkeys": [
    { "chord": "Ctrl+Alt+F12", "action": { "type": "toggle", "groups": ["6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60", "6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60"] }, "onLockScreen": true },
    { "chord": "Ctrl+Alt+Shift+F12", "action": { "type": "recover" }, "onLockScreen": true }
  ],
  "settings": {
    "theme": "dark", "language": "en",
    "startWithWindows": true, "startMinimized": true,
    "osd": { "enabled": true, "durationMs": 1800 },
    "disconnectFallback": { "enabled": false, "group": null, "debounceMs": 3000 },
    "lockScreenSupport": false, "logLevel": "info"
  },
  "futureTopLevel": { "kept": true }
}"#;

#[test]
fn example_parses_with_expected_values() {
    let cfg = store::parse(EXAMPLE).unwrap();
    let g = &cfg.groups[0];
    assert_eq!(g.name, "Gaming on Couch");
    assert_eq!(g.others_policy, OthersPolicy::Disable);
    assert_eq!(g.displays[0].mode.refresh_milli_hz, 120_000);
    assert_eq!(
        g.displays[0].mode.refresh_policy,
        RefreshPolicy::BestAtOrBelow
    );
    assert_eq!(g.displays[0].scale_percent, Some(175));
    assert_eq!(g.audio.volume_percent, Some(40));
    assert!(
        matches!(&g.actions.after[0], Action::Open { target } if target == "steam://open/bigpicture")
    );
    assert!(matches!(&g.actions.after[1], Action::Program { args, .. } if args == &["-fs"]));
    assert!(matches!(cfg.hotkeys[1].action, HotkeyAction::Recover));
    assert_eq!(cfg.settings.theme, Theme::Dark);
    assert!(cfg.settings.start_with_windows);
    assert_eq!(g.primary().unwrap().monitor, "mon-sam-721b-376965120");
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let cfg = store::parse(EXAMPLE).unwrap();
    let text = serde_json::to_string(&cfg).unwrap();
    let back: Config = store::parse(&text).unwrap();
    assert_eq!(back, cfg);
    assert_eq!(back.extra["futureTopLevel"]["kept"], true);
    assert_eq!(back.groups[0].displays[0].extra["futureDisplayField"], 7);
}

#[test]
fn example_is_valid() {
    let cfg = store::parse(EXAMPLE).unwrap();
    let issues = validate::validate(&cfg);
    assert!(!validate::has_errors(&issues), "{issues:?}");
}

#[test]
fn minimal_file_gets_defaults() {
    let cfg = store::parse(r#"{ "schemaVersion": 1, "groups": [ { "id": "6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60", "name": "A" } ] }"#).unwrap();
    let g = &cfg.groups[0];
    assert_eq!(g.icon, "builtin:monitor");
    assert_eq!(g.safety.confirm_seconds, 15);
    assert!(!g.actions.enabled);
    assert!(cfg.settings.osd.enabled);
    assert_eq!(cfg.settings.osd.duration_ms, 1800);
}

#[test]
fn duplicate_group_copies_under_new_name_and_id() {
    let mut cfg = store::parse(EXAMPLE).unwrap();
    let original = cfg.groups[0].id;
    let first = cfg.duplicate_group(original).unwrap();
    let second = cfg.duplicate_group(original).unwrap();
    assert_ne!(first, original);
    let names: Vec<&str> = cfg.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Gaming on Couch",
            "Gaming on Couch (copy) 2",
            "Gaming on Couch (copy)"
        ]
    );
    assert_eq!(
        cfg.group_by_id(first).unwrap().displays,
        cfg.groups[0].displays
    );
    assert!(cfg.group_by_id(second).is_some());
}

#[test]
fn monitor_numbers_follow_key_order() {
    let mut cfg = store::parse(EXAMPLE).unwrap();
    let first = cfg.monitors.keys().next().unwrap().clone();
    assert_eq!(cfg.monitor_number(&first), Some(1));
    assert_eq!(cfg.monitor_number("no such monitor"), None);
    let entry = cfg.monitors[&first].clone();
    cfg.monitors.insert("0000-first".into(), entry);
    assert_eq!(cfg.monitor_number("0000-first"), Some(1));
    assert_eq!(cfg.monitor_number(&first), Some(2));
}

#[test]
fn language_defaults_to_the_windows_language() {
    assert_eq!(Config::default().settings.language, "system");
}

#[test]
fn find_group_by_name_or_id() {
    let cfg = store::parse(EXAMPLE).unwrap();
    assert!(cfg.find_group("  gaming ON couch ").is_some());
    assert!(cfg
        .find_group("6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60")
        .is_some());
    assert!(cfg.find_group("Desktop").is_none());
}

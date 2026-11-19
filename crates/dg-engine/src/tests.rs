//! End-to-end engine scenarios against the fake backend modelled on the test machine.

use std::cell::Cell;

use dg_model::{EndpointMatch, OthersPolicy};

use crate::backend::AudioEndpoint;
use crate::ccd::{sdc, win32, Scope};
use crate::fake::*;
use crate::plan::{self, Strategy};
use crate::resolve::resolve_group;
use crate::{next_in_cycle, ApplyOptions, Confirmer, Engine, EngineError, RecoverMethod, Snapshot};

struct Answer(bool, Cell<u32>);

impl Confirmer for Answer {
    fn confirm(&self, seconds: u32, _group: &str) -> bool {
        self.1.set(seconds);
        self.0
    }
}

fn no_confirm() -> ApplyOptions<'static> {
    ApplyOptions::default()
}

#[test]
fn couch_switch_uses_s2_then_fixes_refresh_and_scale() {
    let fake = rig();
    let cfg = rig_config();
    let couch = cfg.group_by_id(COUCH_GROUP).unwrap();

    let report = Engine::new(&fake)
        .apply(&cfg, couch, &no_confirm())
        .unwrap();

    assert_eq!(
        report.strategy,
        Some(Strategy::SourceModes),
        "S1 needs a capture; S2 is next"
    );
    assert_eq!(fake.active_names(), ["SAMSUNG"]);
    // Windows picked 60 Hz; reconcile raised it to 120 Hz.
    assert_eq!(fake.active_entry(TV).unwrap().hz, 120);
    // The TV came back at Recommended (300%) and was corrected to 175%.
    assert_eq!(fake.dpi_of(TV).unwrap().current_percent(), Some(175));
    let primary = report.primary.unwrap();
    assert_eq!(
        (primary.width, primary.height, primary.refresh_milli_hz),
        (3840, 2160, Some(120_000))
    );
    assert!(report.kept);

    let calls = fake.calls();
    assert!(calls[0].is_validate() && !calls[0].is_apply());
    assert!(calls
        .iter()
        .all(|c| !(c.uses(sdc::USE_SUPPLIED_DISPLAY_CONFIG)
            && c.uses(sdc::ALLOW_PATH_ORDER_CHANGES))));
}

#[test]
fn forgotten_tv_set_falls_back_to_paths_only() {
    // The real failure of 2026-11-19: Windows forgot the TV-only set after a reboot.
    let fake = rig();
    fake.respond(|c| {
        let windows_db = c.uses(sdc::TOPOLOGY_SUPPLIED);
        let with_modes = c.uses(sdc::USE_SUPPLIED_DISPLAY_CONFIG) && !c.modes.is_empty();
        (windows_db || with_modes).then_some(win32::GEN_FAILURE)
    });
    let cfg = rig_config();
    let report = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();

    assert_eq!(report.strategy, Some(Strategy::PathsOnly));
    let codes: Vec<_> = report
        .attempts
        .iter()
        .map(|a| (a.strategy, a.validate, a.apply))
        .collect();
    assert_eq!(
        codes,
        [
            (Strategy::SourceModes, win32::GEN_FAILURE, None),
            (Strategy::PathsOnly, 0, Some(0)),
        ]
    );
    assert_eq!(fake.active_names(), ["SAMSUNG"]);
    assert_eq!(fake.dpi_of(TV).unwrap().current_percent(), Some(175));
}

#[test]
fn the_engine_never_sends_arrays_windows_rejects_with_87() {
    let fake = rig();
    let cfg = rig_config();
    let engine = Engine::new(&fake);
    for group in &cfg.groups {
        let report = engine.validate(&cfg, group).unwrap();
        assert!(
            report
                .attempts
                .iter()
                .all(|a| a.validate != win32::INVALID_PARAMETER),
            "{report:?}"
        );
        assert_eq!(report.would_use, Some(Strategy::SourceModes));
    }
    // S4 is accepted too, because the engine always pairs TOPOLOGY_SUPPLIED correctly.
    let report = engine
        .validate(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap())
        .unwrap();
    assert!(report
        .attempts
        .iter()
        .any(|a| a.strategy == Strategy::WindowsDatabase && a.validate == 0));
    // Validation never changes the screen.
    assert_eq!(fake.active_names(), ["DFI27280", "XG32UCWMG"]);
}

#[test]
fn every_strategy_rejected_reports_each_attempt() {
    let fake = rig();
    fake.respond(|_| Some(win32::GEN_FAILURE));
    let cfg = rig_config();
    let err = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap_err();
    match err {
        EngineError::AllStrategiesFailed { attempts } => {
            let used: Vec<_> = attempts.iter().map(|a| a.strategy).collect();
            assert_eq!(
                used,
                [
                    Strategy::SourceModes,
                    Strategy::PathsOnly,
                    Strategy::WindowsDatabase
                ]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fake.active_names(), ["DFI27280", "XG32UCWMG"]);
}

#[test]
fn locked_session_is_reported_as_such() {
    let fake = rig();
    fake.respond(|_| Some(win32::ACCESS_DENIED));
    let cfg = rig_config();
    let err = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap_err();
    assert!(matches!(err, EngineError::Locked));
}

#[test]
fn tv_that_windows_cannot_see_is_missing_before_any_call() {
    let fake = rig();
    fake.set_available(TV, false);
    let cfg = rig_config();
    let err = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap_err();
    assert!(matches!(err, EngineError::MonitorMissing { ref name } if name == "Living-room TV"));
    assert!(fake.calls().is_empty());
}

#[test]
fn declining_the_countdown_reverts_layout_and_scale() {
    let fake = rig();
    let cfg = rig_config();
    let answer = Answer(false, Cell::new(0));
    let opts = ApplyOptions {
        confirm_seconds: 15,
        confirmer: Some(&answer),
        on_applied: None,
    };
    let report = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &opts)
        .unwrap();

    assert_eq!(answer.1.get(), 15);
    assert!(!report.kept);
    assert!(report.notes.iter().any(|n| n.contains("reverted")));
    assert_eq!(fake.active_names(), ["DFI27280", "XG32UCWMG"]);
    let asus = fake.active_entry(ASUS).unwrap();
    assert_eq!((asus.x, asus.y, asus.hz), (0, 0, 240));
    let dfi = fake.active_entry(DFI).unwrap();
    assert_eq!((dfi.x, dfi.y), (-1920, 457));
}

#[test]
fn keeping_the_countdown_keeps_the_layout() {
    let fake = rig();
    let cfg = rig_config();
    let answer = Answer(true, Cell::new(0));
    let opts = ApplyOptions {
        confirm_seconds: 15,
        confirmer: Some(&answer),
        on_applied: None,
    };
    let report = Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &opts)
        .unwrap();
    assert!(report.kept);
    assert_eq!(fake.active_names(), ["SAMSUNG"]);
}

#[test]
fn on_applied_fires_before_the_confirmation() {
    let fake = rig();
    let cfg = rig_config();
    let seen = Cell::new(None);
    let order = std::cell::RefCell::new(Vec::new());
    struct Recorder<'a>(&'a std::cell::RefCell<Vec<&'static str>>);
    impl Confirmer for Recorder<'_> {
        fn confirm(&self, _: u32, _: &str) -> bool {
            self.0.borrow_mut().push("confirm");
            true
        }
    }
    let confirmer = Recorder(&order);
    let callback = |p: Option<crate::PrimaryInfo>| {
        seen.set(p);
        order.borrow_mut().push("applied");
    };
    let opts = ApplyOptions {
        confirm_seconds: 15,
        confirmer: Some(&confirmer),
        on_applied: Some(&callback),
    };
    Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &opts)
        .unwrap();
    assert_eq!(*order.borrow(), ["applied", "confirm"]);
    assert_eq!(seen.get().unwrap().refresh_milli_hz, Some(120_000));
}

#[test]
fn round_trip_back_to_desktop_restores_positions_and_rates() {
    let fake = rig();
    let cfg = rig_config();
    let engine = Engine::new(&fake);
    engine
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();
    let report = engine
        .apply(&cfg, cfg.group_by_id(DESKTOP_GROUP).unwrap(), &no_confirm())
        .unwrap();

    assert_eq!(fake.active_names(), ["DFI27280", "XG32UCWMG"]);
    let asus = fake.active_entry(ASUS).unwrap();
    assert_eq!((asus.width, asus.x, asus.y, asus.hz), (3840, 0, 0, 240));
    let dfi = fake.active_entry(DFI).unwrap();
    assert_eq!((dfi.x, dfi.y, dfi.hz), (-1920, 457, 60));
    assert_eq!(fake.dpi_of(ASUS).unwrap().current_percent(), Some(150));
    assert_eq!(report.primary.unwrap().refresh_milli_hz, Some(240_000));
}

#[test]
fn toggle_detects_the_group_on_screen() {
    let fake = rig();
    let cfg = rig_config();
    let engine = Engine::new(&fake);
    let pair = [DESKTOP_GROUP, COUCH_GROUP];

    let current = engine.current_group(&cfg, &pair).unwrap();
    assert_eq!(current, Some(DESKTOP_GROUP));
    assert_eq!(next_in_cycle(&pair, current), Some(COUCH_GROUP));

    engine
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();
    let current = engine.current_group(&cfg, &pair).unwrap();
    assert_eq!(current, Some(COUCH_GROUP));
    assert_eq!(next_in_cycle(&pair, current), Some(DESKTOP_GROUP));
    assert_eq!(next_in_cycle(&pair, None), Some(DESKTOP_GROUP));
}

#[test]
fn capture_reuses_known_monitors_and_records_layout() {
    let fake = rig();
    let cfg = rig_config();
    let captured = Engine::new(&fake).capture(&cfg).unwrap();
    assert!(captured.new_monitors.is_empty());
    let asus = captured
        .displays
        .iter()
        .find(|d| d.monitor == ASUS_KEY)
        .unwrap();
    assert_eq!(
        (
            asus.mode.width,
            asus.mode.height,
            asus.mode.refresh_milli_hz
        ),
        (3840, 2160, 240_000)
    );
    assert_eq!((asus.position.x, asus.position.y), (0, 0));
    assert_eq!(asus.scale_percent, Some(150));
    assert!(asus.captured.is_some());
    let dfi = captured
        .displays
        .iter()
        .find(|d| d.monitor == DFI_KEY)
        .unwrap();
    assert_eq!((dfi.position.x, dfi.position.y), (-1920, 457));
}

#[test]
fn capture_into_an_empty_config_creates_monitor_entries() {
    let fake = rig();
    let captured = Engine::new(&fake)
        .capture(&dg_model::Config::default())
        .unwrap();
    let keys: Vec<&str> = captured
        .new_monitors
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(keys, ["mon-hxm-40f3-uid4353", "mon-aus-aa06-uid4355"]);
    let (_, asus) = &captured.new_monitors[1];
    assert_eq!(asus.display_name, "XG32UCWMG");
    assert_eq!(asus.identity.pnp_id, "AUS");
    assert_eq!(
        asus.identity.output_technology.as_deref(),
        Some("displayport")
    );
}

#[test]
fn captured_timings_enable_s1() {
    let fake = rig();
    let mut cfg = rig_config();
    let captured = Engine::new(&fake).capture(&cfg).unwrap();
    let desktop = cfg.group_by_id_mut(DESKTOP_GROUP).unwrap();
    desktop.displays = captured.displays;

    let report = Engine::new(&fake)
        .validate(&cfg, cfg.group_by_id(DESKTOP_GROUP).unwrap())
        .unwrap();
    assert_eq!(report.would_use, Some(Strategy::FullSupplied));
}

#[test]
fn last_known_good_survives_a_luid_change() {
    let fake = rig();
    let cfg = rig_config();
    let engine = Engine::new(&fake);
    let snapshot = Snapshot::now(fake.query_active());
    engine
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();

    let rebooted = crate::ccd::Luid {
        low: 99_001,
        high: 0,
    };
    fake.change_adapter_luid(rebooted);
    let method = engine.recover(Some(&snapshot), false).unwrap();

    assert_eq!(method, RecoverMethod::LastKnownGood);
    assert_eq!(fake.active_names(), ["DFI27280", "XG32UCWMG"]);
    let last = fake.calls().pop().unwrap();
    assert!(last
        .paths
        .iter()
        .all(|p| p.target.adapter == rebooted && p.source.adapter == rebooted));
}

#[test]
fn recover_without_snapshot_extends_everything() {
    let fake = rig();
    let method = Engine::new(&fake).recover(None, false).unwrap();
    assert_eq!(method, RecoverMethod::Extend);
    assert_eq!(fake.active_names(), ["DFI27280", "SAMSUNG", "XG32UCWMG"]);
}

#[test]
fn audio_waits_for_the_tv_endpoint_and_sets_volume() {
    let fake = rig();
    let audio = FakeAudio::default();
    audio.endpoints.borrow_mut().push(AudioEndpoint {
        id: "astro".into(),
        name: "Headphones (Astro A50 Game)".into(),
    });
    audio.pending.borrow_mut().push((
        AudioEndpoint {
            id: "tv".into(),
            name: "SAMSUNG (NVIDIA High Definition Audio)".into(),
        },
        2,
    ));

    let mut cfg = rig_config();
    let couch = cfg.group_by_id_mut(COUCH_GROUP).unwrap();
    couch.audio.enabled = true;
    couch.audio.endpoint = Some(EndpointMatch {
        name_contains: "samsung".into(),
    });
    couch.audio.volume_percent = Some(40);

    let report = Engine::new(&fake)
        .with_audio(&audio)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();
    assert_eq!(audio.default.borrow().as_deref(), Some("tv"));
    assert_eq!(audio.volumes.borrow().get("tv"), Some(&40));
    assert!(report.notes.iter().any(|n| n.contains("volume")));
}

#[test]
fn hdr_is_switched_only_when_needed() {
    let fake = rig();
    let mut cfg = rig_config();
    cfg.group_by_id_mut(COUCH_GROUP).unwrap().displays[0].hdr = Some(true);
    Engine::new(&fake)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();
    assert_eq!(fake.state.borrow().hdr_sets, [(TV, true)]);
}

#[test]
fn lock_screen_broker_skips_per_user_fixups() {
    let fake = rig();
    let cfg = rig_config();
    let options = crate::EngineOptions {
        user_context: false,
        ..Default::default()
    };
    Engine::new(&fake)
        .with_options(options)
        .apply(&cfg, cfg.group_by_id(COUCH_GROUP).unwrap(), &no_confirm())
        .unwrap();
    assert!(fake.state.borrow().dpi_sets.is_empty());
    assert_eq!(fake.dpi_of(TV).unwrap().current_percent(), Some(300));
}

#[test]
fn leave_policy_keeps_other_displays_with_their_modes() {
    let fake = rig();
    let mut cfg = rig_config();
    let couch = cfg.group_by_id_mut(COUCH_GROUP).unwrap();
    couch.others_policy = OthersPolicy::Leave;
    couch.displays[0].position = dg_model::Position { x: 3840, y: 0 };
    let couch = cfg.group_by_id(COUCH_GROUP).unwrap().clone();

    let all = fake.query_all();
    let active = fake.query_active();
    let resolved = resolve_group(&cfg, &couch, &all).unwrap();
    let placements = plan::placements(&cfg, &couch, &resolved, &all, &active).unwrap();
    assert_eq!(placements.len(), 3);
    let kept: Vec<_> = placements
        .iter()
        .filter(|p| p.display_index.is_none())
        .collect();
    assert_eq!(kept.len(), 2);
    // The TV must not take a source the kept displays already use.
    let tv = placements.iter().find(|p| p.target == TV).unwrap();
    assert!(kept.iter().all(|k| k.source_id != tv.source_id));

    let s2 = plan::build(Strategy::SourceModes, &placements).unwrap();
    plan::check_invariants(&s2).unwrap();
}

#[test]
fn invariants_catch_the_known_87_and_31_causes() {
    let fake = rig();
    let cfg = rig_config();
    let couch = cfg.group_by_id(COUCH_GROUP).unwrap();
    let all = fake.query_all();
    let active = fake.query_active();
    let resolved = resolve_group(&cfg, couch, &all).unwrap();
    let placements = plan::placements(&cfg, couch, &resolved, &all, &active).unwrap();

    for strategy in Strategy::LADDER.into_iter().skip(1) {
        let p = plan::build(strategy, &placements).unwrap();
        plan::check_invariants(&p).unwrap();
    }
    let mut bad = plan::build(Strategy::PathsOnly, &placements).unwrap();
    bad.paths[0].target.rotation = 0;
    assert!(plan::check_invariants(&bad)
        .unwrap_err()
        .contains("rotation"));

    let mut bad = plan::build(Strategy::PathsOnly, &placements).unwrap();
    bad.flags |= sdc::ALLOW_PATH_ORDER_CHANGES;
    assert!(plan::check_invariants(&bad).unwrap_err().contains("87"));

    let mut bad = plan::build(Strategy::WindowsDatabase, &placements).unwrap();
    bad.flags &= !sdc::ALLOW_PATH_ORDER_CHANGES;
    assert!(plan::check_invariants(&bad).unwrap_err().contains("31"));
}

#[test]
fn status_maps_screens_to_config_keys() {
    let fake = rig();
    let status = Engine::new(&fake).status(&rig_config()).unwrap();
    let primary = status.displays.iter().find(|d| d.primary).unwrap();
    assert_eq!(primary.monitor_key.as_deref(), Some(ASUS_KEY));
    assert_eq!(status.connected.len(), 3);
    assert!(status
        .connected
        .iter()
        .any(|c| c.monitor_key.as_deref() == Some(TV_KEY) && !c.active));
    assert!(status.connected.iter().all(|c| c.new_monitor.is_none()));
}

#[test]
fn status_offers_monitors_not_in_the_config() {
    let fake = rig();
    let mut cfg = rig_config();
    // The TV was never captured, and its usual key belongs to a monitor that is not
    // connected.
    let mut absent = cfg.monitors[ASUS_KEY].clone();
    absent.identity.pnp_id = "XXX".into();
    cfg.monitors.insert(TV_KEY.into(), absent);
    let status = Engine::new(&fake).status(&cfg).unwrap();
    let fresh: Vec<_> = status
        .connected
        .iter()
        .filter_map(|c| c.new_monitor.as_ref().map(|n| (c, n)))
        .collect();
    assert_eq!(fresh.len(), 1, "only the TV is unknown");
    let (connected, new) = fresh[0];
    assert!(connected.monitor_key.is_none() && !connected.active);
    assert_eq!(new.entry.identity.pnp_id, "SAM");
    assert_ne!(
        new.key, TV_KEY,
        "the key must not collide with an existing one"
    );
    assert!(new.key.starts_with(TV_KEY));
    // Adding the offered entry makes the monitor known.
    cfg.monitors.insert(new.key.clone(), new.entry.clone());
    let status = Engine::new(&fake).status(&cfg).unwrap();
    assert!(status
        .connected
        .iter()
        .any(|c| c.monitor_key.as_deref() == Some(new.key.as_str())));
}

impl FakeBackend {
    fn query_all(&self) -> crate::ccd::Topology {
        crate::DisplayBackend::query(self, Scope::All).unwrap()
    }
    fn query_active(&self) -> crate::ccd::Topology {
        crate::DisplayBackend::query(self, Scope::Active).unwrap()
    }
}

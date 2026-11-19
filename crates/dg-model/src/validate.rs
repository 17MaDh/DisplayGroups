//! Semantic validation of a [`Config`]. Problems are reported, never silently fixed.

use std::collections::HashSet;

use serde::Serialize;
use uuid::Uuid;

use crate::config::{
    names_equal, Action, Config, Group, HotkeyAction, TriggerWhen, MAX_CONFIRM_SECONDS,
    MAX_REVERT_DELAY_MS, SCALE_STEPS,
};
use crate::geometry::{is_connected, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The affected group or setting cannot be used until fixed.
    Error,
    /// Works, but probably not what the user meant (Windows may adjust it).
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    /// JSON-ish location, e.g. `groups[1].displays[0].scalePercent`.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{level}: {}: {}", self.path, self.message)
    }
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// Issues located inside `groups[index]`.
pub fn issues_for_group(issues: &[Issue], index: usize) -> Vec<&Issue> {
    let prefix = format!("groups[{index}]");
    issues
        .iter()
        .filter(|i| match i.path.strip_prefix(&prefix) {
            Some(rest) => rest.is_empty() || rest.starts_with('.') || rest.starts_with('['),
            None => false,
        })
        .collect()
}

#[derive(Default)]
struct Sink(Vec<Issue>);

impl Sink {
    fn error(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(Severity::Error, path, message);
    }

    fn warning(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(Severity::Warning, path, message);
    }

    fn push(&mut self, severity: Severity, path: impl Into<String>, message: impl Into<String>) {
        self.0.push(Issue {
            severity,
            path: path.into(),
            message: message.into(),
        });
    }
}

pub fn validate(cfg: &Config) -> Vec<Issue> {
    let mut sink = Sink::default();
    let group_ids: HashSet<Uuid> = cfg.groups.iter().map(|g| g.id).collect();

    let mut seen_ids = HashSet::new();
    for (gi, g) in cfg.groups.iter().enumerate() {
        let gp = format!("groups[{gi}]");
        if !seen_ids.insert(g.id) {
            sink.error(format!("{gp}.id"), format!("duplicate group id {}", g.id));
        }
        if g.name.trim().is_empty() {
            sink.error(format!("{gp}.name"), "group name is empty");
        } else if cfg.groups[..gi]
            .iter()
            .any(|other| names_equal(&other.name, &g.name))
        {
            sink.error(
                format!("{gp}.name"),
                format!("another group is already named \"{}\"", g.name),
            );
        }
        validate_group(cfg, g, &gp, &mut sink);
    }

    for (hi, h) in cfg.hotkeys.iter().enumerate() {
        let hp = format!("hotkeys[{hi}]");
        if h.chord.trim().is_empty() {
            sink.error(format!("{hp}.chord"), "hotkey chord is empty");
        }
        let (ids, min): (&[Uuid], usize) = match &h.action {
            HotkeyAction::Apply { group } => (std::slice::from_ref(group), 1),
            HotkeyAction::Toggle { groups } | HotkeyAction::Cycle { groups } => (groups, 2),
            HotkeyAction::Recover => (&[], 0),
        };
        if ids.len() < min {
            sink.error(
                format!("{hp}.action"),
                format!("needs at least {min} groups"),
            );
        }
        for id in ids.iter().filter(|id| !group_ids.contains(id)) {
            sink.error(format!("{hp}.action"), format!("unknown group id {id}"));
        }
    }

    let mut trigger_ids = HashSet::new();
    for (ti, tr) in cfg.triggers.iter().enumerate() {
        let tp = format!("triggers[{ti}]");
        if !trigger_ids.insert(tr.id) {
            sink.error(
                format!("{tp}.id"),
                format!("duplicate trigger id {}", tr.id),
            );
        }
        if !group_ids.contains(&tr.group) {
            sink.error(
                format!("{tp}.group"),
                format!("unknown group id {}", tr.group),
            );
        }
        if tr.revert_delay_ms > MAX_REVERT_DELAY_MS {
            sink.error(
                format!("{tp}.revertDelayMs"),
                format!("must be 0..={MAX_REVERT_DELAY_MS}"),
            );
        }
        if let TriggerWhen::Program {
            names,
            window_title,
        } = &tr.when
        {
            if names.iter().all(|n| n.trim().is_empty()) {
                sink.error(format!("{tp}.when.names"), "no program names");
            }
            for name in names.iter().filter(|n| !n.trim().is_empty()) {
                if name.contains(['\\', '/', ':']) {
                    sink.error(
                        format!("{tp}.when.names"),
                        format!("\"{name}\" must be a file name such as game.exe, not a path"),
                    );
                } else if !name.trim().to_ascii_lowercase().ends_with(".exe") {
                    sink.warning(
                        format!("{tp}.when.names"),
                        format!("\"{name}\" does not end in .exe"),
                    );
                }
            }
            if window_title.as_deref().is_some_and(|t| t.trim().is_empty()) {
                sink.warning(
                    format!("{tp}.when.windowTitle"),
                    "the window title filter is empty and is ignored",
                );
            }
        }
    }

    let fb = &cfg.settings.disconnect_fallback;
    if fb.enabled {
        let path = "settings.disconnectFallback.group";
        match fb.group {
            None => sink.error(path, "fallback is enabled but no group is chosen"),
            Some(id) if !group_ids.contains(&id) => {
                sink.error(path, format!("unknown group id {id}"))
            }
            Some(_) => {}
        }
    }

    sink.0
}

fn validate_group(cfg: &Config, g: &Group, gp: &str, sink: &mut Sink) {
    let mut monitors_seen = HashSet::new();
    for (di, d) in g.displays.iter().enumerate() {
        let dp = format!("{gp}.displays[{di}]");
        if !cfg.monitors.contains_key(&d.monitor) {
            sink.error(
                format!("{dp}.monitor"),
                format!("unknown monitor \"{}\"", d.monitor),
            );
        }
        if !monitors_seen.insert(d.monitor.as_str()) {
            sink.error(
                format!("{dp}.monitor"),
                format!("monitor \"{}\" is listed twice", d.monitor),
            );
        }
        if d.mode.width == 0 || d.mode.height == 0 {
            sink.error(format!("{dp}.mode"), "width and height must be > 0");
        }
        if d.mode.refresh_milli_hz == 0 {
            sink.error(
                format!("{dp}.mode.refreshMilliHz"),
                "refresh rate must be > 0",
            );
        }
        if let Some(s) = d.scale_percent {
            if !SCALE_STEPS.contains(&s) {
                sink.error(
                    format!("{dp}.scalePercent"),
                    format!("{s}% is not a Windows scale step ({SCALE_STEPS:?})"),
                );
            }
        }
    }

    let enabled: Vec<_> = g.enabled_displays().collect();
    if enabled.is_empty() {
        sink.error(
            format!("{gp}.displays"),
            "a group needs at least one enabled display",
        );
    } else {
        let at_origin = enabled
            .iter()
            .filter(|d| d.position.x == 0 && d.position.y == 0)
            .count();
        if at_origin != 1 {
            sink.error(
                format!("{gp}.displays"),
                format!(
                    "exactly one enabled display must be at (0,0), the primary; found {at_origin}"
                ),
            );
        }
        let rects: Vec<Rect> = enabled.iter().map(|d| d.rect()).collect();
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                if rects[i].overlaps(&rects[j]) {
                    sink.error(
                        format!("{gp}.displays"),
                        format!(
                            "\"{}\" and \"{}\" overlap",
                            enabled[i].monitor, enabled[j].monitor
                        ),
                    );
                }
            }
        }
        if !is_connected(&rects) {
            sink.warning(
                format!("{gp}.displays"),
                "some displays do not touch the others; Windows will move them when applying",
            );
        }
    }

    if let Some(v) = g.audio.volume_percent {
        if v > 100 {
            sink.error(
                format!("{gp}.audio.volumePercent"),
                format!("{v} is outside 0..=100"),
            );
        }
    }
    if g.audio.enabled && g.audio.endpoint.is_none() && g.audio.volume_percent.is_none() {
        sink.warning(
            format!("{gp}.audio"),
            "audio is enabled but neither an endpoint nor a volume is set",
        );
    }
    if let Some(m) = &g.audio.endpoint {
        if m.name_contains.trim().is_empty() {
            sink.error(
                format!("{gp}.audio.endpoint.nameContains"),
                "endpoint name filter is empty",
            );
        }
    }

    for (list, name) in [(&g.actions.before, "before"), (&g.actions.after, "after")] {
        for (ai, a) in list.iter().enumerate() {
            let ap = format!("{gp}.actions.{name}[{ai}]");
            match a {
                Action::Program { path, .. } if path.trim().is_empty() => {
                    sink.error(ap, "program path is empty")
                }
                Action::Open { target } if target.trim().is_empty() => {
                    sink.error(ap, "open target is empty")
                }
                _ => {}
            }
        }
    }

    if g.safety.confirm_seconds > MAX_CONFIRM_SECONDS {
        sink.error(
            format!("{gp}.safety.confirmSeconds"),
            format!("must be 0..={MAX_CONFIRM_SECONDS}"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;

    fn monitor(cfg: &mut Config, key: &str) {
        cfg.monitors.insert(
            key.into(),
            MonitorEntry {
                display_name: key.into(),
                identity: MonitorIdentity {
                    pnp_id: "AAA".into(),
                    product_code: "0001".into(),
                    ..Default::default()
                },
                extra: Default::default(),
            },
        );
    }

    fn display(key: &str, x: i32, y: i32, w: u32, h: u32) -> DisplaySetting {
        DisplaySetting {
            monitor: key.into(),
            enabled: true,
            mode: Mode {
                width: w,
                height: h,
                refresh_milli_hz: 60_000,
                refresh_policy: RefreshPolicy::BestAtOrBelow,
            },
            position: Position { x, y },
            rotation: Rotation::Landscape,
            scale_percent: None,
            hdr: None,
            captured: None,
            extra: Default::default(),
        }
    }

    fn base() -> Config {
        let mut cfg = Config::default();
        monitor(&mut cfg, "a");
        monitor(&mut cfg, "b");
        let mut g = Group::new("Desk");
        g.displays.push(display("a", 0, 0, 3840, 2160));
        g.displays.push(display("b", -1920, 457, 1920, 1080));
        cfg.groups.push(g);
        cfg
    }

    #[test]
    fn valid_config_has_no_issues() {
        let issues = validate(&base());
        assert!(issues.is_empty(), "{issues:?}");
    }

    fn trigger(group: Uuid, when: TriggerWhen) -> Trigger {
        Trigger {
            id: Uuid::new_v4(),
            enabled: true,
            when,
            group,
            switch_back: true,
            revert_delay_ms: DEFAULT_REVERT_DELAY_MS,
            confirm: false,
            extra: Default::default(),
        }
    }

    #[test]
    fn triggers_are_checked() {
        let mut cfg = base();
        let desk = cfg.groups[0].id;
        cfg.triggers.push(trigger(
            desk,
            TriggerWhen::Program {
                names: vec!["EldenRing.exe".into()],
                window_title: None,
            },
        ));
        cfg.triggers.push(trigger(desk, TriggerWhen::Controller));
        assert!(validate(&cfg).is_empty(), "{:?}", validate(&cfg));

        let mut bad = cfg.clone();
        bad.triggers[0].group = Uuid::new_v4();
        bad.triggers[0].revert_delay_ms = MAX_REVERT_DELAY_MS + 1;
        bad.triggers[1].id = bad.triggers[0].id;
        let issues = validate(&bad);
        for needle in ["unknown group", "revertDelayMs", "duplicate trigger"] {
            assert!(
                issues
                    .iter()
                    .any(|i| i.message.contains(needle) || i.path.contains(needle)),
                "{needle}: {issues:?}"
            );
        }

        let mut paths = cfg.clone();
        paths.triggers[0].when = TriggerWhen::Program {
            names: vec![r"C:\Games\game.exe".into(), "steam".into()],
            window_title: Some(" ".into()),
        };
        let issues = validate(&paths);
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("not a path")));
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains(".exe")));
        assert!(issues.iter().any(|i| i.path.ends_with("windowTitle")));

        paths.triggers[0].when = TriggerWhen::Program {
            names: vec!["  ".into()],
            window_title: None,
        };
        assert!(validate(&paths)
            .iter()
            .any(|i| i.message == "no program names"));
    }

    #[test]
    fn triggers_round_trip_with_defaults() {
        let json = r#"{"id":"6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60","when":{"type":"controller"},
                       "group":"6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f61","future":1}"#;
        let t: Trigger = serde_json::from_str(json).unwrap();
        assert!(t.enabled && t.switch_back && !t.confirm);
        assert_eq!(t.revert_delay_ms, DEFAULT_REVERT_DELAY_MS);
        assert_eq!(t.when, TriggerWhen::Controller);
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["future"], 1);
        assert_eq!(back["switchBack"], true);
        let program: TriggerWhen = serde_json::from_str(
            r#"{"type":"program","names":["steam.exe"],"windowTitle":"Big Picture"}"#,
        )
        .unwrap();
        assert_eq!(
            program,
            TriggerWhen::Program {
                names: vec!["steam.exe".into()],
                window_title: Some("Big Picture".into())
            }
        );
    }

    #[test]
    fn no_primary_is_an_error() {
        let mut cfg = base();
        cfg.groups[0].displays[0].position = Position { x: 10, y: 0 };
        let issues = validate(&cfg);
        assert!(has_errors(&issues));
        assert!(issues.iter().any(|i| i.message.contains("(0,0)")));
    }

    #[test]
    fn overlap_is_error_gap_is_warning() {
        let mut cfg = base();
        cfg.groups[0].displays[1].position = Position { x: -1000, y: 0 };
        assert!(validate(&cfg)
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("overlap")));

        let mut cfg = base();
        cfg.groups[0].displays[1].position = Position { x: -3000, y: 0 };
        let issues = validate(&cfg);
        assert!(!has_errors(&issues));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].path, "groups[0].displays");
    }

    #[test]
    fn portrait_rotation_changes_footprint() {
        let mut cfg = base();
        // Portrait 1920x1080 is 1080 wide, so it no longer reaches x=0 from -1920.
        cfg.groups[0].displays[1].rotation = Rotation::Portrait;
        let issues = validate(&cfg);
        assert!(issues.iter().any(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn bad_scale_volume_and_references() {
        let mut cfg = base();
        cfg.groups[0].displays[0].scale_percent = Some(130);
        cfg.groups[0].audio.volume_percent = Some(101);
        cfg.groups[0].displays[1].monitor = "nope".into();
        cfg.hotkeys.push(Hotkey {
            chord: "Ctrl+Alt+F12".into(),
            action: HotkeyAction::Toggle {
                groups: vec![cfg.groups[0].id],
            },
            on_lock_screen: false,
            extra: Default::default(),
        });
        let issues = validate(&cfg);
        let paths: Vec<&str> = issues.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"groups[0].displays[0].scalePercent"));
        assert!(paths.contains(&"groups[0].audio.volumePercent"));
        assert!(paths.contains(&"groups[0].displays[1].monitor"));
        assert!(paths.contains(&"hotkeys[0].action"));
    }

    #[test]
    fn duplicate_names_are_case_insensitive() {
        let mut cfg = base();
        let mut g = cfg.groups[0].clone();
        g.id = Uuid::new_v4();
        g.name = "desk".into();
        cfg.groups.push(g);
        assert!(validate(&cfg).iter().any(|i| i.path == "groups[1].name"));
    }

    #[test]
    fn issues_for_group_filters_by_prefix() {
        let mut cfg = base();
        cfg.groups[0].audio.volume_percent = Some(200);
        let issues = validate(&cfg);
        assert_eq!(issues_for_group(&issues, 0).len(), 1);
        assert!(issues_for_group(&issues, 1).is_empty());
    }
}

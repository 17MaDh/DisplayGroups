//! Automatic switching (ARCHITECTURE.md §15.1): decides, from what runs and what is
//! connected, when a trigger switches to its group and when it switches back.
//!
//! Pure logic. The agent samples Windows into an [`Observation`] and calls
//! [`Triggers::evaluate`]; the tests drive it with made-up observations and clocks.
//!
//! Rules:
//! - Triggers react to their condition *changing*. A trigger that is new, edited, or
//!   seen for the first time takes the condition as it is without switching (Steam
//!   already running at sign-in does not switch).
//! - After firing, a trigger waits for its condition to end before it can fire again,
//!   so a failed or reverted switch is not retried in a loop.
//! - Switching back needs the condition to stay ended for `revertDelayMs`, the screen
//!   to still show the trigger's group, and no other active trigger for that group.

use std::collections::{HashMap, HashSet};

use dg_model::{Config, Trigger, TriggerWhen};
use uuid::Uuid;

/// What the agent saw at one moment.
#[derive(Debug, Default, Clone)]
pub struct Observation {
    /// Lower-case executable names of running processes, e.g. `steam.exe`.
    pub programs: HashSet<String>,
    /// Lower-case executable name and title of visible top-level windows. Only filled
    /// when a trigger has a window-title filter.
    pub windows: Vec<(String, String)>,
    /// A game controller is connected.
    pub controller: bool,
    /// Milliseconds since any fixed point (`GetTickCount64` in the agent).
    pub now_ms: u64,
}

/// What an [`Observation`] has to include for the enabled triggers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Needs {
    pub programs: bool,
    pub windows: bool,
    pub controller: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// `trigger` started: switch to `group`.
    SwitchTo { trigger: Uuid, group: Uuid },
    /// `trigger` ended: switch back to `group`.
    SwitchBack { trigger: Uuid, group: Uuid },
}

impl Decision {
    pub fn group(&self) -> Uuid {
        match self {
            Decision::SwitchTo { group, .. } | Decision::SwitchBack { group, .. } => *group,
        }
    }

    pub fn trigger(&self) -> Uuid {
        match self {
            Decision::SwitchTo { trigger, .. } | Decision::SwitchBack { trigger, .. } => *trigger,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct State {
    /// The condition as last acted on.
    active: bool,
    /// When the condition was first seen ended while `active`.
    ended_at: Option<u64>,
    /// The group to return to, set when this trigger switched (and `switchBack` is on).
    return_to: Option<Uuid>,
    /// What the trigger watches and where it switches; an edit re-primes it.
    fingerprint: String,
}

#[derive(Debug, Default)]
pub struct Triggers {
    states: HashMap<Uuid, State>,
}

/// Enabled triggers whose group exists, in config order.
fn enabled(cfg: &Config) -> impl Iterator<Item = &Trigger> {
    cfg.triggers
        .iter()
        .filter(|t| t.enabled && cfg.group_by_id(t.group).is_some())
}

impl Triggers {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the agent must sample for `cfg`'s enabled triggers.
    pub fn needs(cfg: &Config) -> Needs {
        let mut needs = Needs::default();
        for t in enabled(cfg) {
            match &t.when {
                TriggerWhen::Controller => needs.controller = true,
                TriggerWhen::Program { window_title, .. } => {
                    needs.programs = true;
                    if title_filter(window_title.as_deref()).is_some() {
                        needs.windows = true;
                    }
                }
            }
        }
        needs
    }

    /// Evaluates the enabled triggers in config order and returns at most one decision.
    /// `current` gives the group on screen now; it is only called on an edge.
    pub fn evaluate(
        &mut self,
        cfg: &Config,
        obs: &Observation,
        current: &mut dyn FnMut() -> Option<Uuid>,
    ) -> Option<Decision> {
        let triggers: Vec<&Trigger> = enabled(cfg).collect();
        self.states
            .retain(|id, _| triggers.iter().any(|t| t.id == *id));
        let mut on_screen: Option<Option<Uuid>> = None;
        let mut current = || *on_screen.get_or_insert_with(&mut *current);

        for t in &triggers {
            let holds = holds(t, obs);
            let fingerprint = fingerprint(t);
            let state = match self.states.get_mut(&t.id) {
                Some(s) if s.fingerprint == fingerprint => s,
                _ => {
                    self.states.insert(
                        t.id,
                        State {
                            active: holds,
                            fingerprint,
                            ..State::default()
                        },
                    );
                    continue;
                }
            };

            if holds {
                state.ended_at = None;
                if state.active {
                    continue;
                }
                state.active = true;
                let now = current();
                if now == Some(t.group) {
                    // Already there: nothing to switch, nothing to undo later.
                    state.return_to = None;
                    continue;
                }
                state.return_to = if t.switch_back { now } else { None };
                return Some(Decision::SwitchTo {
                    trigger: t.id,
                    group: t.group,
                });
            }

            if !state.active {
                continue;
            }
            let since = *state.ended_at.get_or_insert(obs.now_ms);
            if obs.now_ms.saturating_sub(since) < u64::from(t.revert_delay_ms) {
                continue;
            }
            state.active = false;
            state.ended_at = None;
            let Some(back) = state.return_to.take() else {
                continue;
            };
            if !t.switch_back || cfg.group_by_id(back).is_none() {
                continue;
            }
            // Another trigger for the same group is still on: it inherits the way back.
            let heir = triggers.iter().find(|o| {
                o.id != t.id
                    && o.group == t.group
                    && self.states.get(&o.id).is_some_and(|s| s.active)
            });
            if let Some(heir) = heir {
                if let Some(s) = self.states.get_mut(&heir.id) {
                    s.return_to.get_or_insert(back);
                }
                continue;
            }
            if current() != Some(t.group) {
                // Switched away by hand meanwhile: leave the screen alone.
                continue;
            }
            return Some(Decision::SwitchBack {
                trigger: t.id,
                group: back,
            });
        }
        None
    }
}

fn fingerprint(t: &Trigger) -> String {
    format!("{:?}|{}", t.when, t.group)
}

fn title_filter(title: Option<&str>) -> Option<String> {
    title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
}

/// Lower-case executable names, with `.exe` added when missing (`steam` → `steam.exe`).
fn program_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .map(|n| {
            if n.ends_with(".exe") {
                n
            } else {
                format!("{n}.exe")
            }
        })
        .collect()
}

fn holds(t: &Trigger, obs: &Observation) -> bool {
    match &t.when {
        TriggerWhen::Controller => obs.controller,
        TriggerWhen::Program {
            names,
            window_title,
        } => {
            let names = program_names(names);
            match title_filter(window_title.as_deref()) {
                None => names.iter().any(|n| obs.programs.contains(n)),
                Some(title) => obs
                    .windows
                    .iter()
                    .any(|(exe, text)| names.contains(exe) && text.to_lowercase().contains(&title)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dg_model::{Group, DEFAULT_REVERT_DELAY_MS};

    struct Rig {
        cfg: Config,
        desk: Uuid,
        tv: Uuid,
        triggers: Triggers,
        screen: Option<Uuid>,
        obs: Observation,
    }

    impl Rig {
        fn new(whens: Vec<TriggerWhen>) -> Self {
            let mut cfg = Config::default();
            let desk = Group::new("Desk");
            let tv = Group::new("TV");
            let (desk_id, tv_id) = (desk.id, tv.id);
            cfg.groups.push(desk);
            cfg.groups.push(tv);
            for when in whens {
                cfg.triggers.push(Trigger {
                    id: Uuid::new_v4(),
                    enabled: true,
                    when,
                    group: tv_id,
                    switch_back: true,
                    revert_delay_ms: DEFAULT_REVERT_DELAY_MS,
                    confirm: false,
                    extra: Default::default(),
                });
            }
            Self {
                cfg,
                desk: desk_id,
                tv: tv_id,
                triggers: Triggers::new(),
                screen: Some(desk_id),
                obs: Observation::default(),
            }
        }

        /// One evaluation; a decision is carried out (the screen follows it).
        fn step(&mut self) -> Option<Decision> {
            let screen = self.screen;
            let d = self.triggers.evaluate(&self.cfg, &self.obs, &mut || screen);
            if let Some(d) = d {
                self.screen = Some(d.group());
            }
            d
        }

        fn run(&mut self, exe: &str) {
            self.obs.programs.insert(exe.to_string());
        }

        fn quit(&mut self, exe: &str) {
            self.obs.programs.remove(exe);
        }

        fn wait(&mut self, ms: u64) {
            self.obs.now_ms += ms;
        }
    }

    fn program(names: &[&str]) -> TriggerWhen {
        TriggerWhen::Program {
            names: names.iter().map(|s| s.to_string()).collect(),
            window_title: None,
        }
    }

    #[test]
    fn switches_on_start_and_back_after_the_delay() {
        let mut r = Rig::new(vec![program(&["EldenRing.exe"])]);
        assert_eq!(r.step(), None, "first look only primes");
        r.run("eldenring.exe");
        assert!(matches!(r.step(), Some(Decision::SwitchTo { group, .. }) if group == r.tv));
        assert_eq!(r.step(), None, "no repeat while running");
        r.quit("eldenring.exe");
        assert_eq!(r.step(), None, "the delay starts");
        r.wait(DEFAULT_REVERT_DELAY_MS as u64 - 1);
        assert_eq!(r.step(), None);
        r.wait(1);
        assert!(matches!(r.step(), Some(Decision::SwitchBack { group, .. }) if group == r.desk));
        assert_eq!(r.step(), None);
    }

    #[test]
    fn already_running_at_start_does_not_switch() {
        let mut r = Rig::new(vec![program(&["steam.exe"])]);
        r.run("steam.exe");
        assert_eq!(r.step(), None);
        assert_eq!(r.step(), None);
        r.quit("steam.exe");
        assert_eq!(r.step(), None, "the delay starts when the exit is seen");
        r.wait(10_000);
        assert_eq!(r.step(), None, "nothing to switch back from");
        r.run("steam.exe");
        assert!(matches!(r.step(), Some(Decision::SwitchTo { .. })));
    }

    #[test]
    fn a_restart_within_the_delay_keeps_the_group() {
        let mut r = Rig::new(vec![program(&["launcher.exe", "game.exe"])]);
        r.step();
        r.run("launcher.exe");
        assert!(r.step().is_some());
        r.quit("launcher.exe");
        r.step();
        r.wait(2000);
        r.run("game.exe");
        assert_eq!(r.step(), None);
        r.quit("game.exe");
        r.step();
        r.wait(4000);
        assert_eq!(r.step(), None, "the delay restarted when the game quit");
        r.wait(1000);
        assert!(matches!(r.step(), Some(Decision::SwitchBack { .. })));
    }

    #[test]
    fn no_switch_back_after_a_manual_switch_or_when_off() {
        let mut r = Rig::new(vec![program(&["game.exe"])]);
        r.step();
        r.run("game.exe");
        r.step();
        let other = Group::new("Other");
        r.screen = Some(other.id);
        r.quit("game.exe");
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert_eq!(r.step(), None);

        let mut r = Rig::new(vec![program(&["game.exe"])]);
        r.cfg.triggers[0].switch_back = false;
        r.step();
        r.run("game.exe");
        assert!(r.step().is_some());
        r.quit("game.exe");
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert_eq!(r.step(), None);
    }

    #[test]
    fn already_on_the_group_is_not_undone() {
        let mut r = Rig::new(vec![program(&["game.exe"])]);
        r.screen = Some(r.tv);
        r.step();
        r.run("game.exe");
        assert_eq!(r.step(), None);
        r.quit("game.exe");
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert_eq!(r.step(), None);
        assert_eq!(r.screen, Some(r.tv));
    }

    #[test]
    fn a_reverted_switch_is_not_retried() {
        let mut r = Rig::new(vec![program(&["game.exe"])]);
        r.step();
        r.run("game.exe");
        assert!(r.step().is_some());
        r.screen = Some(r.desk); // the user pressed "Revert"
        for _ in 0..5 {
            r.wait(2000);
            assert_eq!(r.step(), None);
        }
    }

    #[test]
    fn editing_a_trigger_reprimes_it() {
        let mut r = Rig::new(vec![program(&["a.exe"])]);
        r.step();
        r.run("b.exe");
        assert_eq!(r.step(), None);
        r.cfg.triggers[0].when = program(&["b.exe"]);
        assert_eq!(
            r.step(),
            None,
            "b.exe was already running when the trigger changed"
        );
        // Changing only the delay keeps the state.
        r.cfg.triggers[0].revert_delay_ms = 0;
        r.quit("b.exe");
        assert_eq!(r.step(), None);
        r.run("b.exe");
        assert!(r.step().is_some());
    }

    #[test]
    fn controller_and_window_title() {
        let mut r = Rig::new(vec![TriggerWhen::Controller]);
        r.step();
        r.obs.controller = true;
        assert!(matches!(r.step(), Some(Decision::SwitchTo { .. })));
        r.obs.controller = false;
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert!(matches!(r.step(), Some(Decision::SwitchBack { .. })));

        let mut r = Rig::new(vec![TriggerWhen::Program {
            names: vec!["steam".into()],
            window_title: Some("Big Picture".into()),
        }]);
        assert_eq!(
            Triggers::needs(&r.cfg),
            Needs {
                programs: true,
                windows: true,
                controller: false
            }
        );
        r.step();
        r.run("steam.exe");
        r.obs.windows = vec![("steam.exe".into(), "Steam".into())];
        assert_eq!(r.step(), None, "Steam alone is not Big Picture");
        r.obs
            .windows
            .push(("steam.exe".into(), "Steam Big Picture Mode".into()));
        assert!(matches!(r.step(), Some(Decision::SwitchTo { .. })));
    }

    #[test]
    fn one_decision_at_a_time_and_a_shared_group_hands_over() {
        let mut r = Rig::new(vec![program(&["game.exe"]), TriggerWhen::Controller]);
        r.step();
        r.run("game.exe");
        r.obs.controller = true;
        let first = r.step().unwrap();
        assert_eq!(first.trigger(), r.cfg.triggers[0].id);
        assert_eq!(
            r.step(),
            None,
            "the controller trigger sees the TV already on"
        );
        r.quit("game.exe");
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert_eq!(r.step(), None, "the controller still wants the TV");
        r.obs.controller = false;
        r.step();
        r.wait(DEFAULT_REVERT_DELAY_MS as u64);
        assert!(
            matches!(r.step(), Some(Decision::SwitchBack { group, .. }) if group == r.desk),
            "the way back was handed over"
        );
    }

    #[test]
    fn disabled_and_dangling_triggers_are_ignored() {
        let mut r = Rig::new(vec![program(&["a.exe"]), program(&["b.exe"])]);
        r.cfg.triggers[0].enabled = false;
        r.cfg.triggers[1].group = Uuid::new_v4();
        assert_eq!(Triggers::needs(&r.cfg), Needs::default());
        r.step();
        r.run("a.exe");
        r.run("b.exe");
        assert_eq!(r.step(), None);
    }
}

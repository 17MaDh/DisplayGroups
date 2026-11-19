//! Mapping a group's monitors to the targets connected right now.

use std::collections::HashSet;

use dg_identity::{match_monitor, MatchError, MatchTier, Observed};
use dg_model::{Config, Group};

use crate::ccd::{TargetRef, Topology};
use crate::observe::observe;
use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// Index into `group.displays`.
    pub display_index: usize,
    pub target: TargetRef,
    pub tier: MatchTier,
}

/// Resolves every display of `group` against the available targets in `all`.
/// Enabled displays must be found; disabled ones are skipped if absent.
pub fn resolve_group(
    cfg: &Config,
    group: &Group,
    all: &Topology,
) -> Result<Vec<Resolved>, EngineError> {
    let available: Vec<_> = all.targets.iter().filter(|t| t.available).collect();
    let observed: Vec<Observed> = available.iter().map(|d| observe(d)).collect();

    let mut out = Vec::new();
    for (index, display) in group.displays.iter().enumerate() {
        let entry = cfg
            .monitors
            .get(&display.monitor)
            .ok_or_else(|| EngineError::UnknownMonitor(display.monitor.clone()))?;
        match match_monitor(&entry.identity, &observed) {
            Ok(m) => out.push(Resolved {
                display_index: index,
                target: available[m.index].target,
                tier: m.tier,
            }),
            Err(_) if !display.enabled => {}
            Err(MatchError::NotFound) => {
                return Err(EngineError::MonitorMissing {
                    name: entry.display_name.clone(),
                })
            }
            Err(MatchError::Ambiguous { .. }) => {
                return Err(EngineError::AmbiguousMonitor {
                    name: entry.display_name.clone(),
                })
            }
        }
    }

    let mut seen = HashSet::new();
    for r in &out {
        if !seen.insert(r.target) {
            let name = cfg
                .monitors
                .get(&group.displays[r.display_index].monitor)
                .map(|m| m.display_name.clone())
                .unwrap_or_default();
            return Err(EngineError::AmbiguousMonitor { name });
        }
    }
    Ok(out)
}

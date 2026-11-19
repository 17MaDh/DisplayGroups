//! Last-known-good layouts. Raw CCD arrays include adapter LUIDs and target ids that
//! change across reboots, so a saved layout is re-linked to today's ids by monitor
//! device path before it is applied.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ccd::{Luid, ModeInfo, TargetRef, Topology};
use crate::EngineError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub saved_at_unix: u64,
    /// Active paths and modes, plus the target details used for re-linking.
    pub topology: Topology,
}

impl Snapshot {
    pub fn now(topology: Topology) -> Self {
        let saved_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        Self {
            saved_at_unix,
            topology,
        }
    }
}

/// Rewrites `saved` so its LUIDs and target ids match `current_all`.
pub fn relink(saved: &Topology, current_all: &Topology) -> Result<Topology, EngineError> {
    let mut targets: HashMap<TargetRef, TargetRef> = HashMap::new();
    let mut adapters: HashMap<Luid, Luid> = HashMap::new();
    for path in saved.active_paths() {
        let old = path.target_ref();
        if targets.contains_key(&old) {
            continue;
        }
        let details = saved.target(old).ok_or_else(|| {
            EngineError::Unsupported("the saved layout has no monitor details".into())
        })?;
        let now = current_all
            .targets
            .iter()
            .find(|d| d.available && d.device_path.eq_ignore_ascii_case(&details.device_path))
            .ok_or_else(|| EngineError::MonitorMissing {
                name: details.friendly_name.clone(),
            })?;
        targets.insert(old, now.target);
        adapters.insert(old.adapter, now.target.adapter);
        adapters.insert(path.source.adapter, now.target.adapter);
    }
    let adapter = |a: Luid| adapters.get(&a).copied().unwrap_or(a);

    let paths = saved
        .active_paths()
        .map(|p| {
            let mut q = *p;
            let t = targets[&p.target_ref()];
            q.target.adapter = t.adapter;
            q.target.id = t.id;
            q.source.adapter = adapter(p.source.adapter);
            q
        })
        .collect();
    let modes = saved
        .modes
        .iter()
        .map(|m| match m.clone() {
            ModeInfo::Source {
                adapter: a,
                id,
                width,
                height,
                pixel_format,
                x,
                y,
            } => ModeInfo::Source {
                adapter: adapter(a),
                id,
                width,
                height,
                pixel_format,
                x,
                y,
            },
            ModeInfo::Target {
                adapter: a,
                id,
                signal,
            } => {
                let t = targets
                    .get(&TargetRef { adapter: a, id })
                    .copied()
                    .unwrap_or(TargetRef {
                        adapter: adapter(a),
                        id,
                    });
                ModeInfo::Target {
                    adapter: t.adapter,
                    id: t.id,
                    signal,
                }
            }
            ModeInfo::Other {
                info_type,
                adapter: a,
                id,
                raw,
            } => ModeInfo::Other {
                info_type,
                adapter: adapter(a),
                id,
                raw,
            },
        })
        .collect();
    let details = saved
        .targets
        .iter()
        .map(|d| {
            let mut d = d.clone();
            if let Some(t) = targets.get(&d.target) {
                d.target = *t;
            }
            d
        })
        .collect();
    Ok(Topology {
        paths,
        modes,
        targets: details,
    })
}

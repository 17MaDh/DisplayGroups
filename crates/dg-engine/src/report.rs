use serde::Serialize;
use uuid::Uuid;

use crate::ccd::Topology;
use crate::plan::Strategy;

/// One rung of the strategy ladder: the `SDC_VALIDATE` result and, if validation
/// passed, the `SDC_APPLY` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub strategy: Strategy,
    pub validate: i32,
    pub apply: Option<i32>,
}

impl Attempt {
    pub fn summary(attempts: &[Attempt]) -> String {
        attempts
            .iter()
            .map(|a| match a.apply {
                Some(code) => format!("{}: validate {}, apply {code}", a.strategy, a.validate),
                None => format!("{}: validate {}", a.strategy, a.validate),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// What the new primary display shows, for the CLI output and the OSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_milli_hz: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub group_id: Uuid,
    pub group_name: String,
    pub attempts: Vec<Attempt>,
    pub strategy: Option<Strategy>,
    /// Human-readable remarks: corrections made, things Windows refused, etc.
    pub notes: Vec<String>,
    /// False when the user (or the countdown) reverted the switch.
    pub kept: bool,
    pub primary: Option<PrimaryInfo>,
    #[serde(skip)]
    pub final_state: Topology,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateReport {
    pub group_id: Uuid,
    pub group_name: String,
    pub attempts: Vec<Attempt>,
    /// The first strategy Windows accepted; the one `apply` would use.
    pub would_use: Option<Strategy>,
}

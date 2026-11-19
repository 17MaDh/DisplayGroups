use crate::report::Attempt;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("\"{name}\" is not connected (or it is switched off and Windows cannot see it)")]
    MonitorMissing { name: String },

    #[error("more than one connected display matches \"{name}\"; capture the group again")]
    AmbiguousMonitor { name: String },

    #[error("the group uses monitor \"{0}\", which is not defined in the config")]
    UnknownMonitor(String),

    #[error("no free display output is available for \"{name}\"")]
    NoSourceAvailable { name: String },

    #[error("Windows rejected every way of applying this group ({})", Attempt::summary(.attempts))]
    AllStrategiesFailed { attempts: Vec<Attempt> },

    #[error(
        "Windows is locked or another desktop is active, so displays cannot be changed right now"
    )]
    Locked,

    #[error("{op} failed with Windows error {code}")]
    Win32 { op: &'static str, code: i32 },

    #[error("internal error: {0}")]
    InvalidPlan(String),

    #[error("{0}")]
    Unsupported(String),
}

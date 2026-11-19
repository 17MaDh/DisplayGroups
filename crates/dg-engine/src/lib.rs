//! The DisplayGroups switching engine (ARCHITECTURE.md §6).
//!
//! The engine never calls Win32 directly: everything goes through the
//! [`DisplayBackend`] and [`AudioBackend`] traits. `dg-win` implements them for real
//! hardware; [`fake`] implements them in memory so the whole strategy ladder,
//! including Windows' error 31/87 behaviour, is testable in CI.

pub mod backend;
pub mod ccd;
pub mod dpi;
mod engine;
mod error;
pub mod observe;
pub mod plan;
pub mod refresh;
mod report;
pub mod resolve;
pub mod snapshot;
pub mod triggers;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

#[cfg(test)]
mod tests;

pub use backend::{AudioBackend, AudioEndpoint, DeviceMode, DisplayBackend, HdrState, ModeChange};
pub use engine::{
    next_in_cycle, ActiveDisplay, ApplyOptions, Captured, Confirmer, ConnectedMonitor, Engine,
    EngineOptions, NewMonitor, PreferredMode, RecoverMethod, Status,
};
pub use error::EngineError;
pub use plan::Strategy;
pub use report::{ApplyReport, Attempt, PrimaryInfo, ValidateReport};
pub use snapshot::Snapshot;

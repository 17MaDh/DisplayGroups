//! DisplayGroups on GNOME (ARCHITECTURE.md §16).
//!
//! GNOME's compositor, Mutter, owns the display configuration on both Wayland and X11 and
//! exposes it on the session bus as `org.gnome.Mutter.DisplayConfig`:
//! `GetCurrentState` describes monitors, their modes and the logical layout, and
//! `ApplyMonitorsConfig` replaces the whole layout (verify, temporary or persistent;
//! a persistent change makes GNOME Shell ask "Keep these display settings?").
//!
//! * [`state`]: the current state as plain data (testable without a bus).
//! * [`ops`]: status, current group and capture, mapped onto the shared config model.
//! * [`plan`]: a group turned into the `ApplyMonitorsConfig` arguments.
//! * [`layout`]: converting between the config's pixel layout and Mutter's logical one.
//! * `dbus` (Linux only): the D-Bus client, plus EDIDs from `/sys/class/drm`.

pub mod layout;
pub mod observe;
pub mod ops;
pub mod plan;
pub mod state;

#[cfg(target_os = "linux")]
pub mod dbus;

pub use ops::{capture, current_group, status};
pub use plan::{extend_all, plan, ApplyConfig, LogicalConfig};
pub use state::{DisplayState, LayoutMode, LogicalMonitor, Monitor, MonitorMode};

#[derive(Debug, thiserror::Error)]
pub enum GnomeError {
    #[error("\"{name}\" is not connected (or it is switched off and GNOME cannot see it)")]
    MonitorMissing { name: String },

    #[error("more than one connected display matches \"{name}\"; capture the group again")]
    AmbiguousMonitor { name: String },

    #[error("the group uses monitor \"{0}\", which is not defined in the config")]
    UnknownMonitor(String),

    #[error("the group has no display that is turned on")]
    NothingToShow,

    #[error("GNOME rejected the layout: {0}")]
    Rejected(String),

    #[error(
        "the GNOME display service is not available ({0}). DisplayGroups needs a GNOME \
         session (Ubuntu's default desktop)"
    )]
    Unavailable(String),
}

#[cfg(test)]
mod tests;

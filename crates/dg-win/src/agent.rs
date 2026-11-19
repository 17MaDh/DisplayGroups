//! Telling the resident agent that a switch happened, so it can show the OSD.
//! The agent owns a hidden top-level window of class [`AGENT_WINDOW_CLASS`] and
//! listens for the registered message [`SWITCHED_MESSAGE`]; the details are in
//! `state\last-switch.json`.

use std::ptr::{null, null_mut};

use crate::ffi_ui::{FindWindowExW, PostMessageW, RegisterWindowMessageW};
use crate::util::wide;

pub const AGENT_WINDOW_CLASS: &str = "DisplayGroups.Agent";
pub const SWITCHED_MESSAGE: &str = "DisplayGroups.Switched";
/// Name of the agent's single-instance mutex.
pub const AGENT_MUTEX: &str = "Local\\DisplayGroups.Agent";

/// True when a DisplayGroups agent is running in this session.
pub fn is_running() -> bool {
    let class = wide(AGENT_WINDOW_CLASS);
    // SAFETY: lookup with a NUL-terminated class name.
    unsafe { !FindWindowExW(null_mut(), null_mut(), class.as_ptr(), null()).is_null() }
}

/// Returns false when no agent is running (then there is simply no OSD).
pub fn notify_switched() -> bool {
    let class = wide(AGENT_WINDOW_CLASS);
    let name = wide(SWITCHED_MESSAGE);
    // SAFETY: lookups with NUL-terminated strings; posting to a found window is harmless.
    unsafe {
        let hwnd = FindWindowExW(null_mut(), null_mut(), class.as_ptr(), null());
        if hwnd.is_null() {
            return false;
        }
        let msg = RegisterWindowMessageW(name.as_ptr());
        msg != 0 && PostMessageW(hwnd, msg, 0, 0) != 0
    }
}

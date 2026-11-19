//! Telling the resident agent (Phase 3) that a switch happened, so it can show the OSD.
//! The agent owns a message-only window of class [`AGENT_WINDOW_CLASS`] and listens for
//! the registered message [`SWITCHED_MESSAGE`]; details are in `state\last-switch.json`.

use std::ptr::{null, null_mut};

use crate::ffi::{FindWindowExW, PostMessageW, RegisterWindowMessageW, HWND_MESSAGE};
use crate::util::wide;

pub const AGENT_WINDOW_CLASS: &str = "DisplayGroups.Agent";
pub const SWITCHED_MESSAGE: &str = "DisplayGroups.Switched";

/// Returns false when no agent is running (then there is simply no OSD).
pub fn notify_switched() -> bool {
    let class = wide(AGENT_WINDOW_CLASS);
    let name = wide(SWITCHED_MESSAGE);
    // SAFETY: lookups with NUL-terminated strings; posting to a found window is harmless.
    unsafe {
        let hwnd = FindWindowExW(HWND_MESSAGE, null_mut(), class.as_ptr(), null());
        if hwnd.is_null() {
            return false;
        }
        let msg = RegisterWindowMessageW(name.as_ptr());
        msg != 0 && PostMessageW(hwnd, msg, 0, 0) != 0
    }
}

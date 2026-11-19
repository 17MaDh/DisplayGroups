//! Minimal native UI for the console-less binary: message boxes and the
//! "Keep these display settings?" countdown. The polished dialog arrives with the
//! agent (Phase 3/4); this one uses only `MessageBoxW`.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use dg_engine::Confirmer;

use crate::ffi::*;
use crate::util::wide;

/// Makes the process per-monitor DPI aware (v2) so modes and positions are physical pixels.
pub fn set_dpi_aware() {
    // SAFETY: affects only this process; failure (already set) is harmless.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

pub fn show_error(title: &str, text: &str) {
    message_box(title, text, MB_OK | MB_ICONERROR);
}

pub fn show_info(title: &str, text: &str) {
    message_box(title, text, MB_OK | MB_ICONINFORMATION);
}

fn message_box(title: &str, text: &str, kind: u32) -> i32 {
    let (t, c) = (wide(text), wide(title));
    // SAFETY: NUL-terminated strings, no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            kind | MB_TOPMOST | MB_SETFOREGROUND,
        )
    }
}

/// Shows a topmost OK/Cancel box on the (new) primary display. If nobody answers in
/// time, the box is closed and the switch is reverted. Cancel is the default button,
/// so pressing Enter blind also reverts.
#[derive(Debug, Default)]
pub struct MessageBoxConfirmer;

impl Confirmer for MessageBoxConfirmer {
    fn confirm(&self, seconds: u32, group_name: &str) -> bool {
        let text = format!(
            "Switched to \"{group_name}\".\n\nKeep these display settings?\n\n\
             If you do nothing, the previous layout comes back in {seconds} seconds.\n\n\
             OK = keep      Cancel = revert"
        );
        let (answer_tx, answer_rx) = mpsc::channel();
        let (thread_tx, thread_rx) = mpsc::channel();
        std::thread::spawn(move || {
            // SAFETY: trivial kernel32 call.
            let _ = thread_tx.send(unsafe { GetCurrentThreadId() });
            let r = message_box(
                "DisplayGroups",
                &text,
                MB_OKCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2,
            );
            let _ = answer_tx.send(r == IDOK);
        });
        let thread_id = thread_rx.recv().unwrap_or(0);
        match answer_rx.recv_timeout(Duration::from_secs(seconds.into())) {
            Ok(keep) => keep,
            Err(_) => {
                close_thread_windows(thread_id);
                let _ = answer_rx.recv_timeout(Duration::from_secs(2));
                false
            }
        }
    }
}

unsafe extern "system" fn post_close(hwnd: *mut c_void, _lparam: isize) -> i32 {
    // SAFETY: posting WM_CLOSE to a window of our own thread; a closed MB_OKCANCEL
    // box returns IDCANCEL.
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    1
}

fn close_thread_windows(thread_id: u32) {
    if thread_id != 0 {
        // SAFETY: enumerates top-level windows of the message-box thread.
        unsafe { EnumThreadWindows(thread_id, post_close, 0) };
    }
}

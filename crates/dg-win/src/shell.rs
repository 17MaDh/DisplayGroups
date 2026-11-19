//! Launching user-configured actions. Runs only in user context (ARCHITECTURE.md §11.1).

use std::process::Command;
use std::ptr::{null, null_mut};

use crate::ffi::{ShellExecuteW, SW_SHOWNORMAL};
use crate::util::wide;

/// Opens a URL or registered protocol (e.g. `steam://open/bigpicture`).
pub fn open(target: &str) -> Result<(), String> {
    let verb = wide("open");
    let file = wide(target);
    // SAFETY: NUL-terminated strings; null parameters/directory are allowed.
    let r = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if r > 32 {
        Ok(())
    } else {
        Err(format!("Windows could not open \"{target}\" (code {r})"))
    }
}

/// Starts a program with an explicit argument list. Arguments are quoted by the
/// standard library; no shell is involved, so nothing in them is interpreted.
pub fn spawn(path: &str, args: &[String], working_dir: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new(path);
    cmd.args(args);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.spawn()
        .map(drop)
        .map_err(|e| format!("could not start \"{path}\": {e}"))
}

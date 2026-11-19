//! Small native UI pieces: DPI awareness, message boxes, the standard Open dialog
//! (custom group icons), and a `MessageBoxW` fallback confirmer. The polished
//! countdown dialog is in [`crate::confirm`].

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

/// The standard Windows "Open" dialog for one kind of file, e.g. `("Icons", "*.ico;*.png")`.
/// `owner` is the parent window handle (0 for none). `Ok(None)` when cancelled.
pub fn pick_file(
    owner: isize,
    title: &str,
    filter_name: &str,
    patterns: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, SIGDN_FILESYSPATH,
    };
    /// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`
    const CANCELLED: i32 = 0x8007_04C7_u32 as i32;
    let e = |op: &'static str| move |e: windows::core::Error| format!("{op}: {e}");
    let (name, spec, title) = (
        HSTRING::from(filter_name),
        HSTRING::from(patterns),
        HSTRING::from(title),
    );
    // SAFETY: COM calls on a freshly created dialog; the filter strings outlive the
    // call that reads them, and the returned path is freed with CoTaskMemFree.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(e("FileOpenDialog"))?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        }];
        dialog.SetFileTypes(&filters).map_err(e("SetFileTypes"))?;
        dialog.SetTitle(&title).map_err(e("SetTitle"))?;
        let options = dialog.GetOptions().map_err(e("GetOptions"))?;
        dialog
            .SetOptions(options | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)
            .map_err(e("SetOptions"))?;
        let parent = (owner != 0).then_some(HWND(owner as *mut c_void));
        match dialog.Show(parent) {
            Ok(()) => {}
            Err(err) if err.code().0 == CANCELLED => return Ok(None),
            Err(err) => return Err(format!("Show: {err}")),
        }
        let item = dialog.GetResult().map_err(e("GetResult"))?;
        let path = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(e("GetDisplayName"))?;
        let text = path.to_string().map_err(|err| err.to_string());
        CoTaskMemFree(Some(path.0 as *const c_void));
        Ok(Some(std::path::PathBuf::from(text?)))
    }
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

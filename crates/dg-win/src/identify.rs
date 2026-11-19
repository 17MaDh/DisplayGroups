//! "Identify" (ARCHITECTURE.md §9): a large number and the display's name on every
//! active display for a few seconds, so people can tell which screen is which.
//!
//! Each panel is the same kind of window as the OSD (layered, click-through, never
//! focused, not in the taskbar). [`show`] runs its own message loop on the calling
//! thread and returns once the panels are gone, so it suits a short-lived process
//! (`displaygroups identify`), not a thread with its own windows.

use std::cell::RefCell;
use std::ptr::{null, null_mut};

use crate::ffi_ui::*;
use crate::osd::{draw, font};
use crate::util::wide;

const CLASS: &str = "DisplayGroups.Identify";
const TIMER_ID: usize = 1;
const SW_SHOWNOACTIVATE: i32 = 4;
const ALPHA: u8 = 235;
/// Logical size at 96 DPI.
const WIDTH: i32 = 300;
const HEIGHT: i32 = 230;

/// What one display shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifyLabel {
    pub number: u32,
    pub title: String,
    pub detail: String,
    /// The display's desktop rectangle, in physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

struct State {
    panels: Vec<(HWND, IdentifyLabel)>,
    dark: bool,
    rtl: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Shows a panel in the middle of each display for `duration_ms`, then removes them.
pub fn show(
    labels: &[IdentifyLabel],
    duration_ms: u32,
    dark: bool,
    rtl: bool,
) -> Result<(), String> {
    if labels.is_empty() {
        return Ok(());
    }
    let class = wide(CLASS);
    // SAFETY: window creation, a local message loop and destruction, all on this thread.
    unsafe {
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: identify_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);

        let mut panels = Vec::with_capacity(labels.len());
        for label in labels {
            let center = POINT {
                x: label.x + label.width as i32 / 2,
                y: label.y + label.height as i32 / 2,
            };
            let monitor = MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
            let (mut dpi, mut _dpi_y) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
            let (w, h) = (WIDTH * dpi as i32 / 96, HEIGHT * dpi as i32 / 96);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                class.as_ptr(),
                null(),
                WS_POPUP,
                center.x - w / 2,
                center.y - h / 2,
                w,
                h,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                continue;
            }
            let round = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&round as *const u32).cast(),
                4,
            );
            SetLayeredWindowAttributes(hwnd, 0, ALPHA, LWA_ALPHA);
            panels.push((hwnd, label.clone()));
        }
        let Some(&(first, _)) = panels.first() else {
            return Err(format!(
                "CreateWindowExW (identify) failed ({})",
                GetLastError()
            ));
        };
        let hwnds: Vec<HWND> = panels.iter().map(|(h, _)| *h).collect();
        STATE.with(|s| *s.borrow_mut() = Some(State { panels, dark, rtl }));
        for &hwnd in &hwnds {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        SetTimer(first, TIMER_ID, duration_ms.max(500), null());

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for hwnd in hwnds {
            DestroyWindow(hwnd);
        }
        STATE.with(|s| *s.borrow_mut() = None);
    }
    Ok(())
}

fn paint(hwnd: HWND) {
    let found = STATE.with(|s| {
        s.borrow().as_ref().and_then(|st| {
            st.panels
                .iter()
                .find(|(h, _)| *h == hwnd)
                .map(|(_, l)| (l.clone(), st.dark, st.rtl))
        })
    });
    // SAFETY: standard WM_PAINT handling; every GDI object created is deleted.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        if let Some((label, dark, rtl)) = found {
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let s = |v: i32| v * dpi / 96;
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let (bg, fg, sub, accent) = if dark {
                (
                    rgb(32, 32, 32),
                    rgb(255, 255, 255),
                    rgb(200, 200, 200),
                    rgb(96, 205, 255),
                )
            } else {
                (
                    rgb(249, 249, 249),
                    rgb(26, 26, 26),
                    rgb(96, 96, 96),
                    rgb(0, 95, 184),
                )
            };
            let brush = CreateSolidBrush(bg);
            FillRect(hdc, &rc, brush);
            DeleteObject(brush);
            SetBkMode(hdc, TRANSPARENT);
            // Only the name reads right to left; the number and the technical detail
            // ("3840 × 2160 · 120 Hz") keep their order.
            let title_reading = if rtl { DT_RTLREADING } else { 0 };

            for (text, size, weight, color, top, bottom, reading) in [
                (label.number.to_string(), 112, 600, accent, 4, 140, 0),
                (label.title.clone(), 20, 600, fg, 146, 178, title_reading),
                (label.detail.clone(), 14, 400, sub, 182, 208, 0),
            ] {
                let f = font(s(size), weight, "Segoe UI");
                let old = SelectObject(hdc, f);
                SetTextColor(hdc, color);
                let mut r = RECT {
                    left: s(16),
                    top: s(top),
                    right: rc.right - s(16),
                    bottom: s(bottom),
                };
                draw(
                    hdc,
                    &text,
                    &mut r,
                    DT_CENTER
                        | DT_VCENTER
                        | DT_SINGLELINE
                        | DT_END_ELLIPSIS
                        | DT_NOPREFIX
                        | reading,
                );
                SelectObject(hdc, old);
                DeleteObject(f);
            }
        }
        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn identify_proc(
    hwnd: HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_TIMER if wparam == TIMER_ID => {
            // SAFETY: stops our timer and ends the loop in `show`.
            unsafe {
                KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
            }
            0
        }
        // SAFETY: default processing.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

//! "Keep these display settings?" with a live countdown (ARCHITECTURE.md §6).
//!
//! A small owner-painted Win32 window: it opens instantly from the agent or a shortcut
//! (no WebView), follows the dark/light theme and works with the keyboard. "Revert" has
//! the initial focus and every way out except "Keep changes" reverts: Esc, closing the
//! window, or the countdown reaching zero.

use std::cell::RefCell;
use std::ptr::{null, null_mut};

use dg_engine::Confirmer;

use crate::ffi_ui::*;
use crate::util::wide;

const CLASS: &str = "DisplayGroups.Confirm";
const TIMER_ID: usize = 1;
/// Logical size at 96 DPI.
const WIDTH: i32 = 460;
const HEIGHT: i32 = 190;

/// Texts come from the caller (already translated). `body` may contain `{group}` and
/// `{seconds}`.
#[derive(Debug, Clone)]
pub struct CountdownDialog {
    pub title: String,
    pub body: String,
    pub keep: String,
    pub revert: String,
    pub dark: bool,
    /// Right-to-left language: the window is mirrored (buttons swap sides) and text
    /// reads from the right.
    pub rtl: bool,
}

impl CountdownDialog {
    /// English defaults.
    pub fn english(dark: bool) -> Self {
        Self {
            title: "Keep these display settings?".into(),
            body: "Switched to \u{201C}{group}\u{201D}. Reverting to the previous layout in {seconds} s.".into(),
            keep: "Keep changes".into(),
            revert: "Revert".into(),
            dark,
            rtl: false,
        }
    }
}

impl Confirmer for CountdownDialog {
    fn confirm(&self, seconds: u32, group_name: &str) -> bool {
        run(self, seconds, group_name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Button {
    Keep,
    Revert,
}

struct State {
    texts: CountdownDialog,
    group: String,
    remaining: u32,
    focus: Button,
    hover: Option<Button>,
    result: Option<bool>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn run(texts: &CountdownDialog, seconds: u32, group: &str) -> bool {
    let class = wide(CLASS);
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            texts: texts.clone(),
            group: group.to_string(),
            remaining: seconds.max(1),
            focus: Button::Revert,
            hover: None,
            result: None,
        })
    });
    // SAFETY: window creation and a local message loop on the calling thread.
    unsafe {
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: proc_,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);

        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let (mut dpi, mut _y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y);
        let (w, h) = (WIDTH * dpi as i32 / 96, HEIGHT * dpi as i32 / 96);
        let work = info.rcWork;
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;

        let title = wide(&texts.title);
        let mirrored = if texts.rtl { WS_EX_LAYOUTRTL } else { 0 };
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | mirrored,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            // No window means no way to confirm: be safe and revert.
            return false;
        }
        let round = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&round as *const u32).cast(),
            4,
        );
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetTimer(hwnd, TIMER_ID, 1000, null());

        let mut msg: MSG = std::mem::zeroed();
        while STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.result.is_none())) {
            if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        KillTimer(hwnd, TIMER_ID);
        DestroyWindow(hwnd);
    }
    STATE.with(|s| {
        s.borrow_mut()
            .take()
            .and_then(|st| st.result)
            .unwrap_or(false)
    })
}

fn finish(keep: bool) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.result.get_or_insert(keep);
        }
    });
}

/// Button rectangles in client coordinates: (keep, revert).
fn button_rects(hwnd: HWND) -> (RECT, RECT) {
    // SAFETY: our own window.
    let (dpi, rc) = unsafe {
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc);
        (GetDpiForWindow(hwnd).max(96) as i32, rc)
    };
    let s = |v: i32| v * dpi / 96;
    let (bw, bh, gap, margin) = (s(130), s(34), s(10), s(24));
    let top = rc.bottom - margin - bh;
    let revert = RECT {
        left: rc.right - margin - bw,
        top,
        right: rc.right - margin,
        bottom: top + bh,
    };
    let keep = RECT {
        left: revert.left - gap - bw,
        top,
        right: revert.left - gap,
        bottom: top + bh,
    };
    (keep, revert)
}

fn hit(hwnd: HWND, lparam: isize) -> Option<Button> {
    let x = (lparam & 0xFFFF) as i16 as i32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    let (keep, revert) = button_rects(hwnd);
    let inside = |r: &RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
    if inside(&keep) {
        Some(Button::Keep)
    } else if inside(&revert) {
        Some(Button::Revert)
    } else {
        None
    }
}

fn update(hwnd: HWND, f: impl FnOnce(&mut State)) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            f(st);
        }
    });
    // SAFETY: schedules a repaint of our own window.
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_TIMER if wparam == TIMER_ID => {
            let mut done = false;
            update(hwnd, |st| {
                st.remaining = st.remaining.saturating_sub(1);
                done = st.remaining == 0;
            });
            if done {
                finish(false);
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            let over = hit(hwnd, lparam);
            update(hwnd, |st| st.hover = over);
            return 0;
        }
        WM_LBUTTONUP => {
            match hit(hwnd, lparam) {
                Some(Button::Keep) => finish(true),
                Some(Button::Revert) => finish(false),
                None => {}
            }
            return 0;
        }
        WM_KEYDOWN => {
            match wparam as u32 {
                VK_ESCAPE => finish(false),
                VK_RETURN | VK_SPACE => {
                    let focus = STATE.with(|s| s.borrow().as_ref().map(|st| st.focus));
                    finish(focus == Some(Button::Keep));
                }
                VK_TAB | VK_LEFT | VK_RIGHT => update(hwnd, |st| {
                    st.focus = if st.focus == Button::Keep {
                        Button::Revert
                    } else {
                        Button::Keep
                    }
                }),
                _ => {}
            }
            return 0;
        }
        WM_CLOSE => {
            finish(false);
            return 0;
        }
        _ => {}
    }
    // SAFETY: default processing.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn paint(hwnd: HWND) {
    let Some((texts, group, remaining, focus, hover)) = STATE.with(|s| {
        s.borrow().as_ref().map(|st| {
            (
                st.texts.clone(),
                st.group.clone(),
                st.remaining,
                st.focus,
                st.hover,
            )
        })
    }) else {
        return;
    };
    let (keep_rc, revert_rc) = button_rects(hwnd);
    // SAFETY: standard WM_PAINT; every GDI object created here is deleted.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let s = |v: i32| v * dpi / 96;
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc);

        let dark = texts.dark;
        // The DC of a mirrored window already right-aligns; this sets the bidi order.
        let reading = if texts.rtl { DT_RTLREADING } else { 0 };
        let bg = if dark {
            rgb(43, 43, 43)
        } else {
            rgb(249, 249, 249)
        };
        let fg = if dark {
            rgb(255, 255, 255)
        } else {
            rgb(26, 26, 26)
        };
        let sub = if dark {
            rgb(206, 206, 206)
        } else {
            rgb(80, 80, 80)
        };
        let accent = if dark {
            rgb(96, 205, 255)
        } else {
            rgb(0, 95, 184)
        };
        let accent_hover = if dark {
            rgb(88, 189, 235)
        } else {
            rgb(25, 117, 197)
        };
        let on_accent = if dark {
            rgb(0, 0, 0)
        } else {
            rgb(255, 255, 255)
        };
        let neutral = if dark {
            rgb(61, 61, 61)
        } else {
            rgb(255, 255, 255)
        };
        let neutral_hover = if dark {
            rgb(72, 72, 72)
        } else {
            rgb(240, 240, 240)
        };
        let border = if dark {
            rgb(80, 80, 80)
        } else {
            rgb(210, 210, 210)
        };

        let brush = CreateSolidBrush(bg);
        FillRect(hdc, &rc, brush);
        DeleteObject(brush);
        SetBkMode(hdc, TRANSPARENT);

        let title_font = font(s(20), 600);
        let old = SelectObject(hdc, title_font);
        SetTextColor(hdc, fg);
        let mut r = RECT {
            left: s(24),
            top: s(20),
            right: rc.right - s(24),
            bottom: s(52),
        };
        text(
            hdc,
            &texts.title,
            &mut r,
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | reading,
        );
        SelectObject(hdc, old);
        DeleteObject(title_font);

        let body_font = font(s(14), 400);
        let old = SelectObject(hdc, body_font);
        SetTextColor(hdc, sub);
        let body = texts
            .body
            .replace("{group}", &group)
            .replace("{seconds}", &remaining.to_string());
        let mut r = RECT {
            left: s(24),
            top: s(60),
            right: rc.right - s(24),
            bottom: keep_rc.top - s(8),
        };
        text(hdc, &body, &mut r, DT_WORDBREAK | DT_NOPREFIX | reading);

        for (button, rect, label) in [
            (Button::Keep, keep_rc, &texts.keep),
            (Button::Revert, revert_rc, &texts.revert),
        ] {
            let primary = button == Button::Keep;
            let hovered = hover == Some(button);
            let fill = match (primary, hovered) {
                (true, false) => accent,
                (true, true) => accent_hover,
                (false, false) => neutral,
                (false, true) => neutral_hover,
            };
            let outline = if focus == button {
                fg
            } else if primary {
                fill
            } else {
                border
            };
            let pen = CreatePen(0, if focus == button { s(2) } else { 1 }, outline);
            let fill_brush = CreateSolidBrush(fill);
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, fill_brush);
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                s(8),
                s(8),
            );
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen);
            DeleteObject(fill_brush);
            SetTextColor(hdc, if primary { on_accent } else { fg });
            let mut r = rect;
            text(
                hdc,
                label,
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | reading,
            );
        }
        SelectObject(hdc, old);
        DeleteObject(body_font);
        EndPaint(hwnd, &ps);
    }
}

fn font(height: i32, weight: i32) -> HANDLE {
    let face = wide("Segoe UI");
    // SAFETY: logical font; callers delete it.
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            0,
            0,
            CLEARTYPE_QUALITY,
            0,
            face.as_ptr(),
        )
    }
}

fn text(hdc: HANDLE, s: &str, rect: &mut RECT, format: u32) {
    let w: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: explicit length.
    unsafe { DrawTextW(hdc, w.as_ptr(), w.len() as i32, rect, format) };
}

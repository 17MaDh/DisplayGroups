//! The on-screen display (ARCHITECTURE.md §10): a click-through, non-activating,
//! layered Win32 window painted with GDI. No WebView, no image libraries.

use std::cell::RefCell;
use std::ptr::{null, null_mut};

use crate::ffi_ui::*;
use crate::util::wide;

const CLASS: &str = "DisplayGroups.OSD";
const TIMER_ID: usize = 1;
const FRAME_MS: u32 = 16;
const FADE_MS: u64 = 150;
const MAX_ALPHA: u8 = 240;
/// Logical size at 96 DPI.
const WIDTH: i32 = 380;
const HEIGHT: i32 = 92;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdContent {
    /// A Segoe Fluent Icons / MDL2 glyph, e.g. `'\u{E7F4}'` (TV/monitor).
    pub glyph: Option<char>,
    pub title: String,
    pub detail: String,
    pub dark: bool,
    /// Right-to-left language: the icon goes on the right and text reads from the right.
    pub rtl: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Hidden,
    FadeIn { start: u64 },
    Hold { until: u64 },
    FadeOut { start: u64 },
}

struct State {
    content: OsdContent,
    phase: Phase,
    hold_ms: u64,
    animate: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub struct Osd {
    hwnd: HWND,
}

impl Osd {
    /// Creates the (hidden) OSD window on the calling thread, which must run a message
    /// loop (the agent's).
    pub fn create() -> Result<Self, String> {
        let class = wide(CLASS);
        // SAFETY: standard window-class registration and creation.
        unsafe {
            let instance = GetModuleHandleW(null());
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: osd_proc,
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
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                class.as_ptr(),
                null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return Err(format!("CreateWindowExW (OSD) failed ({})", GetLastError()));
            }
            let round = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&round as *const u32).cast(),
                4,
            );
            Ok(Self { hwnd })
        }
    }

    /// Shows `content` on the primary display for about `duration_ms`.
    pub fn show(&self, content: OsdContent, duration_ms: u32) {
        let animate = animations_enabled();
        let fades = if animate { 2 * FADE_MS } else { 0 };
        let hold_ms = u64::from(duration_ms).saturating_sub(fades).max(300);
        // SAFETY: geometry queries and window updates on our own window.
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let (mut dpi, mut _dpi_y) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _dpi_y);
            let scale = |v: i32| v * dpi as i32 / 96;
            let (w, h) = (scale(WIDTH), scale(HEIGHT));
            let work = info.rcWork;
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.bottom - h - (work.bottom - work.top) / 10;

            let now = GetTickCount64();
            STATE.with(|s| {
                *s.borrow_mut() = Some(State {
                    content,
                    phase: if animate {
                        Phase::FadeIn { start: now }
                    } else {
                        Phase::Hold {
                            until: now + hold_ms,
                        }
                    },
                    hold_ms,
                    animate,
                })
            });
            SetLayeredWindowAttributes(
                self.hwnd,
                0,
                if animate { 0 } else { MAX_ALPHA },
                LWA_ALPHA,
            );
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(self.hwnd, null(), 1);
            SetTimer(self.hwnd, TIMER_ID, FRAME_MS, null());
        }
    }
}

impl Drop for Osd {
    fn drop(&mut self) {
        // SAFETY: our own window.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// False when the user turned off "Animation effects" (reduced motion).
fn animations_enabled() -> bool {
    let mut enabled: i32 = 1;
    // SAFETY: reads a BOOL system setting.
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };
    enabled != 0
}

fn tick(hwnd: HWND) {
    // SAFETY: GetTickCount64 has no preconditions.
    let now = unsafe { GetTickCount64() };
    let mut hide = false;
    let mut alpha = None;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else {
            hide = true;
            return;
        };
        match st.phase {
            Phase::Hidden => hide = true,
            Phase::FadeIn { start } => {
                let t = now.saturating_sub(start);
                if t >= FADE_MS {
                    st.phase = Phase::Hold {
                        until: now + st.hold_ms,
                    };
                    alpha = Some(MAX_ALPHA);
                } else {
                    alpha = Some((u64::from(MAX_ALPHA) * t / FADE_MS) as u8);
                }
            }
            Phase::Hold { until } => {
                if now >= until {
                    if st.animate {
                        st.phase = Phase::FadeOut { start: now };
                    } else {
                        st.phase = Phase::Hidden;
                        hide = true;
                    }
                }
            }
            Phase::FadeOut { start } => {
                let t = now.saturating_sub(start);
                if t >= FADE_MS {
                    st.phase = Phase::Hidden;
                    hide = true;
                } else {
                    alpha = Some((u64::from(MAX_ALPHA) * (FADE_MS - t) / FADE_MS) as u8);
                }
            }
        }
    });
    // SAFETY: updates on our own window, outside the RefCell borrow.
    unsafe {
        if let Some(a) = alpha {
            SetLayeredWindowAttributes(hwnd, 0, a, LWA_ALPHA);
        }
        if hide {
            KillTimer(hwnd, TIMER_ID);
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn paint(hwnd: HWND) {
    let content = STATE.with(|s| s.borrow().as_ref().map(|st| st.content.clone()));
    // SAFETY: standard WM_PAINT handling; every GDI object created is deleted.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        if let Some(c) = content {
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let s = |v: i32| v * dpi / 96;
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let (bg, fg, sub) = if c.dark {
                (rgb(32, 32, 32), rgb(255, 255, 255), rgb(200, 200, 200))
            } else {
                (rgb(249, 249, 249), rgb(26, 26, 26), rgb(96, 96, 96))
            };
            let brush = CreateSolidBrush(bg);
            FillRect(hdc, &rc, brush);
            DeleteObject(brush);
            SetBkMode(hdc, TRANSPARENT);

            // Text runs between `text_start` and `text_end`, measured from the leading
            // edge (left, or right for RTL); the icon sits on the leading edge.
            let text_start = if c.glyph.is_some() { s(84) } else { s(24) };
            let text_end = s(20);
            // The title is text in the UI language; the detail ("3840 × 2160 · 120 Hz")
            // is technical and keeps left-to-right order, only aligned to the right.
            let (text_left, text_right, title_align, detail_align) = if c.rtl {
                (
                    text_end,
                    rc.right - text_start,
                    DT_RIGHT | DT_RTLREADING,
                    DT_RIGHT,
                )
            } else {
                (text_start, rc.right - text_end, 0, 0)
            };
            if let Some(g) = c.glyph {
                let font = font(s(30), 400, "Segoe Fluent Icons");
                let old = SelectObject(hdc, font);
                SetTextColor(hdc, fg);
                let (left, right) = if c.rtl {
                    (rc.right - s(76), rc.right - s(20))
                } else {
                    (s(20), s(76))
                };
                let mut r = RECT {
                    left,
                    top: 0,
                    right,
                    bottom: rc.bottom,
                };
                draw(
                    hdc,
                    &g.to_string(),
                    &mut r,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                SelectObject(hdc, old);
                DeleteObject(font);
            }
            let title = font(s(20), 600, "Segoe UI");
            let old = SelectObject(hdc, title);
            SetTextColor(hdc, fg);
            let mut r = RECT {
                left: text_left,
                top: s(18),
                right: text_right,
                bottom: s(48),
            };
            draw(
                hdc,
                &c.title,
                &mut r,
                DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | title_align,
            );
            SelectObject(hdc, old);
            DeleteObject(title);

            let detail = font(s(14), 400, "Segoe UI");
            let old = SelectObject(hdc, detail);
            SetTextColor(hdc, sub);
            let mut r = RECT {
                left: text_left,
                top: s(50),
                right: text_right,
                bottom: s(76),
            };
            draw(
                hdc,
                &c.detail,
                &mut r,
                DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | detail_align,
            );
            SelectObject(hdc, old);
            DeleteObject(detail);
        }
        EndPaint(hwnd, &ps);
    }
}

pub(crate) fn font(height: i32, weight: i32, face: &str) -> HANDLE {
    let face = wide(face);
    // SAFETY: creates a logical font; the caller deletes it.
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

pub(crate) fn draw(hdc: HANDLE, text: &str, rect: &mut RECT, format: u32) {
    let w: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: explicit length, so no NUL terminator is needed.
    unsafe { DrawTextW(hdc, w.as_ptr(), w.len() as i32, rect, format) };
}

unsafe extern "system" fn osd_proc(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_TIMER if wparam == TIMER_ID => {
            tick(hwnd);
            0
        }
        // SAFETY: default processing.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

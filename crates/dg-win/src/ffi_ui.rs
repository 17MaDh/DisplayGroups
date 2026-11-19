//! Hand-written Win32 declarations for windows, painting, notifications and
//! process utilities used by the agent and the OSD.

#![allow(
    non_snake_case,
    non_camel_case_types,
    clippy::upper_case_acronyms,
    dead_code
)]

use std::ffi::c_void;
use std::mem::size_of;

pub type HWND = *mut c_void;
pub type HANDLE = *mut c_void;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, usize, isize) -> isize;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HANDLE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HANDLE,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: usize,
    pub lParam: isize,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HANDLE,
    pub fErase: i32,
    pub rcPaint: RECT,
    pub fRestore: i32,
    pub fIncUpdate: i32,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// `GUID_DEVINTERFACE_MONITOR` {E6F07B5F-EE97-4A90-B076-33F57BF4EAA7}
pub const GUID_DEVINTERFACE_MONITOR: GUID = GUID {
    data1: 0xE6F0_7B5F,
    data2: 0xEE97,
    data3: 0x4A90,
    data4: [0xB0, 0x76, 0x33, 0xF5, 0x7B, 0xF4, 0xEA, 0xA7],
};

#[repr(C)]
pub struct DEV_BROADCAST_DEVICEINTERFACE_W {
    pub dbcc_size: u32,
    pub dbcc_devicetype: u32,
    pub dbcc_reserved: u32,
    pub dbcc_classguid: GUID,
    pub dbcc_name: [u16; 1],
}

#[repr(C)]
#[derive(Default)]
pub struct PROCESS_MEMORY_COUNTERS {
    pub cb: u32,
    pub PageFaultCount: u32,
    pub PeakWorkingSetSize: usize,
    pub WorkingSetSize: usize,
    pub QuotaPeakPagedPoolUsage: usize,
    pub QuotaPagedPoolUsage: usize,
    pub QuotaPeakNonPagedPoolUsage: usize,
    pub QuotaNonPagedPoolUsage: usize,
    pub PagefileUsage: usize,
    pub PeakPagefileUsage: usize,
}

const _: () = {
    assert!(size_of::<MSG>() == 48);
    assert!(size_of::<PAINTSTRUCT>() == 72);
    assert!(size_of::<MONITORINFO>() == 40);
    assert!(size_of::<WNDCLASSEXW>() == 80);
};

// Messages
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SETTINGCHANGE: u32 = 0x001A;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_DEVICECHANGE: u32 = 0x0219;
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_APP: u32 = 0x8000;

pub const WTS_SESSION_LOCK: usize = 0x7;
pub const WTS_SESSION_UNLOCK: usize = 0x8;
pub const NOTIFY_FOR_THIS_SESSION: u32 = 0;

pub const DBT_DEVNODES_CHANGED: usize = 0x0007;
pub const DBT_DEVICEARRIVAL: usize = 0x8000;
pub const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;
pub const DBT_DEVTYP_DEVICEINTERFACE: u32 = 5;
pub const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0;

// Input
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_LEFT: u32 = 0x25;
pub const VK_RIGHT: u32 = 0x27;
pub const CS_DROPSHADOW: u32 = 0x0002_0000;
pub const SW_SHOW: i32 = 5;
pub const IDC_ARROW: *const u16 = 32512 as *const u16;
pub const DT_WORDBREAK: u32 = 0x0010;

// Menus
pub const MF_STRING: u32 = 0x0000;
pub const MF_CHECKED: u32 = 0x0008;
pub const MF_GRAYED: u32 = 0x0001;
pub const MF_SEPARATOR: u32 = 0x0800;
pub const MF_POPUP: u32 = 0x0010;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
pub const TPM_RETURNCMD: u32 = 0x0100;
pub const TPM_NONOTIFY: u32 = 0x0080;
/// Right-to-left menu (Arabic, Hebrew).
pub const TPM_LAYOUTRTL: u32 = 0x8000;

// Window styles
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
/// Mirrors the window's coordinates and drawing for right-to-left languages.
pub const WS_EX_LAYOUTRTL: u32 = 0x0040_0000;
pub const SW_HIDE: i32 = 0;
pub const HWND_TOPMOST: HWND = -1isize as HWND;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const LWA_ALPHA: u32 = 0x2;
pub const MONITOR_DEFAULTTOPRIMARY: u32 = 0x1;
pub const MONITOR_DEFAULTTONEAREST: u32 = 0x2;
pub const MDT_EFFECTIVE_DPI: i32 = 0;
pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
pub const DWMWCP_ROUND: u32 = 2;
pub const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;

// GDI
pub const TRANSPARENT: i32 = 1;
pub const DEFAULT_CHARSET: u32 = 1;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
/// Right-to-left reading order for bidirectional text.
pub const DT_RTLREADING: u32 = 0x0002_0000;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const DT_END_ELLIPSIS: u32 = 0x8000;

// Hotkeys
pub const MOD_NOREPEAT: u32 = 0x4000;

// Kernel
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const WAIT_OBJECT_0: u32 = 0;
pub const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 0x1;
pub const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 0x10;

/// `(HKEY)(ULONG_PTR)(LONG)0x80000001`
pub const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
pub const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

#[link(name = "user32")]
extern "system" {
    pub fn LoadCursorW(instance: HANDLE, name: *const u16) -> HANDLE;
    pub fn SetForegroundWindow(hwnd: HWND) -> i32;
    pub fn CreatePopupMenu() -> HANDLE;
    pub fn AppendMenuW(menu: HANDLE, flags: u32, id: usize, text: *const u16) -> i32;
    pub fn TrackPopupMenu(
        menu: HANDLE,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: HWND,
        rect: *const RECT,
    ) -> i32;
    pub fn DestroyMenu(menu: HANDLE) -> i32;
    pub fn GetCursorPos(point: *mut POINT) -> i32;
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HANDLE,
        instance: HANDLE,
        param: *const c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize;
    pub fn DestroyWindow(hwnd: HWND) -> i32;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    pub fn PeekMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> i32;
    pub fn TranslateMessage(msg: *const MSG) -> i32;
    pub fn DispatchMessageW(msg: *const MSG) -> isize;
    pub fn PostMessageW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> i32;
    pub fn PostQuitMessage(code: i32);
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub fn RegisterHotKey(hwnd: HWND, id: i32, modifiers: u32, vk: u32) -> i32;
    pub fn UnregisterHotKey(hwnd: HWND, id: i32) -> i32;
    pub fn SetTimer(hwnd: HWND, id: usize, ms: u32, func: *const c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> i32;
    pub fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    pub fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, w: i32, h: i32, flags: u32)
        -> i32;
    pub fn SetLayeredWindowAttributes(hwnd: HWND, key: u32, alpha: u8, flags: u32) -> i32;
    pub fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: i32) -> i32;
    pub fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HANDLE;
    pub fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> i32;
    pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
    pub fn FillRect(hdc: HANDLE, rect: *const RECT, brush: HANDLE) -> i32;
    pub fn DrawTextW(hdc: HANDLE, text: *const u16, len: i32, rect: *mut RECT, format: u32) -> i32;
    pub fn MonitorFromPoint(pt: POINT, flags: u32) -> HANDLE;
    pub fn GetMonitorInfoW(monitor: HANDLE, info: *mut MONITORINFO) -> i32;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, ini: u32) -> i32;
    pub fn RegisterDeviceNotificationW(
        recipient: HANDLE,
        filter: *const c_void,
        flags: u32,
    ) -> HANDLE;
    pub fn UnregisterDeviceNotification(handle: HANDLE) -> i32;
    pub fn FindWindowExW(parent: HWND, after: HWND, class: *const u16, title: *const u16) -> HWND;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreatePen(style: i32, width: i32, color: u32) -> HANDLE;
    pub fn RoundRect(hdc: HANDLE, l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> i32;
    pub fn CreateSolidBrush(color: u32) -> HANDLE;
    pub fn DeleteObject(obj: HANDLE) -> i32;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> HANDLE;
    pub fn SelectObject(hdc: HANDLE, obj: HANDLE) -> HANDLE;
    pub fn SetTextColor(hdc: HANDLE, color: u32) -> u32;
    pub fn SetBkMode(hdc: HANDLE, mode: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HANDLE;
    pub fn CreateMutexW(attrs: *const c_void, owner: i32, name: *const u16) -> HANDLE;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(h: HANDLE) -> i32;
    pub fn FindFirstChangeNotificationW(dir: *const u16, subtree: i32, filter: u32) -> HANDLE;
    pub fn FindNextChangeNotification(h: HANDLE) -> i32;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn GetTickCount64() -> u64;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn K32GetProcessMemoryInfo(
        process: HANDLE,
        counters: *mut PROCESS_MEMORY_COUNTERS,
        cb: u32,
    ) -> i32;
    pub fn K32EmptyWorkingSet(process: HANDLE) -> i32;
    pub fn GetUserDefaultUILanguage() -> u16;
    pub fn LCIDToLocaleName(lcid: u32, name: *mut u16, len: i32, flags: u32) -> i32;
}

#[link(name = "shcore")]
extern "system" {
    pub fn GetDpiForMonitor(monitor: HANDLE, kind: i32, x: *mut u32, y: *mut u32) -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmSetWindowAttribute(hwnd: HWND, attr: u32, value: *const c_void, size: u32) -> i32;
}

#[link(name = "wtsapi32")]
extern "system" {
    pub fn WTSRegisterSessionNotification(hwnd: HWND, flags: u32) -> i32;
    pub fn WTSUnRegisterSessionNotification(hwnd: HWND) -> i32;
}

/// `RGB(r, g, b)` as a GDI `COLORREF`.
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

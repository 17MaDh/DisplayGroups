//! The notification-area icon and its popup menu (ARCHITECTURE.md §9).

use std::ptr::null;

use crate::ffi_ui::*;
use crate::icons;
use crate::util::wide;
use crate::window::AgentWindow;

/// `WM_APP + TRAY_CODE` is the tray callback message; the agent window reports it as
/// [`crate::window::AgentEvent::Tray`].
pub const TRAY_CODE: u32 = 0x200;
const NIN_SELECT: u32 = 0x0400;
const NIN_KEYSELECT: u32 = 0x0401;

#[allow(non_snake_case, clippy::upper_case_acronyms)]
#[repr(C)]
struct NOTIFYICONDATAW {
    cbSize: u32,
    hWnd: HWND,
    uID: u32,
    uFlags: u32,
    uCallbackMessage: u32,
    hIcon: HANDLE,
    szTip: [u16; 128],
    dwState: u32,
    dwStateMask: u32,
    szInfo: [u16; 256],
    uVersion: u32,
    szInfoTitle: [u16; 64],
    dwInfoFlags: u32,
    guidItem: GUID,
    hBalloonIcon: HANDLE,
}

const _: () = assert!(std::mem::size_of::<NOTIFYICONDATAW>() == 976);

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_SHOWTIP: u32 = 0x80;
const NOTIFYICON_VERSION_4: u32 = 4;

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *mut NOTIFYICONDATAW) -> i32;
}

/// What a tray callback means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayClick {
    /// Left click or keyboard selection.
    Primary,
    /// Right click / context-menu key.
    Menu,
    Other,
}

pub fn classify(event: u32) -> TrayClick {
    match event {
        NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP => TrayClick::Primary,
        WM_CONTEXTMENU | WM_RBUTTONUP => TrayClick::Menu,
        _ => TrayClick::Other,
    }
}

pub struct Tray {
    hwnd: HWND,
    icon: icons::OwnedIcon,
    tooltip: String,
}

impl Tray {
    pub fn add(window: &AgentWindow, tooltip: &str) -> Result<Self, String> {
        let hwnd = window.hwnd();
        // SAFETY: DPI of our own window.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let ico = icons::render_ico(icons::builtin_style("monitor"))?;
        let icon =
            icons::hicon_from_ico(&ico, 16 * dpi / 96).ok_or("cannot create the tray icon")?;
        let tray = Self {
            hwnd,
            icon,
            tooltip: tooltip.to_string(),
        };
        tray.register()?;
        Ok(tray)
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut tip = [0u16; 128];
        for (slot, c) in tip.iter_mut().zip(self.tooltip.encode_utf16().take(127)) {
            *slot = c;
        }
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_APP + TRAY_CODE,
            hIcon: self.icon.as_raw(),
            szTip: tip,
            dwState: 0,
            dwStateMask: 0,
            szInfo: [0; 256],
            uVersion: NOTIFYICON_VERSION_4,
            szInfoTitle: [0; 64],
            dwInfoFlags: 0,
            guidItem: GUID {
                data1: 0,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            hBalloonIcon: std::ptr::null_mut(),
        }
    }

    /// (Re-)adds the icon, e.g. after Explorer restarted ("TaskbarCreated").
    pub fn register(&self) -> Result<(), String> {
        let mut d = self.data();
        // SAFETY: fully initialised structure of the documented size.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &mut d);
            if Shell_NotifyIconW(NIM_ADD, &mut d) == 0 {
                return Err("Shell_NotifyIcon(NIM_ADD) failed".into());
            }
            Shell_NotifyIconW(NIM_SETVERSION, &mut d);
        }
        Ok(())
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.tooltip = tooltip.to_string();
        let mut d = self.data();
        // SAFETY: as above.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut d) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let mut d = self.data();
        // SAFETY: removes our icon; the HICON is freed when `self.icon` drops.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &mut d) };
    }
}

#[derive(Debug, Clone)]
pub enum MenuItem {
    Command {
        id: u32,
        text: String,
        checked: bool,
        enabled: bool,
    },
    Separator,
    Submenu {
        text: String,
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    pub fn command(id: u32, text: impl Into<String>) -> Self {
        MenuItem::Command {
            id,
            text: text.into(),
            checked: false,
            enabled: true,
        }
    }

    pub fn checkbox(id: u32, text: impl Into<String>, checked: bool) -> Self {
        MenuItem::Command {
            id,
            text: text.into(),
            checked,
            enabled: true,
        }
    }

    pub fn disabled(text: impl Into<String>) -> Self {
        MenuItem::Command {
            id: 0,
            text: text.into(),
            checked: false,
            enabled: false,
        }
    }
}

fn build(items: &[MenuItem]) -> HANDLE {
    // SAFETY: menu handles are owned by the returned root (submenus are destroyed with it).
    unsafe {
        let menu = CreatePopupMenu();
        for item in items {
            match item {
                MenuItem::Separator => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, null());
                }
                MenuItem::Command {
                    id,
                    text,
                    checked,
                    enabled,
                } => {
                    let mut flags = MF_STRING;
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    let t = wide(text);
                    AppendMenuW(menu, flags, *id as usize, t.as_ptr());
                }
                MenuItem::Submenu { text, items } => {
                    let sub = build(items);
                    let t = wide(text);
                    AppendMenuW(menu, MF_STRING | MF_POPUP, sub as usize, t.as_ptr());
                }
            }
        }
        menu
    }
}

/// Shows the menu at the cursor and returns the chosen command id.
pub fn show_menu(window: &AgentWindow, items: &[MenuItem]) -> Option<u32> {
    let hwnd = window.hwnd();
    let menu = build(items);
    // SAFETY: the window must be foreground for the menu to close when clicking away.
    let chosen = unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let id = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            null(),
        );
        PostMessageW(hwnd, 0, 0, 0); // WM_NULL, per the TrackPopupMenu documentation
        DestroyMenu(menu);
        id
    };
    u32::try_from(chosen).ok().filter(|&id| id != 0)
}

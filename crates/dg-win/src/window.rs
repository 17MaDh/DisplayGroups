//! The agent's hidden top-level window. It receives hotkeys, timers, display and device
//! changes, session lock/unlock and the "a switch happened" broadcast, and turns them
//! into [`AgentEvent`]s.
//!
//! The window procedure only queues events; the handler runs from the message loop,
//! never from inside the window procedure, so it can never be re-entered.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ptr::{null, null_mut};

use crate::agent::{AGENT_WINDOW_CLASS, SWITCHED_MESSAGE};
use crate::ffi_ui::*;
use crate::util::wide;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEvent {
    Hotkey(i32),
    Timer(usize),
    /// Resolution or topology changed (`WM_DISPLAYCHANGE`).
    DisplayChanged,
    /// A monitor was connected or removed.
    DeviceChanged,
    SessionLocked,
    SessionUnlocked,
    /// Another DisplayGroups process switched groups (see `agent::notify_switched`).
    Switched,
    /// Mouse/keyboard event on the tray icon (see [`crate::tray::classify`]).
    Tray(u32),
    /// Explorer restarted; the tray icon must be added again.
    TaskbarCreated,
    /// A message posted through [`Poster`].
    App {
        code: u32,
        data: usize,
    },
}

thread_local! {
    static QUEUE: RefCell<VecDeque<AgentEvent>> = const { RefCell::new(VecDeque::new()) };
    static SWITCHED_ID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static TASKBAR_CREATED_ID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Posts [`AgentEvent::App`] messages to the agent window from any thread.
#[derive(Debug, Clone, Copy)]
pub struct Poster(usize);

impl Poster {
    pub fn post(&self, code: u32, data: usize) -> bool {
        // SAFETY: posting to a window handle is safe even if it was destroyed meanwhile.
        unsafe { PostMessageW(self.0 as HWND, WM_APP + code, data, 0) != 0 }
    }
}

pub struct AgentWindow {
    hwnd: HWND,
    device_notification: HANDLE,
}

impl AgentWindow {
    pub fn create() -> Result<Self, String> {
        let class = wide(AGENT_WINDOW_CLASS);
        // SAFETY: standard window-class registration and creation with valid pointers.
        unsafe {
            let instance = GetModuleHandleW(null());
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: agent_proc,
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
            // A hidden *top-level* window: message-only windows miss broadcasts such as
            // WM_DISPLAYCHANGE.
            let title = wide("DisplayGroups Agent");
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
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
                return Err(format!("CreateWindowExW failed ({})", GetLastError()));
            }
            let name = wide(SWITCHED_MESSAGE);
            SWITCHED_ID.with(|c| c.set(RegisterWindowMessageW(name.as_ptr())));
            let taskbar = wide("TaskbarCreated");
            TASKBAR_CREATED_ID.with(|c| c.set(RegisterWindowMessageW(taskbar.as_ptr())));
            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
                dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
                dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE,
                dbcc_reserved: 0,
                dbcc_classguid: GUID_DEVINTERFACE_MONITOR,
                dbcc_name: [0],
            };
            let device_notification = RegisterDeviceNotificationW(
                hwnd,
                (&filter as *const DEV_BROADCAST_DEVICEINTERFACE_W).cast(),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            Ok(Self {
                hwnd,
                device_notification,
            })
        }
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn poster(&self) -> Poster {
        Poster(self.hwnd as usize)
    }

    /// Registers a global hotkey delivered as [`AgentEvent::Hotkey`]`(id)`.
    pub fn register_hotkey(&self, id: i32, modifiers: u32, vk: u32) -> bool {
        // SAFETY: our own window.
        unsafe { RegisterHotKey(self.hwnd, id, modifiers | MOD_NOREPEAT, vk) != 0 }
    }

    pub fn unregister_hotkey(&self, id: i32) {
        // SAFETY: our own window.
        unsafe { UnregisterHotKey(self.hwnd, id) };
    }

    pub fn set_timer(&self, id: usize, ms: u32) {
        // SAFETY: our own window; replaces an existing timer with the same id.
        unsafe { SetTimer(self.hwnd, id, ms, null()) };
    }

    pub fn kill_timer(&self, id: usize) {
        // SAFETY: our own window.
        unsafe { KillTimer(self.hwnd, id) };
    }

    /// Runs the message loop until `handler` returns false.
    pub fn run(&self, mut handler: impl FnMut(AgentEvent) -> bool) {
        // SAFETY: standard message loop on the thread that owns the window.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                while let Some(event) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
                    if !handler(event) {
                        return;
                    }
                }
            }
        }
    }

    /// Discards hotkey messages already waiting (pressed during a switch).
    pub fn drop_pending_hotkeys(&self) {
        // SAFETY: removes our own queued messages.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, self.hwnd, WM_HOTKEY, WM_HOTKEY, 1) != 0 {}
        }
        QUEUE.with(|q| {
            q.borrow_mut()
                .retain(|e| !matches!(e, AgentEvent::Hotkey(_)))
        });
    }
}

impl Drop for AgentWindow {
    fn drop(&mut self) {
        // SAFETY: tearing down our own registrations and window.
        unsafe {
            if !self.device_notification.is_null() {
                UnregisterDeviceNotification(self.device_notification);
            }
            WTSUnRegisterSessionNotification(self.hwnd);
            DestroyWindow(self.hwnd);
        }
    }
}

fn push(event: AgentEvent) {
    QUEUE.with(|q| q.borrow_mut().push_back(event));
}

unsafe extern "system" fn agent_proc(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    match msg {
        WM_HOTKEY => push(AgentEvent::Hotkey(wparam as i32)),
        WM_TIMER => push(AgentEvent::Timer(wparam)),
        WM_DISPLAYCHANGE => push(AgentEvent::DisplayChanged),
        WM_DEVICECHANGE
            if matches!(
                wparam,
                DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE | DBT_DEVNODES_CHANGED
            ) =>
        {
            push(AgentEvent::DeviceChanged)
        }
        WM_WTSSESSION_CHANGE => match wparam {
            WTS_SESSION_LOCK => push(AgentEvent::SessionLocked),
            WTS_SESSION_UNLOCK => push(AgentEvent::SessionUnlocked),
            _ => {}
        },
        m if m == WM_APP + crate::tray::TRAY_CODE => {
            // NOTIFYICON_VERSION_4: the low word of lParam is the mouse/keyboard event.
            push(AgentEvent::Tray((lparam & 0xFFFF) as u32))
        }
        m if (WM_APP..WM_APP + 0x1000).contains(&m) => push(AgentEvent::App {
            code: m - WM_APP,
            data: wparam,
        }),
        m if m != 0 && m == SWITCHED_ID.with(|c| c.get()) => push(AgentEvent::Switched),
        m if m != 0 && m == TASKBAR_CREATED_ID.with(|c| c.get()) => {
            push(AgentEvent::TaskbarCreated)
        }
        WM_CLOSE => {
            // SAFETY: ends the message loop of this thread.
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        _ => {}
    }
    // SAFETY: default processing for everything else.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

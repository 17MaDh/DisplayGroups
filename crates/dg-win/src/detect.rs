//! What automatic switching watches (ARCHITECTURE.md §15.1): running programs, their
//! window titles, and game controllers. Read-only, current user, no admin rights, no
//! hooks into other processes.
//!
//! - Programs: a Toolhelp process snapshot (executable names only).
//! - Windows: `EnumWindows` over visible top-level windows with a title.
//! - Controllers: XInput slots (Xbox-compatible pads) and present HID interfaces whose
//!   top-level usage is Generic Desktop / Joystick or Gamepad (DualSense, Switch Pro,
//!   generic pads).

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use crate::ffi_ui::{CloseHandle, GUID, HANDLE, HWND, INVALID_HANDLE_VALUE};

const TH32CS_SNAPPROCESS: u32 = 0x2;
const MAX_PATH: usize = 260;
const DIGCF_PRESENT: u32 = 0x2;
const DIGCF_DEVICEINTERFACE: u32 = 0x10;
const OPEN_EXISTING: u32 = 3;
const FILE_SHARE_READ_WRITE: u32 = 0x1 | 0x2;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
const XUSER_MAX_COUNT: u32 = 4;
const USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
const USAGE_JOYSTICK: u16 = 0x04;
const USAGE_GAMEPAD: u16 = 0x05;

/// `GUID_DEVINTERFACE_HID` {4D1E55B2-F16F-11CF-88CB-001111000030}
const GUID_DEVINTERFACE_HID: GUID = GUID {
    data1: 0x4D1E_55B2,
    data2: 0xF16F,
    data3: 0x11CF,
    data4: [0x88, 0xCB, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30],
};

#[repr(C)]
struct PROCESSENTRY32W {
    dwSize: u32,
    cntUsage: u32,
    th32ProcessID: u32,
    th32DefaultHeapID: usize,
    th32ModuleID: u32,
    cntThreads: u32,
    th32ParentProcessID: u32,
    pcPriClassBase: i32,
    dwFlags: u32,
    szExeFile: [u16; MAX_PATH],
}

#[repr(C)]
struct SP_DEVICE_INTERFACE_DATA {
    cbSize: u32,
    InterfaceClassGuid: GUID,
    Flags: u32,
    Reserved: usize,
}

const _: () = {
    assert!(std::mem::size_of::<PROCESSENTRY32W>() == 568);
    assert!(std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() == 32);
};

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
    fn Process32FirstW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
    fn Process32NextW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: HANDLE,
    ) -> HANDLE;
}

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(HWND, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: HWND) -> i32;
    fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    fn GetWindowTextW(hwnd: HWND, text: *mut u16, max: i32) -> i32;
}

#[link(name = "setupapi")]
extern "system" {
    fn SetupDiGetClassDevsW(
        class: *const GUID,
        enumerator: *const u16,
        parent: HWND,
        flags: u32,
    ) -> HANDLE;
    fn SetupDiEnumDeviceInterfaces(
        set: HANDLE,
        device: *const c_void,
        class: *const GUID,
        index: u32,
        data: *mut SP_DEVICE_INTERFACE_DATA,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        set: HANDLE,
        data: *const SP_DEVICE_INTERFACE_DATA,
        detail: *mut u8,
        size: u32,
        required: *mut u32,
        device: *mut c_void,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(set: HANDLE) -> i32;
}

#[link(name = "hid")]
extern "system" {
    fn HidD_GetPreparsedData(device: HANDLE, data: *mut isize) -> u8;
    fn HidD_FreePreparsedData(data: isize) -> u8;
    fn HidP_GetCaps(data: isize, caps: *mut [u16; 32]) -> i32;
}

#[link(name = "xinput")]
extern "system" {
    fn XInputGetState(user: u32, state: *mut [u8; 16]) -> u32;
}

fn utf16_until_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Running processes: process id → lower-case executable name (`steam.exe`).
pub fn running_programs() -> HashMap<u32, String> {
    let mut out = HashMap::new();
    // SAFETY: a process snapshot read with a correctly sized entry, then closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let name = utf16_until_nul(&entry.szExeFile).to_lowercase();
            if !name.is_empty() {
                out.insert(entry.th32ProcessID, name);
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    out
}

/// The lower-case executable names of [`running_programs`].
pub fn running_program_names() -> HashSet<String> {
    running_programs().into_values().collect()
}

/// Visible top-level windows with a title: (lower-case executable name, title).
pub fn visible_windows() -> Vec<(String, String)> {
    let programs = running_programs();
    let mut windows: Vec<(u32, String)> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, lparam: isize) -> i32 {
        // SAFETY: `lparam` is the Vec passed to EnumWindows below, alive for the call.
        unsafe {
            let windows = &mut *(lparam as *mut Vec<(u32, String)>);
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if got > 0 {
                windows.push((pid, String::from_utf16_lossy(&buf[..got as usize])));
            }
        }
        1
    }
    // SAFETY: the callback only touches `windows` through the pointer we pass.
    unsafe {
        EnumWindows(collect, (&mut windows as *mut Vec<(u32, String)>) as isize);
    }
    windows
        .into_iter()
        .filter_map(|(pid, title)| programs.get(&pid).map(|exe| (exe.clone(), title)))
        .collect()
}

/// Programs that have a visible window right now, for picking one in the settings app.
/// Lower-case executable names, sorted, without Windows shell processes.
pub fn programs_with_windows() -> Vec<String> {
    const SHELL: [&str; 8] = [
        "explorer.exe",
        "applicationframehost.exe",
        "textinputhost.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "searchhost.exe",
        "systemsettings.exe",
        "lockapp.exe",
    ];
    let mut names: Vec<String> = visible_windows()
        .into_iter()
        .map(|(exe, _)| exe)
        .filter(|exe| !SHELL.contains(&exe.as_str()) && !exe.starts_with("displaygroups"))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// True when a game controller is connected (XInput or a HID joystick/gamepad).
pub fn controller_connected() -> bool {
    xinput_connected() || hid_gamepad_present()
}

fn xinput_connected() -> bool {
    (0..XUSER_MAX_COUNT).any(|slot| {
        let mut state = [0u8; 16];
        // SAFETY: XINPUT_STATE is 16 bytes; 0 (ERROR_SUCCESS) means connected.
        unsafe { XInputGetState(slot, &mut state) == 0 }
    })
}

fn hid_gamepad_present() -> bool {
    // SAFETY: SetupAPI enumeration with correctly sized structs; every handle and the
    // device information list are released.
    unsafe {
        let set = SetupDiGetClassDevsW(
            &GUID_DEVINTERFACE_HID,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if set == INVALID_HANDLE_VALUE || set.is_null() {
            return false;
        }
        let mut found = false;
        let mut index = 0;
        loop {
            let mut data: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, null(), &GUID_DEVINTERFACE_HID, index, &mut data)
                == 0
            {
                break;
            }
            index += 1;
            if let Some(path) = interface_path(set, &data) {
                if is_gamepad(&path) {
                    found = true;
                    break;
                }
            }
        }
        SetupDiDestroyDeviceInfoList(set);
        found
    }
}

/// The device path of an interface, NUL-terminated UTF-16.
///
/// # Safety
/// `set` and `data` come from a live SetupAPI enumeration.
unsafe fn interface_path(set: HANDLE, data: &SP_DEVICE_INTERFACE_DATA) -> Option<Vec<u16>> {
    let mut required = 0u32;
    // SAFETY: the size query (null buffer) is the documented first call.
    unsafe {
        SetupDiGetDeviceInterfaceDetailW(set, data, null_mut(), 0, &mut required, null_mut());
    }
    if required < 8 {
        return None;
    }
    // SP_DEVICE_INTERFACE_DETAIL_DATA_W: cbSize (8 on 64-bit, 6 on 32-bit), then the path.
    let mut buf = vec![0u8; required as usize + 2];
    let cb: u32 = if cfg!(target_pointer_width = "64") {
        8
    } else {
        6
    };
    buf[..4].copy_from_slice(&cb.to_le_bytes());
    // SAFETY: the buffer has the size the first call asked for.
    let ok = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            set,
            data,
            buf.as_mut_ptr(),
            required,
            null_mut(),
            null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let path: Vec<u16> = buf[4..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .chain(std::iter::once(0))
        .collect();
    (path.len() > 1).then_some(path)
}

/// Opens the HID interface without read/write access (allowed even for keyboards and
/// mice held by Windows) and reads its top-level usage.
fn is_gamepad(path: &[u16]) -> bool {
    // SAFETY: `path` is NUL-terminated; the handle and preparsed data are released.
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return false;
        }
        let mut preparsed = 0isize;
        let mut result = false;
        if HidD_GetPreparsedData(handle, &mut preparsed) != 0 {
            let mut caps = [0u16; 32];
            if HidP_GetCaps(preparsed, &mut caps) == HIDP_STATUS_SUCCESS {
                let (usage, page) = (caps[0], caps[1]);
                result = page == USAGE_PAGE_GENERIC_DESKTOP
                    && (usage == USAGE_JOYSTICK || usage == USAGE_GAMEPAD);
            }
            HidD_FreePreparsedData(preparsed);
        }
        CloseHandle(handle);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_this_process() {
        let me = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        let programs = running_programs();
        assert_eq!(
            programs.get(&std::process::id()),
            Some(&me),
            "{} processes",
            programs.len()
        );
        assert!(running_program_names().contains(&me));
    }

    #[test]
    fn windows_and_controllers_do_not_fail() {
        // Results depend on the machine; these must simply run and agree with each other.
        let windows = visible_windows();
        let names = programs_with_windows();
        assert!(names
            .iter()
            .all(|n| windows.iter().any(|(exe, _)| exe == n)));
        let _ = controller_connected();
    }
}

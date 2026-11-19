//! Hand-written Win32 declarations (user32 / kernel32 / advapi32 / shell32).
//! Layouts follow the Windows SDK headers; sizes are asserted below.

#![allow(
    non_snake_case,
    non_camel_case_types,
    clippy::upper_case_acronyms,
    dead_code
)]

use std::ffi::c_void;
use std::mem::size_of;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct LUID {
    pub LowPart: u32,
    pub HighPart: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RATIONAL {
    pub Numerator: u32,
    pub Denominator: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PATH_SOURCE_INFO {
    pub adapterId: LUID,
    pub id: u32,
    pub modeInfoIdx: u32,
    pub statusFlags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PATH_TARGET_INFO {
    pub adapterId: LUID,
    pub id: u32,
    pub modeInfoIdx: u32,
    pub outputTechnology: u32,
    pub rotation: u32,
    pub scaling: u32,
    pub refreshRate: RATIONAL,
    pub scanLineOrdering: u32,
    pub targetAvailable: i32,
    pub statusFlags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PATH_INFO {
    pub sourceInfo: PATH_SOURCE_INFO,
    pub targetInfo: PATH_TARGET_INFO,
    pub flags: u32,
}

/// The 48-byte union of `DISPLAYCONFIG_MODE_INFO` (target / source / desktop-image mode).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct MODE_UNION(pub [u8; 48]);

impl Default for MODE_UNION {
    fn default() -> Self {
        Self([0; 48])
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MODE_INFO {
    pub infoType: u32,
    pub id: u32,
    pub adapterId: LUID,
    pub u: MODE_UNION,
}

pub const MODE_INFO_TYPE_SOURCE: u32 = 1;
pub const MODE_INFO_TYPE_TARGET: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct DEVICE_INFO_HEADER {
    pub type_: i32,
    pub size: u32,
    pub adapterId: LUID,
    pub id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TARGET_DEVICE_NAME {
    pub header: DEVICE_INFO_HEADER,
    pub flags: u32,
    pub outputTechnology: u32,
    pub edidManufactureId: u16,
    pub edidProductCodeId: u16,
    pub connectorInstance: u32,
    pub monitorFriendlyDeviceName: [u16; 64],
    pub monitorDevicePath: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SOURCE_DEVICE_NAME {
    pub header: DEVICE_INFO_HEADER,
    pub viewGdiDeviceName: [u16; 32],
}

/// Undocumented DPI packets used by the Settings app (types -3 / -4).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DPI_GET {
    pub header: DEVICE_INFO_HEADER,
    pub minScaleRel: i32,
    pub curScaleRel: i32,
    pub maxScaleRel: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DPI_SET {
    pub header: DEVICE_INFO_HEADER,
    pub scaleRel: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ADVANCED_COLOR_INFO {
    pub header: DEVICE_INFO_HEADER,
    pub value: u32,
    pub colorEncoding: u32,
    pub bitsPerColorChannel: u32,
}

/// Windows 11 24H2+: separates HDR from "advanced color".
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ADVANCED_COLOR_INFO_2 {
    pub header: DEVICE_INFO_HEADER,
    pub value: u32,
    pub colorEncoding: u32,
    pub bitsPerColorChannel: u32,
    pub activeColorMode: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SET_STATE {
    pub header: DEVICE_INFO_HEADER,
    pub value: u32,
}

pub const GET_SOURCE_NAME: i32 = 1;
pub const GET_TARGET_NAME: i32 = 2;
pub const GET_ADVANCED_COLOR_INFO: i32 = 9;
pub const SET_ADVANCED_COLOR_STATE: i32 = 10;
pub const GET_ADVANCED_COLOR_INFO_2: i32 = 15;
pub const SET_HDR_STATE: i32 = 16;
pub const GET_DPI_SCALE: i32 = -3;
pub const SET_DPI_SCALE: i32 = -4;

/// `DEVMODEW` with the display branch of its unions.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DEVMODEW {
    pub dmDeviceName: [u16; 32],
    pub dmSpecVersion: u16,
    pub dmDriverVersion: u16,
    pub dmSize: u16,
    pub dmDriverExtra: u16,
    pub dmFields: u32,
    pub dmPositionX: i32,
    pub dmPositionY: i32,
    pub dmDisplayOrientation: u32,
    pub dmDisplayFixedOutput: u32,
    pub dmColor: i16,
    pub dmDuplex: i16,
    pub dmYResolution: i16,
    pub dmTTOption: i16,
    pub dmCollate: i16,
    pub dmFormName: [u16; 32],
    pub dmLogPixels: u16,
    pub dmBitsPerPel: u32,
    pub dmPelsWidth: u32,
    pub dmPelsHeight: u32,
    pub dmDisplayFlags: u32,
    pub dmDisplayFrequency: u32,
    pub dmICMMethod: u32,
    pub dmICMIntent: u32,
    pub dmMediaType: u32,
    pub dmDitherType: u32,
    pub dmReserved1: u32,
    pub dmReserved2: u32,
    pub dmPanningWidth: u32,
    pub dmPanningHeight: u32,
}

const _: () = {
    assert!(size_of::<PATH_SOURCE_INFO>() == 20);
    assert!(size_of::<PATH_TARGET_INFO>() == 48);
    assert!(size_of::<PATH_INFO>() == 72);
    assert!(size_of::<MODE_INFO>() == 64);
    assert!(size_of::<DEVICE_INFO_HEADER>() == 20);
    assert!(size_of::<TARGET_DEVICE_NAME>() == 420);
    assert!(size_of::<SOURCE_DEVICE_NAME>() == 84);
    assert!(size_of::<DPI_GET>() == 32);
    assert!(size_of::<DPI_SET>() == 24);
    assert!(size_of::<ADVANCED_COLOR_INFO>() == 32);
    assert!(size_of::<ADVANCED_COLOR_INFO_2>() == 36);
    assert!(size_of::<SET_STATE>() == 24);
    assert!(size_of::<DEVMODEW>() == 220);
};

pub const QDC_ALL_PATHS: u32 = 0x1;
pub const QDC_ONLY_ACTIVE_PATHS: u32 = 0x2;
pub const ERROR_INSUFFICIENT_BUFFER: i32 = 122;

pub const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;
pub const DM_POSITION: u32 = 0x0000_0020;
pub const DM_DISPLAYORIENTATION: u32 = 0x0000_0080;
pub const DM_PELSWIDTH: u32 = 0x0008_0000;
pub const DM_PELSHEIGHT: u32 = 0x0010_0000;
pub const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
pub const CDS_UPDATEREGISTRY: u32 = 0x0000_0001;
pub const CDS_SET_PRIMARY: u32 = 0x0000_0010;
pub const CDS_NORESET: u32 = 0x1000_0000;

/// `(HKEY)(ULONG_PTR)(LONG)0x80000002`, sign-extended like the SDK macro.
pub const HKEY_LOCAL_MACHINE: isize = 0x8000_0002_u32 as i32 as isize;
pub const RRF_RT_REG_BINARY: u32 = 0x0000_0008;

pub const MB_OK: u32 = 0x0000_0000;
pub const MB_OKCANCEL: u32 = 0x0000_0001;
pub const MB_ICONERROR: u32 = 0x0000_0010;
pub const MB_ICONQUESTION: u32 = 0x0000_0020;
pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
pub const MB_DEFBUTTON2: u32 = 0x0000_0100;
pub const MB_SETFOREGROUND: u32 = 0x0001_0000;
pub const MB_TOPMOST: u32 = 0x0004_0000;
pub const IDOK: i32 = 1;
pub const WM_CLOSE: u32 = 0x0010;
pub const SW_SHOWNORMAL: i32 = 1;
pub const HWND_MESSAGE: *mut c_void = -3isize as *mut c_void;
/// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`.
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

pub type EnumWindowsProc = unsafe extern "system" fn(hwnd: *mut c_void, lparam: isize) -> i32;

#[link(name = "user32")]
extern "system" {
    pub fn GetDisplayConfigBufferSizes(flags: u32, num_paths: *mut u32, num_modes: *mut u32)
        -> i32;
    pub fn QueryDisplayConfig(
        flags: u32,
        num_paths: *mut u32,
        paths: *mut PATH_INFO,
        num_modes: *mut u32,
        modes: *mut MODE_INFO,
        topology_id: *mut u32,
    ) -> i32;
    pub fn SetDisplayConfig(
        num_paths: u32,
        paths: *const PATH_INFO,
        num_modes: u32,
        modes: *const MODE_INFO,
        flags: u32,
    ) -> i32;
    pub fn DisplayConfigGetDeviceInfo(packet: *mut DEVICE_INFO_HEADER) -> i32;
    pub fn DisplayConfigSetDeviceInfo(packet: *mut DEVICE_INFO_HEADER) -> i32;
    pub fn EnumDisplaySettingsExW(
        device: *const u16,
        mode: u32,
        dm: *mut DEVMODEW,
        flags: u32,
    ) -> i32;
    pub fn ChangeDisplaySettingsExW(
        device: *const u16,
        dm: *const DEVMODEW,
        hwnd: *mut c_void,
        flags: u32,
        param: *const c_void,
    ) -> i32;
    pub fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    pub fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn EnumThreadWindows(thread_id: u32, callback: EnumWindowsProc, lparam: isize) -> i32;
    pub fn PostMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> i32;
    pub fn FindWindowExW(
        parent: *mut c_void,
        after: *mut c_void,
        class: *const u16,
        title: *const u16,
    ) -> *mut c_void;
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetCurrentThreadId() -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegGetValueW(
        hkey: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        data_len: *mut u32,
    ) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn ShellExecuteW(
        hwnd: *mut c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

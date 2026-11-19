//! Small process-level helpers for the agent: single instance, config watching,
//! theme, hotkey probing and memory use.

use std::path::Path;
use std::ptr::{null, null_mut};

use crate::ffi::RegGetValueW;
use crate::ffi_ui::*;
use crate::util::wide;

/// Held for the lifetime of the agent; a second agent sees the mutex and exits.
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// `None` when another process already holds `name`.
    pub fn acquire(name: &str) -> Option<Self> {
        let n = wide(name);
        // SAFETY: named mutex creation; we only test for prior existence.
        unsafe {
            let h = CreateMutexW(null(), 0, n.as_ptr());
            if h.is_null() {
                return None;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                return None;
            }
            Some(Self(h))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: our own handle.
        unsafe { CloseHandle(self.0) };
    }
}

// SAFETY: a kernel handle may be closed from any thread.
unsafe impl Send for SingleInstance {}

/// Calls `on_change` (on a background thread) whenever a file in `dir` is written,
/// created, renamed or deleted. The directory is created if needed.
pub fn watch_dir(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let d = wide(&dir.to_string_lossy());
    // SAFETY: valid NUL-terminated path.
    let handle = unsafe {
        FindFirstChangeNotificationW(
            d.as_ptr(),
            0,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        // SAFETY: trivial.
        return Err(format!("cannot watch {} ({})", dir.display(), unsafe {
            GetLastError()
        }));
    }
    let raw = handle as usize;
    std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let h = raw as HANDLE;
            // SAFETY: the handle stays open for the life of the process.
            unsafe {
                while WaitForSingleObject(h, INFINITE) == WAIT_OBJECT_0 {
                    on_change();
                    if FindNextChangeNotification(h) == 0 {
                        break;
                    }
                }
            }
        })
        .map(drop)
        .map_err(|e| e.to_string())
}

/// True when Windows apps are set to dark mode.
pub fn system_prefers_dark() -> bool {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut data: u32 = 1;
    let mut size: u32 = 4;
    // SAFETY: reads a DWORD into a 4-byte buffer.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    r == 0 && data == 0
}

/// Windows build number (e.g. 26300), or 0 if it cannot be read.
pub fn windows_build() -> u32 {
    const HKEY_LOCAL_MACHINE: isize = 0x8000_0002_u32 as i32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x2;
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let value = wide("CurrentBuildNumber");
    let mut buf = [0u16; 32];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: reads a short string into a fixed buffer of `size` bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if r != 0 {
        return 0;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
        .trim()
        .parse()
        .unwrap_or(0)
}

/// True when no other process holds the global hotkey. (Our own registrations also
/// count as taken.)
pub fn hotkey_is_free(modifiers: u32, vk: u32) -> bool {
    const PROBE_ID: i32 = 0xBFFE;
    // SAFETY: registers and immediately releases a thread hotkey.
    unsafe {
        if RegisterHotKey(null_mut(), PROBE_ID, modifiers | MOD_NOREPEAT, vk) != 0 {
            UnregisterHotKey(null_mut(), PROBE_ID);
            true
        } else {
            false
        }
    }
}

/// Returns idle pages to Windows. Called when the agent goes idle (after start-up and
/// after each switch); pages it needs again are faulted back in on demand.
pub fn trim_working_set() {
    // SAFETY: affects only the current process.
    unsafe { K32EmptyWorkingSet(GetCurrentProcess()) };
}

/// The Windows display language as a lowercase language code such as `"ar"` or `"en"`
/// (the part before the region: `ar-SA` → `ar`). Used for the `"system"` language setting.
pub fn ui_language() -> String {
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
                              // SAFETY: a LANGID is a valid LCID with the default sort; the buffer length is passed.
    let len = unsafe {
        let lcid = u32::from(GetUserDefaultUILanguage());
        LCIDToLocaleName(lcid, buf.as_mut_ptr(), buf.len() as i32, 0)
    };
    if len <= 1 {
        return "en".to_string();
    }
    let name = String::from_utf16_lossy(&buf[..len as usize - 1]);
    language_code(&name)
}

/// `"ar-SA"` → `"ar"`; anything unusable → `"en"`.
pub fn language_code(locale: &str) -> String {
    let code = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if (2..=3).contains(&code.len()) && code.chars().all(|c| c.is_ascii_lowercase()) {
        code
    } else {
        "en".to_string()
    }
}

/// Current working set of this process in bytes.
pub fn working_set_bytes() -> Option<usize> {
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: fills a correctly sized struct for the current process.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some(c.WorkingSetSize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes() {
        assert_eq!(language_code("ar-SA"), "ar");
        assert_eq!(language_code("en-US"), "en");
        assert_eq!(language_code("zh-Hans-CN"), "zh");
        assert_eq!(language_code("fil"), "fil");
        assert_eq!(language_code(""), "en");
        assert_eq!(language_code("x-y"), "en");
        let ui = ui_language();
        assert!((2..=3).contains(&ui.len()), "{ui}");
    }

    #[test]
    fn single_instance_is_exclusive() {
        let name = format!("Local\\DisplayGroups.Test.{}", std::process::id());
        let first = SingleInstance::acquire(&name).unwrap();
        assert!(SingleInstance::acquire(&name).is_none());
        drop(first);
        assert!(SingleInstance::acquire(&name).is_some());
    }

    #[test]
    fn probe_sees_a_held_hotkey() {
        // Ctrl+Alt+Shift+F20: practically never used.
        let (mods, vk) = (0x1 | 0x2 | 0x4, 0x83);
        assert!(hotkey_is_free(mods, vk));
        // SAFETY: test-only registration on this thread.
        unsafe { RegisterHotKey(null_mut(), 7, mods, vk) };
        assert!(!hotkey_is_free(mods, vk));
        // SAFETY: as above.
        unsafe { UnregisterHotKey(null_mut(), 7) };
    }

    #[test]
    fn working_set_is_reported() {
        assert!(working_set_bytes().unwrap() > 0);
    }
}

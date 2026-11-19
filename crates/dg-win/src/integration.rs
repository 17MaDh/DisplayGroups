//! Windows integration for the current user only (no admin rights):
//! `.lnk` shortcuts, "Start with Windows" and the `displaygroups://` protocol.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    KF_FLAG_DEFAULT,
};

use crate::util::wide;

// ----- shortcuts -----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Desktop,
    /// `Start Menu\Programs\DisplayGroups`, ready to pin.
    StartMenu,
}

pub struct ShortcutSpec<'a> {
    /// File name without `.lnk`; unsafe characters are replaced.
    pub name: &'a str,
    pub target: &'a Path,
    pub arguments: &'a str,
    pub icon: Option<&'a Path>,
    pub description: &'a str,
}

/// The folder for `place`, created if needed. Uses the real known-folder location,
/// so redirected (e.g. OneDrive) and non-ASCII Desktop paths work.
pub fn folder(place: Place) -> Result<PathBuf, String> {
    let id = match place {
        Place::Desktop => &FOLDERID_Desktop,
        Place::StartMenu => &FOLDERID_Programs,
    };
    // SAFETY: the returned string is copied and freed with CoTaskMemFree.
    let base = unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map_err(|e| e.to_string())?;
        let s = p.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(p.0 as *const _));
        PathBuf::from(s?)
    };
    let dir = match place {
        Place::Desktop => base,
        Place::StartMenu => base.join("DisplayGroups"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Replaces characters that are not allowed in file names.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        "DisplayGroups".into()
    } else {
        cleaned
    }
}

/// Creates or overwrites `<dir>\<name>.lnk` and returns its path.
pub fn create_shortcut(dir: &Path, spec: &ShortcutSpec) -> Result<PathBuf, String> {
    let path = dir.join(format!("{}.lnk", safe_file_name(spec.name)));
    fn err(op: &'static str) -> impl Fn(windows::core::Error) -> String {
        move |e| format!("{op}: {e}")
    }
    // SAFETY: COM calls on a freshly created ShellLink object.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err("ShellLink"))?;
        link.SetPath(&HSTRING::from(spec.target.as_os_str()))
            .map_err(err("SetPath"))?;
        link.SetArguments(&HSTRING::from(spec.arguments))
            .map_err(err("SetArguments"))?;
        if let Some(dir) = spec.target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))
                .map_err(err("SetWorkingDirectory"))?;
        }
        link.SetDescription(&HSTRING::from(spec.description))
            .map_err(err("SetDescription"))?;
        if let Some(icon) = spec.icon {
            link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)
                .map_err(err("SetIconLocation"))?;
        }
        let file: IPersistFile = link.cast().map_err(err("IPersistFile"))?;
        file.Save(&HSTRING::from(path.as_os_str()), true)
            .map_err(err("Save"))?;
    }
    Ok(path)
}

// ----- registry (HKCU) -----

const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
const REG_SZ: u32 = 1;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const ERROR_FILE_NOT_FOUND: i32 = 2;

#[link(name = "advapi32")]
extern "system" {
    fn RegSetKeyValueW(
        hkey: isize,
        subkey: *const u16,
        value: *const u16,
        kind: u32,
        data: *const u16,
        len: u32,
    ) -> i32;
    fn RegDeleteKeyValueW(hkey: isize, subkey: *const u16, value: *const u16) -> i32;
    fn RegDeleteTreeW(hkey: isize, subkey: *const u16) -> i32;
    fn RegGetValueW(
        hkey: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut std::ffi::c_void,
        len: *mut u32,
    ) -> i32;
}

fn set_string(subkey: &str, value: Option<&str>, data: &str) -> Result<(), String> {
    let k = wide(subkey);
    let v = value.map(wide);
    let d = wide(data);
    let bytes = u32::try_from(d.len() * 2).map_err(|e| e.to_string())?;
    // SAFETY: NUL-terminated buffers; a null value name sets the key's default value.
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            v.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
            REG_SZ,
            d.as_ptr(),
            bytes,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("cannot write HKCU\\{subkey} (error {r})"))
    }
}

fn get_string(subkey: &str, value: Option<&str>) -> Option<String> {
    let k = wide(subkey);
    let v = value.map(wide);
    let vp = v.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    let mut len = 0u32;
    // SAFETY: size query, then a read into a buffer of that size.
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            vp,
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut len,
        ) != 0
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            vp,
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        ) != 0
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

fn delete_value(subkey: &str, value: &str) -> Result<(), String> {
    let (k, v) = (wide(subkey), wide(value));
    // SAFETY: NUL-terminated names.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr()) } {
        0 | ERROR_FILE_NOT_FOUND => Ok(()),
        r => Err(format!("cannot delete HKCU\\{subkey}\\{value} (error {r})")),
    }
}

fn delete_tree(subkey: &str) -> Result<(), String> {
    let k = wide(subkey);
    // SAFETY: NUL-terminated key name under HKCU.
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr()) } {
        0 | ERROR_FILE_NOT_FOUND => Ok(()),
        r => Err(format!("cannot delete HKCU\\{subkey} (error {r})")),
    }
}

fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

// ----- start with Windows -----

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_VALUE: &str = "DisplayGroups";

/// The command registered to start the agent at sign-in.
pub fn startup_command(agent_exe: &Path) -> String {
    format!("{} agent", quoted(agent_exe))
}

pub fn startup_enabled() -> bool {
    get_string(RUN_KEY, Some(RUN_VALUE)).is_some()
}

pub fn set_startup(agent_exe: Option<&Path>) -> Result<(), String> {
    match agent_exe {
        Some(exe) => set_string(RUN_KEY, Some(RUN_VALUE), &startup_command(exe)),
        None => delete_value(RUN_KEY, RUN_VALUE),
    }
}

// ----- displaygroups:// -----

pub const PROTOCOL_KEY: &str = r"Software\Classes\displaygroups";

/// Registers `displaygroups://` for this user, routed to `displaygroupsw.exe uri "%1"`.
pub fn register_protocol(handler_exe: &Path) -> Result<(), String> {
    set_string(PROTOCOL_KEY, None, "URL:DisplayGroups")?;
    set_string(PROTOCOL_KEY, Some("URL Protocol"), "")?;
    set_string(
        &format!(r"{PROTOCOL_KEY}\DefaultIcon"),
        None,
        &format!("{},0", quoted(handler_exe)),
    )?;
    set_string(
        &format!(r"{PROTOCOL_KEY}\shell\open\command"),
        None,
        &format!("{} uri \"%1\"", quoted(handler_exe)),
    )
}

pub fn unregister_protocol() -> Result<(), String> {
    delete_tree(PROTOCOL_KEY)
}

/// The registered handler command, if any.
pub fn protocol_command() -> Option<String> {
    get_string(&format!(r"{PROTOCOL_KEY}\shell\open\command"), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("Gaming on Couch"), "Gaming on Couch");
        assert_eq!(safe_file_name("A/B: C?"), "A_B_ C_");
        assert_eq!(safe_file_name("وضع الكنب"), "وضع الكنب");
        assert_eq!(safe_file_name(" ... "), "DisplayGroups");
    }

    #[test]
    fn shortcut_is_written_and_resolves_to_target() {
        let dir = std::env::temp_dir().join(format!("dg-lnk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = std::env::current_exe().unwrap();
        let path = create_shortcut(
            &dir,
            &ShortcutSpec {
                name: "وضع الكنب: Couch",
                target: &target,
                arguments: "apply 6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60",
                icon: None,
                description: "DisplayGroups test",
            },
        )
        .unwrap();
        assert!(path.ends_with("وضع الكنب_ Couch.lnk"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &[0x4C, 0, 0, 0], "shell link header");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn registry_round_trip_in_a_scratch_key() {
        let key = format!(r"Software\DisplayGroups-Test-{}", std::process::id());
        set_string(&key, Some("v"), "hello").unwrap();
        assert_eq!(get_string(&key, Some("v")).as_deref(), Some("hello"));
        delete_value(&key, "v").unwrap();
        assert_eq!(get_string(&key, Some("v")), None);
        delete_tree(&key).unwrap();
        delete_tree(&key).unwrap(); // deleting twice is fine
    }

    #[test]
    fn commands_are_quoted() {
        let exe = Path::new(r"C:\Program Files\DisplayGroups\displaygroupsw.exe");
        assert_eq!(
            startup_command(exe),
            r#""C:\Program Files\DisplayGroups\displaygroupsw.exe" agent"#
        );
    }
}

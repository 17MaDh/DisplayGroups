//! Where DisplayGroups keeps its files.
//!
//! * Windows: config in `%APPDATA%\DisplayGroups`, logs and machine-local state in
//!   `%LOCALAPPDATA%\DisplayGroups`.
//! * Linux: config in `$XDG_CONFIG_HOME/displaygroups` (`~/.config/displaygroups`), logs
//!   and state in `$XDG_STATE_HOME/displaygroups` (`~/.local/state/displaygroups`).
//! * Portable (a `portable.flag` file next to the executable): everything in `./data`.

use std::path::{Path, PathBuf};

use crate::ModelError;

pub const APP_DIR: &str = "DisplayGroups";
pub const PORTABLE_FLAG: &str = "portable.flag";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub icons_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub state_dir: PathBuf,
    pub portable: bool,
}

impl AppPaths {
    /// Resolves paths for an executable living in `exe_dir`.
    pub fn resolve(exe_dir: Option<&Path>) -> Result<Self, ModelError> {
        if let Some(dir) = exe_dir {
            if dir.join(PORTABLE_FLAG).is_file() {
                return Ok(Self::portable(&dir.join("data")));
            }
        }
        #[cfg(windows)]
        let (roaming, local) = (
            env_dir("APPDATA")?.join(APP_DIR),
            env_dir("LOCALAPPDATA")?.join(APP_DIR),
        );
        #[cfg(not(windows))]
        let (roaming, local) = (
            xdg_dir("XDG_CONFIG_HOME", ".config")?.join("displaygroups"),
            xdg_dir("XDG_STATE_HOME", ".local/state")?.join("displaygroups"),
        );
        Ok(Self {
            config_file: roaming.join("config.json"),
            icons_dir: roaming.join("icons"),
            logs_dir: local.join("logs"),
            state_dir: local.join("state"),
            portable: false,
        })
    }

    pub fn portable(data: &Path) -> Self {
        Self {
            config_file: data.join("config.json"),
            icons_dir: data.join("icons"),
            logs_dir: data.join("logs"),
            state_dir: data.join("state"),
            portable: true,
        }
    }

    /// Last-known-good display layout (raw CCD arrays; machine-local, never exported).
    pub fn lkg_file(&self) -> PathBuf {
        self.state_dir.join("lkg.json")
    }

    /// Details of the most recent switch, read by the agent to draw the OSD.
    pub fn last_switch_file(&self) -> PathBuf {
        self.state_dir.join("last-switch.json")
    }

    /// Timestamp of the last `displaygroups://` switch, for rate limiting.
    pub fn uri_stamp_file(&self) -> PathBuf {
        self.state_dir.join("uri-last.json")
    }
}

/// Where the translations (`<code>.json`) are installed for executables in `exe_dir`:
/// `locales` beside them (the Windows installer and ZIP, the Linux tarball), else the
/// packaged resources folder `../lib/DisplayGroups/locales` (the `.deb` puts the
/// programs in `/usr/bin` and the resources in `/usr/lib/DisplayGroups`, and an
/// AppImage does the same under its mount). `None` when neither exists.
pub fn installed_locales(exe_dir: &Path) -> Option<PathBuf> {
    [
        exe_dir.join("locales"),
        exe_dir.join("../lib").join(APP_DIR).join("locales"),
    ]
    .into_iter()
    .find(|dir| dir.is_dir())
}

fn env_dir(name: &'static str) -> Result<PathBuf, ModelError> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or(ModelError::MissingEnv(name))
}

/// An XDG base directory: the variable if set to an absolute path, else `$HOME/<default>`.
#[cfg(not(windows))]
fn xdg_dir(name: &'static str, default: &str) -> Result<PathBuf, ModelError> {
    match std::env::var_os(name).map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => Ok(env_dir("HOME")?.join(default)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_flag_switches_to_data_dir() {
        let dir = std::env::temp_dir().join(format!("dg-paths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PORTABLE_FLAG), b"").unwrap();
        let p = AppPaths::resolve(Some(&dir)).unwrap();
        assert!(p.portable);
        assert_eq!(p.config_file, dir.join("data").join("config.json"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn locales_are_found_beside_or_in_the_package_resources() {
        let root = std::env::temp_dir().join(format!("dg-locales-{}", uuid::Uuid::new_v4()));
        let bin = root.join("usr").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(installed_locales(&bin), None);

        // A .deb or AppImage: /usr/bin and /usr/lib/DisplayGroups/locales.
        let packaged = root.join("usr").join("lib").join(APP_DIR).join("locales");
        std::fs::create_dir_all(&packaged).unwrap();
        let found = installed_locales(&bin).unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            packaged.canonicalize().unwrap()
        );

        // A tarball or the Windows install: locales next to the programs win.
        std::fs::create_dir_all(bin.join("locales")).unwrap();
        assert_eq!(installed_locales(&bin), Some(bin.join("locales")));
        std::fs::remove_dir_all(root).unwrap();
    }
}

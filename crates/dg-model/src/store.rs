//! Loading and crash-safe saving of `config.json` and other JSON state files.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::{migrate, Config, ModelError};

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    /// Set when the file was migrated from an older schema (a backup was written).
    pub migrated_from: Option<u32>,
    /// True when no file existed and a default config was returned.
    pub created: bool,
}

/// Loads the config at `path`, migrating it if needed. A missing file yields defaults.
/// Before a migrated config is used, the original is copied to `config.v{n}.bak.json`.
pub fn load(path: &Path) -> Result<Loaded, ModelError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                config: Config::default(),
                migrated_from: None,
                created: true,
            })
        }
        Err(e) => return Err(ModelError::io(path, e)),
    };
    let value: Value =
        serde_json::from_slice(strip_bom(&bytes)).map_err(|source| ModelError::Json {
            path: path.into(),
            source,
        })?;
    let (value, migrated_from) = migrate::upgrade(value)?;
    if let Some(from) = migrated_from {
        let backup = path.with_file_name(format!("config.v{from}.bak.json"));
        if !backup.exists() {
            fs::copy(path, &backup).map_err(|e| ModelError::io(&backup, e))?;
        }
    }
    let config = serde_json::from_value(value).map_err(|source| ModelError::Json {
        path: path.into(),
        source,
    })?;
    Ok(Loaded {
        config,
        migrated_from,
        created: false,
    })
}

/// Parses config text (import, tests). Applies migrations; writes nothing.
pub fn parse(text: &str) -> Result<Config, ModelError> {
    let json_err = |source| ModelError::Json {
        path: "<text>".into(),
        source,
    };
    let value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(json_err)?;
    let (value, _) = migrate::upgrade(value)?;
    serde_json::from_value(value).map_err(json_err)
}

pub fn save(path: &Path, config: &Config) -> Result<(), ModelError> {
    write_json(path, config)
}

/// Serializes `value` as pretty JSON and replaces `path` atomically: the data is written
/// to a temporary file in the same directory, flushed to disk, then renamed over the
/// target, so a crash never leaves a half-written file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ModelError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|source| ModelError::Json {
        path: path.into(),
        source,
    })?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ModelError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(strip_bom(&bytes))
            .map(Some)
            .map_err(|source| ModelError::Json {
                path: path.into(),
                source,
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ModelError::io(path, e)),
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ModelError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| ModelError::io(dir, e))?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // std::fs::rename replaces an existing target on Windows (MoveFileExW REPLACE_EXISTING).
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(ModelError::io(path, e));
    }
    Ok(())
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Group;

    fn temp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dg-model-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir();
        let loaded = load(&dir.join("config.json")).unwrap();
        assert!(loaded.created);
        assert_eq!(loaded.config, Config::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_files() {
        let dir = temp_dir();
        let path = dir.join("nested").join("config.json");
        let mut cfg = Config::default();
        cfg.groups.push(Group::new("Gaming on Couch"));
        save(&path, &cfg).unwrap();
        save(&path, &cfg).unwrap(); // replacing an existing file works too
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.config, cfg);
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["config.json".to_string()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bom_is_accepted() {
        let cfg = parse("\u{feff}{\"schemaVersion\":1}").unwrap();
        assert_eq!(cfg.schema_version, 1);
    }
}

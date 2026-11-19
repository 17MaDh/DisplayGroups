//! Schema migrations, applied to raw JSON before it is deserialized.
//!
//! To add schema version N+1: bump `SCHEMA_VERSION`, then append `(N, step)` to
//! `MIGRATIONS` where `step` rewrites a version-N document into version N+1.

use serde_json::Value;

use crate::config::SCHEMA_VERSION;
use crate::ModelError;

type Step = fn(&mut Value);

/// `(from_version, step)` pairs, applied in order.
const MIGRATIONS: &[(u32, Step)] = &[];

/// Brings `value` up to [`SCHEMA_VERSION`]. Returns the version it was migrated from,
/// or `None` if it was already current.
pub fn upgrade(mut value: Value) -> Result<(Value, Option<u32>), ModelError> {
    let found = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or(ModelError::MissingSchemaVersion)?;
    if found > u64::from(SCHEMA_VERSION) {
        return Err(ModelError::NewerSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found == u64::from(SCHEMA_VERSION) {
        return Ok((value, None));
    }

    let from = u32::try_from(found).map_err(|_| ModelError::UnsupportedSchema(found))?;
    let mut version = from;
    for (step_from, step) in MIGRATIONS {
        if *step_from == version {
            step(&mut value);
            version += 1;
        }
    }
    if version != SCHEMA_VERSION {
        return Err(ModelError::UnsupportedSchema(found));
    }
    value["schemaVersion"] = Value::from(SCHEMA_VERSION);
    Ok((value, Some(from)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_version_passes_through() {
        let (v, from) = upgrade(json!({ "schemaVersion": 1, "x": 1 })).unwrap();
        assert_eq!(from, None);
        assert_eq!(v["x"], 1);
    }

    #[test]
    fn newer_version_is_refused() {
        let e = upgrade(json!({ "schemaVersion": 99 })).unwrap_err();
        assert!(matches!(e, ModelError::NewerSchema { found: 99, .. }));
    }

    #[test]
    fn missing_or_unknown_old_version_is_refused() {
        assert!(matches!(
            upgrade(json!({})).unwrap_err(),
            ModelError::MissingSchemaVersion
        ));
        assert!(matches!(
            upgrade(json!({ "schemaVersion": 0 })).unwrap_err(),
            ModelError::UnsupportedSchema(0)
        ));
    }
}

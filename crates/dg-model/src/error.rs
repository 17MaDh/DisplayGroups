use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("cannot read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid DisplayGroups config: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("the config has no schemaVersion")]
    MissingSchemaVersion,

    #[error("config schemaVersion {found} is newer than this build supports ({supported}); update DisplayGroups")]
    NewerSchema { found: u64, supported: u32 },

    #[error("config schemaVersion {0} is not supported")]
    UnsupportedSchema(u64),

    #[error("no group named or with id \"{0}\"")]
    GroupNotFound(String),

    #[error("environment variable {0} is not set")]
    MissingEnv(&'static str),
}

impl ModelError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

//! Rotating diagnostic logs in `%LOCALAPPDATA%\DisplayGroups\logs` (7 daily files).
//! `DISPLAYGROUPS_LOG=debug` overrides the configured level.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

/// Keep the returned guard alive until exit so buffered lines are flushed.
pub fn init(dir: &Path, level: &str) -> Option<WorkerGuard> {
    std::fs::create_dir_all(dir).ok()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("displaygroups")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_env("DISPLAYGROUPS_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init()
        .ok()?;
    Some(guard)
}
